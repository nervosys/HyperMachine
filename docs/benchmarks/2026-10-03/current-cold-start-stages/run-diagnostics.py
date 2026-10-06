from pathlib import Path
import os,subprocess,json
r=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-current-cold-stages-v1');os.sched_setaffinity(0,set(range(8)))
for c,pairs in [(8,2),(100,1)]:
 command=['python3',str(r/'tools/diagnose-concurrent-startup.py'),'--hypermachine',str(out/'hv2-sandboxd'),'--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-competitive/guest-output-drain.cpio.gz','--environment','unscored owned WSL startup-stage diagnostic; same optimized executable and eight allowed CPUs','--pairs',str(pairs),'--concurrency',str(c),'--memory-idle-seconds','5','--collect-cold-readiness','--collect-dispatch']
 print('Starting diagnostic C'+str(c),flush=True)
 with (out/('c'+str(c)+'.json')).open('x') as log, (out/('c'+str(c)+'.stderr')).open('x') as err:result=subprocess.run(command,stdout=log,stderr=err)
 (out/('c'+str(c)+'-exit.json')).write_text(json.dumps({'exit_code':result.returncode,'argv':command},indent=2)+'\n')
 print('Terminal diagnostic C'+str(c)+' exit '+str(result.returncode),flush=True)
