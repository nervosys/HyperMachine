import hashlib,json,shutil
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');docs=root/'docs/benchmarks/2026-10-01'
fixture=Path('/var/tmp/hm-tcp-fixture-diagnostic');api=Path('/var/tmp/hm-tcp-api-nodelay')
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
manifest={'files':{},'purpose':'TCP delayed-tail investigation and verification; local shared nested-KVM host'}
def copy(path,name):
 destination=docs/name
 if Path(path).resolve()!=destination.resolve():shutil.copyfile(path,destination)
 manifest['files'][name]=sha(destination)
for path in sorted(fixture.glob('*.json')):copy(path,'tcp-flow-fixture-'+path.name)
for name in ['measured-harness.py','measured-fixture.c','trace-node.log']:
 copy(fixture/name,'tcp-flow-'+name)
for name in ['tools/bench-tcp-local.py','tools/guest-image/tcp-fixture.c','tools/bench-local-engines.py','tools/bench-firecracker-local.py','tools/e2e-tcp-tunnel.py','target/build-tcp-fixture-diagnostic.py','target/diagnose-tcp-fixture.py','target/check-tcp-fixture-mode.py','target/trace-tcp-transfers.py','target/build-tcp-api-nodelay.py']:
 copy(root/name,'tcp-flow-'+Path(name).name)
assert sha(fixture/'measured-image.cpio.gz')==json.loads((fixture/'measured-image-build.json').read_text())['output_sha256']
assert sha(fixture/'guest-tcp.cpio.gz')==json.loads((fixture/'image-build.json').read_text())['output_sha256']
for path in sorted(api.glob('*.json')):copy(path,'tcp-flow-api-'+path.name)
for path in sorted(api.glob('*.log')):copy(path,'tcp-flow-api-'+path.name)
copy(api/'original-verify-tcp.py','tcp-flow-original-verify-tcp.py')
for path in sorted((api/'compiled-source').rglob('*')):
 if path.is_file():copy(path,'tcp-flow-api-compiled-'+path.relative_to(api/'compiled-source').as_posix().replace('/','--'))
for path in sorted((api/'final').glob('*.json')):
 name='tcp-flow-checks-linux.json' if path.name=='checks-linux.json' else 'tcp-flow-final-'+path.name
 copy(path,name)
for path in sorted((api/'final').glob('*.log')):copy(path,'tcp-flow-final-'+path.name)
for path in sorted((api/'final/compiled-source').rglob('*')):
 if path.is_file():copy(path,'tcp-flow-final-compiled-'+path.relative_to(api/'final/compiled-source').as_posix().replace('/','--'))
windows=root/'target/tcp-flow-checks-windows.json'
if windows.exists():copy(windows,'tcp-flow-checks-windows.json')
for name in ['target/build-tcp-api-nodelay-final.py','target/verify-tcp-api-nodelay.py','target/check-tcp-final.py']:
 copy(root/name,'tcp-flow-'+Path(name).name)
copy(Path(__file__),'tcp-flow-archive-coordinator.py')
copy(root/'docs/benchmarks/2026-10-01/verify-tcp-flow.py','verify-tcp-flow.py')
(docs/'tcp-flow-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'files':len(manifest['files']),'success':True}))
