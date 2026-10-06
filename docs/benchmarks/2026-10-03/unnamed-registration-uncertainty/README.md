# Unnamed registration uncertainty

Clustered unnamed creation and resume previously used record_event, which logs shared-store errors and returns no error. A locally running guest could therefore answer success while its shared record remained missing or paused. Unnamed registration now installs a pending marker before shared publication, publishes through the node directly, clears the marker only after success, and returns 503 on uncertain publication while preserving the local guest.

PendingRegistration distinguishes trusted named operations from unnamed lifecycle event publication. Existing authenticated reconciliation retries the corresponding path using the retained local record; existing guards against mutations and idle expiration apply to both kinds. Cancellation during shared publication leaves the pending marker installed. Standalone registration continues through the local event path. Retry can republish an event after an uncertain prior commit; exactly-once delivery is not claimed.

All 53 existing daemon tests pass in the isolated checkout. They provide compilation and regression evidence, not targeted Redis failure proof for this new unnamed path. Real Redis publication-failure, cancellation and reconciliation fixtures remain pending. VM-startup cancellation and machine-crash recovery remain incomplete. No performance or competitor win is claimed.

The updated 133-file permitted catalog matches root and isolated sources. Snapshots, patch and test log preserve the change. Protected root core files remain excluded.
