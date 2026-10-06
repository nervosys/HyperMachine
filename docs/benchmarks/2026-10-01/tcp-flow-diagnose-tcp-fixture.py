import hashlib,json,os,subprocess,time
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-fixture-diagnostic')
os.sched_setaffinity(0,sorted(os.sched_getaffinity(0))[:8])
report={'success':False,'purpose':'fixture buffering diagnostic; production daemon unchanged','coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'results':[]}
try:
 for index,mode in enumerate(['default','nodelay','nodelay','default'],1):
  command=['python3','tools/bench-tcp-local.py','--hypermachine','/var/tmp/hm-tcp-bench/hv2-sandboxd-release','--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd',str(out/'guest-tcp.cpio.gz'),'--output',str(out/f'run-{index}-{mode}.json'),'--pairs','20','--rounds','5','--environment','shared-WSL-nested-KVM-eight-host-CPU-affinity-fixture-diagnostic']
  if mode=='nodelay':command.append('--fixture-nodelay')
  before=os.getloadavg();started=time.time()
  result=subprocess.run(command,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  report['results'].append({'index':index,'mode':mode,'command':command,'exit_code':result.returncode,'started_at_unix':started,'load_before':before,'load_after':os.getloadavg(),'output_utf8':result.stdout.decode(),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  print(json.dumps({'index':index,'mode':mode,'exit_code':result.returncode}),flush=True)
  assert result.returncode==0,result.stdout.decode()[-3000:]
 report['success']=True
finally:
 (out/'checks.json').write_text(json.dumps(report,indent=2)+'\n')
