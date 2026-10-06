import hashlib,json,os,shutil,subprocess,time
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-refusal-retry');out.mkdir(exist_ok=True)
binary=Path('/var/tmp/hm-competitive-target/release/hv2-sandboxd')
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
original=digest(binary)
assert original=='8ea52a369067d99f281b96bc513c59216304ac8e7eea0462d746f854e9d7aa5d'
backup=out/'scored-original';shutil.copyfile(binary,backup)
paths={name:root/name for name in ['crates/hv2-agent/src/guest_agent.rs','crates/hv2-core/src/backends/kvm.rs','crates/hv2-core/src/snapshot/types.rs','crates/hv2-sandboxd/src/main.rs','Cargo.lock']}
metadata={'source_sha256':{k:digest(p) for k,p in paths.items()},'source_commit':subprocess.check_output(['git','-c',f'safe.directory={root}','rev-parse','HEAD'],cwd=root,text=True).strip(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'original_binary_sha256':original,'command':['cargo','build','--locked','--release','-p','hv2-sandboxd'],'build_exit_code':None}
try:
 os.utime(paths['crates/hv2-sandboxd/src/main.rs'],None)
 result=subprocess.run(metadata['command'],cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 (out/'candidate-build-output.txt').write_bytes(result.stdout)
 metadata['build_exit_code']=result.returncode
 assert result.returncode==0
 assert all(digest(p)==metadata['source_sha256'][k] for k,p in paths.items())
 shutil.copyfile(binary,out/'hv2-sandboxd-candidate')
 metadata['candidate_binary_sha256']=digest(out/'hv2-sandboxd-candidate')
finally:
 shutil.copyfile(backup,binary)
 os.utime(paths['crates/hv2-sandboxd/src/main.rs'],None)
 metadata['canonical_scored_binary_restored']=digest(binary)==original
 (out/'candidate-build-metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
print(json.dumps(metadata),flush=True)