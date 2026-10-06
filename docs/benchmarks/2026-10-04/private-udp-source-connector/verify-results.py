from pathlib import Path
import json
p=Path(__file__).parent;s=(p/'tests.txt').read_text()
assert '119 passed; 0 failed; 1 ignored' in s
for name in ['bound_private_udp_preserves_framed_early_and_binary_bytes','bound_private_udp_refuses_setup_generation_placement_and_lease_races','bound_private_udp_refuses_tcp_upgrade_bad_token_and_anonymous_tls','stale_address_bindings_are_refused_without_contacting_destination']:assert 'private_node::tests::'+name+' ... ok' in s
for name in ['private_route_contract completed','private_address_ledger_contract completed','private_source_router_contract completed']:assert s.count(name)==2
assert 'skipped: set HV2_TEST_REDIS' not in s
c=json.loads((p/'cleanup.json').read_text());assert c['owned_redis'] and c['process_reaped'] and c['redis_exit_code']==c['test_exit_code']==0
print('119 passing tests, new UDP proofs, both store/router contract markers and Redis cleanup verified.')
