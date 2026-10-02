import hashlib,json,math,subprocess,sys,time
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-kvm-events')
coordinator=root/'target/kvm-events-stateful-coordinator.py'
manifest={'matrix_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'coordinator_sha256':hashlib.sha256(coordinator.read_bytes()).hexdigest(),'profiles':{'baseline':'/var/tmp/hm-boot-sizing/hv2-sandboxd-baseline','events':'/var/tmp/hm-kvm-events/hv2-sandboxd-events'},'cohorts':[]}
for position,c in enumerate((1,8,50,100)):
 for operation_position,operation in enumerate(('resume','fork') if position%2==0 else ('fork','resume')):
  profiles=('baseline','events') if (position+operation_position)%2==0 else ('events','baseline')
  for profile in profiles:
   name=f'events-{profile}-{operation}-c{c}'
   started=time.monotonic()
   result=subprocess.run([sys.executable,str(coordinator),'--concurrency',str(c),'--operation',operation,'--batches',str(max(2,math.ceil(100/c))),'--name',name,'--profile',profile,'--daemon',manifest['profiles'][profile]],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
   report=out/(name+'.json')
   row={'file':report.name,'operation':operation,'concurrency':c,'profile':profile,'exit_code':result.returncode,'duration_seconds':time.monotonic()-started,'report_sha256':hashlib.sha256(report.read_bytes()).hexdigest() if report.exists() else None,'stdout':result.stdout,'stderr':result.stderr}
   manifest['cohorts'].append(row)
   (out/'matrix.json').write_text(json.dumps(manifest,indent=2))
   print(json.dumps({k:row[k] for k in ('file','exit_code','duration_seconds')}),flush=True)