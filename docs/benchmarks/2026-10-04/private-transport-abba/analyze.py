from pathlib import Path
import json,math
p=Path(__file__).parent;result=[];common=None
nearest=lambda xs,q:sorted(xs)[math.ceil(len(xs)*q)-1]
for index,kind in enumerate(['baseline','candidate','candidate','baseline']):
 label=str(index)+'-'+kind;r=json.loads((p/(label+'-report.json')).read_text());b=r['private_transport_benchmark'];rows=b['rows']
 assert rows==[json.loads(x) for x in (p/(label+'-private-transport-rows.jsonl')).read_text().splitlines()]
 assert b==json.loads((p/(label+'-private-transport-benchmark.json')).read_text())
 assert len(rows)==136 and all(x['success'] for x in rows) and len(r['checks'])==32 and r['guests_remaining']==0
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 hashes={path:digest for path,digest in r['inputs_sha256'].items() if 'node-v' not in path}
 if common is None:common=hashes
 else:assert hashes==common
 for s in b['summary']:
  g=[x for x in rows if not x['warmup'] and x['path']==s['path'] and x['payload_bytes']==s['payload_bytes']]
  assert len(g)==s['planned']==s['successful']==32 and s['failed']==0
  for metric in ['setup','echo']:
   for suffix,q in [('p50',.5),('p95',.95)]:assert nearest([x[metric+'_ms'] for x in g],q)==s[metric+'_'+suffix+'_ms']
  for key in ['target_cpu_ms','redis_cpu_ms']:assert sum(x[key] for x in g)==s[key+'_sum']
  assert nearest([x['held_target_pss_mib'] for x in g],.5)==s['held_target_pss_mib_p50']
  assert nearest([x['payload_bytes']/1048576/(x['echo_ms']/1000) for x in g],.5)==s['echo_payload_mib_per_second_p50']
 for size in [64,1048576]:
  gaps=[]
  for pair in range(-2,32):
   g=[x for x in rows if x['payload_bytes']==size and x['pair']==pair];assert len(g)==2
   assert next(x['path'] for x in g if x['order']==0)==('private' if pair%2==0 else 'standard')
   bypath={x['path']:x for x in g};assert set(bypath)=={'private','standard'}
   if pair>=0:gaps.append(bypath['private']['setup_ms']-bypath['standard']['setup_ms'])
  result.append({'cohort':label,'payload_bytes':size,'paired_setup_overhead_p50_ms':nearest(gaps,.5),'paired_setup_differences_ms':gaps})
print(json.dumps({'scored_operations_verified':512,'warmups_verified':32,'cohort_results':result},indent=2))
