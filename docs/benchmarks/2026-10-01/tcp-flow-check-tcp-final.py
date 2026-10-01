import argparse,hashlib,json,os,platform,subprocess
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);a=parser.parse_args()
root=Path(__file__).resolve().parents[1]
commands=[
 ['cargo','test','--locked','-p','hv2-api','tcp_tunnel','--lib'],
 ['cargo','test','--locked','-p','hv2-cluster','--lib','--test','control_plane'],
 ['cargo','test','--locked','-p','hm-cli','--test','sandbox_vm_client'],
 ['cargo','test','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd'],
 ['cargo','clippy','--locked','-p','hv2-api','-p','hv2-cluster','-p','hv2-sandboxd','-p','hm-cli','--all-targets','--','-D','warnings']]
r={'platform':platform.platform(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'results':[],'success':False,'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
try:
 for command in commands:
  result=subprocess.run(command,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  r['results'].append({'command':command,'exit_code':result.returncode,'output_utf8':result.stdout.decode('utf8'),'output_sha256':hashlib.sha256(result.stdout).hexdigest()})
  assert result.returncode==0,result.stdout.decode('utf8')[-6000:]
 r['success']=True
finally:
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(r,indent=2)+'\n')
 print(json.dumps({'platform':r['platform'],'success':r['success'],'exit_codes':[x['exit_code'] for x in r['results']]}),flush=True)
