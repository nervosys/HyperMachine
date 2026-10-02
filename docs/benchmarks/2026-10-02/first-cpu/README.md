# First backend-call CPU and wall-time diagnosis

All 436 native guest attempts passed across concurrency 1/8/100, with 218
HyperMachine IDs matched to dispatch, first-exit, readiness and CPU records.
Each first return was an OUT to PCI CONFIG_ADDRESS (`0xCF8`). Empty inventory,
guest/process cleanup and unchanged artifacts passed. The previously retained
100-guest timeout was not reproduced or fixed.

The diagnostic reads Linux `CLOCK_THREAD_CPUTIME_ID` before and after the first
backend operation on the dedicated owner thread. This clock reports calling-thread
CPU consumption ([Linux manual](https://www.man7.org/linux/man-pages/man2/clock_gettime.2.html)).
The KVM backend blocks on that same thread. The wall interval encloses both clock
reads and excludes the preceding first-call log. Measurements use integer
nanoseconds; missing clocks remain unavailable rather than becoming zero.
The reads execute only on the first call when the cold diagnostic target is
enabled. Unsupported platforms report no CPU value. No readiness deadline,
retry policy or VM exit handling changed.

| Concurrent guests | HyperMachine samples | Mean bracket wall ms | Mean thread CPU ms | Mean elapsed not charged to thread ms | Median CPU/wall ratio |
|---|---:|---:|---:|---:|---:|
| 1 | 2 | 174.60 | 174.53 | 0.07 | 99.96% |
| 8 | 16 | 195.02 | 188.82 | 6.20 | 98.65% |
| 100 | 200 | 2967.31 | 218.42 | 2748.90 | 7.30% |

At C100, median bracket wall time was 2986.04 ms and median CPU consumption
218.03 ms. Much of this measured interval was not charged as CPU time to the
calling thread. Wall minus CPU does not separate scheduling, blocking and
nested-host effects. The CPU sample includes all thread CPU inside the bracket,
without separating guest instructions from host kernel work. In particular,
it is not a measurement of decompression alone.

The result supports a next experiment limiting simultaneous cold boots while
retaining the original guest readiness budget. It establishes no optimal limit
or performance improvement. C1 has only two samples; this diagnostic does not
establish a repeatable concurrency trend or competitor CPU ranking. The accepted
benchmark baseline remains unchanged. Firecracker's first-backend CPU interval
was not measured; its raw control attempts are retained without a CPU comparison.

Two alternating native-engine pairs per profile used the same kernel/initrd,
1 vCPU/1024 MiB per guest, eight host CPUs in affinity, no added CPU worker and
15-second guest deadlines. The clean source excludes the three provisional boot
edits. Tracing affects timing, so this cohort is excluded from scored latency
rankings. Reproduce through the frozen `refresh-local-engines.py --first-cpu
--profiles 1,8,100 --pairs 2` with the diagnostic wrapper and explicit artifact
paths. The matrix retains exact input hashes.

A separate negative collection used the previous first-exit binary without CPU
fields. All four guest attempts and cleanup passed, but measurement failed with
`missing or invalid first backend CPU clock`. Raw reports and logs remain in
`missing-clock/`; the collector did not substitute a zero or discard the run.
That collection overlapped the release build and contributes no timing claim.

Linux core tests passed 2294 with two ignored; Windows all-target compilation
passed. Nine parser tests passed on each platform. Linux daemon Clippy passed
with the existing `too_many_arguments` exception. Sources, build context and
logs are archived. Run `python -O verify.py` to verify hashes, every clock pair,
all 436 positive and four negative attempts, and the recomputed analysis.
