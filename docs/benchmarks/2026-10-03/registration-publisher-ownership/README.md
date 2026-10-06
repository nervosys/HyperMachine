# Initial publication ownership

Initial creation does not hold the per-sandbox transition lock used by reconciliation. Its local pending marker appears before asynchronous shared publication finishes, so reconciliation could previously retry that same publication concurrently. A local atomic in-progress flag now gives the original register future ownership of this phase. Reconciliation returns 409 while it is active. A Drop guard clears the flag when the future completes, returns an error or is cancelled; the pending marker remains available for later recovery after uncertain publication.

All 56 daemon tests pass in the isolated checkout. New synchronized task-abort and normal-completion tests verify that publisher ownership is active until cancellation/completion and is then released. These test the exact guard used by register; they do not inject cancellation or a concurrent HTTP recovery call into a real guest. Controlled Redis/KVM race and updated runtime verification remain pending.

The flag is local and never serialized into shared records or guest descriptors. Discovery still shows transient pending entries, as documented; automatic reconciliation is not implemented. Event retries can duplicate delivery after uncertainty. VM-startup cancellation and durable crash recovery remain incomplete.

The updated 133-file permitted catalog matches root and isolated checkout. Snapshots, patch and test log preserve the change. Builds ran only in the isolated checkout; protected root core files remain excluded. No performance or competitor advantage is claimed.
