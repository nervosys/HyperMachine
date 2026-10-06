from pathlib import Path
import subprocess,json
r=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-current-cold-failure-symbols-v1');out.mkdir(exist_ok=False)
for cohort in ['initial','repeat']:
 command=['python3',str(r/'tools/diagnose-readiness-failures.py'),'--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-competitive/guest-output-drain.cpio.gz','--failure-report',str(r/'docs/benchmarks/2026-10-03/current-admission-sixteen'/(cohort+'.json')),'--output',str(out/(cohort+'.json'))]
 print('Starting '+cohort,flush=True)
 with (out/(cohort+'.stdout')).open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 (out/(cohort+'-exit.json')).write_text(json.dumps({'exit_code':result.returncode,'argv':command},indent=2)+'\n')
 print('Terminal '+cohort+' exit '+str(result.returncode),flush=True)
