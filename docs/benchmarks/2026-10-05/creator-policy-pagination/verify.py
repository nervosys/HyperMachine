from pathlib import Path
import hashlib,json
root=Path(__file__).resolve().parent
for p,h in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/p).read_bytes()).hexdigest()==h,p
log=(root/'integration.txt').read_text()
assert '32 passed; 0 failed' in log
assert 'test creator_bound_keys_filter_inventory_and_deny_cross_owner_routes ... ok' in log
print('Verified: 32 passing integration tests and immutable source/log hashes')
