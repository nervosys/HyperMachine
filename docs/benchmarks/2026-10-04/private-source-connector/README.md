# Source-bound private TCP connector

Final isolated cluster suite with owned Redis: **102 passed, 0 failed, 1 existing ignored**. Three new connector tests cover origin validation, source lease/stream behavior and current source incarnation binding. Optional Redis ACL fixtures remain skipped; normal Redis membership/route contracts and the membership HTTP test run against the owned server. The driver verifies Redis termination.

The connector is bound to a trusted source VM record. Every open requires a source lifecycle/activity lease; no production no-op lease is supplied. Lookup reads current paired memberships, checks the fixed source owner/node/start timestamp, requires live registered nodes and builds only an HTTPS destination origin. Credentials, paths, queries and fragments in stored node origins are refused. The mTLS client sends fixed source-node and validated route context on a bodyless HTTP/1.1 private upgrade, with a fifteen-second total setup deadline and no redirects. Claim, node and URL are looked up again after upgrade; changed state prevents returning the stream.

Stream reads, writes and flushes check the source lease. A failed check drops the underlying stream and lease immediately, permanently refusing further data even if the test lease later becomes active again. Local shutdown remains allowed. Tests verify exact in-memory bytes, downstream EOF, immediate lease release and non-revival, plus current source replacement refusal and unsafe destination origin refusal. Compilation verifies that setup futures are Send-compatible.

Transport tests here use an in-memory duplex stream and target lookup. They do not execute this connector's mTLS HTTP upgrade, post-upgrade race branch, setup timeout or real VM-local lease adapter. The earlier KVM receiving-path fixture uses a Python host client and does not prove Rust connector or source guest gateway integration.

The retained driver runs `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test --locked -p hv2-cluster --lib -- --nocapture` in `/var/tmp/hm-egress-log-mA2CCL`. All 135 permitted root/isolate files and separately accepted isolated core hashes match. Protected root core sources were not read or built.

Guest gateway/DNS/address integration, actual local lease implementation and connector mTLS/KVM traffic remain unfinished. No guest-facing tag-network completion or competitor performance advantage is established.
