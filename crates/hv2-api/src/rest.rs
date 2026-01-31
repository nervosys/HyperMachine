//! REST API implementation using Axum

use crate::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use hv2_agent::AgentVM;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Active VMs managed by the API
    vms: Arc<RwLock<HashMap<String, Arc<AgentVM>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// REST API error response
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: String,
}

impl IntoResponse for crate::ApiError {
    fn into_response(self) -> Response {
        let (status, code, error) = match &self {
            crate::ApiError::VmNotFound(msg) => (StatusCode::NOT_FOUND, "VM_NOT_FOUND", msg.clone()),
            crate::ApiError::InvalidRequest(msg) => {
                (StatusCode::BAD_REQUEST, "INVALID_REQUEST", msg.clone())
            }
            crate::ApiError::Transport(msg) => {
                (StatusCode::SERVICE_UNAVAILABLE, "TRANSPORT_ERROR", msg.clone())
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                self.to_string(),
            ),
        };

        let body = Json(ErrorResponse {
            error,
            code: code.to_string(),
        });
        (status, body).into_response()
    }
}

/// Create REST API router
pub fn create_router() -> Router {
    let state = AppState::new();

    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/vms", get(list_vms).post(create_vm))
        .route("/api/v1/vms/{id}", get(get_vm).delete(delete_vm))
        .route("/api/v1/vms/{id}/start", post(start_vm))
        .route("/api/v1/vms/{id}/stop", post(stop_vm))
        .route("/api/v1/vms/{id}/pause", post(pause_vm))
        .route("/api/v1/vms/{id}/resume", post(resume_vm))
        .route("/api/v1/vms/{id}/metrics", get(get_metrics))
        .route("/api/v1/vms/{id}/script", post(execute_script))
        .with_state(Arc::new(state))
}

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
}

async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: 0, // Would track actual server uptime
    })
}

/// VM list response
#[derive(Serialize)]
struct VmListResponse {
    vms: Vec<VmSummary>,
    total: usize,
}

/// VM summary for list view
#[derive(Serialize)]
struct VmSummary {
    id: String,
    name: String,
    state: String,
}

async fn list_vms(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let vms = state.vms.read().await;
    let summaries: Vec<VmSummary> = vms
        .iter()
        .map(|(id, vm)| VmSummary {
            id: id.clone(),
            name: id.clone(),
            state: format!("{:?}", vm.state()),
        })
        .collect();

    let total = summaries.len();
    Json(VmListResponse { vms: summaries, total })
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
            }),
        )
    })?;

    let metrics = vm.get_metrics().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "METRICS_ERROR".to_string(),
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
            }),
        )
    })?;

    vm.start().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "START_FAILED".to_string(),
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
            }),
        )
    })?;

    vm.stop().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "STOP_FAILED".to_string(),
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
            }),
        )
    })?;

    vm.pause().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "PAUSE_FAILED".to_string(),
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
            }),
        )
    })?;

    vm.resume().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "RESUME_FAILED".to_string(),
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
            }),
        )
    })?;

    let metrics = vm.get_metrics().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "METRICS_ERROR".to_string(),
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
            }),
        )
    })?;

    // Suppress unused variable warning
    let _ = req.timeout_seconds;

    match vm.execute_agent_script(&req.script).await {
        Ok(result) => Ok(Json(ExecuteScriptResponse {
            id,
            success: true,
            result: Some(result),
            error: None,
            execution_time_ms: start.elapsed().as_millis() as u64,
        })),
        Err(e) => Ok(Json(ExecuteScriptResponse {
            id,
            success: false,
            result: None,
            error: Some(e.to_string()),
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
