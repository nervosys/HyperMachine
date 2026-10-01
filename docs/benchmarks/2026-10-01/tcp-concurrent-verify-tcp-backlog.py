import hashlib,json,os,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-backlog')
os.sched_setaffinity(0,sorted(os.sched_getaffinity(0))[:8])
r={'success':False,'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'results':[]}
commands=[]
for index,mode in enumerate(['backlog4','backlog128','backlog128','backlog4'],1):
 image=Path('/var/tmp/hm-tcp/guest-tcp.cpio.gz') if mode=='backlog4' else out/'guest-tcp.cpio.gz'
 name=f'run-{index}-{mode}'
 commands.append((name,['python3','tools/bench-tcp-local.py','--hypermachine','/var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release','--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd',str(image),'--output',str(out/(name+'.json')),'--pairs','10','--rounds','3','--concurrency','8','--environment','shared-nested-KVM matched guest-agent backlog control']))
commands.extend([
 ('e2e',['python3','tools/e2e-tcp-tunnel.py','--daemon','/var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release','--control-plane','/var/tmp/hm-tcp-api-nodelay/final/hv2-control-plane-release','--cli','/var/tmp/hm-tcp/hm','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd',str(out/'guest-tcp.cpio.gz'),'--output',str(out/'e2e.json')]),
 ('guest-tests',['cargo','test','--locked','-p','hv2-guest-agent']),
 ('guest-clippy',['cargo','clippy','--locked','-p','hv2-guest-agent','--all-targets','--','-D','warnings']),
 ('python-scorer',['python3','tools/test-bench-tcp-local.py'])])
try:
 for name,command in commands:
  before=os.getloadavg();result=subprocess.run(command,cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  r['results'].append({'name':name,'command':command,'exit_code':result.returncode,'load_before':before,'load_after':os.getloadavg(),'output_utf8':result.stdout.decode(),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  print(json.dumps({'name':name,'exit_code':result.returncode}),flush=True)
  if name.startswith('run-'):
   assert result.returncode in [0,1]
   report=json.loads((out/(name+'.json')).read_text());assert report['remaining_sandboxes']==0 and report['node_stopped'] and report['artifacts_unchanged']
  else:assert result.returncode==0,result.stdout.decode()[-3000:]
 r['success']=True;r['all_benchmarks_passed']=all(x['exit_code']==0 for x in r['results'] if x['name'].startswith('run-'))
finally:
 (out/'verification.json').write_text(json.dumps(r,indent=2)+'\n')
