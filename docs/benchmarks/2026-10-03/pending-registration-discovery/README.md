# Administrator pending-registration discovery

`hm sandbox vm pending-registrations --node-id NODE [--after CURSOR]` queries administrator-only `/cluster/nodes/NODE/registrations/pending`. The control plane selects a registered node, requires administrator context and a configured cluster token, and sends a fresh authenticated node request. The node independently checks cluster credentials before listing locally retained pending registrations. No sandbox shared record is needed.

Pages contain at most 32 sorted entries with sandboxID and kind (named/unnamed), plus nextCursor. No descriptors, guest tokens or owner credentials are emitted by the node. The cursor is exclusive; concurrent registration changes mean pages are not a transactional snapshot. The CLI URL-encodes the cursor and bounds responses to 16 KiB. Discovery does not reconcile automatically or change guest state; invoke reconcile-registration for an explicitly selected guest.

All 54 daemon tests and 88 cluster tests pass (one pre-existing cluster test ignored). All 39 sandbox_vm-filtered CLI tests pass; 112 other CLI tests were filtered. New tests verify 65 entries span 32/32/1 pages without skipping/duplicating IDs and contain only ID/kind fields, administrator/cluster-token/unknown-node preconditions, and required node/cursor command parsing. Real KVM discovery/recovery and scoped-key middleware tests for this new GET route remain pending. Runtime response-bound validation and machine-crash recovery remain unverified or incomplete.

Source snapshots, patches, updated 133-file permitted catalog and test logs preserve provenance. Builds ran only in the isolated checkout; protected root core files remain excluded. No performance or competitor superiority is claimed.
