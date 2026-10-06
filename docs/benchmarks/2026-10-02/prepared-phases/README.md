# Client phases of prepared startup

Two fresh cohorts each run four matched HM/FC AB/BA pairs at concurrency 100,
using the accepted daemon and the [matched guest clock/RNG contract](../prepared-contract/README.md).
All 1600 restores pass state validation, resource checks and cleanup. No
production binary changes and no additional guest RPCs are introduced. The
coordinator adds monotonic timestamps at existing response boundaries.

HyperMachine `create_and_notice` ends when the HTTP create response returns.
It includes transport/queueing, VM provisioning, snapshot restore, startup
and the acknowledged guest clock/RNG notice. Its `exec` phase covers the
existing state command through validated output. Firecracker records process
startup and snapshot load, then guest connection/notice acknowledgement,
then state-command execution. Resource GETs and held-memory reads remain
outside total readiness timing. Every successful sample's nonnegative finite
phases must sum to its total latency within floating-point tolerance.

| Cohort | Engine | Passed/planned | Total P50 ms | Total P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---|---|---|---|---|
| c100 | hypermachine | 400/400 | 662.329 | 1268.887 | 325.398 | 244.438 |
| c100 | firecracker | 400/400 | 828.901 | 1019.471 | 280.244 | 280.244 |
| c100-repeat | hypermachine | 400/400 | 831.938 | 1155.310 | 343.868 | 263.346 |
| c100-repeat | firecracker | 400/400 | 841.141 | 1215.073 | 286.590 | 286.590 |

| Cohort | Engine phase | Mean ms | P50 ms | P99 ms |
|---|---|---|---|---|
| c100 | hypermachine: create_and_notice | 455.608 | 448.307 | 845.865 |
| c100 | hypermachine: exec | 251.470 | 218.276 | 706.167 |
| c100 | firecracker: process_and_load | 230.865 | 184.435 | 576.183 |
| c100 | firecracker: connect_and_notice | 316.127 | 314.786 | 580.864 |
| c100 | firecracker: exec | 266.569 | 271.205 | 429.119 |
| c100-repeat | hypermachine: create_and_notice | 542.696 | 553.916 | 811.957 |
| c100-repeat | hypermachine: exec | 291.319 | 266.488 | 650.959 |
| c100-repeat | firecracker: process_and_load | 229.088 | 206.828 | 644.975 |
| c100-repeat | firecracker: connect_and_notice | 382.507 | 377.771 | 676.622 |
| c100-repeat | firecracker: exec | 235.635 | 232.996 | 466.173 |

c100: 24/400 HyperMachine attempts exceed or equal one second. Creation/readiness accounts for 60.25% of their summed time; execution accounts for 39.75%.

c100-repeat: 14/400 HyperMachine attempts exceed or equal one second. Creation/readiness accounts for 64.12% of their summed time; execution accounts for 35.88%.

The predefined slow threshold is total readiness of at least 1000 ms. Those
samples remain in all readiness statistics. Time fractions divide the sum
of each phase among slow samples by their summed total readiness; they are
not averages of per-sample ratios. Phase percentiles need not sum to the
corresponding total percentile.

Neither cohort reproduces the earlier accepted-daemon 5.739-second P99 from
the linked comparison. These runs therefore do not localize that specific
multi-second event or establish that it is fixed. They show that command
execution is a material part of the observed one-second tails, alongside
creation/readiness. No sole bottleneck, server CPU/queue delay, guest scheduling
cause or optimization benefit is established. Server-side stage observation
is the next investigation; client timing alone cannot distinguish these causes.

Both engines use one vCPU, 1024 MiB, identical images and state commands, with
64 fresh entropy bytes and guest clock resynchronization before execution.
Sources are resident or cache-warm and hashed outside timing; no caches are
dropped. A requested five-second hold follows all validation, with varying
guest ages. Total PSS covers the persistent HyperMachine daemon or all live
Firecracker processes; incremental HyperMachine PSS subtracts its immediately
preceding empty-node baseline. PSS excludes kernel memory and unmapped page
cache, and does not establish fleet density. Latencies are conditional on
successful validation and cleanup; memory is conditional on complete batches.

The driver uses eight pinned CPUs on a shared WSL/KVM host with uncontrolled
background load. HTTP-daemon/fresh-VMM paths and kernel arguments differ.
No managed-platform or universal performance claim follows. The frozen build
context binds the accepted daemon, excluding the three provisional core edits.
All earlier reports and their frozen harnesses remain unchanged.

From the repository root:

```sh
python3 -O tools/verify-prepared-phases.py docs/benchmarks/2026-10-02/prepared-phases
python3 -O tools/check-prepared-analysis.py tools/analyze-prepared-engines.py docs/benchmarks/2026-10-02/prepared-phases/c100-report.json
```

Linux and Windows verify source bindings, profiles, all attempts, state/clock/RNG
validation, phase sums, recomputed analyses and cleanup. Twenty-one malformed
cases are rejected with assertions disabled, including missing phases,
negative/NaN duration and a sum inconsistent with total readiness.

To reproduce on a Linux KVM host with inputs from `manifest.json`:

```sh
python3 tools/bench-prepared-engines.py --hypermachine /path/hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --pairs 4 --concurrency 100 --output /new/path/report.json
python3 -O tools/analyze-prepared-engines.py /new/path/report.json --output /new/path/base-analysis.json
python3 -O tools/analyze-prepared-phases.py /new/path/report.json --output /new/path/phases.json
```

Fresh output paths are required. The coordinator owns and cleans up its guest
processes. Executables and snapshot memory files are not committed.
