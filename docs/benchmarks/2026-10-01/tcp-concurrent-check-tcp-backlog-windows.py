import hashlib,json,subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[1]
r={'success':False,'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'results':[]}
try:
 for command in [['cargo','clippy','--locked','-p','hv2-guest-agent','--all-targets','--','-D','warnings'],['python','tools/test-bench-tcp-local.py']]:
  result=subprocess.run(command,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  r['results'].append({'command':command,'exit_code':result.returncode,'output_utf8':result.stdout.decode(),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  assert result.returncode==0,result.stdout.decode()[-3000:]
 r['success']=True
finally:
 (root/'target/tcp-backlog-windows.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'success':r['success'],'exit_codes':[x['exit_code'] for x in r['results']]}),flush=True)
