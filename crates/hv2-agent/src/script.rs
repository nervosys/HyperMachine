//! Script execution engine for AI agents

use crate::{AgentError, CapabilitySet, Result};
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
    /// Create a new script engine
    pub fn new(capabilities: CapabilitySet) -> Self {
        let mut engine = Engine::new();

        // Configure engine for safety
        engine.set_max_expr_depths(50, 25);
        engine.set_max_operations(100_000);
        engine.set_max_string_size(1024 * 1024); // 1MB

        Self {
            engine: Arc::new(RwLock::new(engine)),
            capabilities,
        }
    }

    /// Execute a script with VM context
    pub async fn execute(&self, script: &str, vm: Arc<VM>) -> Result<serde_json::Value> {
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

    #[tokio::test]
    async fn test_script_execution() {
        let vm = Arc::new(VM::new(VMConfig::default()).unwrap());
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
        let vm = Arc::new(VM::new(VMConfig::default()).unwrap());
        let engine = ScriptEngine::new(CapabilitySet::default());

        let script = r#"
            vcpu_count
        "#;

        let result = engine.execute(script, vm).await.unwrap();
        assert_eq!(result, serde_json::json!(1));
    }
}
