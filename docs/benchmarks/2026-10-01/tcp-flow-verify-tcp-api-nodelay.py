import hashlib,json,os,subprocess,time
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-api-nodelay/final')
os.sched_setaffinity(0,sorted(os.sched_getaffinity(0))[:8])
r={'success':False,'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'results':[]}
common=['--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp/guest-tcp.cpio.gz','--pairs','20','--rounds','5','--environment','shared-WSL-nested-KVM-eight-host-CPU-affinity-API-socket-comparison']
commands=[]
for index,mode in enumerate(['baseline','candidate','candidate','baseline'],1):
 daemon=Path('/var/tmp/hm-tcp-bench/hv2-sandboxd-release') if mode=='baseline' else out/'hv2-sandboxd-release'
 commands.append((f'run-{index}-{mode}',['python3','tools/bench-tcp-local.py','--hypermachine',str(daemon),'--output',str(out/f'run-{index}-{mode}.json')]+common))
commands.extend([
 ('e2e',['python3','tools/e2e-tcp-tunnel.py','--daemon',str(out/'hv2-sandboxd-release'),'--control-plane',str(out/'hv2-control-plane-release'),'--cli','/var/tmp/hm-tcp/hm','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp/guest-tcp.cpio.gz','--output',str(out/'e2e.json')]),
 ('linux-checks',['python3','target/check-tcp-final.py','--output',str(out/'checks-linux.json')]),
 ('python-scorer',['python3','tools/test-bench-tcp-local.py']),
])
try:
 for name,command in commands:
  before=os.getloadavg();started=time.time()
  result=subprocess.run(command,cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  r['results'].append({'name':name,'command':command,'exit_code':result.returncode,'started_at_unix':started,'load_before':before,'load_after':os.getloadavg(),'output_utf8':result.stdout.decode(),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  print(json.dumps({'name':name,'exit_code':result.returncode}),flush=True)
  assert result.returncode==0,result.stdout.decode()[-3000:]
 r['success']=True
finally:
 (out/'verification.json').write_text(json.dumps(r,indent=2)+'\n')
