# Admission tuning and matched native engines

These two subsequent cohorts retain all 1600 attempts. They execute the exact
current-source daemon from the admission archive (`b7d2aba…`), without rebuilding
or incorporating the three provisional boot changes. All attempts passed,
artifacts remained unchanged, inventories were empty and owned processes stopped.

## Four-slot budget versus disabled

Four fresh-daemon AB/BA pairs at 100 concurrent requests use identical binaries,
one-vCPU/1024-MiB guests, eight CPUs in affinity and no added CPU worker. The
15-second guest readiness deadline is unchanged. Queue time is included.

| Setting | Passed/attempted | P50 readiness ms | P99 readiness ms |
|---|---:|---:|---:|
| Disabled | 400/400 | 8201 | 11461 |
| Four slots | 400/400 | 5654 | 15628 |

Four slots improved paired mean readiness in all four pairs, but worsened P99
by 36.4%. This rejects the hypothesis that four slots would improve the tail
in this cohort. It does not establish relative superiority over eight slots:
those settings were tested in separate cohorts, with visibly changing baseline
latencies on the shared host. No default or recommended universal budget changed.

## Eight slots versus Firecracker 1.17.0

Four alternating native-engine AB/BA pairs at 100 concurrent guests use the same
kernel/initrd, boot arguments, one vCPU, 1024 MiB and eight-CPU affinity. Both
engines use a 15-second guest-answer deadline. HyperMachine is one persistent
HTTP daemon with an explicit eight-slot budget; Firecracker starts independent
VMMs through Unix APIs without this budget. Queueing is part of HyperMachine
creation time. Readiness ends after a verified shell command; each batch's guests
remain alive until validation and a five-second idle hold finish.

| Engine/configuration | Passed/attempted | P50 ms | P99 ms | Median held PSS MiB | Median incremental PSS MiB |
|---|---:|---:|---:|---:|---:|
| HyperMachine, eight slots | 400/400 | 3530 | 12857 | 8617.59 | 8326.38 |
| Firecracker, uncapped | 400/400 | 5678 | 9168 | 8360.78 | 8360.78 |

HyperMachine's P50 was 37.8% lower and paired mean readiness was lower in four
of four pairs. Its P99 was 40.2% higher and held PSS was 3.1% higher.
Incremental PSS subtracts the same-batch empty HyperMachine daemon; Firecracker
has a zero-process baseline. The small incremental difference does not prove
whole-host density: process PSS excludes kernel allocations, and only four
batch aggregates per engine contribute to these medians.

This is a measured local median-readiness improvement over a native engine,
with a worse tail and held memory. Firecracker is a building block, rather than
a matched managed sandbox service. Shared WSL nested KVM, inherited host load,
different control APIs and limited repetitions constrain interpretation. There
is no managed endpoint, dedicated bare-metal, snapshot, fleet, reliability SLA
or across-the-board competitor win in these records.

`off-on` freezes the original coordinators for the four-slot cohort. `engines`
freezes the extended native coordinator, which records the actual daemon argument
vector, configured budget and terminal daemon exit. `build-context.json` and
`compiled-main.rs` preserve the previously verified binary provenance.
Raw JSON includes every attempt, memory batch and cleanup result. Percentiles
are nearest-rank and conditional on successful, cleaned-up attempts.

Verify with `python -O tools/verify-cold-budget-tuning.py` and this directory.
Reproduce the native run on an owned Linux x86-64 KVM host with the frozen
`engines/bench-local-engines-concurrent.py`, explicit binary/image arguments,
`--pairs 4 --concurrency 100 --memory-idle-seconds 5 --cold-start-concurrency 8`
and the recorded affinity. Preserve stdout as a new JSON file; retain nonzero
exits and failures. No private key, executable or operator credential is archived.
