# Allocator limit: matched C1 ABBA investigation

Four fresh daemon cohorts run default, arena limit 2, arena limit 2, default. Each cohort uses four alternating HyperMachine/Firecracker cold-create-to-exact-command pairs and holds each guest for at least five seconds. All 32 engine attempts pass, with empty inventories, normal daemon exits, unchanged input identities and no cleanup errors. HyperMachine defaults remain unchanged.

| HyperMachine result, eight pooled attempts per setting | Default allocator | MALLOC_ARENA_MAX=2 |
| --- | ---: | ---: |
| Cold readiness P50 | 423.26 ms | 461.67 ms |
| Cold readiness P95/P99 | 574.70 ms | 1395.42 ms |
| Median held PSS | 114.42 MiB | 121.47 MiB |
| Median incremental PSS | 99.80 MiB | 99.55 MiB |
| Median same-batch empty-daemon PSS | 17.32 MiB | 24.04 MiB |

Held HyperMachine PSS medians by cohort are 114.69, 114.40, 135.63 and 114.33 MiB. The allocator limit does not show a repeatable memory benefit in this run and has worse pooled latency. Incremental memory is similar, while empty-daemon retention varies. summary.json retains per-pair empty baseline, held and post-cleanup PSS to make that variation visible. Medians cannot be subtracted to infer another median. These samples do not identify the retained memory's cause.

Firecracker reference cohorts pool to readiness P50 403.71 ms for default-setting cohorts and 404.00 ms for limited-setting cohorts; held PSS is 85.89 MiB in both. Its observed maxima also vary (533.79 versus 923.23 ms). Only HyperMachine receives the allocator environment variable. Small samples and shared-host effects preclude causal or production-tail claims; P95/P99 of eight attempts are maxima.

The earlier sequential eight-pair allocator exploration appeared to reduce held HyperMachine PSS, but this equal-length interleaved comparison does not support adopting the limit. Four-pair daemon cohorts are shorter than those earlier eight-pair runs and cannot establish steady-state retention. Longer controlled runs and concurrency/throughput gates remain open. No across-the-board or managed competitor win is claimed.

All cohorts use the same frozen release (dc315d313fe2f156ab7aaa593e02c4e0d04ffa024cfb5c69c85d2780a270676e), default MMIO, disabled templates, unchanged output-drain guest and kernel, one vCPU/1024 MiB, eight-CPU affinity 0..7 and warn-level logs. No production source is changed or built. Affinity does not reserve CPUs. PSS excludes kernel allocations. Permitted source-context.json is a nine-file catalog, not a complete build closure; protected root sources are excluded.

Reproduce each terminal.json argv sequentially in default/2/2/default order, with the context.json affinity and unchanged input hashes, writing outputs to fresh files. Raw reports, stderr, context, exact harnesses and independent analyzers are retained. python3 verify.py checks archive hashes, exact coverage, resource/cold configuration, actual idle holds, input/harness identities, quantiles and cleanup before comparing the derived summary. Longer-run memory is a separate gate.
