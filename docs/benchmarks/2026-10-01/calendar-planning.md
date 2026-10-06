# Local calendar planner diagnostic and UTC optimization

Forward search in the named `UTC` zone now uses the existing UTC calendar
planner, avoiding civil gap/fold enumeration while preserving the named-zone
1970-9999 range contract. Other named zones continue through the general
candidate search. Tests compare UTC results across ordinary dates, leap years,
partial minutes and the upper calendar bound.

Four alternating release runs (baseline/candidate/candidate/baseline) used nine
samples per case, giving 18 samples per binary per case. Pooled medians:

| Expression | Timezone | Occurrences | Baseline ms | Candidate ms |
| --- | --- | ---: | ---: | ---: |
| `* * * * *` | UTC | 1 | 0.160 | 0.001 |
| `* * * * *` | UTC | 1000 | 159.878 | 0.728 |
| `* * * * *` | America/Los_Angeles | 1 | 0.183 | 0.183 |
| `* * * * *` | America/Los_Angeles | 1000 | 198.275 | 193.796 |
| `30 1 * * *` | America/Los_Angeles | 367 | 0.215 | 0.223 |
| `0 0 29 2 *` | America/Los_Angeles | 1 | 0.002 | 0.002 |

The diagnostic times `due_occurrences` only: no storage, guest execution, network
or competitor endpoint. It checks stable, strictly ordered bounded output on
every sample; the comparison additionally checks matching counts and first/last
timestamps. All cases use the same inclusive start before the 2026 Los Angeles
fold. The daily case spans 366 days and the leap-day case spans eight leap-year
lengths. These are isolated local timings, not scheduler throughput or guest
start latency. Dense non-UTC catch-up remains expensive and needs optimization.
No improvement is attributed to the small non-UTC timing differences.

The [initial raw run](calendar-planning-run-1.json),
[matched comparison](calendar-planning-comparison.json),
[manifest](calendar-planning-manifest.json), [frozen example source](calendar-planning-source.rs)
and [candidate patch](calendar-planning-utc.patch) preserve the evidence.
To reproduce, use the manifest base commit and copy the frozen example to
`crates/hv2-jobs/examples/calendar_planning.rs`. Build the baseline, then apply
the candidate patch and rebuild separately:

```sh
CARGO_TARGET_DIR=/var/tmp/hm-competitive-target cargo run --release -p hv2-jobs --example calendar_planning
```

Frozen binaries are `/var/tmp/hm-calendar-planning/baseline` and `candidate`.
`python tools/verify-calendar-planning.py` verifies archive identities and sample
coverage; optional `--baseline` and `--candidate` check actual binary identities.
All 49 durable-job tests and strict all-target Clippy passed on Windows and Linux.
The provisional boot edits do not enter this jobs-only example.
