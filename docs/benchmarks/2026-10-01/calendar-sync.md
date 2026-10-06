# Schedule directory sync cost

Same local WSL Debian host and frozen release diagnostic, baseline/candidate/candidate/baseline order, five fresh stores per case per run (ten samples per variant). Pooled median milliseconds include record publication and progress commit; setup, guest execution and networking are excluded. This is not a competitor benchmark or physical power-loss test.

| Timezone | Batch limit | Before directory sync ms | With directory sync ms |
|---|---:|---:|---:|
| UTC | 1 | 0.062441 | 0.071188 |
| UTC | 1000 | 21.484265 | 26.953076 |
| America/Los_Angeles | 1 | 0.644804 | 0.759214 |
| America/Los_Angeles | 1000 | 22.624597 | 29.874542 |

All scheduled UTC timestamps match. The example reopens each store and verifies its watermark and full committed page. The Unix candidate syncs the store directory after ensuring the record subdirectory exists, and syncs the record directory after publishing its final hard link, including an already-existing name. This orders acknowledged publications and retry reconciliation. Temporary-file removal remains best effort. A pre-existing persistent store root and a filesystem honoring sync are required; root ancestor durability, Windows directory entries, network filesystems and actual power loss are unverified.

Baseline is the frozen optimized publication binary built from e901a11, whose source is unchanged at c26c712. The candidate adds the source patch. Fifty-one library tests and strict all-target Clippy passed on Windows and Linux. The change affects schedules, occurrences, progress, dispatch claims and completions through their shared publication helper. [Real KVM validation of the sync change](calendar-sync-kvm.md) passed all 16 functional cases. This does not establish physical power-loss recovery.

[Raw samples](calendar-sync-comparison.json), [manifest](calendar-sync-manifest.json), [source](calendar-sync-source.rs), [patch](calendar-sync.patch). Verify with `python tools/verify-calendar-sync.py`. Timing differences on this host do not establish a general speedup or the cost on other storage.
