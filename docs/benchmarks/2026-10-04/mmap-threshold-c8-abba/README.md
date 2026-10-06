# Fixed mmap threshold: matched C8 ABBA memory and latency

Four fresh daemon cohorts run default, fixed 1 MiB threshold, fixed 1 MiB threshold, default. Each cohort executes four alternating engine pairs of eight simultaneously released cold starts. All 256 exact-command engine attempts pass (64 per engine per setting), with empty inventory, normal daemon exits, unchanged input hashes and no cleanup errors. Each batch holds eight validated guests for at least five seconds before PSS sampling.

| HyperMachine C8 result | Default | Fixed 1 MiB threshold |
| --- | ---: | ---: |
| Passed attempts | 64/64 | 64/64 |
| Cold readiness P50 | 487.07 ms | 547.33 ms |
| Cold readiness P95 | 529.44 ms | 892.35 ms |
| Cold readiness P99 | 571.25 ms | 912.32 ms |
| Median held PSS, eight guests | 888.01 MiB | 680.16 MiB |
| Median incremental PSS | 698.02 MiB | 665.63 MiB |
| Median same-batch empty-daemon PSS | 200.74 MiB | 14.80 MiB |

Held HyperMachine PSS medians by cohort are 870.71, 680.16, 680.18 and 903.94 MiB. The memory benefit repeats in both fixed-threshold cohorts, but both have worse median and P95 readiness than either default cohort. This extends the C1 memory candidate to this C8 fixture while demonstrating a concurrency latency cost. Defaults remain unchanged; this is not an accepted across-the-board optimization. Per-pair empty baseline, held and post-cleanup memory is retained in summary.json. Medians cannot be subtracted to infer another median; process PSS includes shared daemon costs and excludes kernel allocations.

| Firecracker 1.17.0 C8 reference | Default-setting cohorts | Fixed-setting cohorts |
| --- | ---: | ---: |
| Passed attempts | 64/64 | 64/64 |
| Cold readiness P50 | 437.32 ms | 428.42 ms |
| Cold readiness P95 | 1269.15 ms | 498.33 ms |
| Cold readiness P99 | 1425.79 ms | 507.51 ms |
| Median held PSS, eight VMMs | 670.87 MiB | 670.98 MiB |

The threshold is applied only to HyperMachine; Firecracker receives no allocator experiment. Its varying reference tails expose shared-host variability. HyperMachine still has higher median readiness and held PSS in both settings. These short burst samples establish no sustainable capacity, production-tail ranking, long-run density, independent-host or managed competitor win. P99 of 64 samples is the maximum; attempts within a batch are dependent. PSS has eight batch observations per engine per setting, not 64 independent memory samples.

Inputs are the same frozen current release dc315d313fe2f156ab7aaa593e02c4e0d04ffa024cfb5c69c85d2780a270676e, kernel and output-drain guest used in C1. Default MMIO, --no-template, one vCPU/1024 MiB per VM, eight-CPU affinity 0..7, warn logging, and no cold admission budget apply to every cohort. Affinity does not reserve CPUs. The permitted source catalog covers nine files, not complete build closure; protected root sources were excluded. No product source was changed or built.

MALLOC_MMAP_THRESHOLD_=1048576 is a glibc-specific fixed-threshold experiment. The benchmark flag is opt-in, with unset preserving the default environment. The C1 archive documents allocator rationale and option validation. Exact current harness sources and their hashes are retained here, so older benchmark executables/data remain separately scoped.

Reproduce terminal.json argv sequentially in default/fixed/fixed/default order using context.json affinity and unchanged input identities, writing fresh outputs. python3 verify.py validates hashes, all attempts, resource/cold configuration, actual idle holds, input/harness identities, latency quantiles, memory and cleanup against the independently recomputed summary. Longer lifecycle retention, prepared restore and network CPU/throughput remain open gates before any product default decision.
