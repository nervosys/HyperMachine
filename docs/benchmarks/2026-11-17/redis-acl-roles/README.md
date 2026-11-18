# Per-role Redis ACL (security finding O12), on KVM

Functional verification, not a benchmark. `tools/check-redis-acl-roles.py` starts an owned Redis 8.0.2 with the
ACL that `tools/redis-acl.py` generates (default user off), one `hv2-sandboxd` node as `hv2-node` and
`hv2-control-plane` as `hv2-control-plane`, over verified TLS and node mTLS, in WSL2 with KVM
(`Detected hypervisor platform: Kvm` twice, `Tcg` never). 35 of 35 checks pass; `report.json` has each reply.

| Check | Result |
|---|---|
| Node registers; the control plane lists it live | pass |
| A managed key creates a sandbox that boots on the node, runs `echo`, and the sandbox is deleted | pass |
| Control plane revokes the key; the key is then 401 | pass |
| As `hv2-node`: `HDEL` tombstone, `DEL` tombstone hash, `HSET` the old key record, the same two inside `EVAL`, and even `HGET`/`HGETALL` of the key and tombstone hashes | each `NOPERM No permissions to access a key` |
| As `hv2-node`: `FLUSHALL`, `KEYS *` | `NOPERM ... no permissions to run the 'flushall' / 'keys' command` |
| After those attempts the tombstone still exists and the revoked key is still | 401 |
| As `hv2-node`: writing its own family (`environments`) | allowed |
| As `hv2-gateway`: `HDEL`/`DEL`/`HSET` on the key keyspace; `SADD nodes`; `HGETALL environments`; a writing `EVAL` | refused; `SMEMBERS nodes` is allowed |
| CONTROL, legacy shared user (`~hv2:<ns>:*  +@all`), second revoked key: `HDEL` tombstone then `HSET` old record | both return 1; the revoked key answers **200** again |
| No `NOPERM` in the node or control-plane log; no password or key in any log | pass |

The control row is the point: with one shared Redis user the revocation can be undone from a node's credentials;
with the role ACLs the same commands are refused at Redis.

An earlier run of this same script failed usefully: the first ACL lacked `+@transaction`, and with `MULTI`
refused the node *panicked* in redis-rs 1.7.1 (`multiplexed_connection.rs:246`, "attempt to subtract with
overflow") instead of returning an error. The node registers with an atomic pipeline. Do not narrow the command
list in `tools/redis-acl.py` without rerunning this check.

`users.acl` is the generated file plus the harness's own admin and legacy users, with the password hashes
replaced by a placeholder (`report.json` likewise). Inputs are hashed in `report.json`.

## NOT VERIFIED

- **`hv2-native-gateway` was not run against the ACL.** Its `gateway` role was derived from the code (it calls
  `sandbox`, `node`, `port_allocations`, which read `sandbox:*`, `nodes`/`node:*`, `ports:*`/`public-ports`) and
  checked only with `redis-cli`, not with the process. If the gateway needs another key, it fails with NOPERM; it
  does not widen anything.
- Only the paths this run exercised were checked as a *node*: register, heartbeat, create, exec, delete. Named
  sandboxes, domains, public ports, private networks, web sharing, webhooks and usage were not run under the node
  user. The table in `tools/redis-acl.py` covers them by reading `store.rs` (`--check-source` confirms all 24 key
  families are named), and the Lua scripts build no key names of their own, but that is a reading, not a run.
- The control-plane role was run for keys, nodes and sandboxes only; team telemetry, webhooks and domain routes
  were not.
- Redis 8.0.2 only. The generated ACL needs 7.0+ (`%R~`, `EVAL_RO`); not tried on Valkey or Redis 6.
- A compromised node can still write its own families (sandboxes, names, ports, events) and delete them. Only
  the key and tombstone keyspace (and team telemetry) is taken away.
- The compose and Helm deployments in `deploy/` still use one `requirepass`; they were not changed.
- Keys revoked before the tombstone existed have no revocation record; ACLs do not help those.
