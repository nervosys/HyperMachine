# Concurrent HTTPS CPU accounting

Two alternating pairs use the same distinct immutable matched release binaries as ../secret-https-release, eight curl clients inside one KVM guest, 128 bindings, 24 warm-up requests and 96 measured requests per cohort. All four cohorts passed 15 checks each, 556 successful upstream requests in total, and process cleanup.

| Cohort | Measured batch wall (s) | Daemon user (s, includes guest) | Daemon system (s) | Daemon guest (s) | Driver + HTTPS server CPU (s) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline AB | 1.775 | 1.70 | 1.41 | 1.04 | 0.130 |
| Candidate AB | 1.752 | 1.53 | 1.34 | 0.95 | 0.114 |
| Candidate BA | 1.745 | 1.57 | 1.28 | 0.96 | 0.112 |
| Baseline BA | 1.764 | 1.68 | 1.43 | 0.97 | 0.116 |

Median of request run medians: 113.961 to 112.242 ms (-1.5%). This small repeat difference does not establish a concurrent improvement. Batch wall includes guest exec API and response validation and differs from curl time_total. CPU snapshots exclude warm-ups. Daemon counters are Linux /proc/PID/stat deltas at 100 ticks/s with start-time identity validation. User includes guest: do not add guest again. Multiple daemon threads can consume more CPU seconds than wall seconds. The Python driver and HTTPS server share one process.

The driver/server CPU is small relative to daemon accounting; profiling daemon and guest work is the next investigation. These counters do not establish a causal bottleneck or CPU saturation. No managed competitor measurement, fixed CPU placement, throughput scaling claim, or fleet result.

Counter semantics: [Linux proc_pid_stat manual](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html). Full binary provenance remains in ../secret-https-release/build-context.json. Inputs and checker hashes are in summary.json. Raw synthetic payloads and private keys are not archived.
