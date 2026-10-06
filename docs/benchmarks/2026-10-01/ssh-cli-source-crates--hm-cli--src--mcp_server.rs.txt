//! MCP HTTP Server for AI Agent Access
//!
//! Provides a JSON-over-HTTP API for AI agents to manage VMs using the
//! Model Context Protocol pattern. Compatible with OpenAI function calling
//! and Anthropic tool use.

use crate::vm_manager::{VmManager, VmMetrics};
use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

/// MCP Server state
pub struct McpServerState {
    /// VM manager
    pub vm_manager: Arc<VmManager>,
    /// Active sessions
    pub sessions: RwLock<HashMap<String, McpSession>>,
    /// API key for authentication (optional)
    pub api_key: Option<String>,
    /// Rate limiter state
    pub rate_limiter: RateLimiter,
}

/// An agent session
#[derive(Debug, Clone)]
pub struct McpSession {
    pub agent_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
    pub call_count: u64,
}

/// Simple rate limiter (token bucket per IP)
pub struct RateLimiter {
    /// Request counts per IP
    buckets: RwLock<HashMap<String, RateBucket>>,
    /// Max requests per window
    pub max_requests: u64,
    /// Window duration
    pub window: Duration,
}

struct RateBucket {
    count: AtomicU64,
    window_start: Instant,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window: Duration) -> Self {
        Self {
            buckets: RwLock::new(HashMap::new()),
            max_requests,
            window,
        }
    }

    /// Record a request against `key`'s quota.
    ///
    /// Returns the quota remaining after this request, or `None` if the
    /// request exceeded the limit and should be refused.
    pub async fn check(&self, key: &str) -> Option<u64> {
        let now = Instant::now();
        let mut buckets = self.buckets.write().await;

        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| RateBucket {
                count: AtomicU64::new(0),
                window_start: now,
            });

        // Reset if window expired
        if now.duration_since(bucket.window_start) > self.window {
            bucket.count.store(0, Ordering::SeqCst);
            bucket.window_start = now;
        }

        let current = bucket.count.fetch_add(1, Ordering::SeqCst);
        if current >= self.max_requests {
            None
        } else {
            Some(self.max_requests - current - 1)
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        // Default: 100 requests per minute
        Self::new(100, Duration::from_secs(60))
    }
}

/// Tool definition (OpenAI/Anthropic compatible)
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Tool call request
#[derive(Debug, Deserialize)]
pub struct ToolCallRequest {
    pub tool: String,
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// Tool call response
#[derive(Debug, Serialize)]
pub struct ToolCallResponse {
    pub success: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// VM creation request
#[derive(Debug, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default = "default_cpu")]
    pub cpu_cores: u32,
    #[serde(default = "default_memory")]
    pub memory_gb: u64,
    #[serde(default)]
    pub gpu_enabled: bool,
    #[serde(default)]
    pub network_enabled: bool,
    /// What this VM boots. Omit it for a VM with no guest code.
    #[serde(default)]
    pub boot: Option<hv2_core::BootSource>,
}

fn default_cpu() -> u32 {
    2
}
fn default_memory() -> u64 {
    4
}

/// VM info response
#[derive(Debug, Serialize)]
pub struct VmInfo {
    pub name: String,
    pub cpu_cores: u32,
    pub memory_gb: u64,
    pub gpu_enabled: bool,
    pub network_enabled: bool,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Script execution request
#[derive(Debug, Deserialize)]
pub struct ExecuteScriptRequest {
    pub script: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    300
}

/// Authentication middleware
async fn auth_middleware(
    State(state): State<Arc<McpServerState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // If no API key configured, allow all requests
    let Some(ref required_key) = state.api_key else {
        return Ok(next.run(request).await);
    };

    // Check Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            if token == required_key {
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Rate limiting middleware
async fn rate_limit_middleware(
    State(state): State<Arc<McpServerState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Use X-Forwarded-For or connection IP as key
    let ip = request
        .headers()
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("unknown").trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    match state.rate_limiter.check(&ip).await {
        Some(_remaining) => Ok(next.run(request).await),
        None => Err(StatusCode::TOO_MANY_REQUESTS),
    }
}

/// Start the MCP HTTP server
pub async fn start_mcp_server(addr: SocketAddr) -> Result<()> {
    let vm_manager = Arc::new(VmManager::new()?);

    // Load API key from environment
    let api_key = std::env::var("HM_API_KEY").ok();
    if api_key.is_some() {
        tracing::info!("API key authentication enabled");
    } else {
        tracing::warn!("No API key configured (set HM_API_KEY for authentication)");
    }

    let state = Arc::new(McpServerState {
        vm_manager,
        sessions: RwLock::new(HashMap::new()),
        api_key,
        rate_limiter: RateLimiter::default(),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Protected routes (require auth if configured)
    let protected_routes = Router::new()
        .route("/mcp/call", post(call_tool))
        .route("/vms", post(create_vm))
        .route("/vms/{name}", delete(delete_vm))
        .route("/vms/{name}/start", post(start_vm))
        .route("/vms/{name}/stop", post(stop_vm))
        .route("/vms/{name}/script", post(execute_script))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/mcp/tools", get(list_tools))
        .route("/vms", get(list_vms))
        .route("/vms/{name}", get(get_vm))
        .route("/vms/{name}/metrics", get(get_vm_metrics))
        .route("/health", get(health_check))
        // Agentic AI endpoints for LLM discovery and introspection
        .route("/agentic/ontology", get(agentic_ontology))
        .route("/agentic/schema", get(agentic_schema))
        .route("/agentic/schema/compact", get(agentic_schema_compact))
        .route("/agentic/capabilities", get(agentic_capabilities))
        .route("/agentic/providers/{provider}", get(agentic_provider_config))
        .route("/agentic/tools/openai", get(agentic_openai_tools))
        .route("/agentic/tools/anthropic", get(agentic_anthropic_tools))
        .route("/agentic/tools/gemini", get(agentic_gemini_tools));

    let app = Router::new()
        .merge(protected_routes)
        .merge(public_routes)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(cors)
        .with_state(state);

    tracing::info!("MCP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// List available tools (OpenAI/Anthropic function calling format)
async fn list_tools() -> Json<Vec<ToolDefinition>> {
    let tools = vec![
        ToolDefinition {
            name: "vm.create".to_string(),
            description: "Create a new virtual machine".to_string(),
            parameters: serde_json::json!({
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
                        "description": "Enable GPU passthrough",
                        "default": false
                    },
                    "network_enabled": {
                        "type": "boolean",
                        "description": "Enable networking",
                        "default": false
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "vm.list".to_string(),
            description: "List all virtual machines".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "vm.get".to_string(),
            description: "Get details of a specific virtual machine".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "VM name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "vm.start".to_string(),
            description: "Start a virtual machine".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "VM name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "vm.stop".to_string(),
            description: "Stop a virtual machine".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "VM name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "vm.delete".to_string(),
            description: "Delete a virtual machine".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "VM name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "vm.metrics".to_string(),
            description: "Get VM metrics (CPU, memory, uptime)".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "VM name"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "vm.execute_script".to_string(),
            description: "Evaluate a Rhai script on the host against a read-only view of the VM. \n                          Does NOT run commands inside the guest OS."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "VM name"
                    },
                    "script": {
                        "type": "string",
                        "description": "Script content to execute"
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Execution timeout in seconds",
                        "default": 300
                    }
                },
                "required": ["name", "script"]
            }),
        },
    ];

    Json(tools)
}

/// Unified tool call endpoint (for MCP clients)
async fn call_tool(
    State(state): State<Arc<McpServerState>>,
    Json(request): Json<ToolCallRequest>,
) -> Json<ToolCallResponse> {
    let result = match request.tool.as_str() {
        "vm.create" => {
            let args: CreateVmRequest = match serde_json::from_value(request.arguments) {
                Ok(a) => a,
                Err(e) => {
                    return Json(ToolCallResponse {
                        success: false,
                        result: None,
                        error: Some(format!("Invalid arguments: {}", e)),
                    })
                }
            };
            match state
                .vm_manager
                // create_bootable_vm, not create_vm: the five-argument form
                // hardcodes `boot: None`, so an agent that supplied a kernel
                // got a success record back and then a VM with no guest code in
                // it. The REST handler on this same request type already calls
                // this; only the MCP tool path dropped the field.
                .create_bootable_vm(
                    &args.name,
                    args.cpu_cores,
                    args.memory_gb,
                    args.gpu_enabled,
                    args.network_enabled,
                    args.boot.clone(),
                )
                .await
            {
                Ok(record) => Ok(serde_json::json!({
                    "name": record.name,
                    "state": format!("{}", record.state),
                    "cpu_cores": record.cpu_cores,
                    "memory_gb": record.memory_gb
                })),
                Err(e) => Err(e.to_string()),
            }
        }
        "vm.list" => {
            let vms = state.vm_manager.list_vms().await;
            Ok(serde_json::json!(vms
                .iter()
                .map(|v| serde_json::json!({
                    "name": v.name,
                    "state": format!("{}", v.state),
                    "cpu_cores": v.cpu_cores,
                    "memory_gb": v.memory_gb
                }))
                .collect::<Vec<_>>()))
        }
        "vm.get" => {
            let name = request.arguments["name"].as_str().unwrap_or_default();
            match state.vm_manager.get_vm(name).await {
                Ok(record) => Ok(serde_json::json!({
                    "name": record.name,
                    "state": format!("{}", record.state),
                    "cpu_cores": record.cpu_cores,
                    "memory_gb": record.memory_gb,
                    "gpu_enabled": record.gpu_enabled,
                    "network_enabled": record.network_enabled
                })),
                Err(e) => Err(e.to_string()),
            }
        }
        "vm.start" => {
            let name = request.arguments["name"].as_str().unwrap_or_default();
            match state.vm_manager.start_vm(name).await {
                Ok(()) => Ok(serde_json::json!({"status": "started", "name": name})),
                Err(e) => Err(e.to_string()),
            }
        }
        "vm.stop" => {
            let name = request.arguments["name"].as_str().unwrap_or_default();
            match state.vm_manager.stop_vm(name).await {
                Ok(()) => Ok(serde_json::json!({"status": "stopped", "name": name})),
                Err(e) => Err(e.to_string()),
            }
        }
        "vm.delete" => {
            let name = request.arguments["name"].as_str().unwrap_or_default();
            match state.vm_manager.delete_vm(name).await {
                Ok(()) => Ok(serde_json::json!({"status": "deleted", "name": name})),
                Err(e) => Err(e.to_string()),
            }
        }
        "vm.metrics" => {
            let name = request.arguments["name"].as_str().unwrap_or_default();
            match state.vm_manager.get_metrics(name).await {
                Ok(metrics) => Ok(serde_json::json!({
                    "name": metrics.name,
                    "state": format!("{}", metrics.state),
                    "cpu_cores": metrics.cpu_cores,
                    "memory_gb": metrics.memory_gb,
                    "uptime_seconds": metrics.uptime_seconds
                })),
                Err(e) => Err(e.to_string()),
            }
        }
        "vm.execute_script" => {
            let name = request.arguments["name"].as_str().unwrap_or_default();
            let script = request.arguments["script"].as_str().unwrap_or_default();
            match state.vm_manager.execute_script(name, script).await {
                Ok(result) => Ok(result),
                Err(e) => Err(e.to_string()),
            }
        }
        _ => Err(format!("Unknown tool: {}", request.tool)),
    };

    match result {
        Ok(value) => Json(ToolCallResponse {
            success: true,
            result: Some(value),
            error: None,
        }),
        Err(e) => Json(ToolCallResponse {
            success: false,
            result: None,
            error: Some(e),
        }),
    }
}

/// List all VMs
async fn list_vms(State(state): State<Arc<McpServerState>>) -> Json<Vec<VmInfo>> {
    let vms = state.vm_manager.list_vms().await;
    Json(
        vms.into_iter()
            .map(|v| VmInfo {
                name: v.name,
                cpu_cores: v.cpu_cores,
                memory_gb: v.memory_gb,
                gpu_enabled: v.gpu_enabled,
                network_enabled: v.network_enabled,
                state: format!("{}", v.state),
                created_at: v.created_at.to_rfc3339(),
                updated_at: v.updated_at.to_rfc3339(),
            })
            .collect(),
    )
}

/// Create a new VM
async fn create_vm(
    State(state): State<Arc<McpServerState>>,
    Json(request): Json<CreateVmRequest>,
) -> Result<Json<VmInfo>, (StatusCode, String)> {
    state
        .vm_manager
        .create_bootable_vm(
            &request.name,
            request.cpu_cores,
            request.memory_gb,
            request.gpu_enabled,
            request.network_enabled,
            request.boot,
        )
        .await
        .map(|v| {
            Json(VmInfo {
                name: v.name,
                cpu_cores: v.cpu_cores,
                memory_gb: v.memory_gb,
                gpu_enabled: v.gpu_enabled,
                network_enabled: v.network_enabled,
                state: format!("{}", v.state),
                created_at: v.created_at.to_rfc3339(),
                updated_at: v.updated_at.to_rfc3339(),
            })
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Get a specific VM
async fn get_vm(
    State(state): State<Arc<McpServerState>>,
    Path(name): Path<String>,
) -> Result<Json<VmInfo>, (StatusCode, String)> {
    state
        .vm_manager
        .get_vm(&name)
        .await
        .map(|v| {
            Json(VmInfo {
                name: v.name,
                cpu_cores: v.cpu_cores,
                memory_gb: v.memory_gb,
                gpu_enabled: v.gpu_enabled,
                network_enabled: v.network_enabled,
                state: format!("{}", v.state),
                created_at: v.created_at.to_rfc3339(),
                updated_at: v.updated_at.to_rfc3339(),
            })
        })
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}

/// Delete a VM
async fn delete_vm(
    State(state): State<Arc<McpServerState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .vm_manager
        .delete_vm(&name)
        .await
        .map(|_| Json(serde_json::json!({"status": "deleted", "name": name})))
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}

/// Start a VM
async fn start_vm(
    State(state): State<Arc<McpServerState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .vm_manager
        .start_vm(&name)
        .await
        .map(|_| Json(serde_json::json!({"status": "started", "name": name})))
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Stop a VM
async fn stop_vm(
    State(state): State<Arc<McpServerState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .vm_manager
        .stop_vm(&name)
        .await
        .map(|_| Json(serde_json::json!({"status": "stopped", "name": name})))
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Get VM metrics
async fn get_vm_metrics(
    State(state): State<Arc<McpServerState>>,
    Path(name): Path<String>,
) -> Result<Json<VmMetrics>, (StatusCode, String)> {
    state
        .vm_manager
        .get_metrics(&name)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}

/// Execute script on VM
async fn execute_script(
    State(state): State<Arc<McpServerState>>,
    Path(name): Path<String>,
    Json(request): Json<ExecuteScriptRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .vm_manager
        .execute_script(&name, &request.script)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "hypermachine-mcp",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ============================================================================
// Agentic AI Endpoints
// ============================================================================

use crate::agentic::adapters::LlmProvider;
use crate::agentic::schema::SchemaEndpoints;

/// Get the complete HyperMachine ontology
///
/// Returns a fully discoverable programming language ontology that AI agents
/// can use to understand all available operations, types, and relationships.
async fn agentic_ontology() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::ontology())
}

/// Get the full JSON Schema for AI agents
///
/// Returns a complete JSON Schema document with all type definitions,
/// operations, and examples.
async fn agentic_schema() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::full_schema())
}

/// Get a compact schema for bandwidth-constrained scenarios
async fn agentic_schema_compact() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::compact_schema())
}

/// Get capabilities summary
///
/// Quick reference for available operations and their descriptions.
async fn agentic_capabilities() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::capabilities())
}

/// Get provider-specific configuration
///
/// Returns tools, system prompt, and hints optimized for the specified LLM provider.
async fn agentic_provider_config(
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let provider = match provider.to_lowercase().as_str() {
        // OpenAI models (GPT-5, GPT-4, o-series)
        "openai" | "gpt" | "chatgpt" | "gpt-5" | "gpt-5-turbo" | "gpt-4" | "gpt-4o"
        | "gpt-4o-mini" | "o1" | "o1-mini" | "o1-preview" | "o3" | "o3-mini" => LlmProvider::OpenAI,
        // Anthropic models (Claude 4.5, Claude 4, Claude 3.5)
        "anthropic" | "claude" | "claude-4.5" | "claude-4.5-opus" | "claude-4.5-sonnet"
        | "claude-4" | "claude-4-opus" | "claude-4-sonnet" | "claude-3.5" | "claude-3"
        | "sonnet" | "opus" | "haiku" => LlmProvider::Anthropic,
        // Google models (Gemini 2.5, Gemini 2.0, Flash)
        "google" | "gemini" | "gemini-2.5" | "gemini-2.5-pro" | "gemini-2.5-ultra"
        | "gemini-2.0" | "gemini-2.0-pro" | "gemini-2.0-ultra" | "gemini-pro" | "gemini-ultra"
        | "gemini-flash" => LlmProvider::Google,
        "generic" | "other" | "custom" => LlmProvider::Generic,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    Ok(Json(SchemaEndpoints::provider_config(provider)))
}

/// Get tools in OpenAI function calling format
async fn agentic_openai_tools() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::openai_tools())
}

/// Get tools in Anthropic tool use format
async fn agentic_anthropic_tools() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::anthropic_tools())
}

/// Get tools in Google Gemini format
async fn agentic_gemini_tools() -> Json<serde_json::Value> {
    Json(SchemaEndpoints::gemini_tools())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Rate limiter tests
    #[tokio::test]
    async fn test_rate_limiter_window_reset() {
        let limiter = RateLimiter::new(2, Duration::from_millis(50));

        // Exhaust the limit
        assert!(limiter.check("test").await.is_some());
        assert!(limiter.check("test").await.is_some());
        assert!(limiter.check("test").await.is_none());

        // Wait for window to reset
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Should be able to make requests again
        assert!(limiter.check("test").await.is_some());
    }

    #[tokio::test]
    async fn test_rate_limiter_default() {
        let limiter = RateLimiter::default();
        // Default allows 100 requests per minute
        assert_eq!(limiter.max_requests, 100);
        assert_eq!(limiter.window, Duration::from_secs(60));
    }

    // MCP Session tests
    #[test]
    fn test_mcp_session_creation() {
        let now = chrono::Utc::now();
        let session = McpSession {
            agent_id: "test-agent".to_string(),
            created_at: now,
            last_activity: now,
            call_count: 0,
        };
        assert_eq!(session.agent_id, "test-agent");
        assert_eq!(session.call_count, 0);
    }

    // Tool call tests
    #[tokio::test]
    async fn test_tool_call_request_deserialization() {
        let json = r#"{"tool": "vm.create", "arguments": {"name": "test-vm", "cpu_cores": 2}}"#;
        let call: ToolCallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(call.tool, "vm.create");
        assert_eq!(call.arguments["name"], "test-vm");
    }

    #[test]
    fn test_tool_call_response_serialization() {
        let response = ToolCallResponse {
            success: true,
            result: Some(serde_json::json!({"id": "vm-1"})),
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"vm-1\""));
    }

    #[test]
    fn test_tool_call_response_error() {
        let response = ToolCallResponse {
            success: false,
            result: None,
            error: Some("VM not found".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("VM not found"));
    }

    // Schema endpoint tests
    #[test]
    fn test_schema_endpoints_provider_config() {
        let openai = SchemaEndpoints::provider_config(LlmProvider::OpenAI);
        assert!(openai["provider"].as_str().is_some());
        assert!(openai["system_prompt"]
            .as_str()
            .unwrap()
            .contains("HyperMachine"));

        let anthropic = SchemaEndpoints::provider_config(LlmProvider::Anthropic);
        assert!(anthropic["provider"].as_str().is_some());

        let google = SchemaEndpoints::provider_config(LlmProvider::Google);
        assert!(google["provider"].as_str().is_some());

        let generic = SchemaEndpoints::provider_config(LlmProvider::Generic);
        assert!(generic["provider"].as_str().is_some());
    }

    #[test]
    fn test_schema_endpoints_openai_tools() {
        let tools = SchemaEndpoints::openai_tools();
        assert!(tools.as_array().is_some());
        let tools_array = tools.as_array().unwrap();
        assert!(!tools_array.is_empty());

        // All OpenAI tools should have type "function"
        for tool in tools_array {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].as_str().is_some());
            assert!(tool["function"]["description"].as_str().is_some());
        }
    }

    #[test]
    fn test_schema_endpoints_anthropic_tools() {
        let tools = SchemaEndpoints::anthropic_tools();
        assert!(tools.as_array().is_some());
        let tools_array = tools.as_array().unwrap();
        assert!(!tools_array.is_empty());

        // Anthropic tools should have name, description, input_schema
        for tool in tools_array {
            assert!(tool["name"].as_str().is_some());
            assert!(tool["description"].as_str().is_some());
            assert!(tool["input_schema"].is_object());
        }
    }

    #[test]
    fn test_schema_endpoints_gemini_tools() {
        let tools = SchemaEndpoints::gemini_tools();
        assert!(tools.is_object());
        assert!(tools["function_declarations"].as_array().is_some());
        let declarations = tools["function_declarations"].as_array().unwrap();
        assert!(!declarations.is_empty());
        for decl in declarations {
            assert!(decl["name"].as_str().is_some());
            assert!(decl["description"].as_str().is_some());
        }
    }

    // LLM Provider tests
    #[test]
    fn test_llm_provider_display() {
        assert_eq!(format!("{:?}", LlmProvider::OpenAI), "OpenAI");
        assert_eq!(format!("{:?}", LlmProvider::Anthropic), "Anthropic");
        assert_eq!(format!("{:?}", LlmProvider::Google), "Google");
        assert_eq!(format!("{:?}", LlmProvider::Generic), "Generic");
    }

    // Tool definition tests

    #[tokio::test]
    async fn test_rate_limiter_allows_requests_within_limit() {
        let limiter = RateLimiter::new(5, Duration::from_secs(60));

        // First 5 requests should succeed
        for i in 0..5 {
            let result = limiter.check("test-ip").await;
            assert!(result.is_some(), "Request {} should be allowed", i);
        }

        // 6th request should fail
        let result = limiter.check("test-ip").await;
        assert!(result.is_none(), "Request 6 should be rate limited");
    }

    #[tokio::test]
    async fn test_rate_limiter_separate_keys() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));

        // Each key has its own limit
        assert!(limiter.check("ip-1").await.is_some());
        assert!(limiter.check("ip-1").await.is_some());
        assert!(limiter.check("ip-1").await.is_none()); // ip-1 exhausted

        // ip-2 should still have quota
        assert!(limiter.check("ip-2").await.is_some());
        assert!(limiter.check("ip-2").await.is_some());
        assert!(limiter.check("ip-2").await.is_none()); // ip-2 exhausted
    }

    #[tokio::test]
    async fn test_rate_limiter_returns_remaining() {
        let limiter = RateLimiter::new(5, Duration::from_secs(60));

        assert_eq!(limiter.check("test").await, Some(4)); // 5-1=4 remaining
        assert_eq!(limiter.check("test").await, Some(3)); // 5-2=3 remaining
        assert_eq!(limiter.check("test").await, Some(2));
        assert_eq!(limiter.check("test").await, Some(1));
        assert_eq!(limiter.check("test").await, Some(0)); // Last allowed request
        assert!(limiter.check("test").await.is_none()); // Rate limited
    }

    #[test]
    fn test_tool_definitions_are_valid() {
        // Verify list_tools returns properly structured tools
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(async { list_tools().await.0 });

        assert!(!tools.is_empty(), "Should have at least one tool");

        for tool in &tools {
            assert!(!tool.name.is_empty(), "Tool name should not be empty");
            assert!(
                !tool.description.is_empty(),
                "Tool description should not be empty"
            );
            assert!(
                tool.parameters.is_object(),
                "Tool parameters should be an object"
            );
        }

        // Check for expected tools
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"vm.create"));
        assert!(tool_names.contains(&"vm.list"));
        assert!(tool_names.contains(&"vm.start"));
        assert!(tool_names.contains(&"vm.stop"));
        assert!(tool_names.contains(&"vm.delete"));
        assert!(tool_names.contains(&"vm.get"));
        assert!(tool_names.contains(&"vm.metrics"));
        assert!(tool_names.contains(&"vm.execute_script"));
    }

    #[test]
    fn test_vm_create_tool_schema() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(async { list_tools().await.0 });

        let create_tool = tools.iter().find(|t| t.name == "vm.create").unwrap();

        // Verify required fields
        let params = &create_tool.parameters;
        let props = params.get("properties").unwrap();

        assert!(props.get("name").is_some(), "Should have name property");
        assert!(
            props.get("cpu_cores").is_some(),
            "Should have cpu_cores property"
        );
        assert!(
            props.get("memory_gb").is_some(),
            "Should have memory_gb property"
        );

        // Verify 'name' is required
        let required = params.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|v| v == "name"));
    }

    #[test]
    fn test_create_vm_request_defaults() {
        let json = r#"{"name": "test-vm"}"#;
        let req: CreateVmRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "test-vm");
        assert_eq!(req.cpu_cores, 2);
        assert_eq!(req.memory_gb, 4);
        assert!(!req.gpu_enabled);
        assert!(!req.network_enabled);
    }

    #[test]
    fn test_create_vm_request_custom_values() {
        let json = r#"{"name": "big-vm", "cpu_cores": 16, "memory_gb": 64, "gpu_enabled": true, "network_enabled": true}"#;
        let req: CreateVmRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.cpu_cores, 16);
        assert_eq!(req.memory_gb, 64);
        assert!(req.gpu_enabled);
        assert!(req.network_enabled);
    }

    #[test]
    fn test_execute_script_request_default_timeout() {
        let json = r#"{"script": "print('hello')"}"#;
        let req: ExecuteScriptRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.script, "print('hello')");
        assert_eq!(req.timeout_seconds, 300);
    }

    #[test]
    fn test_execute_script_request_custom_timeout() {
        let json = r#"{"script": "long_task()", "timeout_seconds": 600}"#;
        let req: ExecuteScriptRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.timeout_seconds, 600);
    }

    #[test]
    fn test_tool_call_request_with_agent_id() {
        let json = r#"{"tool": "vm.list", "arguments": {}, "agent_id": "agent-007"}"#;
        let req: ToolCallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.agent_id.unwrap(), "agent-007");
    }

    #[test]
    fn test_tool_call_request_without_agent_id() {
        let json = r#"{"tool": "vm.list", "arguments": {}}"#;
        let req: ToolCallRequest = serde_json::from_str(json).unwrap();
        assert!(req.agent_id.is_none());
    }

    #[test]
    fn test_tool_definitions_count() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(async { list_tools().await.0 });
        assert_eq!(tools.len(), 8);
    }

    #[test]
    fn test_tool_definitions_all_have_names_and_descriptions() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(async { list_tools().await.0 });
        for tool in &tools {
            assert!(!tool.name.is_empty(), "Tool should have a name");
            assert!(
                !tool.description.is_empty(),
                "Tool {} should have description",
                tool.name
            );
            assert!(
                tool.parameters.is_object(),
                "Tool {} params should be object",
                tool.name
            );
        }
    }

    #[test]
    fn test_tool_definitions_expected_names() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tools = rt.block_on(async { list_tools().await.0 });
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        for expected in &[
            "vm.create",
            "vm.list",
            "vm.get",
            "vm.start",
            "vm.stop",
            "vm.delete",
            "vm.metrics",
            "vm.execute_script",
        ] {
            assert!(names.contains(expected), "Missing tool: {}", expected);
        }
    }

    #[test]
    fn test_health_check_response() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let json = rt.block_on(async { health_check().await.0 });
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["service"], "hypermachine-mcp");
        assert!(json["version"].is_string());
    }

    #[test]
    fn test_vm_info_serialization() {
        let info = VmInfo {
            name: "test".to_string(),
            cpu_cores: 4,
            memory_gb: 8,
            gpu_enabled: true,
            network_enabled: false,
            state: "Running".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["cpu_cores"], 4);
        assert_eq!(json["state"], "Running");
    }
}
