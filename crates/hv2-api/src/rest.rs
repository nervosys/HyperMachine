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
use hv2_core::BootSource;
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
    /// Image allowlist applied to every VM this API creates.
    ///
    /// `None` — the default — admits any readable boot image. Share the same
    /// registry the `/api/v1/images/*` routes serve and approving, denying, or
    /// revoking an image there decides whether a VM can boot it, instead of
    /// only answering questions about it.
    image_registry: Option<Arc<hv2_core::security::image_registry::ImageRegistry>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
            runtime_enabled: false,
            events_enabled: false,
            image_registry: None,
        }
    }

    /// Builder-style: gate this API's VMs on an image allowlist.
    ///
    /// Pass the registry the image-registry routes were built with, so the two
    /// cannot disagree about which images are approved.
    pub fn with_image_registry(
        mut self,
        registry: Arc<hv2_core::security::image_registry::ImageRegistry>,
    ) -> Self {
        self.image_registry = Some(registry);
        self
    }

    /// Apply the installed allowlist, if any, to a newly built VM.
    fn gate_images(&self, vm: &AgentVM) {
        if let Some(registry) = &self.image_registry {
            vm.vm().set_image_registry(Arc::clone(registry));
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

/// Exposes the REST server's VM inventory as a [`VmHost`].
///
/// The ontology's plan executor and the `/api/v1/vms` endpoints have to act on
/// the *same* VMs — a plan that creates a VM the REST API cannot see, or stops
/// one that isn't the one it named, would be worse than no plan execution at
/// all. Rather than duplicate the inventory, this adapter hands the executor
/// the map the handlers already use.
///
/// VMs are keyed by name, matching `POST /api/v1/vms`.
struct ApiVmHost {
    vms: Arc<RwLock<HashMap<String, Arc<AgentVM>>>>,
    /// The same allowlist `AppState` applies, so a VM created by a plan is
    /// gated exactly as one created through `POST /api/v1/vms`.
    image_registry: Option<Arc<hv2_core::security::image_registry::ImageRegistry>>,
}

impl ApiVmHost {
    async fn describe(&self, id: &str) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        let vms = self.vms.read().await;
        let vm = vms.get(id).ok_or_else(|| format!("VM not found: {id}"))?;
        let config = vm.vm().config().clone();

        Ok(hv2_agent::VmDescriptor {
            vm_id: id.to_string(),
            name: config.name,
            cpu_cores: config.vcpu_count,
            // The REST API speaks gigabytes; the VM config stores bytes.
            memory_gb: config.memory_size / (1024 * 1024 * 1024),
            status: format!("{:?}", vm.state()).to_lowercase(),
            boot_protocol: config.boot.as_ref().map(|b| b.protocol().to_string()),
        })
    }

    async fn get(&self, id: &str) -> std::result::Result<Arc<AgentVM>, String> {
        self.vms
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| format!("VM not found: {id}"))
    }
}

#[async_trait::async_trait]
impl hv2_agent::VmHost for ApiVmHost {
    async fn create(
        &self,
        spec: hv2_agent::VmSpec,
    ) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        if self.vms.read().await.contains_key(&spec.name) {
            return Err(format!("VM already exists: {}", spec.name));
        }
        if let Some(source) = &spec.boot {
            source.load().map_err(|e| e.to_string())?;
        }

        let mut builder = AgentVM::builder()
            .name(&spec.name)
            .cpu_cores(spec.cpu_cores)
            .memory_gb(spec.memory_gb)
            .enable_gpu(spec.enable_gpu)
            .enable_networking(spec.enable_networking);
        if let Some(source) = spec.boot.clone() {
            builder = builder.boot(source);
        }

        let vm = builder.build().await.map_err(|e| e.to_string())?;
        if let Some(registry) = &self.image_registry {
            vm.vm().set_image_registry(Arc::clone(registry));
        }
        let id = spec.name.clone();
        self.vms.write().await.insert(id.clone(), Arc::new(vm));

        self.describe(&id).await
    }

    async fn start(&self, vm_id: &str) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        let vm = self.get(vm_id).await?;
        let started = if vm.has_boot_source() {
            vm.launch().await
        } else {
            vm.start().await
        };
        started.map_err(|e| e.to_string())?;
        self.describe(vm_id).await
    }

    async fn stop(
        &self,
        vm_id: &str,
        _force: bool,
    ) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        self.get(vm_id)
            .await?
            .stop()
            .await
            .map_err(|e| e.to_string())?;
        self.describe(vm_id).await
    }

    async fn pause(&self, vm_id: &str) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        self.get(vm_id)
            .await?
            .pause()
            .await
            .map_err(|e| e.to_string())?;
        self.describe(vm_id).await
    }

    async fn resume(&self, vm_id: &str) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        self.get(vm_id)
            .await?
            .resume()
            .await
            .map_err(|e| e.to_string())?;
        self.describe(vm_id).await
    }

    async fn delete(&self, vm_id: &str) -> std::result::Result<(), String> {
        // Stop first: dropping a running VM would strand its backend partition
        // and execution loop.
        if let Ok(vm) = self.get(vm_id).await {
            let _ = vm.stop().await;
        }
        self.vms
            .write()
            .await
            .remove(vm_id)
            .map(|_| ())
            .ok_or_else(|| format!("VM not found: {vm_id}"))
    }

    async fn status(&self, vm_id: &str) -> std::result::Result<hv2_agent::VmDescriptor, String> {
        self.describe(vm_id).await
    }

    async fn list(&self) -> std::result::Result<Vec<hv2_agent::VmDescriptor>, String> {
        let ids: Vec<String> = self.vms.read().await.keys().cloned().collect();
        let mut descriptors = Vec::with_capacity(ids.len());
        for id in ids {
            descriptors.push(self.describe(&id).await?);
        }
        Ok(descriptors)
    }

    async fn metrics(&self, vm_id: &str) -> std::result::Result<hv2_agent::VmMetrics, String> {
        // The same telemetry `GET /api/v1/vms/{id}/metrics` serves, so a plan
        // step and a direct request cannot disagree about a VM.
        let descriptor = self.describe(vm_id).await?;
        let measured = self
            .get(vm_id)
            .await?
            .get_metrics()
            .await
            .map_err(|e| e.to_string())?;

        Ok(hv2_agent::VmMetrics {
            vm_id: descriptor.vm_id,
            status: descriptor.status,
            vcpu_count: measured.vcpu_count,
            memory_total_bytes: measured.memory_size,
            uptime_seconds: Some(measured.uptime_seconds),
            cpu_usage_percent: measured.cpu_usage_percent,
            memory_used_bytes: measured.memory_used_bytes,
        })
    }

    async fn console(&self, vm_id: &str) -> std::result::Result<hv2_agent::VmConsole, String> {
        // Same buffer `GET /api/v1/vms/{id}/console` serves, for the same
        // reason: an agent following a plan and an operator watching the boot
        // log must not see different consoles.
        let descriptor = self.describe(vm_id).await?;
        let output = self.get(vm_id).await?.console_output().await;

        Ok(hv2_agent::VmConsole {
            vm_id: descriptor.vm_id,
            attached: output.is_some(),
            output: output.unwrap_or_default(),
        })
    }

    async fn exec(
        &self,
        vm_id: &str,
        command: hv2_agent::vm_host::GuestCommand,
    ) -> std::result::Result<hv2_agent::vm_host::VmExec, String> {
        // The same guest `POST /api/v1/vms/{id}/exec` reaches, for the same
        // reason console is shared: an agent following a plan and an operator
        // with curl must not be running commands in different places.
        let descriptor = self.describe(vm_id).await?;
        let out = self
            .get(vm_id)
            .await?
            .exec_in_guest(
                &command.program,
                &command.args,
                std::time::Duration::from_secs(command.timeout_seconds),
            )
            .await
            .map_err(|e| e.to_string())?;

        Ok(hv2_agent::vm_host::VmExec {
            vm_id: descriptor.vm_id,
            exit_code: out.exit_code,
            signal: out.signal,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
            timed_out: out.timed_out,
        })
    }
}

/// Create REST API router with the given application state
///
/// The `state` argument lets callers inject component-awareness (e.g.
/// runtime/events flags) for richer health-check responses.  The
/// convenience [`create_router`] function delegates here with a
/// default `AppState`.
pub fn create_router_with_state(state: AppState) -> Router {
    // Plan execution acts on the same VM inventory the REST handlers use, so
    // `/agentic/plans/execute` is a control plane over this server's VMs rather
    // than a rehearsal against nothing.
    let plan_executor = Arc::new(crate::ontology::VmHostExecutor::new(Arc::new(ApiVmHost {
        vms: Arc::clone(&state.vms),
        image_registry: state.image_registry.clone(),
    })));

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
        .route("/api/v1/vms/{id}/console", get(get_console))
        .route("/api/v1/vms/{id}/script", post(execute_script))
        .route("/api/v1/vms/{id}/exec", post(exec_in_guest))
        // Agentic ontology routes for AI agent discovery
        .merge(crate::ontology::create_ontology_router_with_executor(
            plan_executor,
        ))
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
    /// What this VM boots. Omit it to create a VM with no guest code — it can
    /// be started, but nothing will execute until something loads guest memory.
    #[serde(default)]
    boot: Option<BootSource>,
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
    // Reject an unusable boot image before anything is allocated, and report it
    // as the client error it is rather than a 500.
    if let Some(source) = &req.boot {
        source.load().map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "INVALID_BOOT_SOURCE".to_string(),
                    request_id: None,
                }),
            )
        })?;
    }

    // Build the VM
    let mut builder = AgentVM::builder()
        .name(&req.name)
        .cpu_cores(req.vcpu_count)
        .memory_gb(req.memory_gb)
        .enable_gpu(req.enable_gpu)
        .enable_networking(req.enable_networking);

    if let Some(source) = req.boot.clone() {
        builder = builder.boot(source);
    }

    let vm = builder.build().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "VM_CREATE_FAILED".to_string(),
                request_id: None,
            }),
        )
    })?;

    state.gate_images(&vm);

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

    // A VM with a boot source is launched — provisioned on the hypervisor
    // backend, its images loaded, and its guest running. One without has no
    // guest code, so it only transitions state.
    let started = if vm.has_boot_source() {
        vm.launch().await
    } else {
        vm.start().await
    };

    started.map_err(|e| {
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

/// Guest console output.
///
/// `attached` distinguishes a VM with no console device from one whose guest
/// has printed nothing — both have an empty `output`, and a caller debugging a
/// silent boot needs to know which it is looking at.
#[derive(Serialize)]
struct ConsoleResponse {
    id: String,
    attached: bool,
    output: String,
}

/// Read a guest's console output without consuming it.
///
/// Polling is the expected access pattern, so this does not drain: each call
/// returns the whole buffer, which is capped at 1 MiB per device with the
/// oldest bytes dropped first.
async fn get_console(
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

    let output = vm.console_output().await;

    Ok(Json(ConsoleResponse {
        id,
        attached: output.is_some(),
        output: output.unwrap_or_default(),
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

/// A program to run inside a guest.
#[derive(Deserialize)]
struct ExecRequest {
    /// Program to execute, run directly rather than through a shell.
    program: String,
    /// Arguments, already split. Nothing here parses a command line.
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

/// What a program did inside a guest.
///
/// `exit_code` and `signal` are separate fields on purpose: a program killed
/// by a signal did not exit 0, and a response that carried one number for both
/// would report a crash as a success.
#[derive(Serialize)]
struct ExecResponse {
    id: String,
    exit_code: Option<i32>,
    signal: Option<i32>,
    stdout: String,
    stderr: String,
    /// Whether output was cut short at the guest agent's per-stream ceiling.
    truncated: bool,
    /// Whether the guest agent killed the program for overrunning.
    timed_out: bool,
}

/// `POST /api/v1/vms/{id}/exec` — run a program inside the guest.
///
/// The counterpart to `/script`, and the difference between them is the whole
/// point: `/script` evaluates a Rhai script on the host against a read-only
/// view of the VM, and this runs a program inside the guest operating system
/// through `hv2-guest-agentd` over vsock.
///
/// A non-zero exit code is a 200 with that code, not an HTTP error: the
/// program ran, and its output is what explains the failure. A 4xx or 5xx here
/// means the command could not be run at all — no guest channel attached, or
/// nothing answering inside the guest.
async fn exec_in_guest(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if req.program.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "program must not be empty".to_string(),
                code: "INVALID_REQUEST".to_string(),
                request_id: None,
            }),
        ));
    }

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

    let timeout = std::time::Duration::from_secs(req.timeout_seconds.unwrap_or(30).max(1));

    match vm.exec_in_guest(&req.program, &req.args, timeout).await {
        Ok(out) => Ok(Json(ExecResponse {
            id,
            exit_code: out.exit_code,
            signal: out.signal,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
            timed_out: out.timed_out,
        })),
        // Not being able to reach a guest is a server-side condition, not a
        // malformed request. The message names which of the several ways it
        // failed, because they send an operator to different places.
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "GUEST_UNAVAILABLE".to_string(),
                request_id: None,
            }),
        )),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert!(!state.runtime_enabled);
        assert!(!state.events_enabled);
    }

    #[test]
    fn test_app_state_builder() {
        let state = AppState::new()
            .with_runtime_enabled(true)
            .with_events_enabled(true);
        assert!(state.runtime_enabled);
        assert!(state.events_enabled);
    }

    #[test]
    fn test_pagination_params_default() {
        let params = PaginationParams::default();
        assert_eq!(params.offset, 0);
        assert!(params.limit.is_none());
        assert_eq!(params.effective_limit(), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn test_pagination_params_clamp_max() {
        let params = PaginationParams {
            offset: 0,
            limit: Some(999),
        };
        assert_eq!(params.effective_limit(), MAX_PAGE_SIZE);
    }

    #[test]
    fn test_pagination_params_clamp_min() {
        let params = PaginationParams {
            offset: 0,
            limit: Some(0),
        };
        assert_eq!(params.effective_limit(), 1);
    }

    #[test]
    fn test_paginated_response_from_vec() {
        let items = vec![1, 2, 3];
        let resp = PaginatedResponse::from_vec(items, 10, 0, 3);
        assert_eq!(resp.items.len(), 3);
        assert_eq!(resp.total, 10);
        assert_eq!(resp.offset, 0);
        assert_eq!(resp.limit, 3);
        assert!(resp.has_more);
    }

    #[test]
    fn test_paginated_response_no_more() {
        let items = vec![1, 2, 3];
        let resp = PaginatedResponse::from_vec(items, 3, 0, 10);
        assert!(!resp.has_more);
    }

    #[test]
    fn test_paginated_response_with_offset() {
        let items = vec![4, 5];
        let resp = PaginatedResponse::from_vec(items, 5, 3, 2);
        assert!(!resp.has_more); // 3 + 2 = 5 = total
        assert_eq!(resp.offset, 3);
    }

    #[test]
    fn test_vm_list_params_effective_limit() {
        let params = VmListParams {
            offset: 0,
            limit: Some(50),
            state: None,
        };
        assert_eq!(params.effective_limit(), 50);
    }

    #[test]
    fn test_error_response_serialization() {
        let err = ErrorResponse {
            error: "not found".into(),
            code: "VM_NOT_FOUND".into(),
            request_id: None,
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["error"], "not found");
        assert_eq!(json["code"], "VM_NOT_FOUND");
        // request_id should be skipped when None
        assert!(json.get("request_id").is_none());
    }

    #[test]
    fn test_error_response_with_request_id() {
        let err = ErrorResponse {
            error: "bad".into(),
            code: "INVALID".into(),
            request_id: Some("req-123".into()),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["request_id"], "req-123");
    }

    #[test]
    fn test_create_router_returns_router() {
        let _router = create_router();
    }

    #[test]
    fn test_create_router_with_custom_state() {
        let state = AppState::new().with_runtime_enabled(true);
        let _router = create_router_with_state(state);
    }

    #[test]
    fn test_paginated_response_empty() {
        let items: Vec<String> = vec![];
        let resp = PaginatedResponse::from_vec(items, 0, 0, 20);
        assert!(resp.items.is_empty());
        assert_eq!(resp.total, 0);
        assert!(!resp.has_more);
    }
}
