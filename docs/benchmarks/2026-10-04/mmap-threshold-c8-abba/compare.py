from pathlib import Path
import json, statistics, math, importlib.util, hashlib
root=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('independent',root/'analysis.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def analyze():
 context=json.loads((root/'context.json').read_text());terminal=json.loads((root/'terminal.json').read_text())
 assert len(terminal)==4 and [t['mmap_threshold'] for t in terminal]==[None,1048576,1048576,None]
 cohorts=[];groups={str(v):{e:[] for e in ['hypermachine','firecracker']} for v in [None,1048576]}
 for i,t in enumerate(terminal,1):
  assert t['cohort']==i and t['exit_code']==0
  r=json.loads((root/f'cohort-{i}.json').read_text());s=m.analyze(root/f'cohort-{i}.json')
  assert s['profile_success'] and s['concurrency']==8 and s['pairs']==4
  assert all(e['passed']==e['attempted']==32 and e['memory_batches']==4 and not e['failures'] for e in s['engines'].values())
  assert r['daemon_allocator_mmap_threshold']==t['mmap_threshold'] and r['daemon_allocator_arena_max'] is None
  assert r['daemon_log_filter']=='warn'
  for name,digest in context['inputs_sha256'].items():assert r['artifact_sha256'][name]==digest
  for name,field in [('bench-local-engines-concurrent.py','harness'),('bench-local-engines.py','shared_harness'),('bench-firecracker-local.py','firecracker_harness')]:assert hashlib.sha256((root/name).read_bytes()).hexdigest()==r['artifact_sha256'][field]
  for b in r['batches']:
   assert b['success'] and b['all_guests_validated_while_held'] and b['memory_idle_actual_seconds']>=5 and b['guest_idle_at_measurement_start_ms']['min']>=5000
   assert all(x['success'] and x['cleanup_success'] for x in b['samples'])
   groups[str(t['mmap_threshold'])][b['engine']].append(b)
  cohorts.append({'cohort':i,'mmap_threshold':t['mmap_threshold'],'summary':s,'hm_memory_by_pair':[{'pair':b['pair'],'empty_baseline_pss_mib':b['empty_process_memory_baseline_kib']['Pss_kib']/1024,'held_pss_mib':b['idle_process_memory_kib']['Pss_kib']/1024,'after_cleanup_pss_mib':b['empty_node_memory_after_cleanup_kib']['Pss_kib']/1024} for b in r['batches'] if b['engine']=='hypermachine']})
 pooled={}
 for setting,engines in groups.items():
  pooled[setting]={}
  for engine,batches in engines.items():
   latencies=sorted(x['ready_ms'] for b in batches for x in b['samples']);assert len(latencies)==64
   pooled[setting][engine]={'passed':len(latencies),'ready_ms':{f'p{p}':latencies[math.ceil(p/100*len(latencies))-1] for p in [50,95,99]},'median_held_pss_mib':statistics.median(b['idle_process_memory_kib']['Pss_kib']/1024 for b in batches),'median_incremental_pss_mib':statistics.median(b['incremental_idle_process_memory_kib']['Pss_kib']/1024 for b in batches),'median_empty_baseline_pss_mib':statistics.median(b['empty_process_memory_baseline_kib']['Pss_kib']/1024 for b in batches)}
 return {'cohorts':cohorts,'pooled':pooled}
if __name__=='__main__':print(json.dumps(analyze(),indent=2))
