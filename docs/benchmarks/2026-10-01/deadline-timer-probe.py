import hashlib,json,os,platform,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=root/'docs/benchmarks/2026-10-01'
paths={'backend':root/'crates/hv2-core/src/backends/kvm.rs','snapshot_types':root/'crates/hv2-core/src/snapshot/types.rs','lock':root/'Cargo.lock'}
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
report={'source_sha256':{k:digest(p) for k,p in paths.items()},'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'kernel':platform.release(),'checks':[]}
env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-competitive-target')
for args in [['test','--locked','-p','hv2-core','--lib','restored_deadline_timer_wakes_halted_guest','--','--ignored','--nocapture'],['test','--locked','-p','hv2-core','--lib','snapshot::types'],['clippy','--locked','-p','hv2-core','--lib','--tests','--','-D','warnings']]:
 result=subprocess.run(['cargo',*args],cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 report['checks'].append({'command':['cargo',*args],'exit_code':result.returncode,'output':result.stdout.decode()})
 print(json.dumps({'command':args,'exit_code':result.returncode,'output':result.stdout.decode()[-1800:]}),flush=True)
report['sources_unchanged']=all(digest(p)==report['source_sha256'][k] for k,p in paths.items())
(out/'deadline-timer-probe.json').write_text(json.dumps(report,indent=2)+'\n')
assert report['sources_unchanged'] and all(c['exit_code']==0 for c in report['checks'])