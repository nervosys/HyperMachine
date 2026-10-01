import hashlib,json,os,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-fixture-diagnostic')
os.sched_setaffinity(0,sorted(os.sched_getaffinity(0))[:8])
r={'success':False,'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'results':[]}
try:
 for name,image,flag,expected in [('mode-default',out/'guest-tcp.cpio.gz',False,0),('mode-nodelay',out/'guest-tcp.cpio.gz',True,0),('mode-unsupported',Path('/var/tmp/hm-tcp/guest-tcp.cpio.gz'),True,1)]:
  command=['python3','tools/bench-tcp-local.py','--hypermachine','/var/tmp/hm-tcp-bench/hv2-sandboxd-release','--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd',str(image),'--output',str(out/(name+'.json')),'--pairs','2','--rounds','2','--environment','fixture mode acknowledgement verification']
  if flag:command.append('--fixture-nodelay')
  result=subprocess.run(command,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  r['results'].append({'name':name,'command':command,'expected_exit_code':expected,'exit_code':result.returncode,'output_utf8':result.stdout.decode(),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  assert result.returncode==expected,result.stdout.decode()[-2000:]
  report=json.loads((out/(name+'.json')).read_text());assert report['remaining_sandboxes']==0 and report['node_stopped']
  if expected:
   assert not report['success'] and not report['rows']
   assert all(not x['success'] and x['cleanup_success'] and 'mode unsupported' in x['error'] for x in report['preparation'])
  print(json.dumps({'name':name,'expected_exit_code_observed':True}),flush=True)
 r['success']=True
finally:
 (out/'mode-checks.json').write_text(json.dumps(r,indent=2)+'\n')
