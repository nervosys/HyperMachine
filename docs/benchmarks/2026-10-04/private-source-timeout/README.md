# Private connector setup deadline

An owned real loopback mTLS server accepts and validates the authenticated private request, then sends no upgrade response. The production connector returns `TimedOut` at its 15-second total setup deadline. The fixture verifies the source lease is dropped exactly once and the server observes the closed TLS connection. All seven connector tests pass.

The complete isolated cluster suite passes **106 tests, zero failures, one ignored**, with owned Redis enabled for both store contracts. Redis is reaped with exit code zero. ACL fault opt-in tests remain unset. The 15-second test is functional deadline verification, not a performance benchmark or an SLA.

Only the permitted connector test source changed. The catalog verifies 135 root/isolate source pairs and the accepted isolated core hashes. Protected root core source was neither read nor built. Daemon-local source lease adapter, guest gateway/DNS and cross-node KVM integration remain unfinished. The full feature and performance goal remains incomplete.
