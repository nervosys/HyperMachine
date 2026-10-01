import json,subprocess,sys
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-refusal-retry')
results=[]
for concurrency,rounds in [(1,100),(8,100),(50,20),(100,20)]:
 for block in range(2):
  order=['baseline','candidate'] if block==0 else ['candidate','baseline']
  for profile in order:
   name=f'commands-{profile}-c{concurrency}-block{block}'
   command=[sys.executable,str(root/'target/refusal-command-coordinator.py'),'--daemon',str(out/f'hv2-sandboxd-{profile}'),'--concurrency',str(concurrency),'--rounds',str(rounds),'--name',name,'--profile',profile]
   result=subprocess.run(command)
   results.append({'name':name,'exit_code':result.returncode,'command':command})
   (out/'commands-matrix.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps({'cohorts':len(results),'nonzero_cohorts':sum(r['exit_code']!=0 for r in results)}),flush=True)