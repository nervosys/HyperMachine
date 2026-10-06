# Parent names remain usable across a real KVM fork

All 20 functional checks passed with the fork metadata fix. OpenSSH reached the parent by its reserved creation name while the child was alive. CLI inspection still returned the parent ID, the child had no `hm.name`, and the parent's metadata name was unchanged. Both VMs retained an unrelated metadata marker. Assigning the parent's name to the child returned 409. Named SSH remained usable after child deletion, and deleting the parent allowed another named creation to reuse its name.

The daemon removes `hm.name` from the metadata copied to children before launching the fork tasks. It preserves other metadata and never mutates the parent's record. Names and reserved aliases are not inherited. The [older daemon run](../reserved-create/README.md) remains frozen and demonstrates the original conflict; deployment of the new daemon is required to fix that behavior on each node. Pending-name reconciliation, direct-node/legacy migration and creation idempotency remain incomplete.

The test used local WSL nested KVM, an owned Redis server and 1 vCPU / 1024 MiB guests. It also verified exact 1 MiB SSH transfer, remote exit code 7, client and host-key rejection, both TCP half-closes, idle/pause/resume and deletion cleanup. Zero sandboxes remained, with no cleanup errors, and all 22 registered processes stopped. This is functional evidence with no performance or competitor ranking claim.

The new daemon was built from committed sources at `01ace7d` plus `fork-fix.patch`, in an isolated source directory. The uncommitted borrowed-boot edits were excluded. `build-context.json` records the committed boot-file hashes, archive hash and overlay hash; frozen boot inputs and the exact daemon source are retained here. The control plane, CLI, kernel and guest image were unchanged from the earlier named-creation run. The coordinator changed to require parent lookup during a live fork and preservation of unrelated metadata.

Daemon tests passed: 32 on Windows and 36 on Linux. Windows strict Clippy passed. Linux strict Clippy first flagged the existing eight-argument `bring_up` and `register` signatures; lint checking then passed with the explicit `clippy::too_many_arguments` baseline exception. The release build completed before artifact capture. This exception is recorded in the build context.

Verify the raw report, logs, source hashes and preservation of the old conflict with `python docs/benchmarks/2026-10-02/fork-name/verify.py`. The archive does not establish multi-node rollout, name recovery or performance superiority.
