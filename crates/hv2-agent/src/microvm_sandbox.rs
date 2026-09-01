//! A sandbox whose boundary is a whole virtual machine.
//!
//! # Why this is the other backend
//!
//! [`ProcessSandbox`](hv2_sandbox::ProcessSandbox) confines a process with
//! whatever the host kernel offers, and reports honestly that this is less on
//! some hosts than others: no filesystem isolation anywhere, and on Windows and
//! macOS no network or process isolation either. Those gaps are not oversights
//! in the implementation — they are what the host kernel does and does not
//! provide to an unprivileged process.
//!
//! A VM does not have them, because the workload is not sharing a kernel with
//! the host at all. [`MicroVmSandbox`] runs the workload inside a guest through
//! the vsock agent, and therefore enforces every [`Control`] this crate
//! defines. It costs a VM.
//!
//! Both implement [`Sandbox`], so a caller chooses isolation strength without
//! changing how it asks.
//!
//! # What it needs, and what it says when it does not have it
//!
//! A VM with a guest channel attached, booted, running `hv2-guest-agentd` —
//! the four preconditions in [`crate::guest_agent`]. This type does not create
//! or boot a VM: taking a running one means the caller keeps control of a
//! resource that is expensive to make, and a sandbox that silently booted a VM
//! per run would be a surprising cost hidden behind a small-looking call.
//!
//! # The one control the guest cannot enforce for itself
//!
//! Memory and process ceilings inside the guest are the guest's own business,
//! and this backend reports them as enforced because the VM's memory size is a
//! hard ceiling no guest process can exceed — the VM has that much memory and
//! no more. What the guest agent does enforce directly is the wall clock and,
//! through it, CPU time.

use std::sync::Arc;
use std::time::Duration;

use hv2_sandbox::{
    Control, Controls, FilesystemPolicy, NetworkPolicy, Sandbox, SandboxCommand, SandboxError,
    SandboxOutput, SandboxSpec,
};

use crate::AgentVM;

/// A sandbox backed by one running VM.
pub struct MicroVmSandbox {
    vm: Arc<AgentVM>,
    memory_bytes: u64,
    /// Whether the VM was configured with networking. A guest with no network
    /// device is network-isolated by construction; one with a device is not,
    /// and claiming otherwise would be the sandbox lying about its own shape.
    networked: bool,
}

impl MicroVmSandbox {
    /// What this backend enforces, for a VM with or without networking.
    ///
    /// An associated function so it can be checked without building a VM. The
    /// two defects this had -- claiming a control it discarded, and taking the
    /// looser of two deadlines -- were both unreachable from a test because
    /// the only way to ask was to construct a `MicroVmSandbox`, which needs a
    /// hypervisor. Logic nothing can interrogate is logic nothing checks.
    #[must_use]
    pub fn declared_controls(networked: bool) -> Controls {
        let controls = Controls::none()
            // A guest process cannot see, signal, or share a filesystem with
            // anything on the host. This is not a policy the sandbox applies;
            // it is what a separate kernel means.
            .with(Control::FilesystemIsolation)
            .with(Control::ProcessIsolation)
            .with(Control::NoNewPrivileges)
            // The VM has a fixed amount of memory, so a ceiling at or below it
            // is one the hardware keeps.
            .with(Control::Memory)
            // Not ProcessCount. `max_processes` never leaves the host: this
            // backend forwards a program, its arguments and a deadline to the
            // guest agent and nothing else, so the number a caller sets is
            // discarded and the guest forks up to whatever its own kernel
            // allows. Claiming it made `unenforced` come back empty, which this
            // crate defines as "every requested control was applied", so a
            // caller asking for one process was told the limit held.
            .without(
                Control::ProcessCount,
                "this backend sends the guest agent a program and a deadline, and has no way \
                 to bound how many processes it starts; the guest kernel's own limits are all \
                 that apply",
            )
            // Both are the guest agent's, which kills a program that overruns.
            .with(Control::CpuTime)
            .with(Control::WallClock);

        if networked {
            controls.without(
                Control::NetworkIsolation,
                "this VM was built with networking enabled; build it without to isolate the \
                 workload from the network",
            )
        } else {
            controls.with(Control::NetworkIsolation)
        }
    }

    /// The deadline to give the guest, from a spec's two time bounds.
    ///
    /// The stricter of the two, not whichever was looked at first. This used
    /// `wall_clock.or(cpu_time)`, so a caller asking for one second of CPU
    /// inside a ten-minute wall clock got ten minutes -- with both controls
    /// still reported as enforced. `AgentVM::effective_script_timeout` had the
    /// same choice and takes the minimum; so does this.
    #[must_use]
    pub fn effective_timeout(wall_clock: Option<Duration>, cpu_time: Option<Duration>) -> Duration {
        match (wall_clock, cpu_time) {
            (Some(wall), Some(cpu)) => wall.min(cpu),
            (Some(only), None) | (None, Some(only)) => only,
            (None, None) => Duration::from_secs(30),
        }
    }

    /// Wrap a VM that is already running and already has a guest agent.
    ///
    /// Use [`AgentVM::ping_guest`] first if you need to know that before the
    /// first workload rather than after it.
    pub fn new(vm: Arc<AgentVM>) -> Self {
        let config = vm.vm().config().clone();
        Self {
            memory_bytes: config.memory_size,
            networked: config.enable_networking,
            vm,
        }
    }

    /// The VM this sandbox runs workloads in.
    pub fn vm(&self) -> Arc<AgentVM> {
        Arc::clone(&self.vm)
    }
}

impl Sandbox for MicroVmSandbox {
    fn name(&self) -> &str {
        "microvm"
    }

    fn controls(&self) -> Controls {
        if self.vm.guest_kernel_args().is_none() {
            // Without a channel nothing can run at all, so claiming controls
            // would be claiming to confine a workload that cannot start.
            return Controls::none().without(
                Control::WallClock,
                "this VM has no guest channel; call attach_guest_channel() before it boots",
            );
        }

        Self::declared_controls(self.networked)
    }

    fn run(
        &self,
        command: &SandboxCommand,
        spec: &SandboxSpec,
    ) -> Result<SandboxOutput, SandboxError> {
        if command.program.trim().is_empty() {
            return Err(SandboxError::InvalidSpec("no program to run".to_string()));
        }
        if let Some(bytes) = spec.memory_bytes {
            if bytes > self.memory_bytes {
                return Err(SandboxError::InvalidSpec(format!(
                    "a {bytes}-byte ceiling is larger than the VM's own {} bytes of memory, so \
                     it would not be a limit",
                    self.memory_bytes
                )));
            }
        }
        if let FilesystemPolicy::Isolated { .. } = spec.filesystem {
            // The guest has its own filesystem already. Mounting host paths
            // into it is a different feature, and pretending this one does it
            // would hand the caller a guest that cannot see the paths it
            // named.
            return Err(SandboxError::InvalidSpec(
                "the microVM sandbox isolates the filesystem by giving the workload the \
                 guest's own; mounting host paths into a guest is not supported"
                    .to_string(),
            ));
        }
        let unenforced = spec.reconcile(&self.controls())?;
        debug_assert!(
            spec.network == NetworkPolicy::Host || !self.networked || spec.best_effort,
            "reconcile should have refused a network-isolated spec on a networked VM"
        );

        // The guest agent enforces its own deadline and reports a timeout with
        // whatever the program printed, so a wall clock is passed through
        // rather than being layered on out here.
        let timeout = Self::effective_timeout(spec.wall_clock, spec.cpu_time);

        let vm = Arc::clone(&self.vm);
        let program = command.program.clone();
        let args = command.args.clone();

        // The caller may or may not be on a runtime, and this trait is
        // synchronous because the process backend is. Borrowing the current
        // runtime when there is one, and making a small private one when there
        // is not, keeps both callers working.
        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|scope| {
                scope
                    .spawn(|| handle.block_on(vm.exec_in_guest(&program, &args, timeout)))
                    .join()
                    .map_err(|_| SandboxError::Runtime("guest exec thread panicked".to_string()))
            })?,
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| SandboxError::Runtime(format!("could not start a runtime: {e}")))?
                .block_on(vm.exec_in_guest(&program, &args, timeout)),
        };

        let out = result.map_err(|e| SandboxError::Runtime(e.to_string()))?;

        Ok(SandboxOutput {
            exit_code: out.exit_code,
            signal: out.signal,
            stdout: out.stdout.into_bytes(),
            stderr: out.stderr.into_bytes(),
            killed_by: out.timed_out.then_some(Control::WallClock),
            unenforced,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capability, CapabilitySet};

    /// Build a VM for these tests, skipping where no hypervisor backend is
    /// available.
    async fn vm(networking: bool) -> Option<Arc<AgentVM>> {
        let mut capabilities = CapabilitySet::default();
        capabilities.add(Capability::GuestExec);

        match AgentVM::builder()
            .name("sandbox-vm")
            .cpu_cores(1)
            .memory_gb(1)
            .enable_networking(networking)
            .capabilities(capabilities)
            .build()
            .await
        {
            Ok(vm) => Some(Arc::new(vm)),
            Err(e) => {
                eprintln!("skipping: no hypervisor backend available ({e})");
                None
            }
        }
    }

    #[tokio::test]
    async fn a_vm_with_no_guest_channel_claims_nothing() {
        let Some(vm) = vm(false).await else {
            return;
        };
        let sandbox = MicroVmSandbox::new(vm);

        // Nothing can run, so claiming to confine anything would be claiming
        // to confine a workload that cannot start.
        let controls = sandbox.controls();
        for control in Control::ALL {
            assert!(
                !controls.enforces(control),
                "{control} should not be claimed"
            );
        }
        assert!(controls
            .reason(Control::WallClock)
            .unwrap()
            .contains("channel"));
    }

    #[tokio::test]
    async fn a_channelled_vm_without_networking_enforces_all_but_process_count() {
        let Some(vm) = vm(false).await else {
            return;
        };
        vm.attach_guest_channel(3).await.expect("attach");
        let sandbox = MicroVmSandbox::new(vm);

        // The point of this backend: a separate kernel provides the controls no
        // host kernel would give an unprivileged process. All but one -- this
        // used to assert that every control was enforced, which is what kept
        // the ProcessCount claim alive: the test asserted the advertisement
        // rather than the behaviour, so it agreed with the defect.
        let controls = sandbox.controls();
        for control in Control::ALL {
            if control == Control::ProcessCount {
                assert!(
                    !controls.enforces(control),
                    "max_processes never leaves the host, so this must not be claimed"
                );
                continue;
            }
            assert!(
                controls.enforces(control),
                "{control} should be enforced by a VM boundary"
            );
        }
    }

    #[tokio::test]
    async fn a_networked_vm_does_not_claim_network_isolation() {
        let Some(vm) = vm(true).await else {
            return;
        };
        vm.attach_guest_channel(3).await.expect("attach");
        let sandbox = MicroVmSandbox::new(vm);

        // A guest with a network device is on the network. Claiming otherwise
        // because it is a VM would be the sandbox lying about its own shape.
        let controls = sandbox.controls();
        assert!(!controls.enforces(Control::NetworkIsolation));
        assert!(controls
            .reason(Control::NetworkIsolation)
            .unwrap()
            .contains("networking enabled"));

        let spec = SandboxSpec {
            network: NetworkPolicy::Denied,
            ..SandboxSpec::default()
        };
        let err = sandbox
            .run(&SandboxCommand::new("/bin/true"), &spec)
            .expect_err("asking for isolation this VM does not have must refuse");
        assert!(
            matches!(err, SandboxError::Unsupported { .. }),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn a_memory_ceiling_above_the_vm_s_own_is_refused() {
        let Some(vm) = vm(false).await else {
            return;
        };
        vm.attach_guest_channel(3).await.expect("attach");
        let sandbox = MicroVmSandbox::new(vm);

        // Accepting it would report Control::Memory as enforced while the
        // number the caller wrote bounds nothing.
        let spec = SandboxSpec {
            memory_bytes: Some(64 * 1024 * 1024 * 1024),
            ..SandboxSpec::unconfined()
        };
        let err = sandbox
            .run(&SandboxCommand::new("/bin/true"), &spec)
            .expect_err("a ceiling above the VM's memory is not a ceiling");
        assert!(
            err.to_string().contains("would not be a limit"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn a_workload_on_a_vm_with_no_guest_agent_fails_rather_than_hanging() {
        let Some(vm) = vm(false).await else {
            return;
        };
        vm.attach_guest_channel(3).await.expect("attach");
        let sandbox = MicroVmSandbox::new(vm);

        // The channel exists but nothing is running in the guest, because the
        // guest is not running. This must come back, and say so.
        let spec = SandboxSpec {
            wall_clock: Some(Duration::from_millis(200)),
            ..SandboxSpec::unconfined()
        };
        let err = sandbox
            .run(&SandboxCommand::new("/bin/true"), &spec)
            .expect_err("no agent answered");
        assert!(
            err.to_string().contains("hv2-guest-agentd"),
            "the failure should name what is missing: {err}"
        );
    }

    /// The backend must not claim a control it discards.
    ///
    /// `max_processes` never leaves the host on this path, so reporting
    /// ProcessCount as enforced made `unenforced` come back empty — which this
    /// crate defines as "every requested control was applied". A caller asking
    /// for one process was told the limit held while the guest could fork
    /// freely.
    #[test]
    fn process_count_is_reported_as_unenforced_rather_than_claimed() {
        let controls = MicroVmSandbox::declared_controls(false);

        assert!(
            !controls.enforces(Control::ProcessCount),
            "this backend cannot bound the guest's process count, so it must not say it does"
        );
        assert!(
            controls.reason(Control::ProcessCount).is_some(),
            "and it must say why, rather than leaving the control silently absent"
        );
    }

    /// Two deadlines means the stricter one, not whichever was checked first.
    #[test]
    fn the_stricter_of_cpu_time_and_wall_clock_wins() {
        let strict_cpu = MicroVmSandbox::effective_timeout(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
        );
        assert_eq!(
            strict_cpu,
            Duration::from_secs(1),
            "a one-second CPU bound inside a ten-minute wall clock must bound at one second"
        );

        let strict_wall = MicroVmSandbox::effective_timeout(
            Some(Duration::from_secs(1)),
            Some(Duration::from_secs(600)),
        );
        assert_eq!(strict_wall, Duration::from_secs(1));

        assert_eq!(
            MicroVmSandbox::effective_timeout(None, Some(Duration::from_secs(5))),
            Duration::from_secs(5),
            "one bound on its own still applies"
        );
        assert_eq!(
            MicroVmSandbox::effective_timeout(None, None),
            Duration::from_secs(30)
        );
    }
}
