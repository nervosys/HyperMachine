# Membership API store-failure recovery

Final isolated cluster suite: **98 passed, 0 failed, 1 existing ignored**. The new test serves the production protected membership GET/PUT routes over real loopback HTTP, with a test-only adapter delegating normal operations to MemoryStore.

Eight injected conditions return HTTP 503: membership read error/hang, VM-record read error/hang, pre-commit CAS error/hang, and post-commit error/hang. All four hang cases hit the production five-second operation deadline (recorded as 5,001 ms in this cohort). Pre-commit failures leave membership untouched; post-commit failures retain the exact committed revision.

Both committed outcomes recover by retrying the identical request and revision. Reusing the revision with changed tags is refused, as is substituting a new revision while retaining the stale expected revision. The final persisted state equals the originally committed state. All four timed-out store futures are dropped, counted by test guards; the HTTP server task is aborted and awaited after the fixture.

The generic injected internal error detail is absent from every returned error body. The adapter deliberately hangs after a real in-memory commit; this models commit/response uncertainty. It does not simulate actual Redis transport interruption, process crash, fleet durability or guest connectivity. Normal real Redis HTTP behavior is retained in the earlier private-network-membership-http archive.

Command: `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test --locked -p hv2-cluster --lib -- --nocapture` in `/var/tmp/hm-egress-log-mA2CCL`. The 134-file permitted root/isolate catalog and separately accepted isolated core hashes are verified. Protected root core files were not read or built.

The first test-adapter compile failed because generated forwarding methods crossed default-method bodies; those test-only methods were corrected before runtime verification. Its output is retained as excluded evidence and proves no runtime behavior. The final suite output also includes an expected owned panic from the existing UDP failure-recovery test, which passes.

Private network address allocation, DNS, guest forwarding and existing-stream revocation remain unfinished. This is recovery evidence, not a performance benchmark or competitor win.
