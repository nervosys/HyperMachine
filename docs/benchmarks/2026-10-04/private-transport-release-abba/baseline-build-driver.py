from pathlib import Path
import subprocess,os,hashlib,json
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');iso=Path('/var/tmp/hm-egress-log-mA2CCL');p=iso/'crates/hv2-sandboxd/src/forwards.rs';candidate=p.read_bytes();baseline=(root/'docs/benchmarks/2026-10-04/private-transport-abba/baseline-forwards.rs').read_bytes()
log=Path('/var/tmp/hm-private-transport-release-baseline-build-v1.txt');binary=Path('/var/tmp/hm-private-transport-release-baseline-node-v1');assert not binary.exists()
try:
 p.write_bytes(baseline)
 with log.open('x') as f:r=subprocess.run(['cargo','build','--locked','--release','--manifest-path',str(iso/'Cargo.toml'),'-p','hv2-sandboxd','--bin','hv2-sandboxd'],env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target'),stdout=f,stderr=subprocess.STDOUT)
 assert r.returncode==0,log.read_text()[-3000:]
 binary.write_bytes(Path('/var/tmp/hm-object-backup/target/release/hv2-sandboxd').read_bytes());binary.chmod(0o755)
finally:p.write_bytes(candidate)
assert p.read_bytes()==candidate==(root/'crates/hv2-sandboxd/src/forwards.rs').read_bytes()
result={'baseline_binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'baseline_forwards_sha256':hashlib.sha256(baseline).hexdigest(),'candidate_forwards_sha256':hashlib.sha256(candidate).hexdigest(),'isolated_candidate_restored':True}
Path('/var/tmp/hm-private-transport-release-baseline-build-context-v1.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
