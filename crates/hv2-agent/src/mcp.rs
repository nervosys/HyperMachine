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
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// Details of a reclaimed session, handed to a [`TeardownHook`] so the runtime
/// can release the real resources (e.g. VMs) the session owned.
#[derive(Debug, Clone)]
pub struct SessionTeardown {
    /// The closed session's id.
    pub session_id: String,
    /// The agent that owned the session.
    pub agent_id: String,
    /// VM ids the session created and still owned at teardown.
    pub owned_vms: Vec<String>,
}

/// Callback invoked when a session is closed or reclaimed. A runtime registers
/// one via [`McpServer::on_session_teardown`] to tear down owned resources.
pub type TeardownHook = Arc<dyn Fn(&SessionTeardown) + Send + Sync>;

/// MCP Server - manages agent sessions and tool execution
pub struct McpServer {
    /// Registered tools
    tools: RwLock<HashMap<String, McpTool>>,
    /// Active sessions
    sessions: RwLock<HashMap<String, Arc<AgentSession>>>,
    /// Server configuration
    config: McpConfig,
    /// Audit log
    audit_log: RwLock<VecDeque<AuditEntry>>,
    /// Optional hook invoked when a session is closed/reclaimed.
    teardown_hook: RwLock<Option<TeardownHook>>,
    /// Where the `vm.*` tools dispatch.
    ///
    /// `None` — the default — keeps the server's own bookkeeping, so tool
    /// schemas, capability scoping, and agent logic are exercisable with no
    /// hypervisor present. Install one with [`McpServer::set_vm_host`] and the
    /// same tool calls drive real VMs.
    vm_host: RwLock<Option<Arc<dyn crate::vm_host::VmHost>>>,
    /// Where the `gpu.*` tools dispatch, on the same terms as `vm_host`.
    gpu_host: RwLock<Option<Arc<dyn crate::gpu_host::GpuHost>>>,
    /// Where the `image.*` tools read the fleet allowlist.
    ///
    /// `None` leaves those tools reporting that no registry is installed,
    /// rather than inventing an empty one — an agent must be able to tell "no
    /// images are approved" from "nobody is tracking images".
    image_host: RwLock<Option<Arc<dyn crate::image_host::ImageHost>>>,
    /// Governance evaluated before every tool call.
    ///
    /// `None` — the default — leaves capabilities and VM ownership as the only
    /// gate, which is what the server has always done. Install a set with
    /// [`McpServer::set_policy_set`] and every call is additionally checked
    /// against it. Note that [`PolicySet`](crate::policies::PolicySet) denies by
    /// default, so an installed set must explicitly allow what agents may do —
    /// including any tool added later, which a set written today cannot name.
    policies: RwLock<Option<Arc<crate::policies::PolicySet>>>,
    /// Ceiling on tool calls executing at once, if one is installed.
    ///
    /// Distinct from `McpConfig::rate_limit`, which bounds calls per minute
    /// per session: an agent can sit under that and still have many long calls
    /// in flight at once. `None` — the default — places no ceiling.
    concurrency: RwLock<Option<Arc<crate::limits::ConcurrencyLimiter>>>,
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
    /// Maximum retained audit-log entries; the oldest are dropped beyond this
    /// so a long-running agent runtime cannot grow the log without bound.
    pub max_audit_entries: usize,
    /// Sessions idle longer than this are eligible for automatic reclamation
    /// when `create_session` hits `max_sessions`, so dead agent sessions don't
    /// permanently consume slots.
    pub session_idle_timeout: Duration,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            default_timeout: Duration::from_secs(60),
            audit_enabled: true,
            rate_limit: 100,
            max_audit_entries: 10_000,
            session_idle_timeout: Duration::from_secs(30 * 60),
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
    /// GPU fabric: device inventory and VM accelerator allocation
    GpuFabric,
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
            audit_log: RwLock::new(VecDeque::new()),
            teardown_hook: RwLock::new(None),
            vm_host: RwLock::new(None),
            gpu_host: RwLock::new(None),
            image_host: RwLock::new(None),
            policies: RwLock::new(None),
            concurrency: RwLock::new(None),
        };

        // Register default tools
        server.register_default_tools();
        server
    }

    /// Point the `vm.*` tools at a real hypervisor.
    ///
    /// Until this is called the server keeps its own per-session VM records:
    /// the tools validate parameters and enforce the lifecycle state machine,
    /// but no guest is created. After it, `vm.create` allocates a VM and
    /// `vm.start` boots it.
    ///
    /// Ownership is still enforced by the server — a session may only act on
    /// VMs it created — so a host shared between sessions stays isolated.
    pub fn set_vm_host(&self, host: Arc<dyn crate::vm_host::VmHost>) {
        *self.vm_host.write().unwrap_or_else(|e| e.into_inner()) = Some(host);
    }

    /// The installed VM host, if any.
    pub fn vm_host(&self) -> Option<Arc<dyn crate::vm_host::VmHost>> {
        self.vm_host
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Point the `gpu.*` tools at a real GPU fabric.
    ///
    /// Without one the server tracks devices per session, which cannot model
    /// the thing that actually matters about GPUs: a physical device attached
    /// to one VM is unavailable to every other, across every session. A host
    /// enforces that exclusivity fleet-wide.
    pub fn set_gpu_host(&self, host: Arc<dyn crate::gpu_host::GpuHost>) {
        *self.gpu_host.write().unwrap_or_else(|e| e.into_inner()) = Some(host);
    }

    /// The installed GPU host, if any.
    pub fn gpu_host(&self) -> Option<Arc<dyn crate::gpu_host::GpuHost>> {
        self.gpu_host
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Let agents read the fleet image allowlist.
    ///
    /// Admission is enforced at `VM::provision` regardless; this exists so an
    /// agent can ask what it may boot *before* composing a plan, instead of
    /// discovering the answer when the VM refuses to start. Share the same
    /// registry the API server uses so the two cannot disagree.
    pub fn set_image_host(&self, host: Arc<dyn crate::image_host::ImageHost>) {
        *self.image_host.write().unwrap_or_else(|e| e.into_inner()) = Some(host);
    }

    /// The installed image host, if any.
    pub fn image_host(&self) -> Option<Arc<dyn crate::image_host::ImageHost>> {
        self.image_host
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Govern every tool call with a policy set.
    ///
    /// Capabilities answer "may this agent use this kind of tool at all"; a
    /// policy set answers "may this agent take this action, on this resource,
    /// right now" — the questions capabilities cannot express, such as denying
    /// destructive actions outside a maintenance window.
    ///
    /// [`PolicySet`](crate::policies::PolicySet) denies by default. A set that
    /// does not name a tool therefore blocks it, which is the safe direction for
    /// tools added after the set was written, but does mean an installed policy
    /// must be kept current with the registry.
    pub fn set_policy_set(&self, policies: Arc<crate::policies::PolicySet>) {
        *self.policies.write().unwrap_or_else(|e| e.into_inner()) = Some(policies);
    }

    /// The installed policy set, if any.
    pub fn policy_set(&self) -> Option<Arc<crate::policies::PolicySet>> {
        self.policies
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Cap how many tool calls may execute at once.
    ///
    /// `McpConfig::rate_limit` already bounds calls per minute per session,
    /// which is a different question: a well-behaved agent can stay under a
    /// rate limit while holding dozens of slow calls open simultaneously. This
    /// bounds that. Off by default, because the right ceiling depends on what
    /// the installed hosts do.
    pub fn set_concurrency_limit(&self, max_in_flight: u32) {
        *self.concurrency.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(
            crate::limits::ConcurrencyLimiter::new(max_in_flight),
        ));
    }

    /// The installed concurrency limiter, if any.
    pub fn concurrency_limiter(&self) -> Option<Arc<crate::limits::ConcurrencyLimiter>> {
        self.concurrency
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remove the concurrency ceiling.
    pub fn clear_concurrency_limit(&self) {
        *self.concurrency.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Stop governing tool calls, returning to capabilities and ownership only.
    pub fn clear_policy_set(&self) {
        *self.policies.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The action and resource a tool call represents, for policy evaluation.
    ///
    /// A tool the mapping does not recognise becomes
    /// [`PolicyAction::Custom`](crate::policies::PolicyAction::Custom) carrying
    /// the tool name, so a policy can still speak about it by name and a
    /// deny-by-default set still refuses it.
    fn policy_request(
        tool_name: &str,
        params: &JsonValue,
    ) -> (crate::policies::PolicyAction, crate::policies::ResourceId) {
        use crate::policies::{PolicyAction, ResourceId};

        let action = match tool_name {
            "vm.create" => PolicyAction::VmCreate,
            "vm.start" => PolicyAction::VmStart,
            "vm.stop" => PolicyAction::VmStop,
            "vm.pause" => PolicyAction::VmPause,
            "vm.resume" => PolicyAction::VmResume,
            "vm.delete" => PolicyAction::VmDelete,
            "vm.resize" => PolicyAction::ResourceModify,
            "vm.list" | "vm.status" | "vm.metrics" | "gpu.list" | "snapshot.list"
            | "agent.list" | "system.info" | "system.health" => PolicyAction::ResourceRead,
            "snapshot.create" => PolicyAction::SnapshotCreate,
            "snapshot.restore" => PolicyAction::SnapshotRestore,
            "network.attach" => PolicyAction::NetworkAttach,
            "network.detach" => PolicyAction::NetworkDetach,
            "gpu.attach" | "gpu.register" => PolicyAction::ResourceAllocate,
            "gpu.detach" => PolicyAction::ResourceDeallocate,
            other => PolicyAction::Custom(other.to_string()),
        };

        // The namespace before the dot is the resource type, so a rule can be
        // written against `vm` or `gpu` as a whole with `ResourceId::wildcard`.
        let resource_type = tool_name.split('.').next().unwrap_or(tool_name);
        let identifier = ["vm_id", "device_id", "snapshot_id", "name"]
            .iter()
            .find_map(|key| params.get(*key).and_then(|v| v.as_str()));

        let resource = match identifier {
            Some(id) => ResourceId::new(resource_type, id),
            None => ResourceId::wildcard(resource_type),
        };

        (action, resource)
    }

    /// Build the evaluation context for `session`, stamped with the current
    /// UTC wall-clock time so time-window rules can fire.
    fn policy_context(session: &AgentSession) -> crate::policies::PolicyContext {
        let secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let days = secs / 86_400;
        let seconds_today = secs % 86_400;
        // 1970-01-01 was a Thursday, and `TimeWindow` counts 0 = Sunday.
        let day_of_week = ((days + 4) % 7) as u8;

        crate::policies::PolicyContext::new(&session.agent_id).with_time(
            (seconds_today / 3600) as u8,
            ((seconds_today % 3600) / 60) as u8,
            day_of_week,
        )
    }

    /// Whether a session may act on `vm_id`.
    ///
    /// With a shared host, the VM's existence is no longer proof that the
    /// caller created it — so ownership is checked explicitly before any tool
    /// touches a VM.
    fn session_owns(session: &AgentSession, vm_id: &str) -> bool {
        session
            .owned_vms
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|id| id == vm_id)
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
                        "boot": {
                            "type": "object",
                            "description": "What the VM boots. Omit for a VM with no guest \
                                            code — it can be started, but nothing executes.",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["linux", "multiboot", "raw"],
                                    "description": "Boot protocol"
                                },
                                "kernel": {
                                    "type": "string",
                                    "description": "Path to the kernel image (linux, multiboot)"
                                },
                                "initrd": {
                                    "type": "string",
                                    "description": "Path to an initial ramdisk (linux)"
                                },
                                "cmdline": {
                                    "type": "string",
                                    "description": "Kernel command line (linux, multiboot)"
                                },
                                "image": {
                                    "type": "string",
                                    "description": "Path to a raw image loaded verbatim (raw)"
                                }
                            },
                            "required": ["type"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to start"
                        },
                        "wait_for_boot": {
                            "type": "boolean",
                            "description": "Wait for VM to fully boot before returning",
                            "default": false
                        }
                    },
                    "required": ["vm_id"]
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
                        "vm_id": {
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
                    "required": ["vm_id"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmWrite],
                enabled: true,
            },
        );

        tools.insert(
            "vm.pause".to_string(),
            McpTool {
                name: "vm.pause".to_string(),
                description: "Pause a running virtual machine, freezing its vCPUs while retaining memory state".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to pause"
                        }
                    },
                    "required": ["vm_id"]
                }),
                category: ToolCategory::VmLifecycle,
                required_capabilities: vec![AgentCapability::VmWrite],
                enabled: true,
            },
        );

        tools.insert(
            "vm.resume".to_string(),
            McpTool {
                name: "vm.resume".to_string(),
                description: "Resume a paused virtual machine, restoring vCPU execution from frozen memory state".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to resume"
                        }
                    },
                    "required": ["vm_id"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to delete"
                        },
                        "delete_storage": {
                            "type": "boolean",
                            "description": "Also delete associated storage volumes",
                            "default": false
                        }
                    },
                    "required": ["vm_id"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM"
                        }
                    },
                    "required": ["vm_id"]
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
                        "vm_id": {
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
                    "required": ["vm_id"]
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
                        "vm_id": {
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
                    "required": ["vm_id"]
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
                        "vm_id": {
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
                    "required": ["vm_id", "snapshot_name"]
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
                        "snapshot_id": {
                            "type": "string",
                            "description": "ID of the snapshot to restore (from snapshot.create)"
                        }
                    },
                    "required": ["snapshot_id"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM"
                        }
                    },
                    "required": ["vm_id"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "network": {
                            "type": "string",
                            "description": "Name of the network to attach"
                        },
                        "mac_address": {
                            "type": "string",
                            "description": "MAC address (auto-generated if not specified)"
                        }
                    },
                    "required": ["vm_id", "network"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM"
                        },
                        "interface_id": {
                            "type": "string",
                            "description": "ID of the interface to detach (from network.attach)"
                        }
                    },
                    "required": ["vm_id", "interface_id"]
                }),
                category: ToolCategory::Network,
                required_capabilities: vec![AgentCapability::NetworkAdmin],
                enabled: true,
            },
        );

        // GPU fabric tools — device inventory and accelerator allocation. The
        // agent card advertises "GPU Compute Orchestration"; these are the
        // concrete tools an agent uses to drive it.
        tools.insert(
            "gpu.register".to_string(),
            McpTool {
                name: "gpu.register".to_string(),
                description:
                    "Register a GPU device into the fabric inventory so it can be allocated to VMs"
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "device_id": {
                            "type": "string",
                            "description": "Unique device identifier (e.g. gpu-0)"
                        },
                        "model": {
                            "type": "string",
                            "description": "GPU model (e.g. H100, A100)"
                        },
                        "vram_gb": {
                            "type": "integer",
                            "description": "On-board VRAM in GB",
                            "minimum": 1,
                            "default": 80
                        },
                        "compute_capability": {
                            "type": "integer",
                            "description": "Compute capability tier (80 = A100, 90 = H100)",
                            "default": 90
                        }
                    },
                    "required": ["device_id", "model"]
                }),
                category: ToolCategory::GpuFabric,
                required_capabilities: vec![AgentCapability::Admin],
                enabled: true,
            },
        );

        tools.insert(
            "gpu.list".to_string(),
            McpTool {
                name: "gpu.list".to_string(),
                description: "List GPU devices in the fabric and their allocation status"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "only_available": {
                            "type": "boolean",
                            "description": "Return only unallocated devices",
                            "default": false
                        }
                    }
                }),
                category: ToolCategory::GpuFabric,
                required_capabilities: vec![AgentCapability::MetricsRead],
                enabled: true,
            },
        );

        tools.insert(
            "gpu.attach".to_string(),
            McpTool {
                name: "gpu.attach".to_string(),
                description: "Attach a free GPU device to a VM, reserving it for that VM".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to attach the GPU to"
                        },
                        "device_id": {
                            "type": "string",
                            "description": "GPU device to attach (must be registered and unallocated)"
                        }
                    },
                    "required": ["vm_id", "device_id"]
                }),
                category: ToolCategory::GpuFabric,
                required_capabilities: vec![AgentCapability::VmWrite],
                enabled: true,
            },
        );

        tools.insert(
            "gpu.detach".to_string(),
            McpTool {
                name: "gpu.detach".to_string(),
                description: "Detach a GPU device from a VM and return it to the available pool"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM the GPU is attached to"
                        },
                        "device_id": {
                            "type": "string",
                            "description": "GPU device to detach"
                        }
                    },
                    "required": ["vm_id", "device_id"]
                }),
                category: ToolCategory::GpuFabric,
                required_capabilities: vec![AgentCapability::VmWrite],
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
                        "vm_id": {
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
                    "required": ["vm_id", "command"]
                }),
                category: ToolCategory::System,
                required_capabilities: vec![AgentCapability::GuestExec],
                enabled: true,
            },
        );

        tools.insert(
            "guest.exec.status".to_string(),
            McpTool {
                name: "guest.exec.status".to_string(),
                description:
                    "Poll the result of a submitted guest request (guest.exec / guest.file.*)"
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "request_id": {
                            "type": "string",
                            "description": "Request ID returned by the submitting call"
                        }
                    },
                    "required": ["request_id"]
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
                        "vm_id": {
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
                    "required": ["vm_id", "path"]
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
                        "vm_id": {
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
                            "description": "Content encoding",
                            "enum": ["text", "base64"],
                            "default": "text"
                        }
                    },
                    "required": ["vm_id", "path", "content"]
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
                            "description": "Message priority",
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to claim"
                        },
                        "duration_seconds": {
                            "type": "integer",
                            "description": "Claim duration",
                            "default": 300
                        }
                    },
                    "required": ["vm_id"]
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
                        "vm_id": {
                            "type": "string",
                            "description": "Name of the VM to release"
                        }
                    },
                    "required": ["vm_id"]
                }),
                category: ToolCategory::Coordination,
                required_capabilities: vec![AgentCapability::Coordination],
                enabled: true,
            },
        );

        // Image allowlist. Read-only on purpose: an agent may ask what it is
        // permitted to boot, but approving an image is a human review step and
        // is not reachable from here.
        tools.insert(
            "image.list".to_string(),
            McpTool {
                name: "image.list".to_string(),
                description: "List images in the fleet allowlist with their approval status. Requires an image host; without one the registry is not visible to agents.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
                category: ToolCategory::Storage,
                required_capabilities: vec![],
                enabled: true,
            },
        );

        tools.insert(
            "image.get".to_string(),
            McpTool {
                name: "image.get".to_string(),
                description: "Look up one image in the allowlist by its registry reference.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {"reference": {"type": "string", "description": "Registry reference, e.g. registry.internal/kernels/ubuntu:6.8"}},
                    "required": ["reference"]
                }),
                category: ToolCategory::Storage,
                required_capabilities: vec![],
                enabled: true,
            },
        );

        tools.insert(
            "image.check".to_string(),
            McpTool {
                name: "image.check".to_string(),
                description: "Ask whether an image would be admitted, by SHA-256 digest. This is the question VM::provision asks, so a VM whose image fails this check will refuse to start.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {"sha256": {"type": "string", "description": "Lower-case hex SHA-256 of the image bytes"}},
                    "required": ["sha256"]
                }),
                category: ToolCategory::Storage,
                required_capabilities: vec![],
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
        if self.session_count() >= self.config.max_sessions {
            // At capacity — reclaim idle sessions before rejecting, so a
            // long-running runtime doesn't deadlock on leaked sessions.
            self.expire_idle_sessions(self.config.session_idle_timeout);
            if self.session_count() >= self.config.max_sessions {
                return Err("Maximum sessions reached".to_string());
            }
        }

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

    /// Number of live agent sessions.
    pub fn session_count(&self) -> usize {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Register a hook invoked whenever a session is closed or reclaimed, so the
    /// runtime can tear down the VMs/resources the session owned. Replaces any
    /// previously-registered hook.
    pub fn on_session_teardown<F>(&self, hook: F)
    where
        F: Fn(&SessionTeardown) + Send + Sync + 'static,
    {
        *self
            .teardown_hook
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(hook));
    }

    /// Run teardown for a reclaimed session: record it in the audit log and
    /// invoke the teardown hook with the session's owned VMs. The caller must
    /// not hold the `sessions` lock (the hook may be slow or re-enter).
    fn run_teardown(&self, session: &AgentSession) {
        let owned_vms = session
            .owned_vms
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        if self.config.audit_enabled {
            let entry = AuditEntry {
                timestamp: SystemTime::now(),
                session_id: session.id.clone(),
                agent_id: session.agent_id.clone(),
                tool: "session.teardown".to_string(),
                parameters: json!({ "owned_vms": owned_vms }),
                success: true,
                error: None,
                execution_time_ms: 0,
            };
            let mut log = self.audit_log.write().unwrap_or_else(|e| e.into_inner());
            log.push_back(entry);
            while log.len() > self.config.max_audit_entries {
                log.pop_front();
            }
        }

        let hook = self
            .teardown_hook
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(&SessionTeardown {
                session_id: session.id.clone(),
                agent_id: session.agent_id.clone(),
                owned_vms,
            });
        }
    }

    /// Explicitly close (remove) a session, running teardown. Returns `true` if
    /// it existed.
    pub fn close_session(&self, session_id: &str) -> bool {
        let removed = self
            .sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        match removed {
            Some(session) => {
                self.run_teardown(&session);
                true
            }
            None => false,
        }
    }

    /// Reap sessions idle longer than `max_idle`, running teardown for each, and
    /// return the number removed. A long-running agent runtime can call this
    /// periodically; `create_session` also calls it automatically at capacity.
    pub fn expire_idle_sessions(&self, max_idle: Duration) -> usize {
        let now = Instant::now();
        let removed: Vec<Arc<AgentSession>> = {
            let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let expired: Vec<String> = sessions
                .iter()
                .filter(|(_, s)| {
                    let last = *s.last_activity.read().unwrap_or_else(|e| e.into_inner());
                    now.duration_since(last) >= max_idle
                })
                .map(|(k, _)| k.clone())
                .collect();
            expired.iter().filter_map(|k| sessions.remove(k)).collect()
        };
        for session in &removed {
            self.run_teardown(session);
        }
        removed.len()
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

        // Verify the tool exists and the session holds its required
        // capabilities under a short read lock — without cloning the tool's
        // (potentially large) JSON schema on every call. `request.tool` is the
        // tool name, so dispatch needs no clone. The lock is released before the
        // `.await` below.
        {
            let tools = self.tools.read().unwrap_or_else(|e| e.into_inner());
            let tool = match tools.get(&request.tool) {
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
        }

        // Hold a concurrency permit for the duration of the call, if a ceiling
        // is installed. The guard releases on drop, so an early return below
        // frees the slot too. Rejection flows through the same result path as a
        // policy denial so that it is audited rather than silently dropped.
        let limiter = self.concurrency_limiter();
        let _permit = match limiter.as_deref().map(|l| l.try_acquire()) {
            Some(Ok(permit)) => Some(permit),
            Some(Err(err)) => {
                let response = ToolCallResponse {
                    id: request.id.clone(),
                    success: false,
                    result: None,
                    error: Some(err.to_string()),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                };
                self.audit(&request.tool, &request.parameters, session, &response);
                return response;
            }
            None => None,
        };

        // Governance, when an operator installed a policy set. A denial flows
        // through the same result path as a failed call rather than returning
        // early, so it lands in the audit log — an unrecorded denial is the one
        // an incident review most needs to see.
        let denial = self.policy_set().and_then(|policies| {
            let (action, resource) = Self::policy_request(&request.tool, &request.parameters);
            match policies.evaluate(&action, &resource, &Self::policy_context(session)) {
                crate::policies::PolicyEffect::Allow => None,
                crate::policies::PolicyEffect::Deny => Some(format!(
                    "Denied by policy '{}': {:?} on {}:{}",
                    policies.name, action, resource.resource_type, resource.identifier
                )),
            }
        });

        // Execute tool via the dispatch table in execute_tool_impl
        let result = match denial {
            Some(error) => Err(error),
            None => {
                self.execute_tool_impl(&request.tool, &request.parameters, session)
                    .await
            }
        };

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

        self.audit(&request.tool, &request.parameters, session, &response);

        response
    }

    /// Record one tool call in the audit log.
    ///
    /// Shared by the normal path and by every early refusal, so a call that
    /// was rejected before dispatch is recorded exactly like one that ran. A
    /// refusal nobody can see afterwards is the one an incident review needs.
    fn audit(
        &self,
        tool: &str,
        parameters: &JsonValue,
        session: &AgentSession,
        response: &ToolCallResponse,
    ) {
        if !self.config.audit_enabled {
            return;
        }

        let entry = AuditEntry {
            timestamp: SystemTime::now(),
            session_id: session.id.clone(),
            agent_id: session.agent_id.clone(),
            tool: tool.to_string(),
            parameters: parameters.clone(),
            success: response.success,
            error: response.error.clone(),
            execution_time_ms: response.execution_time_ms,
        };

        let mut log = self.audit_log.write().unwrap_or_else(|e| e.into_inner());
        log.push_back(entry);
        // Bound memory: drop the oldest entries beyond the configured cap. A
        // fixed-size ring also avoids reallocation under the lock at steady
        // state, shortening the hold time on this shared path.
        while log.len() > self.config.max_audit_entries {
            log.pop_front();
        }
    }

    /// Run a VM lifecycle tool against an installed [`VmHost`](crate::vm_host::VmHost).
    ///
    /// Returns `None` when there is no host, or when the tool has no host
    /// equivalent — in both cases the caller falls through to the server's own
    /// session-state handling.
    async fn dispatch_to_vm_host(
        &self,
        tool_name: &str,
        params: &JsonValue,
        session: &AgentSession,
    ) -> Option<Result<JsonValue, String>> {
        let host = self.vm_host()?;

        if !matches!(
            tool_name,
            "vm.create"
                | "vm.start"
                | "vm.stop"
                | "vm.pause"
                | "vm.resume"
                | "vm.delete"
                | "vm.status"
                | "vm.list"
                | "vm.metrics"
        ) {
            return None;
        }

        Some(
            self.run_vm_host_tool(host.as_ref(), tool_name, params, session)
                .await,
        )
    }

    /// The body of [`Self::dispatch_to_vm_host`], once a host is known to apply.
    async fn run_vm_host_tool(
        &self,
        host: &dyn crate::vm_host::VmHost,
        tool_name: &str,
        params: &JsonValue,
        session: &AgentSession,
    ) -> Result<JsonValue, String> {
        use crate::vm_host::VmSpec;

        /// Pull `vm_id` out and confirm this session created that VM.
        fn owned_vm_id<'a>(
            params: &'a JsonValue,
            session: &AgentSession,
        ) -> Result<&'a str, String> {
            let vm_id = params
                .get("vm_id")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: vm_id")?;
            if !McpServer::session_owns(session, vm_id) {
                // Same message as a genuinely absent VM: a session should not
                // be able to probe for the existence of another's VMs.
                return Err(format!("VM not found: {vm_id}"));
            }
            Ok(vm_id)
        }

        /// Keep the session's mirror of a VM in step with the host's view, so
        /// tools that read session state (`vm.metrics`, snapshots) still work.
        fn mirror(session: &AgentSession, descriptor: &crate::vm_host::VmDescriptor) -> JsonValue {
            let value = serde_json::to_value(descriptor).unwrap_or(JsonValue::Null);
            session
                .state
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(format!("vm:{}", descriptor.vm_id), value.clone());
            value
        }

        match tool_name {
            "vm.create" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: name")?;

                let mut spec = VmSpec::new(name);
                if let Some(cores) = params.get("cpu_cores").and_then(|v| v.as_u64()) {
                    spec.cpu_cores = cores as u32;
                }
                if let Some(memory) = params.get("memory_gb").and_then(|v| v.as_u64()) {
                    spec.memory_gb = memory;
                }
                if let Some(boot) = params.get("boot") {
                    spec.boot = serde_json::from_value(boot.clone())
                        .map_err(|e| format!("Invalid boot source: {e}"))?;
                }

                let descriptor = host.create(spec).await?;
                session
                    .owned_vms
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(descriptor.vm_id.clone());
                Ok(mirror(session, &descriptor))
            }

            "vm.start" => {
                let vm_id = owned_vm_id(params, session)?;
                let descriptor = host.start(vm_id).await?;
                Ok(mirror(session, &descriptor))
            }

            "vm.stop" => {
                let vm_id = owned_vm_id(params, session)?;
                let force = params
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let descriptor = host.stop(vm_id, force).await?;
                let mut value = mirror(session, &descriptor);
                value["force"] = json!(force);
                Ok(value)
            }

            "vm.pause" => {
                let vm_id = owned_vm_id(params, session)?;
                let descriptor = host.pause(vm_id).await?;
                Ok(mirror(session, &descriptor))
            }

            "vm.resume" => {
                let vm_id = owned_vm_id(params, session)?;
                let descriptor = host.resume(vm_id).await?;
                Ok(mirror(session, &descriptor))
            }

            "vm.delete" => {
                let vm_id = owned_vm_id(params, session)?;
                host.delete(vm_id).await?;
                session
                    .state
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&format!("vm:{vm_id}"));
                session
                    .owned_vms
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|id| id != vm_id);
                Ok(json!({ "vm_id": vm_id, "status": "deleted" }))
            }

            "vm.status" => {
                let vm_id = owned_vm_id(params, session)?;
                let descriptor = host.status(vm_id).await?;
                Ok(mirror(session, &descriptor))
            }

            "vm.list" => {
                // The host may be shared, so report only this session's VMs.
                let owned = session
                    .owned_vms
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let vms: Vec<JsonValue> = host
                    .list()
                    .await?
                    .into_iter()
                    .filter(|v| owned.contains(&v.vm_id))
                    .map(|v| serde_json::to_value(v).unwrap_or(JsonValue::Null))
                    .collect();
                let total = vms.len();
                Ok(json!({ "vms": vms, "total": total }))
            }

            "vm.metrics" => {
                let vm_id = owned_vm_id(params, session)?;
                let metrics = host.metrics(vm_id).await?;
                serde_json::to_value(metrics).map_err(|e| e.to_string())
            }

            other => Err(format!("Tool has no VM-host implementation: {other}")),
        }
    }

    /// Answer an `image.*` tool from an installed
    /// [`ImageHost`](crate::image_host::ImageHost).
    ///
    /// Takes no session: the allowlist is fleet-wide, not per-agent, and there
    /// is nothing here an agent could own. Returns `None` for tools this does
    /// not handle, so the caller falls through.
    async fn dispatch_to_image_host(
        &self,
        tool_name: &str,
        params: &JsonValue,
    ) -> Option<Result<JsonValue, String>> {
        if !matches!(tool_name, "image.list" | "image.get" | "image.check") {
            return None;
        }

        // Say plainly that nothing is tracking images, rather than returning an
        // empty list that reads as "nothing is approved".
        let Some(host) = self.image_host() else {
            return Some(Err(
                "No image registry is installed; this server does not track image admission"
                    .to_string(),
            ));
        };

        fn required<'a>(params: &'a JsonValue, key: &str) -> Result<&'a str, String> {
            params
                .get(key)
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("Missing required parameter: {key}"))
        }

        Some(match tool_name {
            "image.list" => host.list().await.and_then(|images| {
                let total = images.len();
                serde_json::to_value(images)
                    .map(|images| json!({ "images": images, "total": total }))
                    .map_err(|e| e.to_string())
            }),
            "image.get" => match required(params, "reference") {
                Ok(reference) => host
                    .get(reference)
                    .await
                    .and_then(|image| serde_json::to_value(image).map_err(|e| e.to_string())),
                Err(e) => Err(e),
            },
            "image.check" => match required(params, "sha256") {
                Ok(digest) => host
                    .check_digest(digest)
                    .await
                    .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
                Err(e) => Err(e),
            },
            other => Err(format!("Tool has no image-host implementation: {other}")),
        })
    }

    /// Run a GPU tool against an installed [`GpuHost`](crate::gpu_host::GpuHost).
    ///
    /// Returns `None` when there is no host or the tool has no host equivalent,
    /// in which case the caller falls through to session state.
    async fn dispatch_to_gpu_host(
        &self,
        tool_name: &str,
        params: &JsonValue,
        session: &AgentSession,
    ) -> Option<Result<JsonValue, String>> {
        let host = self.gpu_host()?;

        if !matches!(
            tool_name,
            "gpu.register" | "gpu.list" | "gpu.attach" | "gpu.detach"
        ) {
            return None;
        }

        Some(
            self.run_gpu_host_tool(host.as_ref(), tool_name, params, session)
                .await,
        )
    }

    /// The body of [`Self::dispatch_to_gpu_host`], once a host is known to apply.
    async fn run_gpu_host_tool(
        &self,
        host: &dyn crate::gpu_host::GpuHost,
        tool_name: &str,
        params: &JsonValue,
        session: &AgentSession,
    ) -> Result<JsonValue, String> {
        use crate::gpu_host::GpuDescriptor;

        fn device_id(params: &JsonValue) -> Result<&str, String> {
            params
                .get("device_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter: device_id".to_string())
        }

        /// A GPU may only be attached to a VM the calling session owns.
        fn owned_vm_id<'a>(
            params: &'a JsonValue,
            session: &AgentSession,
        ) -> Result<&'a str, String> {
            let vm_id = params
                .get("vm_id")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: vm_id")?;
            if !McpServer::session_owns(session, vm_id) {
                return Err(format!("VM not found: {vm_id}"));
            }
            Ok(vm_id)
        }

        /// Mirror an attachment onto the session's VM record, so tools that
        /// read session state still see the VM's GPUs.
        fn mirror_attachment(session: &AgentSession, vm_id: &str, device_id: &str, attached: bool) {
            let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
            let Some(vm) = state.get_mut(&format!("vm:{vm_id}")) else {
                return;
            };
            if !vm.get("gpus").map(|g| g.is_array()).unwrap_or(false) {
                vm["gpus"] = json!([]);
            }
            let gpus = vm["gpus"].as_array_mut().expect("gpus is an array");
            if attached {
                if !gpus.iter().any(|d| d.as_str() == Some(device_id)) {
                    gpus.push(json!(device_id));
                }
            } else {
                gpus.retain(|d| d.as_str() != Some(device_id));
            }
            let any = !gpus.is_empty();
            vm["gpu_enabled"] = json!(any);
        }

        match tool_name {
            "gpu.register" => {
                let device = GpuDescriptor {
                    device_id: device_id(params)?.to_string(),
                    model: params
                        .get("model")
                        .and_then(|v| v.as_str())
                        .ok_or("Missing required parameter: model")?
                        .to_string(),
                    vram_gb: params.get("vram_gb").and_then(|v| v.as_u64()).unwrap_or(80),
                    compute_capability: params
                        .get("compute_capability")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(90) as u32,
                    allocated_to: None,
                };
                let registered = host.register(device).await?;
                Ok(serde_json::to_value(registered).unwrap_or(JsonValue::Null))
            }

            "gpu.list" => {
                let only_available = params
                    .get("only_available")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let all = host.list().await?;
                let available = all.iter().filter(|d| d.is_available()).count();
                let devices: Vec<JsonValue> = all
                    .iter()
                    .filter(|d| !only_available || d.is_available())
                    .map(|d| serde_json::to_value(d).unwrap_or(JsonValue::Null))
                    .collect();

                Ok(json!({
                    "devices": devices,
                    "total": devices.len(),
                    "available": available,
                }))
            }

            "gpu.attach" => {
                let vm_id = owned_vm_id(params, session)?;
                let device_id = device_id(params)?;
                host.attach(vm_id, device_id).await?;
                mirror_attachment(session, vm_id, device_id, true);
                Ok(json!({ "vm_id": vm_id, "device_id": device_id, "status": "attached" }))
            }

            "gpu.detach" => {
                let vm_id = owned_vm_id(params, session)?;
                let device_id = device_id(params)?;
                host.detach(vm_id, device_id).await?;
                mirror_attachment(session, vm_id, device_id, false);
                Ok(json!({ "vm_id": vm_id, "device_id": device_id, "status": "detached" }))
            }

            other => Err(format!("Tool has no GPU-host implementation: {other}")),
        }
    }

    /// Internal tool execution dispatch
    async fn execute_tool_impl(
        &self,
        tool_name: &str,
        params: &JsonValue,
        session: &AgentSession,
    ) -> Result<JsonValue, String> {
        // A real hypervisor or GPU fabric, when installed, takes the lifecycle
        // tools for its resource. Everything else — and every tool when neither
        // is installed — falls through to the server's own session-state
        // implementation below.
        if let Some(result) = self.dispatch_to_vm_host(tool_name, params, session).await {
            return result;
        }
        if let Some(result) = self.dispatch_to_image_host(tool_name, params).await {
            return result;
        }
        if let Some(result) = self.dispatch_to_gpu_host(tool_name, params, session).await {
            return result;
        }

        match tool_name {
            // ── VM lifecycle ──────────────────────────────────────────
            "vm.create" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: name")?;
                let cpu_cores = params
                    .get("cpu_cores")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(2);
                let memory_gb = params
                    .get("memory_gb")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(4);
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
                    "cpu_cores": cpu_cores,
                    "memory_gb": memory_gb,
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
                    json!({ "vm_id": vm_id, "status": "created", "name": name, "cpu_cores": cpu_cores, "memory_gb": memory_gb }),
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

            "vm.pause" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                let vm = state
                    .get_mut(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;
                if vm["status"] != json!("running") {
                    return Err(format!(
                        "VM {} cannot be paused (status: {})",
                        vm_id, vm["status"]
                    ));
                }
                vm["status"] = json!("paused");
                Ok(json!({ "vm_id": vm_id, "status": "paused" }))
            }

            "vm.resume" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let key = format!("vm:{}", vm_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                let vm = state
                    .get_mut(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;
                if vm["status"] != json!("paused") {
                    return Err(format!(
                        "VM {} cannot be resumed (status: {})",
                        vm_id, vm["status"]
                    ));
                }
                vm["status"] = json!("running");
                Ok(json!({ "vm_id": vm_id, "status": "running" }))
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
                if let Some(cpu_cores) = params.get("cpu_cores").and_then(|v| v.as_u64()) {
                    vm["cpu_cores"] = json!(cpu_cores);
                }
                if let Some(memory_gb) = params.get("memory_gb").and_then(|v| v.as_u64()) {
                    vm["memory_gb"] = json!(memory_gb);
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
                let record = state
                    .get(&key)
                    .ok_or_else(|| format!("VM not found: {}", vm_id))?;

                // With no host installed there is nothing to measure. Null is
                // the honest answer; the zeros this used to return were
                // indistinguishable from a genuinely idle guest.
                Ok(json!({
                    "vm_id": vm_id,
                    "status": record.get("status").cloned().unwrap_or(JsonValue::Null),
                    "vcpu_count": record.get("cpu_cores").cloned().unwrap_or(JsonValue::Null),
                    "memory_total_bytes": record
                        .get("memory_gb")
                        .and_then(|v| v.as_u64())
                        .map(|gb| json!(gb * 1024 * 1024 * 1024))
                        .unwrap_or(JsonValue::Null),
                    "uptime_seconds": JsonValue::Null,
                    "cpu_usage_percent": JsonValue::Null,
                    "memory_used_bytes": JsonValue::Null
                }))
            }

            // ── Snapshots ────────────────────────────────────────────
            "snapshot.create" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let snapshot_name = params
                    .get("snapshot_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("snapshot");
                let snapshot_id = uuid_v4();
                let snap = json!({
                    "snapshot_id": snapshot_id,
                    "vm_id": vm_id,
                    "name": snapshot_name,
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

            // ── GPU fabric ───────────────────────────────────────────
            "gpu.register" => {
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: device_id")?;
                let model = params
                    .get("model")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: model")?;
                let vram_gb = params.get("vram_gb").and_then(|v| v.as_u64()).unwrap_or(80);
                let compute_capability = params
                    .get("compute_capability")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(90);
                let key = format!("gpu:{}", device_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                if state.contains_key(&key) {
                    return Err(format!("GPU device already registered: {}", device_id));
                }
                let device = json!({
                    "device_id": device_id,
                    "model": model,
                    "vram_gb": vram_gb,
                    "compute_capability": compute_capability,
                    "allocated_to": JsonValue::Null,
                });
                state.insert(key, device.clone());
                Ok(device)
            }

            "gpu.list" => {
                let only_available = params
                    .get("only_available")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());
                let devices: Vec<&JsonValue> = state
                    .iter()
                    .filter(|(k, _)| k.starts_with("gpu:"))
                    .map(|(_, v)| v)
                    .filter(|d| !only_available || d["allocated_to"].is_null())
                    .collect();
                let available = devices
                    .iter()
                    .filter(|d| d["allocated_to"].is_null())
                    .count();
                Ok(json!({
                    "devices": devices,
                    "total": devices.len(),
                    "available": available,
                }))
            }

            "gpu.attach" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: device_id")?;
                let vm_key = format!("vm:{}", vm_id);
                let gpu_key = format!("gpu:{}", device_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                if !state.contains_key(&vm_key) {
                    return Err(format!("VM not found: {}", vm_id));
                }
                // Claim the device (rejecting a double-allocation), then record
                // it on the VM. Two sequential get_mut borrows, never aliased.
                {
                    let dev = state
                        .get_mut(&gpu_key)
                        .ok_or_else(|| format!("GPU device not found: {}", device_id))?;
                    if !dev["allocated_to"].is_null() {
                        return Err(format!(
                            "GPU {} already allocated to {}",
                            device_id, dev["allocated_to"]
                        ));
                    }
                    dev["allocated_to"] = json!(vm_id);
                }
                let vm = state.get_mut(&vm_key).expect("vm presence checked above");
                if !vm.get("gpus").map(|g| g.is_array()).unwrap_or(false) {
                    vm["gpus"] = json!([]);
                }
                vm["gpus"]
                    .as_array_mut()
                    .expect("gpus is an array")
                    .push(json!(device_id));
                vm["gpu_enabled"] = json!(true);
                Ok(json!({ "vm_id": vm_id, "device_id": device_id, "status": "attached" }))
            }

            "gpu.detach" => {
                let vm_id = params
                    .get("vm_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: vm_id")?;
                let device_id = params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: device_id")?;
                let vm_key = format!("vm:{}", vm_id);
                let gpu_key = format!("gpu:{}", device_id);
                let mut state = session.state.write().unwrap_or_else(|e| e.into_inner());
                {
                    let dev = state
                        .get_mut(&gpu_key)
                        .ok_or_else(|| format!("GPU device not found: {}", device_id))?;
                    if dev["allocated_to"].as_str() != Some(vm_id) {
                        return Err(format!("GPU {} is not attached to VM {}", device_id, vm_id));
                    }
                    dev["allocated_to"] = JsonValue::Null;
                }
                if let Some(vm) = state.get_mut(&vm_key) {
                    if let Some(gpus) = vm["gpus"].as_array_mut() {
                        gpus.retain(|d| d.as_str() != Some(device_id));
                        if gpus.is_empty() {
                            vm["gpu_enabled"] = json!(false);
                        }
                    }
                }
                Ok(json!({ "vm_id": vm_id, "device_id": device_id, "status": "detached" }))
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

            "guest.exec.status" => {
                let request_id = params
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: request_id")?;
                let state = session.state.read().unwrap_or_else(|e| e.into_inner());

                // A delivered response (from the guest agent channel) wins.
                if let Some(resp) = state.get(&format!("guest_resp:{}", request_id)) {
                    let mut out = resp.clone();
                    out["request_id"] = json!(request_id);
                    out["status"] = json!("completed");
                    return Ok(out);
                }
                // Otherwise the request is still pending (or unknown).
                if state.contains_key(&format!("guest_req:{}", request_id)) {
                    Ok(json!({ "request_id": request_id, "status": "pending" }))
                } else {
                    Err(format!("Unknown request_id: {}", request_id))
                }
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

    /// Deliver a guest-agent response for a previously submitted request.
    ///
    /// In production this is called by the guest-agent channel transport when
    /// the in-guest agent returns a result; an agent then retrieves it via the
    /// `guest.exec.status` tool. Tests and examples call this directly to
    /// complete a `guest.exec` / `guest.file.*` round-trip. The `response`
    /// object is returned verbatim by `guest.exec.status` (with `status` set to
    /// `"completed"`), so it typically carries `exit_code` / `stdout` / `stderr`.
    pub fn deliver_guest_response(&self, request_id: &str, response: JsonValue) {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        state.insert(format!("guest_resp:{}", request_id), response);
        if let Some(req) = state.get_mut(&format!("guest_req:{}", request_id)) {
            req["status"] = json!("completed");
        }
    }
}

/// Generate a UUID v4 string
pub(crate) fn uuid_v4() -> String {
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

    #[tokio::test]
    async fn vm_pause_resume_lifecycle() {
        let server = McpServer::new();
        let session = server
            .create_session("lifecycle-agent", AgentCapabilities::full())
            .unwrap();

        let created = session
            .call_tool(&server, "vm.create", json!({ "name": "pausable" }))
            .await;
        assert!(created.success, "create failed: {:?}", created.error);
        let vm_id = created.result.unwrap()["vm_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Pausing a non-running VM is rejected.
        let bad = session
            .call_tool(&server, "vm.pause", json!({ "vm_id": vm_id.clone() }))
            .await;
        assert!(!bad.success, "pause should reject a non-running VM");

        // running -> paused -> running
        session
            .call_tool(&server, "vm.start", json!({ "vm_id": vm_id.clone() }))
            .await;
        let paused = session
            .call_tool(&server, "vm.pause", json!({ "vm_id": vm_id.clone() }))
            .await;
        assert!(paused.success, "pause failed: {:?}", paused.error);
        assert_eq!(paused.result.unwrap()["status"], "paused");

        // Resuming is only valid from paused; a second resume must fail.
        let resumed = session
            .call_tool(&server, "vm.resume", json!({ "vm_id": vm_id.clone() }))
            .await;
        assert!(resumed.success, "resume failed: {:?}", resumed.error);
        assert_eq!(resumed.result.unwrap()["status"], "running");
        let resume_again = session
            .call_tool(&server, "vm.resume", json!({ "vm_id": vm_id.clone() }))
            .await;
        assert!(!resume_again.success, "resume should reject a running VM");
    }

    #[tokio::test]
    async fn gpu_attach_detach_lifecycle() {
        let server = McpServer::new();
        let session = server
            .create_session("gpu-agent", AgentCapabilities::full())
            .unwrap();

        // Register two GPUs into the fabric inventory.
        for dev in ["gpu-0", "gpu-1"] {
            let r = session
                .call_tool(
                    &server,
                    "gpu.register",
                    json!({ "device_id": dev, "model": "H100", "vram_gb": 80 }),
                )
                .await;
            assert!(r.success, "register {dev} failed: {:?}", r.error);
        }
        // Duplicate registration is rejected.
        let dup = session
            .call_tool(
                &server,
                "gpu.register",
                json!({ "device_id": "gpu-0", "model": "H100" }),
            )
            .await;
        assert!(!dup.success, "duplicate register should fail");

        let listed = session.call_tool(&server, "gpu.list", json!({})).await;
        assert_eq!(listed.result.unwrap()["available"], 2);

        // A VM to attach to.
        let created = session
            .call_tool(&server, "vm.create", json!({ "name": "trainer" }))
            .await;
        let vm_id = created.result.unwrap()["vm_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Attach gpu-0; availability drops to 1.
        let attached = session
            .call_tool(
                &server,
                "gpu.attach",
                json!({ "vm_id": vm_id.clone(), "device_id": "gpu-0" }),
            )
            .await;
        assert!(attached.success, "attach failed: {:?}", attached.error);
        let avail = session
            .call_tool(&server, "gpu.list", json!({ "only_available": true }))
            .await;
        assert_eq!(avail.result.unwrap()["available"], 1);

        // Double-allocation and unknown-VM attach are both rejected.
        let dbl = session
            .call_tool(
                &server,
                "gpu.attach",
                json!({ "vm_id": vm_id.clone(), "device_id": "gpu-0" }),
            )
            .await;
        assert!(!dbl.success, "attaching an allocated GPU should fail");
        let no_vm = session
            .call_tool(
                &server,
                "gpu.attach",
                json!({ "vm_id": "ghost", "device_id": "gpu-1" }),
            )
            .await;
        assert!(!no_vm.success, "attach to unknown VM should fail");

        // Detach returns the device to the pool; a second detach fails.
        let detached = session
            .call_tool(
                &server,
                "gpu.detach",
                json!({ "vm_id": vm_id.clone(), "device_id": "gpu-0" }),
            )
            .await;
        assert!(detached.success, "detach failed: {:?}", detached.error);
        let detach_again = session
            .call_tool(
                &server,
                "gpu.detach",
                json!({ "vm_id": vm_id.clone(), "device_id": "gpu-0" }),
            )
            .await;
        assert!(!detach_again.success, "detaching a free GPU should fail");

        let final_list = session.call_tool(&server, "gpu.list", json!({})).await;
        assert_eq!(final_list.result.unwrap()["available"], 2);
    }

    #[tokio::test]
    async fn gpu_register_requires_admin() {
        // Operators can attach/list but not register fabric hardware.
        let server = McpServer::new();
        let session = server
            .create_session("op-agent", AgentCapabilities::operator())
            .unwrap();
        let r = session
            .call_tool(
                &server,
                "gpu.register",
                json!({ "device_id": "gpu-9", "model": "H100" }),
            )
            .await;
        assert!(!r.success, "operator must not register GPU hardware");
    }

    #[tokio::test]
    async fn audit_log_is_bounded() {
        // A long-running agent runtime must not grow the audit log without
        // bound; the oldest entries are dropped beyond `max_audit_entries`.
        let server = McpServer::with_config(McpConfig {
            audit_enabled: true,
            max_audit_entries: 5,
            rate_limit: u32::MAX,
            ..Default::default()
        });
        let session = server
            .create_session("audit-agent", AgentCapabilities::full())
            .unwrap();

        for _ in 0..20 {
            let _ = session.call_tool(&server, "system.info", json!({})).await;
        }
        assert_eq!(server.get_audit_log(100).len(), 5);
    }

    #[tokio::test]
    async fn close_session_removes_it() {
        let server = McpServer::new();
        let s = server
            .create_session("a", AgentCapabilities::full())
            .unwrap();
        assert_eq!(server.session_count(), 1);
        assert!(server.close_session(&s.id));
        assert_eq!(server.session_count(), 0);
        assert!(!server.close_session("nope"));
    }

    #[tokio::test]
    async fn expire_idle_sessions_reaps_idle() {
        let server = McpServer::new();
        let _a = server
            .create_session("a", AgentCapabilities::full())
            .unwrap();
        let _b = server
            .create_session("b", AgentCapabilities::full())
            .unwrap();
        assert_eq!(server.session_count(), 2);
        // max_idle = 0 makes every session eligible for reclamation.
        assert_eq!(server.expire_idle_sessions(Duration::from_secs(0)), 2);
        assert_eq!(server.session_count(), 0);
    }

    #[tokio::test]
    async fn create_session_reclaims_at_capacity() {
        // With a zero idle timeout, an at-capacity create reclaims the idle
        // session instead of rejecting the new agent.
        let server = McpServer::with_config(McpConfig {
            max_sessions: 1,
            session_idle_timeout: Duration::from_secs(0),
            ..Default::default()
        });
        let _first = server
            .create_session("a", AgentCapabilities::full())
            .unwrap();
        assert_eq!(server.session_count(), 1);
        let second = server.create_session("b", AgentCapabilities::full());
        assert!(second.is_ok());
        assert_eq!(server.session_count(), 1);
    }

    #[tokio::test]
    async fn teardown_hook_fires_with_owned_vms() {
        let server = McpServer::new();
        let torn_down: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
        let captured = Arc::clone(&torn_down);
        server.on_session_teardown(move |t| {
            captured.lock().unwrap().extend(t.owned_vms.iter().cloned());
        });

        let session = server
            .create_session("a", AgentCapabilities::full())
            .unwrap();
        // The session creates (and thus owns) a VM.
        let resp = session
            .call_tool(&server, "vm.create", json!({ "name": "x" }))
            .await;
        assert!(resp.success);

        // Closing the session must invoke the hook with the owned VM.
        assert!(server.close_session(&session.id));
        assert_eq!(torn_down.lock().unwrap().len(), 1);
    }
}
