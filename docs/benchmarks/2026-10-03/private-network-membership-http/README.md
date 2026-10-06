# Owner-scoped private membership HTTP API

The isolated cluster suite passes **97 tests**, with one existing ignored test. Two new real HTTP tests run the membership contract against MemoryStore and RedisStore. The Redis test was separately executed against an ephemeral owned server; its output contains no skipped fixture, and the driver verifies clean process termination.

The protected GET/PUT route is `/sandboxes/{id}/private-networks`. Tests verify unauthorized credentials, another owner despite a forged owner header, observer/inventory scope refusal, missing creator identity including legacy admin, ownerless and missing VMs, minimal GET response, exact replay after key rotation, strict JSON, malformed revisions, duplicate/oversized tag lists, missing tags, invalid tags, owner injection and the 4 KiB request limit. Competing replacements have one winner; stale create replay after removal is refused. Deleted VMs cannot be modified.

Stable caller-selected canonical UUID v4 revisions identify operations and enable exact payload retries; they do not authorize traffic. The store still atomically checks trusted owner and VM incarnation. Each store operation has a five-second deadline. Error/timeout branches are implemented but not fault-injected by these HTTP tests.

The retained driver runs the exact Redis HTTP test on final isolated sources. The 134 permitted-source hashes and separately accepted isolated core hashes are verified. Protected root core sources were neither read nor built.

This API configures membership only. No guest DNS, private address allocation, forwarding, fork/create integration, existing-stream revocation or private-network performance is established. It does not clear a pending registration or authorize a private route. VM-to-VM networks remain absent in the feature comparison.
