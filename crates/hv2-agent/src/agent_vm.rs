//! AI-scriptable VM with enhanced capabilities for autonomous agents

use crate::{
    AgentError, Capability, CapabilitySet, GuestAgent, GuestExec, GuestOutput, Result, Sandbox,
    SandboxConfig, ScriptEngine,
};
use hv2_core::{BootSource, VMConfig, VMState, VM};
use hv2_guest_agent::PtySize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;

/// VM builder with AI agent capabilities
pub struct AgentVMBuilder {
    config: VMConfig,
    sandbox_config: SandboxConfig,
    capabilities: CapabilitySet,
    script_timeout: Duration,
}

impl AgentVMBuilder {
    pub fn new() -> Self {
        Self {
            config: VMConfig::default(),
            sandbox_config: SandboxConfig::default(),
            capabilities: CapabilitySet::default(),
            script_timeout: Duration::from_secs(300),
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    pub fn cpu_cores(mut self, count: u32) -> Self {
        self.config.vcpu_count = count;
        self
    }

    pub fn memory_gb(mut self, gb: u64) -> Self {
        self.config.memory_size = gb * 1024 * 1024 * 1024;
        self
    }

    /// Guest memory in MiB, for sizes a whole GiB does not fit: E2B sizes
    /// sandboxes in MiB.
    pub fn memory_mb(mut self, mb: u64) -> Self {
        self.config.memory_size = mb * 1024 * 1024;
        self
    }

    pub fn enable_gpu(mut self, enable: bool) -> Self {
        self.config.enable_gpu = enable;
        self
    }

    pub fn enable_networking(mut self, enable: bool) -> Self {
        self.config.enable_networking = enable;
        self
    }

    pub fn with_tracing(mut self) -> Self {
        self.config.enable_tracing = true;
        self
    }

    /// Set what this VM boots.
    ///
    /// Without one the VM is created with vCPUs and empty guest memory, so
    /// [`AgentVM::launch`] has nothing to execute.
    pub fn boot(mut self, source: BootSource) -> Self {
        self.config.boot = Some(source);
        self
    }

    /// Boot a Linux kernel, with an optional initrd and command line.
    pub fn boot_linux(
        mut self,
        kernel: impl Into<std::path::PathBuf>,
        initrd: Option<impl Into<std::path::PathBuf>>,
        cmdline: impl Into<String>,
    ) -> Self {
        let mut source = BootSource::linux(kernel).with_cmdline(cmdline);
        if let Some(initrd) = initrd {
            source = source.with_initrd(initrd);
        }
        self.config.boot = Some(source);
        self
    }

    pub fn script_timeout(mut self, timeout: Duration) -> Self {
        self.script_timeout = timeout;
        self
    }

    /// What scripts on this VM are allowed to do.
    ///
    /// Defaults to [`CapabilitySet::default`] (`VmRead` + `Metrics`).
    /// [`ScriptEngine`] requires `VmRead`, so a set without it refuses every
    /// script.
    pub fn capabilities(mut self, capabilities: CapabilitySet) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Resource limits for script execution.
    ///
    /// `max_cpu_time` bounds a script alongside [`Self::script_timeout`] — the
    /// stricter of the two wins. See [`Sandbox`] for what this does and does
    /// not enforce.
    pub fn sandbox(mut self, config: SandboxConfig) -> Self {
        self.sandbox_config = config;
        self
    }

    pub async fn build(self) -> Result<AgentVM> {
        let vm = VM::new(self.config)?;
        let script_engine =
            ScriptEngine::with_limits(self.capabilities.clone(), &self.sandbox_config);
        let sandbox = Sandbox::new(self.sandbox_config);

        Ok(AgentVM {
            vm: Arc::new(vm),
            script_engine: Arc::new(script_engine),
            sandbox: Arc::new(sandbox),
            script_timeout: self.script_timeout,
            started_at: RwLock::new(None),
            capabilities: self.capabilities,
        })
    }
}

impl Default for AgentVMBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Virtual machine with AI agent scripting capabilities
pub struct AgentVM {
    vm: Arc<VM>,
    script_engine: Arc<ScriptEngine>,
    sandbox: Arc<Sandbox>,
    script_timeout: Duration,
    started_at: RwLock<Option<Instant>>,
    /// What this VM's callers are allowed to do.
    ///
    /// The script engine holds its own copy, but the guest-exec path needs to
    /// consult the set directly: running a command inside the guest is a
    /// different power from reading a VM, and gating it on the same capability
    /// would make `VmRead` mean "may run anything in there".
    capabilities: CapabilitySet,
}

impl AgentVM {
    /// Create a new builder
    pub fn builder() -> AgentVMBuilder {
        AgentVMBuilder::new()
    }

    /// The wall-clock bound applied to a script.
    ///
    /// Two settings can bound a script — the builder's `script_timeout` and the
    /// sandbox's `max_cpu_time` — so the stricter one wins. Honouring only one
    /// would silently ignore a caller who tightened the other.
    pub fn effective_script_timeout(&self) -> Duration {
        self.script_timeout
            .min(Duration::from_secs(self.sandbox.config().max_cpu_time))
    }

    /// Evaluate a Rhai script on the host against a read-only view of this VM.
    ///
    /// This does **not** execute anything inside the guest — see the
    /// [`script`](crate::script) module docs.
    pub async fn execute_agent_script(&self, script: &str) -> Result<serde_json::Value> {
        tracing::info!("Executing agent script");

        // Bounded by wall-clock time and by the engine's own operation limit.
        // The `Sandbox` is a policy object, not OS-level containment: what keeps
        // a script off the network and filesystem is that Rhai's default engine
        // registers no I/O at all.
        let vm = Arc::clone(&self.vm);
        let script_engine = Arc::clone(&self.script_engine);
        let script = script.to_string();

        let result = timeout(self.effective_script_timeout(), async move {
            script_engine.execute(&script, vm).await
        })
        .await
        .map_err(|_| AgentError::Timeout("Script execution timed out".to_string()))??;

        tracing::info!("Agent script completed successfully");
        Ok(result)
    }

    /// Get VM state
    pub fn state(&self) -> VMState {
        self.vm.state()
    }

    /// Start the VM without executing guest code.
    ///
    /// This moves the VM into the `Running` state and makes it visible to the
    /// rest of the stack, but nothing drives its vCPUs. Use [`Self::launch`] to
    /// actually boot the guest.
    pub async fn start(&self) -> Result<()> {
        self.vm.start().await?;
        *self.started_at.write().await = Some(Instant::now());
        Ok(())
    }

    /// Provision the VM on its hypervisor backend, load its boot source, and
    /// run the guest.
    ///
    /// On return the guest is executing on a background task. This is what a
    /// CLI or API "start this VM" handler wants.
    pub async fn launch(&self) -> Result<()> {
        self.vm.launch().await?;
        *self.started_at.write().await = Some(Instant::now());
        Ok(())
    }

    /// Become the guest a snapshot holds and run it, instead of booting.
    ///
    /// Attach the guest channel (same CID) and any network device (same MAC)
    /// first, as the snapshotted VM had them. The clock is continued, because
    /// a guest agent runs on Linux and Linux schedules by time; its wall clock
    /// is then stale by as long as the snapshot sat, which
    /// [`Self::set_guest_clock`] corrects.
    pub async fn launch_from_snapshot(&self, snapshot: &std::path::Path) -> Result<()> {
        self.launch_from_snapshot_prefaulted(snapshot, &[]).await
    }

    /// [`Self::launch_from_snapshot`], with the guest-physical ranges in
    /// `prefault` mapped before the guest runs -- the snapshot's working set,
    /// as [`Self::touched_ranges`] found it on a guest restored earlier.
    pub async fn launch_from_snapshot_prefaulted(
        &self,
        snapshot: &std::path::Path,
        prefault: &[(u64, u64)],
    ) -> Result<()> {
        self.vm
            .launch_from_snapshot_prefaulted(
                snapshot,
                hv2_core::snapshot::machine::ClockOnRestore::Continue,
                prefault,
            )
            .await?;
        *self.started_at.write().await = Some(Instant::now());
        Ok(())
    }

    /// Every guest page this VM has touched since its memory was mapped, as
    /// guest-physical (address, length) ranges. See `VM::touched_ranges`.
    pub fn touched_ranges(&self) -> Result<Vec<(u64, u64)>> {
        Ok(self.vm.touched_ranges()?)
    }

    /// Pause, write this VM to `snapshot`, and resume it.
    pub async fn snapshot_to(&self, snapshot: &std::path::Path) -> Result<()> {
        self.vm.pause().await?;
        let written = self.vm.snapshot_with(snapshot, true).await;
        self.vm.resume().await?;
        written?;
        Ok(())
    }

    /// Pause, write this VM to `snapshot` as only what it changed since the
    /// image it was restored from, and stop it.
    ///
    /// What suspending an idle sandbox is: its host memory goes back to the
    /// host, and [`Self::launch_from_snapshot`] on a fresh VM brings it back.
    /// On failure the VM is resumed rather than left paused.
    pub async fn suspend_to(&self, snapshot: &std::path::Path) -> Result<()> {
        self.vm.pause().await?;
        if let Err(e) = self.vm.snapshot_layered(snapshot).await {
            self.vm.resume().await?;
            return Err(e.into());
        }
        self.stop().await
    }

    /// Pause, write this VM to `snapshot` as only what it changed since the
    /// image it was restored from, and resume it: a checkpoint to fork from,
    /// which the guest sees as a pause of a few milliseconds.
    pub async fn checkpoint_to(&self, snapshot: &std::path::Path) -> Result<()> {
        self.vm.pause().await?;
        let written = self.vm.snapshot_layered(snapshot).await;
        self.vm.resume().await?;
        written?;
        Ok(())
    }

    /// Set the guest's wall clock to the host's, through the guest agent.
    ///
    /// # Errors
    ///
    /// The agent did not run `date`, or it failed.
    pub async fn set_guest_clock(&self, timeout: Duration) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let exec = self
            .exec_in_guest("/bin/date", &["-s".to_string(), format!("@{now}")], timeout)
            .await?;
        if exec.exit_code != Some(0) {
            return Err(AgentError::Script(format!(
                "setting the guest clock: {}",
                exec.stderr.trim()
            )));
        }
        Ok(())
    }

    /// Whether this VM has a boot source configured.
    pub fn has_boot_source(&self) -> bool {
        self.vm.config().boot.is_some()
    }

    /// Stop the VM
    pub async fn stop(&self) -> Result<()> {
        self.vm.stop().await?;
        *self.started_at.write().await = None;
        Ok(())
    }

    /// Pause the VM
    pub async fn pause(&self) -> Result<()> {
        self.vm.pause().await?;
        Ok(())
    }

    /// Resume the VM
    pub async fn resume(&self) -> Result<()> {
        self.vm.resume().await?;
        Ok(())
    }

    /// Get VM metrics for AI monitoring
    pub async fn get_metrics(&self) -> Result<VMMetrics> {
        Ok(VMMetrics {
            state: self.vm.state(),
            vcpu_count: self.vm.vcpus().len() as u32,
            memory_size: self.vm.memory().total_size(),
            uptime_seconds: self
                .started_at
                .read()
                .await
                .map(|s| s.elapsed().as_secs())
                .unwrap_or(0),
            cpu_usage_percent: self.cpu_usage_percent().await,
            // Still unmeasured: knowing how much guest memory is actually in
            // use needs cooperation from inside the guest (virtio-balloon or a
            // guest agent), which does not exist yet.
            memory_used_bytes: None,
        })
    }

    /// Share of available vCPU time this VM has spent executing guest code.
    ///
    /// `VM`'s run loop already times every `run_vcpu` call, so this is measured
    /// rather than estimated: total guest time across all vCPUs, over the
    /// wall-clock time the VM has been started, times the vCPU count.
    ///
    /// Returns `None` when no vCPU has ever exited. That distinguishes a VM
    /// that is idle from one whose run loop never started at all -- `start()`
    /// without a boot source moves a VM to `Running` but never executes it, and
    /// reporting 0% there would claim an idle guest where there is no guest.
    pub async fn cpu_usage_percent(&self) -> Option<f64> {
        let stats = self.vm.all_vcpu_stats();
        if stats.iter().all(|s| s.exits() == 0) {
            return None;
        }

        let elapsed = self.started_at.read().await.map(|s| s.elapsed())?;
        let available_ns = elapsed.as_nanos().checked_mul(stats.len() as u128)?;
        if available_ns == 0 {
            return None;
        }

        let busy_ns: u128 = stats.iter().map(|s| s.run_time_ns() as u128).sum();

        // Clamp: the two clocks are sampled independently, so a vCPU that was
        // executing when we read them can total marginally over 100%.
        Some(((busy_ns as f64 / available_ns as f64) * 100.0).clamp(0.0, 100.0))
    }

    /// What the guest has written to its console, without consuming it.
    ///
    /// `None` means no console device is attached, which is not the same as a
    /// guest that has printed nothing: nothing registers a serial device
    /// automatically, so a caller wanting a boot log must attach one to
    /// [`Self::vm`]'s device manager. Returning `Some("")` for an unattached
    /// console would read as a silent guest and send an agent looking for a
    /// bug that is not there.
    pub async fn console_output(&self) -> Option<String> {
        let per_device = self.vm.console_output_by_device().await;
        if per_device.is_empty() {
            return None;
        }
        Some(
            per_device
                .iter()
                .map(|(_, bytes)| String::from_utf8_lossy(bytes))
                .collect(),
        )
    }

    /// Attach the vsock channel a guest agent will connect over.
    ///
    /// Call this before the guest boots, and put [`Self::guest_kernel_args`]
    /// on its command line: virtio-mmio has no enumeration, so a guest that is
    /// not told the window exists will never probe it.
    pub async fn attach_guest_channel(&self, guest_cid: u64) -> Result<()> {
        self.vm.attach_vsock(guest_cid).await?;
        Ok(())
    }

    /// Kernel arguments that make a Linux guest find the channel attached by
    /// [`Self::attach_guest_channel`], or `None` if none is attached.
    pub fn guest_kernel_args(&self) -> Option<String> {
        self.vm.vsock_kernel_args()
    }

    /// Run a program inside the guest and wait for it to finish.
    ///
    /// This is the operation `execute_script` was described as being and never
    /// was. It runs `program` directly — not through a shell — inside the
    /// guest operating system, through `hv2-guest-agentd` over vsock.
    ///
    /// A non-zero exit is a [`GuestExec`] with that exit code, not an error:
    /// the program ran, and its output is what explains the failure. An error
    /// here means the command could not be run at all.
    ///
    /// # Errors
    ///
    /// - [`AgentError::PermissionDenied`] without [`Capability::GuestExec`].
    /// - [`AgentError::Script`] when no channel is attached — which is a
    ///   different problem from a guest that does not answer, and is reported
    ///   differently so an operator is not sent looking in the guest for a
    ///   device the host never gave it.
    /// - [`AgentError::Timeout`] when nothing in the guest answers.
    pub async fn exec_in_guest(
        &self,
        program: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<GuestExec> {
        if !self.capabilities.has(Capability::GuestExec) {
            return Err(AgentError::PermissionDenied(
                "running a command inside the guest requires the GuestExec capability".to_string(),
            ));
        }

        let Some(device) = self.vm.vsock() else {
            return Err(AgentError::Script(
                "this VM has no guest channel: call attach_guest_channel() before the guest \
                 boots, and put guest_kernel_args() on its kernel command line"
                    .to_string(),
            ));
        };

        // The client polls a device that only moves bytes when the guest kicks
        // a queue, so it blocks. Keeping that off the async runtime is what
        // stops one slow guest from stalling every other task.
        let program = program.to_string();
        let args = args.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.exec(&program, &args, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest exec task failed: {e}")))?
    }

    /// Write `data` to `path` in the guest, creating its directories.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability; propagates the guest's refusal.
    pub async fn write_file_in_guest(
        &self,
        path: &str,
        data: Vec<u8>,
        timeout: Duration,
    ) -> Result<()> {
        self.write_file_in_guest_as(path, data, None, timeout).await
    }

    /// [`Self::write_file_in_guest`], the file and the directories made for
    /// it owned by `owner`: a user, or `hv2_guest_agent::TEMPLATE_USER` for
    /// the template's.
    ///
    /// # Errors
    ///
    /// As [`Self::write_file_in_guest`]; and an owner with no account.
    pub async fn write_file_in_guest_as(
        &self,
        path: &str,
        data: Vec<u8>,
        owner: Option<&str>,
        timeout: Duration,
    ) -> Result<()> {
        let device = self.file_channel()?;
        let path = path.to_string();
        let owner = owner.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.write_file_as(&path, &data, owner.as_deref(), timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest file write task failed: {e}")))?
    }

    /// A connection to TCP `port` inside the guest, carried over vsock: a
    /// sandbox's web server reached from the host, with or without a network.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability; nothing listening on `port`.
    pub async fn forward_port(
        &self,
        port: u16,
        timeout: Duration,
    ) -> Result<(crate::guest_agent::VsockStream, Vec<u8>)> {
        let device = self.file_channel()?;
        tokio::task::spawn_blocking(move || {
            GuestAgent::over_vsock(device, timeout)?.forward(port, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest forward task failed: {e}")))?
    }

    /// Open a framed UDP connection to a guest loopback port over vsock.
    ///
    /// # Errors
    /// Requires GuestExec, a nonzero port and successful guest socket setup.
    /// A successful setup does not establish that a UDP peer is listening.
    pub async fn forward_udp_port(
        &self,
        port: u16,
        timeout: Duration,
    ) -> Result<(crate::guest_agent::VsockStream, Vec<u8>)> {
        if port == 0 {
            return Err(AgentError::Script("UDP port must be nonzero".into()));
        }
        let device = self.file_channel()?;
        tokio::task::spawn_blocking(move || {
            GuestAgent::over_vsock(device, timeout)?.forward_udp(port, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest UDP forward task failed: {e}")))?
    }

    /// Open a framed IPv6 loopback UDP connection over vsock.
    ///
    /// # Errors
    /// Requires GuestExec, a nonzero port and guest IPv6 socket support.
    pub async fn forward_udp_port_ipv6(
        &self,
        port: u16,
        timeout: Duration,
    ) -> Result<(crate::guest_agent::VsockStream, Vec<u8>)> {
        if port == 0 {
            return Err(AgentError::Script("UDP port must be nonzero".into()));
        }
        let device = self.file_channel()?;
        tokio::task::spawn_blocking(move || {
            GuestAgent::over_vsock(device, timeout)?.forward_udp_ipv6(port, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest IPv6 UDP forward task failed: {e}")))?
    }

    /// Have the guest mount a volume at `path`, served over a connection of
    /// its own: returned, with whatever already arrived on it, for a 9P
    /// server to run on.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability; propagates the guest's refusal.
    pub async fn mount_volume_in_guest(
        &self,
        path: &str,
        timeout: Duration,
    ) -> Result<(Box<dyn crate::GuestChannel>, Vec<u8>)> {
        let device = self.file_channel()?;
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            GuestAgent::over_vsock(device, timeout)?.mount_volume(&path, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest mount task failed: {e}")))?
    }

    /// What the guest is using, as it sees it.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability; propagates the guest's refusal.
    pub async fn stats_in_guest(&self, timeout: Duration) -> Result<hv2_guest_agent::GuestStats> {
        let device = self.file_channel()?;
        tokio::task::spawn_blocking(move || GuestAgent::over_vsock(device, timeout)?.stats(timeout))
            .await
            .map_err(|e| AgentError::Script(format!("guest stats task failed: {e}")))?
    }

    /// Read `path` from the guest, up to `limit` bytes.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability; propagates the guest's refusal.
    pub async fn read_file_in_guest(
        &self,
        path: &str,
        limit: u64,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let device = self.file_channel()?;
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.read_file(&path, limit, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest file read task failed: {e}")))?
    }

    /// The guest channel, for moving files: gated as running a command is,
    /// since writing any file is as much power over the guest.
    fn file_channel(
        &self,
    ) -> Result<std::sync::Arc<parking_lot::Mutex<hv2_core::devices::virtio_vsock::VsockDevice>>>
    {
        if !self.capabilities.has(Capability::GuestExec) {
            return Err(AgentError::PermissionDenied(
                "moving files in the guest requires the GuestExec capability".to_string(),
            ));
        }
        self.vm.vsock().ok_or_else(|| {
            AgentError::Script(
                "this VM has no guest channel: call attach_guest_channel() before the guest boots"
                    .to_string(),
            )
        })
    }

    /// Start a program in the guest and leave it running.
    ///
    /// Returns the guest pid. Unlike [`Self::exec_in_guest`], the program is
    /// still running when this returns -- which is what everything below
    /// needs, and what a one-shot exec cannot give: `Connect`, `SendInput`,
    /// `SendSignal` and the rest all require a process to still be there.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability. Propagates the guest agent's
    /// reason for not starting the program.
    pub async fn start_in_guest(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        envs: &BTreeMap<String, String>,
        pty: Option<PtySize>,
        timeout: Duration,
    ) -> Result<u32> {
        self.start_in_guest_as(program, args, cwd, envs, pty, None, timeout)
            .await
    }

    /// [`Self::start_in_guest`], as `user`: `None` is the template's user --
    /// a Dockerfile's last `USER` -- or root for a template that names none.
    ///
    /// # Errors
    ///
    /// As [`Self::start_in_guest`]; and a user the guest has no account for.
    #[allow(clippy::too_many_arguments)]
    pub async fn start_in_guest_as(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        envs: &BTreeMap<String, String>,
        pty: Option<PtySize>,
        user: Option<&str>,
        timeout: Duration,
    ) -> Result<u32> {
        let device = self.guest_channel("start a program in the guest")?;
        let program = program.to_string();
        let args = args.to_vec();
        let cwd = cwd.map(str::to_string);
        let envs = envs.clone();
        let user = user.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.start_as(
                &program,
                &args,
                cwd.as_deref(),
                &envs,
                pty,
                user.as_deref(),
                timeout,
            )
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest start task failed: {e}")))?
    }

    /// Collect what a started program has printed since the last poll.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability. Fails if the guest does not know
    /// that pid.
    pub async fn poll_in_guest(&self, pid: u32, timeout: Duration) -> Result<GuestOutput> {
        let device = self.guest_channel("poll a program in the guest")?;
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.poll(pid, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest poll task failed: {e}")))?
    }

    /// Write to a started program's standard input.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability. Fails if the guest does not know
    /// that pid, or its stdin is already closed.
    pub async fn write_stdin_in_guest(
        &self,
        pid: u32,
        data: &str,
        close: bool,
        timeout: Duration,
    ) -> Result<()> {
        let device = self.guest_channel("write to a program in the guest")?;
        let data = data.to_string();
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.write_stdin(pid, &data, close, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest write task failed: {e}")))?
    }

    /// Tell a program's terminal it is a different size.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability. Fails if the guest does not know
    /// that pid, or if the program was started with pipes and so has no
    /// terminal to resize.
    pub async fn resize_pty_in_guest(
        &self,
        pid: u32,
        size: PtySize,
        timeout: Duration,
    ) -> Result<()> {
        let device = self.guest_channel("resize a terminal in the guest")?;
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.resize_pty(pid, size, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest resize task failed: {e}")))?
    }

    /// After [`Self::launch_from_snapshot`]: set the guest's clock and reseed
    /// its random number generator from the host, so this guest does not
    /// share the RNG state of every other guest restored from the same
    /// snapshot.
    ///
    /// Against a guest agent too old to reseed, the clock is still set (with
    /// `date`), and the error that comes back says the reseed did not happen
    /// -- a caller should treat such a guest as sharing its randomness with
    /// its siblings, not carry on as though it did not.
    ///
    /// # Errors
    ///
    /// The agent could not be reached, or could not reseed.
    pub async fn after_restore(&self, timeout: Duration) -> Result<()> {
        let device = self.guest_channel("resynchronise a restored guest")?;
        let mut entropy = rand::random::<[u8; 32]>().to_vec();
        entropy.extend_from_slice(&rand::random::<[u8; 32]>());
        let vm_name = self.vm.config().name.clone();
        let queued_at = std::time::Instant::now();
        let reseeded = tokio::task::spawn_blocking(move || {
            let started_at = std::time::Instant::now();
            let mut agent = match GuestAgent::over_vsock(device, timeout) {
                Ok(agent) => agent,
                Err(error) => {
                    tracing::debug!(vm = %vm_name,
                        blocking_queue_ms = (started_at - queued_at).as_secs_f64() * 1000.0,
                        connect_ms = started_at.elapsed().as_secs_f64() * 1000.0,
                        succeeded = false, phase = "connect",
                        "restored guest readiness stages");
                    return Err(error);
                }
            };
            let connected_at = std::time::Instant::now();
            let result = agent.restored_now(entropy, timeout);
            tracing::debug!(vm = %vm_name,
                blocking_queue_ms = (started_at - queued_at).as_secs_f64() * 1000.0,
                connect_ms = (connected_at - started_at).as_secs_f64() * 1000.0,
                restored_ms = connected_at.elapsed().as_secs_f64() * 1000.0,
                succeeded = result.is_ok(), phase = "restored",
                "restored guest readiness stages");
            result
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest restore task failed: {e}")))?;
        if let Err(e) = reseeded {
            // An older agent: the clock at least can still be set.
            self.set_guest_clock(timeout).await?;
            return Err(AgentError::Script(format!(
                "the guest's clock is set, but its RNG was not reseeded ({e}); it shares \
                 random state with every guest restored from the same snapshot"
            )));
        }
        Ok(())
    }

    /// Send a signal to a started program.
    ///
    /// # Errors
    ///
    /// Requires the `GuestExec` capability. Fails if the guest does not know
    /// that pid -- which includes every pid the agent did not start, so this
    /// cannot be used to signal arbitrary processes in the guest.
    pub async fn signal_in_guest(&self, pid: u32, signal: i32, timeout: Duration) -> Result<()> {
        let device = self.guest_channel("signal a program in the guest")?;
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.signal(pid, signal, timeout)
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest signal task failed: {e}")))?
    }

    /// The vsock device to talk to the guest agent over, with the capability
    /// checked.
    ///
    /// One place rather than five copies: every call above needs the same two
    /// refusals, and five copies of them is five chances for one to drift.
    fn guest_channel(
        &self,
        what: &str,
    ) -> Result<std::sync::Arc<parking_lot::Mutex<hv2_core::devices::virtio_vsock::VsockDevice>>>
    {
        if !self.capabilities.has(Capability::GuestExec) {
            return Err(AgentError::PermissionDenied(format!(
                "to {what} requires the GuestExec capability"
            )));
        }
        self.vm.vsock().ok_or_else(|| {
            AgentError::Script(format!(
                "this VM has no guest channel, so it cannot {what}: call \
                 attach_guest_channel() before the guest boots"
            ))
        })
    }

    /// Ask the guest agent to identify itself, returning its version.
    ///
    /// The cheapest honest answer to "is this VM ready to be given work": a
    /// running guest with no agent in it looks exactly like a running guest
    /// with one, until something asks.
    pub async fn ping_guest(&self, timeout: Duration) -> Result<String> {
        if !self.capabilities.has(Capability::GuestExec) {
            return Err(AgentError::PermissionDenied(
                "talking to the guest agent requires the GuestExec capability".to_string(),
            ));
        }
        let Some(device) = self.vm.vsock() else {
            return Err(AgentError::Script(
                "this VM has no guest channel: call attach_guest_channel() first".to_string(),
            ));
        };

        let vm_name = self.vm.config().name.clone();
        let queued_at = std::time::Instant::now();
        tokio::task::spawn_blocking(move || {
            let started_at = std::time::Instant::now();
            let mut agent = match GuestAgent::over_vsock(device, timeout) {
                Ok(agent) => agent,
                Err(error) => {
                    tracing::debug!(target: "hv2_agent::cold_readiness", vm = %vm_name,
                        blocking_queue_ms = (started_at - queued_at).as_secs_f64() * 1000.0,
                        connect_ms = started_at.elapsed().as_secs_f64() * 1000.0,
                        succeeded = false, phase = "connect",
                        "cold guest readiness stages");
                    return Err(error);
                }
            };
            let connected_at = std::time::Instant::now();
            let result = agent.ping(timeout);
            tracing::debug!(target: "hv2_agent::cold_readiness", vm = %vm_name,
                blocking_queue_ms = (started_at - queued_at).as_secs_f64() * 1000.0,
                connect_ms = (connected_at - started_at).as_secs_f64() * 1000.0,
                ping_ms = connected_at.elapsed().as_secs_f64() * 1000.0,
                succeeded = result.is_ok(), phase = "ping",
                "cold guest readiness stages");
            result
        })
        .await
        .map_err(|e| AgentError::Script(format!("guest ping task failed: {e}")))?
    }

    /// Get the underlying VM
    pub fn vm(&self) -> Arc<VM> {
        Arc::clone(&self.vm)
    }
}

/// VM metrics for AI agent monitoring
#[derive(Debug, Clone, serde::Serialize)]
pub struct VMMetrics {
    pub state: VMState,
    pub vcpu_count: u32,
    pub memory_size: u64,
    pub uptime_seconds: u64,
    /// CPU usage as percentage (0-100) across all vCPUs
    pub cpu_usage_percent: Option<f64>,
    /// Memory used in bytes (requires guest OS support via virtio-balloon)
    pub memory_used_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an AgentVM with `capabilities`, skipping when this host has no
    /// hypervisor backend.
    async fn agent_vm(capabilities: CapabilitySet) -> Option<AgentVM> {
        match AgentVM::builder()
            .name("guest-exec-vm")
            .cpu_cores(1)
            .memory_gb(1)
            .capabilities(capabilities)
            .build()
            .await
        {
            Ok(vm) => Some(vm),
            Err(e) => {
                eprintln!("skipping: no hypervisor backend available ({e})");
                None
            }
        }
    }

    #[tokio::test]
    async fn running_a_command_in_the_guest_requires_the_guest_exec_capability() {
        // The default set is VmRead + Metrics. Reading a VM and running
        // arbitrary programs inside it are not the same power, and a set that
        // conflated them would grant the second to every caller of the first.
        let Some(vm) = agent_vm(CapabilitySet::default()).await else {
            return;
        };

        let err = vm
            .exec_in_guest("uname", &[], Duration::from_secs(1))
            .await
            .expect_err("the default set must not run commands in the guest");
        assert!(matches!(err, AgentError::PermissionDenied(_)), "got: {err}");
    }

    #[tokio::test]
    async fn a_vm_with_no_guest_channel_says_so_rather_than_timing_out() {
        let Some(vm) = agent_vm(CapabilitySet::all()).await else {
            return;
        };

        // "No device was ever attached" and "the guest never answered" send an
        // operator to entirely different places. Reporting the first as a
        // timeout would have them looking inside a guest for a device the host
        // never gave it.
        assert!(vm.guest_kernel_args().is_none());
        let err = vm
            .exec_in_guest("uname", &[], Duration::from_millis(50))
            .await
            .expect_err("no channel is not a slow guest");
        assert!(err.to_string().contains("no guest channel"), "got: {err}");
        assert!(!matches!(err, AgentError::Timeout(_)));
    }

    #[tokio::test]
    async fn attaching_a_channel_gives_the_guest_something_to_be_told_about() {
        let Some(vm) = agent_vm(CapabilitySet::all()).await else {
            return;
        };

        vm.attach_guest_channel(3).await.expect("attach");

        // virtio-mmio has no enumeration, so these arguments are the whole of
        // how a guest learns the channel exists.
        let args = vm.guest_kernel_args().expect("kernel args");
        assert!(args.starts_with("virtio_mmio.device="), "got: {args}");
    }

    #[tokio::test]
    async fn a_channel_with_nothing_listening_times_out_and_says_why() {
        let Some(vm) = agent_vm(CapabilitySet::all()).await else {
            return;
        };
        vm.attach_guest_channel(3).await.expect("attach");

        // The device is attached but no guest is running, so nothing services
        // the queues. This is the case that must not hang.
        let err = vm
            .exec_in_guest("uname", &[], Duration::from_millis(200))
            .await
            .expect_err("an unanswered channel is a timeout");
        assert!(matches!(err, AgentError::Timeout(_)), "got: {err}");
        assert!(
            err.to_string().contains("hv2-guest-agentd"),
            "the message should name what might be missing: {err}"
        );
    }

    #[tokio::test]
    async fn test_agent_vm_builder() {
        // Skip when no hypervisor backend is available (e.g. CI / WSL2 without
        // /dev/kvm access); AgentVM::build constructs a real VM underneath.
        let built = AgentVM::builder()
            .name("test-vm")
            .cpu_cores(2)
            .memory_gb(4)
            .enable_gpu(false)
            .build()
            .await;
        let Ok(vm) = built else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        assert_eq!(vm.state(), VMState::Created);
    }

    #[tokio::test]
    async fn cpu_usage_is_none_until_a_vcpu_has_executed() {
        // A VM started without a boot source reaches Running but never
        // enters the run loop. 0% there would read as an idle guest; there
        // is no guest.
        let Ok(vm) = AgentVM::builder().name("never-ran").build().await else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        assert_eq!(vm.cpu_usage_percent().await, None, "before start");

        vm.start().await.unwrap();
        assert_eq!(
            vm.cpu_usage_percent().await,
            None,
            "started but never executed is not the same as idle"
        );

        let metrics = vm.get_metrics().await.unwrap();
        assert_eq!(metrics.cpu_usage_percent, None);
        vm.stop().await.unwrap();
    }

    #[tokio::test]
    async fn the_stricter_of_the_two_script_bounds_wins() {
        // Neither setting had a builder method before, so both were pinned at
        // their defaults and `SandboxConfig` bounded nothing.
        let tight_sandbox = SandboxConfig {
            max_cpu_time: 5,
            ..Default::default()
        };
        let Ok(vm) = AgentVM::builder()
            .name("bounded")
            .script_timeout(Duration::from_secs(600))
            .sandbox(tight_sandbox)
            .build()
            .await
        else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        assert_eq!(vm.effective_script_timeout(), Duration::from_secs(5));

        // ...and the same in the other direction.
        let Ok(vm) = AgentVM::builder()
            .name("bounded-other-way")
            .script_timeout(Duration::from_secs(2))
            .build()
            .await
        else {
            return;
        };
        assert_eq!(vm.effective_script_timeout(), Duration::from_secs(2));
    }

    #[tokio::test]
    async fn a_vm_built_without_vm_read_refuses_scripts() {
        // The capability gate is only meaningful if a caller can actually
        // install a set that lacks `VmRead`.
        let Ok(vm) = AgentVM::builder()
            .name("uncapable")
            .capabilities(CapabilitySet::new())
            .build()
            .await
        else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        let err = vm
            .execute_agent_script("vcpu_count")
            .await
            .expect_err("must refuse");
        assert!(err.to_string().contains("VmRead"), "got: {err}");
    }

    #[tokio::test]
    async fn test_agent_vm_lifecycle() {
        let Ok(vm) = AgentVM::builder().name("lifecycle-test").build().await else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        vm.start().await.unwrap();
        assert_eq!(vm.state(), VMState::Running);

        vm.stop().await.unwrap();
        assert_eq!(vm.state(), VMState::Stopped);
    }

    #[tokio::test]
    async fn console_output_is_none_until_a_console_is_attached() {
        let Ok(vm) = AgentVM::builder().name("no-console").build().await else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        // Not `Some("")`: an agent debugging a silent boot has to be able to
        // tell a quiet guest from a VM with nowhere to print.
        assert_eq!(vm.console_output().await, None);
    }

    #[tokio::test]
    async fn console_output_returns_what_the_guest_wrote() {
        use hv2_core::{Device, SerialDevice};
        use tokio::sync::RwLock as AsyncRwLock;

        let Ok(vm) = AgentVM::builder().name("with-console").build().await else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        let device = Arc::new(AsyncRwLock::new(SerialDevice::new(
            "COM1".to_string(),
            0x3F8,
        )));
        {
            let mut guard = device.write().await;
            for byte in b"hello" {
                guard.write(0, &[*byte]).await.unwrap();
            }
        }
        vm.vm()
            .devices()
            .register_device("COM1", device)
            .await
            .unwrap();

        assert_eq!(vm.console_output().await.as_deref(), Some("hello"));
        assert_eq!(
            vm.console_output().await.as_deref(),
            Some("hello"),
            "reading must not drain"
        );
    }
}
