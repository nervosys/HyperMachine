from pathlib import Path
import json
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());g=r['private_guest_gateway']
assert len(r['checks'])==31 and r['guests_remaining']==0
for k in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[k]
for k in ['source_gateway_verified','guest_dns_verified','internet_disabled','source_membership_stream_revocation_verified','source_rejoin_stale_binding_refused','source_rejoin_fresh_binding_recovery_verified','long_lived_guest_revocation_verified']:assert g[k]
assert not g['same_node'] and g['source_node']!=g['destination_node']
assert 0<g['source_membership_revocation_elapsed_ms']<5000
s=(p/'checker.py').read_text();assert "launch.replace('live-probe','source-probe')" in s and "ready.get('stdout')=='source-probe'" in s
print('31-check actual guest source revocation/rejoin and complete cleanup evidence verified.')
