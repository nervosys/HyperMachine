# Files: bytes put into a machine's guest, and taken out of it

**What this checks:** [files](../../../MACHINES.md#files) for machines:
`PUT` and `GET /machines/{name}/files?path=`. boat has `scp`; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-files-kvm.py` runs an owned `hv2-sandboxd` with one
machine from the base template, one vCPU and 512 MiB.

1. **Round trips.** Four files are written and read back: text, every byte
   value three times over, an empty file, and 9 MiB and 123 bytes of random
   data, which is more than two of the 4 MiB pieces the guest agent moves a
   file in and not a multiple of one. Each must come back identical, and the
   guest's own `wc -c` and `sha256sum` of it must agree with what was sent.
2. **Owned.** A file written beneath three new directories with `user=dev`:
   those directories and the file must be that user's, and a run as that
   user must be able to read it and write beside it. Without a user, root's.
3. **Overwritten.** A short file written over a long one must be exactly
   the short one.
4. **From the guest.** A file the guest made, whose name has spaces, quotes,
   `&` and a non-ASCII letter, must come out; and one written by such a name
   must be there.
5. **Refusals.** A relative path, a directory's name, no path, a file that
   is not there, a directory read as a file, a file beneath a file, a user
   on a read, a user the guest has not, a file of 65 MiB out, one of 64 MiB
   and a byte in, a machine that is not there. Each must have the status the
   documentation gives, and none of the refused file in may be in the guest.
6. **Stopped.** A read and a write to a stopped machine are refused.

## Result

All six passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-11.
[`report.json`](report.json) has each file's size, the owners the guest
reported, and the words of each refusal.

The first run of the check failed in case 5. A write beneath a path that is
a file was answered 502, as a failure between the node and the guest, when
it is the caller's path that is wrong. The guest words that refusal `File
exists`; it is now a 400.

## What this does not show

- **A machine from a disk image.** The check uses a template machine. The
  same routes serve one whose agent was installed by cloud-init, and were
  not run against one.
- **The control plane.** It forwards these routes to the machine's node; the
  check talks to the node.
- **`hm sandbox vm machine upload` and `download` against a node.** They are
  tested against a fixture that answers as a node does.
- **A file of exactly 64 MiB**, a write cut off part way, or two writes to
  one path at once.
- **How fast.** Nothing here is timed.
