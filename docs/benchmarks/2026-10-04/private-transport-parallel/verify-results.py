from pathlib import Path
import json,math,sys
p=Path(__file__).parent
r=json.loads((p/(sys.argv[1] if len(sys.argv)>1 else 'report-dev-v1.json')).read_text());b=r['private_transport_benchmark'];rows=b['rows']
assert len(rows)==136 and all(x['success'] for x in rows)
assert r['guests_remaining']==0 and r['daemon_reaped'] and r['secondary_node_reaped'] and r['control_and_redis_reaped']
assert len(r['checks'])==30
nearest=lambda xs,q:sorted(xs)[math.ceil(len(xs)*q)-1]
result=[]
for s in b['summary']:
 group=[x for x in rows if not x['warmup'] and x['path']==s['path'] and x['payload_bytes']==s['payload_bytes']]
 assert len(group)==s['planned']==s['successful']==32 and s['failed']==0
 assert {x['pair'] for x in group}==set(range(32))
 for metric in ('setup','echo'):
  for suffix,q in [('p50',.5),('p95',.95)]:assert nearest([x[metric+'_ms'] for x in group],q)==s[metric+'_'+suffix+'_ms']
 assert nearest([x['held_target_pss_mib'] for x in group],.5)==s['held_target_pss_mib_p50']
 for metric in ('target_cpu_ms','redis_cpu_ms'):assert sum(x[metric] for x in group)==s[metric+'_sum']
 assert nearest([x['payload_bytes']/1048576/(x['echo_ms']/1000) for x in group],.5)==s['echo_payload_mib_per_second_p50']
 result.append(s)
for size in b['payload_bytes']:
 for pair in range(-2,32):
  g=[x for x in rows if x['pair']==pair and x['payload_bytes']==size]
  assert len(g)==2 and {x['path'] for x in g}=={'private','standard'}
  assert next(x['path'] for x in g if x['order']==0)==('private' if pair%2==0 else 'standard')
print(json.dumps({'raw_rows_verified':len(rows),'scored_rows':128,'summary':result},indent=2))
