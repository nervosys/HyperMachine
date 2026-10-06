# Unregistered startup cancellation cleanup

The bring_up path now retains an ownership guard from VM construction through launch, readiness, network/mount/environment setup and the caller's wait for registration. Cancellation schedules VM stop and aborts an attached unregistered network bridge. Explicit startup errors also stop through the guard. Running carries the guard until register inserts the guest into the local registry, then disarms it synchronously before awaiting shared publication. A guest with uncertain publication therefore remains locally owned for reconciliation.

Verification on accepted isolated sources:

- 58 ordinary daemon tests pass; network-task cancellation and successful handoff are checked. Two Linux/KVM tests are explicitly ignored in the ordinary suite.
- Both explicit KVM tests pass against the owned kernel/image: cancellation stops launched guests before or after readiness is checked; a disarmed guard keeps a third guest running and answering until explicit stop.
- The final frozen daemon passes two publication/lifecycle profiles with 32 checks each, including named uncertainty, operation identity, duplicate refusal, name reuse, unnamed resume and native TCP/UDP.
- Sixteen actual guest HTTPS/network checks pass, including successful startup/fork/pause/resume, policy reload/revocation, reused HTTPS connections and fresh-daemon paused-network reconstruction.

Runtime input hashes match final binaries; all API/network fixture processes and guests are cleaned up. The direct KVM tests observe stopped VM state; they do not claim an independent OS-thread inventory audit.

Cleanup needs a live Tokio runtime and remains asynchronous/best effort. These checks do not induce HTTP-disconnect cancellation of a complete daemon request, cancel inside VM construction or every mount/environment await, or establish atomic shared-claim fencing across cancellation/process crash. They provide no performance or competing-product win. The intermediate VM-only guard build is excluded from this final archive.

Run ordinary daemon tests in the accepted isolated checkout. Run the ignored tests explicitly with HM_STARTUP_CLEANUP_KERNEL and HM_STARTUP_CLEANUP_INITRD pointing to the frozen owned images and `cargo test -p hv2-sandboxd --bin hv2-sandboxd startup_cleanup_kvm -- --ignored --test-threads=1`. The lifecycle driver identifies final daemon/control/CLI/gateway paths. Protected modified root core files are excluded.
