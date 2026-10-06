from pathlib import Path
import subprocess,os,hashlib
log=Path('/var/tmp/hm-private-transport-parallel-build-v1.txt')
env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target')
with log.open('x') as f:r=subprocess.run(['cargo','build','--locked','--release','--manifest-path','/var/tmp/hm-egress-log-mA2CCL/Cargo.toml','-p','hv2-sandboxd','--bin','hv2-sandboxd'],env=env,stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0,log.read_text()[-5000:]
src=Path('/var/tmp/hm-object-backup/target/release/hv2-sandboxd');dst=Path('/var/tmp/hm-private-transport-parallel-node-v1');assert not dst.exists();dst.write_bytes(src.read_bytes());dst.chmod(0o755)
print('Frozen',dst,'SHA256',hashlib.sha256(dst.read_bytes()).hexdigest())
