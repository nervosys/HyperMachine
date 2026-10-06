# In-memory resume ownership lookup cancellation

Resume now retains its paused value in a rollback guard while waiting for authoritative ownership. Cancellation restores that value to the local paused map when no shared snapshot store exists. Shared-store cancellation continues to restore the description through SnapshotClaim. Completed lookups return the exact paused value and result to the existing caller cleanup paths without rollback.

All 51 daemon tests pass in the isolated checkout. New synchronized cancellation and completed-error tests exercise the same generic rollback helper used by resume, verifying exact owned payload preservation and no rollback after completion. Existing shared claim cancellation and completed-error tests also pass. The local paused-map wiring is inspected source evidence; these helper tests are not a real VM cancellation experiment.

The 133 permitted source hashes match root and isolated checkout. Source snapshots and patch preserve this change; protected root core files remain excluded. Updated real-KVM verification is pending. Cancellation during VM startup/registration and machine-crash recovery remain incomplete. No performance or competitor advantage is claimed.
