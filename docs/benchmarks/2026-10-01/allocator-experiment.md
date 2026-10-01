# Counterbalanced allocator-limit experiment

This is a candidate evaluation, not a production configuration change. The
same scored HyperMachine daemon, Firecracker 1.17.0, Linux kernel and initrd
are used throughout. Each guest has one vCPU and 1024 MiB and must execute its
unique shell marker with a successful status before readiness is accepted.

Four blocks alternate the allocator-profile order: default/arena2,
arena2/default, default/arena2, arena2/default. Each profile launches a fresh
HyperMachine daemon and runs two engine pairs at concurrency 100, with the
engine order HM/FC then FC/HM. The arena2 profile sets MALLOC_ARENA_MAX=2 only
in the owned HyperMachine daemon environment. The default child environment
is explicit and has no inherited allocator settings. Firecracker is unchanged.

The driver and engines share eight-CPU affinity on nested WSL/KVM. Each cohort
owns one busy worker pinned to the first CPU and records its liveness and
cleanup. Guest readiness is measured before the fixed five-second memory hold.
Memory reports include held process PSS, a same-batch empty-daemon baseline,
individual idle ages, sequential read duration and post-cleanup empty-node PSS.

Failures remain in the denominator. Successful latency quantiles exclude
failed attempts. A paired block's latency and memory means are compared only
when all of that engine's attempts passed and cleaned up in both allocator
profiles. Guest rows in a batch share load and are not independent trials.
Four blocks, variable shared-host load, and different daemon/VMM lifecycles
limit extrapolation. Neither long-term density nor snapshot/resume/fork
behavior is tested by these cold cohorts.

The allocator flag is available only as an explicit benchmark option:

```sh
python3 tools/bench-local-engines-concurrent.py \
  --hypermachine /path/to/hv2-sandboxd --firecracker /path/to/firecracker \
  --kernel /path/to/bzImage --initrd /path/to/guest.cpio.gz \
  --concurrency 100 --pairs 2 --memory-idle-seconds 5 \
  --daemon-allocator-arena-max 2 --environment 'documented host and load' \
> allocator-arena2.json
```

## Result: arena limit not adopted

All 3200 attempts are retained; 3029 passed. The failed cohorts kept their
nonzero exit status and `success:false`. Artifact checks, isolated-node cleanup
and owned load-worker teardown passed in all eight profiles.

| Profile | Engine | Passed / attempts | Successful P50 / P99 ready (ms) | Valid memory batches | Median held idle PSS (MiB) |
|---|---|---:|---:|---:|---:|
| Default | HyperMachine | 701 / 800 | 9078.10 / 14149.63 | 7 / 8 | 8491.94 |
| Arena limit 2 | HyperMachine | 800 / 800 | 8957.91 / 13816.16 | 8 / 8 | 8543.10 |
| Default-profile cohort | Firecracker | 799 / 800 | 6155.65 / 15244.54 | 7 / 8 | 8360.10 |
| Arena-profile cohort | Firecracker | 729 / 800 | 7713.55 / 13113.50 | 7 / 8 | 8361.25 |

Firecracker receives no allocator override in either cohort. Its varying
readiness results reinforce the need to retain failure denominators and avoid
attributing differences to HyperMachine's setting alone. The latency quantiles
are conditional on success, not failure-inclusive readiness guarantees.

| Block | HyperMachine mean ready delta, arena2 minus default (ms) | Held idle PSS delta (MiB) | Post-cleanup PSS delta (MiB) |
|---|---:|---:|---:|
| 0 | +185.69 | +21.53 | +21.47 |
| 1 | -1210.97 | +76.40 | +75.82 |
| 2 | Excluded: incomplete HyperMachine block | Excluded | Excluded |
| 3 | +620.73 | +47.87 | +34.78 |

The three complete paired HyperMachine blocks all used more held and
post-cleanup process PSS with the arena limit. Only one had a lower mean
readiness time; the median paired mean-time delta was +185.69 ms. The smaller
sequential concurrency-8 diagnostic did not generalize to this workload.

The default HyperMachine failure was 99 readiness timeouts in block 2's first
batch. Firecracker had 71 timeouts in block 2's arena-profile first batch and
one timeout in block 3's default-profile first batch. Their causes are not
established here. The arena-limited HyperMachine pass count does not prove a
reliability fix. Incomplete memory batches are excluded; complete blocks are
reported per engine, so Firecracker block 3 is also excluded from its paired
analysis while HyperMachine block 3 remains valid.

No production or deployment defaults changed. The option remains available for
explicit experiments; no repeatable performance, memory, density or competitor
win is claimed. A source review found that boot-size checking currently
materializes copied kernel/initrd regions to compute the highest address; that
is a targeted allocation lead for separate implementation and verification,
not a cause established by these measurements.

The [manifest](allocator-blocks.json) links all eight raw profiles and their
exit statuses/hashes. The [summary](allocator-summary.json) preserves failed
rows and per-engine paired block exclusions. Exact executed harnesses,
coordinator, launcher and analysis are stored alongside these files.
