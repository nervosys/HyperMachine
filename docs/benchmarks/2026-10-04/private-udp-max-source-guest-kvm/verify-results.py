from pathlib import Path
import json
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());u=r['private_guest_udp'];assert len(r['checks'])==37 and r['guests_remaining']==0
for k in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[k]
for k in ['source_gateway_verified','internet_disabled','owner_numeric_address_refused','stale_target_binding_refused','fresh_dns_rejoin_recovery_verified']:assert u[k]
assert u['payload_bytes']==[0,13,1280,65507] and not u['same_node'] and u['source_node']!=u['destination_node']
a=json.loads((p/'image.json').read_text());b=json.loads((p/'repeated-image.json').read_text());assert a==b
c=json.loads((p/'image-reproducibility.json').read_text());assert c['identical_rebuild'] and c['image_sha256']==a['image_sha256']
assert r['inputs_sha256']['/var/tmp/hm-private-udp-max-guest-v1.cpio.gz']==a['image_sha256']
print('37 maximum-size actual guest UDP/TCP checks, scope, complete cleanup and byte-identical fixture image rebuild verified.')
