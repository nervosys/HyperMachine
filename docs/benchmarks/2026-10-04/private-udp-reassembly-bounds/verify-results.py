from pathlib import Path
import json,hashlib
p=Path(__file__).parent;log=(p/'tests.txt').read_text()
assert '204 passed; 0 failed' in log
for name in ['private_udp_reordered_fragments_preserve_maximum_payload','private_udp_reassembly_capacity_refuses_incomplete_third_and_recovers','private_udp_expired_fragments_refuse_and_reassembly_slots_recover']:
 assert 'test gateway::tests::'+name+' ... ok' in log,name
context=json.loads((p/'source-context.json').read_text())
assert hashlib.sha256((p/'gateway-tests.rs').read_bytes()).hexdigest()==context['permitted_root_isolated_sha256']['crates/hv2-net/src/gateway/tests.rs']
assert 'runtime_inputs_sha256' not in context
print('204 networking tests, three reassembly gates and exact accepted test source verified; no new runtime/KVM claim.')
