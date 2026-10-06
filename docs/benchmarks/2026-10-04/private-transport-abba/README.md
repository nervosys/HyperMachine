# Interleaved baseline/candidate private transport comparison

Four sequential owned KVM cohorts run in **baseline–candidate–candidate–baseline** order, using identical current checker, mTLS client, target guest image, kernel and dev compiler profile. All **512/512 scored operations**, 32 warmups and **32 functional checks per cohort** succeed. Every cohort deletes its guests and reaps all fixture processes.

Each payload/path receives 32 scored fresh TCP/TLS operations following two warmups. Within each cohort, private/standard first path alternates per pair. The table measures the nearest-rank median of each pair's private setup time minus standard setup time; it is not a difference of unrelated medians.

| Cohort | Payload | Median paired setup overhead ms |
|---|---:|---:|
| 0-baseline | 64 B | 1.258 |
| 0-baseline | 1 MiB | 1.528 |
| 1-candidate | 64 B | 0.929 |
| 1-candidate | 1 MiB | 1.176 |
| 2-candidate | 64 B | 0.906 |
| 2-candidate | 1 MiB | 1.275 |
| 3-baseline | 64 B | 1.290 |
| 3-baseline | 1 MiB | 1.511 |

Both candidate cohorts have lower paired median overhead than both surrounding baseline cohorts. The observed gap is 1.26–1.29 versus 0.91–0.93 ms at 64 B, and 1.51–1.53 versus 1.18–1.28 ms at one MiB. This strengthens local evidence that concurrent independent authorization reads reduce receiving setup overhead. Private setup remains slower than standard. The test does not establish a competitor win, P99 superiority, a confidence interval or performance across workloads.

The baseline is the previously frozen private gateway dev binary (`795880f95b4a0d201c785de101fb81656a88691aa851a8168973d493706502f1`); the candidate is the concurrent-lookup dev binary (`d7c2d34157cde7268a6f9e1cd71dd0f32c6a05dbec4bc3c6e32a8c19de0f4e2e`). Build logs and receiver source snapshots are preserved. All other runtime input hashes match. The candidate overlaps the atomic membership snapshot with two fresh live-node lookups while preserving both setup authorization barriers and active-stream revocation.

Concurrency is one; both daemon nodes share one WSL host. Setup spans connection initiation through HTTP 101. Source guest DNS/gateway/connector functionality is exercised separately by the lifecycle checks but excluded from timing. Echo uses sequential 16-KiB send/receive chunks; throughput is payload MiB divided by full echo duration with the payload in each direction, not isolated one-way bandwidth. Whole-daemon held PSS is not incremental connection memory. CPU includes vCPU/background work and 10-ms tick quantization. This reduces monotonic host-drift bias but does not remove shared-host noise or prove exclusive resource costs. Release-mode, independent-host, high-concurrency and guest-origin optimization comparisons remain pending.

Run `python3 analyze.py` to recompute all raw ranks, throughput, CPU sums, paired order, common input hashes, overhead differences and cleanup checks. Output should reproduce `analysis.json`. Drivers preserve original absolute owned-input paths; reproduction elsewhere requires equivalent frozen inputs and fresh output paths. All 138 current permitted root/isolate source pairs, accepted isolated core and runtime hashes were verified; protected root core files were neither read nor built. Production is unchanged from the previously tested optimization.
