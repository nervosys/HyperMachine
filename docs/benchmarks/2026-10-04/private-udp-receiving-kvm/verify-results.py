from pathlib import Path
import json
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());u=r['private_receiving_udp']
assert len(r['checks'])==34 and r['guests_remaining']==0
for k in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[k]
assert u['payload_bytes']==[0,13,65507] and u['oversized_frame_closed'] and u['source_rejoin_stream_closed']
assert not u['same_node'] and not u['source_gateway_verified']
assert {x['check']:x['status'] for x in u['refusals']}=={'wrong-upgrade':400,'wrong-cluster-token':401,'wrong-source-node':403,'duplicate-context':400,'cross-owner':403,'stale-source-generation':403}
assert '64 passed; 0 failed; 2 ignored' in (p/'tests.txt').read_text()
print('Private receiving UDP exact payload, refusal, revocation, test totals and cleanup evidence verified.')
