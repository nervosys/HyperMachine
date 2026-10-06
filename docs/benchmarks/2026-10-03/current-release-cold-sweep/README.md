# Current optimized daemon: matched cold-start comparison

All 336 attempts per engine passed across concurrency 1, 8, 50 and 100. The current permitted daemon sources were built in the verified isolated checkout with accepted core sources; three modified protected root core files were excluded. Firecracker 1.17.0 and HyperMachine share one immutable kernel/image, eight-CPU affinity, one vCPU/1,024 MiB per guest and the same exact shell-output readiness workload. HyperMachine uses its disabled cold-admission-budget default and `--no-template`. Engine order alternates within each profile.

| Concurrency | Engine | Passed/attempted | P50 ms | P95 ms | P99 ms | Median held PSS MiB | Median incremental PSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | hypermachine | 4/4 | 402.77 | 418.03 | 418.03 | 150.72 | 105.32 |
| 1 | firecracker | 4/4 | 341.64 | 358.75 | 358.75 | 85.92 | 85.92 |
| 8 | hypermachine | 32/32 | 504.84 | 549.63 | 554.34 | 897.75 | 690.23 |
| 8 | firecracker | 32/32 | 410.66 | 487.83 | 520.40 | 670.91 | 670.91 |
| 50 | hypermachine | 100/100 | 3114.63 | 3309.70 | 3318.44 | 4305.52 | 4218.06 |
| 50 | firecracker | 100/100 | 2743.81 | 4257.77 | 4284.52 | 4180.28 | 4180.28 |
| 100 | hypermachine | 200/200 | 6871.04 | 7211.94 | 7238.33 | 8586.02 | 8446.25 |
| 100 | firecracker | 200/200 | 5673.70 | 5912.15 | 5943.06 | 8360.31 | 8360.31 |

HyperMachine has higher P50 and held PSS in all four profiles. Its observed P99 is lower at concurrency 50 and higher in the other profiles. These results identify cold-start latency and retained memory as continuing gaps; they do not establish a repeatable optimization or competitor-wide win. The earlier prepared-restore measurements are a different operation.

Latency includes creation, guest readiness and exact command validation. HyperMachine uses a persistent HTTP daemon; Firecracker uses fresh VMMs and Unix APIs. Both keep guests alive through validation and wait five seconds before idle PSS measurement. Incremental PSS subtracts HyperMachine’s same-batch empty-daemon baseline; Firecracker has a zero-process baseline. PSS excludes host/kernel allocations. CPU affinity constrains allowed CPUs but does not reserve them against other WSL work.

C1 has four samples per engine; its P95/P99 are the maximum and cannot estimate production tails. C8 has 32 samples, and C50/C100 have only two paired batches each. All raw samples and failures are retained. Every profile reports unchanged input hashes, no remaining guests, no cleanup errors and a reaped daemon. All benchmark input hashes were independently rechecked after completion.

The independent analysis checks attempt/pair coverage, cold-mode metadata, resource settings, affinity, finite latency, matching quantiles and cleanup. Seven deliberately malformed reports were rejected. This remains an owned local engine comparison; boxd/exe.dev and other managed endpoints are unmeasured. The automatic registration worker is disabled here because these nodes use standalone cold creation; its earlier clustered recovery fixtures are separate.

Reproduce with the archived coordinator and harnesses, preserving the frozen paths or equivalent hashes listed in source-context.json and choosing a fresh output directory.
