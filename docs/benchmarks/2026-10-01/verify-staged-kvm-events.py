import hashlib,json,math,subprocess
prefix='docs/benchmarks/2026-10-01/'
def raw(name):return subprocess.check_output(['git','show',':'+prefix+name])
def data(name):return json.loads(raw(name))
def digest(name):return hashlib.sha256(raw(name)).hexdigest()
metadata=data('kvm-events-build-metadata.json');m=data('kvm-events-results-matrix.json')
for key,name in [('backend_source','kvm-events-backend.rs'),('ffi_source','kvm-events-ffi.rs'),('snapshot_source','kvm-events-snapshot.rs'),('test_log','kvm-events-direct-test.txt'),('release_build_log','kvm-events-build-output.txt')]:assert digest(name)==metadata['artifact_sha256'][key],key
assert subprocess.check_output(['git','diff',metadata['baseline_runtime_commit'],metadata['source_base_commit'],'--','crates'])==b''
assert digest('kvm-events-matrix.py')==m['matrix_sha256']
assert digest('kvm-events-stateful-coordinator.py')==m['coordinator_sha256']
assert len(m['cohorts'])==16
cases=[json.loads(line.removeprefix('KVM_EVENTS_EVIDENCE ')) for line in raw('kvm-events-direct-test.txt').decode().splitlines() if line.startswith('KVM_EVENTS_EVIDENCE ')]
assert len(cases)==4 and cases==metadata['direct_test']['cases']
assert all(v['captured']==v['restored'] and len(v['captured'])==64 for v in cases)
irq=next(v for v in cases if v['case']=='interrupt')
assert irq['captured'][8]==1 and irq['legacy_restored'][8]==0
assert irq['restored_guest_marker']==34 and irq['legacy_guest_marker']==17
assert b'1 passed; 0 failed; 0 ignored' in raw('kvm-events-direct-test.txt')
counts={p:{'attempts':0,'passed':0} for p in ('baseline','events')};failed=[]
for entry in m['cohorts']:
 assert digest(entry['file'])==entry['report_sha256']
 r=data(entry['file']);s=r['sdk'];p=entry['profile'];passed=[v for v in s['samples'] if v['success']]
 assert r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
 assert all(r['controlled_cpu_load'][k] for k in ('all_alive_through_cohort','workers_cleaned_up'))
 assert r['runtime_profile']==p and r['artifact_sha256']['coordinator']==m['coordinator_sha256']
 assert r['artifact_sha256']['daemon']==metadata['artifact_sha256'][p+'_binary']
 assert s['harness_sha256']==r['artifact_sha256']['sdk_harness']==digest('kvm-events-sdk-harness.py')
 assert r['artifact_sha256']['request_harness']==digest('stateful-request-harness.py')
 assert s['harness_unchanged_during_run'] and s['synchronized_operation_batches'] and s['sdk_retries']==0
 assert (entry['exit_code']==0)==r['success'] and (r['sdk_exit_code']==0)==(len(passed)==len(s['samples']))
 assert len(passed)==s['successful_samples'] and len(s['samples'])-len(passed)==s['failed_samples']
 for row in passed:
  assert row['cpu_count']==1 and row['memory_mb']==1024 and not row.get('cleanup_errors')
  if s['operation']=='fork':assert row['fork_filesystem_isolation_verified'] and row['sandbox_id']!=row['parent_sandbox_id']
 values=sorted(v['ready_ms'] for v in passed)
 for key,q in [('p50',.5),('p95',.95),('p99',.99)]:assert s['ready_ms'][key]==values[math.ceil(len(values)*q)-1]
 for batch in s['operation_batches']:
  rows=[v for v in s['samples'] if v['batch_index']==batch['batch_index']]
  offsets=[v['operation_start_offset_ms'] for v in rows if v.get('operation_start_offset_ms') is not None]
  assert batch['attempted']==len(rows)==s['concurrency'] and batch['operations_started']==len(offsets)
  assert batch['operation_start_spread_ms']==max(offsets)-min(offsets)
 counts[p]['attempts']+=len(s['samples']);counts[p]['passed']+=len(passed)
 failed.extend((entry['file'],v) for v in s['samples'] if not v['success'])
assert counts=={'baseline':{'attempts':1008,'passed':1007},'events':{'attempts':1008,'passed':1008}}
assert len(failed)==1 and failed[0][0]=='events-baseline-resume-c1.json'
assert failed[0][1]['phase']=='resume' and failed[0][1]['index']==43 and failed[0][1]['failure_elapsed_ms']>15000
summary=data('kvm-events-summary.json');assert summary['total_attempts']==2016 and summary['total_passed']==2015 and len(summary['failed_rows'])==1
assert next(v for v in summary['paired_deltas'] if v['concurrency']==1 and v['operation']=='resume')['complete_pair'] is False
state=data('kvm-events-state.json')
assert state['success'] and state['artifacts_unchanged'] and not state['cleanup_errors']
assert state['artifact_sha256']['probe']==digest('kvm-events-state-coordinator.py')
assert state['artifact_sha256']['daemon']==metadata['artifact_sha256']['events_binary']
header=data('kvm-events-state-header.json')
assert header['success'] and header['artifacts_unchanged'] and not header['cleanup_errors']
assert header['checkpoint_event_payload_bytes']==[64]
assert header['artifact_sha256']['probe']==digest('kvm-events-state-header-coordinator.py')
assert header['artifact_sha256']['daemon']==metadata['artifact_sha256']['events_binary']
print(json.dumps({'staged_hashes_verified':True,'direct_cases':4,'guest_handler_control_verified':True,'counts':counts,'failed_rows':1}))