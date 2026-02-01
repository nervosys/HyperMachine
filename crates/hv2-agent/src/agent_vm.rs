//! AI-scriptable VM with enhanced capabilities for autonomous agents

use crate::{AgentError, CapabilitySet, Result, Sandbox, SandboxConfig, ScriptEngine};
use hv2_core::{VMConfig, VMState, VM};
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

    pub fn script_timeout(mut self, timeout: Duration) -> Self {
        self.script_timeout = timeout;
        self
    }

    pub async fn build(self) -> Result<AgentVM> {
        let vm = VM::new(self.config)?;
        let script_engine = ScriptEngine::new(self.capabilities);
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

    /// Execute an AI agent script
    pub async fn execute_agent_script(&self, script: &str) -> Result<serde_json::Value> {
        tracing::info!("Executing agent script");

        // Run script in sandbox with timeout
        let vm = Arc::clone(&self.vm);
        let script_engine = Arc::clone(&self.script_engine);
        let script = script.to_string();

        let result = timeout(self.script_timeout, async move {
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

    /// Start the VM
    pub async fn start(&self) -> Result<()> {
        self.vm.start().await?;
        *self.started_at.write().await = Some(Instant::now());
        Ok(())
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
            uptime_seconds: self.started_at.read().await.map(|s| s.elapsed().as_secs()).unwrap_or(0),
            cpu_usage_percent: None,
            memory_used_bytes: None,
        })
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
        let vm = AgentVM::builder()
            .name("test-vm")
            .cpu_cores(2)
            .memory_gb(4)
            .enable_gpu(false)
            .build()
            .await
            .unwrap();

        assert_eq!(vm.state(), VMState::Created);
    }

    #[tokio::test]
    async fn test_agent_vm_lifecycle() {
        let vm = AgentVM::builder()
            .name("lifecycle-test")
            .build()
            .await
            .unwrap();

        vm.start().await.unwrap();
        assert_eq!(vm.state(), VMState::Running);

        vm.stop().await.unwrap();
        assert_eq!(vm.state(), VMState::Stopped);
    }
}
