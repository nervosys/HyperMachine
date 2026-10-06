# UDP peer recovery performance cost

This matched ABBA experiment compares the preserved combined-write CLI before peer task-ID cleanup with the preserved verified cleanup binary. Four fresh HTTPS/mTLS/Redis/KVM stacks use eight concurrent peers, ten warmups each, then synchronized intervals of 1,000 exact 4 KiB replies per peer. Both binaries are development builds; the guest has one vCPU and 1 GiB. Input hashes are unchanged throughout.

| Metric | Before cleanup | With cleanup | Change |
|---|---:|---:|---:|
| Average cohort rate (round trips/sec) | 4,170.9 | 3,938.3 | -5.6% |
| Mean per-peer sample medians (ms) | 1.8516 | 1.9721 | +6.5% (slower) |

All ten correctness and cleanup checks pass in every cohort. The preceding [recovery evidence](../udp-peer-recovery/README.md) establishes slot release after injected panic/cancellation and real CLI EOF reconnection. This comparison exposes a development-build throughput/median regression after the fix, so it supplies no performance-win claim for the revised binary. P99 is mixed. The exact mechanism is not established; a matched release-build comparison and profiling remain required before attributing the cost to task bookkeeping or generalizing it to production.

Reproduce with the archived ABBA runner, the paths and hashes in summary.json, --peer-count 8 --payload-bytes 4096 --samples-per-peer 1000 and a fresh output directory. Candidate source and source context are preserved here; baseline source is the candidate archived in [the original combined-write comparison](../udp-combined-write/README.md). The mutable shared CLI binary was replaced by a test-profile build after the prior regression tests. A hash gate refused it before any cohort ran. Rebuilding the development profile restored the exact verified candidate hash, which was then copied to a distinct immutable executable before this experiment.

These are short unpinned WSL closed-loop measurements without concurrent compilation. They establish neither sustained capacity nor competitor performance. Raw reports, samples, binary hashes and runner/checker source are archived. Boxd and exe.dev endpoints remain unavailable.
