import hashlib,json,subprocess
prefix='docs/benchmarks/2026-10-01/'
def raw(name):return subprocess.check_output(['git','show',':'+prefix+name])
def data(name):return json.loads(raw(name))
def digest(name):return hashlib.sha256(raw(name)).hexdigest()
matrix=data('boot-sizing-results-matrix.json')
assert len(matrix['cohorts'])==12
assert digest('boot-sizing-matrix.py')==matrix['matrix_sha256']
assert digest('boot-sizing-coordinator.py')==matrix['coordinator_sha256']
metadata=data('boot-sizing-build-metadata.json')
for key,name in [('optimized_linux_source','boot-sizing-linux-optimized.rs'),('optimized_loaded_boot_source','boot-sizing-loaded-optimized.rs'),('baseline_linux_source','boot-sizing-linux-baseline.rs'),('baseline_loaded_boot_source','boot-sizing-loaded-baseline.rs')]:
 assert digest(name)==metadata['artifact_sha256'][key],key
counts={'hypermachine':0,'firecracker':0}
for entry in matrix['cohorts']:
 name='boot-sizing-c{}-block{}-{}.json'.format(entry['concurrency'],entry['block'],entry['profile'])
 assert digest(name)==entry['report_sha256'],name
 r=data(name)
 assert r['success'] and entry['exit_code']==0 and r['cohort_exit_code']==0
 assert r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
 assert r['controlled_cpu_load']['all_alive_through_cohort'] and r['controlled_cpu_load']['workers_cleaned_up']
 assert r['coordinator_sha256']==digest('boot-sizing-coordinator.py')
 for key,path in [('harness','boot-sizing-bench-local-engines-concurrent.py'),('shared_harness','boot-sizing-bench-local-engines.py'),('firecracker_harness','boot-sizing-bench-firecracker-local.py')]:assert r['artifact_sha256'][key]==digest(path)
 key='baseline_binary' if entry['profile']=='baseline' else 'optimized_binary'
 assert r['artifact_sha256']['hypermachine']==metadata['artifact_sha256'][key]
 for batch in r['batches']:
  assert batch['success'] and batch['all_guests_validated_while_held']
  assert len(batch['samples'])==entry['concurrency']
  assert all(row['success'] and row['cleanup_success'] for row in batch['samples'])
  counts[batch['engine']]+=len(batch['samples'])
state=data('boot-sizing-state.json')
assert state['success'] and state['artifacts_unchanged'] and not state['cleanup_errors']
assert state['artifact_sha256']['probe']==digest('boot-sizing-state-coordinator.py')
assert state['artifact_sha256']['daemon']==metadata['artifact_sha256']['optimized_binary']
assert counts=={'hypermachine':1664,'firecracker':1664}
for c in (8,100):
 summary=data(f'boot-sizing-c{c}-summary.json')
 assert summary['candidate_adopted'] is False
 for profile in summary['profiles'].values():
  for engine in profile.values():assert engine['failed']==0
print(json.dumps({'staged_hashes_verified':True,'attempts':counts,'cohorts':12,'candidate_adopted':False}))