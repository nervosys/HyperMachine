# Sixteen versus thirty-two cold-boot slots

Thirty-two slots are not adopted as a default or a generally preferred setting. In this matched C100 cohort, 16 slots passed 400/400 guest attempts; 32 slots passed 381/400, with 19 guest-readiness timeouts in the first candidate batch. All 800 planned attempts are retained. Both settings used the same accepted daemon, kernel and initrd. Cleanup left empty inventories and all eight owned daemons exited with code zero; final input hashes were unchanged.

| Cold slots | Passed / attempted | Successful P50 readiness ms | Successful P99 readiness ms | Median held daemon PSS MiB | Median incremental PSS MiB |
|---|---:|---:|---:|---:|---:|
| 16 | 400 / 400 | 9202.79 | 29501.37 | 8541.92 | 8534.81 |
| 32 | 381 / 400 | 7171.98 | 23862.91 | 8630.55 | 8623.50 |

Latency percentiles condition on successful, cleaned-up attempts; the 19 failures remain in the totals and are excluded from latency rankings. Memory medians condition on complete successful runs (four baseline runs and three candidate runs). The failed pair has no paired latency or memory difference. Of the three complete pairs, 32 slots lowered means in two and P99 in all three, but held PSS was higher in all three by 86.39, 91.12 and 52.33 MiB. These mixed latency results, failures and memory cost do not establish an improvement across the board. The current disabled admission default is unchanged. Failure association with a setting does not isolate a cause on this shared host.

The retained failure records are HTTP 503 guest-readiness errors at the unchanged 15-second guest deadline, not unattempted requests. Total readiness includes admission queueing, creation and verified shell execution, so it can exceed 15 seconds. The API create timeout also remains unchanged. No failed attempts or warmups are discarded.

Four pairs run fresh daemons in AB/BA order on eight CPUs, with 1 vCPU / 1024 MiB per guest and no added CPU worker. Empty-daemon memory is read after a five-second idle interval; guests remain alive for five seconds after all readiness checks before held process PSS is read. Incremental PSS subtracts that run's empty baseline. Actual holds, read duration and guest idle ages are recorded. PSS excludes kernel memory and is not a fleet-density measurement. Windows/WSL background load remains uncontrolled, and results cannot be compared causally to older cohorts or managed vendors.

A separate eight-request setup cohort passed all 32 attempts. Both limits exceed that concurrency, so this is measurement/setup validation rather than a test of admission contention. The new analyzer rejected 12 malformed reports on Linux and Windows under Python `-O`, including shortened holds, invalid PSS, mismatched baseline subtraction, missing attempts and incorrect settings/resources. The analyzer retains incomplete pairs in failure totals and withholds their paired differences.

The daemon is the previously accepted object-backup artifact, SHA256 `2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f`. Its compiled main and prior build context are frozen here; all three provisional workspace core changes were excluded from that build. No runtime source or executable was changed for this experiment. Coordinators, analyzers, raw reports, checks and hashes are preserved in this archive.

Reproduce with the frozen `bench-cold-start-limit.py`, explicit identical `--baseline` and `--candidate` daemon paths, the recorded `--kernel` / `--initrd`, and `--pairs 4 --concurrency 100 --baseline-limit 16 --candidate-limit 32 --memory-idle-seconds 5 --output NEW_PATH`. Verify with `python -O tools/verify-cold-budget-memory.py docs/benchmarks/2026-10-02/cold-budget-32`.
