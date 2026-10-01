import hashlib,json,os,shutil,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-ssh/cli-name');out.mkdir(exist_ok=False)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
names=['Cargo.toml','Cargo.lock','crates/hm-cli/Cargo.toml','crates/hv2-jobs/Cargo.toml']+[p.relative_to(root).as_posix() for d in ['crates/hm-cli/src','crates/hv2-jobs/src'] for p in sorted((root/d).rglob('*.rs'))]
r={'success':False,'coordinator_sha256':sha(__file__),'source_commit':subprocess.check_output(['git','-c','safe.directory='+str(root),'rev-parse','HEAD'],cwd=root,text=True).strip(),'source_sha256':{n:sha(root/n) for n in names},'rustc':subprocess.check_output(['rustc','--version'],text=True).strip()}
try:
 command=['cargo','build','--locked','-p','hm-cli','--bin','hm'];result=subprocess.run(command,cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 r.update(command=command,exit_code=result.returncode,output_utf8=result.stdout.decode(),output_sha256=hashlib.sha256(result.stdout).hexdigest());assert result.returncode==0
 assert all(sha(root/n)==v for n,v in r['source_sha256'].items())
 for name in names:
  path=out/'source'/name;path.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,path)
 shutil.copy2('/var/tmp/hm-competitive-target/debug/hm',out/'hm');r['cli_sha256']=sha(out/'hm');r['success']=True
finally:
 (out/'build.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'success':r['success'],'cli_sha256':r.get('cli_sha256')}))
