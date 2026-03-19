//! REST API implementation using Axum

use crate::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hv2_agent::AgentVM;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Active VMs managed by the API
    vms: Arc<RwLock<HashMap<String, Arc<AgentVM>>>>,
    /// Server start time for uptime calculation
    start_time: Instant,
    /// Whether the runtime subsystem is enabled
    pub runtime_enabled: bool,
    /// Whether the events subsystem is enabled
    pub events_enabled: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
            runtime_enabled: false,
            events_enabled: false,
        }
    }

    /// Builder-style: set runtime enabled flag
    pub fn with_runtime_enabled(mut self, enabled: bool) -> Self {
        self.runtime_enabled = enabled;
        self
    }

    /// Builder-style: set events enabled flag
    pub fn with_events_enabled(mut self, enabled: bool) -> Self {
        self.events_enabled = enabled;
        self
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified API error response body.
///
/// Used across all API handlers and middleware layers for a consistent
/// JSON error format.  The optional `request_id` field is populated
/// from the `X-Request-Id` header when available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Human-readable error message.
    pub error: String,
    /// Machine-readable error code (e.g. `VM_NOT_FOUND`).
    pub code: String,
    /// Request ID from the `X-Request-Id` header, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl IntoResponse for crate::ApiError {
    fn into_response(self) -> Response {
        let (status, code, error) = match &self {
            crate::ApiError::VmNotFound(msg) => {
                (StatusCode::NOT_FOUND, "VM_NOT_FOUND", msg.clone())
            }
            crate::ApiError::InvalidRequest(msg) => {
                (StatusCode::BAD_REQUEST, "INVALID_REQUEST", msg.clone())
            }
            crate::ApiError::Transport(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "TRANSPORT_ERROR",
                msg.clone(),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                self.to_string(),
            ),
        };

        let body = Json(ErrorResponse {
            error,
            code: code.to_string(),
            request_id: None,
        });
        (status, body).into_response()
    }
}

/// Create REST API router with default state
pub fn create_router() -> Router {
    create_router_with_state(AppState::new())
}

/// Create REST API router with the given application state
///
/// The `state` argument lets callers inject component-awareness (e.g.
/// runtime/events flags) for richer health-check responses.  The
/// convenience [`create_router`] function delegates here with a
/// default `AppState`.
pub fn create_router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/health/live", get(liveness_check))
        .route("/health/ready", get(readiness_check))
        .route("/api/v1/vms", get(list_vms).post(create_vm))
        .route("/api/v1/vms/{id}", get(get_vm).delete(delete_vm))
        .route("/api/v1/vms/{id}/start", post(start_vm))
        .route("/api/v1/vms/{id}/stop", post(stop_vm))
        .route("/api/v1/vms/{id}/pause", post(pause_vm))
        .route("/api/v1/vms/{id}/resume", post(resume_vm))
        .route("/api/v1/vms/{id}/metrics", get(get_metrics))
        .route("/api/v1/vms/{id}/script", post(execute_script))
        // Agentic ontology routes for AI agent discovery
        .merge(crate::ontology::create_ontology_router())
        .with_state(Arc::new(state))
}

// ============================================================================
// Health Check Types & Handlers
// ============================================================================

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
    vm_count: usize,
    components: ComponentStatus,
}

/// Status of server subsystem components
#[derive(Debug, Serialize, Deserialize)]
struct ComponentStatus {
    runtime: String,
    events: String,
}

/// Liveness probe response
#[derive(Debug, Serialize, Deserialize)]
struct LivenessResponse {
    status: String,
}

/// Readiness probe response
#[derive(Debug, Serialize, Deserialize)]
struct ReadinessResponse {
    status: String,
    checks: Vec<ReadinessCheck>,
}

/// Individual readiness check result
#[derive(Debug, Serialize, Deserialize)]
struct ReadinessCheck {
    name: String,
    status: String,
}

/// Full health check — reports uptime, VM count, and component status.
async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    let vm_count = state.vms.read().await.len();

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: uptime,
        vm_count,
        components: ComponentStatus {
            runtime: if state.runtime_enabled {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
            events: if state.events_enabled {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
        },
    })
}

/// Liveness probe — returns 200 if the process is alive.
///
/// Used by orchestrators (e.g. Kubernetes) to detect unresponsive servers.
async fn liveness_check() -> impl IntoResponse {
    Json(LivenessResponse {
        status: "alive".to_string(),
    })
}

/// Readiness probe — returns 200 when the server can accept traffic.
///
/// Reports per-component readiness. Disabled subsystems are marked
/// `"disabled"` rather than `"down"` so operators can distinguish
/// intentional configuration from failures.
async fn readiness_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let checks = vec![
        ReadinessCheck {
            name: "http_server".to_string(),
            status: "up".to_string(),
        },
        ReadinessCheck {
            name: "runtime".to_string(),
            status: if state.runtime_enabled {
                "up".to_string()
            } else {
                "disabled".to_string()
            },
        },
        ReadinessCheck {
            name: "events".to_string(),
            status: if state.events_enabled {
                "up".to_string()
            } else {
                "disabled".to_string()
            },
        },
    ];

    // Server is ready if the HTTP server is up (always true if we're responding)
    let ready = checks.iter().all(|c| c.status != "down");
    let status_code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(ReadinessResponse {
            status: if ready {
                "ready".to_string()
            } else {
                "not_ready".to_string()
            },
            checks,
        }),
    )
}

// ============================================================================
// Pagination
// ============================================================================

/// Default pagination page size.
const DEFAULT_PAGE_SIZE: usize = 20;
/// Maximum allowed page size.
const MAX_PAGE_SIZE: usize = 100;

/// Query parameters for paginated list endpoints.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PaginationParams {
    /// Page offset (0-based, default 0).
    #[serde(default)]
    pub offset: usize,
    /// Maximum items to return (default 20, max 100).
    pub limit: Option<usize>,
}

impl PaginationParams {
    /// Clamp `limit` between 1 and `MAX_PAGE_SIZE`.
    pub fn effective_limit(&self) -> usize {
        self.limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }
}

/// Envelope returned by all paginated list endpoints.
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T: Serialize> {
    /// Page of results.
    pub items: Vec<T>,
    /// Total number of matching items (before pagination).
    pub total: usize,
    /// Offset used for this page.
    pub offset: usize,
    /// Limit used for this page.
    pub limit: usize,
    /// Whether more items exist after this page.
    pub has_more: bool,
}

impl<T: Serialize> PaginatedResponse<T> {
    /// Build a paginated response from a full collection.
    pub fn from_vec(items: Vec<T>, total: usize, offset: usize, limit: usize) -> Self {
        let has_more = offset + items.len() < total;
        Self {
            items,
            total,
            offset,
            limit,
            has_more,
        }
    }
}

/// Query parameters for the VM list endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct VmListParams {
    /// Page offset (0-based, default 0).
    #[serde(default)]
    pub offset: usize,
    /// Maximum items to return (default 20, max 100).
    pub limit: Option<usize>,
    /// Optional state filter (e.g. `Running`, `Paused`).
    pub state: Option<String>,
}

impl VmListParams {
    /// Pagination limit clamped to valid range.
    pub fn effective_limit(&self) -> usize {
        self.limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }
}

/// VM list response
#[derive(Debug, Serialize, Deserialize)]
struct VmListResponse {
    vms: Vec<VmSummary>,
    total: usize,
    offset: usize,
    limit: usize,
    has_more: bool,
}

/// VM summary for list view
#[derive(Debug, Serialize, Deserialize)]
struct VmSummary {
    id: String,
    name: String,
    state: String,
}

async fn list_vms(
    State(state): State<Arc<AppState>>,
    Query(params): Query<VmListParams>,
) -> impl IntoResponse {
    let vms = state.vms.read().await;

    // Collect and optionally filter by state
    let mut summaries: Vec<VmSummary> = vms
        .iter()
        .map(|(id, vm)| VmSummary {
            id: id.clone(),
            name: id.clone(),
            state: format!("{:?}", vm.state()),
        })
        .filter(|s| {
            if let Some(ref filter) = params.state {
                s.state.eq_ignore_ascii_case(filter)
            } else {
                true
            }
        })
        .collect();

    // Sort for deterministic pagination
    summaries.sort_by(|a, b| a.id.cmp(&b.id));

    let total = summaries.len();
    let limit = params.effective_limit();
    let offset = params.offset.min(total);
    let page: Vec<VmSummary> = summaries.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + page.len() < total;

    Json(VmListResponse {
        vms: page,
        total,
        offset,
        limit,
        has_more,
    })
}

/// VM creation request
#[derive(Deserialize)]
struct CreateVMRequest {
    name: String,
    #[serde(default = "default_vcpu_count")]
    vcpu_count: u32,
    #[serde(default = "default_memory_gb")]
    memory_gb: u64,
    #[serde(default)]
    enable_gpu: bool,
    #[serde(default)]
    enable_networking: bool,
}

fn default_vcpu_count() -> u32 {
    2
}

fn default_memory_gb() -> u64 {
    4
}

/// VM creation response
#[derive(Serialize)]
struct CreateVMResponse {
    id: String,
    name: String,
    state: String,
    vcpu_count: u32,
    memory_gb: u64,
}

async fn create_vm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateVMRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // Build the VM
    let vm = AgentVM::builder()
        .name(&req.name)
        .cpu_cores(req.vcpu_count)
        .memory_gb(req.memory_gb)
        .enable_gpu(req.enable_gpu)
        .enable_networking(req.enable_networking)
        .build()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "VM_CREATE_FAILED".to_string(),
                    request_id: None,
                }),
            )
        })?;

    let id = req.name.clone();
    let response = CreateVMResponse {
        id: id.clone(),
        name: req.name,
        state: format!("{:?}", vm.state()),
        vcpu_count: req.vcpu_count,
        memory_gb: req.memory_gb,
    };

    // Store the VM
    state.vms.write().await.insert(id, Arc::new(vm));

    Ok((StatusCode::CREATED, Json(response)))
}

/// VM details response
#[derive(Serialize)]
struct VmDetailsResponse {
    id: String,
    state: String,
    vcpu_count: u32,
    memory_size: u64,
    uptime_seconds: u64,
}

async fn get_vm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    let metrics = vm.get_metrics().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "METRICS_ERROR".to_string(),
                request_id: None,
            }),
        )
    })?;

    Ok(Json(VmDetailsResponse {
        id,
        state: format!("{:?}", metrics.state),
        vcpu_count: metrics.vcpu_count,
        memory_size: metrics.memory_size,
        uptime_seconds: metrics.uptime_seconds,
    }))
}

/// Delete VM response
#[derive(Serialize)]
struct DeleteVMResponse {
    id: String,
    deleted: bool,
}

async fn delete_vm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let mut vms = state.vms.write().await;

    if vms.remove(&id).is_some() {
        Ok(Json(DeleteVMResponse { id, deleted: true }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        ))
    }
}

/// Operation result response
#[derive(Serialize)]
struct OperationResponse {
    id: String,
    operation: String,
    success: bool,
    new_state: String,
}

async fn start_vm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    vm.start().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "START_FAILED".to_string(),
                request_id: None,
            }),
        )
    })?;

    Ok(Json(OperationResponse {
        id,
        operation: "start".to_string(),
        success: true,
        new_state: format!("{:?}", vm.state()),
    }))
}

async fn stop_vm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    vm.stop().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "STOP_FAILED".to_string(),
                request_id: None,
            }),
        )
    })?;

    Ok(Json(OperationResponse {
        id,
        operation: "stop".to_string(),
        success: true,
        new_state: format!("{:?}", vm.state()),
    }))
}

async fn pause_vm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    vm.pause().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "PAUSE_FAILED".to_string(),
                request_id: None,
            }),
        )
    })?;

    Ok(Json(OperationResponse {
        id,
        operation: "pause".to_string(),
        success: true,
        new_state: format!("{:?}", vm.state()),
    }))
}

async fn resume_vm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    vm.resume().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "RESUME_FAILED".to_string(),
                request_id: None,
            }),
        )
    })?;

    Ok(Json(OperationResponse {
        id,
        operation: "resume".to_string(),
        success: true,
        new_state: format!("{:?}", vm.state()),
    }))
}

/// Metrics response
#[derive(Serialize)]
struct MetricsResponse {
    id: String,
    state: String,
    vcpu_count: u32,
    memory_size: u64,
    uptime_seconds: u64,
}

async fn get_metrics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    let metrics = vm.get_metrics().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "METRICS_ERROR".to_string(),
                request_id: None,
            }),
        )
    })?;

    Ok(Json(MetricsResponse {
        id,
        state: format!("{:?}", metrics.state),
        vcpu_count: metrics.vcpu_count,
        memory_size: metrics.memory_size,
        uptime_seconds: metrics.uptime_seconds,
    }))
}

/// Script execution request
#[derive(Deserialize)]
struct ExecuteScriptRequest {
    script: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

/// Script execution response
#[derive(Serialize)]
struct ExecuteScriptResponse {
    id: String,
    success: bool,
    result: Option<serde_json::Value>,
    error: Option<String>,
    execution_time_ms: u64,
}

async fn execute_script(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteScriptRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let start = std::time::Instant::now();

    let vms = state.vms.read().await;
    let vm = vms.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("VM not found: {}", id),
                code: "VM_NOT_FOUND".to_string(),
                request_id: None,
            }),
        )
    })?;

    // Apply timeout if specified (default: 30 seconds)
    let timeout = std::time::Duration::from_secs(req.timeout_seconds.unwrap_or(30));

    let script_future = vm.execute_agent_script(&req.script);
    match tokio::time::timeout(timeout, script_future).await {
        Ok(Ok(result)) => Ok(Json(ExecuteScriptResponse {
            id,
            success: true,
            result: Some(result),
            error: None,
            execution_time_ms: start.elapsed().as_millis() as u64,
        })),
        Ok(Err(e)) => Ok(Json(ExecuteScriptResponse {
            id,
            success: false,
            result: None,
            error: Some(e.to_string()),
            execution_time_ms: start.elapsed().as_millis() as u64,
        })),
        Err(_) => Ok(Json(ExecuteScriptResponse {
            id,
            success: false,
            result: None,
            error: Some(format!(
                "Script execution timed out after {}s",
                timeout.as_secs()
            )),
            execution_time_ms: start.elapsed().as_millis() as u64,
        })),
    }
}

/// Start REST API server
pub async fn serve(addr: impl Into<std::net::SocketAddr>) -> Result<()> {
    let addr = addr.into();
    let app = create_router();

    tracing::info!("Starting REST API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| crate::ApiError::Transport(e.to_string()))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| crate::ApiError::Transport(e.to_string()))?;

    Ok(())
}
