//! Resource and permission policy for script execution.
//!
//! # This is a policy object, not OS-level containment
//!
//! [`Sandbox`] holds limits and answers questions about them. It does not
//! install seccomp filters, spawn an isolated process, or account for memory —
//! [`Sandbox::check_permission`] and [`Sandbox::validate_resources`] only take
//! effect where a caller consults them, and a caller that never asks is never
//! constrained.
//!
//! What actually keeps an agent script off the network and filesystem is that
//! [`ScriptEngine`](crate::ScriptEngine) builds a Rhai engine that registers no
//! I/O functions at all, so there is nothing for a script to call. `max_cpu_time`
//! is enforced, as a wall-clock bound, by
//! [`AgentVM::effective_script_timeout`](crate::AgentVM::effective_script_timeout);
//! `max_memory` and `allowed_syscalls` describe intent for a future
//! process-isolated backend and constrain nothing today.

use crate::Result;
use serde::{Deserialize, Serialize};

/// Sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Maximum memory usage in bytes.
    ///
    /// Declarative. Scripts run in-process, so there is no allocator
    /// boundary to enforce this against; use [`Self::max_operations`] and
    /// [`Self::max_string_size`], which are enforced, or run the whole
    /// process under the container module for a real memory limit.
    pub max_memory: u64,

    /// Maximum Rhai operations a single script may execute.
    ///
    /// Enforced. This is the practical bound on a runaway script: an
    /// infinite loop terminates here rather than running until the
    /// wall-clock timeout.
    pub max_operations: u64,

    /// Maximum size of any single string a script may build, in bytes.
    ///
    /// Enforced. The closest thing to a memory bound available in-process,
    /// since string building is how a script would most easily allocate
    /// without limit.
    pub max_string_size: usize,

    /// Maximum CPU time in seconds. Enforced as a wall-clock bound on script
    /// execution, alongside `AgentVM`'s `script_timeout`; the stricter wins.
    pub max_cpu_time: u64,

    /// Enable network access. Moot for the Rhai engine, which registers no
    /// networking for a script to reach.
    pub allow_network: bool,

    /// Enable filesystem access. Moot for the Rhai engine, which registers no
    /// file I/O for a script to reach.
    pub allow_filesystem: bool,

    /// Allowed syscalls. Declarative only — there is no process boundary to
    /// filter, so nothing consults this.
    pub allowed_syscalls: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory: 512 * 1024 * 1024, // 512MB
            max_operations: 100_000,
            max_string_size: 1024 * 1024, // 1 MiB
            max_cpu_time: 300,            // 5 minutes
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
