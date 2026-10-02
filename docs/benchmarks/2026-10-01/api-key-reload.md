# API key rotation: Unix process verification

A debug control-plane binary built from 0c90d03 passed all 16 checks through real loopback HTTP. The fixture used a memory cluster store and synthetic credentials; it created and stopped one owned process. No VM, TLS, concurrent replica reload or performance comparison is established.

The fixture atomically replaced the configured policy file, signalled the running process with SIGHUP and waited for each reload acknowledgement. It verified old-key revocation, new-key access and continued scope enforcement. Malformed JSON, empty policies, legacy-admin collision and a missing file each preserved the active policy set. An expired replacement denied scoped access while keeping authentication required. A subsequent valid reload recovered access and revoked the prior key. Legacy admin access remained unchanged.

[Raw report](api-key-reload-run-1.json), [manifest](api-key-reload-manifest.json), [frozen coordinator](api-key-reload-coordinator.py). The manifest records the unchanged provisional core patch linked into the build; the test does not execute a VM or adopt that boot candidate.

```sh
python3 docs/benchmarks/2026-10-01/api-key-reload-coordinator.py \
  --control-plane /var/tmp/hm-api-key-reload/control-plane \
  --output /var/tmp/hm-api-key-reload/run-2.json
```

Verify archived hashes and outcomes with `python tools/verify-api-key-reload.py`. See [operator behavior and limitations](../../API_KEY_ROTATION.md).
