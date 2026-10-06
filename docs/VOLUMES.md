# Persistent shared volumes

Volume management and content APIs are exposed by Linux sandbox nodes and
forwarded by the sandbox control plane (`hv2-cluster`). Management requires an
authorized API key; content requests use the volume bearer token. The control
plane chooses a live node by rendezvous hashing of the volume ID and forwards
the query and streaming body. All eligible nodes must share the intended volume
root if node membership changes are to preserve access. Direct cluster-node
access may also require the configured cluster credential.

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

## Metadata persistence and failed creation

New token-bearing metadata files use mode `0600`. Creation flushes metadata and the data directory before publishing, then flushes the volume directory and store root before returning success. Provision the store root durably: automatically created ancestor directories are not recursively flushed. Filesystem support and durability guarantees vary.

An error before publication cleans up the owned empty reservation. An error while flushing after publication preserves metadata and data; the API may return failure even though the volume is readable. Inspect the existing volume by name/ID. Retrying the same name returns a conflict and does not replace its token or data. Incomplete reservations still require operator review. [Flush evidence](benchmarks/2026-10-03/volume-metadata-sync/README.md) covers injected sync failures, private metadata permissions, concurrent creation across two daemons, and restart. Power loss, network-filesystem behavior, guest data flushes, and deletion durability remain unverified.

## Content upload completion

Successful content uploads flush buffered writes, apply requested ownership/mode, sync the file, and sync containing directories back to the volume data root, including nested parents created with `force=true`. Filesystem sync errors are returned to the caller. Replacement remains streamed in place: interrupted or failed uploads may leave a partial file, and concurrent writers require caller coordination.

[Content persistence evidence](benchmarks/2026-10-03/volume-content-sync/README.md) verifies nested binary upload, cross-node reads, replacement, wrong-token refusal and daemon restart on a shared local filesystem. Flush latency, simulated power loss, network-filesystem guarantees, guest 9P writes and deletion durability remain unverified.

## Atomic content replacement

Use `PUT /volumecontent/{id}/file?path=/your/file&atomic=true` to preserve the previous file until the entire replacement is written and synced. Combine with `force=true` to create missing parent directories. Default uploads continue streaming in place. Atomic mode publishes a new inode; already-open readers and hard links retain the old inode. The replacement uses mode `0644` and service ownership unless `mode`, `uid` or `gid` are supplied, rather than inheriting old-file metadata.

Before publication, ordinary request/write/metadata/sync errors remove the owned staging file and preserve previous bytes. After rename, a directory-sync error can return failure while the new file is visible. A hard kill can leave a private `.hm-upload-*` file; inspect it manually rather than deleting staging files while uploads are active. Aborted `force=true` requests can leave empty parent directories. Atomic uploads need disk space for the complete replacement; concurrent writers are last-publisher-wins and namespace changes require coordination.

[Atomic upload evidence](benchmarks/2026-10-03/volume-atomic-upload/README.md) verifies incomplete HTTP upload refusal, previous-byte preservation, staging cleanup, successful binary replacement, cross-node reads, restart, and unchanged default behavior. Power-loss and network-storage guarantees remain unverified.

## Upload performance evidence

The [paired upload fixture](benchmarks/2026-10-03/volume-upload-timing/README.md) measures sequential 1,048,586-byte host HTTP PUTs including flush completion, then verifies exact bytes on two local daemons outside the timed interval. Atomic uploads were slower in both pairs: 22.905 versus 19.660 ms and 15.083 versus 9.975 ms. Large cohort drift and a dev-profile binary prevent a stable production overhead estimate. Choose the upload mode for its replacement/inode semantics and measure your storage; these local results establish no competitor win or network-storage throughput guarantee.

## Streaming operator upload helper

Configure `HV2_VOLUME_TOKEN` in your environment using the volume's content token, then run from the repository root:

```sh
python3 tools/upload-volume-file.py \
  --endpoint https://your-node.example \
  --volume-id vol-your-volume \
  --path /workspace/input.bin \
  --source ./input.bin --force
```

Use `--ca-cert /path/to/node-ca.pem` for a private CA. Loopback HTTP is available for local nodes. The helper defaults to atomic replacement; `--in-place` selects the compatibility behavior. It streams 128 KiB chunks rather than buffering the entire source, permits regular files up to 4 GiB, and prints the returned file stat. Keep the source stable while uploading. It sends the volume bearer token, never the platform API key, and does not follow redirects. The helper can use a node or the sandbox control-plane endpoint; volume content routes rely on the volume bearer token. It remains a standalone alternative; the integrated `hm` upload command is documented below. E2B SDK-specific atomic options are not added here. [Client evidence](benchmarks/2026-10-03/volume-upload-client/README.md) verifies an exact binary roundtrip on two local nodes.

[Owned HTTPS client tests](benchmarks/2026-10-03/volume-client-tls/README.md) verify custom CA trust, certificate/hostname refusal, redirect refusal, query encoding and token handling, no platform API-key forwarding, empty uploads, invalid-input refusal, delayed-response timeout and wrong-size response refusal. These protocol fixtures complement the two-node storage check; public CA/proxy operation and successful maximum-size transfers remain unverified.

The helper's `--timeout` also bounds the connected transfer and response, including slowly arriving response bodies; expiry shuts down the connection and reports failure. Connection setup time is deducted from the remaining transfer budget. System DNS resolution and local file operations are outside a strict process-exit guarantee. A timeout may follow successful server publication, so inspect the destination before retrying. [Deadline evidence](benchmarks/2026-10-03/volume-client-deadline/README.md) includes the slow-response refusal test and repeated two-node upload verification.

[Control-plane route evidence](benchmarks/2026-10-03/control-volume-routing/README.md) verifies stable selection of one of two live protocol nodes, binary/query forwarding, management/API-key and content/bearer authentication, cluster credential replacement, and duplicate-list merging. This corrects an earlier node-only documentation finding. Later [real-daemon forwarding evidence](benchmarks/2026-10-03/control-real-volumes/README.md) verifies atomic binary upload, exact shared-storage reads/token access when either node is removed from eligibility, both daemon restarts, list merging and deletion. Redis fleet placement, abrupt in-flight failures and TLS/mTLS forwarding remain separate checks.

[Selected-node termination evidence](benchmarks/2026-10-03/control-volume-crash/README.md) verifies a process killed after partial atomic staging yields `502` through the control plane while the previous file remains exact on the live shared-storage peer. A private `0600` staging file remains after the kill. Controlled node eligibility changes, both daemon restarts and explicit owned-volume cleanup pass. Automatic orphan repair, post-rename kill, power loss and Redis heartbeat-driven failover remain unverified.

## Create-only uploads

Use `overwrite=false` on the file PUT, or `--no-clobber` with the upload helper, to refuse replacing an existing regular file. Default behavior still permits replacement. Atomic mode uses publication-time `RENAME_NOREPLACE`, so a concurrent creator cannot be overwritten after an earlier absence check. In-place mode uses exclusive file creation without truncation. Conflicts return `409`; unsupported filesystem operations fail without a replacement fallback.

[Create-only evidence](benchmarks/2026-10-03/volume-create-only-upload/README.md) verifies one winner and fifteen conflicts for concurrent uploads in each mode across two nodes, exact winner bytes and actual client refusal. This is exclusive creation, not compare-and-swap updates. In-place winners can leave partial new files if interrupted; atomic mode retains its staging-space and hard-kill recovery limits.

## Volume management with hm

Configure `HV2_API_KEY` for the management endpoint, then use the node or sandbox control plane:

```sh
hm sandbox vm --endpoint https://control.example volume create workspace
hm sandbox vm --endpoint https://control.example volume list
hm sandbox vm --endpoint https://control.example volume inspect vol-your-volume
hm sandbox vm --endpoint https://control.example volume delete vol-your-volume
```

Creation and inspection print JSON including the content bearer token. Successful deletion prints nothing and removes the volume contents. Names and IDs accept 1–64 ASCII letters, digits, underscores or hyphens. Existing `--api-ca-cert` and `--request-timeout` options apply. Management responses are bounded to 64 KiB and listings to 1 MiB. Use the integrated streamed upload command below, or the Python helper, for atomic/create-only uploads. [Management CLI evidence](benchmarks/2026-10-03/volume-management-cli/README.md) verifies shipped-binary protocol behavior, existing CLI regressions, and real shared-storage operations through the control-plane router.

## Streamed upload with hm

Configure `HV2_VOLUME_TOKEN` with the content bearer token, then run:

```sh
hm sandbox vm --endpoint https://control.example volume upload \
  vol-your-volume ./input.bin --path /workspace/input.bin --force --no-clobber
```

Atomic replacement is default. `--no-clobber` refuses existing destinations; `--in-place` selects streaming in-place behavior. The command streams a regular file up to 4 GiB, uses the volume token without loading or forwarding `HV2_API_KEY`, and verifies the returned size. HTTPS or loopback HTTP is required, and `--api-ca-cert` / `--request-timeout` apply. Keep the source stable during upload. [Integrated upload evidence](benchmarks/2026-10-03/volume-cli-stream-upload/README.md) includes exact binary reads through the control plane and create-only refusal on real storage.

## Streamed download with hm

With `HV2_VOLUME_TOKEN` configured, download into a new local file:

```sh
hm sandbox vm --endpoint https://control.example volume download \
  vol-your-volume ./download.bin --path /workspace/input.bin
```

The command streams up to 4 GiB, flushes/syncs a temporary file beside the destination, and publishes without replacing an existing name. Existing files or symlinks are refused; incomplete responses and ordinary write failures discard staging. It uses the same content-token, HTTPS/loopback, custom-CA and timeout rules as uploads. JSON reports the local path and byte count. A hard termination can leave local staging; parent-directory power-loss durability is not established. [Download evidence](benchmarks/2026-10-03/volume-cli-stream-download/README.md) verifies exact binary data, existing-file preservation, truncated-response cleanup and real control-plane storage roundtrip.

[Integrated CLI HTTPS evidence](benchmarks/2026-10-03/volume-cli-tls/README.md) verifies upload/download with a proper private CA and localhost leaf, exact binary data, bearer-only authorization, untrusted/wrong-hostname refusal before HTTP arrival, redirect refusal, slow-response timeout, and partial/advertised-oversized download cleanup. Shipped control-plane HTTPS/mTLS forwarding, public CA and successful maximum-size transfers remain separate checks.

Unix upload clients open sources nonblocking and then check the opened file type. A named pipe without a writer is refused before network contact, verified by the [source type tests](benchmarks/2026-10-03/volume-source-type/README.md). This does not bound filesystem or device stalls; keep regular-file sources stable during transfer.

The integrated CLI also passed byte-verified 64 MiB and 256 MiB uploads and downloads against an owned loopback fixture, with observed process memory below 16 MiB in all four runs. See the [streaming memory evidence](benchmarks/2026-10-03/volume-cli-streaming-memory/README.md) for sampling limits and raw reports. This verifies those transfer sizes, not the 4 GiB boundary or production storage throughput.

The CLI subsequently passed exact 4 GiB uploads and downloads against the owned HTTP fixture, and refuses an upload source one byte over the limit before network contact. [Boundary verification](benchmarks/2026-10-03/volume-cli-size-boundary/README.md) supersedes the earlier unverified CLI size-boundary qualification. Maximum-sized transfers through the real control plane and volume store remain unverified.

Browse volume contents with `hm sandbox vm volume ls VOLUME_ID --path / --depth 1`, or inspect a path with `hm sandbox vm volume stat VOLUME_ID --path /file`. Both require `HV2_VOLUME_TOKEN` and ignore `HV2_API_KEY`. Depth accepts 1–32; JSON responses are limited to 1 MiB and larger listings fail explicitly. [CLI browsing verification](benchmarks/2026-10-03/volume-cli-browse/README.md) covers the owned protocol fixture; real-route integration is pending.

The browsing commands subsequently passed through the real control router to each of two real shared-volume daemons, including wrong-token refusal. [Real-route browsing evidence](benchmarks/2026-10-03/volume-cli-browse-real/README.md) supersedes the protocol-only integration qualification above; Redis, service-process TLS and guest mounts remain outside this check.

Create a directory with `hm sandbox vm volume mkdir VOLUME_ID --path /nested/path --force`, using `HV2_VOLUME_TOKEN`. `--force` creates missing parents and accepts an existing directory; without it, either condition fails. [Real-route directory creation tests](benchmarks/2026-10-03/volume-cli-mkdir/README.md) verify these semantics. Directory power-loss durability is not established.

`mkdir --force` accepts only an existing directory. An existing regular file is refused and its bytes preserved, as verified by the [real-route file conflict regression](benchmarks/2026-10-03/volume-mkdir-file-refusal/README.md). Concurrent namespace changes remain the caller’s responsibility.

All three path commands (`ls`, `stat`, `mkdir`) also pass [owned HTTPS trust and hostname checks](benchmarks/2026-10-03/volume-path-cli-tls/README.md), with volume bearer authentication and no platform API-key forwarding. This complements real HTTP router/daemon integration; separately launched service-process HTTPS remains unverified.

Directory creation now syncs the created/updated directory and all containing directories through the data root before success, including parents made by `--force`. On a flush error, created entries may remain visible and the request fails. [Directory flush evidence](benchmarks/2026-10-03/volume-directory-sync/README.md) supersedes the earlier absence of mkdir flushing; power-loss and network-storage guarantees remain unverified.
