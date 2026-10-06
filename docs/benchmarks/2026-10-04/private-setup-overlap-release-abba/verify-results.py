from pathlib import Path
import json,hashlib,statistics,math
p=Path(__file__).parent;a=json.loads((p/'analysis.json').read_text());assert a['order']==['baseline','candidate','candidate','baseline'] and a['scored_operations']==512 and a['all_scored_success']
assert a['daemon_sha256']=={'baseline':'8afb215a2ff476147b8ebf9ce96f25d161ae361876d16b32f9f72b97808c9fa3','candidate':'aef812594544d708d6c0101c27b3cdc0c93f6c3ded3e408f1eff9e253be955ef'}
common=None;runtime={}
for index,kind in enumerate(a['order'],1):
 r=json.loads((p/f'cohort-{index}-report.json').read_text());b=json.loads((p/f'cohort-{index}-benchmark.json').read_text())
 assert len(r['checks'])==51 and r['guests_remaining']==0 and r['private_udp_transport_benchmark']==b
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 assert b['host_build_profile']=='release' and b['all_scored_success'] and b['samples_per_path_payload']==32 and b['concurrency']==1
 rows=[json.loads(line) for line in (p/f'cohort-{index}-rows.jsonl').read_text().splitlines()];assert rows==b['rows'] and len(rows)==136 and all(row['success'] for row in rows)
 assert len([row for row in rows if not row['warmup']])==128
 for size in [64,65507]:
  for pair in range(-2,32):
   selected=sorted([row for row in rows if row['pair']==pair and row['payload_bytes']==size],key=lambda row:row['order'])
   assert [row['path'] for row in selected]==(['private','standard'] if pair%2==0 else ['standard','private'])
   assert all(row['warmup']==(pair<0) for row in selected)
  for path in ['private','standard']:
   selected=[row for row in rows if row['payload_bytes']==size and row['path']==path and not row['warmup']]
   summary=next(row for row in b['summary'] if row['payload_bytes']==size and row['path']==path)
   assert summary['successful']==summary['planned']==32 and summary['failed']==0
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
 u=r['private_guest_udp'];assert u['payload_bytes']==[0,13,1280,65507] and u['store_lookup_outage_verified'] and u['source_resume_max_payload_verified']
 for phase in ['source_pause','source_delete']:
  assert u[phase+'_active_target_socket_closed'] and u[phase+'_observed_target_sockets']==1 and 0<u[phase+'_probe_to_observation_seconds']<15
  assert not set(map(tuple,u[phase+'_target_socket_ids'])) & set(map(tuple,u[phase+'_target_sockets_after']))
 daemon='/var/tmp/hm-private-'+('route-live-snapshot-release-node-v1' if kind=='baseline' else 'setup-overlap-release-node-v1')
 assert r['inputs_sha256'][daemon]==a['daemon_sha256'][kind]
 other={key:value for key,value in r['inputs_sha256'].items() if key!=daemon}
 if common is None:common=other
 else:assert common==other
 if kind in runtime:assert runtime[kind]==r['inputs_sha256']
 else:runtime[kind]=r['inputs_sha256']
 assert a['cohorts'][index-1]['paired_differences']==b['paired_differences'] and a['cohorts'][index-1]['summary']==b['summary']
c=json.loads((p/'source-context.json').read_text());assert c['runtime_inputs_sha256_by_kind']==runtime and c['checker_sha256']==hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()==a['checker_sha256']
source={kind:json.loads((p/f'{kind}-source-context.json').read_text()) for kind in ['baseline','candidate']}
changed=[key for key,value in source['baseline']['permitted_root_isolated_sha256'].items() if source['candidate']['permitted_root_isolated_sha256'][key]!=value]
assert sorted(changed)==['crates/hv2-sandboxd/src/forwards.rs']
for kind in ['baseline','candidate']:
 for snapshot,key in [('forwards.rs','crates/hv2-sandboxd/src/forwards.rs')]:assert hashlib.sha256((p/f'{kind}-{snapshot}').read_bytes()).hexdigest()==source[kind]['permitted_root_isolated_sha256'][key]
assert 'release' in (p/'build.txt').read_text() and 'optimized' in (p/'build.txt').read_text()
print('Four 51-check release ABBA cohorts, 512/512 scored operations, 544 raw rows, independently recomputed timing statistics, exact source delta and full cleanup verified.')
