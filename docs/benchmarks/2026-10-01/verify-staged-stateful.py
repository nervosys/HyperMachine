import hashlib,json,math,subprocess
prefix='docs/benchmarks/2026-10-01/'
def raw(name):return subprocess.check_output(['git','show',':'+prefix+name])
def data(name):return json.loads(raw(name))
def digest(name):return hashlib.sha256(raw(name)).hexdigest()
counts={'initial_attempts':0,'initial_passed':0,'final_attempts':0,'final_passed':0,'smoke_attempts':0,'smoke_passed':0}
failed=[]
for phase,manifest_name,matrix_source,coordinator_source,report_prefix in [('initial','stateful-initial-matrix.json','stateful-initial-matrix.py','stateful-initial-coordinator.py','stateful-initial-'),('final','stateful-results-matrix.json','stateful-sweep-matrix.py','stateful-sweep-coordinator.py','')]:
 manifest=data(manifest_name);assert len(manifest['cohorts'])==8
 assert manifest['matrix_sha256']==digest(matrix_source)
 assert manifest['coordinator_sha256']==digest(coordinator_source)
 for entry in manifest['cohorts']:
  name=report_prefix+entry['file'];assert digest(name)==entry['report_sha256']
  r=data(name);s=r['sdk'];passed=[v for v in s['samples'] if v['success']]
  assert r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
  assert all(r['controlled_cpu_load'][k] for k in ('all_alive_through_cohort','workers_cleaned_up'))
  assert r['artifact_sha256']['coordinator']==digest(coordinator_source)
  assert r['artifact_sha256']['sdk_harness']==s['harness_sha256']==digest('stateful-sdk-harness.py')
  assert r['artifact_sha256']['request_harness']==digest('stateful-request-harness.py')
  assert r['artifact_sha256']['daemon']==data('boot-sizing-build-metadata.json')['artifact_sha256']['baseline_binary']
  assert s['harness_unchanged_during_run'] and s['synchronized_operation_batches'] and s['sdk_retries']==0
  assert len(passed)==s['successful_samples'] and len(s['samples'])-len(passed)==s['failed_samples']
  assert (entry['exit_code']==0)==r['success'] and (r['sdk_exit_code']==0)==(len(passed)==len(s['samples']))
  assert s['expected_cpus']==1 and s['expected_memory_mb']==1024
  for row in passed:
   assert row['cpu_count']==1 and row['memory_mb']==1024 and not row.get('cleanup_errors')
   if s['operation']=='fork':assert row['fork_filesystem_isolation_verified'] and row['parent_sandbox_id']!=row['sandbox_id']
  values=sorted(v['ready_ms'] for v in passed)
  for key,q in [('p50',.5),('p95',.95),('p99',.99)]:assert s['ready_ms'][key]==values[math.ceil(len(values)*q)-1]
  for batch in s['operation_batches']:
   rows=[v for v in s['samples'] if v['batch_index']==batch['batch_index']]
   offsets=[v['operation_start_offset_ms'] for v in rows if v.get('operation_start_offset_ms') is not None]
   assert batch['attempted']==len(rows)==s['concurrency'] and batch['operations_started']==len(offsets)
   assert batch['operation_start_spread_ms']==max(offsets)-min(offsets)
  counts[phase+'_attempts']+=len(s['samples']);counts[phase+'_passed']+=len(passed)
  failed.extend((name,v) for v in s['samples'] if not v['success'])
for operation in ('fork','resume'):
 r=data(f'stateful-initial-smoke-{operation}-c1.json')
 assert r['success'] and r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
 assert r['artifact_sha256']['coordinator']==digest('stateful-initial-coordinator.py')
 assert r['artifact_sha256']['sdk_harness']==digest('stateful-sdk-harness.py')
 counts['smoke_attempts']+=len(r['sdk']['samples']);counts['smoke_passed']+=r['sdk']['successful_samples']
assert counts=={'initial_attempts':636,'initial_passed':636,'final_attempts':1008,'final_passed':1007,'smoke_attempts':2,'smoke_passed':2}
assert len(failed)==1 and failed[0][0]=='stateful-final-fork-c1.json' and failed[0][1]['index']==8 and failed[0][1]['phase']=='fork'
summary=data('stateful-summary.json');assert summary['total_attempts']==1008 and summary['total_passed']==1007 and len(summary['failed_rows'])==1
assert 'within 15s' in data('stateful-final-fork-c1.json')['node_log_tail']
print(json.dumps({'staged_hashes_verified':True,'counts':counts,'failed_rows':1}))