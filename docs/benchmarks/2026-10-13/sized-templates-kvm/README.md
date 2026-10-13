# Sandboxes of another size, restored rather than booted

**What this checks:** a node that builds its template again at each size a
sandbox asks for, and restores later sandboxes at that size from it. Before
this, every sandbox not its template's size booted. boat's "fast start" is
at any of its sizes; see the [comparison](../../../BOAT_PARITY.md) and
[how it works](../../../SANDBOX_SIZES.md#a-fast-start-at-any-size).

The times below are the check's own stopwatch around one request. They
show which sandboxes boot and which restore; they are not a benchmark.

## Method

`tools/check-sized-templates-kvm.py` runs an owned `hv2-sandboxd`. Its
template is one vCPU and 256 MiB, and it runs with
`--sized-templates 2 --template-size 2x1024`.

What each guest was restored from, if anything, is read from the daemon's
own line for it, `VM '<id>' restored from <snapshot>`, after that guest's
latest `Provisioned` line. A guest with no such line booted. (The guest's
`boot_id` could not tell: the kernel makes it on first read, after the
restore, so each restored guest has its own.)

1. **Built at start:** once the log says `base@2x1024` is built, two
   sandboxes at 2 vCPUs and 1024 MiB are created.
   - Both are restored from one snapshot, which is not the base
     template's.
   - Each sees 2 processors and about 1 GiB.
   - Their random numbers differ.
2. **Built when first asked for:** the first sandbox at 2 vCPUs and
   512 MiB boots. Once `base@2x512` is built, the next two are restored from
   it, a snapshot other than the base's and 2x1024's.
3. **Resize and fork:** a sandbox with a variable is resized to 2x1024 and
   is restored from that size's snapshot. A fork of it at 2x512 is restored
   from 2x512's. Both have the variable.
4. **Still its own size:** that resized sandbox is paused and resumed, and
   a file in its `/tmp` survives. A checkpoint of it is refused (409).
5. **The limit:** with two sizes held, two sandboxes at 1 vCPU and 512 MiB
   both boot, and no `base@1x512` is built.

## Result

All five cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM, on a 24-thread host. From [`report.json`](report.json):

| | Started from | Took |
|---|---|---|
| 2 vCPUs, 1024 MiB, built at start | its size's snapshot | 0.025 s, 0.019 s |
| 2 vCPUs, 512 MiB, first asked for | booted | 0.248 s |
| 2 vCPUs, 512 MiB, after it was built | its size's snapshot | 0.024 s, 0.020 s |
| Resized to 2x1024 | its size's snapshot | 0.024 s |
| Forked at 2x512 | its size's snapshot | 0.025 s |

The node's log: `base@2x1024` built in 0.62 s, and `base@2x512` in
0.44 s.

The 2x1024 guest saw 994,508 kB. The checkpoint refusal read:
"is a size of its own, not its template's, and such a sandbox cannot be
checkpointed; it can be paused".

## What this does not show

- **A large size.** Up to two vCPUs and a gibibyte. A sized template takes
  as much disk as its guest's memory, and a larger one takes longer to
  build.
- **A template rebuilt** under the same name, which forgets its sizes.
  That path is in the code and not exercised here.
- **Another template than `base`,** or one built from an image.
- **A cluster.** The sized snapshots are each node's own.
- **Speed.** One or two samples each.
