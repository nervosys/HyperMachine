# Reserved aliases with real KVM and authenticated SSH

The fixed run passed 20/20 functional checks through API TLS, node mTLS and a real KVM guest. The CLI assigned and inspected an alias without `hm.name` metadata, preserved its parent ownership after a fork, refused transfer to the child and reused the alias on a fresh VM after deletion. OpenSSH transferred an exact 1 MiB payload, preserved remote exit code 7 and rejected incorrect client and host keys. Both runs left zero sandboxes and stopped all 22 registered processes.

The first run passed 19/20 checks but alias reuse failed with 409. The accepted older daemon deleted the VM record without cleaning the newer reserved-name index. The control plane now repeats atomic store deletion after a successful forwarded sandbox DELETE and returns 503 if cleanup fails. It does not apply this cleanup to checkpoint deletion. The failed report and logs are retained alongside the successful report. Only the control-plane binary changed between runs.

The release client and original control plane came from commit 731361a; the fixed control plane adds `deletion-fix.patch`. All builds also included the uncommitted `provisional-core.patch`; these client/control-plane processes do not execute VM boot. The unchanged daemon is the previously accepted artifact, not the borrowed-boot candidate. Binary hashes, exact coordinator, frozen relevant sources and raw process logs are in this directory and `manifest.json`. This is local WSL nested KVM functional evidence, with no competitor or performance claim.

Windows and Linux each passed all 19 control-plane HTTP tests and strict cluster library/test Clippy. The regression test uses a node that acknowledges deletion without cleaning shared storage. The real KVM run additionally exercises an older daemon that removes the VM record before control-plane cleanup.

Creation-time exclusive ownership, legacy metadata migration, pending-operation reconciliation and explicit alias removal remain incomplete. `create --name` still writes advisory metadata. Guest SSH provisioning is supplied by the fixture.

Verify the archive with `python docs/benchmarks/2026-10-02/reserved-alias/verify.py`.
