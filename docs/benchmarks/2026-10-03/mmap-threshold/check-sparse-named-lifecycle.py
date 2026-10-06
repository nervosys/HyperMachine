#!/usr/bin/env python3
"""Compare real guest survival after named-source deletion and child pause/resume."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

spec=importlib.util.spec_from_file_location('named_lifecycle_warm',Path(__file__).with_name('bench-prepared-engines.py'))
warm=importlib.util.module_from_spec(spec);spec.loader.exec_module(warm)
engines=warm.engines

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('baseline','candidate','kernel','initrd','output'):
        parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args()
    if args.output.exists(): raise ValueError('preserve earlier evidence')
    paths={n:getattr(args,n).resolve(strict=True) for n in ('baseline','candidate','kernel','initrd')}
    report={'purpose':'owned named-source deletion and child lifecycle regression check',
            'artifact_sha256':{n:engines.digest(p) for n,p in paths.items()},
            'runtime_change_adopted':False,'performance_win_established':False,'runs':[]}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    for variant in ('baseline','candidate'):
        row={'variant':variant,'checks':[],'cleanup_errors':[]};node=None;log=None
        with tempfile.TemporaryDirectory(prefix='hm-named-lifecycle-',dir='/var/tmp') as scratch:
            root=Path(scratch);url='http://127.0.0.1:'+str(engines.free_port())
            def request(method,path,value=None): return engines.request(url,method,path,value)
            try:
                log=(root/'console.log').open('wb')
                node=subprocess.Popen([str(paths[variant]),'--port',url.rsplit(':',1)[1],
                    '--proxy-port',str(engines.free_port()),'--memory-mb','1024','--cpu-cores','1','--capacity','4',
                    '--snapshot-store',str(root/'store'),'--volume-dir',str(root/'volumes')],
                    env={'PATH':'/usr/local/bin:/usr/bin:/bin','HV2_KERNEL':str(paths['kernel']),
                         'HV2_INITRD':str(paths['initrd']),'RUST_LOG':'warn'},
                    stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
                deadline=time.monotonic()+30
                while True:
                    if node.poll() is not None: raise ValueError('owned node exited during startup')
                    try: request('GET','/templates');break
                    except OSError:
                        if time.monotonic()>=deadline: raise
                        time.sleep(.02)
                seed=uuid.uuid4().hex;marker='hm-lifecycle-'+uuid.uuid4().hex
                parent=request('POST','/v2/sandboxes',{'templateID':'base','timeout':300,'allowInternetAccess':False})['sandboxID']
                warm.valid_exec(request('POST',f'/sandboxes/{parent}/exec',{'cmd':warm.prepare_command(seed),'timeout_secs':10}),'prepared')
                request('POST',f'/sandboxes/{parent}/snapshots',{'name':'lifecycle-source'})
                request('DELETE',f'/sandboxes/{parent}')
                child=request('POST','/v2/sandboxes',{'templateID':'lifecycle-source','timeout':300,'allowInternetAccess':False})['sandboxID']
                warm.valid_exec(request('POST',f'/sandboxes/{child}/exec',{'cmd':warm.verify_command(seed,marker),'timeout_secs':10}),marker)
                row['checks'].append('prepared-child-state')
                request('DELETE','/templates/lifecycle-source')
                command=f'test "$(cat /tmp/hm-warm-marker)" = {seed} && test "$(cat /tmp/hm-warm-child)" = {marker} && p=$(cat /tmp/hm-warm-pid) && kill -0 "$p" && printf %s {marker}'
                warm.valid_exec(request('POST',f'/sandboxes/{child}/exec',{'cmd':command,'timeout_secs':10}),marker)
                row['checks'].append('child-runs-after-source-deletion')
                request('POST',f'/sandboxes/{child}/pause',{})
                row['checks'].append('child-paused-after-source-deletion')
                request('POST',f'/sandboxes/{child}/resume',{})
                warm.valid_exec(request('POST',f'/sandboxes/{child}/exec',{'cmd':command,'timeout_secs':10}),marker)
                row['checks'].append('child-resumes-state-after-source-deletion')
                row['success']=True
            except Exception as error:
                row.update(success=False,error=str(error))
            finally:
                if node is not None:
                    try:
                        for item in request('GET','/sandboxes'):
                            request('DELETE',f"/sandboxes/{item['sandboxID']}")
                        row['remaining_sandboxes']=len(request('GET','/sandboxes'))
                    except Exception as error: row['cleanup_errors'].append(str(error))
                    row['owned_node_stopped']=engines.stop(node);row['owned_node_exit_code']=node.returncode
                if log:
                    log.close();row['node_log_tail']=(root/'console.log').read_bytes()[-8000:].decode(errors='replace')
        report['runs'].append(row)
        args.output.write_text(json.dumps(report,indent=2)+'\n')
    report['artifacts_unchanged']=all(engines.digest(path)==report['artifact_sha256'][name] for name,path in paths.items())
    report['cleanup_verified']=all(not r['cleanup_errors'] and r.get('remaining_sandboxes')==0 and r.get('owned_node_stopped') is True and r.get('owned_node_exit_code')==0 for r in report['runs'])
    report['all_variants_passed']=all(r['success'] for r in report['runs'])
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('all_variants_passed','cleanup_verified','artifacts_unchanged')}))
    return 0 if report['cleanup_verified'] and report['artifacts_unchanged'] else 1

if __name__=='__main__': raise SystemExit(main())
