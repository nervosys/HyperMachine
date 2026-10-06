# Native UDP reply-buffer reuse: release experiment

The candidate is deferred and production source is unchanged. All eight owned KVM profiles passed 23 functional checks, with zero remaining guests and all owned processes reaped. Five targeted native UDP tests passed. Results do not establish an improvement across metrics or any competitor advantage.

| Native metric | Two IPv4 peers: baseline → candidate | Change | Eight IPv6 peers: baseline → candidate | Change |
|---|---:|---:|---:|---:|
| Mean block roundtrips/s | 1816.16 → 2050.23 | +12.89% | 5367.92 → 4700.03 | −12.44% |
| Mean peer P50, ms | 0.9575 → 0.9032 | −5.68% | 1.3753 → 1.4962 | +8.79% |
| Mean peer P99, ms | 3.6181 → 2.6540 | −26.65% | 3.3529 → 6.1726 | +84.10% |
| Gateway PSS after traffic, KiB | 5463.5 → 5491 | +0.50% | 5593 → 5976 | +6.85% |
| Gateway RSS after traffic, KiB | 7608 → 7606 | −0.03% | 7740 → 8126 | +4.99% |

The unchanged CLI reference rate also changed: +14.43% with two peers and −6.09% with eight. Host/order variation and shared load remain unresolved. CLI reference paths share the guest and host, with the native gateway still live; they are a confounding indicator rather than an independent control. These samples cannot isolate the causal effect of the buffer change. No confidence intervals or statistical significance claims are made.

Each family used baseline/candidate/candidate/baseline outer order, two runs per variant. Each run used CLI/native/native/CLI blocks with 2000 timed 4096-byte exchanges per peer and 10 warmups, one outstanding exchange, no retries, and complete payload/source verification. Percentiles use nearest rank; summaries average per-peer P50/P99 values and block rates rather than pooling samples. IPv4 used two peers and IPv6 eight, so their absolute rates are not a matched family comparison.

Both builds used Cargo release optimization level 3, fat LTO, one codegen unit, stripping, overflow checks and panic abort. Runtime node, control plane, CLI, kernel and image were identical immutable inputs; only the gateway executable differed. Builds completed before timed runs. The host was unpinned WSL with 24 logical CPUs; guests had one vCPU and 1024 MiB. Ingress was loopback and guest destinations IPv4; native TCP was idle. This does not measure public network performance, reboot persistence, capacity traffic, migration or competitor endpoints.

The candidate reuses an inbound reply Vec, retaining up to 65,507 bytes per peer. Gateway-only /proc/PID/smaps_rollup snapshots were sampled outside timed blocks after maximum-size UDP checks and after all four comparison blocks. They measure RSS, PSS, private and anonymous KiB at those instants, not peak or whole-stack memory. Eight-peer anonymous memory after traffic rose from 1694 to 2060 KiB (+21.61%).

Reports preserve raw latency samples, measured durations, input hashes, cleanup and resource snapshots. Summary JSON files were regenerated with the archived analyzer. Analyzer regression checks reproduce earlier development summaries and reject ten malformed data/scope cases, including mismatched CLI reference counts. Source context retains the accepted permitted source catalog and runtime input hashes. Baseline/candidate source snapshots and patch preserve the experiment; all 133 permitted isolated sources were restored to the production baseline afterward. The earlier development experiment remains separately archived.
