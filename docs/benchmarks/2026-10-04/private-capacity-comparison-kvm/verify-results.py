from pathlib import Path
import json,hashlib,statistics,math
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());c=json.loads((p/'source-context.json').read_text())
assert len(r['checks'])==55 and r['guests_remaining']==0
for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
x=r['private_receiving_udp']['receiving_capacity'];assert x['configured_private_slots']==128 and x['additional_held_tunnels']==125 and x['extra_status']==x['refilled_extra_status']==503
assert x['existing_traffic_verified'] and x['standard_path_verified'] and x['recovered'] and x['recovery_attempts']==1 and 0<x['recovery_seconds']<3
b=json.loads((p/'benchmark.json').read_text());rows=[json.loads(line) for line in (p/'rows.jsonl').read_text().splitlines()]
assert b==r['private_udp_transport_benchmark'] and rows==b['rows'] and len(rows)==136 and b['all_scored_success'] and all(row['success'] for row in rows)
assert len([row for row in rows if not row['warmup']])==128
for s in b['summary']:
 selected=[row for row in rows if not row['warmup'] and row['path']==s['path'] and row['payload_bytes']==s['payload_bytes']];assert len(selected)==s['successful']==s['planned']==32 and s['failed']==0
 for metric in ['setup_ms','echo_ms','total_ms']:
  values=sorted(row[metric] for row in selected);assert s[metric+'_p50']==statistics.median(values) and s[metric+'_p95']==values[math.ceil(len(values)*.95)-1]
for paired in b['paired_differences']:
 assert paired['complete_pairs']==32
 for metric in ['setup_ms','echo_ms','total_ms']:
  deltas=[]
  for pair in range(32):
   selected={row['path']:row for row in rows if row['pair']==pair and row['payload_bytes']==paired['payload_bytes']};deltas.append(selected['private'][metric]-selected['standard'][metric])
  assert paired[metric+'_private_minus_standard_p50']==statistics.median(deltas)
assert c['runtime_inputs_sha256']==r['inputs_sha256'] and c['checker_sha256']==hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()
assert hashlib.sha256((p/'forwards.rs').read_bytes()).hexdigest()==c['permitted_root_isolated_sha256']['crates/hv2-sandboxd/src/forwards.rs']
assert r['inputs_sha256']['/var/tmp/hm-private-setup-overlap-release-node-v1']=='aef812594544d708d6c0101c27b3cdc0c93f6c3ded3e408f1eff9e253be955ef'
print('55 KVM checks, actual 125 additional private UDP tunnels, excess/refilled 503, existing/standard traffic, slot recovery, 128 scored operations and full cleanup verified.')
from pathlib import Path
import json,math
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());t=json.loads((p/'traffic.json').read_text())
assert t==r['private_receiving_udp']['receiving_capacity']['concurrent_traffic']
assert t['workers']==32 and t['roundtrips_per_worker']==100 and t['payload_bytes']==[64,1280,65507] and t['all_success']
assert len(t['rows'])==32 and sorted(row['worker'] for row in t['rows'])==list(range(32))
expected=sum([64,1280,65507][sequence%3] for sequence in range(100))
for row in t['rows']:assert row['success'] and row['planned']==row['completed']==100 and row['echoed_payload_bytes']==expected and row['elapsed_ms']>0
assert t['echoed_payload_bytes']==32*expected and t['elapsed_seconds_including_worker_start']>0
assert math.isclose(t['payload_mib_per_second'],t['echoed_payload_bytes']/(1024*1024)/t['elapsed_seconds_including_worker_start'],rel_tol=1e-12)
assert len(r['checks'])==55 and r['guests_remaining']==0
for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
print('32 workers, 3200 exact completed datagrams, independently recomputed byte totals/throughput, 55 KVM checks and full cleanup verified.')
from pathlib import Path
import json,math
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());a=json.loads((p/'comparison.json').read_text());assert a==r['private_receiving_udp']['receiving_capacity']['concurrent_comparison']
assert a['order']==['private','standard','standard','private'] and a['workers']==32 and a['roundtrips_per_worker']==100 and a['payload_bytes']==[64,1280,65507] and a['both_paths_held_during_all_blocks'] and len(a['blocks'])==4
expected=sum([64,1280,65507][sequence%3] for sequence in range(100))
def counters(text):
 lines=text.splitlines();udp=[line.split()[1:] for line in lines if line.startswith('Udp:')];assert len(udp)==2
 sockets=[line.split() for line in lines if len(line.split())>12 and line.split()[1].endswith(':46A2')];assert len(sockets)==1
 return dict(zip(udp[0],map(int,udp[1]))),int(sockets[0][-1]),sockets[0][9]
for index,b in enumerate(a['blocks'],1):
 assert b['block']==index and b['path']==a['order'][index-1] and b['all_success'] and len(b['rows'])==32
 assert sorted(row['worker'] for row in b['rows'])==list(range(32))
 for row in b['rows']:assert row['success'] and row['planned']==row['completed']==100 and row['echoed_payload_bytes']==expected and row['elapsed_ms']>0
 assert b['echoed_payload_bytes']==32*expected and b['elapsed_seconds_including_worker_start']>0
 assert math.isclose(b['payload_mib_per_second'],32*expected/(1024*1024)/b['elapsed_seconds_including_worker_start'],rel_tol=1e-12)
 before,bd,bi=counters(b['guest_udp_counters_before']);after,ad,ai=counters(b['guest_udp_counters_after']);assert bi==ai and bd==ad==0
 for key in ['RcvbufErrors','SndbufErrors','InErrors','InCsumErrors','MemErrors']:assert after[key]-before[key]==0
assert len(r['checks'])==55 and r['guests_remaining']==0
for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
print('Matched concurrent ABBA: 12800 exact scored datagrams, independently recomputed throughput, zero UDP error/drop deltas, 55 KVM checks and full cleanup verified.')
