import subprocess, sys
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
for concurrency,batches in ((1,100),(8,13),(50,2),(100,2)):
 command=[sys.executable,str(root/'target/pause-burst-coordinator.py'),'--concurrency',str(concurrency),'--operation','resume','--batches',str(batches),'--name',f'pause-resume-sweep-c{concurrency}','--daemon','/var/tmp/hm-kvm-events/hv2-sandboxd-events','--profile','events']
 result=subprocess.run(command)
 print(f'COHORT c={concurrency} exit={result.returncode}',flush=True)