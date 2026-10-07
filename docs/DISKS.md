# Block disks

A disk is a persistent ext4 block device that one sandbox holds at a time. The
guest sees it as `/dev/vda`, mounted at the path the sandbox asked for. When that
sandbox ends, the disk is free, and the next sandbox that asks for it gets the
same files. This is the counterpart to [volumes](VOLUMES.md). A volume is a
directory that many guests share live over 9P. A disk is exclusive and is served
at block-device speed.

Disks are served by Linux sandbox nodes (`hv2-sandboxd`). The control plane does
not route them yet; see [Limits](#limits).

## Using one

```sh
# On the node: a 1 GiB disk, formatted on creation
curl -s -X POST localhost:3980/disks -d '{"name":"workspace","sizeMiB":1024}'

# A sandbox holding it, mounted at /data
curl -s -X POST localhost:3980/sandboxes \
  -d '{"timeout":600,"diskMount":{"name":"workspace","path":"/data"}}'

# Which sandbox holds which disk
curl -s localhost:3980/disks
```

With `hm`: `hm vm disk create workspace --size-mib 1024`, `hm vm disk list`,
`hm vm disk inspect <diskID>`, `hm vm disk delete <diskID>`, and
`hm vm create --disk workspace:/data`.

| Route | What it does |
|---|---|
| `POST /disks` | `{"name","sizeMiB"}`. Creates a sparse image of 16 MiB to 1 TiB and formats it ext4 on the host with `mkfs.ext4`, which the node must have (e2fsprogs). The response is 201 with `diskID`, or 409 if the name is taken. |
| `GET /disks`, `GET /disks/{diskID}` | Each disk with `diskID`, `name`, `sizeMiB` and `attachedTo` (the sandbox holding it, or `null`). |
| `DELETE /disks/{diskID}` | Deletes the disk and its image. Refused with 409 while a sandbox holds it. |
| `"diskMount": {"name","path"}` on `POST /sandboxes` | Attaches the disk and mounts it at `path`. The response is 404 if no disk has that name, and 409 if another sandbox holds it. |

The ID is derived from the name (`disk-…`), as a volume's is.

## What holds a disk

Each disk is a directory `<disk-dir>/<diskID>/` holding `disk.img` and
`meta.json`. `attachedTo` is written into `meta.json` with an atomic rename
under a lock, so two sandboxes asking for one disk at once cannot both be told
yes. A claim counts only while the daemon that made it is running. If the daemon
dies, its VMs die with it, and a restarted daemon treats their claims as free
rather than leaving the disk held forever.

That is why a disk directory belongs to one node. It is `--disk-dir`, or
`<temp>/hv2-sandboxd-disks` by default, and never the shared snapshot store:
two daemons sharing one directory would each treat the other's claims as stale.

A guest's writes go to the image on the vCPU thread that issued them, before the
guest sees them complete. A guest flush is an `fsync` of the image. Write-back
caching, discard and write-zeroes are not offered.

## What a sandbox with a disk cannot do

- **Restore from a template.** virtio-mmio has no hot-plug, and a template's
  guest booted with no disk has no driver bound to one. A sandbox with a disk
  cold-boots instead, which is slower to create than a template restore. Sandboxes
  without a disk are unaffected.
- **Pause, fork, snapshot or checkpoint.** Each would restore the guest's memory
  (its page cache and ext4 journal state) against a disk that may have changed
  since. The routes answer 409, and `autoPause` with a `diskMount` is refused
  with 400 before anything boots.
- **Start from a snapshot.** This is refused with 400 for the same reason.

## Limits

- **Node-local.** The control plane neither forwards `/disks` nor places a
  sandbox on the node that holds its disk. Use the node API directly. The
  control plane's rendezvous placement suits volumes on shared storage. It is
  wrong for disks, whose image is on one node, and is not reused for them.
- **Moving between nodes** is not supported. A disk moves between sandboxes on
  the node that holds it.
- **Raw images only.** The image is not copy-on-write and has no snapshots of
  its own.
- **Power-loss durability** has not been tested beyond `fsync` on a guest flush.

Evidence: [real KVM move between sandboxes, and daemon restart](benchmarks/2026-10-06/block-disk-kvm/README.md).
