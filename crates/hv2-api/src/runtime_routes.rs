//! Runtime REST API Routes
//!
//! Fleet-level endpoints for the HyperMachine Stateful Runtime Environment.
//! These routes expose session lifecycle, workload scheduling, workflow
//! orchestration, maintenance, and observability operations backed by
//! [`hv2_runtime::Runtime`].
//!
//! ## Route Groups
//!
//! | Prefix                          | Description                      |
//! |---------------------------------|----------------------------------|
//! | `/api/v1/runtime`               | Runtime status & health          |
//! | `/api/v1/runtime/sessions`      | Session lifecycle (create/destroy)|
//! | `/api/v1/runtime/workloads`     | Workload submission & scheduling |
//! | `/api/v1/runtime/workflows`     | DAG workflow orchestration       |
//! | `/api/v1/runtime/maintenance`   | Maintenance tick                 |

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use hv2_runtime::{
    BillingTier, MaintenanceReport, Placement, Runtime, RuntimeConfig, SessionInfo, StepOutcome,
    WorkflowSpec, WorkloadDescriptor, WorkloadResult,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ============================================================================
// Shared State
// ============================================================================

/// Application state wrapping the runtime
#[derive(Clone)]
pub struct RuntimeAppState {
    /// The runtime instance
    pub runtime: Arc<Runtime>,
}

impl RuntimeAppState {
    /// Create a new runtime app state with the given config
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            runtime: Arc::new(Runtime::new(config)),
        }
    }

    /// Create from an existing runtime
    pub fn from_runtime(runtime: Runtime) -> Self {
        Self {
            runtime: Arc::new(runtime),
        }
    }

    /// Create from an existing `Arc<Runtime>`
    pub fn from_runtime_arc(runtime: Arc<Runtime>) -> Self {
        Self { runtime }
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Create session request
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    /// Session ID
    pub session_id: String,
    /// Billing tier (defaults to Standard)
    #[serde(default)]
    pub tier: BillingTier,
}

/// Create session response
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    /// Session info
    pub session: SessionInfo,
}

/// Destroy session response
#[derive(Debug, Serialize, Deserialize)]
pub struct DestroySessionResponse {
    /// Session ID
    pub session_id: String,
    /// Whether a final invoice was generated
    pub invoice_generated: bool,
    /// Total charges (if invoice was generated)
    pub total_charges: Option<f64>,
}

/// Submit workload request — thin wrapper to allow JSON deserialization
/// of the workload descriptor
#[derive(Debug, Deserialize)]
pub struct SubmitWorkloadRequest {
    /// Workload descriptor
    #[serde(flatten)]
    pub workload: WorkloadDescriptor,
}

/// Submit workload response
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitWorkloadResponse {
    /// Workload result
    #[serde(flatten)]
    pub result: WorkloadResult,
}

/// Schedule pending response
#[derive(Debug, Serialize, Deserialize)]
pub struct SchedulePendingResponse {
    /// Number of placements made
    pub placed: usize,
    /// Placement details
    pub placements: Vec<Placement>,
}

/// Run workflow request
#[derive(Debug, Deserialize)]
pub struct RunWorkflowRequest {
    /// Workflow specification
    #[serde(flatten)]
    pub spec: WorkflowSpec,
}

/// Run workflow response
#[derive(Debug, Serialize, Deserialize)]
pub struct RunWorkflowResponse {
    /// Workflow ID
    pub workflow_id: String,
}

/// Advance workflow step request
#[derive(Debug, Deserialize)]
pub struct AdvanceStepRequest {
    /// Step outcome
    #[serde(flatten)]
    pub outcome: StepOutcome,
}

/// Advance workflow step response
#[derive(Debug, Serialize, Deserialize)]
pub struct AdvanceStepResponse {
    /// Next ready steps
    pub ready_steps: Vec<String>,
}

/// Maintenance tick response
#[derive(Debug, Serialize, Deserialize)]
pub struct MaintenanceResponse {
    /// Maintenance report
    #[serde(flatten)]
    pub report: MaintenanceReport,
}

/// Runtime error response
#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeErrorResponse {
    /// Error message
    pub error: String,
    /// Error code
    pub code: String,
}

// ============================================================================
// Router
// ============================================================================

/// Create the runtime API router
///
/// Routes:
/// - `GET  /api/v1/runtime/status` — Runtime status snapshot
/// - `POST /api/v1/runtime/sessions` — Create a session
/// - `DELETE /api/v1/runtime/sessions/:id` — Destroy a session
/// - `POST /api/v1/runtime/workloads` — Submit a workload
/// - `POST /api/v1/runtime/workloads/schedule` — Schedule pending workloads
/// - `POST /api/v1/runtime/workflows` — Run a workflow
/// - `POST /api/v1/runtime/workflows/:id/steps/:step` — Advance a step
/// - `DELETE /api/v1/runtime/workflows/:id` — Cancel a workflow
/// - `POST /api/v1/runtime/maintenance` — Trigger maintenance tick
pub fn create_runtime_router(state: Arc<RuntimeAppState>) -> Router {
    Router::new()
        .route("/api/v1/runtime/status", get(get_runtime_status))
        .route("/api/v1/runtime/health", get(get_runtime_health))
        .route("/api/v1/runtime/metrics", get(get_runtime_metrics))
        .route(
            "/api/v1/runtime/metrics/prometheus",
            get(get_runtime_metrics_prometheus),
        )
        .route("/api/v1/runtime/sessions", post(create_session))
        .route("/api/v1/runtime/sessions/:id", delete(destroy_session))
        .route("/api/v1/runtime/workloads", post(submit_workload))
        .route("/api/v1/runtime/workloads/schedule", post(schedule_pending))
        .route("/api/v1/runtime/workflows", post(run_workflow))
        .route(
            "/api/v1/runtime/workflows/:id/steps/:step",
            post(advance_workflow_step),
        )
        .route("/api/v1/runtime/workflows/:id", delete(cancel_workflow))
        .route("/api/v1/runtime/maintenance", post(maintenance_tick))
        .with_state(state)
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/v1/runtime/status
///
/// Returns a comprehensive runtime status snapshot.
async fn get_runtime_status(State(state): State<Arc<RuntimeAppState>>) -> impl IntoResponse {
    let status = state.runtime.status();
    Json(status)
}

/// GET /api/v1/runtime/health
///
/// Returns runtime health with pool, health-monitor summary, and uptime.
async fn get_runtime_health(State(state): State<Arc<RuntimeAppState>>) -> impl IntoResponse {
    let metrics = state.runtime.collect_metrics();
    let pool_stats = state.runtime.pool().stats();

    let overall_status = if metrics.health_unhealthy > 0 {
        "degraded"
    } else if metrics.pool_failed > 0 {
        "warning"
    } else {
        "healthy"
    };

    Json(RuntimeHealthResponse {
        status: overall_status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: metrics.uptime_seconds,
        instance_id: metrics.instance_id.clone(),
        pool_total: pool_stats.total,
        pool_warm: pool_stats.warm,
        pool_assigned: pool_stats.assigned,
        pool_failed: pool_stats.failed,
        healthy_vms: metrics.health_healthy,
        degraded_vms: metrics.health_degraded,
        unhealthy_vms: metrics.health_unhealthy,
        active_sessions: metrics.billing_sessions,
        pending_workloads: metrics.scheduler_pending,
    })
}

/// Runtime health response
#[derive(Debug, Serialize)]
struct RuntimeHealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
    instance_id: String,
    pool_total: usize,
    pool_warm: usize,
    pool_assigned: usize,
    pool_failed: usize,
    healthy_vms: u64,
    degraded_vms: u64,
    unhealthy_vms: u64,
    active_sessions: u64,
    pending_workloads: u64,
}

/// GET /api/v1/runtime/metrics
///
/// Returns a full metrics snapshot as JSON.
async fn get_runtime_metrics(State(state): State<Arc<RuntimeAppState>>) -> impl IntoResponse {
    let metrics = state.runtime.collect_metrics();
    Json(metrics)
}

/// GET /api/v1/runtime/metrics/prometheus
///
/// Returns metrics in Prometheus text exposition format.
async fn get_runtime_metrics_prometheus(
    State(state): State<Arc<RuntimeAppState>>,
) -> impl IntoResponse {
    let metrics = state.runtime.collect_metrics();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        metrics.to_prometheus(),
    )
}

/// POST /api/v1/runtime/sessions
///
/// Create a new agent session: acquires a VM from the pool, registers
/// with billing, creates a gateway route, and starts health monitoring.
async fn create_session(
    State(state): State<Arc<RuntimeAppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    let session = state
        .runtime
        .create_session(&req.session_id, req.tier)
        .map_err(|e| runtime_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok((StatusCode::CREATED, Json(CreateSessionResponse { session })))
}

/// DELETE /api/v1/runtime/sessions/{id}
///
/// Destroy an agent session: generates a final invoice, removes gateway
/// route, releases the VM back to the pool.
async fn destroy_session(
    State(state): State<Arc<RuntimeAppState>>,
    Path(session_id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    let invoice = state
        .runtime
        .destroy_session(&session_id)
        .map_err(|e| runtime_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (invoice_generated, total_charges) = match &invoice {
        Some(inv) => (true, Some(inv.total())),
        None => (false, None),
    };

    Ok(Json(DestroySessionResponse {
        session_id,
        invoice_generated,
        total_charges,
    }))
}

/// POST /api/v1/runtime/workloads
///
/// Submit a workload for scheduling. Attempts immediate placement.
async fn submit_workload(
    State(state): State<Arc<RuntimeAppState>>,
    Json(req): Json<SubmitWorkloadRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    let result = state
        .runtime
        .submit_workload(req.workload)
        .map_err(|e| runtime_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let status = if result.placed {
        StatusCode::CREATED
    } else {
        StatusCode::ACCEPTED
    };

    Ok((status, Json(SubmitWorkloadResponse { result })))
}

/// POST /api/v1/runtime/workloads/schedule
///
/// Schedule all pending workloads in a batch.
async fn schedule_pending(
    State(state): State<Arc<RuntimeAppState>>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    let placements = state
        .runtime
        .schedule_pending()
        .map_err(|e| runtime_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(SchedulePendingResponse {
        placed: placements.len(),
        placements,
    }))
}

/// POST /api/v1/runtime/workflows
///
/// Submit and start a workflow.
async fn run_workflow(
    State(state): State<Arc<RuntimeAppState>>,
    Json(req): Json<RunWorkflowRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    let workflow_id = state
        .runtime
        .run_workflow(req.spec)
        .map_err(|e| runtime_error(StatusCode::BAD_REQUEST, &e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(RunWorkflowResponse { workflow_id }),
    ))
}

/// POST /api/v1/runtime/workflows/{id}/steps/{step}
///
/// Advance a workflow step with an outcome.
async fn advance_workflow_step(
    State(state): State<Arc<RuntimeAppState>>,
    Path((workflow_id, step_name)): Path<(String, String)>,
    Json(req): Json<AdvanceStepRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    let ready_steps = state
        .runtime
        .advance_workflow_step(&workflow_id, &step_name, req.outcome)
        .map_err(|e| runtime_error(StatusCode::BAD_REQUEST, &e.to_string()))?;

    Ok(Json(AdvanceStepResponse { ready_steps }))
}

/// DELETE /api/v1/runtime/workflows/{id}
///
/// Cancel a running workflow.
async fn cancel_workflow(
    State(state): State<Arc<RuntimeAppState>>,
    Path(workflow_id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<RuntimeErrorResponse>)> {
    state
        .runtime
        .cancel_workflow(&workflow_id)
        .map_err(|e| runtime_error(StatusCode::BAD_REQUEST, &e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/runtime/maintenance
///
/// Trigger a single maintenance tick.
async fn maintenance_tick(State(state): State<Arc<RuntimeAppState>>) -> impl IntoResponse {
    let report = state.runtime.maintenance_tick();
    Json(MaintenanceResponse { report })
}

// ============================================================================
// Helpers
// ============================================================================

fn runtime_error(status: StatusCode, message: &str) -> (StatusCode, Json<RuntimeErrorResponse>) {
    (
        status,
        Json(RuntimeErrorResponse {
            error: message.to_string(),
            code: status_to_code(status),
        }),
    )
}

fn status_to_code(status: StatusCode) -> String {
    match status {
        StatusCode::BAD_REQUEST => "BAD_REQUEST".to_string(),
        StatusCode::NOT_FOUND => "NOT_FOUND".to_string(),
        StatusCode::CONFLICT => "CONFLICT".to_string(),
        StatusCode::INTERNAL_SERVER_ERROR => "INTERNAL_ERROR".to_string(),
        _ => "UNKNOWN".to_string(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use hv2_runtime::{PoolConfig, RuntimeConfig, RuntimeStatus};
    use tower::ServiceExt;

    fn test_state(warm: usize) -> Arc<RuntimeAppState> {
        let config = RuntimeConfig::builder()
            .pool(PoolConfig {
                min_warm: warm,
                max_size: 64,
                ..Default::default()
            })
            .instance_id("test-api-runtime")
            .build();
        let rt = Runtime::new(config);

        // Pre-warm the pool
        for _ in 0..warm {
            let vm_id = rt.pool().provision().unwrap();
            rt.pool().mark_warm(&vm_id).unwrap();
        }

        Arc::new(RuntimeAppState::from_runtime(rt))
    }

    fn test_router(state: Arc<RuntimeAppState>) -> Router {
        create_runtime_router(state)
    }

    #[tokio::test]
    async fn test_get_runtime_status() {
        let state = test_state(2);
        let app = test_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let status: RuntimeStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(status.instance_id, "test-api-runtime");
        assert_eq!(status.pool.total, 2);
    }

    #[tokio::test]
    async fn test_create_session() {
        let state = test_state(2);
        let app = test_router(state);

        let body = serde_json::json!({
            "session_id": "sess-1",
            "tier": "Standard"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: CreateSessionResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.session.session_id, "sess-1");
        assert!(!resp.session.vm_id.is_empty());
    }

    #[tokio::test]
    async fn test_create_session_no_vms() {
        let state = test_state(0);
        let app = test_router(state);

        let body = serde_json::json!({
            "session_id": "sess-fail",
            "tier": "Standard"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_destroy_session() {
        let state = test_state(2);
        let app = test_router(state.clone());

        // Create a session first
        let body = serde_json::json!({
            "session_id": "sess-destroy",
            "tier": "Standard"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Destroy it
        let app = test_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/runtime/sessions/sess-destroy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: DestroySessionResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.session_id, "sess-destroy");
    }

    #[tokio::test]
    async fn test_submit_workload() {
        let state = test_state(2);
        let app = test_router(state);

        let body = serde_json::json!({
            "id": "wl-1",
            "session_id": "sess-1",
            "required_vcpus": 2,
            "required_memory": 536870912_u64,
            "requires_gpu": false,
            "priority": 50,
            "constraints": [],
            "submitted_at": { "secs_since_epoch": 0, "nanos_since_epoch": 0 },
            "placement_timeout": { "secs": 30, "nanos": 0 },
            "attempts": 0
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/workloads")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should be CREATED (placed) or ACCEPTED (queued)
        let status = response.status();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::ACCEPTED,
            "Expected 201 or 202, got {status}"
        );
    }

    #[tokio::test]
    async fn test_schedule_pending() {
        let state = test_state(2);
        let app = test_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/workloads/schedule")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: SchedulePendingResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.placed, 0);
    }

    #[tokio::test]
    async fn test_run_workflow() {
        let state = test_state(2);
        let app = test_router(state);

        let body = serde_json::json!({
            "name": "test-pipeline",
            "description": "test workflow",
            "steps": [
                {
                    "name": "step-1",
                    "description": "do something",
                    "depends_on": [],
                    "timeout": { "secs": 300, "nanos": 0 },
                    "max_retries": 2,
                    "retry_delay": { "secs": 5, "nanos": 0 },
                    "command": "echo hello",
                    "optional": false
                }
            ],
            "timeout": { "secs": 3600, "nanos": 0 },
            "variables": {},
            "max_parallel_steps": 4
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/workflows")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: RunWorkflowResponse = serde_json::from_slice(&body).unwrap();
        assert!(!resp.workflow_id.is_empty());
    }

    #[tokio::test]
    async fn test_cancel_workflow() {
        let state = test_state(2);

        // First, create a workflow
        let spec = serde_json::json!({
            "name": "cancel-me",
            "description": "",
            "steps": [
                {
                    "name": "step-1",
                    "description": "",
                    "depends_on": [],
                    "timeout": { "secs": 300, "nanos": 0 },
                    "max_retries": 0,
                    "retry_delay": { "secs": 5, "nanos": 0 },
                    "command": "echo",
                    "optional": false
                }
            ],
            "timeout": { "secs": 3600, "nanos": 0 },
            "variables": {},
            "max_parallel_steps": 4
        });

        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/workflows")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&spec).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: RunWorkflowResponse = serde_json::from_slice(&body).unwrap();

        // Now cancel it
        let app = test_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/runtime/workflows/{}", resp.workflow_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_advance_workflow_step() {
        let state = test_state(2);

        // Create a workflow with two steps
        let spec = serde_json::json!({
            "name": "advance-test",
            "description": "",
            "steps": [
                {
                    "name": "step-1",
                    "description": "first",
                    "depends_on": [],
                    "timeout": { "secs": 300, "nanos": 0 },
                    "max_retries": 0,
                    "retry_delay": { "secs": 5, "nanos": 0 },
                    "command": "echo",
                    "optional": false
                },
                {
                    "name": "step-2",
                    "description": "second",
                    "depends_on": ["step-1"],
                    "timeout": { "secs": 300, "nanos": 0 },
                    "max_retries": 0,
                    "retry_delay": { "secs": 5, "nanos": 0 },
                    "command": "echo",
                    "optional": false
                }
            ],
            "timeout": { "secs": 3600, "nanos": 0 },
            "variables": {},
            "max_parallel_steps": 4
        });

        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/workflows")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&spec).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: RunWorkflowResponse = serde_json::from_slice(&body).unwrap();
        let wf_id = resp.workflow_id;

        // Start step-1 via workflow engine directly (simulating executor)
        state
            .runtime
            .workflow_engine()
            .start_step(&wf_id, "step-1")
            .unwrap();

        // Advance step-1 via API
        let outcome = serde_json::json!({
            "Success": { "output": "done" }
        });

        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/runtime/workflows/{}/steps/step-1", wf_id))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&outcome).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: AdvanceStepResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.ready_steps.contains(&"step-2".to_string()));
    }

    #[tokio::test]
    async fn test_maintenance_tick() {
        let state = test_state(2);
        let app = test_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/maintenance")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: MaintenanceResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.report.scale_decision.is_some());
    }

    #[tokio::test]
    async fn test_full_session_lifecycle_via_api() {
        let state = test_state(4);

        // 1. Check status (should have 4 warm VMs)
        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let status: RuntimeStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(status.pool.warm, 4);

        // 2. Create a session
        let app = test_router(state.clone());
        let req = serde_json::json!({
            "session_id": "lifecycle-sess",
            "tier": "Premium"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // 3. Run maintenance
        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/maintenance")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // 4. Destroy the session
        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/runtime/sessions/lifecycle-sess")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: DestroySessionResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.session_id, "lifecycle-sess");
    }

    // ── Metrics endpoint tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_metrics_json_endpoint() {
        let state = test_state(2);
        let app = test_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let metrics: hv2_runtime::RuntimeMetrics = serde_json::from_slice(&body).unwrap();
        assert_eq!(metrics.pool_total, 2);
        assert!(metrics.autoscale_enabled);
    }

    #[tokio::test]
    async fn test_metrics_prometheus_endpoint() {
        let state = test_state(2);
        let app = test_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/metrics/prometheus")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("text/plain"));

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("# HELP hm_pool_total_vms"));
        assert!(text.contains("# TYPE hm_pool_total_vms gauge"));
        assert!(text.contains("hm_pool_total_vms 2"));
        assert!(text.contains("hm_autoscale_enabled 1"));
    }

    #[tokio::test]
    async fn test_runtime_health_endpoint() {
        let state = test_state(3);
        let app = test_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(health["status"], "healthy");
        assert_eq!(health["pool_total"], 3);
        assert!(health["version"].as_str().is_some());
        assert!(health["instance_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_metrics_reflect_session_lifecycle() {
        let state = test_state(2);

        // Create a session
        let app = test_router(state.clone());
        let req = serde_json::json!({
            "session_id": "metrics-sess",
            "tier": "Standard"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Check metrics reflect the session
        let app = test_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let metrics: hv2_runtime::RuntimeMetrics = serde_json::from_slice(&body).unwrap();
        assert_eq!(metrics.sessions_created_total, 1);
        assert_eq!(metrics.billing_sessions, 1);
        assert_eq!(metrics.pool_assigned, 1);
    }
}
