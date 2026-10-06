import hashlib,json,os,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-unix')
os.sched_setaffinity(0,sorted(os.sched_getaffinity(0))[:8])
r={'success':False,'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'results':[]}
commands=[]
for concurrency,pairs,rounds in [(1,20,5),(8,10,3)]:
 for index,mode in enumerate(['baseline','candidate','candidate','baseline'],1):
  daemon=Path('/var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release') if mode=='baseline' else out/'hv2-sandboxd-release'
  name=f'c{concurrency}-run-{index}-{mode}'
  commands.append((name,['python3','tools/bench-tcp-local.py','--hypermachine',str(daemon),'--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz','--output',str(out/(name+'.json')),'--pairs',str(pairs),'--rounds',str(rounds),'--concurrency',str(concurrency),'--environment','shared-WSL-nested-KVM Unix relay control']))
commands.extend([
 ('e2e',['python3','tools/e2e-tcp-tunnel.py','--daemon',str(out/'hv2-sandboxd-release'),'--control-plane','/var/tmp/hm-tcp-api-nodelay/final/hv2-control-plane-release','--cli','/var/tmp/hm-tcp/hm','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz','--output',str(out/'e2e.json')]),
 ('linux-checks',['python3','target/check-tcp-final.py','--output',str(out/'checks-linux.json')]),
 ('scorer',['python3','tools/test-bench-tcp-local.py'])])
try:
 for name,command in commands:
  if name.startswith('c'):assert not (out/(name+'.json')).exists(),name
  before=os.getloadavg();result=subprocess.run(command,cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  r['results'].append({'name':name,'command':command,'exit_code':result.returncode,'load_before':before,'load_after':os.getloadavg(),'output_utf8':result.stdout.decode(),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  print(json.dumps({'name':name,'exit_code':result.returncode}),flush=True)
  assert result.returncode==0,result.stdout.decode()[-3000:]
 r['success']=True
finally:
 (out/'verification.json').write_text(json.dumps(r,indent=2)+'\n')
