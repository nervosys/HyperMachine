from pathlib import Path
import subprocess,hashlib,json,os,shutil
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');iso=Path('/var/tmp/hm-egress-log-mA2CCL')
c=json.loads((root/'docs/benchmarks/2026-10-04/private-route-live-snapshot/source-context.json').read_text())
for name,h in c['permitted_root_isolated_sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==hashlib.sha256((iso/name).read_bytes()).hexdigest()==h,name
for name,h in c['accepted_isolated_core_sha256'].items():assert hashlib.sha256((iso/name).read_bytes()).hexdigest()==h,name
source=iso/'crates/hv2-sandboxd/src/forwards.rs';original=source.read_bytes();s=original.decode()
a=s.index('async fn port_tunnel(');b=s.index('pub(crate) struct Forwards',a);part=s[a:b]
needle='    use axum::http::StatusCode;'
part=part.replace(needle,needle+"\n    let profile_start=std::time::Instant::now();\n    let profile_label=request.headers().get(\"X-Hv2-Setup-Profile\").and_then(|v|v.to_str().ok()).filter(|v|v.len()<=64 && v.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'-')).map(str::to_owned);",1)
part=part.replace('    let _held = lock.lock().await;','    let _held = lock.lock().await;\n    let preauthorization_us=profile_start.elapsed().as_micros();\n    let profile_stage=std::time::Instant::now();',1)
part=part.replace('    let vm = state.sandboxes.lock().get(&sandbox).map(|live| {','    let authorization_before_us=profile_stage.elapsed().as_micros();\n    let vm = state.sandboxes.lock().get(&sandbox).map(|live| {',1)
part=part.replace('    let opened = if ipv6 {','    let profile_stage=std::time::Instant::now();\n    let opened = if ipv6 {',1)
part=part.replace('    let mut unregistered = UnregisteredStream','    let guest_open_us=profile_stage.elapsed().as_micros();\n    let mut unregistered = UnregisteredStream',1)
part=part.replace('    let pair = async {','    let profile_stage=std::time::Instant::now();\n    let pair = async {',1)
needle='    if let Some(incoming) = &private {';i=part.rindex(needle)
part=part[:i]+'    let loopback_pair_us=profile_stage.elapsed().as_micros();\n    let profile_stage=std::time::Instant::now();\n'+part[i:]
needle='    state\n        .forwards\n        .open'
part=part.replace(needle,'    let authorization_after_us=profile_stage.elapsed().as_micros();\n    if let Some(profile_label)=profile_label {\n        tracing::info!(%profile_label, udp, private_route=private.is_some(), preauthorization_us, authorization_before_us, guest_open_us, loopback_pair_us, authorization_after_us, setup_us=profile_start.elapsed().as_micros(), \"owned_private_setup_profile\");\n    }\n'+needle,1)
s=s[:a]+part+s[b:];diagnostic=Path('/var/tmp/hm-private-setup-profile-forwards-v1.rs');diagnostic.write_text(s)
checker=(root/'tools/check-udp-cluster-kvm.py').read_text()
checker=checker.replace('standard=False, udp=False, protocol=None):','standard=False, udp=False, protocol=None, setup_profile=None):',1)
checker=checker.replace("                            if duplicate:message+=", "                            if setup_profile is not None:message+=f'X-Hv2-Setup-Profile: {setup_profile}\\r\\n'\n                            if duplicate:message+=",1)
checker=checker.replace("standard=kind=='standard',udp=True)","standard=kind=='standard',udp=True,setup_profile=f'udp-{size}-{pair}-{kind}')",1)
checker=checker.replace('{setup_profile}'+chr(92)*2+'r'+chr(92)*2+'n','{setup_profile}'+chr(92)+'r'+chr(92)+'n')
compile(checker,'owned-profile-checker','exec');Path('/var/tmp/hm-private-setup-profile-checker-v1.py').write_text(checker)
print('Prepared diagnostic-only timers and tagged owned benchmark checker',flush=True)
try:
 source.write_text(s)
 log=Path('/var/tmp/hm-private-setup-profile-release-build-v1.txt')
 with log.open('x') as f:r=subprocess.run(['cargo','build','--locked','--release','-p','hv2-sandboxd','--bin','hv2-sandboxd'],cwd=iso,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target'),stdout=f,stderr=subprocess.STDOUT)
 assert r.returncode==0,log.read_text()[-5000:]
 binary=Path('/var/tmp/hm-private-setup-profile-release-node-v1');assert not binary.exists();shutil.copy2('/var/tmp/hm-object-backup/target/release/hv2-sandboxd',binary)
 print('Frozen diagnostic daemon',hashlib.sha256(binary.read_bytes()).hexdigest(),flush=True)
finally:
 source.write_bytes(original);assert source.read_bytes()==original
 print('Restored accepted isolated receiver source',flush=True)
