# Concurrent tunnel setup release ABBA

The receiver now overlaps independent guest-port opening and temporary loopback socket-pair creation. The guest-open future installs `UnregisteredStream` before returning, so a completed open remains guarded while the pair is pending. The first fresh private authorization barrier precedes both operations; the second remains after both are ready. The destination transition lock and registration ownership transfer remain intact. Pair errors drop the guard. No cache or weakened authorization is introduced.

A locked isolated release build and 64 ordinary daemon tests pass (2 KVM-only tests ignored). Four owned KVM cohorts run baseline, candidate, candidate, baseline with identical checker/kernel/image/control/CLI/gateway inputs: **204 functional checks, 512/512 scored operations and 544 raw rows** pass. All guests and tracked processes are cleaned up. The immediately preceding combined-read release is the baseline; only `forwards.rs` changes.

| Payload / path | Baseline setup P50 range (ms) | Candidate setup P50 range (ms) |
|---|---:|---:|
| 64 B / private | 2.344–2.364 | 2.273–2.290 |
| 64 B / standard | 2.010–2.046 | 1.867–1.888 |
| 65,507 B / private | 2.250–2.271 | 2.121–2.151 |
| 65,507 B / standard | 1.893–1.980 | 1.749–1.769 |

Both candidate medians are below both baseline medians for every tested setup group. This supports a local setup-latency improvement. P95/echo/total metrics do not improve uniformly; private-minus-standard setup gaps remain and do not consistently shrink. No across-the-board latency, throughput, CPU/memory or competitor win is claimed. Each row range represents two cohort medians, not a confidence interval. The shared WSL host is nonexclusive and concurrency is one. These are fresh mTLS host-to-target transports with one exact UDP echo, not end-to-end source-guest Ethernet latency or steady-state throughput. IPv6, independent-host behavior and broader races/stress remain incomplete. Cancellation while one setup future is pending is guarded by construction but has no dedicated injected-cancellation runtime test in this cohort.

Run `python3 verify-results.py` to independently recompute every median/P95 and paired difference, match raw row journals, check source delta and prove lifecycle/outage cleanup. Drivers retain original owned paths and require fresh output directories. Source catalogs avoid protected root core files; only accepted isolated core was built. Baseline/candidate binary hashes, all fixture identities and raw logs are retained. Manifest pins all payloads.
