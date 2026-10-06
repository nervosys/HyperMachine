# Persistent shared volumes

Linux sandbox nodes expose persistent directories to guests using 9P over vsock.
Create a volume through `POST /volumes` with `{"name":"workspace"}`, then use
`"volumeMounts":[{"name":"workspace","path":"/workspace"}]` when creating a
sandbox. Volume names accept letters, digits, underscores and hyphens, up to 64
characters. The API returns the volume ID and its content access token.

Multiple guests can mount the same directory. Guest memory snapshots and forks
do not copy volume data; mounts remain shared storage. This is directory sharing,
not a standalone block device with exclusive attach/detach semantics. Deleting a
volume removes its files and makes it unavailable through the volume API.

The daemon selects storage in this order:

| Configuration | Volume metadata and data location |
|---|---|
| `--volume-dir /absolute/path` | The explicit directory |
| `--snapshot-store /absolute/path` without `--volume-dir` | `volumes/` within the snapshot store |
| Neither setting | Node-local transient storage |

Use a storage directory that survives the node process and is accessible to
every node that may run or resume a guest with that volume. Volume creation
reserves its deterministic directory before publishing metadata, so concurrent
duplicates cannot overwrite the winning access token. Existing incomplete
reservations conflict and need operator review; automatic crash repair is absent.

## Sandbox Helm chart

The chart can use an existing ReadWriteMany PVC independently of its snapshot
store. The PVC must exist in the release namespace and support the shared
filesystem operations the daemon uses.

```yaml
node:
  volumeStore:
    claimName: sandbox-volumes
  snapshotStore:
    claimName: sandbox-snapshots
```

`volumeStore.claimName` mounts at `/var/lib/hv2-volumes` and configures
`--volume-dir`. `snapshotStore.claimName` mounts separately at
`/var/lib/hv2-store`. Either can be configured alone. With only the snapshot
claim, the existing `volumes/` fallback remains. With neither claim, volumes do
not survive pod replacement.

A dedicated volume claim persists volume data, not guest memory, templates or
paused sandboxes. Those need the snapshot-store configuration. Changing either
claim or introducing a dedicated volume directory does not migrate existing
data. Plan storage migration before switching a deployment's paths, and include
the explicit volume directory in your existing backup/recovery process.

[Creation evidence](benchmarks/2026-10-03/volume-shared-create/README.md) verifies
concurrent creation across two standalone daemon processes sharing a local
directory, consistent access tokens, duplicate preservation and process restart.
Network-filesystem semantics, power-loss durability and an actual Kubernetes
storage rollout remain unverified. The chart renders and references existing
claims; it does not provision a storage service.
