# Opt-in automatic registration worker

Adds `--registration-reconcile-interval SECS` (1–3600), requiring a cluster store and nonempty cluster token. The worker shares the existing publication transition lock with creation and manual reconciliation. Each pass selects at most 32 pending local guests, rotating the cursor across failures, and bounds each attempt to five seconds. It retains pending markers on timeout or publication failure.

All 56 isolated daemon tests passed, including interval bounds and selection fairness over 65 persistently pending registrations. This archive does not establish automatic recovery in a running KVM guest or after a daemon crash. Pending registrations remain process-local; store events may be repeated after uncertain publication.
