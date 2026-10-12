# A sandbox's own size, chosen at creation

**What this checks:** `cpuCount` and `memoryMB` on a sandbox create. Before
this a sandbox had its template's size and nothing else. boat chooses a
size at creation; see the [comparison](../../../BOAT_PARITY.md).

The start times below are the check's own stopwatch around one request.
They are recorded to show that a sized sandbox boots rather than being
restored, not as a benchmark.

## Method

`tools/check-sandbox-sizes-kvm.py` runs an owned `hv2-sandboxd` whose
template is one vCPU and 256 MiB, and asks each guest what it has: the
processors in `/proc/cpuinfo` and `MemTotal`.

1. **No size:** one processor, under 256 MiB, and a record that says 1 and
   256.
2. **`cpuCount` 2, `memoryMB` 1024:** two processors, between 900 MiB and
   1 GiB, and a record that says 2 and 1024.
3. **Kept.** Paused and resumed: still two processors and the same memory.
   Forked: the fork is two and 1024 too. A named snapshot and a checkpoint of
   it are refused with 409, saying why.
4. **Usage** counts its vCPU-seconds as twice its running time and its
   GiB-seconds as its running time.
5. **The template's own size, asked for,** is no change: restored, one and 256.
6. **Refusals:** no processors, more than the host has, 128 MiB, an odd
   amount, a mebibyte of mebibytes. Each 400, and no sandbox made.

## Result

All six passed, on Linux 6.18.33 under WSL2 with nested KVM, on a 24-thread
host, on 2026-10-12. From [`report.json`](report.json):

| | Processors | `MemTotal` | Start |
|---|---|---|---|
| The template's size, restored | 1 | 221,732 kB | 0.054 s |
| `cpuCount` 2, `memoryMB` 1024, booted | 2 | 994,508 kB | 0.253 s |
| After pause and resume, and its fork | 2 | 994,508 kB | |

The first run stopped at case 3: a sized sandbox could not be paused. Its
guest booted, so its memory is not mapped from the template's image and a
layered snapshot of it cannot be taken. It is now written whole when paused
or forked, as a sandbox with a disk already was, and refused for checkpoints
and named snapshots, as such a sandbox already was.

## What this does not show

- **A large size.** Two processors and a gibibyte. A node does not count a
  sandbox's memory against its own; `--capacity` counts sandboxes.
- **Speed.** One sample each, on a small guest; a template with more in its
  image boots for longer.
- **A size changed after creation**, which is not offered.
- **A cluster**, where the control plane forwards the create.
