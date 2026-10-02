# Real guest behavior after registration write denial

The clean-source daemon and control plane include the [registration ACL fix](../registration-acl/README.md). The fixture denies `SADD` on its owned Redis server during control-plane named creation. Creation returns 503 after boot. Authenticated local-node inventory finds exactly one new guest, and direct execution confirms it remains live. Shared inventory contains only the original VM: the new VM record and both registration indexes are absent. Its name remains pending, and another creation returns 409.

After restoring permission, the fixture deletes the unregistered guest through the authenticated node endpoint. Local inventory returns to its original contents, while name lookup and creation still return 409. Deletion does not blindly release uncertain pending ownership. This verifies preservation and controlled cleanup, not a public recovery workflow: operator reconciliation and in-flight operation identity remain incomplete. Definitive post-boot registration refusal and boot-crash cleanup are also unverified.

The combined run includes the earlier response-drop, real 60-second timeout, post-commit event failure, name/fork ownership, SSH, TCP and lifecycle checks. Reports establish case counts and cleanup. This is local WSL nested KVM functional evidence, with no performance comparison.

Run 1 failed before daemon startup because copied binaries lacked executable permissions; its raw report and Redis log are preserved. Run 2 uses the identical binary bytes and coordinator after restoring executable permissions. `build-context.json` identifies binary hashes, store overlay and committed boot hashes; source provenance is the node-completion clean source plus the archived registration-acl store overlay. The CLI, kernel and guest image are unchanged. No provisional boot changes were included.

Run `python docs/benchmarks/2026-10-02/node-registration-fault/verify.py` to check the evidence hashes, combined cases and clean-source provenance.
