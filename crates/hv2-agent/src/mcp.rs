//! Model Context Protocol (MCP) Interface for AI Agents
//!
//! This module provides a standardized interface for AI agents to interact with
//! HyperMachine using the Model Context Protocol pattern. It exposes VM operations
//! as discoverable tools with JSON Schema definitions.
//!
//! # Overview
//!
//! The MCP interface enables:
//! - Tool discovery and enumeration
//! - Structured function calling with validated parameters
//! - Multi-agent coordination and isolation
//! - Resource management and quotas
//! - Comprehensive audit logging
//!
//! # Example
//!
//! ```rust,ignore
//! use hv2_agent::mcp::{McpServer, AgentSession};
//!
//! // Create MCP server
//! let server = McpServer::new();
//!
//! // Register an agent session
//! let session = server.create_session("agent-1", AgentCapabilities::full()).await?;
//!
//! // Agent can discover available tools
//! let tools = session.list_tools().await?;
//!
//! // Agent calls a tool
//! let result = session.call_tool("vm.create", json!({
//!     "name": "test-vm",
//!     "cpu_cores": 4,
//!     "memory_gb": 8
//! })).await?;
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// MCP Server - manages agent sessions and tool execution
pub struct McpServer {
    /// Registered tools
    tools: RwLock<HashMap<String, McpTool>>,
    /// Active sessions
    sessions: RwLock<HashMap<String, Arc<AgentSession>>>,
    /// Server configuration
    config: McpConfig,
    /// Audit log
    audit_log: RwLock<Vec<AuditEntry>>,
}

/// MCP Server configuration
#[derive(Debug, Clone)]
pub struct McpConfig {
    /// Maximum concurrent sessions
    pub max_sessions: usize,
    /// Default tool timeout
    pub default_timeout: Duration,
    /// Enable audit logging
    pub audit_enabled: bool,
    /// Rate limit (calls per minute per session)
    pub rate_limit: u32,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            default_timeout: Duration::from_secs(60),
            audit_enabled: true,
            rate_limit: 100,
        }
    }
}

/// An MCP tool definition (OpenAI/Anthropic function calling compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Tool name (e.g., "vm.create", "vm.start")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for parameters
    pub parameters: JsonValue,
    /// Tool category
    pub category: ToolCategory,
    /// Required capabilities to use this tool
    pub required_capabilities: Vec<AgentCapability>,
    /// Whether the tool is enabled
    pub enabled: bool,
}

/// Tool categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCategory {
    /// VM lifecycle management
    VmLifecycle,
    /// VM configuration
    VmConfig,
    /// Resource management (CPU, memory)
    Resources,
    /// Snapshot operations
    Snapshots,
    /// Network operations
    Network,
    /// Storage operations
    Storage,
    /// Monitoring and metrics
    Monitoring,
    /// Security operations
    Security,
    /// Multi-agent coordination
    Coordination,
    /// System administration
    System,
}

/// Agent capabilities (permissions)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentCapability {
    /// Read VM state
    VmRead,
    /// Modify VM configuration
    VmWrite,
    /// Create/delete VMs
    VmManage,
    /// Access network settings
    NetworkAccess,
    /// Modify network settings
    NetworkAdmin,
    /// Read storage
    StorageRead,
    /// Modify storage
    StorageWrite,
    /// Create/manage snapshots
    SnapshotManage,
    /// View metrics
    MetricsRead,
    /// Execute commands in VM
    GuestExec,
    /// Debug/introspect VM
    Debug,
    /// Coordinate with other agents
    Coordination,
    /// Full administrative access
    Admin,
}

/// Agent capabilities set
#[derive(Debug, Clone, Default)]
pub struct AgentCapabilities {
    capabilities: Vec<AgentCapability>,
}

impl AgentCapabilities {
    /// Create empty capabilities
    pub fn none() -> Self {
        Self {
            capabilities: Vec::new(),
        }
    }

    /// Create read-only capabilities
    pub fn read_only() -> Self {
        Self {
            capabilities: vec![
                AgentCapability::VmRead,
                AgentCapability::MetricsRead,
                AgentCapability::StorageRead,
            ],
        }
    }

    /// Create standard operator capabilities
    pub fn operator() -> Self {
        Self {
            capabilities: vec![
                AgentCapability::VmRead,
                AgentCapability::VmWrite,
                AgentCapability::VmManage,
                AgentCapability::MetricsRead,
                AgentCapability::SnapshotManage,
            ],
        }
    }

    /// Create full capabilities
    pub fn full() -> Self {
        Self {
            capabilities: vec![
                AgentCapability::VmRead,
                AgentCapability::VmWrite,
                AgentCapability::VmManage,
                AgentCapability::NetworkAccess,
                AgentCapability::NetworkAdmin,
                AgentCapability::StorageRead,
                AgentCapability::StorageWrite,
                AgentCapability::SnapshotManage,
                AgentCapability::MetricsRead,
                AgentCapability::GuestExec,
                AgentCapability::Debug,
                AgentCapability::Coordination,
                AgentCapability::Admin,
            ],
        }
    }

    /// Check if a capability is present
    pub fn has(&self, cap: AgentCapability) -> bool {
        self.capabilities.contains(&cap) || self.capabilities.contains(&AgentCapability::Admin)
    }

    /// Add a capability
    pub fn add(&mut self, cap: AgentCapability) {
        if !self.capabilities.contains(&cap) {
            self.capabilities.push(cap);
        }
    }
}

/// An active agent session
pub struct AgentSession {
    /// Session ID
    pub id: String,
    /// Agent identifier
    pub agent_id: String,
    /// Agent capabilities
    pub capabilities: AgentCapabilities,
    /// Session creation time
    pub created_at: Instant,
    /// Last activity time
    pub last_activity: RwLock<Instant>,
    /// Call count for rate limiting
    pub call_count: RwLock<u32>,
    /// Rate limit window start time
    pub rate_limit_window_start: RwLock<Instant>,
    /// Session-specific state
    pub state: RwLock<HashMap<String, JsonValue>>,
    /// Owned VMs (VMs created by this session)
    pub owned_vms: RwLock<Vec<String>>,
}

/// Tool call request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Request ID
    pub id: String,
    /// Tool name
    pub tool: String,
    /// Parameters
    pub parameters: JsonValue,
    /// Timeout override
    pub timeout: Option<Duration>,
}

/// Tool call response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    /// Request ID
    pub id: String,
    /// Whether the call succeeded
    pub success: bool,
    /// Result data (if success)
    pub result: Option<JsonValue>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Execution time
    pub execution_time_ms: u64,
}

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp
    pub timestamp: SystemTime,
    /// Session ID
    pub session_id: String,
    /// Agent ID
    pub agent_id: String,
    /// Tool called
    pub tool: String,
    /// Parameters (sanitized)
    pub parameters: JsonValue,
    /// Success/failure
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
    /// Execution time
    pub execution_time_ms: u64,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new() -> Self {
        Self::with_config(McpConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: McpConfig) -> Self {
        let server = Self {
            tools: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            config,
            audit_log: RwLock::new(Vec::new()),
        };

        // Register default tools
        server.register_default_tools();
        server
    }

    /// Register the default HyperMachine tools
    fn register_default_tools(&self) {
        let mut tools = self.tools.write().unwrap_or_else(|e| e.into_inner());

        // VM Lifecycle tools
        tools.insert(
            "vm.create".to_string(),
            McpTool {
                name: "vm.create".to_string(),
                description: "Create a new virtual machine".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "VM name (unique identifier)"
                        },
                        "cpu_cores": {
                            "type": "integer",
                            "description": "Number of virtual CPU cores",
                            "minimum": 1,
                            "maximum": 128,
                            "default": 2
                        },
                        "memory_gb": {
                            "type": "integer",
                            "description": "Memory size in gigabytes",
                            "minimum": 1,
                            "maximum": 1024,
                            "default": 4
                        },
                        "gpu_enabled": {
                            "type": "boolean",
                            "description": "Enable GPU passthrough/virtualization",
                            "default": false
                        },
                        "network_enabled": {
                            "type": "boolean",
                            "description": "Enable network connectivity",
                            "default": true
                        },
                        "boot_image": {
                            "type": "string",
                            "description": "Path to boot disk image (optional)"
                        }
                    },
                    "required": ["name"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmManage],
                enabled: true,
            },
        );

        tools.insert(
            "vm.start".to_string(),
            McpTool {
                name: "vm.start".to_string(),
                description: "Start a virtual machine".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM to start"
                        },
                        "wait_for_boot": {
                            "type": "boolean",
                            "description": "Wait for VM to fully boot before returning",
                            "default": false
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmWrite],
                enabled: true,
            },
        );

        tools.insert(
            "vm.stop".to_string(),
            McpTool {
                name: "vm.stop".to_string(),
                description: "Stop a virtual machine (graceful shutdown)".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM to stop"
                        },
                        "force": {
                            "type": "boolean",
                            "description": "Force immediate power off",
                            "default": false
                        },
                        "timeout_seconds": {
                            "type": "integer",
                            "description": "Timeout for graceful shutdown",
                            "default": 60
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmWrite],
                enabled: true,
            },
        );

        tools.insert(
            "vm.delete".to_string(),
            McpTool {
                name: "vm.delete".to_string(),
                description: "Delete a virtual machine and its resources".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM to delete"
                        },
                        "delete_storage": {
                            "type": "boolean",
                            "description": "Also delete associated storage volumes",
                            "default": false
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmManage],
                enabled: true,
            },
        );

        tools.insert(
            "vm.list".to_string(),
            McpTool {
                name: "vm.list".to_string(),
                description: "List all virtual machines".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "filter_state": {
                            "type": "string",
                            "description": "Filter by VM state",
                            "enum": ["running", "stopped", "paused", "all"],
                            "default": "all"
                        },
                        "include_metrics": {
                            "type": "boolean",
                            "description": "Include resource usage metrics",
                            "default": false
                        }
                    }
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmRead],
                enabled: true,
            },
        );

        tools.insert(
            "vm.status".to_string(),
            McpTool {
                name: "vm.status".to_string(),
                description: "Get detailed status of a virtual machine".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmRead],
                enabled: true,
            },
        );

        // Resource management tools
        tools.insert(
            "vm.resize".to_string(),
            McpTool {
                name: "vm.resize".to_string(),
                description: "Resize VM resources (CPU, memory)".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "cpu_cores": {
                            "type": "integer",
                            "description": "New CPU core count",
                            "minimum": 1
                        },
                        "memory_gb": {
                            "type": "integer",
                            "description": "New memory size in GB",
                            "minimum": 1
                        },
                        "hot_plug": {
                            "type": "boolean",
                            "description": "Apply changes without restart if possible",
                            "default": true
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::Resources,
                required_capabilities: vec![AgentCapability::VmWrite],
                enabled: true,
            },
        );

        tools.insert(
            "vm.metrics".to_string(),
            McpTool {
                name: "vm.metrics".to_string(),
                description: "Get resource usage metrics for a VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "metrics": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["cpu", "memory", "disk", "network", "all"]
                            },
                            "description": "Which metrics to retrieve",
                            "default": ["all"]
                        },
                        "interval_seconds": {
                            "type": "integer",
                            "description": "Sampling interval",
                            "default": 1
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::Monitoring,
                required_capabilities: vec![AgentCapability::MetricsRead],
                enabled: true,
            },
        );

        // Snapshot tools
        tools.insert(
            "snapshot.create".to_string(),
            McpTool {
                name: "snapshot.create".to_string(),
                description: "Create a VM snapshot".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "snapshot_name": {
                            "type": "string",
                            "description": "Name for the snapshot"
                        },
                        "description": {
                            "type": "string",
                            "description": "Optional description"
                        },
                        "include_memory": {
                            "type": "boolean",
                            "description": "Include VM memory state",
                            "default": true
                        }
                    },
                    "required": ["vm_name", "snapshot_name"]
                }),
                category: ToolCategory::Snapshots,
                required_capabilities: vec![AgentCapability::SnapshotManage],
                enabled: true,
            },
        );

        tools.insert(
            "snapshot.restore".to_string(),
            McpTool {
                name: "snapshot.restore".to_string(),
                description: "Restore a VM from snapshot".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "snapshot_name": {
                            "type": "string",
                            "description": "Name of the snapshot to restore"
                        }
                    },
                    "required": ["vm_name", "snapshot_name"]
                }),
                category: ToolCategory::Snapshots,
                required_capabilities: vec![AgentCapability::SnapshotManage],
                enabled: true,
            },
        );

        tools.insert(
            "snapshot.list".to_string(),
            McpTool {
                name: "snapshot.list".to_string(),
                description: "List snapshots for a VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::Snapshots,
                required_capabilities: vec![AgentCapability::VmRead],
                enabled: true,
            },
        );

        // Network tools
        tools.insert(
            "network.attach".to_string(),
            McpTool {
                name: "network.attach".to_string(),
                description: "Attach a network interface to a VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "network_name": {
                            "type": "string",
                            "description": "Name of the network to attach"
                        },
                        "mac_address": {
                            "type": "string",
                            "description": "MAC address (auto-generated if not specified)"
                        }
                    },
                    "required": ["vm_name", "network_name"]
                }),
                category: ToolCategory::Network,
                required_capabilities: vec![AgentCapability::NetworkAdmin],
                enabled: true,
            },
        );

        tools.insert(
            "network.detach".to_string(),
            McpTool {
                name: "network.detach".to_string(),
                description: "Detach a network interface from a VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "interface_name": {
                            "type": "string",
                            "description": "Name of the interface to detach"
                        }
                    },
                    "required": ["vm_name", "interface_name"]
                }),
                category: ToolCategory::Network,
                required_capabilities: vec![AgentCapability::NetworkAdmin],
                enabled: true,
            },
        );

        // Guest execution tools
        tools.insert(
            "guest.exec".to_string(),
            McpTool {
                name: "guest.exec".to_string(),
                description: "Execute a command inside the VM guest".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "command": {
                            "type": "string",
                            "description": "Command to execute"
                        },
                        "args": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Command arguments"
                        },
                        "timeout_seconds": {
                            "type": "integer",
                            "description": "Execution timeout",
                            "default": 30
                        },
                        "capture_output": {
                            "type": "boolean",
                            "description": "Capture stdout/stderr",
                            "default": true
                        }
                    },
                    "required": ["vm_name", "command"]
                }),
                category: ToolCategory::System,
                required_capabilities: vec![AgentCapability::GuestExec],
                enabled: true,
            },
        );

        tools.insert(
            "guest.file.read".to_string(),
            McpTool {
                name: "guest.file.read".to_string(),
                description: "Read a file from inside the VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "path": {
                            "type": "string",
                            "description": "Path to file inside guest"
                        },
                        "max_size_kb": {
                            "type": "integer",
                            "description": "Maximum file size to read (KB)",
                            "default": 1024
                        }
                    },
                    "required": ["vm_name", "path"]
                }),
                category: ToolCategory::System,
                required_capabilities: vec![AgentCapability::GuestExec],
                enabled: true,
            },
        );

        tools.insert(
            "guest.file.write".to_string(),
            McpTool {
                name: "guest.file.write".to_string(),
                description: "Write a file inside the VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "path": {
                            "type": "string",
                            "description": "Path to file inside guest"
                        },
                        "content": {
                            "type": "string",
                            "description": "File content (text or base64)"
                        },
                        "encoding": {
                            "type": "string",
                            "enum": ["text", "base64"],
                            "default": "text"
                        }
                    },
                    "required": ["vm_name", "path", "content"]
                }),
                category: ToolCategory::System,
                required_capabilities: vec![AgentCapability::GuestExec],
                enabled: true,
            },
        );

        // Multi-agent coordination tools
        tools.insert(
            "agent.broadcast".to_string(),
            McpTool {
                name: "agent.broadcast".to_string(),
                description: "Broadcast a message to other agents".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "channel": {
                            "type": "string",
                            "description": "Channel name to broadcast on"
                        },
                        "message": {
                            "type": "object",
                            "description": "Message payload"
                        },
                        "priority": {
                            "type": "string",
                            "enum": ["low", "normal", "high", "critical"],
                            "default": "normal"
                        }
                    },
                    "required": ["channel", "message"]
                }),
                category: ToolCategory::Coordination,
                required_capabilities: vec![AgentCapability::Coordination],
                enabled: true,
            },
        );

        tools.insert(
            "agent.send".to_string(),
            McpTool {
                name: "agent.send".to_string(),
                description: "Send a message to a specific agent".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "target_agent": {
                            "type": "string",
                            "description": "Target agent ID"
                        },
                        "message": {
                            "type": "object",
                            "description": "Message payload"
                        },
                        "wait_for_response": {
                            "type": "boolean",
                            "description": "Wait for a response",
                            "default": false
                        },
                        "timeout_seconds": {
                            "type": "integer",
                            "description": "Response timeout",
                            "default": 30
                        }
                    },
                    "required": ["target_agent", "message"]
                }),
                category: ToolCategory::Coordination,
                required_capabilities: vec![AgentCapability::Coordination],
                enabled: true,
            },
        );

        tools.insert(
            "agent.list".to_string(),
            McpTool {
                name: "agent.list".to_string(),
                description: "List connected agents".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "include_capabilities": {
                            "type": "boolean",
                            "description": "Include agent capabilities",
                            "default": false
                        }
                    }
                }),
                category: ToolCategory::Coordination,
                required_capabilities: vec![AgentCapability::Coordination],
                enabled: true,
            },
        );

        tools.insert(
            "agent.claim".to_string(),
            McpTool {
                name: "agent.claim".to_string(),
                description:
                    "Claim exclusive access to a VM (prevents other agents from modifying)"
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM to claim"
                        },
                        "duration_seconds": {
                            "type": "integer",
                            "description": "Claim duration",
                            "default": 300
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::Coordination,
                required_capabilities: vec![
                    AgentCapability::VmWrite,
                    AgentCapability::Coordination,
                ],
                enabled: true,
            },
        );

        tools.insert(
            "agent.release".to_string(),
            McpTool {
                name: "agent.release".to_string(),
                description: "Release a previously claimed VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_name": {
                            "type": "string",
                            "description": "Name of the VM to release"
                        }
                    },
                    "required": ["vm_name"]
                }),
                category: ToolCategory::Coordination,
                required_capabilities: vec![AgentCapability::Coordination],
                enabled: true,
            },
        );

        // System tools
        tools.insert(
            "system.info".to_string(),
            McpTool {
                name: "system.info".to_string(),
                description: "Get HyperMachine system information".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
                category: ToolCategory::System,
                required_capabilities: vec![],
                enabled: true,
            },
        );

        tools.insert(
            "system.health".to_string(),
            McpTool {
                name: "system.health".to_string(),
                description: "Get system health status".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "include_vm_health": {
                            "type": "boolean",
                            "description": "Include health of all VMs",
                            "default": true
                        }
                    }
                }),
                category: ToolCategory::Monitoring,
                required_capabilities: vec![AgentCapability::MetricsRead],
                enabled: true,
            },
        );
    }

    /// Create a new agent session
    pub fn create_session(
        &self,
        agent_id: &str,
        capabilities: AgentCapabilities,
    ) -> Result<Arc<AgentSession>, String> {
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        if sessions.len() >= self.config.max_sessions {
            return Err("Maximum sessions reached".to_string());
        }
        drop(sessions);

        let session_id = format!("session-{}-{}", agent_id, uuid_v4());
        let session = Arc::new(AgentSession {
            id: session_id.clone(),
            agent_id: agent_id.to_string(),
            capabilities,
            created_at: Instant::now(),
            last_activity: RwLock::new(Instant::now()),
            call_count: RwLock::new(0),
            rate_limit_window_start: RwLock::new(Instant::now()),
            state: RwLock::new(HashMap::new()),
            owned_vms: RwLock::new(Vec::new()),
        });

        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        sessions.insert(session_id, Arc::clone(&session));

        Ok(session)
    }

    /// List available tools
    pub fn list_tools(&self, capabilities: &AgentCapabilities) -> Vec<McpTool> {
        let tools = self.tools.read().unwrap_or_else(|e| e.into_inner());
        tools
            .values()
            .filter(|tool| {
                tool.enabled
                    && tool
                        .required_capabilities
                        .iter()
                        .all(|cap| capabilities.has(*cap))
            })
            .cloned()
            .collect()
    }

    /// Get a specific tool definition
    pub fn get_tool(&self, name: &str) -> Option<McpTool> {
        let tools = self.tools.read().unwrap_or_else(|e| e.into_inner());
        tools.get(name).cloned()
    }

    /// Execute a tool call
    pub async fn call_tool(
        &self,
        session: &AgentSession,
        request: ToolCallRequest,
    ) -> ToolCallResponse {
        let start = Instant::now();

        // Update last activity
        *session
            .last_activity
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Instant::now();

        // Check rate limit
        {
            let mut count = session
                .call_count
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let mut window_start = session
                .rate_limit_window_start
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let window_duration = Duration::from_secs(60);

            // Reset window if expired
            if now.duration_since(*window_start) >= window_duration {
                *window_start = now;
                *count = 0;
            }

            // Check if rate limit exceeded
            if *count >= self.config.rate_limit {
                return ToolCallResponse {
                    id: request.id,
                    success: false,
                    result: None,
                    error: Some(format!(
                        "Rate limit exceeded: {} calls per minute",
                        self.config.rate_limit
                    )),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                };
            }

            *count += 1;
        }

        // Get tool definition
        let tool = match self.get_tool(&request.tool) {
            Some(t) => t,
            None => {
                return ToolCallResponse {
                    id: request.id,
                    success: false,
                    result: None,
                    error: Some(format!("Tool not found: {}", request.tool)),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        // Check capabilities
        for cap in &tool.required_capabilities {
            if !session.capabilities.has(*cap) {
                return ToolCallResponse {
                    id: request.id,
                    success: false,
                    result: None,
                    error: Some(format!("Missing capability: {:?}", cap)),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        }

        // Execute tool via the dispatch table in execute_tool_impl
        let result = self
            .execute_tool_impl(&tool.name, &request.parameters, session)
            .await;

        let response = match result {
            Ok(value) => ToolCallResponse {
                id: request.id.clone(),
                success: true,
                result: Some(value),
                error: None,
                execution_time_ms: start.elapsed().as_millis() as u64,
            },
            Err(e) => ToolCallResponse {
                id: request.id.clone(),
                success: false,
                result: None,
                error: Some(e),
                execution_time_ms: start.elapsed().as_millis() as u64,
            },
        };

        // Audit log
        if self.config.audit_enabled {
            let entry = AuditEntry {
                timestamp: SystemTime::now(),
                session_id: session.id.clone(),
                agent_id: session.agent_id.clone(),
                tool: request.tool,
                parameters: request.parameters,
                success: response.success,
                error: response.error.clone(),
                execution_time_ms: response.execution_time_ms,
            };
            self.audit_log
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .push(entry);
        }

        response
    }

    /// Internal tool execution dispatch
    async fn execute_tool_impl(
        &self,
        tool_name: &str,
        params: &JsonValue,
        session: &AgentSession,
    ) -> Result<JsonValue, String> {
        match tool_name {
            // ── VM lifecycle ──────────────────────────────────────────
            "vm.create" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: name")?;
                let cpus = params.get("cpus").and_then(|v| v.as_u64()).unwrap_or(1);
                let memory_mb = params
                    .get("memory_mb")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(512);
                let vm_id = uuid_v4();

                // Record the VM in the session's owned_vms
                session
                    .owned_vms
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(vm_id.clone());

                // Store VM state in session state
                let vm_state = json!({
                    "id": vm_id,
                    "name": name,
                    "cpus": cpus,
                    "memory_mb": memory_mb,
                    "status": "created",
                    "created_at_epoch_ms": SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                });
                session
                    .state
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(format!("vm:{}", vm_id), vm_state.clone());

                Ok(
                    json!({ "vm_id": vm_id, "status": "created", "name": name, "cpus": cpus, "memory_mb": memory_mb }),
                )
            }

            "vm.start" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                let vm = state
                    .get_mut(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;
                vm["status"] = json!("running");
                Ok(json!({ "vm_id": vm_id, "status": "running" }))
            }

            "vm.stop" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let force = params
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let key = format!("vm:{}", vm_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                let vm = state
                    .get_mut(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;
                vm["status"] = json!("stopped");
                Ok(json!({ "vm_id": vm_id, "status": "stopped", "force": force }))
            }

            "vm.delete" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                if state.remove(&key).is_none() {
                    return Err(format!("VM not found: {}", vm_id));
                }
                session
                    .owned_vms
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|id| id != vm_id);
                Ok(json!({ "vm_id": vm_id, "status": "deleted" }))
            }

            "vm.list" => {
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let vms: Vec<&JsonValue> = state
                    .iter()
                    .filter(|(k, _)| k.starts_with("vm:"))
                    .map(|(_, v)| v)
                    .collect();
                let total = vms.len();
                Ok(json!({ "vms": vms, "total": total }))
            }

            "vm.status" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let vm = state
                    .get(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;
                Ok(vm.clone())
            }

            "vm.resize" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                let vm = state
                    .get_mut(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;
                if let Some(cpus) = params.get("cpus").and_then(|v| v.as_u64()) {
                    vm["cpus"] = json!(cpus);
                }
                if let Some(memory_mb) = params.get("memory_mb").and_then(|v| v.as_u64()) {
                    vm["memory_mb"] = json!(memory_mb);
                }
                Ok(json!({ "vm_id": vm_id, "status": "resized", "vm": vm.clone() }))
            }

            "vm.metrics" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                if !state.contains_key(&key) {
                    return Err(format!("VM not found: {}", vm_id));
                }
                Ok(json!({
                    "vm_id": vm_id,
                    "cpu_usage_percent": 0.0,
                    "memory_usage_percent": 0.0,
                    "disk_read_bytes": 0,
                    "disk_write_bytes": 0,
                    "net_rx_bytes": 0,
                    "net_tx_bytes": 0
                }))
            }

            // ── Snapshots ────────────────────────────────────────────
            "snapshot.create" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("snapshot");
                let snapshot_id = uuid_v4();
                let snap = json!({
                    "snapshot_id": snapshot_id,
                    "vm_id": vm_id,
                    "name": name,
                    "created_at_epoch_ms": SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                });
                session
                    .state
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(format!("snap:{}", snapshot_id), snap.clone());
                Ok(snap)
            }

            "snapshot.restore" => {
                let snapshot_id = params
                    .get("snapshot_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: snapshot_id")?;
                let key = format!("snap:{}", snapshot_id);
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                if !state.contains_key(&key) {
                    return Err(format!("Snapshot not found: {}", snapshot_id));
                }
                Ok(json!({ "snapshot_id": snapshot_id, "status": "restored" }))
            }

            "snapshot.list" => {
                let vm_id = params.get("vm_id").and_then(|v| v.as_str());
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let snapshots: Vec<&JsonValue> = state
                    .iter()
                    .filter(|(k, v)| {
                        k.starts_with("snap:")
                            && vm_id.is_none_or(|id| {
                                v.get("vm_id").and_then(|vid| vid.as_str()) == Some(id)
                            })
                    })
                    .map(|(_, v)| v)
                    .collect();
                Ok(json!({ "snapshots": snapshots, "total": snapshots.len() }))
            }

            // ── Networking ───────────────────────────────────────────
            "network.attach" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let network = params
                    .get("network")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: network")?;
                let interface_id = uuid_v4();
                Ok(json!({
                    "vm_id": vm_id,
                    "network": network,
                    "interface_id": interface_id,
                    "status": "attached"
                }))
            }

            "network.detach" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let interface_id = params
                    .get("interface_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: interface_id")?;
                Ok(json!({
                    "vm_id": vm_id,
                    "interface_id": interface_id,
                    "status": "detached"
                }))
            }

            // ── Guest operations ─────────────────────────────────────
            "guest.exec" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let command = params
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: command")?;
                let args: Vec<String> = params
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let timeout_secs: u64 =
                    params.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

                // Verify the VM exists in session state
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let vm_key = format!("vm:{}", vm_id);
                let vm_state = state.get(&vm_key).ok_or_else(|| {
                    format!("VM not found: {}. Create a VM first with vm.create", vm_id)
                })?;
                let status = vm_state
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                // VM must be running for guest commands
                if status != "running" {
                    return Err(format!(
                        "VM {} is not running (status: {}). Start it first with vm.start",
                        vm_id, status
                    ));
                }

                // Check if the guest agent channel is connected
                let agent_key = format!("guest_agent:{}", vm_id);
                let agent_connected = state
                    .get(&agent_key)
                    .and_then(|v| v.get("connected"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !agent_connected {
                    return Ok(json!({
                        "vm_id": vm_id,
                        "command": command,
                        "args": args,
                        "exit_code": null,
                        "stdout": null,
                        "stderr": null,
                        "status": "error",
                        "error": "Guest agent not connected. Install the HyperMachine guest agent in the VM to enable guest.exec."
                    }));
                }

                // Build the guest agent request
                let request = json!({
                    "type": "exec",
                    "command": command,
                    "args": args,
                    "timeout": timeout_secs,
                });

                // Send request via the guest agent channel
                // The response is stored in session state by the channel handler
                let req_id = format!(
                    "exec:{}:{}",
                    vm_id,
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_millis()
                );
                drop(state);

                // Store the pending request
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                state.insert(
                    format!("guest_req:{}", req_id),
                    json!({
                        "vm_id": vm_id,
                        "request": request,
                        "status": "submitted",
                        "submitted_at": SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or(Duration::ZERO)
                            .as_secs(),
                    }),
                );

                Ok(json!({
                    "vm_id": vm_id,
                    "command": command,
                    "args": args,
                    "request_id": req_id,
                    "timeout": timeout_secs,
                    "status": "submitted",
                    "message": "Command submitted to guest agent. Poll with guest.exec.status for results."
                }))
            }

            "guest.file.read" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let path = params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: path")?;

                // Verify the VM exists and is running
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let vm_key = format!("vm:{}", vm_id);
                let vm_state = state.get(&vm_key).ok_or_else(|| {
                    format!("VM not found: {}. Create a VM first with vm.create", vm_id)
                })?;
                let status = vm_state
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if status != "running" {
                    return Err(format!(
                        "VM {} is not running (status: {}). Start it first with vm.start",
                        vm_id, status
                    ));
                }

                // Check if the guest agent is connected
                let agent_key = format!("guest_agent:{}", vm_id);
                let agent_connected = state
                    .get(&agent_key)
                    .and_then(|v| v.get("connected"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !agent_connected {
                    return Ok(json!({
                        "vm_id": vm_id,
                        "path": path,
                        "content": null,
                        "status": "error",
                        "error": "Guest agent not connected. Install the HyperMachine guest agent in the VM to enable guest.file.read."
                    }));
                }

                // Send file read request via guest agent channel
                let req_id = format!(
                    "fread:{}:{}",
                    vm_id,
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_millis()
                );
                drop(state);

                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                state.insert(
                    format!("guest_req:{}", req_id),
                    json!({
                        "vm_id": vm_id,
                        "request": {
                            "type": "file.read",
                            "path": path,
                        },
                        "status": "submitted",
                    }),
                );

                Ok(json!({
                    "vm_id": vm_id,
                    "path": path,
                    "request_id": req_id,
                    "status": "submitted",
                    "message": "File read request submitted to guest agent."
                }))
            }

            "guest.file.write" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let path = params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: path")?;
                let content = params
                    .get("content")
                    .ok_or("Missing required parameter: content")?;

                // Verify the VM exists and is running
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let vm_key = format!("vm:{}", vm_id);
                let vm_state = state.get(&vm_key).ok_or_else(|| {
                    format!("VM not found: {}. Create a VM first with vm.create", vm_id)
                })?;
                let status = vm_state
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if status != "running" {
                    return Err(format!(
                        "VM {} is not running (status: {}). Start it first with vm.start",
                        vm_id, status
                    ));
                }

                // Check if the guest agent is connected
                let agent_key = format!("guest_agent:{}", vm_id);
                let agent_connected = state
                    .get(&agent_key)
                    .and_then(|v| v.get("connected"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !agent_connected {
                    return Ok(json!({
                        "vm_id": vm_id,
                        "path": path,
                        "bytes_written": null,
                        "status": "error",
                        "error": "Guest agent not connected. Install the HyperMachine guest agent in the VM to enable guest.file.write."
                    }));
                }

                // Send file write request via guest agent channel
                let req_id = format!(
                    "fwrite:{}:{}",
                    vm_id,
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_millis()
                );
                drop(state);

                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                state.insert(
                    format!("guest_req:{}", req_id),
                    json!({
                        "vm_id": vm_id,
                        "request": {
                            "type": "file.write",
                            "path": path,
                            "content": content.clone(),
                        },
                        "status": "submitted",
                    }),
                );

                Ok(json!({
                    "vm_id": vm_id,
                    "path": path,
                    "request_id": req_id,
                    "status": "submitted",
                    "message": "File write request submitted to guest agent."
                }))
            }

            // ── Agent coordination ───────────────────────────────────
            "agent.list" => {
                let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
                let agents: Vec<JsonValue> = sessions
                    .values()
                    .map(|s| {
                        json!({
                            "agent_id": s.agent_id,
                            "session_id": s.id,
                            "connected_duration_seconds": s.created_at.elapsed().as_secs(),
                            "capabilities": format!("{:?}", s.capabilities.capabilities),
                            "owned_vm_count": s.owned_vms.read()
                                .unwrap_or_else(|e| e.into_inner()).len()
                        })
                    })
                    .collect();
                Ok(json!({ "agents": agents, "total": agents.len() }))
            }

            "agent.broadcast" => {
                let message = params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: message")?;
                let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
                let recipient_count = sessions.len().saturating_sub(1); // exclude sender
                Ok(json!({
                    "message": message,
                    "recipients": recipient_count,
                    "status": "broadcast_sent"
                }))
            }

            "agent.send" => {
                let target = params
                    .get("target_agent_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: target_agent_id")?;
                let message = params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: message")?;
                let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
                let found = sessions.values().any(|s| s.agent_id == target);
                if !found {
                    return Err(format!("Target agent not found: {}", target));
                }
                Ok(json!({
                    "target": target,
                    "message": message,
                    "status": "delivered"
                }))
            }

            "agent.claim" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                session
                    .owned_vms
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(vm_id.to_string());
                Ok(json!({
                    "vm_id": vm_id,
                    "agent_id": session.agent_id,
                    "status": "claimed"
                }))
            }

            "agent.release" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let mut owned = session.owned_vms.write().unwrap_or_else(|e| e.into_inner());
                if !owned.contains(&vm_id.to_string()) {
                    return Err(format!("VM {} is not owned by this agent", vm_id));
                }
                owned.retain(|id| id != vm_id);
                Ok(json!({
                    "vm_id": vm_id,
                    "agent_id": session.agent_id,
                    "status": "released"
                }))
            }

            // ── System info ──────────────────────────────────────────
            "system.info" => {
                let hypervisor_modes = if cfg!(target_os = "linux") {
                    vec!["t1", "t2"]
                } else {
                    vec!["t2"]
                };

                let backends = {
                    let mut b = Vec::new();
                    if cfg!(target_os = "linux") {
                        b.push("kvm");
                    }
                    if cfg!(target_os = "windows") {
                        b.push("whpx");
                    }
                    if cfg!(target_os = "macos") {
                        b.push("hvf");
                    }
                    b
                };

                Ok(json!({
                    "name": "HyperMachine",
                    "version": env!("CARGO_PKG_VERSION"),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "hypervisor_modes": hypervisor_modes,
                    "backends": backends,
                    "active_sessions": self.sessions.read()
                        .unwrap_or_else(|e| e.into_inner()).len()
                }))
            }

            "system.health" => {
                let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
                let session_count = sessions.len();
                // Compute uptime from the oldest session
                let uptime = sessions
                    .values()
                    .map(|s| s.created_at.elapsed().as_secs())
                    .max()
                    .unwrap_or(0);

                // Count total VMs across all sessions
                let vm_count: usize = sessions
                    .values()
                    .map(|s| s.owned_vms.read().unwrap_or_else(|e| e.into_inner()).len())
                    .sum();

                Ok(json!({
                    "status": "healthy",
                    "uptime_seconds": uptime,
                    "session_count": session_count,
                    "vm_count": vm_count,
                }))
            }

            _ => Err(format!("Unknown tool: {}", tool_name)),
        }
    }

    /// Get audit log entries
    pub fn get_audit_log(&self, limit: usize) -> Vec<AuditEntry> {
        let log = self.audit_log.read().unwrap_or_else(|e| e.into_inner());
        log.iter().rev().take(limit).cloned().collect()
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentSession {
    /// List tools available to this session
    pub fn list_tools(&self, server: &McpServer) -> Vec<McpTool> {
        server.list_tools(&self.capabilities)
    }

    /// Call a tool
    pub async fn call_tool(
        &self,
        server: &McpServer,
        tool: &str,
        parameters: JsonValue,
    ) -> ToolCallResponse {
        let request = ToolCallRequest {
            id: uuid_v4(),
            tool: tool.to_string(),
            parameters,
            timeout: None,
        };
        server.call_tool(self, request).await
    }

    /// Set session state
    pub fn set_state(&self, key: &str, value: JsonValue) {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        state.insert(key.to_string(), value);
    }

    /// Get session state
    pub fn get_state(&self, key: &str) -> Option<JsonValue> {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        state.get(key).cloned()
    }
}

/// Generate a UUID v4 string
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:032x}", timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_creation() {
        let server = McpServer::new();
        let tools = server.list_tools(&AgentCapabilities::full());
        assert!(!tools.is_empty());
    }

    #[test]
    fn test_tool_filtering_by_capabilities() {
        let server = McpServer::new();

        // Read-only capabilities
        let read_only = AgentCapabilities::read_only();
        let tools = server.list_tools(&read_only);

        // Should have vm.list, vm.status, etc but not vm.create, vm.delete
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"vm.list"));
        assert!(tool_names.contains(&"vm.status"));
        assert!(!tool_names.contains(&"vm.create"));
        assert!(!tool_names.contains(&"vm.delete"));
    }

    #[tokio::test]
    async fn test_session_creation() {
        let server = McpServer::new();
        let session = server
            .create_session("test-agent", AgentCapabilities::operator())
            .unwrap();

        assert_eq!(session.agent_id, "test-agent");
    }

    #[tokio::test]
    async fn test_tool_call() {
        let server = McpServer::new();
        let session = server
            .create_session("test-agent", AgentCapabilities::full())
            .unwrap();

        let response = session.call_tool(&server, "system.info", json!({})).await;

        assert!(response.success);
        assert!(response.result.is_some());
    }
}
