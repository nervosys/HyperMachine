//! REST API implementation using Axum

use crate::Result;
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    // Add shared state here
}

/// REST API error response
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl IntoResponse for crate::ApiError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            crate::ApiError::VmNotFound(msg) => (StatusCode::NOT_FOUND, msg),
            crate::ApiError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(ErrorResponse { error });
        (status, body).into_response()
    }
}

/// Create REST API router
pub fn create_router() -> Router {
    let state = AppState {};

    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/vms", get(list_vms).post(create_vm))
        .route("/api/v1/vms/:id", get(get_vm))
        .route("/api/v1/vms/:id/start", post(start_vm))
        .route("/api/v1/vms/:id/stop", post(stop_vm))
        .route("/api/v1/vms/:id/pause", post(pause_vm))
        .route("/api/v1/vms/:id/resume", post(resume_vm))
        .route("/api/v1/vms/:id/script", post(execute_script))
        .with_state(Arc::new(state))
}

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn list_vms() -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({ "vms": [] }))
}

#[derive(Deserialize)]
struct CreateVMRequest {
    name: String,
    vcpu_count: u32,
    memory_gb: u64,
    enable_gpu: bool,
}

async fn create_vm(Json(req): Json<CreateVMRequest>) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": "example-id",
        "name": req.name,
    }))
}

async fn get_vm(Path(id): Path<String>) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": id,
        "state": "running",
    }))
}

async fn start_vm(Path(id): Path<String>) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": id,
        "success": true,
    }))
}

async fn stop_vm(Path(id): Path<String>) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": id,
        "success": true,
    }))
}

async fn pause_vm(Path(id): Path<String>) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": id,
        "success": true,
    }))
}

async fn resume_vm(Path(id): Path<String>) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": id,
        "success": true,
    }))
}

#[derive(Deserialize)]
struct ExecuteScriptRequest {
    script: String,
}

async fn execute_script(
    Path(id): Path<String>,
    Json(_req): Json<ExecuteScriptRequest>,
) -> impl IntoResponse {
    // TODO: Implement
    Json(serde_json::json!({
        "vm_id": id,
        "result": "script executed",
    }))
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
