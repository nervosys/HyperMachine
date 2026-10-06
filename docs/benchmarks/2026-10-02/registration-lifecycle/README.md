# Preserve uncertain registration through lifecycle requests

Pause, capacity eviction and automatic idle pause now refuse to remove a guest while its registration is uncertain. Fork and timeout changes also return 409 until reconciliation finishes. Timeout mutation takes the same transition lock as reconciliation, so it cannot publish an unbound record concurrently. This protects locally retained ownership context; expiration and explicit deletion retain their existing behavior.

The KVM fixture configures a 30-second idle timeout on the guest whose registration write is denied. It waits 35 seconds and checks local inventory says running before making any potentially resuming request, then executes a command. Pause, fork and timeout requests return 409. Reconciliation still refuses wrong credentials and a replacement owner, then registers the same guest using its original retained owner. Timeout, pause, resume and fork succeed afterward. Named lookup remains on the same parent; the fork child omits the name and is deleted. Named SSH reaches the parent, and deleting it releases bound ownership.

The combined run covers the existing 26 KVM/TLS checks, including response drop, actual 60-second timeout, event failure, SSH and TCP behavior. Raw reports establish outcomes and cleanup. Windows passed 35 daemon tests and strict Clippy. Linux passed 39 tests and lint checking with the existing lifecycle argument-count exception. Clean-source provenance and binary hashes are retained in build-context.json; provisional boot edits were excluded. The control plane, CLI, kernel and guest image match the preceding reconciliation evidence.

This is local WSL nested KVM functional verification, with no performance comparison. Restart identity, a public operator workflow, pending operations without a surviving guest, other lifecycle mutation paths and lost-event reconciliation remain incomplete. No pending ownership is automatically released.

Run `python docs/benchmarks/2026-10-02/registration-lifecycle/verify.py` to verify archived bytes, source provenance and combined outcomes.

Run 1 sent a pause request with JSON headers and an empty body; the JSON extractor returned 400 before the guard ran. Its coordinator, report and logs are preserved. Run 2 sends the existing pause protocol body `{}` with the identical binary bytes and retains its corrected coordinator as `coordinator-2.py`.
