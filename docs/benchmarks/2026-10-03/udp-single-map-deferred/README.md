# Deferred single-map UDP recovery experiment

This candidate replaces the separate peer/task maps with one peer record containing its sender and task ID. Successful joins return their peer directly; failed joins scan the bounded peer map for the matching task ID. It preserves panic/cancellation cleanup in unwinding builds and ordinary EOF recovery, but was not retained because its matched release benchmark does not demonstrate the intended performance improvement.

All 146 CLI library tests pass, including injected panic/cancellation cleanup with exactly one peer removed per completed task. The real release CLI one-peer EOF fixture passes four checks and three upstream sessions. All ten checks pass in each HTTPS/mTLS/Redis/KVM cohort, including eight concurrent peers, empty/binary/maximum payloads, authentication/trust refusal and lifecycle cleanup. No guests or owned processes remain.

| Metric | Retained two-map recovery | Experimental single-map recovery | Change |
|---|---:|---:|---:|
| Average cohort rate (round trips/sec) | 4,242.6 | 4,191.5 | -1.2% |
| Mean per-peer sample medians (ms) | 1.8319 | 1.8333 | +0.08% |

ABBA runs use eight peers, 1,000 exact tagged 4 KiB replies per peer after ten warmups, one guest vCPU/1 GiB, fixed development-build daemon/control services and release-built CLI binaries. Candidate P99 is worse in its first cohort and mixed in the second. No reliable throughput, median or tail improvement is established. Fewer maps alone does not establish a resource-efficiency gain; memory/CPU costs were not measured. Short unpinned WSL runs cannot establish mechanism or statistical significance.

Candidate source, test/build logs, EOF recovery report, raw cohort samples and input hashes are preserved. The retained baseline comes from [the verified release recovery comparison](../udp-peer-recovery-release/README.md). Reproduce with the archived ABBA runner and --peer-count 8 --payload-bytes 4096 --samples-per-peer 1000 using summary.json inputs and a fresh output directory. Release panic=abort terminates the process; injected panic-cleanup checks use the unwinding test profile.

After this experiment the root and accepted isolated UDP module were restored byte-for-byte to the retained two-map implementation. The experimental binary remains preserved separately and is not promoted. No competitor endpoint was available, and no competitor performance win is claimed.
