# Eight-peer 4 KiB UDP comparison

Four fresh control API HTTPS/node mTLS/Redis/KVM stacks compare the preserved two-write CLI baseline with the combined-write candidate in ABBA order. Each has eight concurrent source peers, ten warmups per peer and synchronized intervals of 1,000 exact tagged 4,096-byte round trips per peer. The guest has one vCPU and 1 GiB. Rates exclude warmups and divide 8,000 replies by the slowest peer's measured interval.

| Metric | Baseline | Combined write | Change |
|---|---:|---:|---:|
| Cohort completion rates (round trips/sec) | 3,797.0; 3,730.1 | 4,176.9; 4,142.1 | |
| Average cohort rate | 3,763.5 | 4,159.5 | +10.5% |
| Mean of per-peer sample medians (ms) | 1.7695 | 1.8589 | +5.1% (slower) |

All ten correctness checks pass per cohort: authenticated trust/key refusal, exact empty/binary/maximum datagrams for all eight peers, malformed-frame recovery, concurrent sequence isolation and pause/resume/delete. Owned services are reaped, no guests remain and every recorded input hash is unchanged. Raw samples are archived. Per-peer P99 is mixed, so no universal tail-latency gain is claimed. Average rate and mean per-peer median describe different aspects of these distributions; the latter is not an aggregate sample median.

The combined-write change improves local completion rate here but regresses per-peer median latency. This tradeoff limits the recommendation: it is not an across-the-board performance win. Earlier two-peer trials showed higher rates with mixed medians ([64 bytes](../udp-combined-write/README.md), [4 KiB](../udp-combined-write-4k/README.md)).

The archived compare-udp-cli.py runner makes ABBA repeatable, refuses identical CLI binary contents and existing output directories, guards every input hash before/after cohorts, and requires exact measurement dimensions plus correctness/cleanup gates. Reproduce with --peer-count 8 --payload-bytes 4096 --samples-per-peer 1000 and --checker, --baseline, --candidate, --daemon, --control-plane, --kernel, --initrd and --output paths recorded in summary.json. Both checker and runner source bytes are preserved. Correctness payload probes round-trip each peer individually to avoid conflating a functional maximum-size check with an unmeasured UDP burst-loss workload; timed peers execute concurrently.

These are short closed-loop development-build measurements on unpinned WSL CPUs without concurrent compilation. Sustained capacity, overload loss, fleet tails and resource efficiency remain unverified. Boxd and exe.dev endpoints were unavailable; no competitor performance superiority is established.
