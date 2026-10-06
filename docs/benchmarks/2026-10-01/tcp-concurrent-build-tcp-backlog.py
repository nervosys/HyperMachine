import gzip,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-backlog');out.mkdir(exist_ok=True)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
source=Path('/var/tmp/hm-tcp/guest-tcp.cpio.gz');original=sha(source)
assert original=='f226c599d385609fc03af2223b6829a5249b28193c96179daeb7d47383005d1b'
names=['Cargo.lock','crates/hv2-guest-agent/Cargo.toml']+[p.relative_to(root).as_posix() for p in sorted((root/'crates/hv2-guest-agent/src').rglob('*.rs'))]
r={'success':False,'source_sha256':{n:sha(root/n) for n in names},'source_initrd_sha256':original,'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'coordinator_sha256':sha(Path(__file__))}
try:
 command=['cargo','build','--locked','--release','-p','hv2-guest-agent','--target','x86_64-unknown-linux-gnu']
 result=subprocess.run(command,cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-tcp-static-target',RUSTFLAGS='-C target-feature=+crt-static'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 r.update(command=command,exit_code=result.returncode,output_utf8=result.stdout.decode(),output_sha256=hashlib.sha256(result.stdout).hexdigest())
 assert result.returncode==0,result.stdout.decode()[-3000:]
 assert all(sha(root/n)==v for n,v in r['source_sha256'].items())
 for name in names:
  saved=out/'compiled-source'/name;saved.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,saved)
 agent=out/'hv2-guest-agentd';shutil.copy2('/var/tmp/hm-tcp-static-target/x86_64-unknown-linux-gnu/release/hv2-guest-agentd',agent);r['agent_sha256']=sha(agent)
 archive=gzip.decompress(source.read_bytes());entries=subprocess.check_output(['cpio','-t','--quiet'],input=archive).decode().splitlines()
 assert all(not Path(n).is_absolute() and '..' not in Path(n).parts for n in entries)
 with tempfile.TemporaryDirectory(prefix='hm-backlog-image-',dir='/var/tmp') as directory:
  subprocess.run(['cpio','-id','--quiet','--no-absolute-filenames'],input=archive,cwd=directory,check=True)
  folder=Path(directory);before={n:sha(folder/n) for n in entries if (folder/n).is_file() and not (folder/n).is_symlink()}
  (folder/'bin/hv2-guest-agentd').write_bytes(agent.read_bytes());(folder/'bin/hv2-guest-agentd').chmod(0o755)
  assert all(sha(folder/n)==v for n,v in before.items() if n!='bin/hv2-guest-agentd' and n!='./bin/hv2-guest-agentd')
  r['unchanged_files_sha256']={n:v for n,v in before.items() if n not in ['bin/hv2-guest-agentd','./bin/hv2-guest-agentd']}
  packed=subprocess.check_output(['bash','-c','find . -print0 | LC_ALL=C sort -z | xargs -0 touch -h -d @0; find . -print0 | LC_ALL=C sort -z | cpio --null -o -H newc -R 0:0 --reproducible --quiet'],cwd=directory)
  image=out/'guest-tcp.cpio.gz';image.write_bytes(gzip.compress(packed,compresslevel=9,mtime=0));r['output_sha256_initrd']=sha(image)
 r['source_initrd_unchanged']=sha(source)==original;r['success']=True
finally:
 (out/'build.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k not in ['source_sha256','unchanged_files_sha256','output_utf8']}),flush=True)
