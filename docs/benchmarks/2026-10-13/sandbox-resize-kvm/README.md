# A sandbox's size changed after it was created

**What this checks:** `POST /sandboxes/{id}/resize`, and `resume` with
`cpuCount` and `memoryMB`. Before this a sandbox kept the size it was
created at. boat changes a sandbox's size on resume and on fork; see the
[comparison](../../../BOAT_PARITY.md).

The times below are the check's own stopwatch around one request. They
show which resizes boot and which restore; they are not a benchmark.

## Method

`tools/check-sandbox-resize-kvm.py` runs an owned `hv2-sandboxd`. Its
template is one vCPU and 256 MiB, and it has a disk directory. The check
asks each guest what it has, through `/proc/cpuinfo` and `MemTotal`.

1. **Resized up, while running:** a sandbox with an env var and a file in
   `/tmp` is resized to 2 vCPUs and 1024 MiB.
   - Same ID, listed once.
   - The guest sees 2 processors and about 1 GiB, and the record says so.
   - The variable is there; the file in `/tmp`, which is memory, is gone.
2. **Disk kept:** a sandbox with a disk mounted is resized to 2 vCPUs, and
   the file on its disk is still there.
3. **Unchanged:** a resize to the size it has answers `resized: false`, and
   a file in `/tmp` survives it.
4. **Back to the template's size:** 1 vCPU and 256 MiB. Then it is paused
   and resumed, and a file in `/tmp` survives that. It was restored from
   the template, so it pauses as such a sandbox does.
5. **Resumed at another size:** paused, then resumed at 2 vCPUs and
   512 MiB.
   - The new size is there; the variable is kept; the file in `/tmp` is not.
   - Paused and resumed again with no size, it keeps both the size and the
     file.
6. **Usage:** its vCPU-seconds fall between one and two per running second.
7. **Refusals:**
   - no size; 0 processors; more than the host has; 128 MiB; an odd amount;
   - no such sandbox;
   - resizing a paused sandbox (409, saying to resume it at a size);
   - resuming at a size that cannot be, which leaves it paused, and it then
     resumes.

## Result

All seven cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM, on a 24-thread host. From [`report.json`](report.json):

| | Guest sees | Took |
|---|---|---|
| Resized to 2 vCPUs, 1024 MiB (booted) | 2 processors, 994,508 kB | 0.300 s |
| Disk sandbox resized to 2 vCPUs (booted) | file on disk kept | 0.248 s |
| Asked for the size it had | nothing rebooted | 0.001 s |
| Back to 1 vCPU, 256 MiB (restored from the template) | | 0.030 s |
| Resumed at 2 vCPUs, 512 MiB (booted) | 2 processors, 479,180 kB | 0.206 s |

Usage for the first sandbox: 3.243 running seconds, 6.170 vCPU-seconds and
1.914 GiB-seconds. Most of that time, including the check's two-second wait
before asking, was at two vCPUs.

## What this does not show

- **A size changed on a fork**, which boat offers. Not done.
- **A large size.** Two vCPUs and a gibibyte.
- **A cluster**, where the resize goes to the node that runs the sandbox.
- **Webhooks.** A resize is mapped to `sandbox.lifecycle.updated`; no
  delivery was checked here.
- **Speed.** One sample each.
