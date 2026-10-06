# Resume network-decision cancellation

Resume can await egress proxy hostname resolution while deciding network configuration for a shared paused description. This wait precedes VM startup. It now uses the same shared SnapshotClaim and local paused-value rollback helpers as authoritative ownership lookup. Cancellation returns the shared description or local paused value; completed decisions return state to existing caller cleanup. The request is cloned before moving paused state into the guard.

All 51 daemon tests pass in the isolated checkout, including synchronized shared/local helper cancellation and completed-error cleanup checks. Inspection confirms NetworkRequest::decide computes policy, resolves the proxy and clones tokens without starting a VM. The helper tests prove rollback behavior; no DNS cancellation was injected into a real resume in this archive. Updated networked KVM verification remains pending.

The updated 133-file permitted catalog matches root and isolated sources. Source snapshots, patch and test log preserve the change. Protected root core files remain excluded. VM startup requires coordinated resource cleanup before a snapshot may be released; startup/registration cancellation and machine-crash recovery remain incomplete. No performance or competitor advantage is claimed.
