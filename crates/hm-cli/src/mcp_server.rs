//! MCP HTTP Server for AI Agent Access
//!
//! Provides a JSON-over-HTTP API for AI agents to manage VMs using the
//! Model Context Protocol pattern. Compatible with OpenAI function calling
//! and Anthropic tool use.

use crate::vm_manager::{VmManager, VmMetrics, VmState};
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

    /// Check if request is allowed, returns remaining quota
    pub async fn check(&self, key: &str) -> Result<u64, ()> {
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
            Err(())
        } else {
            Ok(self.max_requests - current - 1)
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
        Ok(_remaining) => Ok(next.run(request).await),
        Err(_) => Err(StatusCode::TOO_MANY_REQUESTS),
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
        .route("/vms/:name", delete(delete_vm))
        .route("/vms/:name/start", post(start_vm))
        .route("/vms/:name/stop", post(stop_vm))
        .route("/vms/:name/script", post(execute_script))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/mcp/tools", get(list_tools))
        .route("/vms", get(list_vms))
        .route("/vms/:name", get(get_vm))
        .route("/vms/:name/metrics", get(get_vm_metrics))
        .route("/health", get(health_check));

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
            description: "Execute a script inside a running VM".to_string(),
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
                .create_vm(
                    &args.name,
                    args.cpu_cores,
                    args.memory_gb,
                    args.gpu_enabled,
                    args.network_enabled,
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
        .create_vm(
            &request.name,
            request.cpu_cores,
            request.memory_gb,
            request.gpu_enabled,
            request.network_enabled,
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
