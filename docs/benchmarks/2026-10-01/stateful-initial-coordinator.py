import argparse, hashlib, importlib.util, json, os, secrets, signal, subprocess, sys, tempfile, time
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--concurrency',type=int,required=True)
parser.add_argument('--operation',choices=('resume','fork'),required=True)
parser.add_argument('--batches',type=int,default=2)
parser.add_argument('--name',required=True)
args=parser.parse_args()
if not 1<=args.concurrency<=100 or not 1<=args.batches<=3:parser.error('bounded profile requires concurrency 1..100 and batches 1..3')
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-stateful-sweep'); out.mkdir(exist_ok=True)
spec=importlib.util.spec_from_file_location('engines',root/'tools/bench-local-engines.py')
engines=importlib.util.module_from_spec(spec);spec.loader.exec_module(engines)
paths={'daemon':Path('/var/tmp/hm-boot-sizing/hv2-sandboxd-baseline'),'kernel':Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),'initrd':Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz'),'sdk_harness':root/'tools/bench-e2b-sdk.py','request_harness':root/'tools/bench-local-engines.py','coordinator':Path(__file__)}
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
cores=sorted(os.sched_getaffinity(0))[:8];os.sched_setaffinity(0,cores)
report={'artifact_sha256':{k:digest(p) for k,p in paths.items()},'success':False,'error':None,'cleanup_errors':[],'host_affinity':cores,'concurrency':args.concurrency,'operation':args.operation,'batches':args.batches,'memory_info_before':Path('/proc/meminfo').read_text(),'baseline_commit':'5c91befd2bad03bf3cb9a2bc359c9840d5f44bf6','daemon_guest_capacity':2*args.concurrency+4}
worker_code='import time\nend=time.monotonic()+7200\nx=1\nwhile time.monotonic()<end:\n for _ in range(10000): x=(x*1664525+1013904223)&0xffffffff\n'
worker=subprocess.Popen([sys.executable,'-c',worker_code],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,preexec_fn=lambda:os.sched_setaffinity(0,{cores[0]}))
report['controlled_cpu_load']={'workers':1,'cpu':cores[0],'worker_code':worker_code,'all_alive_through_cohort':False,'workers_cleaned_up':False}
process=None;child=None
with tempfile.TemporaryDirectory(prefix='hm-stateful-sweep-',dir='/var/tmp') as directory:
 directory=Path(directory);port,proxy=engines.free_port(),engines.free_port()
 while port==proxy:proxy=engines.free_port()
 url=f'http://127.0.0.1:{port}'
 def request(method,path,body=None):return engines.request(url,method,path,body)
 try:
  with (directory/'node.log').open('wb') as log:
   process=subprocess.Popen([str(paths['daemon']),'--port',str(port),'--proxy-port',str(proxy),'--memory-mb','1024','--cpu-cores','1','--capacity',str(2*args.concurrency+4),'--volume-dir',str(directory/'volumes'),'--snapshot-store',str(directory/'snapshots')],env={'PATH':'/usr/local/bin:/usr/bin:/bin','RUST_LOG':'warn','HV2_KERNEL':str(paths['kernel']),'HV2_INITRD':str(paths['initrd'])},stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
   deadline=time.monotonic()+30
   while True:
    if process.poll() is not None:raise RuntimeError('daemon exited before readiness')
    try:templates=request('GET','/templates');break
    except OSError:
     if time.monotonic()>=deadline:raise
     time.sleep(.01)
   base=next((v for v in templates if 'base' in v.get('aliases',[])),None)
   if not base or base.get('snapshot') is not True:raise RuntimeError('snapshot-backed base template required')
   report['template']=base
   env=dict(os.environ)
   for key in ('E2B_API_URL','E2B_SANDBOX_URL','E2B_ENVD_POOL_SHARDS'):env.pop(key,None)
   env['E2B_API_KEY']=secrets.token_hex(32);env['NO_PROXY']='127.0.0.1,localhost'
   command=['/var/tmp/hm-stateful-sdk-venv/bin/python',str(paths['sdk_harness']),'--provider','HyperMachine','--api-url',url,'--sandbox-url',f'http://127.0.0.1:{proxy}','--template','base','--environment','shared-WSL-nested-KVM-eight-host-CPU-affinity-one-pinned-worker-Linux-SDK-client','--image-description','fixed UART IRQ BusyBox guest with agent and envd; snapshot-backed base; 1 vCPU/1024 MiB; exact images in coordinator artifact hashes','--expected-cpus','1','--expected-memory-mb','1024','--workload','posix','--operation',args.operation,'--samples',str(args.concurrency*args.batches),'--concurrency',str(args.concurrency),'--synchronized-operation-batches']
   child=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,start_new_session=True)
   stdout,stderr=child.communicate(timeout=900)
   report['sdk_exit_code']=child.returncode;report['sdk_stderr']=stderr;report['sdk']=json.loads(stdout)
   report['controlled_cpu_load']['all_alive_through_cohort']=worker.poll() is None
   report['success']=child.returncode==0
 except Exception as error:
  report['error']=str(error)
 finally:
  if child is not None and child.poll() is None:
   os.killpg(child.pid,signal.SIGTERM)
   try:child.communicate(timeout=5)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.communicate(timeout=5)
  if process is not None and process.poll() is None:
   try:
    remaining=request('GET','/sandboxes');report['remaining_sandbox_count']=len(remaining)
    if remaining:
     report['cleanup_errors'].append('SDK left owned sandbox records')
     for sandbox in remaining:
      try:request('DELETE',f"/sandboxes/{sandbox['sandboxID']}")
      except Exception as error:report['cleanup_errors'].append(str(error))
   except Exception as error:report['cleanup_errors'].append(str(error))
  if process is not None and not engines.stop(process):report['cleanup_errors'].append('daemon did not stop')
  if worker.poll() is None:
   worker.terminate()
   try:worker.wait(timeout=5)
   except subprocess.TimeoutExpired:worker.kill();worker.wait(timeout=5)
  report['controlled_cpu_load']['workers_cleaned_up']=worker.poll() is not None
  if (directory/'node.log').exists():report['node_log_tail']=(directory/'node.log').read_text(errors='replace')[-12000:]
report['artifacts_unchanged']=all(digest(p)==report['artifact_sha256'][k] for k,p in paths.items())
report['success']=report['success'] and report['artifacts_unchanged'] and not report['cleanup_errors'] and all(report['controlled_cpu_load'][k] for k in ('all_alive_through_cohort','workers_cleaned_up'))
(out/(args.name+'.json')).write_text(json.dumps(report,indent=2))
print(json.dumps({'success':report['success'],'error':report['error'],'cleanup_errors':report['cleanup_errors'],'sdk_exit_code':report.get('sdk_exit_code'),'ready_ms':report.get('sdk',{}).get('ready_ms'),'failed_samples':report.get('sdk',{}).get('failed_samples'),'failed_rows':[r for r in report.get('sdk',{}).get('samples',[]) if not r['success']]}),flush=True)
raise SystemExit(0 if report['success'] else 1)