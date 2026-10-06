# Linux boot sizing without temporary image copies

The experimental Linux `LoadedBoot::highest_address` path calculated region endpoints after the same validation and initrd placement used for guest loading. It avoided constructing and discarding copied kernel/initrd buffers just to determine required memory. Actual guest loading still constructs its boot regions. Raw and Multiboot sizing were unchanged. The candidate was reverted after the high-concurrency comparison; production code remains at the parent implementation.

The allocation regression test observes zero requested allocation bytes on a valid 16 MiB kernel plus 8 MiB initrd fixture. Its control observes the copied regions, and both produce the same endpoint. Layout tests cover 27 combinations, including optional/empty initrd, long command lines, and RAM above the PCI hole; rejection checks compare the same validation errors. All 144 boot regressions, the Linux and Windows allocation tests, and strict Linux Clippy passed.

The real guest [state check](boot-sizing-state.json) passed checkpoint restore, pause/resume and two state-preserving forks, with artifact identity and clean teardown. Its [exact coordinator](boot-sizing-state-coordinator.py) is diagnostic evidence, excluded from timing.

The runtime comparison uses the current parent commit `5c91bef`, including recent product features, rather than the older scored daemon. Both builds use the same lockfile, compiler, release flags, kernel and initrd. [Build metadata](boot-sizing-build-metadata.json) identifies the binaries and actual source bytes. The shared Cargo target initially reused the baseline core; that unscored optimized artifact was replaced after forcing both changed core sources to rebuild. The original scored daemon was restored before measurements. Different checkout paths may affect embedded source paths.

The [matrix](boot-sizing-matrix.py) runs two alternating profile blocks at concurrency 8 and four at concurrency 100. Each profile uses a fresh daemon, two alternating engine pairs, eight host CPUs and one pinned CPU worker. Guests have one vCPU and 1024 MiB RAM, identical kernel/initrd and unchanged readiness limits. The five-second idle hold, per-batch empty baseline and post-cleanup measurement follow the existing concurrent harness. Every attempt and nonzero cohort exit is retained. [Analysis](analyze-boot-sizing.py) checks hashes, exits, resources and teardown; incomplete paired blocks are excluded per engine, while their failed rows remain in totals.

This is cold creation on shared nested WSL/KVM. Conditional successful latency percentiles, short-idle process PSS and burst batches do not establish an SLA, sustained throughput, whole-host memory density or a managed-product win. Broader feature and performance gaps remain open.

## Results and decision

All 3,328 attempts passed: 1,664 per engine, with 832 per engine/profile. All 12 cohorts exited successfully; image/harness identity, held validation, empty-node teardown and owned CPU worker checks passed. Raw cohort reports are listed with hashes and exits in the [complete matrix](boot-sizing-results-matrix.json). Both concurrency summaries retain all attempts.

| Concurrency | Profile | Engine | Passed/attempted | P50 readiness ms | P99 readiness ms | Median held idle PSS MiB | Median post-cleanup PSS MiB |
|---:|---|---|---:|---:|---:|---:|---:|
| 8 | Parent | hypermachine | 32/32 | 517.76 | 718.88 | 791.45 | 126.30 |
| 8 | Parent | firecracker | 32/32 | 425.61 | 604.54 | 671.00 | — |
| 8 | Candidate cohort | hypermachine | 32/32 | 501.50 | 716.56 | 723.36 | 58.23 |
| 8 | Candidate cohort | firecracker | 32/32 | 427.90 | 591.67 | 670.91 | — |
| 100 | Parent | hypermachine | 800/800 | 6883.79 | 11560.69 | 8557.74 | 244.09 |
| 100 | Parent | firecracker | 800/800 | 5608.38 | 10830.40 | 8360.81 | — |
| 100 | Candidate cohort | hypermachine | 800/800 | 8570.61 | 11905.27 | 8511.33 | 198.05 |
| 100 | Candidate cohort | firecracker | 800/800 | 6519.14 | 10899.02 | 8361.50 | — |

At [concurrency 8](boot-sizing-c8-summary.json), both paired blocks reduced HyperMachine mean readiness by about 17 ms and held/post-cleanup PSS. At [concurrency 100](boot-sizing-c100-summary.json), the candidate was slower in three of four paired blocks: +1635.69, +1274.75, +1714.75 and −438.04 ms; the median paired change was **+1455.22 ms**. Held PSS changes were +0.04, −6.18, −146.67 and +48.30 MiB; the median paired change was only −3.07 MiB. Post-cleanup changes were +0.62, −7.32, −147.31 and +49.42 MiB. Aggregated median memory differences are not a consistent block-level improvement.

Firecracker readiness also varied between profile cohorts; the cause is not established on this shared host. These results do not isolate a causal performance regression, but they fail the evidence needed to adopt the candidate as an improvement. **The runtime change was reverted.** Exact candidate [Linux source](boot-sizing-linux-optimized.rs), [LoadedBoot source](boot-sizing-loaded-optimized.rs), [allocation test](boot-sizing-allocation-test.rs), and baseline sources are archived. Micro-level allocation savings and passing state checks did not justify the observed high-concurrency tradeoff. Neither profile beats Firecracker across readiness and held memory.
