# Checkpoints

A checkpoint is a running sandbox's state, saved under a name, to roll the
sandbox back to later. A sandbox's filesystem is in its memory, so a
checkpoint is its memory: its files and its processes together.

```sh
curl -s -X POST localhost:3980/sandboxes/$SBX/checkpoints \
  -H 'content-type: application/json' -d '{"name": "before-upgrade"}'
curl -s localhost:3980/sandboxes/$SBX/checkpoints            # oldest first
curl -s -X POST localhost:3980/sandboxes/$SBX/checkpoints/before-upgrade/restore
curl -s -X DELETE localhost:3980/sandboxes/$SBX/checkpoints/before-upgrade

hm sandbox vm checkpoint save|list|restore|delete $SBX [NAME]
```

- **Up to 10 a sandbox,** named with letters, digits, `-`, `_` and `.`.
- **Restored in place:** the same sandbox ID, token and URL, as it was when
  the checkpoint was taken.
- **Only what changed is written,** against the template's memory, which
  the guest was restored from. So a checkpoint is quick and small, and it
  needs that template to restore.
- `autoCheckpoint` on a create, or `--auto-checkpoint-every` for a node,
  keeps one named `auto`, replaced on the interval.
- Not for a sandbox holding a [disk](DISKS.md), nor one that is
  [a size of its own](SANDBOX_SIZES.md). They can be paused.
- Kept on the node that took them.

## Reading one from outside

A checkpoint's files can be listed and read without touching the sandbox,
running or paused:

```sh
curl -s "localhost:3980/sandboxes/$SBX/checkpoints/before-upgrade/files?path=/root"
# → {"path":"/root","checkpoint":"before-upgrade","entries":[
#     {"name":"notes","type":"directory","mode":"755","uid":0,"gid":0,"size":null}, …]}
curl -s "localhost:3980/sandboxes/$SBX/checkpoints/before-upgrade/file?path=/root/notes/a.txt"
# A whole directory, as a gzipped tar with its contents at the top:
curl -s -o notes.tar.gz "localhost:3980/sandboxes/$SBX/checkpoints/before-upgrade/archive?path=/root/notes"

hm sandbox vm checkpoint ls $SBX before-upgrade /root
hm sandbox vm checkpoint cat $SBX before-upgrade /root/notes/a.txt
```

The answers have the same shape as for a
[machine's snapshot](MACHINES.md#snapshots): each entry's `type` is
`directory`, `file`, `symlink` or `other`, with its permission bits in
octal, its owner, group and size.

**How:** there is no disk image to open, as a machine's snapshot has. So
each request restores the checkpoint into a guest of its own. That guest is
never registered or routed to, and has no network or volumes. The node
stops every program in it except the agent, reads, and stops it.

- **The sandbox is untouched,** and so is the checkpoint: whatever the
  reading guest changes is in its own memory, gone when it stops.
- **It is the checkpoint as taken.** Its own programs are stopped before
  anything is read, so they cannot change a file meanwhile.
- **Symlinks are listed, not followed.** A path that is one is refused for
  reading.
- **At most 64 MiB a file**; larger is refused (413). A listing returns at
  most 10,000 entries.
- **A tree downloads as one archive.** `archive?path=` takes a directory
  (`/` for everything) and answers a gzipped tar of its contents, with
  modes, owners and symlinks kept. The archive is built in the reading
  guest's memory, so it is at most 64 MiB compressed (413 beyond that); for
  `/` the kernel's own `/proc`, `/sys`, `/dev` and `/run` are left out. A
  file or a path that does not exist is refused (400, 404) as for a listing.
- **Not read:** a volume's files, which are on the volume and read through
  [it](VOLUMES.md).
- **Two readers at a time** on a node, each a guest the sandbox's size for
  the length of one request, over and above `--capacity`. Each takes about
  as long as a restore.

Evidence: [real KVM checks](benchmarks/2026-10-13/checkpoint-files-kvm/README.md).
