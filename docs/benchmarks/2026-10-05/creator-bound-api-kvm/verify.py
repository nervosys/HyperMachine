import hashlib,json,sys
from pathlib import Path
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent
for p,h in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/p).read_bytes()).hexdigest()==h,p
r=json.loads((root/'report.json').read_text())
assert r['success'] and len(r['cases'])==14 and all(c['success'] for c in r['cases'])
assert len({c['name'] for c in r['cases']})==14
assert any(c['name']=='creator-api-boundary-on-real-kvm' for c in r['cases'])
assert r['final_inventory_empty'] and r['owned_directory_removed'] and not r['cleanup_errors']
assert len(r['processes'])==7 and all(p['stopped'] for p in r['processes'])
assert r['inputs_unchanged'] and r['input_sha256']==r['input_sha256_after']
assert r['no_wake_checked_via_independent_node_mtls']
assert len(r['outage_denial_elapsed_ms'])==2 and all(0<=v<8000 for v in r['outage_denial_elapsed_ms'])
assert hashlib.sha256((root/'candidate-driver.py').read_bytes()).hexdigest()==r['input_sha256']['driver']
c=json.loads((root/'release-context.json').read_text())
for n in ['control','cli']:assert c['binary_sha256'][n]==r['input_sha256'][n]
f=json.loads((root/'initial-failure/report.json').read_text());assert not f['success'] and 'Permission denied' in f['error'] and not f['cases']
assert all(json.loads((root/'independent-check.json').read_text()).values())
print('Verified: 14 KVM cases, 7 stopped processes, guarded release hashes, retained initial failure')
