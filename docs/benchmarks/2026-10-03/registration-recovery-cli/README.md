# Administrator registration recovery API and CLI

`hm sandbox vm reconcile-registration SANDBOX --node-id NODE` posts an empty JSON object to `/cluster/nodes/NODE/sandboxes/SANDBOX/registration/reconcile`. Explicit node selection supports initial record-write failure where no shared sandbox record exists. The control plane requires an administrator context and nonempty configured cluster token, looks up only registered nodes, and forwards a fresh request with cluster credentials rather than client keys/owner headers. It does not select arbitrary client URLs.

Success is bounded to 16 KiB and must contain the requested sandboxID; the descriptor is rewritten for control-plane routing. Non-success preserves status with a generic refusal/uncertainty message. The CLI uses its existing bounded request and API TLS authentication. Existing node reconciliation retains per-request cluster authentication and trusted local pending-registration state.

All 88 cluster library tests pass (one pre-existing ignored test), including new administrator/token/unknown-node refusal checks. All 38 sandbox_vm-filtered CLI library tests pass, including the new required explicit-node parser test (112 other tests were filtered, not run). End-to-end CLI/API forwarding with a real preserved guest remains pending; these tests do not establish scoped-key middleware behavior or response-size/identity refusal at runtime. Exactly-once events, VM-startup cancellation and durable crash recovery remain incomplete.

Source snapshots, patches, the updated 133-file permitted catalog and test logs preserve this change. All compilation ran in the isolated checkout. Protected root core files remain excluded. No competitor or performance advantage is claimed.
