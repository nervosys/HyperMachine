# Automatic named guest registration recovery

Two owned Redis/API-TLS/node-mTLS/KVM profiles passed 32 checks each: SET refusal with IPv4/two peers and XADD refusal with IPv6/eight peers. Named initial creation goes through the real authenticated control API, which issues the operation reservation and trusted node context. The automatic worker runs at a one-second interval.

SET refusal preserves one running guest while the name remains pending and the sandbox record is absent. XADD refusal preserves one guest with an already committed record and name binding. In both cases, a duplicate named control-API create returns 409 and local inventory gains no second guest. Administrator pending discovery reports kind named without reservation capabilities.

After Redis is restored, automatic reconciliation clears pending state without a reconciliation request. The original reservation token and sandbox identity remain unchanged, trusted owner and name metadata match, and exact guest command output is verified. Deletion releases the reservation; a fresh API create reuses the name for a different guest and cleanup releases it again. The existing unnamed resume recovery and stable native TCP/UDP checks also pass. Input hashes match frozen binaries; all guests and fixture processes are cleaned up.

Named reservations deliberately have no automatic expiry; this is established by the store/model code and these tests do not introduce a TTL. Local pending markers remain process-local. Daemon-crash recovery, fleet recovery, cancellation during VM bring-up and managed competitor performance remain unverified. These are functional checks, not latency or throughput measurements.

Reproduce using the archived driver, frozen hashes, owned kernel/image and a fresh output directory. The node/control/CLI/gateway are the previously verified development binaries; no production Rust source changed in this step.
