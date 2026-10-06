# First KVM-call CPU/wall attribution

Same optimized-binary diagnostics passed 108/108 guests per engine: one C8 pair and one C100 pair. Inputs, one-vCPU/1,024-MiB guests and eight allowed CPUs match the scored cold sweep. Every HyperMachine guest has exactly one matching startup, readiness, dispatch, first-return and CPU-clock record. All guests and owned processes are cleaned up; executable/image/harness hashes remain unchanged.

| Concurrency | Guests | Median thread CPU ms | Median wall ms | Median per-guest CPU/wall |
|---|---:|---:|---:|---:|
| 8 | 8 | 206.31 | 213.01 | 98.51% |
| 100 | 100 | 375.25 | 5006.58 | 7.46% |

The low-concurrency first-call interval is mostly measured thread CPU time. At C100, measured thread CPU is a small fraction of wall time. This is consistent with substantial scheduling or waiting effects under oversubscription; it does not identify their cause, quantify host-steal time, separate guest from VMM CPU, or account for work on other threads. All first returns are io_out at port 3320 (0xcf8); the port alone is not an execution trace or kernel root cause.

This directs the next experiment toward the existing opt-in cold-start admission budget on the current binary, using paired scored runs that include queueing and hold all guests through memory measurement. Existing older admission experiments had mixed median/tail/memory tradeoffs, so this attribution does not justify changing the default.

Tracing changes timing and these one-pair diagnostics are not competitor rankings or a demonstrated optimization. The [scored current-release sweep](../current-release-cold-sweep/README.md) remains authoritative. The ratio column is the median of individual guest ratios, not the ratio of pooled medians. Raw clocks remain in each report. The existing clock/stage parser passed nine tests in the preceding unchanged-source stage verification.

Reproduce with the archived coordinator and frozen hashes; the daemon basename must be hv2-sandboxd for the existing wrapper to intercept its launch. Protected modified root core files remain excluded from the accepted isolated build.
