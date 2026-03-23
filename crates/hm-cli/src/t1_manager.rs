//! Type-1 (Bare-Metal) Hypervisor Manager
//!
//! Manages Type-1 hypervisor VMs through:
//! 1. VM configuration file creation/management
//! 2. Remote API connection to running T1 hypervisor
//! 3. Build/image creation utilities
//!
//! Unlike T2 (hosted) mode, T1 VMs run directly on hardware without a host OS.
//! The CLI can only:
//! - Prepare VM configurations before hypervisor boot
//! - Connect to a running hypervisor via network API
//! - Build bootable hypervisor images

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

/// T1 VM configuration record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T1VmConfig {
    /// VM name (unique identifier)
    pub name: String,
    /// Number of vCPUs
    pub cpu_cores: u32,
    /// Memory size in GB
    pub memory_gb: u64,
    /// GPU passthrough enabled
    pub gpu_passthrough: bool,
    /// Network mode
    pub network: T1NetworkConfig,
    /// Boot configuration
    pub boot: T1BootConfig,
    /// Device passthrough list
    pub devices: Vec<T1DeviceConfig>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last modification timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Network configuration for T1 VM
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct T1NetworkConfig {
    /// Enable SR-IOV network
    pub sriov: bool,
    /// Virtual MAC address
    pub mac_address: Option<String>,
    /// VLAN tag
    pub vlan: Option<u16>,
}

/// Boot configuration for T1 VM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T1BootConfig {
    /// Kernel/image path
    pub kernel: Option<String>,
    /// Initramfs path
    pub initrd: Option<String>,
    /// Kernel command line
    pub cmdline: String,
    /// UEFI boot
    pub uefi: bool,
}

impl Default for T1BootConfig {
    fn default() -> Self {
        Self {
            kernel: None,
            initrd: None,
            cmdline: String::new(),
            uefi: true,
        }
    }
}

/// Device passthrough configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T1DeviceConfig {
    /// Device type
    pub device_type: T1DeviceType,
    /// PCI address (for passthrough)
    pub pci_address: Option<String>,
    /// Device-specific options
    pub options: HashMap<String, String>,
}

/// T1 device types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum T1DeviceType {
    /// PCI device passthrough
    PciPassthrough,
    /// Emulated serial console
    SerialConsole,
    /// Virtio block device
    VirtioBlock,
    /// Virtio network device
    VirtioNet,
}

/// T1 VM runtime state (from hypervisor API)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum T1VmState {
    /// Configuration exists, not deployed
    Configured,
    /// Running on hypervisor
    Running,
    /// Paused
    Paused,
    /// Stopped
    Stopped,
    /// Error state
    Error,
    /// Unknown (no connection to hypervisor)
    Unknown,
}

impl std::fmt::Display for T1VmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            T1VmState::Configured => write!(f, "Configured"),
            T1VmState::Running => write!(f, "Running"),
            T1VmState::Paused => write!(f, "Paused"),
            T1VmState::Stopped => write!(f, "Stopped"),
            T1VmState::Error => write!(f, "Error"),
            T1VmState::Unknown => write!(f, "Unknown"),
        }
    }
}

/// T1 VM runtime metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T1VmMetrics {
    /// VM name
    pub name: String,
    /// Runtime state
    pub state: T1VmState,
    /// CPU cores allocated
    pub cpu_cores: u32,
    /// Memory allocated (GB)
    pub memory_gb: u64,
    /// Uptime in seconds
    pub uptime_seconds: Option<u64>,
    /// CPU usage percentage
    pub cpu_usage: Option<f32>,
    /// Memory usage percentage
    pub memory_usage: Option<f32>,
}

/// T1 VM registry stored to disk
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct T1VmRegistry {
    /// All VM configurations
    pub vms: HashMap<String, T1VmConfig>,
    /// Registry version
    pub version: u32,
}

/// Result of executing a script on a T1 VM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T1ScriptResult {
    /// Whether the script completed successfully
    pub success: bool,
    /// Standard output from the script
    pub stdout: String,
    /// Standard error from the script
    pub stderr: String,
    /// Exit code of the script
    pub exit_code: Option<i32>,
    /// Execution duration in milliseconds
    pub duration_ms: Option<u64>,
}
/// Remote hypervisor connection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T1HypervisorConnection {
    /// Hypervisor API endpoint
    pub endpoint: String,
    /// Port number
    pub port: u16,
    /// TLS enabled
    pub tls: bool,
    /// Authentication token
    pub auth_token: Option<String>,
}

impl Default for T1HypervisorConnection {
    fn default() -> Self {
        Self {
            endpoint: "127.0.0.1".to_string(),
            port: 8443,
            tls: true,
            auth_token: None,
        }
    }
}

/// Type-1 Hypervisor Manager
pub struct T1Manager {
    /// Path to configuration directory
    config_dir: PathBuf,
    /// VM registry
    registry: RwLock<T1VmRegistry>,
    /// Remote hypervisor connection
    connection: RwLock<Option<T1HypervisorConnection>>,
}

impl T1Manager {
    /// Create a new T1 manager with default config directory
    pub fn new() -> Result<Self> {
        let config_dir = Self::default_config_dir()?;
        Self::with_config_dir(config_dir)
    }

    /// Create T1 manager with custom config directory
    pub fn with_config_dir(config_dir: PathBuf) -> Result<Self> {
        // Ensure config directory exists
        std::fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create T1 config directory: {:?}", config_dir))?;

        // Load existing registry
        let registry_path = config_dir.join("t1-registry.json");
        let registry = if registry_path.exists() {
            let data = std::fs::read_to_string(&registry_path)
                .with_context(|| "Failed to read T1 registry file")?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            T1VmRegistry::default()
        };

        // Load connection settings if exist
        let conn_path = config_dir.join("hypervisor.json");
        let connection = if conn_path.exists() {
            let data = std::fs::read_to_string(&conn_path).ok();
            data.and_then(|d| serde_json::from_str(&d).ok())
        } else {
            None
        };

        Ok(Self {
            config_dir,
            registry: RwLock::new(registry),
            connection: RwLock::new(connection),
        })
    }

    /// Get the default config directory
    fn default_config_dir() -> Result<PathBuf> {
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
                .map(|h| h.join(".config"))
                .unwrap_or_else(|| PathBuf::from("/etc"))
        };
        Ok(base.join("hypermachine").join("t1"))
    }

    /// Persist the registry to disk
    async fn save_registry(&self) -> Result<()> {
        let registry = self.registry.read().await;
        let path = self.config_dir.join("t1-registry.json");
        let data = serde_json::to_string_pretty(&*registry)?;
        std::fs::write(&path, data).with_context(|| "Failed to save T1 registry")?;
        Ok(())
    }

    /// Create a new T1 VM configuration
    pub async fn create_vm(
        &self,
        name: &str,
        cpu_cores: u32,
        memory_gb: u64,
        gpu_passthrough: bool,
        network_sriov: bool,
    ) -> Result<T1VmConfig> {
        let mut registry = self.registry.write().await;

        // Check if VM already exists
        if registry.vms.contains_key(name) {
            bail!("T1 VM '{}' already exists", name);
        }

        let now = chrono::Utc::now();
        let config = T1VmConfig {
            name: name.to_string(),
            cpu_cores,
            memory_gb,
            gpu_passthrough,
            network: T1NetworkConfig {
                sriov: network_sriov,
                ..Default::default()
            },
            boot: T1BootConfig::default(),
            devices: Vec::new(),
            created_at: now,
            updated_at: now,
        };

        registry.vms.insert(name.to_string(), config.clone());
        drop(registry);

        self.save_registry().await?;

        Ok(config)
    }

    /// Get a T1 VM configuration
    pub async fn get_vm(&self, name: &str) -> Result<T1VmConfig> {
        let registry = self.registry.read().await;
        registry
            .vms
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("T1 VM '{}' not found", name))
    }

    /// List all T1 VM configurations
    pub async fn list_vms(&self) -> Vec<T1VmConfig> {
        let registry = self.registry.read().await;
        registry.vms.values().cloned().collect()
    }

    /// Delete a T1 VM configuration
    pub async fn delete_vm(&self, name: &str) -> Result<()> {
        let mut registry = self.registry.write().await;

        if registry.vms.remove(name).is_none() {
            bail!("T1 VM '{}' not found", name);
        }

        drop(registry);
        self.save_registry().await?;

        Ok(())
    }

    /// Get VM status (requires hypervisor connection)
    pub async fn get_vm_status(&self, name: &str) -> Result<T1VmMetrics> {
        // First check if config exists
        let config = self.get_vm(name).await?;

        // Try to get runtime status from hypervisor
        if let Some(conn) = self.connection.read().await.as_ref() {
            match self.query_hypervisor_status(conn, name).await {
                Ok(metrics) => return Ok(metrics),
                Err(_) => {
                    // Hypervisor not available, return configured status
                }
            }
        }

        // No hypervisor connection, return configured state
        Ok(T1VmMetrics {
            name: config.name,
            state: T1VmState::Configured,
            cpu_cores: config.cpu_cores,
            memory_gb: config.memory_gb,
            uptime_seconds: None,
            cpu_usage: None,
            memory_usage: None,
        })
    }

    /// Build an HTTP client for the hypervisor API
    fn build_client(conn: &T1HypervisorConnection) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10));

        if conn.tls {
            // Accept self-signed certs for local hypervisor connections
            builder = builder.danger_accept_invalid_certs(true);
        }

        builder
            .build()
            .with_context(|| "Failed to create HTTP client")
    }

    /// Build the base URL for the hypervisor API
    fn base_url(conn: &T1HypervisorConnection) -> String {
        let scheme = if conn.tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, conn.endpoint, conn.port)
    }

    /// Query hypervisor for VM status (internal)
    async fn query_hypervisor_status(
        &self,
        conn: &T1HypervisorConnection,
        name: &str,
    ) -> Result<T1VmMetrics> {
        let client = Self::build_client(conn)?;
        let url = format!("{}/api/v1/vms/{}", Self::base_url(conn), name);

        let mut request = client.get(&url);
        if let Some(token) = &conn.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to connect to T1 hypervisor at {}", url))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "T1 hypervisor returned {} for VM '{}': {}",
                status,
                name,
                body
            );
        }

        let metrics: T1VmMetrics = response
            .json()
            .await
            .with_context(|| format!("Failed to parse VM status response for '{}'", name))?;

        Ok(metrics)
    }

    /// Start a VM (requires hypervisor connection)
    pub async fn start_vm(&self, name: &str) -> Result<()> {
        // Verify config exists
        let _config = self.get_vm(name).await?;

        let conn = self.connection.read().await;
        let Some(conn) = conn.as_ref() else {
            bail!(
                "No T1 hypervisor connection configured.\n\
                 T1 VMs run on bare-metal hypervisor, not on host OS.\n\
                 Configure connection with: hm t1 connect <endpoint>"
            );
        };
        let client = Self::build_client(conn)?;
        let url = format!("{}/api/v1/vms/{}/start", Self::base_url(conn), name);

        let mut request = client.post(&url);
        if let Some(token) = &conn.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to connect to T1 hypervisor to start '{}'", name))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to start VM '{}': {} {}", name, status, body);
        }

        Ok(())
    }

    /// Stop a VM (requires hypervisor connection)
    pub async fn stop_vm(&self, name: &str) -> Result<()> {
        // Verify config exists
        let _config = self.get_vm(name).await?;

        let conn = self.connection.read().await;
        let Some(conn) = conn.as_ref() else {
            bail!(
                "No T1 hypervisor connection configured.\n\
                 Configure connection with: hm t1 connect <endpoint>"
            );
        };
        let client = Self::build_client(conn)?;
        let url = format!("{}/api/v1/vms/{}/stop", Self::base_url(conn), name);

        let mut request = client.post(&url);
        if let Some(token) = &conn.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to connect to T1 hypervisor to stop '{}'", name))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to stop VM '{}': {} {}", name, status, body);
        }

        Ok(())
    }

    /// Configure hypervisor connection
    pub async fn set_connection(&self, conn: T1HypervisorConnection) -> Result<()> {
        let conn_path = self.config_dir.join("hypervisor.json");
        let data = serde_json::to_string_pretty(&conn)?;
        std::fs::write(&conn_path, data).with_context(|| "Failed to save hypervisor connection")?;

        *self.connection.write().await = Some(conn);
        Ok(())
    }

    /// Get current hypervisor connection
    pub async fn get_connection(&self) -> Option<T1HypervisorConnection> {
        self.connection.read().await.clone()
    }

    /// Check if hypervisor is reachable
    pub async fn ping_hypervisor(&self) -> Result<bool> {
        let conn = self.connection.read().await;
        if let Some(conn) = conn.as_ref() {
            let client = Self::build_client(conn)?;
            let url = format!("{}/health", Self::base_url(conn));

            let mut request = client.get(&url);
            if let Some(token) = &conn.auth_token {
                request = request.bearer_auth(token);
            }

            match request.send().await {
                Ok(response) => Ok(response.status().is_success()),
                Err(_) => Ok(false),
            }
        } else {
            Ok(false)
        }
    }

    /// Export VM configuration as JSON
    pub async fn export_config(&self, name: &str) -> Result<String> {
        let config = self.get_vm(name).await?;
        Ok(serde_json::to_string_pretty(&config)?)
    }

    /// Import VM configuration from JSON
    pub async fn import_config(&self, json: &str) -> Result<T1VmConfig> {
        let config: T1VmConfig =
            serde_json::from_str(json).with_context(|| "Invalid T1 VM configuration JSON")?;

        let mut registry = self.registry.write().await;

        if registry.vms.contains_key(&config.name) {
            bail!("T1 VM '{}' already exists", config.name);
        }

        registry.vms.insert(config.name.clone(), config.clone());
        drop(registry);

        self.save_registry().await?;

        Ok(config)
    }

    /// Execute a script on a running T1 VM via hypervisor API
    pub async fn execute_script(
        &self,
        name: &str,
        script: &str,
        timeout_secs: u64,
    ) -> Result<T1ScriptResult> {
        // Verify config exists
        let _config = self.get_vm(name).await?;

        let conn = self.connection.read().await;
        let Some(conn) = conn.as_ref() else {
            bail!(
                "No T1 hypervisor connection configured.\n\
                 Configure connection with: hm t1 connect <endpoint>"
            );
        };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs + 5))
            .build()
            .with_context(|| "Failed to create HTTP client")?;

        let url = format!("{}/api/v1/vms/{}/exec", Self::base_url(conn), name);

        let body = serde_json::json!({
            "script": script,
            "timeout": timeout_secs,
        });

        let mut request = client.post(&url).json(&body);
        if let Some(token) = &conn.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to execute script on VM '{}'", name))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Script execution failed on '{}': {} {}", name, status, body);
        }

        let result: T1ScriptResult = response
            .json()
            .await
            .with_context(|| "Failed to parse script execution response")?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_create_vm() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        let config = manager
            .create_vm("test-vm", 4, 8, false, false)
            .await
            .unwrap();

        assert_eq!(config.name, "test-vm");
        assert_eq!(config.cpu_cores, 4);
        assert_eq!(config.memory_gb, 8);
    }

    #[tokio::test]
    async fn test_list_vms() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager.create_vm("vm1", 2, 4, false, false).await.unwrap();
        manager.create_vm("vm2", 4, 8, true, true).await.unwrap();

        let vms = manager.list_vms().await;
        assert_eq!(vms.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_vm() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("test-vm", 4, 8, false, false)
            .await
            .unwrap();
        manager.delete_vm("test-vm").await.unwrap();

        let vms = manager.list_vms().await;
        assert!(vms.is_empty());
    }

    #[tokio::test]
    async fn test_get_vm_found() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("my-vm", 8, 16, true, true)
            .await
            .unwrap();

        let vm = manager.get_vm("my-vm").await.unwrap();
        assert_eq!(vm.name, "my-vm");
        assert_eq!(vm.cpu_cores, 8);
        assert_eq!(vm.memory_gb, 16);
        assert!(vm.gpu_passthrough);
        assert!(vm.network.sriov);
    }

    #[tokio::test]
    async fn test_get_vm_not_found() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        let result = manager.get_vm("nonexistent").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not found")
        );
    }

    #[tokio::test]
    async fn test_create_duplicate_vm() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("dup-vm", 2, 4, false, false)
            .await
            .unwrap();

        let result = manager.create_vm("dup-vm", 4, 8, false, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_vm() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        let result = manager.delete_vm("ghost").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_get_vm_status_no_connection() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("status-vm", 2, 4, false, false)
            .await
            .unwrap();

        let metrics = manager.get_vm_status("status-vm").await.unwrap();
        assert_eq!(metrics.state, T1VmState::Configured);
        assert_eq!(metrics.cpu_cores, 2);
        assert_eq!(metrics.memory_gb, 4);
        assert!(metrics.uptime_seconds.is_none());
    }

    #[tokio::test]
    async fn test_export_import_config() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("export-vm", 4, 8, true, false)
            .await
            .unwrap();

        let json = manager.export_config("export-vm").await.unwrap();

        // Import into a fresh manager
        let dir2 = tempdir().unwrap();
        let manager2 = T1Manager::with_config_dir(dir2.path().to_path_buf()).unwrap();

        let imported = manager2.import_config(&json).await.unwrap();
        assert_eq!(imported.name, "export-vm");
        assert_eq!(imported.cpu_cores, 4);
        assert!(imported.gpu_passthrough);
    }

    #[tokio::test]
    async fn test_import_duplicate_rejected() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("imp-vm", 2, 4, false, false)
            .await
            .unwrap();
        let json = manager.export_config("imp-vm").await.unwrap();

        let result = manager.import_config(&json).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_set_get_connection() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        assert!(manager.get_connection().await.is_none());

        let conn = T1HypervisorConnection {
            endpoint: "192.168.1.100".to_string(),
            port: 9443,
            tls: true,
            auth_token: Some("tok-abc".to_string()),
        };
        manager.set_connection(conn.clone()).await.unwrap();

        let got = manager.get_connection().await.unwrap();
        assert_eq!(got.endpoint, "192.168.1.100");
        assert_eq!(got.port, 9443);
        assert!(got.tls);
        assert_eq!(got.auth_token.as_deref(), Some("tok-abc"));
    }

    #[tokio::test]
    async fn test_ping_hypervisor_no_connection() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        let reachable = manager.ping_hypervisor().await.unwrap();
        assert!(!reachable);
    }

    #[tokio::test]
    async fn test_start_vm_no_connection() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("start-vm", 2, 4, false, false)
            .await
            .unwrap();

        let result = manager.start_vm("start-vm").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No T1 hypervisor connection")
        );
    }

    #[tokio::test]
    async fn test_stop_vm_no_connection() {
        let dir = tempdir().unwrap();
        let manager = T1Manager::with_config_dir(dir.path().to_path_buf()).unwrap();

        manager
            .create_vm("stop-vm", 2, 4, false, false)
            .await
            .unwrap();

        let result = manager.stop_vm("stop-vm").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No T1 hypervisor connection")
        );
    }

    #[tokio::test]
    async fn test_persistence_across_managers() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        {
            let manager = T1Manager::with_config_dir(path.clone()).unwrap();
            manager.create_vm("persist", 2, 4, false, false).await.unwrap();
        }

        let manager2 = T1Manager::with_config_dir(path).unwrap();
        let vm = manager2.get_vm("persist").await.unwrap();
        assert_eq!(vm.name, "persist");
    }

    #[test]
    fn test_t1_vm_state_display() {
        assert_eq!(format!("{}", T1VmState::Configured), "Configured");
        assert_eq!(format!("{}", T1VmState::Running), "Running");
        assert_eq!(format!("{}", T1VmState::Paused), "Paused");
        assert_eq!(format!("{}", T1VmState::Stopped), "Stopped");
        assert_eq!(format!("{}", T1VmState::Error), "Error");
        assert_eq!(format!("{}", T1VmState::Unknown), "Unknown");
    }

    #[test]
    fn test_t1_boot_config_default() {
        let boot = T1BootConfig::default();
        assert!(boot.kernel.is_none());
        assert!(boot.initrd.is_none());
        assert!(boot.cmdline.is_empty());
        assert!(boot.uefi);
    }

    #[test]
    fn test_t1_hypervisor_connection_default() {
        let conn = T1HypervisorConnection::default();
        assert_eq!(conn.endpoint, "127.0.0.1");
        assert_eq!(conn.port, 8443);
        assert!(conn.tls);
        assert!(conn.auth_token.is_none());
    }

    #[test]
    fn test_t1_network_config_default() {
        let net = T1NetworkConfig::default();
        assert!(!net.sriov);
        assert!(net.mac_address.is_none());
        assert!(net.vlan.is_none());
    }

    #[test]
    fn test_t1_vm_config_serde_roundtrip() {
        let config = T1VmConfig {
            name: "serde-vm".to_string(),
            cpu_cores: 4,
            memory_gb: 16,
            gpu_passthrough: true,
            network: T1NetworkConfig {
                sriov: true,
                mac_address: Some("AA:BB:CC:DD:EE:FF".to_string()),
                vlan: Some(100),
            },
            boot: T1BootConfig::default(),
            devices: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: T1VmConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "serde-vm");
        assert_eq!(restored.cpu_cores, 4);
        assert!(restored.gpu_passthrough);
        assert!(restored.network.sriov);
        assert_eq!(restored.network.vlan, Some(100));
    }

    #[test]
    fn test_t1_script_result_serde() {
        let result = T1ScriptResult {
            success: true,
            stdout: "hello\n".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: Some(42),
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: T1ScriptResult = serde_json::from_str(&json).unwrap();
        assert!(restored.success);
        assert_eq!(restored.exit_code, Some(0));
        assert_eq!(restored.duration_ms, Some(42));
    }

    #[test]
    fn test_t1_vm_registry_default() {
        let reg = T1VmRegistry::default();
        assert!(reg.vms.is_empty());
        assert_eq!(reg.version, 0);
    }
}
