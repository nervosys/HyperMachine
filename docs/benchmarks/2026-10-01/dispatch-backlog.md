# Dispatch selection with a pending backlog

The selector now requests one committed occurrence first, then uses 1024-record
pages after seeing completed history. An unclaimed or unresolved first record
can decide selection without reading a full future page. Every returned record
still receives the same committed-record, claim identity and completion checks.
Corruption in later records is detected when selection reaches those records.
No persistence format or cached cursor is introduced.

Four alternating release runs (baseline/candidate/candidate/baseline), with nine
warm-cache samples per case per run, produced these pooled medians:

| Completed | Pending | Baseline ms | Candidate ms |
| ---: | ---: | ---: | ---: |
| 0 | 1 | 0.022 | 0.018 |
| 100 | 1 | 1.206 | 1.294 |
| 1000 | 1 | 18.964 | 18.561 |
| 5000 | 1 | 91.956 | 86.184 |
| 0 | 1000 | 5.888 | 0.017 |
| 0 | 5000 | 4.972 | 0.019 |

The pending-backlog improvement is large in this local diagnostic. Completed
history still requires a full scan. At 100 completed receipts, this run measured
an increase from 1.206 to 1.294 ms: the additional initial page has a cost.
The four runs do not establish tail guarantees, end-to-end worker throughput,
or competitor superiority. Store setup and guest/network work are excluded.

The [raw samples](dispatch-backlog-comparison.json),
[manifest](dispatch-backlog-manifest.json), [benchmark patch](dispatch-backlog-benchmark.patch)
and [selection patch](dispatch-backlog-selection.patch) preserve reproduction.
Apply the benchmark patch to the manifest base commit for the baseline; apply
both patches for the candidate. Build/run with:

```sh
CARGO_TARGET_DIR=/var/tmp/hm-competitive-target cargo run --release -p hv2-jobs --example dispatch_history
```

Frozen local binaries are `/var/tmp/hm-dispatch-backlog/baseline` and `candidate`.
All 39 durable-job tests and strict all-target Clippy passed on Windows and Linux.
Long-history scaling, guest reconciliation and comparative product performance
remain incomplete.
