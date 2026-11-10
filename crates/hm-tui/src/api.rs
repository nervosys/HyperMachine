//! Clients for the two backends this dashboard shows: `hv2-api`'s `/vms`
//! (Type 2 VMs, the same endpoint `hm-gui` and `hm serve` use) and a
//! sandboxd node's `/sandboxes` (sandbox VMs, the same endpoint `hm sandbox
//! vm list` hits). They are different services with different lifecycles,
//! so this file keeps two small clients rather than one that pretends they
//! are the same thing.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A Type 2 VM, as `GET /vms` on `hv2-api` returns it. Mirrors
/// `hm-gui`'s `VmInfo` -- same backend, same shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInfo {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub state: VmState,
    #[serde(default = "default_cpus")]
    pub cpus: u32,
    #[serde(default = "default_memory", alias = "memory")]
    pub memory_mb: u32,
    #[serde(default)]
    pub ip_address: Option<String>,
}

fn default_cpus() -> u32 {
    2
}
fn default_memory() -> u32 {
    2048
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    Stopped,
    Running,
    Paused,
    Error,
    Creating,
    Starting,
    Stopping,
}

impl std::fmt::Display for VmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Stopped => "stopped",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Error => "error",
            Self::Creating => "creating",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
        };
        f.write_str(s)
    }
}

/// A sandbox VM, as `GET /sandboxes` on a sandboxd node returns it --
/// `SandboxRecord::listed` in `hv2-cluster`. The field names are E2B's, so
/// they are `camelCase` on the wire; everything here is read-only, nothing
/// this dashboard cannot already get from `hm sandbox vm list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    #[serde(rename = "sandboxID")]
    pub sandbox_id: String,
    #[serde(rename = "templateID")]
    pub template_id: String,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "cpuCount")]
    pub cpu_count: u32,
    #[serde(rename = "memoryMB")]
    pub memory_mb: u64,
    pub state: String,
    #[serde(rename = "nodeID", default)]
    pub node_id: String,
}

/// `hv2-api`'s `/vms`: create, start, stop, pause, delete a Type 2 VM.
#[derive(Clone)]
pub struct VmApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl VmApiClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub async fn list(&self) -> Result<Vec<VmInfo>> {
        let url = format!("{}/vms", self.base_url);
        let text = self.client.get(&url).send().await?.text().await?;
        // The server returns either a bare array or `{"vms": [...]}`.
        if let Ok(vms) = serde_json::from_str::<Vec<VmInfo>>(&text) {
            Ok(vms)
        } else {
            let wrapper: serde_json::Value = serde_json::from_str(&text)?;
            Ok(wrapper
                .get("vms")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or_default())
        }
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        self.post(&format!("/vms/{name}/start")).await
    }

    pub async fn stop(&self, name: &str) -> Result<()> {
        self.post(&format!("/vms/{name}/stop")).await
    }

    pub async fn delete(&self, name: &str) -> Result<()> {
        let url = format!("{}/vms/{}", self.base_url, name);
        let response = self.client.delete(&url).send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("delete failed: {}", response.status())
        }
    }

    async fn post(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let response = self.client.post(&url).send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("{path} failed: {}", response.status())
        }
    }
}

/// A sandboxd node's `/sandboxes`: list and delete.
#[derive(Clone)]
pub struct SandboxApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl SandboxApiClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub async fn list(&self) -> Result<Vec<SandboxInfo>> {
        let url = format!("{}/sandboxes", self.base_url);
        let response = self.client.get(&url).send().await?;
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            anyhow::bail!("list sandboxes failed: {}", response.status())
        }
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let url = format!("{}/sandboxes/{}", self.base_url, id);
        let response = self.client.delete(&url).send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("delete sandbox failed: {}", response.status())
        }
    }
}
