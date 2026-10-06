# Atomic private-route endpoint snapshots

The final isolated cluster suite, with owned Redis enabled, reports **98 passed, 0 failed, 1 existing ignored**. Both MemoryStore and real Redis run the paired-route contract; its completion marker occurs twice. The normal Redis membership HTTP test also runs. Separate opt-in Redis ACL tests remain skipped because their dedicated environment variables are unset.

MemoryStore reads both VM records and both membership states under one sandbox-state lock. Redis returns all four records in one Lua execution and validates the decoded typed records in Rust. Missing records, removed memberships, owner mismatches and node/start-timestamp mismatches cannot produce a route snapshot.

Claims bind source and destination membership generations, the shared network tag and guest port. Authorization against a freshly loaded snapshot rejects a forged authenticated source ID, pending source or destination registration, port zero, cross-network membership, stale generations after rejoining, paused or expired VMs and stale migration claims. A migration requires membership rebinding before another snapshot is available. Membership removal and VM deletion yield no snapshot. The retained shared-store contract exercises each of these checks against both backends.

The snapshot is not a credential and is not serialized. Callers must obtain a fresh atomic snapshot at the destination, authenticate the source separately, inspect actual local pending-registration state and require live nodes before opening a guest port. An old in-memory snapshot does not revoke itself, and existing-stream revocation is not implemented here.

The retained driver starts an ephemeral owned Redis server, runs `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test --locked -p hv2-cluster --lib -- --nocapture` in `/var/tmp/hm-egress-log-mA2CCL`, and verifies server termination. All 134 permitted root/isolate source hashes and the separately accepted isolated core hashes match. Protected root core sources were not read or built.

Guest tunnel integration, private DNS, address allocation and guest-to-guest KVM traffic remain unfinished. No competitor or private-network performance win is established by this store-level evidence.
