import hashlib,json,math,statistics
from pathlib import Path
root=Path('/var/tmp/hm-stateful-sweep')
repo=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
archive=repo/'docs/benchmarks/2026-10-01'
manifest=json.loads((root/'full-matrix.json').read_bytes())
assert len(manifest['cohorts'])==8
summary={'sample_groups':[],'failed_rows':[],'total_attempts':0,'total_passed':0,'limitations':['Shared nested WSL/KVM, eight host CPUs and one pinned CPU worker; no bare-metal run','No matched competitor run or performance win','SDK operations are synchronized after preparation; observed client launch spreads are reported','Two batches at concurrency 50/100; guests in a batch are dependent samples','Pause timings come from preparation, not synchronized pause bursts','Readiness stops at the first state-verified command; fork filesystem independence is checked afterward','Short BusyBox workload; no oversubscription, sustained arrivals, large heap or application recovery SLA']}
reference=None
for entry in manifest['cohorts']:
 path=root/entry['file'];raw=path.read_bytes()
 assert hashlib.sha256(raw).hexdigest()==entry['report_sha256']
 r=json.loads(raw);s=r['sdk'];c=entry['concurrency'];operation=entry['operation'];batches=max(2,math.ceil(100/c))
 assert r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
 assert all(r['controlled_cpu_load'][k] for k in ('all_alive_through_cohort','workers_cleaned_up'))
 assert s['harness_unchanged_during_run'] and s['harness_sha256']==r['artifact_sha256']['sdk_harness']
 assert s['synchronized_operation_batches'] and s['operation']==operation and s['concurrency']==c
 assert s['samples_requested']==c*batches and len(s['samples'])==c*batches and r['batches']==batches
 assert len(s['operation_batches'])==batches and s['sdk_retries']==0
 assert r['artifact_sha256']['coordinator']==manifest['coordinator_sha256']
 current={k:v for k,v in r['artifact_sha256'].items() if k!='coordinator'}
 if reference is None:reference=current
 else:assert current==reference
 rows=s['samples'];passed=[v for v in rows if v['success']]
 assert len(passed)==s['successful_samples'] and len(rows)-len(passed)==s['failed_samples']
 assert (entry['exit_code']==0)==r['success'] and (r['sdk_exit_code']==0)==(len(passed)==len(rows))
 for row in passed:
  assert row['cpu_count']==1 and row['memory_mb']==1024 and 'operation_start_offset_ms' in row and not row.get('cleanup_errors')
  if operation=='fork':assert row['fork_filesystem_isolation_verified'] and row['parent_sandbox_id']!=row['sandbox_id']
  else:assert row['pause_ms']>=0
 ids=[v['sandbox_id'] for v in passed];assert len(ids)==len(set(ids))
 if operation=='fork':
  parents=[v['parent_sandbox_id'] for v in passed];assert len(parents)==len(set(parents)) and not set(ids)&set(parents)
 spreads=[]
 for batch in s['operation_batches']:
  rows_in_batch=[v for v in rows if v['batch_index']==batch['batch_index']]
  offsets=[v['operation_start_offset_ms'] for v in rows_in_batch if v.get('operation_start_offset_ms') is not None]
  assert batch['attempted']==len(rows_in_batch)==c and batch['operations_started']==len(offsets)
  spread=max(offsets)-min(offsets) if offsets else None
  assert spread==batch['operation_start_spread_ms']
  if spread is not None:spreads.append(spread)
 group={'operation':operation,'concurrency':c,'batches':batches,'attempts':len(rows),'passed':len(passed),'failed':len(rows)-len(passed),'ready_ms':s['ready_ms'],'operation_ms':s['operation_ms'],'pause_preparation_ms':s['pause_ms'],'max_operation_start_spread_ms':max(spreads) if spreads else None,'raw_file':entry['file']}
 summary['sample_groups'].append(group);summary['total_attempts']+=len(rows);summary['total_passed']+=len(passed)
 summary['failed_rows'].extend(dict(row,operation=operation,concurrency=c) for row in rows if not row['success'])
summary['artifact_sha256']=reference
assert summary['total_attempts']==1008
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))