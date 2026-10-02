# Rejected anonymous Unix relay prototype

The node's raw TCP tunnel opens the guest port and registers the stream under
the sandbox lifecycle lock. Reducing that lock's scope would need to account
for a pause that snapshots a connection while it is still opening. This
experiment preserved the lock and instead replaced the private loopback TCP
adapter with an anonymous Unix socket pair on Unix hosts. Other hosts kept
the TCP fallback. Both paths used the same directional-EOF splice loop.

The prototype was built and tested, then reverted because the measurements
did not establish a repeatable performance improvement. No production source
change ships with this experiment. The accepted API socket buffering fix and
guest backlog improvement remain in place.

| Profile/run | Payload | HyperMachine P50 / P95 / P99 (ms) | Firecracker P50 / P95 / P99 (ms) | Verified per engine |
|---|---|---:|---:|---:|
| c1-run-1-baseline | 4 KiB | 5.59 / 26.71 / 42.98 | 2.68 / 20.79 / 36.36 | 100/100 |
| c1-run-1-baseline | 1 MiB | 22.02 / 83.86 / 101.43 | 12.85 / 56.90 / 127.40 | 100/100 |
| c1-run-2-candidate | 4 KiB | 3.33 / 13.21 / 17.19 | 1.91 / 10.79 / 20.23 | 100/100 |
| c1-run-2-candidate | 1 MiB | 10.47 / 53.64 / 104.38 | 8.35 / 33.45 / 87.46 | 100/100 |
| c1-run-3-candidate | 4 KiB | 2.94 / 8.15 / 14.57 | 1.83 / 4.79 / 12.87 | 100/100 |
| c1-run-3-candidate | 1 MiB | 9.59 / 51.56 / 60.95 | 7.09 / 46.29 / 48.47 | 100/100 |
| c1-run-4-baseline | 4 KiB | 3.33 / 5.66 / 7.62 | 1.85 / 4.38 / 9.05 | 100/100 |
| c1-run-4-baseline | 1 MiB | 9.49 / 15.36 / 51.92 | 6.71 / 47.06 / 52.19 | 100/100 |
| c8-run-1-baseline | 4 KiB | 13.06 / 47.54 / 80.80 | 18.25 / 53.12 / 62.57 | 240/240 |
| c8-run-1-baseline | 1 MiB | 50.52 / 123.24 / 213.38 | 54.07 / 96.71 / 216.66 | 240/240 |
| c8-run-2-candidate | 4 KiB | 18.71 / 69.60 / 93.96 | 24.50 / 158.84 / 199.88 | 240/240 |
| c8-run-2-candidate | 1 MiB | 92.72 / 265.30 / 369.36 | 124.46 / 1659.26 / 3312.99 | 240/240 |
| c8-run-3-candidate | 4 KiB | 16.53 / 40.63 / 55.83 | 18.07 / 28.90 / 50.08 | 240/240 |
| c8-run-3-candidate | 1 MiB | 77.36 / 163.73 / 194.26 | 75.95 / 176.61 / 182.98 | 240/240 |
| c8-run-4-baseline | 4 KiB | 13.99 / 32.76 / 47.62 | 19.62 / 23.73 / 25.74 | 240/240 |
| c8-run-4-baseline | 1 MiB | 57.00 / 109.49 / 143.61 | 63.18 / 126.86 / 135.15 | 240/240 |

Each concurrency profile used baseline, candidate, candidate, baseline order;
engine order alternated within every run. Concurrency 1 used 20 guest pairs
and five rounds per size (400 transfers/run). Concurrency 8 used 10 pairs
and three rounds per size (960 transfers/run). All eight runs passed, totaling
5,440 byte-verified transfers. Every guest preparation and cleanup passed;
all node processes stopped, zero sandboxes remained, and artifact hashes
were unchanged.

Both engines used the same larger-backlog guest image, kernel, 1 vCPU and
1024 MiB. The client and VMMs inherited eight host CPUs. Within each profile
only the HyperMachine daemon artifact changed. The baseline daemon is the
accepted API-buffering release; the guest agent has a 128-entry pending queue,
avoiding the shared small-backlog limitation identified in
[the concurrent controls](tcp-concurrent.md).

In sequential 1 MiB transactions, candidate medians were 10.47 and 9.59 ms,
while bracketing baselines were 22.02 and 9.49 ms. In eight-stream transactions,
candidate medians were 92.72 and 77.36 ms, versus baseline 50.52 and 57.00 ms.
Host load changed sharply: the unchanged Firecracker path's eight-stream
1 MiB P99 reached 3312.99 ms in one candidate run, while its final baseline
P99 was 135.15 ms. The sequential final baseline was faster than both
candidates. These observations do not support accepting the prototype as a
latency improvement, and do not establish that Unix sockets are generally
slower than TCP.

This is a shared WSL nested-KVM host, not an isolated performance environment.
CPU affinity is not CPU isolation. Python client and post-barrier scheduling
costs are included; handshake, transfer and total transaction timing are
retained separately. Repeated rows within a guest are correlated. Percentiles
use nearest rank. Startup and service readiness are excluded. Different native
host transports remain explicit: authenticated HTTP upgrade for HyperMachine
and Firecracker's Unix vsock socket plus the identical guest Forward operation.
There is no TLS performance, sustained-arrival, fleet or managed-product claim.

The prototype nevertheless passed all 15 real KVM/TLS lifecycle cases,
including binary transfers, both EOF directions, repeated CLI interruption,
idle-stream activity, pause/resume, fork and deletion. Regression checks passed
89 tests on Windows (TCP fallback) and 93 on Linux (Unix branch), plus strict
all-target Clippy on both. The six scorer tests passed. Functional correctness
alone was not used as evidence of improved performance.

The prototype release SHA256 is
`f3955822a63ad7050e37f78610503126179d17d523b0f21dd11e7bbad6f3c666`.
The baseline daemon SHA256 is
`03043e33a6efe017bf0f9d2f0d7ee9caf4e0c09705abeaf849f21825128f799d`.
The matched image SHA256 is
`7d36dd6384504ec60abf2e69f9d78941cfae8d96110d9e05f5d45ce87fdd1224`.
The [50-file manifest](tcp-unix-manifest.json) indexes raw rows, exact compiled
prototype source, build output, coordinators, runtime logs and platform checks.
The build restored the original previously scored daemon, and all earlier
accepted/rejected evidence remains intact.

To verify frozen evidence and confirm the production adapter is unchanged:

```sh
python3 docs/benchmarks/2026-10-01/verify-tcp-unix.py --production --staged
```

The archived source can reconstruct the rejected prototype in an isolated
checkout. Adapt the archived builder/coordinator paths to a KVM host. Further
profiling should separate client scheduling, guest processing and host relay
CPU cost on an isolated host before selecting another latency optimization.
The wider feature and performance gaps in [the comparison table](../../PLATFORM_PARITY.md)
remain open.
