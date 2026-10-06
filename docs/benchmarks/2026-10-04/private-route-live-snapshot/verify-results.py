from pathlib import Path
import json,hashlib
p=Path(__file__).parent;c=json.loads((p/'source-context.json').read_text());log=(p/'cluster-tests.txt').read_text()
assert '120 passed; 0 failed; 1 ignored' in log and '64 passed; 0 failed; 2 ignored' in (p/'daemon-tests.txt').read_text()
for marker in ['private_route_contract completed','private_route_live_snapshot_contract completed','private_address_ledger_contract completed','private_source_router_contract completed']:assert log.count(marker)==2,marker
assert 'skipped: set HV2_TEST_REDIS' not in log
r=json.loads((p/'cleanup.json').read_text());assert r['owned_redis'] and r['process_reaped'] and r['redis_exit_code']==0 and r['test_exit_code']==0
for snapshot,source in [('store.rs','crates/hv2-cluster/src/store.rs'),('forwards.rs','crates/hv2-sandboxd/src/forwards.rs')]:assert hashlib.sha256((p/snapshot).read_bytes()).hexdigest()==c['permitted_root_isolated_sha256'][source]
assert 'runtime_inputs_sha256' not in c
print('120 cluster and 64 daemon tests, both live-node contracts, exact source snapshots and owned Redis cleanup verified; candidate timing/KVM remains unmeasured.')
