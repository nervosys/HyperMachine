# Owner sharing Redis AOF hard-restart gate

One explicitly selected Linux-only owned-server test passes five process-kill/restart cycles using Redis 8.0.2 with appendonly=yes, appendfsync=always, snapshots disabled and a private Unix socket (TCP port zero). Logs show six distinct processes and five AOF reloads. Active grants, exact u64 sandbox incarnation above JavaScript precision, revocation revisions, deletion, recreated-instance grants and owner changes survive as expected. Real async proxy admission admits current grants and denies revoked/missing/stale/former-owner grants. Corrupt JSON and wrong Redis types deny reads/admission/updates without silently altering those fault payloads.

The first run failed because a read on the old killed connection returned broken pipe. The test was corrected to bound read-only retries while its connection manager reconnects; no mutation is automatically replayed and no production code changed. Initial failed source/log are retained. Both new and reconnected store handles subsequently agree on persisted active state.

The final gate empties sandbox inventory, intentionally retains revision tombstones, and drops its owned process/directory. An independent driver checked final process absence and directory removal. The archive verifier checks hashes, exact test success, five phases, distinct process starts/AOF reloads, cleanup assertions and that the candidate adds only this test to the previously staged store source. Protected isolated hashes were checked before/after both runs; root protected implementations were not read or built.

This establishes the exact local AOF-always process-restart fixture. It does not verify hardware/host power loss, other Redis persistence settings or versions, managed replication/failover, shipped control-plane process restart, real KVM owner sharing, TLS during Redis outage or competitor performance. Existing local API/CLI/TLS evidence is separate. Retention cleanup, SSO, verified identities and tenant/team isolation remain open.

Reproduce on the guarded isolated source tree with the command in source-context.json. Verify this archive with `python verify.py`.
