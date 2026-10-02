# Protect reconciliation from connect and checkpoint replacement

Connect requests extending a timeout now return 409 while registration is uncertain, and use the same transition lock as reconciliation. Read-only connect remains available. Checkpoint creation remains available because it neither replaces the guest nor publishes its sandbox record; restoring a checkpoint returns 409 while registration is uncertain, preserving the original guest and trusted context.

The real KVM fixture checks read-only connect and checkpoint creation before reconciliation, refuses timeout extension and restoration, then completes same-guest reconciliation and successfully extends the timeout and restores that checkpoint. Existing idle, pause, resume, fork, ownership and named SSH checks remain in the combined 26-case run. Raw evidence establishes the outcomes and cleanup. This is local WSL nested KVM functional evidence, with no performance comparison.

Windows passed 35 daemon tests and strict Clippy. Linux passed 39 tests and lint checking with the existing lifecycle argument-count exception. The source provenance is the prior registration-lifecycle clean source plus the archived daemon main/checkpoint overlays and patch. Committed boot hashes and the binary hash are captured in build-context.json; provisional boot edits were excluded. The control plane, CLI, kernel and guest image are unchanged.

Durable restart identity, a public operator interface, pending operations without a surviving guest and lost-event reconciliation remain incomplete. No uncertain name is automatically released. Run `python docs/benchmarks/2026-10-02/registration-mutations/verify.py` to verify evidence hashes, source provenance and the combined outcomes.
