# Current cold-start stage attribution

Unscored same-binary diagnostic runs passed 116/116 guests per engine: two C8 pairs and one C100 pair. Kernel/image, optimized executable, guest resources and eight-CPU affinity match the scored current-release sweep. Every successful HyperMachine guest ID matches exactly one startup, cold-readiness and dispatch record. All guests and owned processes are cleaned up, input hashes remain unchanged, and nine existing stage-parser tests pass.

| Concurrency | Guests | Build median ms | Launch median ms | Agent-answer median ms | Blocking queue median ms | Connect median ms | Ping median ms | First backend median ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 8 | 16 | 0.72 | 61.90 | 420.82 | 2.75 | 416.81 | 0.32 | 198.18 |
| 100 | 100 | 5.04 | 75.87 | 11403.60 | 6.53 | 11363.42 | 6.21 | 6066.02 |

The agent-answer interval dominates these traces, mainly in connection wait. Blocking-worker queueing and VM building are much smaller. Connection wait includes guest boot, driver and listener readiness; it is not proof of network latency or a polling defect. C100 also has a large first-backend-call wall interval, making guest execution and vCPU scheduling the next investigation target. That interval is not a CPU-time measurement and cannot establish the cause.

Debug tracing changes timing: the C100 diagnostic agent-answer median exceeds 11 seconds, whereas the separate scored cohort has a 6.87-second end-to-end median. Do not use diagnostic latencies as competitor rankings, quantify regression from these different cohorts, sum medians as a per-request budget, or promote an optimization from stage attribution alone. No runtime default or production source was changed.

Per-guest raw traces and maxima remain in reports and summary.json. The scored [cold comparison](../current-release-cold-sweep/README.md) remains the performance evidence. Reproduce with the archived coordinator/harnesses and frozen input hashes; the daemon basename must be hv2-sandboxd for the existing diagnostic wrapper to intercept its launch.
