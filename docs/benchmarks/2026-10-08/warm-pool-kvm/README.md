# The warm pool on real KVM: creates that take a sandbox restored beforehand

**What this checks:** the [warm pool](../../../WARM_POOL.md), which keeps
restored sandboxes ready for creates to take. boxd publishes "a full computer in
<10ms"; see the [comparison](../../../PLATFORM_PARITY.md).

## Method

`tools/check-warm-pool-kvm.py` runs two `hv2-sandboxd` daemons from one binary,
alike except that one has `--warm-pool 4`. Each sandbox has one vCPU and
512 MiB.

1. **The pool fills.** `GET /pool` reaches four ready spares, and
   `GET /sandboxes` lists none of them.
2. **A sandbox of its own.** Two creates each take a spare. Each sandbox has the
   environment it was created with, 16 bytes from `/dev/urandom` that differ
   from the other's, a clock within five seconds of the host's, and a
   filesystem the other does not see. Neither is in standby.
3. **Refill.** The pool is back to four.
4. **More creates than spares.** Seven creates at once all succeed and all run a
   command. The pool's counters must account for all seven: at least four from
   the pool, at least one restored the ordinary way.
5. **Like any other.** A sandbox from the pool is paused to disk, resumed and
   forked, and a file written before is in both it and its fork.
6. **Timings.** `POST /sandboxes` is timed one at a time, in four blocks of 50:
   without the pool, with, with, without. Before each pooled create the check
   waits for the pool to be full, and after each create it runs a command in
   the sandbox, so a create that returned something unusable would fail.

## Result: all six pass

The full record is in [`report.json`](report.json). Neither daemon's log has a
warning, error or panic.

- Of the seven creates at once, four came from the pool and three were restored
  the ordinary way.
- `POST /sandboxes`, end to end over HTTP, 100 creates each:

  | | Minimum | Median | 90th percentile | Maximum |
  |---|---:|---:|---:|---:|
  | Without the pool | 16.42 ms | 18.89 ms | 29.88 ms | 44.08 ms |
  | With the pool | 0.48 ms | 0.65 ms | 0.83 ms | 11.61 ms |

  None of the 100 pooled creates found the pool empty.

## What these numbers are, and are not

- They are one create at a time against a full pool. That is the pool's best
  case. A burst larger than the pool, or creates arriving faster than spares
  are restored, is served at the unpooled speed for the excess, as case 4
  shows.
- The host is a 24-thread Windows machine running WSL2, where a guest is a
  nested KVM guest. WSL's load average was 0.2 to 0.4 and the Windows host read
  25% CPU after the run. One hundred creates a side is a modest sample, and the
  pooled maximum (11.6 ms) is far above its 90th percentile (0.83 ms).
- The guest is the base template: a small initramfs, not a full distribution.
- **No other product was measured.** boxd's "<10ms" is its own published
  statement about its own service, with no stated method or guest. This record
  shows that on this host at least 90 of 100 pooled HyperMachine creates took
  under 1 ms and the slowest took 11.6 ms. It does not show it is faster than
  boxd's.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release build, branch `feat/warm-pool`) | `62c4227ae83ca1f04678fa878b3431b454a657ea4b380929e229e5d57afe6e67` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-warm-pool-kvm.py` | `79a40118a3f63063c8e6957532c6658cec10a9990a36de7f7f35da35fe6419e8` |

## Not shown here

- **A networked node.** Neither daemon ran with `--network`, so no spare had a
  network device and no create had a gateway to start.
- **Sustained load,** and how large a pool a given create rate needs.
- **The memory the spares hold.** Not measured here.
- **A spare left in standby for hours** before being handed out.
- **The control plane.** Creates went to the node; a create through the control
  plane adds its own round trip.
