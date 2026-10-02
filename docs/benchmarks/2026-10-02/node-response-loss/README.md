# Named creation after node response loss

All 24 real KVM/TLS checks passed using the unchanged daemon, control plane, CLI, kernel and guest image from [node-completion](../node-completion/README.md). This is functional verification on local WSL nested KVM, with no performance or competitor ranking claim.

Two new cases use a fixture-owned mTLS HTTP relay between the control plane and node. The relay forwards a normal named creation with the control plane's generated operation context, waits for node status 201 and discards the descriptor. One case closes the connection before sending headers; the other withholds headers beyond the actual 60-second control-plane deadline. Both control-plane requests return 502. The timeout took 60.004 seconds; connection loss took 0.022 seconds. These are fault-test durations, not creation benchmarks.

In each case, exactly one backend creation request occurred. CLI inspection recovered the committed VM by name, named SSH reached the same guest, duplicate creation returned 409, inventory contained no extra VM, and deletion released the name. The relay listener and handler stopped. The fixture briefly pauses only its owned node while advertising the relay in its private Redis namespace, then resumes it before forwarding. No pending reservation is injected for these two cases. The earlier discarded-descriptor case retains its private reservation setup.

The other 22 checks also passed. Cleanup reported zero remaining sandboxes, no errors and all 22 registered processes stopped. Registration-store faults, crashes during boot, in-flight operation identity, pending-operation reconciliation, fleet migration and client creation idempotency remain incomplete.

Raw reports, logs and the exact frozen coordinator are retained here. `manifest.json` hashes every evidence file; `verify.py` checks those bytes, both fault outcomes, cleanup and unchanged binary provenance against the previous archive. Run `python docs/benchmarks/2026-10-02/node-response-loss/verify.py`.
