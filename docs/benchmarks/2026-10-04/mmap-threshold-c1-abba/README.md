# Fixed mmap threshold: matched C1 ABBA memory candidate

Four fresh daemon cohorts run default, fixed 1 MiB threshold, fixed 1 MiB threshold, default. Each cohort has four alternating HyperMachine/Firecracker cold-create-to-exact-command pairs and five-second idle holds. All 32 engine attempts pass, inventory empties, daemons exit normally, input hashes remain unchanged and no cleanup errors occur. Only the owned HyperMachine child receives MALLOC_MMAP_THRESHOLD_=1048576. Product defaults are unchanged.

| HyperMachine result, eight pooled attempts per setting | Default | Fixed 1 MiB threshold |
| --- | ---: | ---: |
| Cold readiness P50 | 400.98 ms | 396.24 ms |
| Cold readiness P95/P99 | 489.57 ms | 778.21 ms |
| Median held PSS after five seconds | 130.06 MiB | 93.88 MiB |
| Median incremental PSS | 99.78 MiB | 83.53 MiB |
| Median same-batch empty-daemon PSS | 31.84 MiB | 10.35 MiB |

Held PSS medians by cohort are 129.39, 93.77, 93.88 and 130.06 MiB. The lower held memory repeats in both candidate cohorts, while median latency is similar and observed maximum worsens. Per-pair measurements in summary.json show candidate post-deletion PSS near 10-11 MiB and larger variable default retention. This supports a candidate memory improvement for the exact fixture; it does not identify specific allocations, prove general causality or justify a product default yet. Medians cannot be subtracted to recover another median. Four pairs per daemon do not establish long-run steady-state retention.

Firecracker held PSS stays about 85.94/85.92 MiB in default/candidate cohorts; readiness P50 is 360.92/353.69 ms and maxima 393.52/394.88 ms. HyperMachine still holds more memory and has higher median readiness than Firecracker here. With eight attempts P95/P99 are maxima, not production-tail estimates. No managed competitor, high-concurrency density, prepared creation, throughput, independent-host or across-the-board superiority is established.

The frozen daemon links glibc. Its allocator can adjust the mmap threshold dynamically; setting the threshold fixes it, and mmap allocations can be returned on free. This motivates the experiment but does not prove the cause of HyperMachine retention. [Primary glibc documentation](https://sourceware.org/glibc/manual/latest/html_node/Malloc-Tunable-Parameters.html). The prior arena-limit ABBA did not establish a memory benefit; this is a different option.

All cohorts use the same frozen release dc315d313fe2f156ab7aaa593e02c4e0d04ffa024cfb5c69c85d2780a270676e, default MMIO, disabled templates, unchanged kernel and output-drain guest, one vCPU/1024 MiB, eight-CPU affinity 0..7 and warn logging. Affinity does not reserve CPUs. PSS excludes kernel allocations. The permitted nine-file source catalog is not a complete build closure; protected root sources were excluded. No product source was changed or built.

The benchmark adds --daemon-allocator-mmap-threshold BYTES, restricted to 4096..33554432. Unset preserves the prior child environment. The report records the setting and exact harness identities. Both invalid boundaries are rejected before daemon launch, unset/configured environment behavior was checked, and the existing 17 benchmark tests pass.

Reproduce terminal.json argv sequentially in default/fixed/fixed/default order with context.json affinity and immutable inputs, retaining fresh outputs. Exact harnesses, raw reports, stderr, identities and independent analyzers are archived. python3 verify.py checks hashes, exact coverage, cold/resource configuration, actual idle holds, input/harness identities, quantiles, memory and cleanup, then compares the recomputed summary. Next gates are longer repeated lifecycle runs, higher concurrency, prepared restore and networking CPU/throughput before any product default decision.
