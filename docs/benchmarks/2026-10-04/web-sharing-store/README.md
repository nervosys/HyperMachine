# Browser sharing store prerequisite

Memory and Redis now implement owner-authorized reads, revision compare-and-swap and atomic authoritative sandbox/grant snapshots for future proxy admission. Exact retries succeed; stale expected revisions, changed contents at the same revision, owner conflicts and changed creation instances refuse writes. Redis fences both original record and grant bytes in its mutation script; it does not compare timestamps as Lua numbers.

Both implementations deliberately retain sharing rows after deletion. Missing records and mismatched creation instances deny admission. Reusing a sandbox ID requires reading the retained revision before an authorized replacement; deletion does not silently erase replay protection. Cleanup/retention policy remains open.

A fresh owned loopback Redis server ran the sharing contract, not a skipped Redis test. Seven targeted tests passed, then the full cluster library passed 127 tests with one ignored. The ACL-specific test reported skipped because its separate fixture variable was unset. Active grants were visible across independent connections. The test servers disabled disk persistence and were terminated with their temporary directories removed; this proves connection consistency, not restart/crash durability.

The full suite tested the archived tested-store.rs. The final candidate differs only in a corrected interface doc comment; exact difference was verified. Protected isolated hashes were checked before and after both builds. Raw initial/final test and Redis logs are retained.

Owner API/CLI, credential integration, proxy use, bounded store failures, restart durability and real guest/TLS gates remain open. No self-service feature completion or performance win is claimed.
