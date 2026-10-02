# Periodic heap reclamation: 250 ms candidate rejected

All 800 C100 attempts passed and cleaned up in four fresh-daemon AB/BA pairs.
Both variants execute the same isolated candidate binary (`8041b9e…`), with
reclamation disabled or enabled through the fixture's explicit environment.
The deployed daemon source and all defaults remain unchanged.

| Variant | Passed/attempted | P50 readiness ms | P99 readiness ms | Median held PSS MiB | Median incremental PSS MiB | Median after-cleanup PSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| Worker disabled | 400/400 | 4140 | 9593 | 8530.63 | 8523.61 | 207.27 |
| 250 ms wait between reclaim calls | 400/400 | 5129 | 19532 | 8438.78 | 8431.60 | 96.81 |

| Pair | Candidate mean readiness reduction ms | Candidate P99 reduction ms | Held PSS reduction MiB | After-cleanup PSS reduction MiB |
|---:|---:|---:|---:|---:|
| 0 | -757.14 | -989.02 | 143.95 | 148.81 |
| 1 | 63.04 | 37.18 | 164.03 | 163.41 |
| 2 | -245.93 | -366.63 | 22.56 | 54.62 |
| 3 | -5509.10 | -8844.54 | 42.49 | 71.29 |

Positive reductions favor the candidate. Held memory was lower in all four
pairs, with a median paired reduction of 93.22 MiB. Mean readiness was lower
in only one pair; median paired mean readiness was 501.53 ms worse. Pooled
P50 was 23.9% higher and P99 103.6% higher. These results reject this tested
policy as a general performance improvement. They do not reject every possible
reclamation schedule or prove the causal source of the last pair's large delay.

## Candidate and measurement scope

The prototype creates a dedicated normal thread only when the fixture supplies
`HV2_EXPERIMENT_HEAP_RECLAIM_MS`. It waits between calls to GNU
[`malloc_trim(0)`](https://man7.org/linux/man-pages/man3/malloc_trim.3.html), rather
than using a signal handler or replacing the allocator. A guard stops, unparks
and joins the worker at daemon exit, including error returns. Malformed intervals
0, 99, 60001 and `bad` exit unsuccessfully before provisioning. The candidate
requires Linux GNU libc when enabled; unsupported platforms were not runtime-tested.

Final worker summaries verify 93–143 calls per enabled daemon and absence of a
worker in disabled runs. Total measured call elapsed time ranged from 221.5 to
679.7 ms; maximum single-call elapsed time ranged from 13.6 to 41.9 ms. These
durations include possible waiting and do not measure thread CPU, allocation
contention or establish how much of the readiness difference the worker caused.

Each guest has one vCPU and 1024 MiB, identical kernel/initrd and eight cold
slots. All processes inherit eight-CPU affinity, with no separate busy worker.
The 15-second guest-answer deadline remains unchanged; whole request readiness
includes admission queueing, creation and verified command execution. The
reclaimer itself is the experimental workload.

Memory uses a five-second empty-daemon hold before the batch, a five-second
hold after every guest validates, and a five-second hold after deletion. Held
PSS includes the persistent daemon; incremental PSS subtracts that same-batch
empty daemon. Four batch aggregates are not independent per-guest samples.
Process PSS excludes kernel allocations and cannot prove density. Shared WSL
nested KVM and uncontrolled background host load limit extrapolation. All
attempts, pair differences, cleanup checks and unchanged-artifact checks remain
in the raw report. No managed/native competitor or lifecycle comparison was run.

## Verification and reproduction

The isolated generator overlays only daemon main on the prior verified clean
current runtime source. Three provisional workspace boot changes are excluded
and their clean hashes checked. `compiled-main.rs` is the exact successful
candidate source; `baseline-main.rs` is the accepted main. The initial compile
failure from missing qualified atomic types, its source and patch are retained.
The corrected candidate passed 39 Linux daemon tests and strict Linux Clippy
with the existing `too_many_arguments` exception. Seventeen burst-harness tests
passed. Windows/Linux Python `-O` checks reject eight malformed reports and
preserve cleanup errors.

The comparison helper now supports explicit idle-memory holds for callers that
request them; its default remains zero. A separate default-mode C8 control
passed all 32 attempts on the accepted daemon and verifies that no idle-memory
holds or additional memory fields were introduced into default runs.

Run `python -O tools/verify-periodic-heap.py` with this directory to verify source
and coordinator hashes, recomputed analyses, 832 total planned attempts, worker
summaries, holds, memory arithmetic, invalid intervals and cleanup. Reproduce the
prototype with the frozen generator, a matching baseline main and a fresh clean
checkout; build it on an owned GNU/Linux KVM host. Then run frozen
`bench-periodic-heap.py` with explicit `--daemon`, `--kernel`, `--initrd`,
`--pairs 4 --concurrency 100 --interval-ms 250` and a new `--output`. Preserve
failures and exits. No executable, private key or operator credential is committed.
