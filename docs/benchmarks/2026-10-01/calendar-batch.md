# Calendar catch-up batch diagnostic

Matched local release binaries, run on the same WSL Debian host in baseline/candidate/candidate/baseline order. Nine warm samples per case per run (18 per variant); milliseconds below are pooled medians. This measures planning only, excluding storage, VM execution and networking. It is not a competitor benchmark.

| Expression | Timezone | Limit | Baseline ms | Candidate ms |
|---|---|---:|---:|---:|
| `* * * * *` | UTC | 1 | 0.000822 | 0.000832 |
| `* * * * *` | UTC | 1000 | 0.746692 | 0.380331 |
| `* * * * *` | America/Los_Angeles | 1 | 0.184077 | 0.185294 |
| `* * * * *` | America/Los_Angeles | 1000 | 193.692588 | 0.461554 |
| `30 1 * * *` | America/Los_Angeles | 1000 | 0.224966 | 0.029602 |
| `0 0 29 2 *` | America/Los_Angeles | 1 | 0.001633 | 0.001503 |

Every scheduled UTC timestamp matched exactly between all four runs. The candidate enumerates civil dates once per batch and retains at most 1024 earliest UTC timestamps. Both ambiguous fold instants remain eligible; nonexistent civil minutes are skipped. Catch-up can span more than 400 years when required to fill a bounded batch; an empty search retains the single planner's 400-year horizon. One-occurrence requests return the already selected timestamp directly. Coalescing is unchanged.

Baseline source is commit 8749288 with the diagnostic's full timestamp reporting added. The patch and frozen diagnostic source reproduce the candidate. Fifty library tests passed on Windows and Linux, including a repeated single-search oracle across folds, gaps, half-hour transitions, skipped days, historical second offsets, day-field OR matching and a 574-year window. Strict all-target Clippy passed on both systems. Live VM validation of this optimization remains outstanding.

Run `python tools/verify-calendar-batch.py` to check archived hashes and exact occurrence equality. Raw evidence: [calendar-batch-comparison.json](calendar-batch-comparison.json).
