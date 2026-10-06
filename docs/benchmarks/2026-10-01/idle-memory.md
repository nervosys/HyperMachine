# Fixed-idle native engine memory comparison

This measurement uses the same Linux guest kernel, initrd, one vCPU, 1024 MiB
configured memory and verified shell marker for both native engines. Guests
stay alive through a five-second idle hold after the last readiness check.
The report records the actual hold, each guest's idle age at the start of the
memory read, and the duration of sequential process-memory reads.

HyperMachine's increment is its held daemon PSS minus an empty-node PSS
reading immediately before that batch, after an equal five-second empty-node
hold. Post-cleanup empty-node PSS is recorded after another five-second hold.
Firecracker's increment is the summed PSS of that batch's fresh VMM processes,
with a zero-process baseline. The benchmark controller is excluded from both.
These are different native process lifecycles, so both the held totals and the
increments matter. Negative increments are retained, rather than clamped.

The option is explicit; without it the previous readiness-hold measurement
remains the default:

```sh
python3 tools/bench-local-engines-concurrent.py \
  --hypermachine /path/to/hv2-sandboxd --firecracker /path/to/firecracker \
  --kernel /path/to/bzImage --initrd /path/to/guest.cpio.gz \
  --environment 'hardware, virtualization and load controls' \
  --concurrency 8 --pairs 3 --memory-idle-seconds 5 > memory.json
```

The recorded matrix uses the original scored HyperMachine daemon and
Firecracker 1.17.0 on shared WSL nested KVM, eight-CPU affinity and one busy
worker pinned to the first CPU. Three alternating pairs at each concurrency
provide three batch measurements per engine, not independent samples for
all guests. This is a bounded process-memory comparison after a lightweight
command; it excludes host kernel allocations, stateful oversubscription,
long-term idle behavior and managed-platform control-plane costs.

All 954 attempts passed (477 per engine); artifact identities stayed unchanged,
isolated nodes were empty after cleanup, and all owned load workers were reaped.
The table gives medians of three batch readings, in MiB of process PSS:

| Concurrency | HyperMachine held total | Firecracker held total | HyperMachine same-batch increment | Firecracker increment |
|---|---:|---:|---:|---:|
| 1 | 135.33 | 85.88 | 99.57 | 85.88 |
| 8 | 785.07 | 670.95 | 669.28 | 670.95 |
| 50 | 4230.82 | 4180.46 | 4157.67 | 4180.46 |
| 100 | 8503.60 | 8360.42 | 8320.55 | 8360.42 |

HyperMachine's held total was higher in all 12 paired batches. Its first batch
also had a larger increment at every concurrency. Some later increments were
slightly lower, while its empty-daemon footprint had grown: at concurrency 100,
the empty baseline increased from 6.77 MiB before the first batch to 183.26 and
189.99 MiB before subsequent batches, and was 191.79 MiB after final cleanup.
This establishes that retained process memory affects the marginal measurement;
it does not establish its allocation source or a memory/density win. Fresh and
warmed daemon states need separate controls in further optimization work.

Raw reports are [C1](idle-memory-c1.json), [C8](idle-memory-c8.json),
[C50](idle-memory-c50.json) and [C100](idle-memory-c100.json). The
[summary](idle-memory-summary.json) preserves all batch increments and paired
differences, including baseline and post-cleanup readings. Exact executed
harnesses, the load coordinator, matrix launcher and analysis are alongside
these reports. Readiness timings remain recorded before the idle hold; this
small cohort does not replace the larger cold-start comparison.
