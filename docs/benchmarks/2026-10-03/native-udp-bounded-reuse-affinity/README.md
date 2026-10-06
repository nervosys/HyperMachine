# Bounded UDP reuse with matched CPU affinity

Deferred; production unchanged. All eight release KVM profiles passed 23 functional checks, with zero guests and all owned processes reaped. Restricting the harness to CPUs 0–3 does not establish an advantage for this candidate.

| Metric | Two IPv4 peers: baseline → candidate | Change | Eight IPv6 peers: baseline → candidate | Change |
|---|---:|---:|---:|---:|
| Native roundtrips/s | 3134.6628 → 2650.4293 | -15.45% | 8624.3826 → 8421.9446 | -2.35% |
| Mean peer P50, ms | 0.6172 → 0.7203 | +16.70% | 0.8703 → 0.8870 | +1.91% |
| Mean peer P99, ms | 1.0449 → 1.9351 | +85.20% | 1.9398 → 2.2114 | +14.00% |
| Unchanged CLI roundtrips/s | 2432.4687 → 2177.4178 | -10.49% | 7383.8684 → 7052.3656 | -4.49% |
| Gateway PSS after traffic, KiB | 4954.5 → 5087.5 | +2.68% | 5413.5 → 5481.5 | +1.26% |

The unchanged CLI reference also slowed, by 10.49% and 4.49%, so shared host/load variation remains a confounder. Affinity controls placement without reserving CPUs, controlling frequency or eliminating contention. These small cohorts provide no statistical confidence or causal proof. Absolute results from earlier unrestricted experiments are not matched controls.

Every report verifies requested and observed harness affinity `[0,1,2,3]`; child-thread affinity was not separately sampled. Each family used outer baseline/candidate/candidate/baseline order, two runs per variant, and internal CLI/native/native/CLI blocks. Blocks contain 2000 timed 4096-byte exchanges per peer and ten warmups, with one outstanding exchange, no retries and complete payload/source verification. Summary percentiles average nearest-rank peer P50/P99, not pooled samples. Family peer counts differ, so absolute family rates are not comparable.

Only the gateway executable differs. Node/control/CLI/kernel/image remain immutable matched baseline inputs. The candidate retains at most 4 KiB of inbound Vec capacity between completed exchanges, releasing larger buffers; allocator memory need not return to the OS. Production source and all 133 permitted isolated sources remain baseline. Candidate tests/build logs are copied unchanged from the preceding release experiment; no new build overlapped these runs.

Cargo release uses opt level 3, fat LTO, one codegen unit, stripping, overflow checks and panic abort. The WSL host has 24 logical CPUs; guests have one vCPU/1024 MiB. Loopback ingress and IPv4 guest destinations, native TCP idle. Untimed gateway smaps_rollup snapshots after maximum UDP checks and all blocks are neither peak nor whole-stack memory. The archived analyzer validates raw samples, derived rates/percentiles, fixture scope and cleanup. Reports preserve raw evidence and hashes; driver records exact invocation. No competitor endpoints, public network, reboot, migration or capacity performance are measured.
