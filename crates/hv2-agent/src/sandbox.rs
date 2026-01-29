//! Sandbox for safe script execution

use crate::Result;
use serde::{Deserialize, Serialize};

/// Sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Maximum memory usage in bytes
    pub max_memory: u64,

    /// Maximum CPU time in seconds
    pub max_cpu_time: u64,

    /// Enable network access
    pub allow_network: bool,

    /// Enable filesystem access
    pub allow_filesystem: bool,

    /// Allowed syscalls
    pub allowed_syscalls: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory: 512 * 1024 * 1024, // 512MB
            max_cpu_time: 300,             // 5 minutes
            allow_network: false,
            allow_filesystem: false,
            allowed_syscalls: vec!["read".to_string(), "write".to_string(), "exit".to_string()],
        }
    }
}

/// Sandbox for executing untrusted code
pub struct Sandbox {
    config: SandboxConfig,
}

impl Sandbox {
    /// Create a new sandbox with the given configuration
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    /// Get sandbox configuration
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Check if an operation is allowed
    pub fn check_permission(&self, operation: &str) -> bool {
        match operation {
            "network" => self.config.allow_network,
            "filesystem" => self.config.allow_filesystem,
            _ => false,
        }
    }

    /// Validate resource usage
    pub fn validate_resources(&self, memory: u64, cpu_time: u64) -> Result<()> {
        if memory > self.config.max_memory {
            return Err(crate::AgentError::ResourceLimit(format!(
                "Memory usage {} exceeds limit {}",
                memory, self.config.max_memory
            )));
        }

        if cpu_time > self.config.max_cpu_time {
            return Err(crate::AgentError::ResourceLimit(format!(
                "CPU time {} exceeds limit {}",
                cpu_time, self.config.max_cpu_time
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_permissions() {
        let config = SandboxConfig {
            allow_network: true,
            allow_filesystem: false,
            ..Default::default()
        };

        let sandbox = Sandbox::new(config);
        assert!(sandbox.check_permission("network"));
        assert!(!sandbox.check_permission("filesystem"));
    }

    #[test]
    fn test_resource_validation() {
        let sandbox = Sandbox::new(SandboxConfig::default());

        // Within limits
        assert!(sandbox.validate_resources(100_000_000, 100).is_ok());

        // Exceeds memory limit
        assert!(sandbox.validate_resources(1_000_000_000, 100).is_err());

        // Exceeds CPU time limit
        assert!(sandbox.validate_resources(100_000_000, 400).is_err());
    }
}
