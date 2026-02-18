//! API client for communicating with HyperMachine backend

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub name: String,
    #[serde(default = "default_cpus")]
    pub cpus: u32,
    #[serde(default = "default_memory", alias = "memory")]
    pub memory_mb: u32,
    pub disk_path: Option<String>,
    #[serde(default = "default_true")]
    pub network_enabled: bool,
    #[serde(default)]
    pub boot_image: Option<String>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

fn default_cpus() -> u32 {
    2
}
fn default_memory() -> u32 {
    2048
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInfo {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub state: VmStateApi,
    #[serde(default = "default_cpus")]
    pub cpus: u32,
    #[serde(default = "default_memory", alias = "memory")]
    pub memory_mb: u32,
    #[serde(default)]
    pub disk_path: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmStateApi {
    Stopped,
    Running,
    Paused,
    Error,
    Creating,
    Starting,
    Stopping,
}

impl std::fmt::Display for VmStateApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stopped => write!(f, "Stopped"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Error => write!(f, "Error"),
            Self::Creating => write!(f, "Creating"),
            Self::Starting => write!(f, "Starting"),
            Self::Stopping => write!(f, "Stopping"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub uptime: u64,
}

#[derive(Debug, Clone)]
pub struct FramebufferData {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub data: Vec<u8>,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    pub async fn health(&self) -> Result<HealthResponse> {
        let url = format!("{}/health", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect")?;
        if response.status().is_success() {
            response.json().await.context("Failed to parse")
        } else {
            anyhow::bail!("Health check failed: {}", response.status())
        }
    }

    pub async fn list_vms(&self) -> Result<Vec<VmInfo>> {
        let url = format!("{}/vms", self.base_url);
        let response = self.client.get(&url).send().await?;
        if response.status().is_success() {
            // Backend returns array directly or wrapped - handle both
            let text = response.text().await?;
            if let Ok(vms) = serde_json::from_str::<Vec<VmInfo>>(&text) {
                Ok(vms)
            } else if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(vms) = wrapper.get("vms") {
                    Ok(serde_json::from_value(vms.clone())?)
                } else {
                    Ok(vec![])
                }
            } else {
                Ok(vec![])
            }
        } else {
            anyhow::bail!("Failed to list VMs: {}", response.status())
        }
    }

    pub async fn create_vm(&self, config: VmConfig) -> Result<VmInfo> {
        let url = format!("{}/vms", self.base_url);
        let response = self.client.post(&url).json(&config).send().await?;
        if response.status().is_success() {
            response.json().await.context("Failed to parse")
        } else {
            anyhow::bail!("Failed to create VM: {}", response.status())
        }
    }

    pub async fn start_vm(&self, vm_name: &str) -> Result<VmInfo> {
        let url = format!("{}/vms/{}/start", self.base_url, vm_name);
        let response = self.client.post(&url).send().await?;
        if response.status().is_success() {
            response.json().await.context("Failed to parse")
        } else {
            anyhow::bail!("Failed to start VM: {}", response.status())
        }
    }

    pub async fn stop_vm(&self, vm_name: &str) -> Result<VmInfo> {
        let url = format!("{}/vms/{}/stop", self.base_url, vm_name);
        let response = self.client.post(&url).send().await?;
        if response.status().is_success() {
            response.json().await.context("Failed to parse")
        } else {
            anyhow::bail!("Failed to stop VM: {}", response.status())
        }
    }

    pub async fn pause_vm(&self, vm_name: &str) -> Result<VmInfo> {
        let url = format!("{}/vms/{}/pause", self.base_url, vm_name);
        let response = self.client.post(&url).send().await?;
        if response.status().is_success() {
            response.json().await.context("Failed to parse")
        } else {
            anyhow::bail!("Failed to pause VM: {}", response.status())
        }
    }

    pub async fn delete_vm(&self, vm_name: &str) -> Result<()> {
        let url = format!("{}/vms/{}", self.base_url, vm_name);
        let response = self.client.delete(&url).send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Failed to delete VM: {}", response.status())
        }
    }

    pub async fn get_framebuffer(&self, vm_name: &str) -> Result<FramebufferData> {
        let url = format!("{}/vms/{}/display", self.base_url, vm_name);
        let response = self.client.get(&url).send().await?;
        if response.status().is_success() {
            #[derive(Deserialize)]
            struct FbResponse {
                width: u32,
                height: u32,
                format: String,
                data: String,
            }
            let fb: FbResponse = response.json().await?;
            use base64::Engine;
            let data = base64::engine::general_purpose::STANDARD
                .decode(&fb.data)
                .unwrap_or_default();
            Ok(FramebufferData {
                width: fb.width,
                height: fb.height,
                format: fb.format,
                data,
            })
        } else {
            anyhow::bail!("Failed to get framebuffer: {}", response.status())
        }
    }

    pub async fn send_key(&self, vm_name: &str, keycode: u32, pressed: bool) -> Result<()> {
        let url = format!("{}/vms/{}/input/keyboard", self.base_url, vm_name);
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({"keycode": keycode, "pressed": pressed}))
            .send()
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Failed to send key: {}", response.status())
        }
    }

    pub async fn send_mouse(
        &self,
        vm_name: &str,
        x: i32,
        y: i32,
        buttons: u8,
        scroll: i32,
    ) -> Result<()> {
        let url = format!("{}/vms/{}/input/mouse", self.base_url, vm_name);
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({"x": x, "y": y, "buttons": buttons, "scroll": scroll}))
            .send()
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Failed to send mouse: {}", response.status())
        }
    }
}

#[allow(dead_code)]
pub type SharedApiClient = Arc<RwLock<ApiClient>>;
