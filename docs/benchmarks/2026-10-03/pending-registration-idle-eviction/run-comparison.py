import subprocess
for path in ['/var/tmp/hm-pending-eviction-baseline-run-v3.py','/var/tmp/hm-pending-eviction-kvm-run-v3.py']:
 result=subprocess.run(['python3',path]);print('Driver terminal',path,result.returncode,flush=True)
 if 'baseline' in path and result.returncode==0:raise RuntimeError('baseline unexpectedly passed')
 if 'baseline' not in path and result.returncode!=0:raise RuntimeError('fixed fixture failed')
