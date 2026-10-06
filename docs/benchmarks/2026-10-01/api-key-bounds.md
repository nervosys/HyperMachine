# Bounded policy reload: Unix process verification

The debug control-plane binary built from 25215b1 passed all 20 real loopback HTTP checks on WSL Debian. A memory cluster store and synthetic credentials isolate the test. One owned process was stopped cleanly. No VM, TLS, distributed rotation or performance comparison is established.

The previous rotation checks are retained: valid replacement revokes the old key and activates the new one; scope remains enforced; malformed JSON, an empty set, admin-key collision and missing files preserve active policies; expired replacements do not open authentication; a valid replacement restores access. Added checks signal a reload of a 1 MiB-plus-one-byte file and invalid UTF-8. Both preserve active-key access and old-key revocation, and a later valid update succeeds. Startup uses the same bounded reader, but these additional process checks exercise reload, not startup rejection.

[Raw report](api-key-bounds-run-1.json), [manifest](api-key-bounds-manifest.json), [frozen coordinator](api-key-bounds-coordinator.py). The manifest includes the unchanged provisional core patch linked into the binary; no VM or boot candidate is exercised.

```sh
python3 docs/benchmarks/2026-10-01/api-key-bounds-coordinator.py \
  --control-plane /var/tmp/hm-api-key-bounds/control-plane \
  --output /var/tmp/hm-api-key-bounds/run-2.json
```

Verify archived evidence with `python tools/verify-api-key-bounds.py`. See [rotation behavior](../../API_KEY_ROTATION.md).
