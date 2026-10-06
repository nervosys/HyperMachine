from pathlib import Path
import re,json,statistics,hashlib,ast
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());a=json.loads((p/'analysis.json').read_text());b=json.loads((p/'benchmark.json').read_text())
assert len(r['checks'])==51 and r['guests_remaining']==0 and b==r['private_udp_transport_benchmark'] and b['all_scored_success']
for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
assert len(b['rows'])==136 and len([row for row in b['rows'] if not row['warmup']])==128
assert b['rows']==[json.loads(line) for line in (p/'rows.jsonl').read_text().splitlines()]
metrics=['preauthorization_us','authorization_before_us','guest_open_us','loopback_pair_us','authorization_after_us','setup_us'];rows=[]
for line in (p/'daemon.txt').read_text().splitlines():
 line=re.sub(r'\x1b\[[0-9;]*m','',line)
 if 'owned_private_setup_profile' not in line:continue
 f=dict(re.findall(r'(\w+)=([^\s]+)',line));m=re.fullmatch(r'udp-(\d+)-(-?\d+)-(private|standard)',f['profile_label']);assert m
 row={'label':f['profile_label'],'payload_bytes':int(m[1]),'pair':int(m[2]),'path':m[3],'warmup':int(m[2])<0,**{key:int(f[key]) for key in metrics}}
 assert f['udp']=='true' and f['private_route']==str(m[3]=='private').lower()
 row['authorization_total_us']=row['authorization_before_us']+row['authorization_after_us'];row['other_us']=row['setup_us']-sum(row[key] for key in metrics if key!='setup_us');assert row['other_us']>=0
 rows.append(row)
assert rows==a['rows'] and len(rows)==136 and len({row['label'] for row in rows})==136
key=lambda row:(row['payload_bytes'],row['pair'],row['path']);assert set(map(key,rows))==set(map(key,b['rows']))
for result in a['summary']:
 selected=[row for row in rows if row['payload_bytes']==result['payload_bytes'] and row['path']==result['path'] and not row['warmup']];assert len(selected)==result['samples']==32
 for metric,value in result['p50_us'].items():assert value==statistics.median(row[metric] for row in selected)
for result in a['paired_differences']:
 assert result['pairs']==32
 for metric,value in result['private_minus_standard_p50_us'].items():
  deltas=[]
  for pair in range(32):
   selected={row['path']:row for row in rows if row['payload_bytes']==result['payload_bytes'] and row['pair']==pair};deltas.append(selected['private'][metric]-selected['standard'][metric])
  assert value==statistics.median(deltas)
c=json.loads((p/'source-context.json').read_text());assert c['diagnostic_runtime_inputs_sha256']==r['inputs_sha256']
assert hashlib.sha256((p/'accepted-forwards.rs').read_bytes()).hexdigest()==c['permitted_root_isolated_sha256']['crates/hv2-sandboxd/src/forwards.rs']
assert hashlib.sha256((p/'diagnostic-forwards.rs').read_bytes()).hexdigest()==c['diagnostic_receiver_sha256']
assert hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()==c['diagnostic_checker_sha256']
assert r['inputs_sha256']['/var/tmp/hm-private-setup-profile-release-node-v1']=='a9416733652cd5db4d443eb708b786d0d5afa5b3ea03bf5657b11bd63bcfcf01'
print('51 diagnostic KVM checks, 128 scored operations, exact 136 tagged records, independently recomputed stage medians and cleanup verified; production timings not claimed.')
