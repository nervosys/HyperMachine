import hashlib,json,shutil
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-unix');docs=root/'docs/benchmarks/2026-10-01'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
m={'files':{},'purpose':'native Unix-relay candidate comparison; no managed-platform or universal performance claim'}
def copy(source,name):
 destination=docs/name
 if Path(source).resolve()!=destination.resolve():shutil.copyfile(source,destination)
 m['files'][name]=sha(destination)
for p in sorted(out.glob('*.json')):copy(p,'tcp-unix-'+p.name)
for p in sorted(out.glob('*.log')):copy(p,'tcp-unix-'+p.name)
for p in sorted((out/'compiled-source').rglob('*')):
 if p.is_file():copy(p,'tcp-unix-source-'+p.relative_to(out/'compiled-source').as_posix().replace('/','--'))
for name in ['target/build-tcp-unix.py','target/verify-tcp-unix.py','target/check-tcp-final.py','tools/bench-tcp-local.py','tools/bench-local-engines.py','tools/bench-firecracker-local.py','tools/e2e-tcp-tunnel.py']:
 copy(root/name,'tcp-unix-'+Path(name).name)
copy(root/'target/tcp-unix-checks-windows.json','tcp-unix-checks-windows.json')
copy(Path(__file__),'tcp-unix-archive-coordinator.py')
copy(root/'docs/benchmarks/2026-10-01/verify-tcp-unix.py','verify-tcp-unix.py')
(docs/'tcp-unix-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
print(json.dumps({'files':len(m['files']),'success':True}))
