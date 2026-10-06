# Matched release-build UDP recovery comparison

The prior [development-build experiment](../udp-peer-recovery-performance/README.md) measured a 5.6% completion-rate regression after peer task-ID cleanup. This follow-up builds both CLIs with the same locked dependencies and release profile (opt-level 3, fat LTO, one codegen unit, overflow checks, stripped binary, panic=abort). Two accepted isolated source copies differ only in crates/hm-cli/src/sandbox_vm_udp.rs. Source hashes were checked before and after builds/measurements. Root protected core modifications were not read or executed.

Four fresh HTTPS/mTLS/Redis/KVM stacks run baseline/candidate/candidate/baseline, with eight concurrent peers and 1,000 measured tagged 4 KiB replies each after ten warmups. Guest resources are one vCPU/1 GiB; daemon/control binaries and guest image stay fixed at their earlier development builds, so this compares release CLI changes rather than a wholly release-built service stack. There is no concurrent compilation during measurement.

| Metric | Before cleanup | With cleanup | Change |
|---|---:|---:|---:|
| Average cohort rate (round trips/sec) | 4,277.6 | 4,220.9 | -1.3% |
| Mean per-peer sample medians (ms) | 1.8178 | 1.8296 | +0.7% (slower) |

All ten correctness and cleanup checks pass in every cohort. The release candidate independently passes the one-peer EOF recovery fixture: capacity refusal while live, a new source after closure, the same source after another closure and clean interrupt/server cleanup. P99 is generally higher in the candidate; no tail improvement or equivalence is claimed. These four short unpinned WSL closed-loop cohorts show a smaller measured regression than the separate development experiment, without establishing production overhead, statistical significance or its mechanism.

The release profile's panic=abort means a task panic terminates the CLI process. The earlier panic-injection unit test uses an unwinding test profile; it proves task bookkeeping cleanup when Tokio returns a panic JoinError, not release-process survival. Cancellation and ordinary EOF remain separate cases. This qualification applies to the earlier recovery guide and evidence.

An initial shared-target candidate build produced the identical binary hash as baseline and was excluded before benchmarking. Touching the candidate CLI modules and library/main entrypoints forced recompilation without changing source bytes; the rebuilt candidate has a distinct hash and is the only candidate used here. The immutable baseline and rebuilt candidate paths/hashes are recorded in summary.json. Build logs are preserved. Source context remains unchanged after rebuilding.

Reproduce cargo build --offline --locked --release -p hm-cli --bin hm in the two recorded source contexts, preserve separate binaries, require distinct hashes and verify exact source bytes. Prefer separate target directories for each source to avoid cross-checkout cache reuse. Run the archived compare-udp-cli.py with --peer-count 8 --payload-bytes 4096 --samples-per-peer 1000 and recorded inputs, then check-udp-cli-recovery.py with the release candidate and a fresh output directory. Raw cohort samples, checker/runner source and input hashes are archived.

This establishes a reproducible local release-CLI comparison and the bounded-peer correctness benefit, with a remaining measured performance tradeoff. Sustained load, resource costs, wholly release-built services and competitor recovery/performance remain unverified. No competitor endpoints were available.
