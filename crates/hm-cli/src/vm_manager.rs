//! VM Manager - tracks and persists VM instances
//!
//! Provides a central registry for all VMs managed by the HyperMachine CLI,
//! with state persistence across CLI invocations.

use anyhow::{Context, Result};
use hv2_agent::AgentVM;
use hv2_core::VMState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// VM record stored in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmRecord {
    /// VM name (unique identifier)
    pub name: String,
    /// Number of vCPUs
    pub cpu_cores: u32,
    /// Memory size in GB
    pub memory_gb: u64,
    /// GPU enabled
    pub gpu_enabled: bool,
    /// Networking enabled
    pub network_enabled: bool,
    /// Current state (persisted)
    pub state: VmState,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last state change timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Process ID if running
    #[serde(skip)]
    pub pid: Option<u32>,
}

/// Simplified VM state for persistence
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmState {
    Created,
    Running,
    Paused,
    Stopped,
    Error,
}

impl From<VMState> for VmState {
    fn from(state: VMState) -> Self {
        match state {
            VMState::Created => VmState::Created,
            VMState::Running => VmState::Running,
            VMState::Paused => VmState::Paused,
            VMState::Stopped => VmState::Stopped,
            VMState::Error => VmState::Error,
            _ => VmState::Error,
        }
    }
}

impl std::fmt::Display for VmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmState::Created => write!(f, "Created"),
            VmState::Running => write!(f, "Running"),
            VmState::Paused => write!(f, "Paused"),
            VmState::Stopped => write!(f, "Stopped"),
            VmState::Error => write!(f, "Error"),
        }
    }
}

/// VM Registry persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmRegistry {
    /// All VM records
    pub vms: HashMap<String, VmRecord>,
    /// Registry version for migrations
    pub version: u32,
}

/// VM Manager - handles VM lifecycle and persistence
pub struct VmManager {
    /// Path to state directory
    state_dir: PathBuf,
    /// In-memory registry
    registry: RwLock<VmRegistry>,
    /// Active VM instances (running VMs)
    active_vms: RwLock<HashMap<String, Arc<AgentVM>>>,
}

impl VmManager {
    /// Create a new VM manager
    pub fn new() -> Result<Self> {
        let state_dir = Self::default_state_dir()?;
        Self::with_state_dir(state_dir)
    }

    /// Create VM manager with in-memory storage (for testing)
    ///
    /// Each call creates a unique isolated directory to ensure test isolation
    /// even when tests run in parallel within the same process.
    pub fn new_in_memory() -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique_id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!(
            "hypermachine-test-{}-{}",
            std::process::id(),
            unique_id
        ));
        std::fs::create_dir_all(&tmp)?;
        Self::with_state_dir(tmp)
    }

    /// Create VM manager with custom state directory
    pub fn with_state_dir(state_dir: PathBuf) -> Result<Self> {
        // Ensure state directory exists
        std::fs::create_dir_all(&state_dir)
            .with_context(|| format!("Failed to create state directory: {:?}", state_dir))?;

        // Load existing registry
        let registry_path = state_dir.join("registry.json");
        let registry = if registry_path.exists() {
            let data = std::fs::read_to_string(&registry_path)
                .with_context(|| "Failed to read registry file")?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            VmRegistry::default()
        };

        Ok(Self {
            state_dir,
            registry: RwLock::new(registry),
            active_vms: RwLock::new(HashMap::new()),
        })
    }

    /// Get the default state directory
    fn default_state_dir() -> Result<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("C:\\ProgramData"))
        } else if cfg!(target_os = "macos") {
            dirs::home_dir()
                .map(|h| h.join("Library/Application Support"))
                .unwrap_or_else(|| PathBuf::from("/var/lib"))
        } else {
            dirs::home_dir()
                .map(|h| h.join(".local/share"))
                .unwrap_or_else(|| PathBuf::from("/var/lib"))
        };

        Ok(base.join("hypermachine"))
    }

    /// Save registry to disk
    async fn save_registry(&self) -> Result<()> {
        let registry = self.registry.read().await;
        let data = serde_json::to_string_pretty(&*registry)?;
        let path = self.state_dir.join("registry.json");
        std::fs::write(&path, data)
            .with_context(|| format!("Failed to write registry to {:?}", path))?;
        Ok(())
    }

    /// Create a new VM
    pub async fn create_vm(
        &self,
        name: &str,
        cpu_cores: u32,
        memory_gb: u64,
        gpu_enabled: bool,
        network_enabled: bool,
    ) -> Result<VmRecord> {
        let mut registry = self.registry.write().await;

        // Check if VM already exists
        if registry.vms.contains_key(name) {
            anyhow::bail!("VM '{}' already exists", name);
        }

        let now = chrono::Utc::now();
        let record = VmRecord {
            name: name.to_string(),
            cpu_cores,
            memory_gb,
            gpu_enabled,
            network_enabled,
            state: VmState::Created,
            created_at: now,
            updated_at: now,
            pid: None,
        };

        registry.vms.insert(name.to_string(), record.clone());
        drop(registry);

        self.save_registry().await?;

        tracing::info!(
            "Created VM '{}' with {} cores, {} GB memory",
            name,
            cpu_cores,
            memory_gb
        );
        Ok(record)
    }

    /// Start a VM
    pub async fn start_vm(&self, name: &str) -> Result<()> {
        // Get VM record
        let record = {
            let registry = self.registry.read().await;
            registry
                .vms
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))?
        };

        // Check state
        if record.state == VmState::Running {
            anyhow::bail!("VM '{}' is already running", name);
        }

        // Build and start the AgentVM
        let vm = AgentVM::builder()
            .name(name)
            .cpu_cores(record.cpu_cores)
            .memory_gb(record.memory_gb)
            .enable_gpu(record.gpu_enabled)
            .enable_networking(record.network_enabled)
            .with_tracing()
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build VM: {}", e))?;

        vm.start()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start VM: {}", e))?;

        // Store active VM
        {
            let mut active = self.active_vms.write().await;
            active.insert(name.to_string(), Arc::new(vm));
        }

        // Update registry
        {
            let mut registry = self.registry.write().await;
            if let Some(record) = registry.vms.get_mut(name) {
                record.state = VmState::Running;
                record.updated_at = chrono::Utc::now();
            }
        }

        self.save_registry().await?;

        tracing::info!("Started VM '{}'", name);
        Ok(())
    }

    /// Stop a VM
    pub async fn stop_vm(&self, name: &str) -> Result<()> {
        // Check if VM exists
        {
            let registry = self.registry.read().await;
            if !registry.vms.contains_key(name) {
                anyhow::bail!("VM '{}' not found", name);
            }
        }

        // Stop active VM if running
        {
            let mut active = self.active_vms.write().await;
            if let Some(vm) = active.remove(name) {
                vm.stop()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to stop VM: {}", e))?;
            }
        }

        // Update registry
        {
            let mut registry = self.registry.write().await;
            if let Some(record) = registry.vms.get_mut(name) {
                record.state = VmState::Stopped;
                record.updated_at = chrono::Utc::now();
                record.pid = None;
            }
        }

        self.save_registry().await?;

        tracing::info!("Stopped VM '{}'", name);
        Ok(())
    }

    /// Delete a VM
    pub async fn delete_vm(&self, name: &str) -> Result<()> {
        // Stop VM if running
        self.stop_vm(name).await.ok();

        // Remove from registry
        {
            let mut registry = self.registry.write().await;
            registry.vms.remove(name);
        }

        self.save_registry().await?;

        tracing::info!("Deleted VM '{}'", name);
        Ok(())
    }

    /// Get VM status
    pub async fn get_vm(&self, name: &str) -> Result<VmRecord> {
        let registry = self.registry.read().await;
        registry
            .vms
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("VM '{}' not found", name))
    }

    /// List all VMs
    pub async fn list_vms(&self) -> Vec<VmRecord> {
        let registry = self.registry.read().await;
        registry.vms.values().cloned().collect()
    }

    /// Get active VM instance
    pub async fn get_active_vm(&self, name: &str) -> Option<Arc<AgentVM>> {
        let active = self.active_vms.read().await;
        active.get(name).cloned()
    }

    /// Execute a script on a VM
    pub async fn execute_script(&self, name: &str, script: &str) -> Result<serde_json::Value> {
        let vm = self
            .get_active_vm(name)
            .await
            .ok_or_else(|| anyhow::anyhow!("VM '{}' is not running", name))?;

        vm.execute_agent_script(script)
            .await
            .map_err(|e| anyhow::anyhow!("Script execution failed: {}", e))
    }

    /// Get VM metrics
    pub async fn get_metrics(&self, name: &str) -> Result<VmMetrics> {
        let record = self.get_vm(name).await?;

        let metrics = if let Some(vm) = self.get_active_vm(name).await {
            let vm_metrics = vm
                .get_metrics()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            VmMetrics {
                name: record.name,
                state: record.state,
                cpu_cores: record.cpu_cores,
                memory_gb: record.memory_gb,
                memory_used_gb: vm_metrics
                    .memory_used_bytes
                    .map(|b| b as f64 / (1024.0 * 1024.0 * 1024.0)),
                cpu_usage_percent: vm_metrics.cpu_usage_percent,
                uptime_seconds: Some(vm_metrics.uptime_seconds),
            }
        } else {
            VmMetrics {
                name: record.name,
                state: record.state,
                cpu_cores: record.cpu_cores,
                memory_gb: record.memory_gb,
                memory_used_gb: None,
                cpu_usage_percent: None,
                uptime_seconds: None,
            }
        };

        Ok(metrics)
    }
}

/// VM metrics for display
#[derive(Debug, Clone, Serialize)]
pub struct VmMetrics {
    pub name: String,
    pub state: VmState,
    pub cpu_cores: u32,
    pub memory_gb: u64,
    pub memory_used_gb: Option<f64>,
    pub cpu_usage_percent: Option<f64>,
    pub uptime_seconds: Option<u64>,
}

impl Default for VmManager {
    fn default() -> Self {
        Self::new().expect("Failed to create default VmManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_and_list_vm() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        // Create a VM
        manager
            .create_vm("test-vm", 2, 4, false, true)
            .await
            .unwrap();

        // List VMs
        let vms = manager.list_vms().await;
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].name, "test-vm");
        assert_eq!(vms[0].cpu_cores, 2);
        assert_eq!(vms[0].memory_gb, 4);
    }

    #[tokio::test]
    async fn test_duplicate_vm_fails() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        manager
            .create_vm("test-vm", 2, 4, false, false)
            .await
            .unwrap();

        let result = manager.create_vm("test-vm", 4, 8, false, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_vm() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        manager
            .create_vm("test-vm", 2, 4, false, false)
            .await
            .unwrap();

        manager.delete_vm("test-vm").await.unwrap();

        let vms = manager.list_vms().await;
        assert!(vms.is_empty());
    }

    #[tokio::test]
    async fn test_persistence() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().to_path_buf();

        // Create manager and add VM
        {
            let manager = VmManager::with_state_dir(path.clone()).unwrap();
            manager
                .create_vm("persist-vm", 4, 16, true, true)
                .await
                .unwrap();
        }

        // Create new manager and verify VM persisted
        {
            let manager = VmManager::with_state_dir(path).unwrap();
            let vms = manager.list_vms().await;
            assert_eq!(vms.len(), 1);
            assert_eq!(vms[0].name, "persist-vm");
            assert_eq!(vms[0].cpu_cores, 4);
            assert_eq!(vms[0].memory_gb, 16);
            assert!(vms[0].gpu_enabled);
        }
    }

    #[tokio::test]
    async fn test_get_vm() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        manager
            .create_vm("my-vm", 8, 32, true, false)
            .await
            .unwrap();

        let vm = manager.get_vm("my-vm").await.unwrap();
        assert_eq!(vm.name, "my-vm");
        assert_eq!(vm.cpu_cores, 8);
        assert_eq!(vm.memory_gb, 32);
        assert!(vm.gpu_enabled);
        assert!(!vm.network_enabled);
    }

    #[tokio::test]
    async fn test_get_vm_not_found() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        let result = manager.get_vm("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vm_state_transitions() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        let record = manager
            .create_vm("state-vm", 2, 4, false, false)
            .await
            .unwrap();
        assert_eq!(record.state, VmState::Created);

        // Update state manually through internal method if exposed
        let vm = manager.get_vm("state-vm").await.unwrap();
        assert_eq!(vm.state, VmState::Created);
    }

    #[test]
    fn test_vm_state_display() {
        assert_eq!(format!("{}", VmState::Created), "Created");
        assert_eq!(format!("{}", VmState::Running), "Running");
        assert_eq!(format!("{}", VmState::Paused), "Paused");
        assert_eq!(format!("{}", VmState::Stopped), "Stopped");
        assert_eq!(format!("{}", VmState::Error), "Error");
    }

    #[test]
    fn test_vm_state_from_vmstate() {
        use hv2_core::VMState;

        assert_eq!(VmState::from(VMState::Created), VmState::Created);
        assert_eq!(VmState::from(VMState::Running), VmState::Running);
        assert_eq!(VmState::from(VMState::Paused), VmState::Paused);
        assert_eq!(VmState::from(VMState::Stopped), VmState::Stopped);
        assert_eq!(VmState::from(VMState::Error), VmState::Error);
    }

    #[tokio::test]
    async fn test_get_metrics_not_running() {
        let tmp = TempDir::new().unwrap();
        let manager = VmManager::with_state_dir(tmp.path().to_path_buf()).unwrap();

        manager
            .create_vm("metrics-vm", 4, 8, false, false)
            .await
            .unwrap();

        let metrics = manager.get_metrics("metrics-vm").await.unwrap();
        assert_eq!(metrics.name, "metrics-vm");
        assert_eq!(metrics.cpu_cores, 4);
        assert_eq!(metrics.memory_gb, 8);
        assert!(metrics.uptime_seconds.is_none()); // Not running
    }

    #[test]
    fn test_vm_metrics_serialization() {
        let metrics = VmMetrics {
            name: "test".to_string(),
            state: VmState::Running,
            cpu_cores: 4,
            memory_gb: 8,
            memory_used_gb: Some(4.5),
            cpu_usage_percent: Some(25.0),
            uptime_seconds: Some(3600),
        };

        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("\"uptime_seconds\":3600"));
        assert!(json.contains("\"cpu_usage_percent\":25.0"));
    }

    #[tokio::test]
    async fn test_new_in_memory_isolation() {
        // Create two managers - they should have separate state
        let manager1 = VmManager::new_in_memory().unwrap();
        let manager2 = VmManager::new_in_memory().unwrap();

        manager1.create_vm("vm1", 2, 4, false, false).await.unwrap();
        manager2.create_vm("vm2", 4, 8, false, false).await.unwrap();

        // Each manager should only see its own VM
        let vms1 = manager1.list_vms().await;
        let vms2 = manager2.list_vms().await;

        assert_eq!(vms1.len(), 1);
        assert_eq!(vms1[0].name, "vm1");

        assert_eq!(vms2.len(), 1);
        assert_eq!(vms2[0].name, "vm2");
    }

    #[test]
    fn test_vm_record_serialization() {
        let now = chrono::Utc::now();
        let record = VmRecord {
            name: "test-vm".to_string(),
            cpu_cores: 4,
            memory_gb: 16,
            gpu_enabled: true,
            network_enabled: true,
            state: VmState::Running,
            created_at: now,
            updated_at: now,
            pid: None,
        };

        // Serialize to JSON and back
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: VmRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "test-vm");
        assert_eq!(deserialized.cpu_cores, 4);
        assert_eq!(deserialized.memory_gb, 16);
        assert!(deserialized.gpu_enabled);
        assert!(deserialized.network_enabled);
        assert_eq!(deserialized.state, VmState::Running);
    }
}
