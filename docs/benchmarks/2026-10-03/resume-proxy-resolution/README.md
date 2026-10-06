# Real proxy resolution through paused-state guard

All 53 daemon tests pass in the isolated checkout. Two new tests execute actual localhost hostname resolution through the paused-value helper. Default policy refuses resolved private/internal addresses and returns the exact paused value for caller cleanup. The explicit operator private-proxy override accepts the same resolved address and returns the paused value without invoking cancellation rollback.

The resolver implementation is extracted behind resolve_egress_proxy; the production wrapper passes the existing operator option unchanged. No network-policy expansion or new public API is introduced. These are direct resolver/guard tests, not a real guest migration or injected DNS cancellation experiment. Existing synchronized cancellation tests continue to pass. Real networked resume, VM startup/registration cancellation and machine-crash recovery remain incomplete or unverified.

The updated 133-file permitted catalog matches root and isolated sources. Snapshots, patch and test log preserve this change; protected root core files remain excluded. No performance or competitor win is claimed.
