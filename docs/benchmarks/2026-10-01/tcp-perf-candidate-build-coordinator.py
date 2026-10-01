import hashlib,json,os,shutil,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-bench/candidate');out.mkdir(exist_ok=True)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
binary=Path('/var/tmp/hm-competitive-target/release/hv2-sandboxd');original=sha(binary)
assert original=='8ea52a369067d99f281b96bc513c59216304ac8e7eea0462d746f854e9d7aa5d'
backup=out/'scored-original';shutil.copy2(binary,backup)
names=['Cargo.lock','crates/hv2-core/src/devices/virtio_vsock.rs','crates/hv2-agent/src/guest_agent.rs','crates/hv2-sandboxd/src/main.rs','crates/hv2-sandboxd/src/forwards.rs','crates/hv2-api/src/tcp_tunnel.rs']
r={'source_commit':subprocess.check_output(['git','-c',f'safe.directory={root}','rev-parse','HEAD'],cwd=root,text=True).strip(),'source_sha256':{n:sha(root/n) for n in names},'command':['cargo','build','--locked','--release','-p','hv2-sandboxd'],'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'success':False}
try:
 os.utime(root/'crates/hv2-sandboxd/src/main.rs',None)
 result=subprocess.run(r['command'],cwd=root,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 r['exit_code']=result.returncode;r['output_utf8']=result.stdout.decode();r['output_sha256']=hashlib.sha256(result.stdout).hexdigest()
 assert result.returncode==0,r['output_utf8'][-6000:]
 assert all(sha(root/n)==v for n,v in r['source_sha256'].items())
 destination=out/'hv2-sandboxd-release';shutil.copy2(binary,destination);destination.chmod(0o755)
 r['binary_sha256']=sha(destination);r['success']=True
finally:
 shutil.copy2(backup,binary);os.utime(root/'crates/hv2-sandboxd/src/main.rs',None)
 r['canonical_scored_binary_restored']=sha(binary)==original
 (out/'release-build.json').write_text(json.dumps(r,indent=2)+'\n')
 print(json.dumps({k:v for k,v in r.items() if k not in ['output_utf8','source_sha256']}),flush=True)
