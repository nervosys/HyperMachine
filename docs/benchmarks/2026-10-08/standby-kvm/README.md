# Standby on real KVM: a stopped guest, woken by the next request

**What this checks:** [standby](../../../STANDBY.md), a sandbox with its vCPUs
stopped and its memory kept, resumed by whatever is sent to it next. boxd
documents the same state ("resume is sub-millisecond"); see the
[comparison](../../../PLATFORM_PARITY.md).

## Method

`tools/check-standby-kvm.py` runs an owned `hv2-sandboxd` with a template and
`--idle-standby-after 30`, and one sandbox with one vCPU and 512 MiB. A process
in the guest increments a counter in a file ten times a second.

1. **Standby stops the guest.** After `POST /sandboxes/{id}/standby`, the
   sandbox is left alone for 12 seconds, which spans two of the node's
   five-second metric samples. Its vCPU exit count must not change, and nothing
   may have woken it.
2. **A command wakes it.** The next `exec` answers. The node records one wake,
   the sandbox is no longer in standby, and the counter has advanced by a few
   counts, not by the 120 that 12 running seconds would have added. A second
   later it has moved on.
3. **A port request wakes it.** The guest serves HTTP on port 8080. After
   another standby, a request through the node's proxy gets the page, and the
   node records another wake.
4. **Pause from standby.** A sandbox in standby is paused to disk (204) and
   resumed (201), and a file written before is still there.
5. **Timings.** Forty rounds, alternating: time a command to the awake sandbox,
   put it in standby, time the same command, and read the node's own figure
   for the wake.
6. **By itself.** With no request for longer than the 30-second window, the
   sandbox goes into standby, and a command wakes it.

## Result: all six pass

The full record is in [`report.json`](report.json). The daemon's log has no
warning, error or panic.

- **Stopped means stopped.** The vCPU exit count was 52 before and 52 after the
  12 seconds. The counter read 9 before and 10 on waking.
- **The node's part of a wake**, from finding the guest stopped to its vCPUs
  being told to run:

  | | Minimum | Median | 90th percentile | Maximum |
  |---|---:|---:|---:|---:|
  | Node wake, 40 rounds | 29 µs | 57 µs | 99 µs | 133 µs |

- **A command, end to end** (`exec` of `true` over HTTP, to the guest and back):

  | | Minimum | Median | 90th percentile | Maximum |
  |---|---:|---:|---:|---:|
  | To an awake sandbox | 1.60 ms | 3.33 ms | 5.45 ms | 7.87 ms |
  | To a sandbox in standby | 1.67 ms | 3.30 ms | 7.14 ms | 10.14 ms |

  So waking adds nothing that these forty rounds can tell apart from the
  command's own variation at the median. The 90th percentile and maximum are
  higher from standby.

## What these numbers are, and are not

- They are one sandbox on one host: a 24-thread Windows machine running WSL2,
  where the guest is a nested KVM guest. WSL's load average was 0.6 to 0.8
  during the run, but the Windows host read 96% CPU immediately after it, so
  the host was not quiet. Forty rounds is a small sample.
- The node's figure stops when the vCPUs have been told to run. It does not
  include the guest being scheduled or doing anything.
- **No other product was measured.** boxd's "sub-millisecond" is its own
  published statement about its own service, with no stated method. This
  record shows HyperMachine's wake is sub-millisecond on this host; it does not
  show it is faster than boxd's.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release build, branch `feat/standby`) | `4ce27684de2a3f1352a9e38a1ee90159e2676755b032d532219a7bae4a020e6f` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-standby-kvm.py` | `7a49fbd710b725ed788b335ec78ca4cde01cc8c3b275b14a7b942e15e741d4ec` |

## Not shown here

- **Many sandboxes in standby at once,** or waking many at once.
- **A fork, snapshot or checkpoint of a sandbox in standby.**
- **A guest with a network device and open outbound connections.** The sandbox
  here had none.
- **Long standbys.** The longest was about 40 seconds; what a guest's clock and
  timers do after hours stopped was not looked at.
- **The control plane and `hm`.** The check used the node API.
