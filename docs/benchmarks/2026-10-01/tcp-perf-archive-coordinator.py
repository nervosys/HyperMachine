import hashlib,json,shutil,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-tcp-bench');docs=root/'docs/benchmarks/2026-10-01'
sha=lambda data:hashlib.sha256(data).hexdigest()
manifest={'files':{},'purpose':'matched native ready-guest TCP transactions; no managed-platform or universal win claim'}
def save(name,data):
 (docs/name).write_bytes(data);manifest['files'][name]=sha(data)
source=(root/'tools/bench-tcp-local.py').read_bytes()
save('tcp-perf-harness.py',source)
without=b''.join(line for line in source.splitlines(keepends=True) if b'stream.setsockopt(socket.IPPROTO_TCP' not in line and b'report["client_tcp_nodelay"]' not in line)
save('tcp-perf-harness-before-client-nodelay.py',without)
for name in ['bench-local-engines.py','bench-firecracker-local.py']:
 save('tcp-perf-'+name,(root/'tools'/name).read_bytes())
for name in ['smoke-1','smoke-2','matched-1','matched-2','candidate-1','baseline-repeat','candidate-repeat']:
 path=out/(name+'.json')
 if not path.exists():continue
 report=json.loads(path.read_text());save('tcp-perf-'+name+'.json',path.read_bytes())
 if name!='smoke-1':
  expected=without if name in ['smoke-2','matched-1'] else source
  assert sha(expected)==report['artifact_sha256']['harness'],name
  assert report['success'] and len(report['rows'])==(16 if name=='smoke-2' else 400)
for label,directory in [('baseline',out),('candidate',out/'candidate')]:
 path=directory/'release-build.json'
 if not path.exists():continue
 report=json.loads(path.read_text());assert report['success'] and report['canonical_scored_binary_restored']
 save('tcp-perf-'+label+'-build.json',path.read_bytes())
 for name,value in report['source_sha256'].items():
  data=(root/name).read_bytes()
  if sha(data)!=value:
   archived=docs/('tcp-perf-'+label+'-'+name.replace('/','_'))
   if archived.exists() and sha(archived.read_bytes())==value:data=archived.read_bytes()
   else:
    data=subprocess.check_output(['git','-c',f'safe.directory={root}','show',report['source_commit']+':'+name],cwd=root)
    if sha(data)!=value:data=data.replace(b'\n',b'\r\n')
  assert sha(data)==value,(label,name)
  save('tcp-perf-'+label+'-'+name.replace('/','_'),data)
save('tcp-perf-archive-coordinator.py',Path(__file__).read_bytes())
save('tcp-perf-baseline-build-coordinator.py',(root/'target/build-tcp-release.py').read_bytes())
save('tcp-perf-candidate-build-coordinator.py',(root/'target/build-tcp-release-candidate.py').read_bytes())
save('tcp-perf-host-load-observation.json',(out/'host-load-observation.json').read_bytes())
(docs/'tcp-perf-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'files':len(manifest['files']),'success':True}))
