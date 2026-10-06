# One parent-directory flush per atomic upload

Atomic publication already flushes its pinned immediate parent after rename. The subsequent directory-chain loop now starts at that parent's ancestors, avoiding a duplicate parent descriptor open and sync. Default in-place uploads continue flushing the complete chain. A root-level atomic upload flushes the pinned data root and has no additional ancestors inside the volume.

All 46 isolated daemon unit tests passed. Four alternating in-place/atomic cohorts passed creation/token checks, nested binary upload/replacement, exact readback on both daemons, restart and cleanup. Atomic cohorts additionally pass root-level publication and incomplete HTTP upload preservation/staging cleanup. All owned processes were reaped, no guests created. Each cohort also records 12 sequential timed PUTs after three warm-ups:

| Cohort | Mode | Median PUT (ms) |
| --- | --- | ---: |
| 0 | In-place | 7.625 |
| 1 | Atomic | 7.525 |
| 2 | Atomic | 10.629 |
| 3 | In-place | 6.837 |

This proves normal behavior after eliminating redundant I/O, not a before/after latency improvement. The earlier binary was not retained as a matched baseline; host/storage timing drift was substantial in the earlier characterization. These are dev-profile local measurements with exact readback outside the timed interval. No power-loss, syscall-trace count, network-storage, optimized-release or competitor performance claim is made. Source inspection establishes the removed duplicate call; the existing pinned-parent flush remains before success. Full reports, logs, source and input hashes are archived.
