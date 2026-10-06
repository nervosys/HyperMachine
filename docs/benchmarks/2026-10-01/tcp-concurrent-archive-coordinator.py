import hashlib,json,shutil
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-combined');docs=root/'docs/benchmarks/2026-10-01'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
m={'files':{},'purpose':'matched native sequential/concurrent TCP transfers and combined-buffering candidate audit'}
def copy(source,name):
 destination=docs/name
 if Path(source).resolve()!=destination.resolve():shutil.copyfile(source,destination)
 m['files'][name]=sha(destination)
for p in sorted(out.glob('*.json')):copy(p,'tcp-concurrent-'+p.name)
for p in sorted((out/'compiled-source').rglob('*')):
 if p.is_file():copy(p,'tcp-concurrent-candidate-source-'+p.relative_to(out/'compiled-source').as_posix().replace('/','--'))
for n in ['tools/bench-tcp-local.py','tools/test-bench-tcp-local.py','tools/bench-local-engines.py','tools/bench-firecracker-local.py','target/build-tcp-combined.py','target/verify-tcp-combined.py','target/complete-tcp-combined.py','target/build-tcp-backlog.py','target/verify-tcp-backlog.py','target/check-tcp-backlog-windows.py','target/verify-tcp-backlog-image.py']:
 copy(root/n,'tcp-concurrent-'+Path(n).name)
backlog=Path('/var/tmp/hm-tcp-backlog')
for p in sorted(backlog.glob('*.json')):copy(p,'tcp-backlog-'+p.name)
for p in sorted(backlog.glob('*.log')):copy(p,'tcp-backlog-'+p.name)
for p in sorted((backlog/'compiled-source').rglob('*')):
 if p.is_file():copy(p,'tcp-backlog-source-'+p.relative_to(backlog/'compiled-source').as_posix().replace('/','--'))
copy(root/'target/tcp-backlog-windows.json','tcp-backlog-windows.json')
copy(Path(__file__),'tcp-concurrent-archive-coordinator.py')
copy(root/'docs/benchmarks/2026-10-01/verify-tcp-concurrent.py','verify-tcp-concurrent.py')
(docs/'tcp-concurrent-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
print(json.dumps({'files':len(m['files']),'success':True}))
