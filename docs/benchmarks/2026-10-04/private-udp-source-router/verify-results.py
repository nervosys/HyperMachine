from pathlib import Path
import json
p=Path(__file__).parent;s=(p/'tests.txt').read_text();assert '120 passed; 0 failed; 1 ignored' in s
for name in ['private_udp_router_commits_dns_binding_and_dispatches_framed_transport','bound_private_udp_preserves_framed_early_and_binary_bytes','bound_private_udp_refuses_setup_generation_placement_and_lease_races','stale_address_bindings_are_refused_without_contacting_destination']:assert 'private_node::tests::'+name+' ... ok' in s
for name in ['private_route_contract completed','private_address_ledger_contract completed','private_source_router_contract completed']:assert s.count(name)==2
assert 'skipped: set HV2_TEST_REDIS' not in s
c=json.loads((p/'cleanup.json').read_text());assert c['owned_redis'] and c['process_reaped'] and c['redis_exit_code']==c['test_exit_code']==0
print('120 tests, DNS-to-UDP transport proof, both store/router markers and Redis cleanup verified.')
