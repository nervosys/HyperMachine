# Matched fixed-rate native arrivals (2026-10-01)

Both engines receive the same fixed-rate schedule with eight client workers, one vCPU and 1024 MiB per guest, identical kernel/initrd, and eight pinned host CPUs on shared nested KVM. Two pairs alternate HM/FC then FC/HM at each rate. HyperMachine uses the event-preserving build identified in [its build evidence](kvm-events.md); this is not a fresh build of the later snapshot-ID compatibility change. Exact executable/image/harness hashes are in each raw report. Firecracker is v1.17.0.

A planned arrival is submitted independently of previous completions. The bounded worker pool queues excess work; queue delay is measured from submission to worker entry, and submission lag from planned arrival to submission. Each successful guest produces its unique command marker and passes resource verification before immediate cleanup. No batch-wide hold is used. Completion rates include cleanup and the final drain, with all attempted arrivals in the denominator. Successful command latency is derived from raw start offset plus command readiness duration minus planned arrival offset; it includes submission lag, queueing and setup after worker entry. The raw report's `scheduled_ready_ms` field refers to the later resource-verification callback, not only the command; both are retained distinctly in the summary.

| Offered arrivals/s | Engine | Passed / attempted | Scheduled command P50 / P99 (ms) | Client queue P50 / P99 (ms) | Completed lifecycles/s by pair |
|---|---|---|---|---|---|
| 5 | hypermachine | 80 / 80 | 371.85 / 475.81 | 0.08 / 0.36 | 4.87 / 4.86 |
| 5 | firecracker | 80 / 80 | 342.80 / 438.56 | 0.12 / 0.37 | 4.88 / 4.87 |
| 25 | hypermachine | 200 / 200 | 1512.83 / 2683.92 | 1072.58 / 2276.62 | 14.66 / 14.95 |
| 25 | firecracker | 200 / 200 | 1156.72 / 2034.47 | 775.15 / 1670.06 | 16.55 / 16.40 |

All 560 attempts passed, with unchanged artifacts and no sandbox records left after cleanup. At five arrivals/s, both engines keep near the offered rate over the short schedule. At 25 arrivals/s, both accumulate client queues and drain below the offered rate; Firecracker has lower command tails and higher completed lifecycle rate in these pairs. The client limit includes cleanup, so these figures do not establish a server-only maximum, a sustained fleet capacity, an SLA or an across-the-board win. The first-to-last planned arrival intervals are 7.8 seconds at rate 5 and 3.96 seconds at rate 25; longer arrivals, overload failures, recovery across load changes and multi-node runs remain unverified. The memory baseline is informational; this run measures no held guest PSS or density.

The 17 synchronization, queue, failure-accounting and cleanup tests pass on Windows and Linux. Raw per-attempt reports, execution manifests, exact executed harness/coordinators and an analyzer are retained beside this document. Failures would invalidate the cohort and remain in its denominator; these two bounded profiles happened to have none.
