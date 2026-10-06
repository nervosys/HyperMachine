from pathlib import Path
import json
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());u=r['private_guest_udp'];assert len(r['checks'])==43 and r['guests_remaining']==0
for k in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[k]
for k in ['source_gateway_verified','internet_disabled','owner_numeric_address_refused','stale_target_binding_refused','fresh_dns_rejoin_recovery_verified']:assert u[k]
assert u['payload_bytes']==[0,13,1280,65507] and not u['same_node'] and u['source_node']!=u['destination_node']
a=json.loads((p/'image.json').read_text());b=json.loads((p/'repeated-image.json').read_text());assert a==b
c=json.loads((p/'image-reproducibility.json').read_text());assert c['identical_rebuild'] and c['image_sha256']==a['image_sha256']
assert r['inputs_sha256']['/var/tmp/hm-private-udp-pause-guest-v1.cpio.gz']==a['image_sha256']


assert u['revocation_grace_seconds']==3 and u['post_grace_attempts']==3
for phase in ['target_membership_active_udp_revocation','source_membership_active_udp_revocation']:
 v=u[phase];assert v['revoked'] and v['same_socket'] and v['replies_before']>=1 and v['post_grace_refusals']==3
assert u['source_rejoin_stale_binding_refused'] and u['source_rejoin_fresh_binding_recovery_verified']
n=json.loads((p/'negative-control.json').read_text());assert n['ready'] and n['marker_created_with_echo_still_active'] and n['exit_code']==5 and not n['false_revocation_report'] and n['client_reaped']

v=u['target_pause_active_udp_revocation'];assert v['revoked'] and v['same_socket'] and v['replies_before']>=1 and v['post_grace_refusals']==3
for key in ['target_pause_numeric_refused','target_pause_dns_refused','target_resume_max_payload_recovery_verified','target_resume_address_preserved']:assert u[key]
print('43 KVM checks, active target-pause UDP refusal, paused DNS/numeric refusal, maximum-size resume recovery, cleanup, identical rebuild and negative control verified.')
