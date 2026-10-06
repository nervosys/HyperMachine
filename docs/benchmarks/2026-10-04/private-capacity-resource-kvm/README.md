# Matched concurrent UDP CPU and memory evidence

The same private-standard-standard-private 32-worker workload now retains raw `/proc/PID/stat` CPU ticks and `/proc/PID/smaps_rollup` PSS before/after each scored block for the target daemon, source daemon, owned Redis and Python checker. PID/start ticks prove process identity across every sample/block. Both tunnel sets remain live. CPU values include all process threads; target includes guest vCPU threads and relay/control work. They are not isolated transport CPU. Reads bracket the workload and are quantized at the recorded clock tick rate; their interval includes resource sampling work and excludes guest-counter API calls. PSS describes whole processes with both route sets held, not per-path allocated memory.

| Block / path | Payload MiB/s | Target CPU seconds | Target CPU ms / payload MiB | Target PSS before → after (MiB) |
|---|---:|---:|---:|---:|
| 1 / private | 21.728 | 21.58 | 320.53 | 102.37 → 105.37 |
| 2 / standard | 22.472 | 21.50 | 319.34 | 105.46 → 107.80 |
| 3 / standard | 21.723 | 22.34 | 331.82 | 107.88 → 107.98 |
| 4 / private | 22.051 | 21.66 | 321.72 | 108.00 → 108.03 |

All **12,800/12,800 matched datagrams**, 3,200 initial concurrent capacity datagrams, 128 regression benchmark operations and **55 KVM checks** pass, with zero per-block guest UDP error/echo-socket drop deltas and full guest/process cleanup. Raw worker byte totals, throughput, stable process identities, CPU deltas/normalization and PSS conversions are independently recomputed.

Private throughput 21.728–22.051 MiB/s overlaps standard 21.723–22.472 MiB/s. Private target CPU cost 320.53–321.72 ms/payload MiB overlaps standard 319.34–331.82 ms/payload MiB. The previous cohort showed a small consistent private gap; this repeat does not. Do not promote either cohort to a durable difference or parity claim. Whole target CPU dominates observed process cost: 21.50–22.34 CPU-seconds per roughly three-second block, versus much smaller checker/Redis/source costs. This supports profiling target threads next; it does not identify a hot function or justify weakening authorization. PSS grows by block order and is not a private/standard memory comparison.

Both paths use the exact accepted buffered fixture image and optimized release daemon. Explicit fixture receive buffering and prior failure diagnosis are documented in `../private-capacity-concurrent-kvm/`; matched workload design is in `../private-capacity-comparison-kvm/`. Source guest Ethernet is outside timed traffic. Nonexclusive WSL host, Python client work, guest echo and scheduler effects remain. No managed competitor performance, source-gateway throughput, CPU saving or per-route memory superiority is established.

Run `python3 verify-results.py` for full workload/status/journal/statistics/resource/source/cleanup validation. Frozen checker/driver, raw block/worker/counter outcomes, input hashes and source catalog are retained. Manifest pins payloads; drivers require fresh outputs. Production receiver is unchanged. Protected root core files were not read or built.
