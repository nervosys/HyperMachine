# Private source connector mTLS verification

Final isolated sources pass all six connector tests and the full cluster suite: **105 passed, zero failed, one ignored**. Owned Redis ran both store contracts and was reaped with exit code zero. Redis ACL opt-in tests remain unset; this is not an ACL fault run.

Three new transport tests use an owned CA and real loopback TLS server. They check the private route path, cluster token, source node and membership generations, exact early upgrade bytes and an 8192-byte binary echo. Source rejoin, destination removal/movement and source lease revocation during upgrade refuse the stale connection and release the lease. Invalid server identity, anonymous TLS peer, wrong cluster token and wrong upgrade protocol are refused. Production client certificate verification remains enabled; only the anonymous negative fixture bypasses server verification.

`targeted-failed-v1.txt` is excluded evidence: the fixture incorrectly expected a path without `/tcp`. The corrected final targeted and full-suite logs are retained separately.

This verifies Rust connector transport against a host fixture. Daemon-local source lease integration, source guest gateway/DNS, cross-node KVM transport, timeout behavior and performance advantages remain unproved. No competitor benchmark or latency claim follows from this run.
