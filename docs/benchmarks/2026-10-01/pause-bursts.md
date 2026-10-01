# Synchronized pause and resume bursts (2026-10-01)

All live-process memory, filesystem and boot-ID probes are prepared before a separate pause barrier. Each pause call is timed through its SDK response, then paused state is verified. A second barrier waits for the whole batch before timing resume through the first successful state-verifying command. Successful samples also verify guest resources and cleanup; pause latency below is conditional on the whole lifecycle succeeding. Start spread is the observed client spread, not a guarantee of simultaneous server arrival.

The runtime is the event-preserving KVM build from [the matched repair experiment](kvm-events.md), with identical kernel/initrd and SDK dependencies. Each concurrency starts a fresh owned node on shared nested KVM, eight host CPUs and one pinned busy worker; guests use one vCPU and 1024 MiB. This is a single-runtime sweep, with no matched competitor or parent comparison. High-concurrency cohorts contain only two batches. No speed win or reliability SLA is established.

| Concurrency | Passed / attempted | Pause P50 / P99 (ms) | Resume-to-command P50 / P99 (ms) | Max pause client start spread (ms) |
|---|---|---|---|---|
| 1 | 99 / 100 | 44.89 / 72.78 | 113.95 / 119.49 | 0.00 |
| 8 | 104 / 104 | 75.40 / 109.60 | 131.95 / 152.01 | 3.08 |
| 50 | 100 / 100 | 132.18 / 243.48 | 418.57 / 673.43 | 29.81 |
| 100 | 200 / 200 | 249.80 / 961.88 | 738.59 / 1095.98 | 39.81 |

The concurrency-1 cohort retains its nonzero exit and one resume failure (sample 24). Pause completed and paused state was verified, but resume missed the unchanged 15-second guest-agent readiness deadline; SDK-observed failure elapsed time was about 30.56 seconds. Diagnostics show a halted vCPU, no pending LAPIC interrupts and a nonzero TSC deadline. The cause is unproven. Event preservation fixes a demonstrated omission but does not eliminate this readiness failure.

All four cohorts verified unchanged source/image/runtime hashes, no remaining sandbox records, and worker teardown. Raw cohorts, failure diagnostics, exact coordinator/matrix/SDK source, hashes and an executable analyzer are retained beside this report. The separate initial C8 smoke passed 16/16; it is excluded from the sweep totals. The harness's 24 failure-accounting, barrier, state and cleanup tests passed on Windows and Linux.
