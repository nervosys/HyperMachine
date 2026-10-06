# Removing redundant schedule reads during dispatch selection

`next_vm_occurrence` now loads the immutable schedule once and reads claim/result
pairs through a private helper. The public receipt reader still validates the
schedule. Every claim identity and completion token is checked as before;
committed publication and oldest-first ordering are unchanged. No cursor cache
or durable format change is introduced.

Eight runs used baseline/candidate/candidate/baseline, then the reverse order.
Each run measured nine warm-cache samples per size on native WSL Debian storage.
Pooled medians (36 samples per size per binary) were:

| Completed occurrences | Baseline ms | Candidate ms | Reduction |
| --- | ---: | ---: | ---: |
| 0 | 0.028 | 0.025 | 12.0% |
| 100 | 1.714 | 1.485 | 13.3% |
| 1000 | 24.994 | 23.628 | 5.5% |
| 5000 | 135.721 | 109.627 | 19.2% |

Individual 5,000-receipt medians show the variation:

| Run | Binary | Median ms |
| --- | --- | ---: |
| 1 | baseline | 149.210 |
| 2 | candidate | 129.804 |
| 3 | candidate | 171.070 |
| 4 | baseline | 132.244 |
| 5 | candidate | 84.403 |
| 6 | baseline | 116.157 |
| 7 | baseline | 143.330 |
| 8 | candidate | 86.552 |

This is a local selection diagnostic. Pooled reduction is not proof of an
end-to-end worker improvement, a tail-latency guarantee or competitor superiority.
The full scan still grows with history; immutable schedule rereads were only
one source of overhead. Larger scaling work remains necessary.

The [raw comparison](dispatch-history-comparison.json),
[manifest](dispatch-history-comparison-manifest.json) and
[source patch](dispatch-history-selection.patch) identify the evidence.
An earlier [same-binary control](dispatch-history-control.json) is retained:
the candidate was accidentally copied before its build finished, so its identical
artifact hash invalidates that run as a candidate comparison. It is excluded
from the table. Subsequent comparisons checked distinct artifact identities.
The frozen binaries are `/var/tmp/hm-dispatch-history/baseline` and `candidate`.
Run each binary to reproduce the methodology in `dispatch_history.rs`.

All 39 durable-job tests and strict all-target Clippy passed on Windows and Linux,
including exclusive claims, restart recovery and unresolved-predecessor refusal.
