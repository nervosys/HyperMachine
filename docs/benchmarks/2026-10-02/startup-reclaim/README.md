# One-time startup reclaim candidate

The isolated candidate adds one GNU/Linux `malloc_trim(0)` call after initial
template preparation and before listening or cluster registration. It runs
only when a prepared template exists. There is no periodic thread, allocator
preload or per-request reclaim. The production main and default daemon remain
unchanged. This candidate is not a shipped option or an adopted optimization.
The [Linux man-pages documentation](https://www.man7.org/linux/man-pages/man3/malloc_trim.3.html)
describes the GNU function as thread-safe and operating on free heap pages.

The earlier [sham-controlled probe](../prepared-reclaim/README.md) demonstrated
reclaimability after named-source preparation. This candidate intentionally
tests the earlier startup placement, so that probe alone cannot establish
its benefit. The generated patch, exact accepted and candidate main, release
build log and context are frozen. Only main differs across the 550 accepted
source files; the three provisional core edits were excluded and their clean
hashes are checked. The accepted executable is the prior verified daemon;
the candidate is a new release build of the isolated source. Executable hashes
and the candidate compiler/build command are retained. ELF compiler comments
match for both executables (Rust 1.95.0 and GCC 14.2.0); this does not
independently reconstruct every flag of the earlier accepted build.

Each outer pair alternates fresh baseline/candidate daemons AB/BA. Every
variant prepares its own HyperMachine named source and Firecracker full
source, then runs two internal engine pairs in HM/FC and FC/HM order.
Resources, commands, images, readiness limits, five-second held-memory wait
and cleanup follow the [matched prepared benchmark](../prepared-engines/README.md).
Timing begins with child creation and ends after verified file, live-process
environment and isolated child-write checks. Source preparation and startup
trim are outside restore timing. This does not measure node startup latency.

All sources are resident or cache-warm; no cache drop is performed. Per-batch
PSS includes the whole daemon, with the immediately preceding empty daemon
subtracted for incremental PSS. Firecracker controls start fresh VMMs and use
zero-process baselines. PSS excludes kernel memory and unmapped page cache.
The persistent-daemon and fresh-VMM control paths differ. Shared WSL nested
KVM and uncontrolled host background load limit generalization. A separate
unowned fuzz build was observed on this host during candidate compilation;
the candidate release build finished before any measured cohort started.

| Cohort | HyperMachine variant | Passed / planned | P50 ms | P95 ms | P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---|---|---|---|---|---|
| c8 | baseline | 96/96 | 88.278 | 230.431 | 247.378 | 90.171 | 47.755 |
| c8 | candidate | 96/96 | 79.301 | 216.807 | 246.094 | 62.678 | 47.746 |
| c8-repeat | baseline | 96/96 | 80.230 | 462.683 | 533.445 | 90.406 | 48.537 |
| c8-repeat | candidate | 96/96 | 79.115 | 174.423 | 178.591 | 61.896 | 48.285 |

c8: candidate paired means are lower in 4/6 pairs, paired P99 in 3/6, and held PSS in 6/6.

c8-repeat: candidate paired means are lower in 6/6 pairs, paired P99 in 6/6, and held PSS in 6/6.

| Cohort | Firecracker control side | Passed / planned | P50 ms | P99 ms |
|---|---|---|---|---|
| c8 | baseline | 96/96 | 75.448 | 224.422 |
| c8 | candidate | 96/96 | 86.202 | 151.566 |
| c8-repeat | baseline | 96/96 | 76.722 | 291.025 |
| c8-repeat | candidate | 96/96 | 62.878 | 101.679 |

All 776 scored attempts, including eight smoke attempts and all Firecracker
controls, pass with cleanup verified. The median held-PSS reduction is
27.494 MiB in the first main cohort and 28.509 MiB in the repeat. All twelve
matched pairs reduce held PSS, while incremental guest PSS is nearly unchanged: 
the improvement is fixed node overhead, not a demonstrated per-guest
fleet-density gain.

The repeat has lower aggregate and paired candidate timing, but Firecracker
control means also fall on all six candidate-side pairs (1.63 to 101.31 ms).
The first cohort has mixed paired tail rankings. Shared-host drift and small
maximum-based tails prevent attributing a repeatable latency improvement to
trim or establishing latency neutrality. The candidate remains experimental.


The single-guest, one-pair smoke cohort is a harness and cleanup check, excluded
from the main ranking. All raw smoke and main reports, including Firecracker
controls, are retained. The analyzer checks one startup trim marker in every
candidate node log and none in baseline logs. It rejects mapping-diagnostic
cohorts from these rankings. Latencies are conditional on successful guest
validation and cleanup; memory is conditional on complete successful batches.
Nearest-rank P99 is the maximum when an aggregate has 96 samples, and each
paired 16-sample P99 is its maximum. These small tails are descriptive.

An observed memory reduction does not establish latency neutrality or a
managed-service win. Larger single-guest and higher-concurrency cohorts, sustained allocations,
lifecycle stress, non-GNU behavior and independent-host validation remain
adoption gates. Windows verification below
checks the evidence and analyzer, not the candidate runtime. No across-the-board
performance claim or runtime change follows from this experiment.

From the repository root:

```sh
python3 -O tools/verify-startup-reclaim.py docs/benchmarks/2026-10-02/startup-reclaim
python3 -O tools/check-startup-reclaim.py docs/benchmarks/2026-10-02/startup-reclaim/c8-report.json
```

To reproduce, generate only inside a clean isolated copy of the exact accepted
source, build the candidate, and supply the input hashes from `manifest.json`:

```sh
python3 tools/experiment-startup-reclaim.py /isolated/source/crates/hv2-sandboxd/src/main.rs --patch /owned/path/candidate.patch
# In /isolated/source:
cargo build --release --locked -p hv2-sandboxd
# From the repository root:
python3 tools/bench-startup-reclaim.py --baseline /path/accepted-hv2-sandboxd --candidate /path/candidate-hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --pairs 6 --concurrency 8 --output /new/path/report.json
python3 -O tools/analyze-startup-reclaim.py /new/path/report.json --output /new/path/analysis.json
```

The coordinator requires new output paths, retains each completed cohort,
and delegates owned VM/process cleanup to the frozen prepared driver. No
executable or snapshot memory image is committed.
