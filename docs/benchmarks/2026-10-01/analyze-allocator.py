import hashlib, json, statistics
from pathlib import Path
root=Path('/var/tmp/hm-competitive')
manifest=json.loads((root/'allocator-blocks.json').read_bytes())
assert len(manifest['profiles'])==8
summary={'experiment':manifest['experiment'],'profiles':{},'paired_block_deltas':[],'production_defaults_changed':False,'limitations':['Four counterbalanced blocks on shared nested WSL/KVM; no dedicated hardware','Within-batch guests share load and are not independent samples','Successful latency quantiles exclude failed attempts; paired latency means exclude incomplete blocks','Fixed five-second process PSS excludes host kernel memory, long idle and density','Cold creation only; no snapshot, resume, fork, sustained arrivals or managed competitors']}
reports={}
for row in manifest['profiles']:
 path=root/row['file']; raw=path.read_bytes()
 assert hashlib.sha256(raw).hexdigest()==row['raw_sha256']
 r=json.loads(raw)
 assert r['artifacts_unchanged'] and not r['cleanup_errors'] and r['remaining_sandbox_count']==0
 assert r['controlled_cpu_load']['all_alive_through_cohort'] and r['controlled_cpu_load']['workers_cleaned_up']
 assert r['concurrency']==100 and r['pairs']==2 and len(r['batches'])==4 and r['memory_idle_seconds']==5
 assert r['daemon_allocator_arena_max']==(2 if row['profile']=='arena2' else None)
 reports[(row['block'],row['profile'])]=r
reference=next(iter(reports.values()))['artifact_sha256']
assert all(r['artifact_sha256']==reference for r in reports.values())
summary['artifact_sha256']=reference
for profile in ['default','arena2']:
 engine_summary={}
 for engine in ['hypermachine','firecracker']:
  batches=[b for (block,p),r in reports.items() if p==profile for b in r['batches'] if b['engine']==engine]
  rows=[row for b in batches for row in b['samples']]
  passed=[row for row in rows if row['success'] and row['cleanup_success']]
  values=sorted(row['ready_ms'] for row in passed)
  valid=[b for b in batches if b['success'] and b['all_guests_validated_while_held']]
  for b in valid:
   assert b['memory_idle_actual_seconds']>=5 and b['guest_idle_at_measurement_start_ms']['min']>=5000
   assert b['held_process_count']==(1 if engine=='hypermachine' else 100)
   assert b['incremental_idle_process_memory_kib']['Pss_kib']==b['idle_process_memory_kib']['Pss_kib']-b['empty_process_memory_baseline_kib']['Pss_kib']
  def quantile(q):
   import math
   return values[max(0,math.ceil(len(values)*q)-1)] if values else None
  data={'attempts':len(rows),'passed':len(passed),'failed':len(rows)-len(passed),'p50_ready_ms':quantile(.5),'p95_ready_ms':quantile(.95),'p99_ready_ms':quantile(.99),'valid_memory_batches':len(valid),'failed_rows':[row for row in rows if not row['success'] or not row['cleanup_success']]}
  if valid:
   data.update(median_held_idle_pss_mib=statistics.median(b['idle_process_memory_kib']['Pss_kib']/1024 for b in valid),median_incremental_idle_pss_mib=statistics.median(b['incremental_idle_process_memory_kib']['Pss_kib']/1024 for b in valid))
   if engine=='hypermachine': data['median_post_cleanup_pss_mib']=statistics.median(b['empty_node_memory_after_cleanup_kib']['Pss_kib']/1024 for b in valid)
  engine_summary[engine]=data
 summary['profiles'][profile]=engine_summary
for block in range(4):
 delta={'block':block}
 for engine in ['hypermachine','firecracker']:
  groups={profile:[b for b in reports[(block,profile)]['batches'] if b['engine']==engine] for profile in ['default','arena2']}
  if all(all(b['success'] and b['all_guests_validated_while_held'] for b in batches) for batches in groups.values()):
   def mean_ready(batches):return statistics.mean(row['ready_ms'] for b in batches for row in b['samples'])
   def mean_field(batches,field):return statistics.mean(b[field]['Pss_kib']/1024 for b in batches)
   entry={'arena2_minus_default_mean_ready_ms':mean_ready(groups['arena2'])-mean_ready(groups['default']), 'arena2_minus_default_mean_held_idle_pss_mib':mean_field(groups['arena2'],'idle_process_memory_kib')-mean_field(groups['default'],'idle_process_memory_kib')}
   if engine=='hypermachine':entry['arena2_minus_default_mean_post_cleanup_pss_mib']=mean_field(groups['arena2'],'empty_node_memory_after_cleanup_kib')-mean_field(groups['default'],'empty_node_memory_after_cleanup_kib')
   delta[engine]=entry
  else:delta[engine]={'excluded_due_to_incomplete_block':True}
 summary['paired_block_deltas'].append(delta)
(root/'allocator-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'profiles':{p:{e:{k:v for k,v in d.items() if k!='failed_rows'} for e,d in engines.items()} for p,engines in summary['profiles'].items()},'paired_block_deltas':summary['paired_block_deltas']},indent=2))