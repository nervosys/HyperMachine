//! Tool Registry and Executor
//!
//! Provides the runtime for executing tool calls from AI agents.

use super::adapters::{ToolCallRequest, ToolCallResponse};
use crate::vm_manager::VmManager;
use serde_json::{json, Value};
use std::sync::Arc;

/// Tool executor that handles tool calls from AI agents
pub struct ToolExecutor {
    vm_manager: Arc<VmManager>,
}

impl ToolExecutor {
    /// Create a new tool executor
    pub fn new(vm_manager: Arc<VmManager>) -> Self {
        Self { vm_manager }
    }

    /// Execute a tool call
    pub async fn execute(&self, request: ToolCallRequest) -> ToolCallResponse {
        let result = match request.tool.as_str() {
            "vm.create" => self.create_vm(&request.arguments).await,
            "vm.start" => self.start_vm(&request.arguments).await,
            "vm.stop" => self.stop_vm(&request.arguments).await,
            "vm.delete" => self.delete_vm(&request.arguments).await,
            "vm.list" => self.list_vms().await,
            "vm.get" => self.get_vm(&request.arguments).await,
            "vm.metrics" => self.get_metrics(&request.arguments).await,
            "vm.execute_script" => self.execute_script(&request.arguments).await,
            _ => Err(format!("Unknown tool: {}", request.tool)),
        };

        match result {
            Ok(value) => ToolCallResponse {
                success: true,
                result: Some(value),
                error: None,
                call_id: request.call_id,
            },
            Err(error) => ToolCallResponse {
                success: false,
                result: None,
                error: Some(error),
                call_id: request.call_id,
            },
        }
    }

    /// Execute multiple tool calls (for parallel execution)
    pub async fn execute_batch(&self, requests: Vec<ToolCallRequest>) -> Vec<ToolCallResponse> {
        let mut responses = Vec::with_capacity(requests.len());

        // Execute sequentially for now (could be parallelized for safe operations)
        for request in requests {
            responses.push(self.execute(request).await);
        }

        responses
    }

    async fn create_vm(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        let cpu_cores = args.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(2) as u32;

        let memory_gb = args.get("memory_gb").and_then(|v| v.as_u64()).unwrap_or(4);

        let gpu_enabled = args
            .get("gpu_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let network_enabled = args
            .get("network_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Validate name
        if !Self::is_valid_vm_name(name) {
            return Err("Invalid VM name: must start with a letter and contain only alphanumeric characters and hyphens".to_string());
        }

        self.vm_manager
            .create_vm(name, cpu_cores, memory_gb, gpu_enabled, network_enabled)
            .await
            .map(|vm| {
                json!({
                    "name": vm.name,
                    "cpu_cores": vm.cpu_cores,
                    "memory_gb": vm.memory_gb,
                    "gpu_enabled": vm.gpu_enabled,
                    "network_enabled": vm.network_enabled,
                    "state": format!("{}", vm.state),
                    "created_at": vm.created_at.to_rfc3339()
                })
            })
            .map_err(|e| e.to_string())
    }

    async fn start_vm(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        self.vm_manager
            .start_vm(name)
            .await
            .map(|_| json!({"status": "started", "name": name}))
            .map_err(|e| e.to_string())
    }

    async fn stop_vm(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        self.vm_manager
            .stop_vm(name)
            .await
            .map(|_| json!({"status": "stopped", "name": name}))
            .map_err(|e| e.to_string())
    }

    async fn delete_vm(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        self.vm_manager
            .delete_vm(name)
            .await
            .map(|_| json!({"status": "deleted", "name": name}))
            .map_err(|e| e.to_string())
    }

    async fn list_vms(&self) -> Result<Value, String> {
        let vms = self.vm_manager.list_vms().await;
        Ok(json!(vms
            .into_iter()
            .map(|vm| json!({
                "name": vm.name,
                "cpu_cores": vm.cpu_cores,
                "memory_gb": vm.memory_gb,
                "gpu_enabled": vm.gpu_enabled,
                "network_enabled": vm.network_enabled,
                "state": format!("{}", vm.state),
                "created_at": vm.created_at.to_rfc3339()
            }))
            .collect::<Vec<_>>()))
    }

    async fn get_vm(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        self.vm_manager
            .get_vm(name)
            .await
            .map(|vm| {
                json!({
                    "name": vm.name,
                    "cpu_cores": vm.cpu_cores,
                    "memory_gb": vm.memory_gb,
                    "gpu_enabled": vm.gpu_enabled,
                    "network_enabled": vm.network_enabled,
                    "state": format!("{}", vm.state),
                    "created_at": vm.created_at.to_rfc3339(),
                    "updated_at": vm.updated_at.to_rfc3339()
                })
            })
            .map_err(|e| e.to_string())
    }

    async fn get_metrics(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        self.vm_manager
            .get_metrics(name)
            .await
            .map(|m| {
                json!({
                    "name": m.name,
                    "state": format!("{}", m.state),
                    "cpu_cores": m.cpu_cores,
                    "memory_gb": m.memory_gb,
                    "cpu_usage_percent": m.cpu_usage_percent,
                    "memory_used_gb": m.memory_used_gb,
                    "uptime_seconds": m.uptime_seconds
                })
            })
            .map_err(|e| e.to_string())
    }

    async fn execute_script(&self, args: &Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;

        let script = args
            .get("script")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: script")?;

        self.vm_manager
            .execute_script(name, script)
            .await
            .map_err(|e| e.to_string())
    }

    fn is_valid_vm_name(name: &str) -> bool {
        if name.is_empty() || name.len() > 63 {
            return false;
        }

        let mut chars = name.chars();

        // Must start with a letter
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() => {}
            _ => return false,
        }

        // Rest must be alphanumeric or hyphen
        chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
    }
}

/// Introspection utilities for AI agents to discover capabilities
pub struct Introspection;

impl Introspection {
    /// Get a concise capability summary
    pub fn capabilities_summary() -> Value {
        json!({
            "api_version": "1.0.0",
            "service": "HyperMachine",
            "capabilities": {
                "vm_management": {
                    "create": true,
                    "start": true,
                    "stop": true,
                    "delete": true,
                    "list": true,
                    "get": true
                },
                "monitoring": {
                    "metrics": true,
                    "state": true
                },
                "execution": {
                    "scripts": true
                }
            },
            "operations": [
                "vm.create",
                "vm.start",
                "vm.stop",
                "vm.delete",
                "vm.list",
                "vm.get",
                "vm.metrics",
                "vm.execute_script"
            ],
            "constraints": {
                "vm_name": {
                    "pattern": "^[a-zA-Z][a-zA-Z0-9-]{0,62}$",
                    "max_length": 63
                },
                "cpu_cores": { "min": 1, "max": 128 },
                "memory_gb": { "min": 1, "max": 1024 }
            }
        })
    }

    /// Get quick reference for a specific operation
    pub fn operation_reference(op_name: &str) -> Option<Value> {
        match op_name {
            "vm.create" => Some(json!({
                "name": "vm.create",
                "description": "Create a new virtual machine",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "Unique VM name" },
                    "cpu_cores": { "type": "integer", "required": false, "default": 2, "range": [1, 128] },
                    "memory_gb": { "type": "integer", "required": false, "default": 4, "range": [1, 1024] },
                    "gpu_enabled": { "type": "boolean", "required": false, "default": false },
                    "network_enabled": { "type": "boolean", "required": false, "default": false }
                },
                "example": { "name": "my-vm", "cpu_cores": 4, "memory_gb": 8 }
            })),
            "vm.start" => Some(json!({
                "name": "vm.start",
                "description": "Start a stopped VM",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "VM name" }
                },
                "example": { "name": "my-vm" }
            })),
            "vm.stop" => Some(json!({
                "name": "vm.stop",
                "description": "Stop a running VM",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "VM name" }
                },
                "example": { "name": "my-vm" }
            })),
            "vm.delete" => Some(json!({
                "name": "vm.delete",
                "description": "Delete a VM (stops it first if running)",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "VM name" }
                },
                "example": { "name": "my-vm" }
            })),
            "vm.list" => Some(json!({
                "name": "vm.list",
                "description": "List all VMs",
                "parameters": {},
                "example": {}
            })),
            "vm.get" => Some(json!({
                "name": "vm.get",
                "description": "Get details of a specific VM",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "VM name" }
                },
                "example": { "name": "my-vm" }
            })),
            "vm.metrics" => Some(json!({
                "name": "vm.metrics",
                "description": "Get VM performance metrics",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "VM name" }
                },
                "example": { "name": "my-vm" }
            })),
            "vm.execute_script" => Some(json!({
                "name": "vm.execute_script",
                "description": "Execute a script in a running VM",
                "parameters": {
                    "name": { "type": "string", "required": true, "description": "VM name" },
                    "script": { "type": "string", "required": true, "description": "Script content" },
                    "timeout_seconds": { "type": "integer", "required": false, "default": 300, "range": [1, 3600] }
                },
                "example": { "name": "my-vm", "script": "echo 'Hello'" }
            })),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_vm_names() {
        assert!(ToolExecutor::is_valid_vm_name("my-vm"));
        assert!(ToolExecutor::is_valid_vm_name("vm1"));
        assert!(ToolExecutor::is_valid_vm_name("a"));
        assert!(ToolExecutor::is_valid_vm_name("production-server-01"));
    }

    #[test]
    fn test_invalid_vm_names() {
        assert!(!ToolExecutor::is_valid_vm_name(""));
        assert!(!ToolExecutor::is_valid_vm_name("1vm")); // starts with number
        assert!(!ToolExecutor::is_valid_vm_name("-vm")); // starts with hyphen
        assert!(!ToolExecutor::is_valid_vm_name("my_vm")); // underscore
        assert!(!ToolExecutor::is_valid_vm_name("my vm")); // space
    }

    #[test]
    fn test_capabilities_summary() {
        let summary = Introspection::capabilities_summary();
        assert_eq!(summary["api_version"], "1.0.0");
        assert!(summary["operations"].as_array().unwrap().len() == 8);
    }

    #[test]
    fn test_operation_reference() {
        let create = Introspection::operation_reference("vm.create").unwrap();
        assert_eq!(create["name"], "vm.create");
        assert!(create["parameters"]["name"]["required"].as_bool().unwrap());

        assert!(Introspection::operation_reference("unknown").is_none());
    }

    #[test]
    fn test_vm_name_max_length() {
        let long_name = format!("a{}", "b".repeat(62)); // 63 chars exactly
        assert!(ToolExecutor::is_valid_vm_name(&long_name));

        let too_long = format!("a{}", "b".repeat(63)); // 64 chars
        assert!(!ToolExecutor::is_valid_vm_name(&too_long));
    }

    #[test]
    fn test_vm_name_unicode_rejected() {
        assert!(!ToolExecutor::is_valid_vm_name("vm-\u{00e9}")); // accented e
        assert!(!ToolExecutor::is_valid_vm_name("\u{4e16}")); // CJK char
    }

    #[test]
    fn test_operation_reference_all_ops() {
        let ops = [
            "vm.create",
            "vm.start",
            "vm.stop",
            "vm.delete",
            "vm.list",
            "vm.get",
            "vm.metrics",
            "vm.execute_script",
        ];
        for op in &ops {
            let r = Introspection::operation_reference(op);
            assert!(r.is_some(), "Missing reference for {}", op);
            assert_eq!(r.as_ref().unwrap()["name"], *op);
        }
    }

    #[test]
    fn test_capabilities_constraints() {
        let summary = Introspection::capabilities_summary();
        let constraints = &summary["constraints"];
        assert_eq!(constraints["vm_name"]["max_length"], 63);
        assert_eq!(constraints["cpu_cores"]["min"], 1);
        assert_eq!(constraints["cpu_cores"]["max"], 128);
        assert_eq!(constraints["memory_gb"]["min"], 1);
        assert_eq!(constraints["memory_gb"]["max"], 1024);
    }
}
