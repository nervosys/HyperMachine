# Automatic registration recovery: owned KVM verification

Four development-build profiles passed 30 checks each: refused Redis SET or XADD, IPv4 with two peers or IPv6 with eight peers. Each profile runs real initial creation and pause/resume with the opt-in one-second reconciliation worker, API TLS, node mTLS, trusted owner context and stable native TCP/UDP ports.

The fixture retains the Redis refusal through a worker retry, confirms the pending guest survives, then restores Redis. Recovery is observed through pending-entry disappearance without calling the reconciliation endpoint. Initial guest execution produces exact expected output; resumed guests retain their original access token and exchange exact TCP/UDP payloads on the reserved port. All input hashes match frozen binaries; all guests and fixture processes are cleaned up.

These are functional recovery checks, not competitor latency or throughput measurements. They do not prove daemon-crash recovery: pending markers are process-local. Five-second timeout behavior and more than 32 live pending guests remain covered indirectly by code and batch-selection tests rather than this runtime fixture.
