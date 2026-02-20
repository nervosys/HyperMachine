//! Configuration management

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level HyperMachine configuration, loaded from TOML.
///
/// Groups all subsystem configurations into a single root structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub vm: VmConfig,
    pub network: NetworkConfig,
    pub gpu: GpuConfig,
    pub agent: AgentConfig,
    pub observability: ObservabilityConfig,
}

/// Virtual machine resource configuration.
///
/// Controls the VM identity, vCPU count, and memory allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// Human-readable VM name.
    pub name: String,
    /// Number of virtual CPUs to allocate.
    pub vcpus: u32,
    /// Memory size in megabytes.
    pub memory_mb: u64,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            vcpus: 2,
            memory_mb: 2048,
        }
    }
}

/// Network subsystem configuration.
///
/// Controls TAP interface setup and optional static IP assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Whether network virtualization is enabled.
    pub enabled: bool,
    /// Host TAP interface name (e.g., `"tap0"`).
    pub interface: String,
    /// Optional static IP address for the TAP interface.
    pub ip_address: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interface: "tap0".to_string(),
            ip_address: None,
        }
    }
}

/// GPU virtualization configuration.
///
/// Controls GPU device selection and passthrough mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    /// Whether GPU virtualization is enabled.
    pub enabled: bool,
    /// GPU device identifier (e.g., `"default"`).
    pub device: String,
    /// Enable direct GPU passthrough to the guest.
    pub passthrough: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device: "default".to_string(),
            passthrough: false,
        }
    }
}

/// AI agent sandbox configuration.
///
/// Controls Wasm-based agent execution limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Whether the agent subsystem is enabled.
    pub enabled: bool,
    /// Maximum script execution time in seconds.
    pub script_timeout_seconds: u64,
    /// Maximum memory available to agent scripts in megabytes.
    pub max_memory_mb: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            script_timeout_seconds: 300,
            max_memory_mb: 512,
        }
    }
}

/// Observability and telemetry configuration.
///
/// Controls tracing, metrics collection, and OTLP export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// Enable distributed tracing.
    pub tracing: bool,
    /// Enable metrics collection.
    pub metrics: bool,
    /// Optional OpenTelemetry Protocol endpoint URL.
    pub otlp_endpoint: Option<String>,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            tracing: true,
            metrics: true,
            otlp_endpoint: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.vm.name, "default");
        assert_eq!(config.vm.vcpus, 2);
        assert_eq!(config.vm.memory_mb, 2048);
        assert!(!config.network.enabled);
        assert!(!config.gpu.enabled);
        assert!(config.agent.enabled);
        assert!(config.observability.tracing);
    }

    #[test]
    fn test_config_roundtrip() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("config.toml");

        let config = Config {
            vm: VmConfig {
                name: "test-vm".to_string(),
                vcpus: 4,
                memory_mb: 4096,
            },
            ..Default::default()
        };

        config.to_file(&path).expect("failed to write config");
        let loaded = Config::from_file(&path).expect("failed to read config");

        assert_eq!(loaded.vm.name, "test-vm");
        assert_eq!(loaded.vm.vcpus, 4);
        assert_eq!(loaded.vm.memory_mb, 4096);
    }

    #[test]
    fn test_config_from_invalid_toml() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("bad.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(b"not valid { toml !!!").expect("write");
        drop(f);

        let err = Config::from_file(&path);
        assert!(err.is_err());
    }

    #[test]
    fn test_config_from_nonexistent_file() {
        let err = Config::from_file("/tmp/__nonexistent_hv2_config.toml");
        assert!(err.is_err());
    }
}
