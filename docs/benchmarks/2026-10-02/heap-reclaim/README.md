# Owned heap-reclaimability diagnostic

All 432 planned guest attempts passed across C8 and C100. Every guest also
executed a verified marker after the heap probe. Held observations kept all
expected KVM VM/vCPU handles; after teardown, VM/vCPU handles and mappings of
at least one GiB were absent. Inventories were empty, owned daemons stopped
and all input/source hashes remained unchanged. No runtime setting was adopted.

Two fresh-daemon AB/BA pairs per profile compare a no-op command with a call to
[`malloc_trim(0)`](https://man7.org/linux/man-pages/man3/malloc_trim.3.html).
The documented GNU operation attempts to release free heap pages. Both variants
load the same diagnostic shared library into only their owned daemon, using an
explicit child environment and inherited socket pair. A dedicated normal thread
handles commands; no signal handler, remote endpoint or allocator replacement is
used. The helper's added thread/socket and allocation history affect the process,
so these are diagnostics rather than scored normal-daemon comparisons.

Each daemon uses the previously verified current binary (`b7d2aba…`), eight cold
slots, one vCPU and 1024 MiB per guest, identical kernel/initrd and eight host CPUs
in affinity. There is no added busy worker. Guests are held for five seconds
before the first observation and five seconds after the probe, then checked again
before release. After deletion and another five-second hold, a second probe and
five-second observation measure residual heap reclaimability.

| Profile/pair | Held PSS reduction, trim MiB | Held PSS reduction, no-op MiB | Sham-adjusted held reduction MiB | Sham-adjusted empty reduction MiB | Held trim call ms |
|---|---:|---:|---:|---:|---:|
| C8 / 0 | 0.215 | -0.008 | 0.223 | 2.590 | 0.339 |
| C8 / 1 | 95.492 | -0.004 | 95.496 | 2.645 | 6.354 |
| C100 / 0 | 99.551 | -0.083 | 99.634 | 34.566 | 11.182 |
| C100 / 1 | 111.721 | -0.109 | 111.830 | 34.445 | 12.975 |

Positive reduction means before minus after. Sham adjustment subtracts the
same-pair no-op reduction. C8 varied markedly. At C100, both pairs observed
about 100–112 MiB reclaimable while guests remained running, and about 34.5 MiB
more after teardown. This identifies free-heap reclamation as a candidate for
the held-memory gap; it does not identify allocation stacks, prove every retained
byte is free, establish a leak fix or close the competitor memory gap.

The operation executes after readiness, while all guests are held. Its measured
call durations do not establish effects on concurrent provisioning, execution
latency, allocator contention or tail readiness. Subsequent allocation costs,
snapshot/resume/fork and long-lived workloads remain untested. Periodic or
post-boot reclamation would require a separate candidate and matched memory,
latency and lifecycle evaluation before adoption. The earlier rejected arena-limit
experiment remains rejected; this intervention uses a different mechanism.

Raw snapshots preserve process memory, mapping categories, the largest mappings,
thread/descriptor counts and KVM handles. Procfs reads are sequential and
non-atomic; process PSS excludes kernel allocations and cannot prove whole-host
density. Shared WSL nested KVM and limited repetitions constrain generalization.
No Firecracker or managed endpoint was run in these diagnostic cohorts; no
latency, reliability, density or competitor performance win is established.

`heap-reclaim-probe.c` compiled with strict GCC warnings. `build-context.json`
records exact source/helper hashes, compiler arguments, GCC 14.2.0 and glibc 2.41.
Rebuilding the frozen C source produced the identical helper SHA-256. The exact
compiled daemon main and its prior build context exclude the three provisional
workspace boot changes. No executable, shared library, private key or credential
is committed. Windows and Linux Python `-O` validation checks reject nine malformed
reports and preserve cleanup errors.

Run `python -O tools/verify-heap-reclaim.py` with this directory to verify hashes,
recomputed observations, all planned attempts, guest checks, KVM handles and
cleanup. On an owned Linux x86-64 glibc/KVM host, compile the helper with the
recorded arguments, then run the frozen `diagnose-heap-reclaim.py` with explicit
`--daemon`, `--kernel`, `--initrd`, `--helper`, `--pairs 2`, `--concurrency 8`
or `100`, and a new `--output`. Preserve every attempt and nonzero exit.
