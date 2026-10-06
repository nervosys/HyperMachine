# Resume ownership lookup cancellation

A shared paused snapshot was claimed before resume awaited the cluster ownership lookup. Cancelling that wait could leave its description renamed to a claimed path, making it unavailable for a later resume. The existing adoption claim guard is now named SnapshotClaim and used around this lookup too. Dropping a cancelled lookup restores the shared description; completion disarms that temporary guard so existing success/error cleanup retains responsibility.

All 49 daemon tests pass in the accepted isolated checkout. The cancellation test synchronizes entry into a pending protected lookup, aborts the task, joins cancellation and verifies the exact original description bytes at the unclaimed path. A completed-error test verifies the guard does not prematurely return the claim, preserves exact bytes and allows caller cleanup. These tests execute the same protection helper used by resume.

The guard covers shared-store ownership lookup before VM startup. It does not establish machine-crash recovery, cancellation safety during VM bring-up or registration, or cancellation recovery for in-memory-only paused state. Real KVM runtime verification of this updated daemon remains pending. Existing production UDP source is unchanged.

Source snapshots and patch preserve the before/after change. The updated 133-file permitted catalog matches root and isolated sources; prior immutable evidence catalogs remain unchanged. All builds/tests ran only in the isolated checkout and protected root core files were excluded.
