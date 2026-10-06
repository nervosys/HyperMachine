# Concurrent HTTPS thread accounting

The unchanged matched release binaries from ../secret-https-release were compared in two alternating pairs with eight curl clients inside one KVM guest and 128 bindings. Each cohort passed all 15 correctness/lifecycle checks and cleanup; 556 upstream requests succeeded in total. Each timing window excludes 24 warm-up requests and includes 96 requests in 12 batches.

| Cohort | vCPU CPU seconds | Network CPU seconds | IRQ CPU seconds | Tokio CPU seconds | Other CPU seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| pair-0-baseline | 1.70 | 0.16 | 0.20 | 1.01 | 0.00 |
| pair-0-candidate | 1.73 | 0.16 | 0.19 | 0.80 | 0.00 |
| pair-1-baseline | 1.74 | 0.16 | 0.19 | 1.00 | 0.01 |
| pair-1-candidate | 1.70 | 0.15 | 0.18 | 0.81 | 0.00 |

The vCPU thread consumed 1.70–1.74 user-plus-system CPU seconds per cohort, more than half of total daemon CPU. This includes guest execution and vCPU-thread host work. User time already includes guest time; guest is not added again. Network/IRQ labels come from Linux thread names, not stack traces. The next profiling target is guest execution and VM exits. These measurements do not prove saturation or a causal bottleneck.

Median of request run medians: baseline 112.948 ms, candidate 112.980 ms (+0.03%, effectively unchanged). No concurrent speedup is established. Thread snapshots bracket the process snapshot, so their totals are approximate. Linux counters have 100 Hz quantization. Only threads surviving both snapshots with equal start-time identity are included; all cohorts had 31 threads before and after. Linux comm names truncate to 15 bytes. CPU seconds can exceed wall time with multiple threads. Batch wall includes guest exec API and validation; request samples remain guest curl time_total.

No managed competitor measurement, fixed CPU placement, fleet evidence, or throughput scaling claim. Full input/checker hashes and detailed per-thread counters are in reports and summary.json. Full source/build provenance remains in ../secret-https-release/build-context.json. Private keys and synthetic HTTP payloads are not archived.
