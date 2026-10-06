from pathlib import Path
import json,hashlib
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
c=json.loads((root/'source-context.json').read_text())
assert hashlib.sha256((root/'proxy-before.rs').read_bytes()).hexdigest()==c['before_sha256']
assert hashlib.sha256((root/'proxy-candidate.rs').read_bytes()).hexdigest()==c['candidate_sha256']
api=(root/'api-tests.txt').read_text();control=(root/'control-tests.txt').read_text()
assert '1070 passed; 0 failed; 0 ignored' in api and 'asynchronous_admission_finishes_before_any_backend_open ... ok' in api
assert '31 passed; 0 failed; 0 ignored' in control and 'private_guest_urls_authenticate_before_open_and_strip_credentials_over_http1_and_http2 ... ok' in control
print('Verified source identities and complete API/control test logs; admission prerequisites only.')
