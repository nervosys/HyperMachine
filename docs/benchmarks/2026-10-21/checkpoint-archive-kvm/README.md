# Downloading a sandbox checkpoint's tree as an archive

**What this checks:** `GET /sandboxes/{id}/checkpoints/{name}/archive?path=`,
a directory of a checkpoint as a gzipped tar. Before this a checkpoint's files
were listed and read one by one; boat downloads all or part of a snapshot's
tree as an archive. See the [comparison](../../../BOAT_PARITY.md) and
[how it works](../../../CHECKPOINTS.md#reading-one-from-outside).

`report.json` is the whole of `tools/check-checkpoint-files-kvm.py` run on KVM
(WSL2, one vCPU and 512 MiB template). Seven cases passed, the five that were
already there and the new case 7 beside them. The times are the check's own
stopwatch around one request, each including a restore; they are not a
benchmark.

## What case 7 does

The sandbox writes `/root/notes` (a mode-600 file, a file named `a b`
newline `c`, a symlink, a 1 MiB random file owned by 1000:1000, a
subdirectory), is checkpointed, then changes some of it and adds a file.
Downloading `/root/notes` from the checkpoint:

- holds exactly the five entries as taken, none of what was added since;
- `a.txt` reads `hello`, mode 600; the 1 MiB file has the SHA-256 the
  sandbox recorded and owner 1000; the odd name reads; the symlink is a
  symlink to `a.txt`; the subdirectory is a directory;
- 1,049,120 bytes of archive in 0.14 s;
- `/root/notes/sub` alone downloads as an empty tree;
- `/` downloads (2,920,804 bytes, 289 entries, 0.38 s), holds the notes, and
  holds nothing under `/proc`, `/sys`, `/dev` or `/run`;
- a file, a missing path and a relative path are refused (400, 404, 400),
  each with why.

## Not verified

- On the distribution template: the guest here is the minimal busybox
  initramfs, so the archive was made by busybox `tar` and `gzip`. The script
  uses only plain `tar -cf -`, `gzip -c` and shell, but GNU tar in the
  template has not run it.
- An archive over 64 MiB (413) is in the code and not exercised; the
  64 MiB limit for a single file is, as before.
- A machine's disk snapshot has no archive download; only a sandbox's
  checkpoint does.
- Through the control plane: the route is forwarded like `/file`, and was
  not run against a cluster here.
