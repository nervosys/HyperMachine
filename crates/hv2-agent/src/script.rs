//! Script execution engine for AI agents.
//!
//! # What this does and does not do
//!
//! Scripts run **on the host**, in an embedded Rhai interpreter, with a
//! read-only view of one VM pushed into scope. Nothing here executes inside the
//! guest: there is no in-guest agent and no serial-console protocol, so a script
//! can observe a VM's configuration and state but cannot run a command in the
//! operating system the VM is booting. An agent choosing this operation to
//! "run something in the VM" would be choosing the wrong tool.
//!
//! Execution requires [`Capability::VmRead`], matching the read-only VM data the
//! scope exposes.

use crate::{AgentError, Capability, CapabilitySet, Result};
use hv2_core::VM;
use parking_lot::RwLock;
use rhai::{Engine, Scope};
use std::sync::Arc;

/// Script execution result
#[derive(Debug, Clone)]
pub struct ScriptResult {
    pub output: serde_json::Value,
    pub logs: Vec<String>,
}

/// Script engine for AI agents
pub struct ScriptEngine {
    engine: Arc<RwLock<Engine>>,
    capabilities: CapabilitySet,
}

impl ScriptEngine {
    /// Create a script engine with the default limits.
    pub fn new(capabilities: CapabilitySet) -> Self {
        Self::with_limits(capabilities, &crate::SandboxConfig::default())
    }

    /// Create a script engine bounded by `limits`.
    ///
    /// `max_operations` and `max_string_size` are applied to the Rhai engine
    /// and are the two limits that actually bind in-process: an infinite loop
    /// terminates on the operation count rather than running until the
    /// wall-clock timeout, and unbounded string building fails on the size
    /// cap. `max_memory` and `allowed_syscalls` have no in-process equivalent
    /// and are documented as declarative on [`crate::SandboxConfig`].
    pub fn with_limits(capabilities: CapabilitySet, limits: &crate::SandboxConfig) -> Self {
        let mut engine = Engine::new();

        engine.set_max_expr_depths(50, 25);
        engine.set_max_operations(limits.max_operations);
        engine.set_max_string_size(limits.max_string_size);

        Self {
            engine: Arc::new(RwLock::new(engine)),
            capabilities,
        }
    }

    /// Execute a script against a read-only view of `vm`, on the host.
    ///
    /// Requires [`Capability::VmRead`]. See the module docs for why this cannot
    /// run anything inside the guest.
    pub async fn execute(&self, script: &str, vm: Arc<VM>) -> Result<serde_json::Value> {
        // The engine was constructed with a capability set; until now nothing
        // consulted it, so a set deliberately stripped of `VmRead` still got a
        // full view of the VM.
        if !self.capabilities.has(Capability::VmRead) {
            return Err(AgentError::Script(
                "script execution requires the VmRead capability".to_string(),
            ));
        }

        let engine = self.engine.read();
        let mut scope = Scope::new();

        // Add VM API to scope
        self.register_vm_api(&mut scope, &vm)?;

        // Execute script with scope
        let result = engine
            .eval_with_scope::<rhai::Dynamic>(&mut scope, script)
            .map_err(|e| AgentError::Script(format!("Execution error: {}", e)))?;

        // Convert result to JSON
        let json_result = self.rhai_to_json(result)?;

        Ok(json_result)
    }

    /// Register VM API functions in the script scope
    fn register_vm_api(&self, scope: &mut Scope, vm: &Arc<VM>) -> Result<()> {
        // Add VM state accessor
        let state = vm.state();
        scope.push("vm_state", format!("{:?}", state));

        // Add VM info
        scope.push("vm_name", vm.config().name.clone());
        scope.push("vcpu_count", vm.vcpus().len() as i64);
        scope.push("memory_size", vm.memory().total_size() as i64);

        Ok(())
    }

    /// Convert Rhai dynamic value to JSON
    fn rhai_to_json(&self, value: rhai::Dynamic) -> Result<serde_json::Value> {
        if value.is::<i64>() {
            Ok(serde_json::json!(value.cast::<i64>()))
        } else if value.is::<f64>() {
            Ok(serde_json::json!(value.cast::<f64>()))
        } else if value.is::<bool>() {
            Ok(serde_json::json!(value.cast::<bool>()))
        } else if value.is::<String>() {
            Ok(serde_json::json!(value.cast::<String>()))
        } else {
            Ok(serde_json::json!(value.to_string()))
        }
    }

    /// Validate script before execution
    pub fn validate(&self, script: &str) -> Result<()> {
        let engine = self.engine.read();

        engine
            .compile(script)
            .map_err(|e| AgentError::Script(format!("Validation error: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_core::VMConfig;

    /// Build a VM for tests, returning `None` (and the test returns early) when
    /// no hypervisor backend is available — e.g. CI or WSL2 without `/dev/kvm`
    /// access. Where a backend exists the tests run in full.
    fn vm_or_skip() -> Option<Arc<VM>> {
        match VM::new(VMConfig::default()) {
            Ok(vm) => Some(Arc::new(vm)),
            Err(e) => {
                eprintln!("skipping: no hypervisor backend available ({e})");
                None
            }
        }
    }

    #[tokio::test]
    async fn execution_requires_the_vm_read_capability() {
        let Some(vm) = vm_or_skip() else {
            return;
        };
        // An empty set is what a caller uses to say "this script gets nothing".
        // Before the gate existed it got the same VM view as a full set.
        let engine = ScriptEngine::new(CapabilitySet::new());

        let err = engine
            .execute("vcpu_count", vm)
            .await
            .expect_err("an uncapable engine must refuse");

        assert!(err.to_string().contains("VmRead"), "got: {err}");
    }

    #[tokio::test]
    async fn the_operation_limit_stops_a_runaway_script() {
        let Some(vm) = vm_or_skip() else {
            return;
        };
        // A tight loop is the case the wall-clock timeout handles badly: it
        // would burn the full script_timeout before failing. The operation
        // count catches it immediately.
        let limits = crate::SandboxConfig {
            max_operations: 1_000,
            ..Default::default()
        };
        let engine = ScriptEngine::with_limits(CapabilitySet::default(), &limits);

        let err = engine
            .execute("let i = 0; loop { i += 1; }", vm)
            .await
            .expect_err("a runaway script must be stopped");
        assert!(
            err.to_string().to_lowercase().contains("operation"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn the_string_limit_bounds_allocation() {
        let Some(vm) = vm_or_skip() else {
            return;
        };
        let limits = crate::SandboxConfig {
            max_string_size: 64,
            ..Default::default()
        };
        let engine = ScriptEngine::with_limits(CapabilitySet::default(), &limits);

        let err = engine
            .execute(r#"let s = "x"; loop { s += s; }"#, vm)
            .await
            .expect_err("unbounded string building must be stopped");
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn a_readonly_capability_set_can_execute() {
        let Some(vm) = vm_or_skip() else {
            return;
        };
        let engine = ScriptEngine::new(CapabilitySet::readonly());

        let result = engine.execute("1 + 1", vm).await.unwrap();
        assert_eq!(result, serde_json::json!(2));
    }

    #[tokio::test]
    async fn test_script_execution() {
        let Some(vm) = vm_or_skip() else {
            return;
        };
        let engine = ScriptEngine::new(CapabilitySet::default());

        let script = r#"
            let x = 10;
            let y = 20;
            x + y
        "#;

        let result = engine.execute(script, vm).await.unwrap();
        assert_eq!(result, serde_json::json!(30));
    }

    #[tokio::test]
    async fn test_vm_api_access() {
        let Some(vm) = vm_or_skip() else {
            return;
        };
        let engine = ScriptEngine::new(CapabilitySet::default());

        let script = r#"
            vcpu_count
        "#;

        let result = engine.execute(script, vm).await.unwrap();
        assert_eq!(result, serde_json::json!(1));
    }
}
