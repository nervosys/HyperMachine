# Remaining borrowed-boot profiles

The unchanged isolated candidate and accepted executables from the
[borrowed-boot archive](../borrowed-boot/README.md) pass 816 additional scored
restores at concurrency one and 50. Held PSS is lower in all four outer pairs.
Latency differs by profile, and Firecracker controls also shift.

| Profile | Accepted HM mean / P99 (ms) | Candidate HM mean / P99 (ms) | Accepted / candidate held PSS (MiB) | Firecracker control P99, accepted / candidate cohorts (ms) |
| --- | ---: | ---: | ---: | ---: |
| One guest | 35.025 / 36.102 | 39.748 / 44.909 | 70.449 / 56.769 | 58.704 / 55.055 |
| 50 guests | 490.535 / 916.818 | 359.779 / 484.021 | 200.717 / 185.563 | 1603.935 / 516.420 |

The single-guest profile contains only **four samples per variant/engine**.
Its P99 is the maximum observed sample, not a stable production tail estimate.
Both single-guest outer pairs have worse candidate means and P99. Both 50-guest
pairs have better candidate means and P99, but their Firecracker means also
improve. These comparisons do not identify a causal latency change.

All guest-state, clock/RNG, source-integrity, and cleanup checks pass. Each fresh
daemon runs two internal HM/FC pairs; baseline/candidate outer order is AB/BA.
The profiles use shared WSL nested KVM, eight inherited CPU-affinity slots,
unchanged recorded guest inputs, and five-second memory holds outside latency
timing. There is no allocator probe, tuning, or diagnostic logging. PSS excludes
kernel memory and unmapped page cache. Samples and batches are not independent
hosts, and preparation is excluded from restore latency.

Combined with the prior archive, all **4,144 scored restores** pass, and held PSS
is lower in all ten outer pairs. Candidate means are lower in five pairs; P99
is lower in seven. This strengthens the fixture memory result, while preserving
the evidence against a universal latency improvement. No default runtime
optimization is adopted and no managed competitor win is established.

Linux and Windows replay checks accept each actual report and reject ten
damaged evidence contracts. The frozen helpers, raw per-cohort reports, validated
analyses, and build binding are archived here. Exact accepted-source recovery,
the candidate patch, tests, compiler details, and source overlays remain in the
prior archive. `manifest.json` binds this profile extension without changing
the earlier frozen evidence.

The next comparison uses one executable with selectable owned/borrowed buffers,
verifying activation before scoring. This will remove separate executable builds
as a confound; the owned mode is a counterfactual within the refactor and will
not be represented as the original accepted binary.
