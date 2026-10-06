# Atomic private membership revisions

Final isolated cluster library tests: **95 passed, 0 failed, 1 existing ignored**. The real Redis store contract separately passed against an ephemeral owned Redis server; it was not a skipped environment-dependent test. The fixture verifies clean Redis termination and retains its exact driver and output.

Both MemoryStore and RedisStore enforce trusted owner identity, current node and VM start timestamp, expected membership revision and exact replay. Concurrent replacements have one winner. Removing tags stores a fresh revision tombstone; replaying an earlier absent-row creation or stale update is refused. Migration requires a newly bound membership. VM deletion denies access and writes; deliberate ID reuse cannot replay the prior incarnation. Nested serialized membership identities/generations are validated.

Redis reads record and membership together, validates typed records in Rust, then atomically compares exact bytes before its single SET. This avoids Lua floating-point timestamp comparisons and prevents the pre-read from authorizing a concurrent changed record or membership. MemoryStore performs the equivalent checks under its sandbox-state lock.

Commands: `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test --locked -p hv2-cluster --lib`; then the retained Redis driver runs `store::tests::redis_store_keeps_the_contract` with `HV2_TEST_REDIS` pointing only to its owned server. All builds use `/var/tmp/hm-egress-log-mA2CCL`; the 134-file permitted-source catalog and separately accepted isolated core hashes are verified. Protected root core sources were not read or built.

Limits: membership storage is not integrated into create/update APIs, guest DNS or gateway routing. Retained removal revisions survive sandbox deletion intentionally; a bounded garbage-collection and identity-retirement policy remains to be designed. Existing streams are not revoked by this model. No KVM, process-crash durability, managed Redis guarantee, private network throughput or competitor performance is established here.
