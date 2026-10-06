#!/usr/bin/env python3
"""Compare prepared snapshot creation with identical guest state and workload."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import uuid

spec=importlib.util.spec_from_file_location('engines',Path(__file__).with_name('bench-local-engines.py'))
engines=importlib.util.module_from_spec(spec);spec.loader.exec_module(engines)
fc=engines.fc


def require(condition,message):
    if not condition:raise ValueError(message)


def fapi(path,method,route,value=None):
    connection=fc.UnixHTTP(path);connection.timeout=30
    try:
        connection.request(method,route,None if value is None else json.dumps(value),{'Content-Type':'application/json'})
        response=connection.getresponse();body=response.read(1024*1024+1)
        require(len(body)<=1024*1024 and 200<=response.status<300,f'Firecracker {route} failed: {response.status}: {body[:1000]!r}')
        return json.loads(body) if body else None
    finally:connection.close()


def prepare_command(seed):
    return f"HM_PREPARED_MARKER={seed} /bin/sh -c 'while :; do sleep 1; done' >/dev/null 2>&1 & printf '%s' \"$!\" >/tmp/hm-warm-pid; printf '%s' {seed} >/tmp/hm-warm-marker; printf prepared"


def verify_command(seed,marker):
    return f"test \"$(cat /tmp/hm-warm-marker)\" = {seed} && p=$(cat /tmp/hm-warm-pid) && kill -0 \"$p\" && tr '\\000' '\\n' </proc/\"$p\"/environ | grep -Fx 'HM_PREPARED_MARKER={seed}' >/dev/null && test ! -e /tmp/hm-warm-child && printf '%s' {marker} >/tmp/hm-warm-child && printf '%s' {marker}"


def valid_exec(value,expected,firecracker=False):
    require(value.get('exit_code')==0 and value.get('stdout')==expected and not value.get('timed_out') and not value.get('truncated'),'guest state/output/status mismatch')
    if firecracker:require(value.get('kind')=='exited','guest response kind differs')


def guest_exec(vsock,process,command,restored_notice=None):
    def readiness():
        entropy=os.urandom(64);now=time.time_ns()
        restored_notice.update(entropy_bytes=64,entropy_sha256=hashlib.sha256(entropy).hexdigest(),unix_time_ns=now,attempts=restored_notice.get('attempts',0)+1)
        return ({'kind':'restored','unix_time_ns':now,'entropy':list(entropy)},'acknowledged')
    with fc.guest(vsock,time.perf_counter()+15,process,readiness=readiness if restored_notice is not None else None) as stream:
        if restored_notice is not None:
            restored_notice['acknowledged']=True
            restored_notice['ready_monotonic']=time.perf_counter()
        value=fc.rpc(stream,2,{'kind':'exec','program':'/bin/sh','args':['-c',command],'timeout_ms':10000})
    return value


def file_catalog(root):
    return {str(p.relative_to(root)):{'bytes':p.stat().st_size,'sha256':engines.digest(p)} for p in sorted(root.rglob('*')) if p.is_file()}


def batch(args,engine,pair,url,node,seed,snapshot,directory):
    barrier=threading.Barrier(args.concurrency)
    handles={};lock=threading.Lock();report={'engine':engine,'pair':pair,'samples':[],'success':False,'cleanup_errors':[]}
    baseline=engines.memory(node.pid) if engine=='hypermachine' else None
    if args.mapping_diagnostics and engine=='hypermachine':
        report['empty_mapping_observations']=[args.mapping_diagnostics.observe(node.pid)]
    def attempt(index):
        row={'index':index,'success':False,'cleanup_success':False};sandbox=None;process=None
        marker='hm-prepared-'+uuid.uuid4().hex
        child=directory/f'child-{pair}-{index}';child.mkdir()
        barrier.wait();started=time.perf_counter();row['started_monotonic']=started
        try:
            if engine=='hypermachine':
                created=engines.request(url,'POST','/v2/sandboxes',{'templateID':'warm-benchmark','timeout':300,'allowInternetAccess':False})
                created_at=time.perf_counter()
                sandbox=created.get('sandboxID');require(isinstance(sandbox,str) and sandbox,'create returned no known sandbox ID')
                with lock:handles[index]=(sandbox,None,None)
                row['restored_notice']={'acknowledged_via_create':True,'entropy_bytes':64}
                result=engines.request(url,'POST',f'/sandboxes/{sandbox}/exec',{'cmd':verify_command(seed,marker),'timeout_secs':10})
                valid_exec(result,marker)
                finished=time.perf_counter();row['ready_ms']=(finished-started)*1000
                row['latency_phases_ms']={'create_and_notice':(created_at-started)*1000,'exec':(finished-created_at)*1000}
                config=engines.request(url,'GET',f'/sandboxes/{sandbox}')
                require(config['cpuCount']==1 and config['memoryMB']==1024,'guest resources differ')
            else:
                api=child/'api.sock';vsock=child/'vsock.sock';log=(child/'console.log').open('wb')
                process=subprocess.Popen([str(args.firecracker),'--api-sock',str(api)],stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
                with lock:
                    handles[index]=(None,process,log)
                    args.owned_firecracker.append(process)
                fc.wait_api(api,started+30,process)
                fapi(api,'PUT','/snapshot/load',{'snapshot_path':str(snapshot/'vm.state'),'mem_backend':{'backend_type':'File','backend_path':str(snapshot/'memory.raw')},'resume_vm':True,'vsock_override':{'uds_path':str(vsock)}})
                loaded_at=time.perf_counter()
                row['restored_notice']={}
                result=guest_exec(vsock,process,verify_command(seed,marker),row['restored_notice']);valid_exec(result,marker,True)
                finished=time.perf_counter();row['ready_ms']=(finished-started)*1000
                notice_at=row['restored_notice']['ready_monotonic']
                row['latency_phases_ms']={'process_and_load':(loaded_at-started)*1000,'connect_and_notice':(notice_at-loaded_at)*1000,'exec':(finished-notice_at)*1000}
                config=fapi(api,'GET','/machine-config');require(config['vcpu_count']==1 and config['mem_size_mib']==1024,'guest resources differ')
            row.update(success=True,prepared_file=True,prepared_process_environment=True,independent_child_write=True,clock_rng_resynchronised=True)
        except Exception as error:row['error']=str(error)
        return row
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        report['samples']=list(pool.map(attempt,range(args.concurrency)))
    try:
        time.sleep(5);started=time.perf_counter()
        readings=[engines.memory(node.pid)] if engine=='hypermachine' else [engines.memory(p.pid) for _,p,_ in handles.values() if p.poll() is None]
        require(readings,'no owned process available for held memory')
        report['held_process_memory_kib']={key:sum(item[key] for item in readings) for key in readings[0]}
        report['empty_process_memory_baseline_kib']=baseline if baseline is not None else {key:0 for key in readings[0]}
        report['incremental_process_memory_kib']={key:value-report['empty_process_memory_baseline_kib'][key] for key,value in report['held_process_memory_kib'].items()}
        report['memory_hold_seconds']=5;report['memory_read_ms']=(time.perf_counter()-started)*1000
        if args.mapping_diagnostics:
            pids=[node.pid] if engine=='hypermachine' else [p.pid for _,p,_ in handles.values() if p.poll() is None]
            report['held_mapping_observations']=[args.mapping_diagnostics.observe(pid) for pid in pids]
    except Exception as error:report['memory_error']=str(error)
    for row in report['samples']:
        handle=handles.get(row['index'])
        if handle is None:row['cleanup_success']=True;continue
        sandbox,process,log=handle
        try:
            if sandbox:engines.request(url,'DELETE',f'/sandboxes/{sandbox}')
            else:require(engines.stop(process),'Firecracker process retained')
            row['cleanup_success']=True
        except Exception as error:row['cleanup_error']=str(error)
        finally:
            if log:log.close()
    if engine=='hypermachine':
        try:
            remaining=engines.request(url,'GET','/sandboxes');report['remaining_sandboxes']=len(remaining)
            for item in remaining:engines.request(url,'DELETE',f"/sandboxes/{item['sandboxID']}")
            require(not remaining,'batch retained guests before fallback cleanup')
        except Exception as error:report['cleanup_errors'].append(str(error))
    report['success']=not report['cleanup_errors'] and 'memory_error' not in report and all(r['success'] and r['cleanup_success'] for r in report['samples'])
    starts=[r['started_monotonic'] for r in report['samples']];report['start_spread_ms']=(max(starts)-min(starts))*1000
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['hypermachine','firecracker','kernel','initrd','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--pairs',type=int,default=10);parser.add_argument('--concurrency',type=int,default=8)
    parser.add_argument('--mapping-diagnostics',action='store_true',help='Read owned-process smaps outside latency timing; diagnostic-only cohort')
    args=parser.parse_args();args.owned_firecracker=[];require(1<=args.pairs<=100 and 1<=args.concurrency<=100,'invalid experiment limits')
    if args.mapping_diagnostics:
        diagnostic_spec=importlib.util.spec_from_file_location('prepared_mappings',Path(__file__).with_name('prepared-memory-mappings.py'))
        args.mapping_diagnostics=importlib.util.module_from_spec(diagnostic_spec);diagnostic_spec.loader.exec_module(args.mapping_diagnostics)
    require(not args.output.exists(),'output exists; preserve earlier attempts')
    paths={name:getattr(args,name).resolve(strict=True) for name in ['hypermachine','firecracker','kernel','initrd']}
    for name,path in paths.items():setattr(args,name,path)
    paths.update(coordinator=Path(__file__).resolve(),engines=Path(engines.__file__).resolve(),firecracker_harness=Path(fc.__file__).resolve())
    if args.mapping_diagnostics:paths['mapping_diagnostics']=Path(args.mapping_diagnostics.__file__).resolve()
    hashes={name:engines.digest(path) for name,path in paths.items()}
    affinity=sorted(os.sched_getaffinity(0))[:8];os.sched_setaffinity(0,affinity);os.umask(0o077)
    report={'success':False,'purpose':'prepared snapshot startup, no managed endpoint or universal performance claim','artifact_sha256':hashes,'driver_cpu_affinity':affinity,'cpu_count':1,'memory_mb':1024,'concurrency':args.concurrency,'pairs':args.pairs,'guest_readiness_timeout_s':15,'preparation':{},'runs':[],'cleanup_errors':[],
        'limitations':['Shared WSL nested KVM and uncontrolled host background load','Persistent HyperMachine HTTP daemon versus fresh Firecracker process/Unix API','Prepared resident/cache-warm sources; no dropped-cache or storage durability comparison','PSS excludes kernel memory and unmapped page cache; not fleet density','Failed attempts retained; conditional latency and memory summaries','Engine-generated device kernel arguments differ']}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    report['diagnostic_only']=bool(args.mapping_diagnostics)
    report['guest_restore_contract']={'clock_rng_resynchronised':True,'entropy_bytes':64,'hypermachine':'acknowledged by successful create, source-bound after_restore','firecracker':'Restored RPC acknowledgement before exec; replaces readiness ping'}
    report['latency_phase_measurement']='client_monotonic_no_added_rpc'
    def save():args.output.write_text(json.dumps(report,indent=2)+'\n')
    with tempfile.TemporaryDirectory(prefix='hm-prepared-',dir='/var/tmp') as scratch:
        scratch=Path(scratch);node=None;parent=None;log=None;seed=uuid.uuid4().hex;url=None
        try:
            hm=scratch/'hm';hm.mkdir();api_port=engines.free_port();proxy=engines.free_port();url=f'http://127.0.0.1:{api_port}'
            log=(hm/'console.log').open('wb');node=subprocess.Popen([str(args.hypermachine),'--port',str(api_port),'--proxy-port',str(proxy),'--memory-mb','1024','--cpu-cores','1','--capacity','128','--volume-dir',str(hm/'volumes'),'--snapshot-store',str(hm/'snapshots')],env={'PATH':'/usr/local/bin:/usr/bin:/bin','HV2_KERNEL':str(args.kernel),'HV2_INITRD':str(args.initrd),'RUST_LOG':'warn'},stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
            deadline=time.perf_counter()+30
            while True:
                require(node.poll() is None,'node exited during startup')
                try:templates=engines.request(url,'GET','/templates');break
                except OSError:
                    require(time.perf_counter()<deadline,'node startup timed out');time.sleep(.01)
            base=next((t for t in templates if 'base' in t.get('aliases',[])),None)
            require(base is not None and base['snapshot'] is True and base['cpuCount']==1 and base['memoryMB']==1024,'base template not prepared at matching resources')
            report['preparation_phase']='hypermachine-create-parent';save()
            started=time.perf_counter();created=engines.request(url,'POST','/v2/sandboxes',{'templateID':'base','timeout':600,'allowInternetAccess':False});id=created['sandboxID']
            report['preparation_phase']='hypermachine-populate-parent';save()
            result=engines.request(url,'POST',f'/sandboxes/{id}/exec',{'cmd':prepare_command(seed),'timeout_secs':10});valid_exec(result,'prepared')
            report['preparation_phase']='hypermachine-capture-named-snapshot';save()
            engines.request(url,'POST',f'/sandboxes/{id}/snapshots',{'name':'warm-benchmark'})
            report['preparation_phase']='hypermachine-delete-parent';save()
            engines.request(url,'DELETE',f'/sandboxes/{id}')
            report['preparation_phase']='hypermachine-verify-named-snapshot';save()
            offerings=engines.request(url,'GET','/snapshots?name=warm-benchmark');offering=next((t for t in offerings if t.get('snapshotID')=='warm-benchmark:default'),None)
            require(offering is not None and offering.get('sandboxID')==id,'named snapshot not listed for prepared parent')
            report['preparation']['hypermachine']={'duration_ms':(time.perf_counter()-started)*1000,'offering':offering,'base_template':base,'source_files':file_catalog(hm/'snapshots')}
            report['preparation_phase']='firecracker-prepare-parent';save()
            snapshot=scratch/'fc';snapshot.mkdir();api=snapshot/'api.sock';vsock=snapshot/'vsock.sock';plog=(snapshot/'console.log').open('wb');started=time.perf_counter()
            parent=subprocess.Popen([str(args.firecracker),'--api-sock',str(api)],stdin=subprocess.DEVNULL,stdout=plog,stderr=subprocess.STDOUT)
            args.owned_firecracker.append(parent)
            fc.wait_api(api,started+30,parent);fapi(api,'PUT','/machine-config',{'vcpu_count':1,'mem_size_mib':1024});fapi(api,'PUT','/boot-source',{'kernel_image_path':str(args.kernel),'initrd_path':str(args.initrd),'boot_args':fc.BOOT_ARGS});fapi(api,'PUT','/vsock',{'guest_cid':3,'uds_path':str(vsock)});fapi(api,'PUT','/actions',{'action_type':'InstanceStart'})
            valid_exec(guest_exec(vsock,parent,prepare_command(seed)),'prepared',True)
            report['preparation_phase']='firecracker-capture-snapshot';save()
            fapi(api,'PATCH','/vm',{'state':'Paused'});fapi(api,'PUT','/snapshot/create',{'snapshot_type':'Full','snapshot_path':str(snapshot/'vm.state'),'mem_file_path':str(snapshot/'memory.raw')});require(engines.stop(parent),'parent Firecracker did not stop');plog.close()
            report['preparation']['firecracker']={'duration_ms':(time.perf_counter()-started)*1000,'state_sha256':engines.digest(snapshot/'vm.state'),'memory_sha256':engines.digest(snapshot/'memory.raw'),'memory_bytes':(snapshot/'memory.raw').stat().st_size,'snapshot_type':'Full','source_parent_stopped':True}
            report['preparation_phase']='complete';save()
            for pair in range(args.pairs):
                for engine in (('hypermachine','firecracker') if pair%2==0 else ('firecracker','hypermachine')):
                    directory=scratch/f'{engine}-{pair}';directory.mkdir();row=batch(args,engine,pair,url,node,seed,snapshot,directory);report['runs'].append(row);save();print(json.dumps({'pair':pair,'engine':engine,'success':row['success']}),flush=True)
            report['preparation']['hypermachine']['source_unchanged']=file_catalog(hm/'snapshots')==report['preparation']['hypermachine']['source_files']
            report['preparation']['firecracker']['source_unchanged']=engines.digest(snapshot/'vm.state')==report['preparation']['firecracker']['state_sha256'] and engines.digest(snapshot/'memory.raw')==report['preparation']['firecracker']['memory_sha256']
        except Exception as error:report['setup_error']=str(error)
        finally:
            report['owned_firecracker_processes']=[]
            for owned in args.owned_firecracker:
                try:require(engines.stop(owned),'owned Firecracker process did not stop')
                except Exception as error:report['cleanup_errors'].append(str(error))
                report['owned_firecracker_processes'].append({'pid':owned.pid,'exit_code':owned.returncode})
            if parent is not None:engines.stop(parent)
            if node is not None:
                try:
                    remaining=engines.request(url,'GET','/sandboxes');report['remaining_sandboxes_before_cleanup']=len(remaining)
                    for item in remaining:engines.request(url,'DELETE',f"/sandboxes/{item['sandboxID']}")
                    report['remaining_sandboxes']=len(engines.request(url,'GET','/sandboxes'))
                    require(report['remaining_sandboxes']==0,'node retained guests after final cleanup')
                except Exception as error:report['cleanup_errors'].append(str(error))
                report['owned_node_stopped']=engines.stop(node);report['owned_node_exit_code']=node.returncode
            if log:log.close();report['node_log_tail']=(scratch/'hm/console.log').read_bytes()[-8000:].decode(errors='replace')
            report['artifacts_unchanged']=all(engines.digest(path)==hashes[name] for name,path in paths.items())
            report['success']='setup_error' not in report and not report['cleanup_errors'] and report.get('owned_node_stopped') is True and report['artifacts_unchanged'] and all(p.get('source_unchanged') for p in report['preparation'].values()) and len(report['runs'])==args.pairs*2 and all(r['success'] for r in report['runs'])
            save()
    return 0 if report['success'] else 1


if __name__=='__main__':raise SystemExit(main())
