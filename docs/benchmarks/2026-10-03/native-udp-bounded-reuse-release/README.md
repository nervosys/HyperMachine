# Bounded UDP reply reuse: release experiment

Deferred; production source is unchanged. Eight owned KVM profiles passed all 23 functional checks with zero guests and all owned processes reaped. Five targeted UDP tests passed before the release build. No performance or memory advantage is established.

| Metric | Two IPv4 peers: baseline → candidate | Change | Eight IPv6 peers: baseline → candidate | Change |
|---|---:|---:|---:|---:|
| Native roundtrips/s | 2891.5388 → 2401.1198 | -16.96% | 7294.6118 → 7237.8901 | -0.78% |
| Mean peer P50, ms | 0.6693 → 0.7784 | +16.31% | 1.0456 → 1.0501 | +0.43% |
| Mean peer P99, ms | 1.0298 → 2.1501 | +108.78% | 1.9049 → 1.9258 | +1.10% |
| Unchanged CLI roundtrips/s | 2244.3308 → 1481.2845 | -34.00% | 6437.8376 → 6456.8118 | +0.29% |
| Gateway PSS after traffic, KiB | 5391 → 5680 | +5.36% | 5805 → 5940 | +2.33% |
| Gateway anonymous after traffic, KiB | 1336 → 1394 | +4.34% | 1726 → 1662 | -3.71% |

The unchanged two-peer CLI reference rate fell 34%, with substantially worse tails. Host/order variation and shared load prevent attributing all differences to this code change. Eight-peer differences are small and lack statistical confidence. Release results differ substantially from the earlier experiment's baseline rates, so comparing candidate variants across experiment dates would be unmatched. Use within-experiment summaries only.

Each family ran baseline/candidate/candidate/baseline, two runs per variant, with internal CLI/native/native/CLI blocks. Each block used 2000 timed 4096-byte exchanges per peer plus ten warmups, one outstanding exchange, no retries and complete payload/source verification. Nearest-rank per-peer P50/P99 values and block rates are averaged; samples are not pooled. IPv4 has two peers and IPv6 eight, so absolute family rates are not a matched comparison.

The candidate retains at most 4096 bytes of inbound Vec capacity between successful exchanges and drops larger buffers after sending. Maximum frames remain supported and allocate transient larger buffers. Dropping capacity does not force the system allocator to return memory to the OS. Outbound queue framing, budgets and timeouts are unchanged.

The gateway was built with the accepted Cargo release profile (opt level 3, fat LTO, one codegen unit, stripping, overflow checks, panic abort) in the isolated checkout. Identical immutable baseline node/control/CLI, kernel and image were used throughout; only gateway bytes differ. The build finished before the timed runs. The unpinned WSL host has 24 logical CPUs; each guest has one vCPU and 1024 MiB. Ingress is loopback, guest destinations IPv4 and native TCP idle. Gateway-only smaps_rollup snapshots after maximum-size UDP checks and after all blocks are untimed observations, not peak or whole-stack memory measurements.

Reports retain raw samples, measured durations, fixture hashes, cleanup and memory snapshots. The archived analyzer verifies scope, raw-derived percentiles/rates and cleanup. The driver and candidate build/test logs preserve reproduction inputs. All 133 permitted isolated sources were restored before benchmarking; production UDP source remains identical to the baseline snapshot. No competitor endpoints were measured, and no public-network, reboot, migration or capacity performance claim follows.
