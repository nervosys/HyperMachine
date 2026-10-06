from pathlib import Path
import subprocess,json
r=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-current-admission16-v1');out.mkdir(exist_ok=False)
for cohort in ['initial','repeat']:
 command=['python3',str(r/'tools/bench-cold-start-limit.py'),'--baseline','/var/tmp/hm-current-competitive-release-node-v1','--candidate','/var/tmp/hm-current-competitive-release-node-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-competitive/guest-output-drain.cpio.gz','--output',str(out/(cohort+'.json')),'--pairs','4','--concurrency','100','--baseline-limit','0','--candidate-limit','16','--memory-idle-seconds','5']
 print('Starting '+cohort,flush=True)
 with (out/(cohort+'.stdout')).open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 (out/(cohort+'-exit.json')).write_text(json.dumps({'exit_code':result.returncode,'argv':command},indent=2)+'\n')
 print('Terminal '+cohort+' exit '+str(result.returncode),flush=True)
