from pathlib import Path
import json,hashlib,statistics,math
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');a=json.loads(Path('/var/tmp/hm-mmio-irq-order-release-abba-analysis-v1.json').read_text())
assert a['order']==['baseline','candidate','candidate','baseline'] and a['scored_operations']==512 and a['concurrent_scored_datagrams']==51200
assert a['daemon_sha256']=={'baseline':'aef812594544d708d6c0101c27b3cdc0c93f6c3ded3e408f1eff9e253be955ef','candidate':'aab60303cf468e9266279e6d701a18df2638b262d50bf8f18ba44877a8072f0a'}
common=None
for i,kind in enumerate(a['order'],1):
 p=Path(f'/var/tmp/hm-mmio-irq-order-release-abba-{i}');r=json.loads((p/'report.json').read_text());assert len(r['checks'])==55 and r['guests_remaining']==0
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 daemon='/var/tmp/hm-'+('private-setup-overlap-release-node-v1' if kind=='baseline' else 'mmio-irq-order-release-node-v1');assert r['inputs_sha256'][daemon]==a['daemon_sha256'][kind]
 other={k:v for k,v in r['inputs_sha256'].items() if k!=daemon}
 if common is None:common=other
 else:assert common==other
 for key,h in r['inputs_sha256'].items():assert hashlib.sha256(Path(key).read_bytes()).hexdigest()==h,key
 c=json.loads((p/'private-capacity-comparison.json').read_text());assert c==a['cohorts'][i-1]['concurrent_comparison']==r['private_receiving_udp']['receiving_capacity']['concurrent_comparison']
 assert c['order']==['private','standard','standard','private'] and len(c['blocks'])==4
 for b in c['blocks']:
  assert b['all_success'] and len(b['rows'])==32 and all(row['success'] and row['completed']==100 for row in b['rows'])
  assert b['echoed_payload_bytes']==sum(row['echoed_payload_bytes'] for row in b['rows'])==70596704
  assert math.isclose(b['payload_mib_per_second'],b['echoed_payload_bytes']/1048576/b['elapsed_seconds_including_worker_start'],rel_tol=1e-12)
  for name,x in b['resources_before'].items():
   y=b['resources_after'][name];assert (x['pid'],x['start_ticks'])==(y['pid'],y['start_ticks']) and y['cpu_ticks']>=x['cpu_ticks']
   assert b['resource_deltas'][name]['cpu_ms']==(y['cpu_ticks']-x['cpu_ticks'])*1000/c['cpu_tick_hz']
print('Four successful matched release cohorts: exact shared fixtures, 220 KVM checks, 512 regression operations, 51200 matched datagrams and independently recomputed throughput/CPU confirmed.')
