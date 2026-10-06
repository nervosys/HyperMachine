import hashlib,json,math,statistics
from pathlib import Path
root=Path('/var/tmp/hm-kvm-events')
m=json.loads((root/'matrix.json').read_bytes());assert len(m['cohorts'])==16
metadata=json.loads((root/'metadata.json').read_bytes())
summary={'profiles':{'baseline':[],'events':[]},'paired_deltas':[],'failed_rows':[],'total_attempts':0,'total_passed':0,'limitations':['One fresh cohort per profile/operation/concurrency; two dependent batches at concurrency 50/100','Shared nested WSL/KVM; same-host official SDK client; no bare-metal or competitor run','Successful percentiles exclude failures; paired means exclude incomplete profile pairs','Observed pass counts do not prove the cause of an earlier timeout or a reliability SLA','The deterministic direct KVM guest-handler control proves the specific event-preservation repair']}
reports={};reference=None
for entry in m['cohorts']:
 raw=(root/entry['file']).read_bytes();assert hashlib.sha256(raw).hexdigest()==entry['report_sha256']
 r=json.loads(raw);s=r['sdk'];p=entry['profile'];c=entry['concurrency'];op=entry['operation'];rows=s['samples'];passed=[v for v in rows if v['success']]
 assert r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
 assert all(r['controlled_cpu_load'][k] for k in ('all_alive_through_cohort','workers_cleaned_up'))
 assert r['runtime_profile']==p and r['artifact_sha256']['coordinator']==m['coordinator_sha256']
 assert r['artifact_sha256']['daemon']==metadata['artifact_sha256'][p+'_binary']
 assert s['harness_unchanged_during_run'] and s['harness_sha256']==r['artifact_sha256']['sdk_harness']
 common={k:v for k,v in r['artifact_sha256'].items() if k!='daemon'}
 if reference is None:reference=common
 else:assert common==reference
 assert s['sdk_retries']==0 and s['synchronized_operation_batches'] and s['concurrency']==c and s['operation']==op
 assert len(rows)==c*max(2,math.ceil(100/c))==s['samples_requested']
 assert len(passed)==s['successful_samples'] and len(rows)-len(passed)==s['failed_samples']
 assert (entry['exit_code']==0)==r['success'] and (r['sdk_exit_code']==0)==(len(passed)==len(rows))
 for row in passed:
  assert row['cpu_count']==1 and row['memory_mb']==1024 and not row.get('cleanup_errors')
  if op=='fork':assert row['fork_filesystem_isolation_verified'] and row['sandbox_id']!=row['parent_sandbox_id']
 for b in s['operation_batches']:
  group=[v for v in rows if v['batch_index']==b['batch_index']];offsets=[v['operation_start_offset_ms'] for v in group if v.get('operation_start_offset_ms') is not None]
  assert b['attempted']==len(group)==c and b['operations_started']==len(offsets)
  assert b['operation_start_spread_ms']==max(offsets)-min(offsets)
 data={'operation':op,'concurrency':c,'attempts':len(rows),'passed':len(passed),'failed':len(rows)-len(passed),'ready_ms':s['ready_ms'],'operation_ms':s['operation_ms'],'max_start_spread_ms':max(b['operation_start_spread_ms'] for b in s['operation_batches']),'raw_file':entry['file']}
 summary['profiles'][p].append(data);reports[(c,op,p)]=s
 summary['total_attempts']+=len(rows);summary['total_passed']+=len(passed)
 summary['failed_rows'].extend(dict(row,profile=p,operation=op,concurrency=c) for row in rows if not row['success'])
for c in (1,8,50,100):
 for op in ('resume','fork'):
  pair={p:reports[(c,op,p)] for p in ('baseline','events')}
  complete=all(s['failed_samples']==0 for s in pair.values())
  summary['paired_deltas'].append({'concurrency':c,'operation':op,'complete_pair':complete,'events_minus_baseline_mean_ready_ms':pair['events']['ready_ms']['mean']-pair['baseline']['ready_ms']['mean'] if complete else None})
summary['artifact_sha256']=reference
assert summary['total_attempts']==2016
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'counts':{p:{'attempts':sum(g['attempts'] for g in groups),'passed':sum(g['passed'] for g in groups)} for p,groups in summary['profiles'].items()},'profiles':summary['profiles'],'paired_deltas':summary['paired_deltas'],'failed_rows':summary['failed_rows']},indent=2))