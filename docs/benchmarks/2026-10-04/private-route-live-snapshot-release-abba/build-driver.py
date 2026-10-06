from pathlib import Path
import subprocess,os,shutil,hashlib,json
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');iso=Path('/var/tmp/hm-egress-log-mA2CCL')
c=json.loads((root/'docs/benchmarks/2026-10-04/private-route-live-snapshot/source-context.json').read_text())
for name,h in c['permitted_root_isolated_sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==hashlib.sha256((iso/name).read_bytes()).hexdigest()==h,name
for name,h in c['accepted_isolated_core_sha256'].items():assert hashlib.sha256((iso/name).read_bytes()).hexdigest()==h,name
print('Verified accepted isolated build source catalog',flush=True)
log=Path('/var/tmp/hm-private-route-live-snapshot-release-build-v1.txt')
with log.open('x') as f:r=subprocess.run(['cargo','build','--locked','--release','-p','hv2-sandboxd','--bin','hv2-sandboxd'],cwd=iso,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target'),stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0,log.read_text()[-5000:]
binary=Path('/var/tmp/hm-private-route-live-snapshot-release-node-v1');assert not binary.exists();shutil.copy2('/var/tmp/hm-object-backup/target/release/hv2-sandboxd',binary)
print('Frozen release daemon',hashlib.sha256(binary.read_bytes()).hexdigest())
