# Reclamation guarded by cold admission: not adopted

This isolated candidate tries to acquire the entire configured cold-start
semaphore before calling GNU `malloc_trim(0)`. If any permit is unavailable,
it skips that attempt. While reclamation holds all permits, a new admitted
cold boot waits. The goal was to retain memory benefits without overlapping
the provisioning work affected by the rejected unguarded periodic candidate.

## Functional evidence

The live fixture recorded seven reclamation calls, 74 failed reservation attempts
and zero overlap with cold-admission events. The two-slot phase admitted and
released eight cold guests, and the one-slot phase admitted and released two,
including an intentional failed boot. Every remaining guest executed a marker
after reclamation. Snapshot resume bypassed the occupied cold slot, the failed
boot released its permit and a subsequent cold guest succeeded. Inventory and
owned-process cleanup passed. Worker guards stopped and joined on daemon exit.

This protects **admitted cold bring-up through guest-agent readiness**. It does
not serialize warm restore, execution, networking/environment setup after
readiness or dynamic template construction against reclamation. Startup template
construction occurs before this worker starts. These checks do not prove complete
cancellation cleanup or a general latency benefit for lifecycle operations.

The generator round-trip reproduced the exact compiled source after an escaping
error in a generator simplification was corrected. The candidate passed 39 Linux
daemon tests and strict Linux Clippy with the existing `too_many_arguments`
exception. Linux/Windows Python `-O` checks accept the real trace and reject six
invalid admission/worker-summary traces. Malformed intervals and a configured
worker without a cold-start budget are refused before provisioning.

## Four-pair C100 comparison

All 800 planned attempts are retained: **725 passed and 75 failed**. The same
candidate binary (`06536b7…`) runs with its worker disabled or configured for a
250 ms wait between attempts. Both variants use eight cold slots, one vCPU and
1024 MiB, the same kernel/initrd and eight-CPU affinity. No separate busy worker
was added; background host load on shared WSL nested KVM was uncontrolled.

| Variant | Passed/attempted | Successful P50 ms | Successful P99 ms | Valid memory batches | Median held PSS MiB | Median after-cleanup PSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| Worker disabled | 325/400 | 6162 | 26140 | 3 | 8512.76 | 203.47 |
| Guarded worker | 400/400 | 7276 | 22342 | 4 | 8511.99 | 163.76 |

These pooled percentiles exclude failed attempts. Memory medians have different
valid-batch counts and are not a matched four-pair memory comparison. They
establish no reliability or performance win.

| Pair | Complete | Candidate mean readiness reduction ms | Candidate P99 reduction ms | Held PSS reduction MiB | After-cleanup PSS reduction MiB |
|---:|---|---:|---:|---:|---:|
| 0 | yes | -635.44 | -1263.14 | 165.72 | 195.50 |
| 1 | no: 75 disabled-worker failures | unavailable | unavailable | unavailable | unavailable |
| 2 | yes | 1831.79 | 3588.78 | -7.81 | 48.16 |
| 3 | yes | -351.85 | -1515.55 | -4.97 | 26.85 |

Positive reduction favors the candidate. Of three complete pairs, one improved
mean readiness and a different one improved held memory; **none improved both**.
After-cleanup PSS was lower in all three complete pairs, but held memory and
readiness did not improve consistently. This policy is not adopted as a general
held-memory optimization. The tested guard's non-overlap property does not resolve
the memory/performance tradeoff.

## Failure and measurement scope

The failed disabled-worker batch contains 74 client `timed out` errors during
creation and one HTTP 503 reporting no guest-agent answer within 15 seconds.
The frozen HTTP helper uses a 30-second socket timeout per request. Guest readiness
retains its 15-second deadline after launch; measured whole-request readiness
includes admission queueing, creation and verified shell execution. Queueing can
exhaust a client timeout even when the guest's deadline has not expired. The cause
of this batch's delays is unproven; no retry or replacement cohort hides it.

Memory follows fixed five-second empty-daemon, held-guest and after-cleanup holds.
Incremental PSS subtracts the same-batch empty daemon. Incomplete pairs retain raw
attempts and unavailable paired statistics. Process PSS excludes kernel memory
and does not prove density. Empty inventory and owned-daemon exit checks passed
throughout; cleanup accounting covers known IDs, observed inventory and process
shutdown. Resource cleanup for the 74 unreturned/cancelled creations was not
independently measured while the daemon remained alive.

No Firecracker or managed endpoint ran in this cohort. The enabled pass count
does not establish a reliability fix. Snapshot/failure-recovery correctness in
the separate functional fixture does not establish lifecycle throughput or tails.
No deployed daemon source, heap policy or defaults changed.

## Archive and reproduction

The archive preserves every captured attempt, worker summary, full functional
logs, error classification, successful source/build/test/lint records, exact
compiled and reproduced main sources and clean-source provenance. Three provisional
workspace boot files were excluded and their clean hashes checked. No executable,
private key or operator credential is committed.

Run `python -O tools/verify-guarded-heap.py` with this directory to verify identities,
recomputed analyses, all 800 attempts, the 75 failures and functional admission
traces. A passing archive verifier explicitly reports the failed benchmark cohort.
Reproduce the generator on its matching baseline main, build the isolated source
on an owned GNU/Linux KVM host and run frozen `bench-periodic-heap.py` with explicit
`--daemon`, `--kernel`, `--initrd`, `--pairs 4 --concurrency 100 --interval-ms 250`
and a new output path. Keep failures and nonzero exits. The source, rather than the
generic coordinator name, distinguishes this guarded policy from the older one.
