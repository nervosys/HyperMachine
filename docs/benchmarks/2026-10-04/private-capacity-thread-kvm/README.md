# Target thread CPU diagnostics under concurrent UDP

The unchanged accepted setup-overlap production daemon is observed during the same matched private-standard-standard-private workload. Each target `/proc/PID/task/TID/stat` snapshot retains TID, thread name, start ticks, user+system CPU ticks and state. Only stable identities present before and after are included in deltas. New/vanished/unmatched identities are recorded. Sequential thread and process snapshots differ in time and quantization, so totals are not required to reconcile exactly. This diagnostic observer can perturb execution; use previous resource cohorts for ordinary throughput comparisons.

| Block / path | Matched threads | Tokio worker CPU seconds | vCPU CPU seconds |
|---|---:|---:|---:|
| 1 / private | 351 | 20.60 | 1.24 |
| 2 / standard | 351 | 19.48 | 1.16 |
| 3 / standard | 351 | 20.34 | 1.18 |
| 4 / private | 351 | 19.85 | 1.17 |

Across all blocks, 351 stable target threads are matched. Tokio-named workers account for 19.48–20.60 CPU-seconds; vCPU threads account for 1.16–1.24 CPU-seconds. The diagnostic localizes most observed cost to Tokio workers, which include blocking relay workers and runtime tasks. Names do not identify a hot function or distinguish CPU from lock contention. Raw per-thread top lists and groups are retained in `thread-analysis.json`.

Permitted source inspection identifies a concrete candidate: `VsockStream::read`/`write_all` use the device-wide `Progress`, whose `signal` calls `notify_all` after consumed guest traffic. Many held streams share that notifier. Connection-specific progress could reduce unrelated wakeups, but requires careful receive/credit/close/reset/removal notification tests, cancellation handling and matched release measurement. This is a hypothesis, not causal proof or a measured optimization; no production change is made here. Frozen permitted device/agent source is included. Protected root KVM/boot files were not read or built.

All 12,800 exact matched datagrams, 3,200 initial capacity datagrams, 128 regression operations and 55 KVM checks pass, with zero UDP error/echo-socket drop deltas and full guest/process cleanup. Stable-process resource arithmetic and thread identity/delta/grouping are independently verified by `python3 verify-results.py`. The checker flag `--private-capacity-thread-profile` requires `--private-capacity-comparison`. Drivers require fresh output paths; manifest pins evidence. Input source catalog now includes the inspected permitted virtio-vsock device and preserves accepted isolated core identities.

Source guest Ethernet, hot-function stack attribution, independent hosts, managed competitor comparison, mixed TCP saturation and injected setup cancellation remain incomplete or unmeasured. No CPU saving, throughput win or competitor superiority is established.
