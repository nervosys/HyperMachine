from pathlib import Path
import json,math,statistics,hashlib
p=Path(__file__).parent;inputs=None
for cohort in ['a','b']:
 r=json.loads((p/f'cohort-{cohort}-report.json').read_text());b=json.loads((p/f'cohort-{cohort}-benchmark.json').read_text())
 assert len(r['checks'])==51 and r['guests_remaining']==0 and r['private_udp_transport_benchmark']==b
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 assert b['host_build_profile']=='release' and b['all_scored_success'] and b['concurrency']==1
 assert b['samples_per_path_payload']==32 and b['payload_bytes']==[64,65507] and len(b['rows'])==136
 rows=[json.loads(line) for line in (p/f'cohort-{cohort}-rows.jsonl').read_text().splitlines()];assert rows==b['rows']
 assert len([row for row in rows if not row['warmup']])==128 and all(row['success'] for row in rows)
 for size in [64,65507]:
  for pair in range(-2,32):
   selected=sorted([row for row in rows if row['pair']==pair and row['payload_bytes']==size],key=lambda row:row['order'])
   assert [row['path'] for row in selected]==(['private','standard'] if pair%2==0 else ['standard','private'])
   assert all(row['warmup']==(pair<0) for row in selected)
  for kind in ['private','standard']:
   selected=[row for row in rows if row['path']==kind and row['payload_bytes']==size and not row['warmup']]
   summary=next(row for row in b['summary'] if row['path']==kind and row['payload_bytes']==size)
   assert summary['planned']==summary['successful']==32 and summary['failed']==0
   for metric in ['setup_ms','echo_ms','total_ms']:
    values=sorted(row[metric] for row in selected);assert all(value>=0 for value in values)
    assert summary[metric+'_p50']==statistics.median(values) and summary[metric+'_p95']==values[math.ceil(32*.95)-1]
  paired=next(row for row in b['paired_differences'] if row['payload_bytes']==size);assert paired['complete_pairs']==32
  for metric in ['setup_ms','echo_ms','total_ms']:
   deltas=[]
   for pair in range(32):
    selected={row['path']:row for row in rows if row['pair']==pair and row['payload_bytes']==size}
    deltas.append(selected['private'][metric]-selected['standard'][metric])
   assert paired[metric+'_private_minus_standard_p50']==statistics.median(deltas)
 if inputs is None:inputs=r['inputs_sha256']
 else:assert inputs==r['inputs_sha256']
c=json.loads((p/'source-context.json').read_text());assert c['runtime_inputs_sha256']==inputs and c['checker_sha256']==hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()
assert inputs['/var/tmp/hm-private-udp-comparison-release-node-v1']=='0250dab6fd392c871aa65e041be5b604c690da43a4c296a81e267471b2d9ccc9'
assert 'release' in (p/'build.txt').read_text() and 'optimized' in (p/'build.txt').read_text()
a=json.loads((p/'analysis.json').read_text());assert a['scored_operations']==256 and a['cohorts']==2 and a['all_scored_success']
print('Two 51-check release KVM cohorts, 256/256 scored operations, 272 raw rows, independently recomputed medians/P95/paired differences and cleanup verified.')
