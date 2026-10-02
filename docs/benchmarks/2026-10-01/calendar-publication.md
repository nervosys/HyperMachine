# Durable calendar publication comparison

Matched WSL Debian release binaries in baseline/candidate/candidate/baseline order. Five fresh stores per case per run yield ten samples per variant; values are pooled medians in milliseconds. Store creation and schedule creation are excluded. The timed operation publishes occurrence files (including file sync) and commits progress. Temp stores reside on the Linux filesystem. Guest execution and networking are excluded; this is not a competitor benchmark or a crash-durability guarantee.

| Timezone | Batch limit | Baseline ms | Candidate ms |
|---|---:|---:|---:|
| UTC | 1 | 0.069250 | 0.071047 |
| UTC | 1000 | 28.127747 | 22.459715 |
| America/Los_Angeles | 1 | 0.967731 | 0.657600 |
| America/Los_Angeles | 1000 | 233.283549 | 22.712876 |

Every scheduled UTC timestamp matched across all four runs. Each sample reopened the store and verified the publication watermark and complete committed occurrence page. The candidate reuses the loaded immutable schedule and timestamps selected by the batch planner. Individually requested records still validate their timestamps. Existing occurrence records still require exact equality, so interrupted publication reconciles matching records and rejects disagreement. Exclusive progress commits and cancellation races retain their existing behavior.

Baseline is af8fac2 with the frozen diagnostic example added; candidate adds the archived source patch. The added regression test verifies that a disagreeing orphan blocks batch publication without advancing progress or overwriting its bytes. Library tests and strict all-target Clippy passed on Windows and Linux. Real VM validation of this publication optimization remains outstanding; the preceding planner optimization was separately KVM-verified.

[Raw samples](calendar-publication-comparison.json), [manifest](calendar-publication-manifest.json), [frozen example](calendar-publication-source.rs), [patch](calendar-publication.patch). Verify with `python tools/verify-calendar-publication.py`. These local file timings do not establish performance on network filesystems or Windows.
