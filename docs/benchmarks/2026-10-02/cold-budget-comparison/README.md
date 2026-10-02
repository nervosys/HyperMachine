# Direct comparison of eight and sixteen cold slots

Two fresh-daemon AB/BA cohorts compare the same previously verified current
binary with eight versus sixteen slots at 100 concurrent cold requests. Each
cohort includes four pairs and all 800 planned attempts. All 1600 attempts
passed; artifacts remained unchanged, guest inventories were empty and owned
daemons stopped. No runtime default or readiness deadline changed.

| Cohort | Budget | Passed/attempted | P50 readiness ms | P99 readiness ms | Sixteen-slot paired means faster | Sixteen-slot paired P99 lower |
|---|---:|---:|---:|---:|---:|---:|
| First | 8 | 400/400 | 4134 | 13001 | — | — |
| First | 16 | 400/400 | 4959 | 8874 | 2/4 | 4/4 |
| Repeat | 8 | 400/400 | 4409 | 15083 | — | — |
| Repeat | 16 | 400/400 | 4350 | 13263 | 2/4 | 2/4 |

Sixteen slots lowered pooled P99 by 31.7% initially and 12.1% in the repeat.
Its initial P50 was 19.9% higher; repeat P50 was 1.3% lower. Paired mean
readiness improved in only four of eight pairs and paired P99 in six of eight.
The first result's consistent paired tail improvement did not fully repeat.
These data do not establish a generally superior setting or justify changing
the disabled default. Operators still need workload-specific measurements.

Both cohorts use eight available CPUs, one vCPU and 1024 MiB per guest, the
same kernel/initrd and no added CPU worker. Guest readiness has the unchanged
15-second deadline after launch; total request readiness includes admission
queueing, creation and verified shell execution. Guests remain alive until
batch validation, then are deleted. Prior cohorts with different settings or
baseline conditions cannot substitute for this direct comparison.

## Sixteen slots versus native Firecracker

A subsequent matched native cohort used four alternating AB/BA pairs, the
same one-vCPU/1024-MiB guests and eight-CPU affinity. HyperMachine ran one
persistent HTTP daemon with sixteen slots; Firecracker 1.17.0 launched fresh
VMMs through Unix APIs without this budget. Both used the same kernel/initrd,
boot arguments and 15-second guest-answer deadline. Each batch retained its
guests through validation and a five-second idle hold.

| Engine | Passed/attempted | P50 readiness ms | P99 readiness ms | Median held PSS MiB | Median incremental PSS MiB |
|---|---:|---:|---:|---:|---:|
| HyperMachine, sixteen slots | 400/400 | 3807 | 11741 | 8658.38 | 8331.90 |
| Firecracker, uncapped | 400/400 | 5579 | 11042 | 8361.85 | 8361.85 |

HyperMachine's P50 was 31.8% lower, with lower paired means in four of four
pairs. Its P99 was 6.3% higher and held PSS was 3.5% higher. Incremental PSS
subtracts the same-batch empty HyperMachine daemon; Firecracker has a zero-process
baseline. Four aggregates per engine, excluding kernel allocations, cannot prove
density or whole-host efficiency. All attempts passed, unchanged artifact checks
and guest/process cleanup passed. Across all three new cohorts, 2400/2400 planned
attempts passed.

This is another local median-readiness improvement, with a worse tail and held
memory. The earlier eight-slot native comparison is a separate cohort with
different baseline latencies; it cannot establish the cause of differences
between the settings. No managed-service, fleet or across-the-board win follows.

The coordinator now accepts `--baseline-limit` as well as `--candidate-limit`.
Zero leaves that variant disabled; 1–1024 sets its explicit daemon option.
Raw rows retain the actual daemon argument vector. The analyzer validates both
budgets, binary identity, cold-template metadata, actual resources, attempt
identities and finite latencies. It emits paired P50/P99 differences along with
paired means. Failed pairs retain all attempts and unavailable paired latencies.

Windows and Linux checks under Python `-O` reject nine deliberately malformed
reports and preserve cleanup errors. All five earlier admission analyses,
including the cohort with 96 creation failures, recompute without alteration.

The archive freezes coordinators, analyzers and the exact compiled daemon main
with its prior clean-source build context. Three provisional workspace boot
changes are excluded. There are no executables, private keys or credentials.
Percentiles are nearest-rank and conditional on successful, cleaned-up attempts.
Shared WSL nested KVM and uncontrolled background host load limit generalization;
these are cold native measurements, not managed endpoint or fleet benchmarks.

Run `python -O tools/verify-budget-comparison.py` with this directory to check
file hashes, recomputed analyses, every planned attempt, configured arguments,
resource/deadline identity and cleanup. Reproduce the direct comparison with
the frozen `bench-cold-start-limit.py`, explicit binary/image paths,
`--pairs 4 --concurrency 100 --baseline-limit 8 --candidate-limit 16` and a new
output path. Reproduce the native cohort with the frozen
`bench-local-engines-concurrent.py`, `--pairs 4 --concurrency 100
--memory-idle-seconds 5 --cold-start-concurrency 16`, explicit owned paths,
the recorded affinity and a new stdout JSON file. Preserve failures and exits.
