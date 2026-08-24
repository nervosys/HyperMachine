//! AI-scriptable VM with enhanced capabilities for autonomous agents

use crate::{AgentError, CapabilitySet, Result, Sandbox, SandboxConfig, ScriptEngine};
use hv2_core::{BootSource, VMConfig, VMState, VM};
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
        let script_engine = ScriptEngine::with_limits(self.capabilities, &self.sandbox_config);
        let sandbox = Sandbox::new(self.sandbox_config);

        Ok(AgentVM {
            vm: Arc::new(vm),
            script_engine: Arc::new(script_engine),
            sandbox: Arc::new(sandbox),
            script_timeout: self.script_timeout,
            started_at: RwLock::new(None),
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
