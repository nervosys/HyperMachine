//! Agentic Ontology Module
//!
//! Provides machine-readable API discovery for AI agents.
//! Supports multiple formats: OpenAPI, JSON-LD, and tool schemas
//! for OpenAI, Anthropic, and Google AI integrations.

use axum::{
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Core Ontology Types
// ============================================================================

/// Complete HyperMachine capability ontology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperMachineOntology {
    /// Ontology metadata
    #[serde(rename = "@context")]
    pub context: OntologyContext,

    /// System identification
    pub system: SystemInfo,

    /// Available capabilities
    pub capabilities: Vec<Capability>,

    /// Resource types
    pub resources: Vec<ResourceType>,

    /// Available operations
    pub operations: Vec<Operation>,

    /// State machine definitions
    pub state_machines: Vec<StateMachine>,

    /// Event types for subscriptions
    pub events: Vec<EventType>,
}

/// JSON-LD context for semantic web compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyContext {
    #[serde(rename = "@vocab")]
    pub vocab: String,
    pub schema: String,
    pub hm: String,
    pub dcterms: String,
}

impl Default for OntologyContext {
    fn default() -> Self {
        Self {
            vocab: "https://schema.nervosys.ai/hypermachine#".to_string(),
            schema: "https://schema.org/".to_string(),
            hm: "https://nervosys.ai/hypermachine/ontology/".to_string(),
            dcterms: "http://purl.org/dc/terms/".to_string(),
        }
    }
}

/// System information for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub documentation_url: String,
    pub api_base_url: String,
    pub supported_protocols: Vec<String>,
    pub authentication: AuthenticationInfo,
}

/// Authentication requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationInfo {
    pub required: bool,
    pub methods: Vec<AuthMethod>,
    pub token_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethod {
    pub method_type: String,
    pub description: String,
    pub header_name: Option<String>,
}

/// A capability that HyperMachine provides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: CapabilityCategory,
    pub operations: Vec<String>,
    pub prerequisites: Vec<String>,
    pub permissions_required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    VirtualMachine,
    Compute,
    Storage,
    Network,
    Security,
    Monitoring,
    AgentExecution,
}

/// A resource type that can be managed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceType {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub lifecycle_states: Vec<String>,
    pub relationships: Vec<ResourceRelationship>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRelationship {
    pub name: String,
    pub target_type: String,
    pub cardinality: String,
    pub description: String,
}

/// An operation that can be performed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub name: String,
    pub description: String,
    pub http_method: String,
    pub path: String,
    pub parameters: Vec<Parameter>,
    pub request_body: Option<RequestBody>,
    pub responses: Vec<OperationResponse>,
    pub idempotent: bool,
    pub async_operation: bool,
    pub rate_limit: Option<RateLimit>,
    pub examples: Vec<OperationExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub location: ParameterLocation,
    pub required: bool,
    pub description: String,
    pub schema: serde_json::Value,
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Body,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub content_type: String,
    pub schema: serde_json::Value,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    pub status_code: u16,
    pub description: String,
    pub schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationExample {
    pub name: String,
    pub description: String,
    pub request: Option<serde_json::Value>,
    pub response: serde_json::Value,
}

/// State machine definition for resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    pub resource_type: String,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub initial_state: String,
    pub terminal_states: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub name: String,
    pub description: String,
    pub allowed_operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from_state: String,
    pub to_state: String,
    pub trigger_operation: String,
    pub conditions: Vec<String>,
}

/// Event type for subscriptions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventType {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub resource_types: Vec<String>,
}

// ============================================================================
// AI Agent Tool Formats
// ============================================================================

/// OpenAI function calling format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITools {
    pub tools: Vec<OpenAITool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Anthropic MCP tool format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTools {
    pub name: String,
    pub version: String,
    pub tools: Vec<AnthropicTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Google Gemini function declarations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTools {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GeminiFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ============================================================================
// Ontology Builder
// ============================================================================

impl HyperMachineOntology {
    /// Build the complete HyperMachine ontology
    pub fn build() -> Self {
        Self {
            context: OntologyContext::default(),
            system: Self::build_system_info(),
            capabilities: Self::build_capabilities(),
            resources: Self::build_resources(),
            operations: Self::build_operations(),
            state_machines: Self::build_state_machines(),
            events: Self::build_events(),
        }
    }

    fn build_system_info() -> SystemInfo {
        SystemInfo {
            name: "HyperMachine".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Next-generation hybrid hypervisor with AI-native capabilities. \
                Provides Type-1 bare-metal and Type-2 hosted virtualization with \
                first-class support for AI agent orchestration."
                .to_string(),
            documentation_url: "https://docs.nervosys.ai/hypermachine".to_string(),
            api_base_url: "/api/v1".to_string(),
            supported_protocols: vec![
                "REST".to_string(),
                "gRPC".to_string(),
                "WebSocket".to_string(),
            ],
            authentication: AuthenticationInfo {
                required: true,
                methods: vec![
                    AuthMethod {
                        method_type: "bearer_token".to_string(),
                        description: "Ed25519-signed JWT bearer token".to_string(),
                        header_name: Some("Authorization".to_string()),
                    },
                    AuthMethod {
                        method_type: "mtls".to_string(),
                        description: "Mutual TLS with client certificate".to_string(),
                        header_name: None,
                    },
                ],
                token_endpoint: Some("/api/v1/auth/token".to_string()),
            },
        }
    }

    fn build_capabilities() -> Vec<Capability> {
        vec![
            Capability {
                id: "vm_management".to_string(),
                name: "Virtual Machine Management".to_string(),
                description: "Create, configure, and manage virtual machines with \
                    customizable CPU, memory, storage, and networking"
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "create_vm".to_string(),
                    "delete_vm".to_string(),
                    "get_vm".to_string(),
                    "list_vms".to_string(),
                    "update_vm".to_string(),
                ],
                prerequisites: vec![],
                permissions_required: vec!["vm:create".to_string(), "vm:read".to_string()],
            },
            Capability {
                id: "vm_lifecycle".to_string(),
                name: "VM Lifecycle Control".to_string(),
                description: "Start, stop, pause, resume, and snapshot virtual machines"
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "start_vm".to_string(),
                    "stop_vm".to_string(),
                    "pause_vm".to_string(),
                    "resume_vm".to_string(),
                    "snapshot_vm".to_string(),
                    "restore_vm".to_string(),
                ],
                prerequisites: vec!["vm_management".to_string()],
                permissions_required: vec!["vm:control".to_string()],
            },
            Capability {
                id: "agent_execution".to_string(),
                name: "AI Agent Execution".to_string(),
                description: "Execute AI agent scripts in sandboxed WASM/Rhai environments \
                    within VMs. Agents can automate VM operations with capability-based security."
                    .to_string(),
                category: CapabilityCategory::AgentExecution,
                operations: vec![
                    "execute_script".to_string(),
                    "list_agents".to_string(),
                    "get_agent_logs".to_string(),
                ],
                prerequisites: vec!["vm_management".to_string()],
                permissions_required: vec!["agent:execute".to_string()],
            },
            Capability {
                id: "gpu_passthrough".to_string(),
                name: "GPU Passthrough".to_string(),
                description: "Attach and manage GPU devices for AI/ML workloads in VMs".to_string(),
                category: CapabilityCategory::Compute,
                operations: vec![
                    "attach_gpu".to_string(),
                    "detach_gpu".to_string(),
                    "list_gpus".to_string(),
                ],
                prerequisites: vec!["vm_management".to_string()],
                permissions_required: vec!["gpu:manage".to_string()],
            },
            Capability {
                id: "metrics_monitoring".to_string(),
                name: "Metrics and Monitoring".to_string(),
                description: "Collect and query VM and system metrics, set up alerts".to_string(),
                category: CapabilityCategory::Monitoring,
                operations: vec![
                    "get_metrics".to_string(),
                    "query_metrics".to_string(),
                    "set_alert".to_string(),
                ],
                prerequisites: vec![],
                permissions_required: vec!["metrics:read".to_string()],
            },
        ]
    }

    fn build_resources() -> Vec<ResourceType> {
        vec![
            ResourceType {
                id: "vm".to_string(),
                name: "Virtual Machine".to_string(),
                description: "A virtual machine instance with dedicated compute, memory, and I/O"
                    .to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "name": { "type": "string", "minLength": 1, "maxLength": 64 },
                        "state": { "type": "string", "enum": ["created", "starting", "running", "paused", "stopping", "stopped", "error"] },
                        "vcpu_count": { "type": "integer", "minimum": 1, "maximum": 256 },
                        "memory_gb": { "type": "integer", "minimum": 1, "maximum": 4096 },
                        "enable_gpu": { "type": "boolean" },
                        "enable_networking": { "type": "boolean" },
                        "created_at": { "type": "string", "format": "date-time" },
                        "updated_at": { "type": "string", "format": "date-time" }
                    },
                    "required": ["id", "name", "state"]
                }),
                lifecycle_states: vec![
                    "created".to_string(),
                    "starting".to_string(),
                    "running".to_string(),
                    "paused".to_string(),
                    "stopping".to_string(),
                    "stopped".to_string(),
                    "error".to_string(),
                ],
                relationships: vec![
                    ResourceRelationship {
                        name: "disks".to_string(),
                        target_type: "disk".to_string(),
                        cardinality: "one-to-many".to_string(),
                        description: "Storage disks attached to the VM".to_string(),
                    },
                    ResourceRelationship {
                        name: "networks".to_string(),
                        target_type: "network".to_string(),
                        cardinality: "many-to-many".to_string(),
                        description: "Networks the VM is connected to".to_string(),
                    },
                    ResourceRelationship {
                        name: "gpus".to_string(),
                        target_type: "gpu".to_string(),
                        cardinality: "one-to-many".to_string(),
                        description: "GPUs attached to the VM".to_string(),
                    },
                ],
            },
            ResourceType {
                id: "agent".to_string(),
                name: "AI Agent".to_string(),
                description: "An AI agent script running in a sandboxed environment".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "vm_id": { "type": "string", "format": "uuid" },
                        "script_type": { "type": "string", "enum": ["rhai", "wasm"] },
                        "state": { "type": "string", "enum": ["pending", "running", "completed", "failed"] },
                        "output": { "type": "string" },
                        "error": { "type": "string" },
                        "started_at": { "type": "string", "format": "date-time" },
                        "completed_at": { "type": "string", "format": "date-time" }
                    },
                    "required": ["id", "vm_id", "script_type", "state"]
                }),
                lifecycle_states: vec![
                    "pending".to_string(),
                    "running".to_string(),
                    "completed".to_string(),
                    "failed".to_string(),
                ],
                relationships: vec![ResourceRelationship {
                    name: "vm".to_string(),
                    target_type: "vm".to_string(),
                    cardinality: "many-to-one".to_string(),
                    description: "The VM this agent runs in".to_string(),
                }],
            },
        ]
    }

    fn build_operations() -> Vec<Operation> {
        vec![
            // VM CRUD Operations
            Operation {
                id: "list_vms".to_string(),
                name: "List Virtual Machines".to_string(),
                description: "Retrieve a list of all virtual machines. Supports filtering and pagination.".to_string(),
                http_method: "GET".to_string(),
                path: "/api/v1/vms".to_string(),
                parameters: vec![
                    Parameter {
                        name: "state".to_string(),
                        location: ParameterLocation::Query,
                        required: false,
                        description: "Filter by VM state".to_string(),
                        schema: serde_json::json!({ "type": "string", "enum": ["running", "stopped", "paused"] }),
                        default: None,
                    },
                    Parameter {
                        name: "limit".to_string(),
                        location: ParameterLocation::Query,
                        required: false,
                        description: "Maximum number of results".to_string(),
                        schema: serde_json::json!({ "type": "integer", "minimum": 1, "maximum": 100 }),
                        default: Some(serde_json::json!(20)),
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "List of VMs".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "vms": { "type": "array", "items": { "$ref": "#/resources/vm" } },
                                "total": { "type": "integer" }
                            }
                        })),
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 100, burst_size: 20 }),
                examples: vec![
                    OperationExample {
                        name: "List all running VMs".to_string(),
                        description: "Get all VMs in the running state".to_string(),
                        request: None,
                        response: serde_json::json!({
                            "vms": [
                                { "id": "vm-123", "name": "web-server", "state": "running" }
                            ],
                            "total": 1
                        }),
                    },
                ],
            },
            Operation {
                id: "create_vm".to_string(),
                name: "Create Virtual Machine".to_string(),
                description: "Create a new virtual machine with specified configuration. The VM will be created in 'stopped' state.".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms".to_string(),
                parameters: vec![],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "minLength": 1, "maxLength": 64, "description": "Human-readable VM name" },
                            "vcpu_count": { "type": "integer", "minimum": 1, "maximum": 256, "default": 2, "description": "Number of virtual CPUs" },
                            "memory_gb": { "type": "integer", "minimum": 1, "maximum": 4096, "default": 4, "description": "Memory in gigabytes" },
                            "enable_gpu": { "type": "boolean", "default": false, "description": "Enable GPU passthrough" },
                            "enable_networking": { "type": "boolean", "default": false, "description": "Enable network access" }
                        },
                        "required": ["name"]
                    }),
                    required: true,
                }),
                responses: vec![
                    OperationResponse {
                        status_code: 201,
                        description: "VM created successfully".to_string(),
                        schema: Some(serde_json::json!({ "$ref": "#/resources/vm" })),
                    },
                    OperationResponse {
                        status_code: 400,
                        description: "Invalid request parameters".to_string(),
                        schema: None,
                    },
                    OperationResponse {
                        status_code: 409,
                        description: "VM with this name already exists".to_string(),
                        schema: None,
                    },
                ],
                idempotent: false,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 10, burst_size: 5 }),
                examples: vec![
                    OperationExample {
                        name: "Create a basic VM".to_string(),
                        description: "Create a VM with 4 vCPUs and 8GB RAM".to_string(),
                        request: Some(serde_json::json!({
                            "name": "my-ai-worker",
                            "vcpu_count": 4,
                            "memory_gb": 8,
                            "enable_gpu": true
                        })),
                        response: serde_json::json!({
                            "id": "vm-456",
                            "name": "my-ai-worker",
                            "state": "created",
                            "vcpu_count": 4,
                            "memory_gb": 8
                        }),
                    },
                ],
            },
            Operation {
                id: "get_vm".to_string(),
                name: "Get Virtual Machine".to_string(),
                description: "Retrieve details of a specific virtual machine by ID".to_string(),
                http_method: "GET".to_string(),
                path: "/api/v1/vms/{id}".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM details".to_string(),
                        schema: Some(serde_json::json!({ "$ref": "#/resources/vm" })),
                    },
                    OperationResponse {
                        status_code: 404,
                        description: "VM not found".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "delete_vm".to_string(),
                name: "Delete Virtual Machine".to_string(),
                description: "Delete a virtual machine. The VM must be in stopped state.".to_string(),
                http_method: "DELETE".to_string(),
                path: "/api/v1/vms/{id}".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM deleted".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "deleted": { "type": "boolean" }
                            }
                        })),
                    },
                    OperationResponse {
                        status_code: 404,
                        description: "VM not found".to_string(),
                        schema: None,
                    },
                    OperationResponse {
                        status_code: 409,
                        description: "VM is not stopped".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            // Lifecycle Operations
            Operation {
                id: "start_vm".to_string(),
                name: "Start Virtual Machine".to_string(),
                description: "Start a stopped virtual machine. Transitions from 'stopped' to 'running' state.".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/start".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM started".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "operation": { "type": "string" },
                                "success": { "type": "boolean" },
                                "new_state": { "type": "string" }
                            }
                        })),
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "stop_vm".to_string(),
                name: "Stop Virtual Machine".to_string(),
                description: "Gracefully stop a running virtual machine".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/stop".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM stopped".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "pause_vm".to_string(),
                name: "Pause Virtual Machine".to_string(),
                description: "Pause a running VM, suspending all CPU activity".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/pause".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM paused".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "resume_vm".to_string(),
                name: "Resume Virtual Machine".to_string(),
                description: "Resume a paused VM".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/resume".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM resumed".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            // Agent Execution
            Operation {
                id: "execute_script".to_string(),
                name: "Execute Agent Script".to_string(),
                description: "Execute a Rhai or WASM script in the VM's sandboxed agent environment. \
                    Scripts have access to VM control operations based on granted capabilities.".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/script".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "script_type": { 
                                "type": "string", 
                                "enum": ["rhai", "wasm"],
                                "default": "rhai",
                                "description": "Script runtime: 'rhai' for dynamic scripts, 'wasm' for compiled modules"
                            },
                            "script": { 
                                "type": "string",
                                "description": "Script content (Rhai source code or base64-encoded WASM)"
                            },
                            "timeout_seconds": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 300,
                                "default": 30,
                                "description": "Maximum execution time"
                            },
                            "capabilities": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Capabilities to grant to the script",
                                "default": []
                            }
                        },
                        "required": ["script"]
                    }),
                    required: true,
                }),
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "Script executed successfully".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "execution_id": { "type": "string" },
                                "status": { "type": "string", "enum": ["completed", "failed", "timeout"] },
                                "output": { "type": "string" },
                                "error": { "type": "string" },
                                "duration_ms": { "type": "integer" }
                            }
                        })),
                    },
                ],
                idempotent: false,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 60, burst_size: 10 }),
                examples: vec![
                    OperationExample {
                        name: "Get VM status via script".to_string(),
                        description: "Execute a Rhai script to check VM state".to_string(),
                        request: Some(serde_json::json!({
                            "script_type": "rhai",
                            "script": "let status = vm_status(); print(`VM is ${status}`); status",
                            "timeout_seconds": 10
                        })),
                        response: serde_json::json!({
                            "execution_id": "exec-789",
                            "status": "completed",
                            "output": "VM is running",
                            "duration_ms": 5
                        }),
                    },
                ],
            },
            // Metrics
            Operation {
                id: "get_metrics".to_string(),
                name: "Get VM Metrics".to_string(),
                description: "Retrieve current performance metrics for a VM".to_string(),
                http_method: "GET".to_string(),
                path: "/api/v1/vms/{id}/metrics".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM metrics".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "cpu_usage_percent": { "type": "number" },
                                "memory_usage_percent": { "type": "number" },
                                "disk_read_bytes": { "type": "integer" },
                                "disk_write_bytes": { "type": "integer" },
                                "network_rx_bytes": { "type": "integer" },
                                "network_tx_bytes": { "type": "integer" },
                                "uptime_seconds": { "type": "integer" }
                            }
                        })),
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 300, burst_size: 50 }),
                examples: vec![],
            },
        ]
    }

    fn build_state_machines() -> Vec<StateMachine> {
        vec![StateMachine {
            resource_type: "vm".to_string(),
            states: vec![
                State {
                    name: "created".to_string(),
                    description: "VM has been created but not started".to_string(),
                    allowed_operations: vec![
                        "start_vm".to_string(),
                        "delete_vm".to_string(),
                        "update_vm".to_string(),
                    ],
                },
                State {
                    name: "starting".to_string(),
                    description: "VM is starting up".to_string(),
                    allowed_operations: vec![],
                },
                State {
                    name: "running".to_string(),
                    description: "VM is running and operational".to_string(),
                    allowed_operations: vec![
                        "stop_vm".to_string(),
                        "pause_vm".to_string(),
                        "execute_script".to_string(),
                        "get_metrics".to_string(),
                    ],
                },
                State {
                    name: "paused".to_string(),
                    description: "VM is paused".to_string(),
                    allowed_operations: vec!["resume_vm".to_string(), "stop_vm".to_string()],
                },
                State {
                    name: "stopping".to_string(),
                    description: "VM is shutting down".to_string(),
                    allowed_operations: vec![],
                },
                State {
                    name: "stopped".to_string(),
                    description: "VM is stopped".to_string(),
                    allowed_operations: vec![
                        "start_vm".to_string(),
                        "delete_vm".to_string(),
                        "update_vm".to_string(),
                    ],
                },
                State {
                    name: "error".to_string(),
                    description: "VM is in error state".to_string(),
                    allowed_operations: vec!["delete_vm".to_string()],
                },
            ],
            transitions: vec![
                Transition {
                    from_state: "created".to_string(),
                    to_state: "starting".to_string(),
                    trigger_operation: "start_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "starting".to_string(),
                    to_state: "running".to_string(),
                    trigger_operation: "auto".to_string(),
                    conditions: vec!["boot_complete".to_string()],
                },
                Transition {
                    from_state: "running".to_string(),
                    to_state: "paused".to_string(),
                    trigger_operation: "pause_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "paused".to_string(),
                    to_state: "running".to_string(),
                    trigger_operation: "resume_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "running".to_string(),
                    to_state: "stopping".to_string(),
                    trigger_operation: "stop_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "stopping".to_string(),
                    to_state: "stopped".to_string(),
                    trigger_operation: "auto".to_string(),
                    conditions: vec!["shutdown_complete".to_string()],
                },
                Transition {
                    from_state: "stopped".to_string(),
                    to_state: "starting".to_string(),
                    trigger_operation: "start_vm".to_string(),
                    conditions: vec![],
                },
            ],
            initial_state: "created".to_string(),
            terminal_states: vec!["error".to_string()],
        }]
    }

    fn build_events() -> Vec<EventType> {
        vec![
            EventType {
                id: "vm.state_changed".to_string(),
                name: "VM State Changed".to_string(),
                description: "Emitted when a VM transitions between states".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "vm_id": { "type": "string" },
                        "previous_state": { "type": "string" },
                        "new_state": { "type": "string" },
                        "timestamp": { "type": "string", "format": "date-time" }
                    }
                }),
                resource_types: vec!["vm".to_string()],
            },
            EventType {
                id: "vm.metrics".to_string(),
                name: "VM Metrics Update".to_string(),
                description: "Periodic metrics update for a VM".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "vm_id": { "type": "string" },
                        "metrics": { "$ref": "#/operations/get_metrics/responses/200/schema" },
                        "timestamp": { "type": "string", "format": "date-time" }
                    }
                }),
                resource_types: vec!["vm".to_string()],
            },
            EventType {
                id: "agent.completed".to_string(),
                name: "Agent Execution Completed".to_string(),
                description: "Emitted when an agent script completes execution".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "execution_id": { "type": "string" },
                        "vm_id": { "type": "string" },
                        "status": { "type": "string" },
                        "output": { "type": "string" },
                        "duration_ms": { "type": "integer" }
                    }
                }),
                resource_types: vec!["agent".to_string()],
            },
        ]
    }

    /// Convert to OpenAI function calling format
    pub fn to_openai_tools(&self) -> OpenAITools {
        let tools = self
            .operations
            .iter()
            .map(|op| OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIFunction {
                    name: op.id.clone(),
                    description: op.description.clone(),
                    parameters: self.build_openai_parameters(op),
                },
            })
            .collect();

        OpenAITools { tools }
    }

    fn build_openai_parameters(&self, op: &Operation) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &op.parameters {
            if param.location == ParameterLocation::Path
                || param.location == ParameterLocation::Query
            {
                // Merge schema with description for tool formats
                let mut param_schema = param.schema.clone();
                if let Some(obj) = param_schema.as_object_mut() {
                    obj.insert("description".to_string(), serde_json::Value::String(param.description.clone()));
                }
                properties.insert(param.name.clone(), param_schema);
                if param.required {
                    required.push(param.name.clone());
                }
            }
        }

        if let Some(ref body) = op.request_body {
            if let Some(props) = body.schema.get("properties") {
                if let Some(obj) = props.as_object() {
                    for (k, v) in obj {
                        properties.insert(k.clone(), v.clone());
                    }
                }
            }
            if let Some(req) = body.schema.get("required") {
                if let Some(arr) = req.as_array() {
                    for r in arr {
                        if let Some(s) = r.as_str() {
                            required.push(s.to_string());
                        }
                    }
                }
            }
        }

        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }

    /// Convert to Anthropic MCP tool format
    pub fn to_anthropic_tools(&self) -> AnthropicTools {
        let tools = self
            .operations
            .iter()
            .map(|op| AnthropicTool {
                name: op.id.clone(),
                description: op.description.clone(),
                input_schema: self.build_openai_parameters(op),
            })
            .collect();

        AnthropicTools {
            name: "hypermachine".to_string(),
            version: self.system.version.clone(),
            tools,
        }
    }

    /// Convert to Google Gemini format
    pub fn to_gemini_tools(&self) -> GeminiTools {
        let function_declarations = self
            .operations
            .iter()
            .map(|op| GeminiFunction {
                name: op.id.clone(),
                description: op.description.clone(),
                parameters: self.build_openai_parameters(op),
            })
            .collect();

        GeminiTools {
            function_declarations,
        }
    }
}

// ============================================================================
// HTTP Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct OntologyQuery {
    /// Output format: json-ld, openapi, openai, anthropic, gemini
    pub format: Option<String>,
}

/// Create ontology router
pub fn create_ontology_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/agentic/ontology", get(get_ontology))
        .route("/agentic/tools/openai", get(get_openai_tools))
        .route("/agentic/tools/anthropic", get(get_anthropic_tools))
        .route("/agentic/tools/gemini", get(get_gemini_tools))
        .route("/.well-known/ai-plugin.json", get(get_ai_plugin_manifest))
}

async fn get_ontology(Query(query): Query<OntologyQuery>) -> Response {
    let ontology = HyperMachineOntology::build();
    let format = query.format.as_deref().unwrap_or("json-ld");

    match format {
        "openai" => {
            let tools = ontology.to_openai_tools();
            Json(tools).into_response()
        }
        "anthropic" => {
            let tools = ontology.to_anthropic_tools();
            Json(tools).into_response()
        }
        "gemini" => {
            let tools = ontology.to_gemini_tools();
            Json(tools).into_response()
        }
        _ => {
            // Default: full JSON-LD ontology
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "application/ld+json".parse().unwrap());
            (headers, Json(ontology)).into_response()
        }
    }
}

async fn get_openai_tools() -> Json<OpenAITools> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.to_openai_tools())
}

async fn get_anthropic_tools() -> Json<AnthropicTools> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.to_anthropic_tools())
}

async fn get_gemini_tools() -> Json<GeminiTools> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.to_gemini_tools())
}

/// OpenAI ChatGPT Plugin manifest
#[derive(Serialize)]
struct AiPluginManifest {
    schema_version: String,
    name_for_human: String,
    name_for_model: String,
    description_for_human: String,
    description_for_model: String,
    auth: PluginAuth,
    api: PluginApi,
    logo_url: String,
    contact_email: String,
    legal_info_url: String,
}

#[derive(Serialize)]
struct PluginAuth {
    #[serde(rename = "type")]
    auth_type: String,
}

#[derive(Serialize)]
struct PluginApi {
    #[serde(rename = "type")]
    api_type: String,
    url: String,
}

async fn get_ai_plugin_manifest() -> Json<AiPluginManifest> {
    Json(AiPluginManifest {
        schema_version: "v1".to_string(),
        name_for_human: "HyperMachine".to_string(),
        name_for_model: "hypermachine".to_string(),
        description_for_human: "Manage virtual machines with HyperMachine hypervisor".to_string(),
        description_for_model: "HyperMachine is a hybrid hypervisor API. Use it to create, \
            manage, and control virtual machines. You can start/stop VMs, execute agent scripts, \
            attach GPUs, and monitor performance. Always check VM state before operations."
            .to_string(),
        auth: PluginAuth {
            auth_type: "service_http".to_string(),
        },
        api: PluginApi {
            api_type: "openapi".to_string(),
            url: "/agentic/openapi.yaml".to_string(),
        },
        logo_url: "https://nervosys.ai/logo.png".to_string(),
        contact_email: "api@nervosys.ai".to_string(),
        legal_info_url: "https://nervosys.ai/legal".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ontology_builds() {
        let ontology = HyperMachineOntology::build();
        assert!(!ontology.capabilities.is_empty());
        assert!(!ontology.operations.is_empty());
        assert!(!ontology.resources.is_empty());
    }

    #[test]
    fn test_openai_conversion() {
        let ontology = HyperMachineOntology::build();
        let tools = ontology.to_openai_tools();
        assert!(!tools.tools.is_empty());
        assert!(tools.tools.iter().any(|t| t.function.name == "create_vm"));
    }

    #[test]
    fn test_anthropic_conversion() {
        let ontology = HyperMachineOntology::build();
        let tools = ontology.to_anthropic_tools();
        assert_eq!(tools.name, "hypermachine");
        assert!(!tools.tools.is_empty());
    }

    #[test]
    fn test_gemini_conversion() {
        let ontology = HyperMachineOntology::build();
        let tools = ontology.to_gemini_tools();
        assert!(!tools.function_declarations.is_empty());
    }
}
