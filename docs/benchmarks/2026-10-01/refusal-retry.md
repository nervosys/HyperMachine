# Refused-agent connection backoff candidate (2026-10-01)

The candidate retries initial refusals after 1, 2 and 4 ms, then returns to the existing 5 ms cadence; each delay is clipped to the remaining deadline. Normal progress polling stays unchanged. Its hypothesis was that reconnecting immediately after a completed command could benefit from a shorter initial refusal delay. This is not a change to the agent protocol or a shared connection pool.

**Rejected and reverted.** All 15,600 measured commands and 636 guest preparations passed, but candidate mean latency was higher in seven of eight pairs. Both pairs were slower at concurrency 1, 50 and 100; concurrency 8 was mixed. No consistent improvement, comparative win or reliability fix is established. Production retains the prior fixed 5 ms refusal sleep.

Each cohort starts a fresh owned node with a snapshot-backed base template, identical kernel/initrd, one vCPU and 1024 MiB per guest, eight pinned host CPUs and no extra busy worker. Every guest passes command and resource preparation. Commands use the same HTTP exec API and fresh guest-agent connections in both builds. Rounds synchronize clients and retain observed start spreads. Each concurrency has two pairs, baseline/candidate then candidate/baseline, with 100 rounds at C1/C8 and 20 at C50/C100. Repeated commands within a guest and rounds within a cohort are correlated; the sample count is not an independent reliability estimate. All nodes verified no remaining sandbox records and unchanged artifacts.

| Concurrency | Pair | Commands per profile | Mean baseline / candidate (ms) | Candidate mean change (ms) | P99 baseline / candidate (ms) |
|---|---|---|---|---|---|
| 1 | 0 | 100 | 5.79 / 6.26 | +0.47 | 39.18 / 41.65 |
| 1 | 1 | 100 | 5.73 / 5.85 | +0.11 | 38.79 / 42.43 |
| 8 | 0 | 800 | 13.45 / 13.52 | +0.07 | 46.95 / 43.89 |
| 8 | 1 | 800 | 13.72 / 13.28 | -0.45 | 42.73 / 43.08 |
| 50 | 0 | 1000 | 53.16 / 57.09 | +3.93 | 365.56 / 382.56 |
| 50 | 1 | 1000 | 53.78 / 56.18 | +2.40 | 374.39 / 395.36 |
| 100 | 0 | 2000 | 119.95 / 121.30 | +1.36 | 449.58 / 459.07 |
| 100 | 1 | 2000 | 111.03 / 116.13 | +5.11 | 466.44 / 463.00 |

The first 16 cohort launches failed before any guest attempt because the build archive copy lacked executable permissions. Their nonzero setup reports and original matrix are retained separately, with zero command attempts. After applying executable bits to the two owned binaries, the corrected sweep used distinct filenames; no failed report was overwritten or counted as a passing attempt. Executable bytes were unchanged by the permission fix.

The current-source baseline and candidate used the same release flags, Cargo lock and compiler. Build metadata and output hashes, exact candidate source/tests, coordinators, both matrices and all raw cohorts are retained. The canonical original scored binary was restored after builds and the main-source timestamp invalidated to force later Cargo relinking. Candidate protocol/deadline checks passed on Windows/Linux (15 tests), and strict Linux Clippy passed before reversion. Restored-production checks are recorded separately. Shared nested hardware, two pairs and no direct refusal counters limit causal attribution; this rejects the default change for lack of a consistent measured benefit.
