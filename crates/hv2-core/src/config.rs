//! Configuration management

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub vm: VmConfig,
    pub network: NetworkConfig,
    pub gpu: GpuConfig,
    pub agent: AgentConfig,
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub name: String,
    pub vcpus: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub enabled: bool,
    pub interface: String,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    pub enabled: bool,
    pub device: String,
    pub passthrough: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub enabled: bool,
    pub script_timeout_seconds: u64,
    pub max_memory_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub tracing: bool,
    pub metrics: bool,
    pub otlp_endpoint: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vm: VmConfig {
                name: "default".to_string(),
                vcpus: 2,
                memory_mb: 2048,
            },
            network: NetworkConfig {
                enabled: false,
                interface: "tap0".to_string(),
                ip_address: None,
            },
            gpu: GpuConfig {
                enabled: false,
                device: "default".to_string(),
                passthrough: false,
            },
            agent: AgentConfig {
                enabled: true,
                script_timeout_seconds: 300,
                max_memory_mb: 512,
            },
            observability: ObservabilityConfig {
                tracing: true,
                metrics: true,
                otlp_endpoint: None,
            },
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| crate::Error::Config(format!("Failed to parse config: {}", e)))?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| crate::Error::Config(format!("Failed to serialize config: {}", e)))?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
