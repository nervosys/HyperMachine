//! GPU VM Fabric REST API Routes
//!
//! Exposes the GPU VM Fabric subsystem — topology discovery, fleet
//! management, capacity reservations, and image registry — as REST
//! endpoints under `/api/v1/gpu-fabric/...`.
//!
//! ## Route Groups
//!
//! | Prefix                                     | Description                          |
//! |--------------------------------------------|--------------------------------------|
//! | `/api/v1/gpu-fabric/topology`              | GPU topology & placement             |
//! | `/api/v1/gpu-fabric/fleet`                 | Fleet hosts & rollouts               |
//! | `/api/v1/gpu-fabric/capacity`              | VM classes & reservations            |

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use hv2_runtime::{
    CapacityManager, FleetManager, GpuDevice, GpuInterconnect, GpuRequirements, GpuTopologyMap,
    SlaTier, VmClass,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ============================================================================
// Shared State
// ============================================================================

/// Application state wrapping GPU fabric managers.
#[derive(Clone)]
pub struct GpuFabricAppState {
    pub topology: Arc<parking_lot::RwLock<GpuTopologyMap>>,
    pub fleet: Arc<FleetManager>,
    pub capacity: Arc<CapacityManager>,
}

impl Default for GpuFabricAppState {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuFabricAppState {
    /// Create a fresh (empty) fabric state — useful for tests.
    pub fn new() -> Self {
        Self {
            topology: Arc::new(parking_lot::RwLock::new(GpuTopologyMap::new())),
            fleet: Arc::new(FleetManager::new()),
            capacity: Arc::new(CapacityManager::new()),
        }
    }
}

// ============================================================================
// Request / Response DTOs
// ============================================================================

// ── Topology ──

#[derive(Debug, Serialize)]
pub struct TopologyOverview {
    pub total_devices: usize,
    pub available_devices: usize,
    pub hosts: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddGpuDeviceRequest {
    pub id: String,
    pub host_id: String,
    pub model: String,
    #[serde(default)]
    pub numa_node: u32,
    #[serde(default)]
    pub pci_address: String,
    #[serde(default)]
    pub vram_bytes: u64,
    #[serde(default)]
    pub compute_capability: u32,
}

#[derive(Debug, Deserialize)]
pub struct AddLinkRequest {
    pub from: String,
    pub to: String,
    pub interconnect: String,
    #[serde(default = "default_link_count")]
    pub link_count: u32,
}

fn default_link_count() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
pub struct PlacementRequest {
    pub gpu_count: u32,
    #[serde(default)]
    pub min_vram_bytes: u64,
    #[serde(default)]
    pub min_compute_capability: u32,
    #[serde(default)]
    pub same_numa: bool,
    #[serde(default)]
    pub require_nvlink: bool,
    #[serde(default)]
    pub min_bandwidth_gbps: f64,
}

#[derive(Debug, Serialize)]
pub struct PlacementResponse {
    pub gpu_ids: Vec<String>,
    pub host_id: String,
    pub affinity_score: f64,
    pub aggregate_bandwidth_gbps: f64,
    pub same_numa: bool,
}

// ── Fleet ──

#[derive(Debug, Serialize)]
pub struct FleetOverview {
    pub host_count: usize,
    pub hosts: Vec<FleetHostSummary>,
}

#[derive(Debug, Serialize)]
pub struct FleetHostSummary {
    pub id: String,
    pub healthy: bool,
    pub active_vm_count: u32,
    pub tags: std::collections::HashMap<String, String>,
}

// ── Capacity ──

#[derive(Debug, Deserialize)]
pub struct RegisterVmClassRequest {
    pub name: String,
    pub sla_tier: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_vcpus")]
    pub vcpus: u32,
    #[serde(default)]
    pub memory_bytes: u64,
    #[serde(default)]
    pub gpu_count: u32,
    #[serde(default)]
    pub gpu_model: String,
    /// Structured GPU spec used for topology-aware placement.
    #[serde(default)]
    pub gpu_min_vram_bytes: u64,
    #[serde(default)]
    pub gpu_min_compute_capability: u32,
    #[serde(default)]
    pub gpu_require_nvlink: bool,
    #[serde(default)]
    pub gpu_same_numa: bool,
    #[serde(default)]
    pub gpu_min_bandwidth_gbps: f64,
    #[serde(default)]
    pub dedicated_host: bool,
    #[serde(default)]
    pub rate_per_hour: f64,
    #[serde(default = "default_max_instances")]
    pub max_instances: u32,
}

fn default_vcpus() -> u32 {
    2
}

fn default_max_instances() -> u32 {
    u32::MAX
}

#[derive(Debug, Deserialize)]
pub struct CreateReservationRequest {
    pub tenant_id: String,
    pub vm_class: String,
    pub count: u32,
    pub duration_hours: u64,
}

#[derive(Debug, Serialize)]
pub struct ReservationResponse {
    pub reservation_id: String,
}

#[derive(Debug, Serialize)]
pub struct CapacityOverview {
    pub vm_classes: Vec<VmClassSummary>,
    pub active_reservations: usize,
}

#[derive(Debug, Serialize)]
pub struct VmClassSummary {
    pub name: String,
    pub gpu_count: u32,
    pub max_instances: u32,
}

/// Returned by the class-placement endpoint for a CPU-only class.
#[derive(Debug, Serialize)]
pub struct NoGpuPlacement {
    pub gpu_required: bool,
}

// ── Common ──

#[derive(Debug, Serialize)]
pub struct FabricErrorResponse {
    pub error: String,
    pub code: String,
}

// ============================================================================
// Router
// ============================================================================

/// Create the GPU VM Fabric API router.
pub fn create_gpu_fabric_router(state: Arc<GpuFabricAppState>) -> Router {
    Router::new()
        // Topology
        .route(
            "/api/v1/gpu-fabric/topology",
            get(get_topology_overview),
        )
        .route(
            "/api/v1/gpu-fabric/topology/devices",
            post(add_gpu_device),
        )
        .route(
            "/api/v1/gpu-fabric/topology/links",
            post(add_topology_link),
        )
        .route(
            "/api/v1/gpu-fabric/topology/placement",
            post(find_placement),
        )
        // Fleet
        .route("/api/v1/gpu-fabric/fleet", get(get_fleet_overview))
        // Capacity
        .route(
            "/api/v1/gpu-fabric/capacity",
            get(get_capacity_overview),
        )
        .route(
            "/api/v1/gpu-fabric/capacity/classes",
            post(register_vm_class),
        )
        .route(
            "/api/v1/gpu-fabric/capacity/classes/{name}/placement",
            get(class_placement),
        )
        .route(
            "/api/v1/gpu-fabric/capacity/reservations",
            post(create_reservation),
        )
        .route(
            "/api/v1/gpu-fabric/capacity/reservations/{id}",
            delete(cancel_reservation),
        )
        .with_state(state)
}

// ============================================================================
// Handlers — Topology
// ============================================================================

/// GET /api/v1/gpu-fabric/topology
async fn get_topology_overview(State(state): State<Arc<GpuFabricAppState>>) -> impl IntoResponse {
    let topo = state.topology.read();
    Json(TopologyOverview {
        total_devices: topo.device_count(),
        available_devices: topo.available_count(),
        hosts: topo.hosts(),
    })
}

/// POST /api/v1/gpu-fabric/topology/devices
async fn add_gpu_device(
    State(state): State<Arc<GpuFabricAppState>>,
    Json(req): Json<AddGpuDeviceRequest>,
) -> impl IntoResponse {
    let device = GpuDevice::new(req.id, req.host_id, req.model)
        .numa(req.numa_node)
        .pci(req.pci_address)
        .vram(req.vram_bytes)
        .capability(req.compute_capability);
    state.topology.write().add_device(device);
    StatusCode::CREATED
}

/// POST /api/v1/gpu-fabric/topology/links
async fn add_topology_link(
    State(state): State<Arc<GpuFabricAppState>>,
    Json(req): Json<AddLinkRequest>,
) -> impl IntoResponse {
    let interconnect = match req.interconnect.as_str() {
        "nvlink" => GpuInterconnect::NvLink,
        "nvswitch" => GpuInterconnect::NvSwitch,
        "pcie_peer" => GpuInterconnect::PciePeer,
        "pcie_root" => GpuInterconnect::PcieRoot,
        "cross_numa" => GpuInterconnect::CrossNuma,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(FabricErrorResponse {
                    error: format!("Unknown interconnect type: {}", req.interconnect),
                    code: "INVALID_INTERCONNECT".into(),
                }),
            )
                .into_response();
        }
    };

    state
        .topology
        .write()
        .add_link(&req.from, &req.to, interconnect, req.link_count);
    StatusCode::CREATED.into_response()
}

/// POST /api/v1/gpu-fabric/topology/placement
async fn find_placement(
    State(state): State<Arc<GpuFabricAppState>>,
    Json(req): Json<PlacementRequest>,
) -> impl IntoResponse {
    let requirements = GpuRequirements {
        gpu_count: req.gpu_count,
        min_vram_bytes: req.min_vram_bytes,
        min_compute_capability: req.min_compute_capability,
        same_numa: req.same_numa,
        require_nvlink: req.require_nvlink,
        prefer_colocated: true,
        min_bandwidth_gbps: req.min_bandwidth_gbps,
    };

    let topo = state.topology.read();
    match topo.find_placement(&requirements) {
        Ok(placement) => Json(PlacementResponse {
            gpu_ids: placement.gpu_ids,
            host_id: placement.host_id,
            affinity_score: placement.affinity_score,
            aggregate_bandwidth_gbps: placement.aggregate_bandwidth_gbps,
            same_numa: placement.same_numa,
        })
        .into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(FabricErrorResponse {
                error: e.to_string(),
                code: "PLACEMENT_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

// ============================================================================
// Handlers — Fleet
// ============================================================================

/// GET /api/v1/gpu-fabric/fleet
async fn get_fleet_overview(State(state): State<Arc<GpuFabricAppState>>) -> impl IntoResponse {
    let hosts = state.fleet.list_hosts();
    Json(FleetOverview {
        host_count: hosts.len(),
        hosts: hosts
            .into_iter()
            .map(|h| FleetHostSummary {
                id: h.id,
                healthy: h.healthy,
                active_vm_count: h.active_vm_count,
                tags: h.tags,
            })
            .collect(),
    })
}

// ============================================================================
// Handlers — Capacity
// ============================================================================

/// GET /api/v1/gpu-fabric/capacity
async fn get_capacity_overview(State(state): State<Arc<GpuFabricAppState>>) -> impl IntoResponse {
    let classes = state.capacity.list_classes();
    let reservations = state.capacity.active_reservations();
    Json(CapacityOverview {
        vm_classes: classes
            .into_iter()
            .map(|c| VmClassSummary {
                name: c.name,
                gpu_count: c.gpu_count,
                max_instances: c.max_instances,
            })
            .collect(),
        active_reservations: reservations.len(),
    })
}

/// POST /api/v1/gpu-fabric/capacity/classes
async fn register_vm_class(
    State(state): State<Arc<GpuFabricAppState>>,
    Json(req): Json<RegisterVmClassRequest>,
) -> impl IntoResponse {
    let sla_tier = match req.sla_tier.as_str() {
        "best_effort" => SlaTier::BestEffort,
        "standard" => SlaTier::Standard,
        "premium" => SlaTier::Premium,
        "dedicated" => SlaTier::Dedicated,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(FabricErrorResponse {
                    error: format!("Unknown SLA tier: {}", req.sla_tier),
                    code: "INVALID_SLA_TIER".into(),
                }),
            )
                .into_response();
        }
    };

    let mut vm_class = VmClass::new(req.name, sla_tier)
        .description(req.description)
        .vcpus(req.vcpus)
        .memory(req.memory_bytes)
        .gpus(req.gpu_count, req.gpu_model)
        .gpu_min_vram(req.gpu_min_vram_bytes)
        .gpu_compute_capability(req.gpu_min_compute_capability)
        .gpu_min_bandwidth(req.gpu_min_bandwidth_gbps)
        .rate(req.rate_per_hour)
        .max(req.max_instances);
    if req.gpu_require_nvlink {
        vm_class = vm_class.gpu_nvlink();
    }
    if req.gpu_same_numa {
        vm_class = vm_class.gpu_same_numa_node();
    }
    if req.dedicated_host {
        vm_class = vm_class.dedicated();
    }

    match state.capacity.register_class(vm_class) {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(FabricErrorResponse {
                error: e.to_string(),
                code: "CLASS_REGISTER_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/v1/gpu-fabric/capacity/classes/{name}/placement
///
/// Resolve a registered class's GPU spec against the current fabric topology:
/// `200` with a concrete placement, `200` `{gpu_required:false}` for a CPU-only
/// class, `404` if the class is unknown, or `422` if the fabric cannot satisfy
/// the spec.
async fn class_placement(
    State(state): State<Arc<GpuFabricAppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if state.capacity.get_class(&name).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(FabricErrorResponse {
                error: format!("VM class not found: {name}"),
                code: "CLASS_NOT_FOUND".into(),
            }),
        )
            .into_response();
    }

    let topo = state.topology.read();
    match state.capacity.place_class(&name, &topo) {
        Ok(Some(placement)) => Json(PlacementResponse {
            gpu_ids: placement.gpu_ids,
            host_id: placement.host_id,
            affinity_score: placement.affinity_score,
            aggregate_bandwidth_gbps: placement.aggregate_bandwidth_gbps,
            same_numa: placement.same_numa,
        })
        .into_response(),
        Ok(None) => Json(NoGpuPlacement {
            gpu_required: false,
        })
        .into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(FabricErrorResponse {
                error: e.to_string(),
                code: "PLACEMENT_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/gpu-fabric/capacity/reservations
async fn create_reservation(
    State(state): State<Arc<GpuFabricAppState>>,
    Json(req): Json<CreateReservationRequest>,
) -> impl IntoResponse {
    let duration = std::time::Duration::from_secs(req.duration_hours * 3600);
    match state
        .capacity
        .create_reservation(&req.tenant_id, &req.vm_class, req.count, duration)
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(ReservationResponse { reservation_id: id }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(FabricErrorResponse {
                error: e.to_string(),
                code: "RESERVATION_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/gpu-fabric/capacity/reservations/:id
async fn cancel_reservation(
    State(state): State<Arc<GpuFabricAppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.capacity.cancel_reservation(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(FabricErrorResponse {
                error: e.to_string(),
                code: "RESERVATION_NOT_FOUND".into(),
            }),
        )
            .into_response(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_app() -> Router {
        let state = Arc::new(GpuFabricAppState::new());
        create_gpu_fabric_router(state)
    }

    #[tokio::test]
    async fn topology_overview_empty() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/gpu-fabric/topology")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn fleet_overview_empty() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/gpu-fabric/fleet")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn capacity_overview_empty() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/gpu-fabric/capacity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn add_device_returns_created() {
        let app = test_app();
        let body = serde_json::json!({
            "id": "gpu-0",
            "host_id": "host-1",
            "model": "A100",
            "numa_node": 0,
            "pci_address": "0000:3b:00.0"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/gpu-fabric/topology/devices")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn register_class_returns_created() {
        let app = test_app();
        let body = serde_json::json!({
            "name": "gpu-a100-8x",
            "sla_tier": "premium",
            "vcpus": 64,
            "memory_bytes": 549755813888_u64,
            "gpu_count": 8,
            "gpu_model": "A100",
            "max_instances": 100
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/gpu-fabric/capacity/classes")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn invalid_sla_tier_returns_bad_request() {
        let app = test_app();
        let body = serde_json::json!({
            "name": "bad",
            "sla_tier": "nonexistent"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/gpu-fabric/capacity/classes")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    async fn post_json(app: &Router, uri: &str, body: serde_json::Value) -> StatusCode {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    async fn get_status(app: &Router, uri: &str) -> StatusCode {
        app.clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn class_placement_unknown_returns_404() {
        let app = test_app();
        assert_eq!(
            get_status(&app, "/api/v1/gpu-fabric/capacity/classes/nope/placement").await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn class_placement_cpu_only_returns_ok() {
        let app = test_app();
        assert_eq!(
            post_json(
                &app,
                "/api/v1/gpu-fabric/capacity/classes",
                serde_json::json!({ "name": "cpu-std", "sla_tier": "standard" }),
            )
            .await,
            StatusCode::CREATED
        );
        // CPU-only class needs no GPU placement.
        assert_eq!(
            get_status(
                &app,
                "/api/v1/gpu-fabric/capacity/classes/cpu-std/placement"
            )
            .await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn class_placement_unsatisfiable_returns_422() {
        let app = test_app();
        // A GPU class, but the fabric has no GPUs registered.
        assert_eq!(
            post_json(
                &app,
                "/api/v1/gpu-fabric/capacity/classes",
                serde_json::json!({ "name": "gpu-1x", "sla_tier": "premium", "gpu_count": 1 }),
            )
            .await,
            StatusCode::CREATED
        );
        assert_eq!(
            get_status(&app, "/api/v1/gpu-fabric/capacity/classes/gpu-1x/placement").await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn class_placement_with_device_returns_ok() {
        let app = test_app();
        // Register one GPU in the fabric, then a 1-GPU class — placeable.
        assert_eq!(
            post_json(
                &app,
                "/api/v1/gpu-fabric/topology/devices",
                serde_json::json!({
                    "id": "gpu-0", "host_id": "host-1", "model": "A100",
                    "numa_node": 0, "pci_address": "0000:3b:00.0"
                }),
            )
            .await,
            StatusCode::CREATED
        );
        assert_eq!(
            post_json(
                &app,
                "/api/v1/gpu-fabric/capacity/classes",
                serde_json::json!({ "name": "gpu-1x", "sla_tier": "premium", "gpu_count": 1 }),
            )
            .await,
            StatusCode::CREATED
        );
        assert_eq!(
            get_status(&app, "/api/v1/gpu-fabric/capacity/classes/gpu-1x/placement").await,
            StatusCode::OK
        );
    }
}
