# Daemon local source lease adapter

The local adapter binds a source lease to the caller's actual VM handle and registered incarnation. Acquisition checks pointer identity, running VM state, trusted nonempty owner, sandbox/node/start identity, expiry, pause and pending publication before entering source activity under the registry lock. Validation repeats these checks. Weak references avoid retaining the daemon or a removed VM; the activity guard stays owned by the lease until drop.

The full isolated daemon suite passes **63 tests, zero failures, two ignored KVM tests**. New ordinary tests exercise the record policy matrix and missing daemon state refusal with activity ownership/drop. These tests do not construct a live registry VM or call acquisition against KVM, so pointer/replacement and contention runtime behavior remain unverified. The adapter is compiled but not called by the guest gateway yet; its unused acquisition warning reflects that incomplete integration.

Stream validation uses registry `try_lock` to stay nonblocking. Contention fails closed and may terminate an otherwise valid connection; availability under concurrent lifecycle changes needs runtime verification before rollout. No private guest traffic or performance win is established.

The excluded v1 compilation used an incorrect test-only owner constructor (`new` instead of `parse`); v2 is the corrected passing source. The source catalog verifies 136 permitted root/isolate pairs and accepted isolated core hashes. Protected root core files were neither read nor built.
