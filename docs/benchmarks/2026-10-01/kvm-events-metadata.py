import hashlib,json,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-kvm-events')
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
paths={'events_binary':out/'hv2-sandboxd-events','baseline_binary':Path('/var/tmp/hm-boot-sizing/hv2-sandboxd-baseline'),'backend_source':root/'crates/hv2-core/src/backends/kvm.rs','ffi_source':root/'crates/hv2-core/src/backends/kvm_ffi.rs','snapshot_source':root/'crates/hv2-core/src/snapshot/vcpu.rs','cargo_lock':root/'Cargo.lock','cargo_manifest':root/'Cargo.toml','test_log':out/'direct-kvm-delivery.log','release_build_log':out/'build.log'}
report={'source_base_commit':'9f48750843b32b3b05e631930bb503a49e371c01', 'baseline_runtime_commit':'5c91befd2bad03bf3cb9a2bc359c9840d5f44bf6','artifact_sha256':{k:digest(p) for k,p in paths.items()},'compiler':subprocess.check_output(['rustc','-Vv'],text=True),'cargo':subprocess.check_output(['cargo','-V'],text=True),'build_command':'CARGO_TARGET_DIR=/var/tmp/hm-competitive-target cargo build --locked --release -p hv2-sandboxd','baseline_note':'Saved parent runtime binary from boot-sizing experiment. git diff 5c91bef HEAD -- crates was empty before these event-state changes; intervening commits changed only benchmarks/docs. Both use the same lockfile and release flags. Different checkout paths can affect embedded source paths.','source_changes':'Additive KVM event payload capture and restoration; event-state control and guest interrupt-handler delivery tests; existing snapshot fields/clock policy retained.'}
log=(out/'direct-kvm-delivery.log').read_text()
cases=[json.loads(line.removeprefix('KVM_EVENTS_EVIDENCE ')) for line in log.splitlines() if line.startswith('KVM_EVENTS_EVIDENCE ')]
assert len(cases)==4 and all(v['captured']==v['restored'] for v in cases)
irq=next(v for v in cases if v['case']=='interrupt')
assert irq['captured'][8]==1 and irq['legacy_restored'][8]==0
assert irq['restored_guest_marker']==34 and irq['legacy_guest_marker']==17
report['direct_test']={'passed':True,'cases':cases}
(out/'metadata.json').write_text(json.dumps(report,indent=2)+'\n')