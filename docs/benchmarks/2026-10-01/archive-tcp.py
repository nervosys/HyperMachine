import hashlib,json,shutil
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp');docs=root/'docs/benchmarks/2026-10-01'
digest=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
manifest={'files':{},'checks':{},'claim':'functional raw TCP verification; no performance or full-platform parity claim'}
def copy(source,name):
 destination=docs/name;shutil.copyfile(source,destination);manifest['files'][name]=digest(destination);return destination
for index in range(1,9):
 report_path=copy(out/f'e2e-attempt{index}.json',f'tcp-e2e-attempt{index}.json');report=json.loads(report_path.read_text())
 source=(root/f'target/e2e-tcp-attempt{index}-source.py') if index<8 else root/'tools/e2e-tcp-tunnel.py'
 archived=copy(source,f'tcp-e2e-attempt{index}-source.py');assert digest(archived)==report['artifact_sha256']['coordinator']
 for logfile in sorted(out.glob(f'e2e-attempt{index}-*.log')):copy(logfile,'tcp-'+logfile.name)
 manifest['checks'][f'attempt{index}']={'success':report['success'],'passed':sum(row['success'] for row in report['cases']),'cases':len(report['cases']),'remaining_sandboxes':report.get('remaining_sandboxes'),'cleanup_errors':report['cleanup_errors']}
 assert all(row['exit_code'] is not None for row in report['owned_processes_stopped'])
assert manifest['checks']['attempt4']=={'success':True,'passed':13,'cases':13,'remaining_sandboxes':0,'cleanup_errors':[]}
assert manifest['checks']['attempt8']=={'success':True,'passed':15,'cases':15,'remaining_sandboxes':0,'cleanup_errors':[]}
before=json.loads((out/'e2e-attempt7.json').read_text());after=json.loads((out/'e2e-attempt8.json').read_text())
assert before['idle_probe']['observed_state']=='paused' and after['idle_probe']['observed_state']=='running'
assert all(before['artifact_sha256'][name]==value for name,value in after['artifact_sha256'].items() if name!='daemon')
manifest['matched_idle_probe']={'before':'paused','after':'running','idle_window_seconds':30,'held_seconds':35,'same_inputs_except_daemon':True}
for number,folder in [(1,out/'build-1'),(2,out/'build-2'),(3,out/'build-3'),(4,out)]:
 metadata=json.loads((folder/'build.json').read_text());assert metadata['success'] and metadata['canonical_scored_binary_unchanged']
 copy(folder/'build.json',f'tcp-build{number}.json')
 for label in ['host','guest']:
  archived=copy(folder/f'{label}-build.txt',f'tcp-build{number}-{label}-output.txt')
  item=next(item for item in metadata['commands'] if label in ('guest' if item['rustflags'] else 'host'))
  assert digest(archived)==item['output_sha256'] and item['exit_code']==0
 for name,expected in metadata['source_sha256'].items():
  archived=copy(folder/'compiled-source'/name,f'tcp-build{number}-source-'+name.replace('/','--')+'.txt');assert digest(archived)==expected
 if number==4:
  assert all(digest(out/name)==expected for name,expected in metadata['binary_sha256'].items())
  latest=json.loads((out/'e2e-attempt8.json').read_text())
  assert latest['artifact_sha256']['daemon']==metadata['binary_sha256']['hv2-sandboxd']
  assert latest['artifact_sha256']['control-plane']==metadata['binary_sha256']['hv2-control-plane']
  assert latest['artifact_sha256']['cli']==metadata['binary_sha256']['hm']
image1=json.loads((out/'build-2/image-build.json').read_text())
copy(out/'build-2/image-build.json','tcp-image1-build.json')
source1=docs/'tcp-image1-fixture.c'
if not source1.exists():shutil.copyfile(docs/'tcp-fixture.c',source1)
assert digest(source1)==image1['fixture_source_sha256'];manifest['files'][source1.name]=digest(source1)
assert digest(out/'build-2/guest-tcp.cpio.gz')==image1['output_sha256']
image=json.loads((out/'image-build.json').read_text());assert image['source_initrd_unchanged']
assert digest(root/'tools/guest-image/tcp-fixture.c')==image['fixture_source_sha256']
assert digest(out/'guest-tcp.cpio.gz')==image['output_sha256']
assert image['output_sha256']==latest['artifact_sha256']['initrd']
copy(out/'image-build.json','tcp-image2-build.json')
copy(root/'tools/guest-image/tcp-fixture.c','tcp-fixture.c')
for source,name in [(root/'target/build-tcp.py','tcp-build.py'),(root/'target/build-tcp-image.py','tcp-build-image.py'),(Path(__file__),'archive-tcp.py')]:copy(source,name)
assert digest(docs/'tcp-build-image.py')==image['builder_sha256']
assert digest('/var/tmp/hm-competitive-target/release/hv2-sandboxd')=='8ea52a369067d99f281b96bc513c59216304ac8e7eea0462d746f854e9d7aa5d'
assert digest('/var/tmp/hm-competitive/guest-output-drain.cpio.gz')==image['source_initrd_sha256']
(docs/'tcp-evidence-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'archived_files':len(manifest['files']),'checks':manifest['checks'],'canonical_scored_artifacts_unchanged':True}))
