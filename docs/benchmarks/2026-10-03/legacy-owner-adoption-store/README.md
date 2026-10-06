# Atomic legacy owner adoption store foundation

MemoryStore and RedisStore gain adopt_sandbox_owner with a validated OwnerId. Only an existing ownerless VM without entries in its per-VM public-port index can be adopted. Same-owner replay returns AlreadyOwned; a different existing owner is refused. No owner transfer or reservation reassignment occurs. Unsupported stores fail closed through the trait default.

Memory uses the shared sandbox/port lock. Redis validates the typed record, preserves the full raw JSON including unknown fields, then atomically compares its original bytes, checks the per-VM port index, and writes once. Changed records retry (bounded at sixteen attempts); missing records and expiring sandbox records are refused. These rules rely on the existing agreement of the global/per-VM reservation indexes; repair of orphaned global reservations is outside this operation.

Both store contracts verify 32 concurrent distinct principals: exactly one adopts and the rest conflict. They verify unchanged other record fields, same-owner replay, different-owner refusal, missing VM handling and refusal when a legacy reservation exists. The explicit Redis fixture additionally checks a wrong-type per-VM index leaves the raw VM record unchanged, full raw JSON field preservation, and successful ownership/replay after an owned AOF restart. This is orderly local AOF persistence, not managed Redis durability.

All 87 library tests pass (the owned Redis fixture is ignored by default); the explicit owned Redis test passes separately. source-context.json records 133 permitted root/isolated source matches and accepted isolated core hashes. Protected modified root core is excluded. Exact store/owner source snapshots and terminal test logs are covered by manifest.json.

Reproduce in the isolated checkout:

```sh
CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test -p hv2-cluster --lib
CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test -p hv2-cluster --lib managed_ports_redis_owned_restart -- --ignored --nocapture
```

This is a foundation, not a usable administrator adoption feature. No API or CLI command is exposed. Callers must authorize an administrator and coordinate with the node transition lock: the node holds in-memory records used by pause/resume and fork, and put_sandbox can overwrite a store-only adoption with stale ownership. Node in-memory synchronization, administrator-only API/CLI wiring, live lifecycle/fork verification and credential rotation checks remain required. The comparison table retains legacy adoption as incomplete. No KVM production binaries were rebuilt for this foundation and no performance claim follows.
