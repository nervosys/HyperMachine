# Owner-scoped private network membership

The isolated cluster library suite passes 94 tests, with one existing ignored test. Five new tests cover validated tags and strict membership records, same-owner/tag authorization, cross-owner and cross-network refusal, stale generations after rejoining or replacing endpoints, authenticated-source mismatch, zero ports, uncertain publication, paused/expired/future-dated records, migration and changed ownership.

Command: `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test --locked -p hv2-cluster --lib`, run in `/var/tmp/hm-egress-log-mA2CCL`.

The catalog verifies 134 permitted root/isolate files and the three separately accepted isolated core sources. Protected root core sources were not read or built. Source copies and exact test output are retained here.

This is a domain model, not a shipped routing feature. There is no store transaction, private address allocator, DNS integration, gateway interception, authenticated private node route or live-stream revocation yet. Endpoint views must come from authoritative state; route claims are not credentials. Tests model destination identity replacement; they do not test actual address allocation or packet routing. No KVM or competitor performance claim follows from these tests.
