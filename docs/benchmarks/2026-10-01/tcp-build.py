import hashlib,json,os,shutil,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-tcp');out.mkdir(exist_ok=True)
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
canonical=Path('/var/tmp/hm-competitive-target/release/hv2-sandboxd')
original=digest(canonical)
assert original=='8ea52a369067d99f281b96bc513c59216304ac8e7eea0462d746f854e9d7aa5d'
names=['Cargo.lock','Cargo.toml','crates/hv2-core/src/devices/virtio_vsock.rs','crates/hv2-agent/src/guest_agent.rs','crates/hv2-guest-agent/src/bin/agentd.rs','crates/hv2-api/src/tcp_tunnel.rs','crates/hv2-sandboxd/src/forwards.rs','crates/hv2-sandboxd/src/main.rs','crates/hv2-cluster/src/control.rs','crates/hv2-cluster/src/mtls.rs','crates/hv2-cluster/src/bin/hv2-control-plane.rs','crates/hm-cli/src/sandbox_vm.rs']
metadata={'source_sha256':{name:digest(root/name) for name in names},'source_commit':subprocess.check_output(['git','-c',f'safe.directory={root}','rev-parse','HEAD'],cwd=root,text=True).strip(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'profile':'debug for functional verification; no performance scores','commands':[],'success':False}
try:
 for label,command,env in [
  ('host',['cargo','build','--locked','-p','hv2-sandboxd','-p','hv2-cluster','--bin','hv2-sandboxd','--bin','hv2-control-plane','-p','hm-cli','--bin','hm'],dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target')),
  ('guest',['cargo','build','--locked','--release','-p','hv2-guest-agent','--target','x86_64-unknown-linux-gnu'],dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-tcp-static-target',RUSTFLAGS='-C target-feature=+crt-static'))]:
  result=subprocess.run(command,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (out/f'{label}-build.txt').write_bytes(result.stdout)
  metadata['commands'].append({'command':command,'target_dir':env['CARGO_TARGET_DIR'],'rustflags':env.get('RUSTFLAGS'),'exit_code':result.returncode,'output_sha256':digest(out/f'{label}-build.txt')})
  assert result.returncode==0, result.stdout.decode(errors='replace')[-6000:]
 assert all(digest(root/name)==value for name,value in metadata['source_sha256'].items())
 metadata['binary_sha256']={}
 for name in ['hv2-sandboxd','hv2-control-plane','hm']:
  shutil.copy2(Path('/var/tmp/hm-competitive-target/debug')/name,out/name)
  (out/name).chmod(0o755);metadata['binary_sha256'][name]=digest(out/name)
 shutil.copy2('/var/tmp/hm-tcp-static-target/x86_64-unknown-linux-gnu/release/hv2-guest-agentd',out/'hv2-guest-agentd')
 metadata['binary_sha256']['hv2-guest-agentd']=digest(out/'hv2-guest-agentd')
 for name in names:
  target=out/'compiled-source'/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,target)
 metadata['success']=True
finally:
 metadata['canonical_scored_binary_unchanged']=digest(canonical)==original
 (out/'build.json').write_text(json.dumps(metadata,indent=2)+'\n')
 print(json.dumps({k:v for k,v in metadata.items() if k not in ['source_sha256']}),flush=True)
