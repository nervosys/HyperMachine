# Durable dispatch history: selection diagnostic

A release-mode run on the native WSL Debian temporary filesystem measured
`Store::next_vm_occurrence` after reopening the store. Nine warm-cache samples
per history size produced these medians:

| Completed occurrences | Selection median (ms) |
| --- | ---: |
| 0 | 0.029 |
| 100 | 1.549 |
| 1000 | 17.850 |
| 5000 | 87.275 |

This measures one pending occurrence after synthetic completed history.
Preparation uses the real serialized claim and completion types, but writes
fixture files directly to exclude sequential dispatch scanning from setup.
The benchmark asserts the selected timestamp on every sample. Store opening,
publication, guest execution and network requests are outside the timed region.
These are local diagnostics, not competitor scores or end-to-end worker latency.
Only one archived run is used; noise and filesystem differences remain relevant.

The [raw samples](dispatch-history-run-1.json) and
[manifest](dispatch-history-manifest.json) identify the binary, source, compiler
and environment. Reproduce with:

```sh
CARGO_TARGET_DIR=/var/tmp/hm-competitive-target cargo run --release -p hv2-jobs --example dispatch_history
```

Selection scans completed receipts from the beginning. Its observed cost grows
with history. The worker selects before dispatch and claiming selects again;
this run does not measure their combined overhead. A future optimization must
preserve committed occurrence validation, exclusive claims, oldest-first order,
restart recovery and blocking on unresolved predecessors. A cached cursor must
not become authority to skip uncertain work. No optimization is adopted here.
