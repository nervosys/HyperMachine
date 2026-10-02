# Named guest preservation after event publication failure

The fixture denies `XADD` on its owned Redis server during one control-plane named creation. The node atomically registers the VM and binds its name, then fails to append the creation event. Creation returns 503. CLI inspection and named SSH recover the same committed guest; duplicate creation is refused and inventory contains no extra VM. Redis's ACL denial log confirms the failed `XADD`. Permission is restored in `finally` before deletion, and deletion releases the name.

The accepted daemon, control plane, CLI, kernel and guest image are unchanged from [node-completion](../node-completion/README.md). This exercises the daemon's uncertain registration-result branch after a committed binding, using real KVM and Redis. It does not verify a fault inside the atomic registration write, a definitive registration refusal after boot, or a boot crash. Lost event reconciliation remains incomplete. This is functional evidence on local WSL nested KVM, with no performance comparison.

Run 1 denied `PUBLISH`, which is not the command this implementation uses for event publication. Creation returned 201 and the fixture failed its expected-status assertion. That ineffective injection, coordinator and raw logs are preserved. Run 2 corrects the injection to `XADD`; its coordinator is `coordinator-2.py`. The report and verifier establish the outcome and cleanup rather than treating the initial run as a product failure.

Run `python docs/benchmarks/2026-10-02/node-publication-fault/verify.py` to check archived bytes, binary provenance, the preserved failed injection and the corrected run.
