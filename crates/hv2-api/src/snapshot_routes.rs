//! Snapshot/Restore REST API
//!
//! Provides REST endpoints for VM snapshot lifecycle management:
//!
//! | Method | Path                                             | Description          |
//! |--------|--------------------------------------------------|----------------------|
//! | POST   | `/api/v1/vms/:id/snapshots`                      | Create snapshot      |
//! | GET    | `/api/v1/vms/:id/snapshots`                      | List snapshots       |
//! | GET    | `/api/v1/vms/:id/snapshots/:snap_id`             | Get snapshot info    |
//! | DELETE | `/api/v1/vms/:id/snapshots/:snap_id`             | Delete snapshot      |
//! | POST   | `/api/v1/vms/:id/snapshots/:snap_id/restore`     | Restore from snap    |
//!
//! These endpoints delegate to the [`SnapshotManager`] in `hv2-core`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use hv2_core::{
    CreateSnapshotOptions, SnapshotConfig, SnapshotId, SnapshotManager,
    SnapshotType,
};

// ============================================================================
// State
// ============================================================================

/// Shared state for snapshot routes
pub struct SnapshotAppState {
    /// Per-VM snapshot managers (vm_id → manager)
    pub managers: RwLock<std::collections::HashMap<String, SnapshotManager>>,
    /// Default config
    pub default_config: SnapshotConfig,
}

impl Default for SnapshotAppState {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotAppState {
    /// Create default state
    pub fn new() -> Self {
        Self {
            managers: RwLock::new(std::collections::HashMap::new()),
            default_config: SnapshotConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: SnapshotConfig) -> Self {
        Self {
            managers: RwLock::new(std::collections::HashMap::new()),
            default_config: config,
        }
    }

    /// Get or create a snapshot manager for a VM
    fn ensure_manager(&self, vm_id: &str) {
        let mut managers = self.managers.write();
        if !managers.contains_key(vm_id) {
            managers.insert(
                vm_id.to_string(),
                SnapshotManager::new(self.default_config.clone()),
            );
        }
    }
}

// ============================================================================
// DTOs
// ============================================================================

/// Create snapshot request body
#[derive(Debug, Deserialize)]
pub struct CreateSnapshotRequest {
    /// Snapshot name (optional, auto-generated if omitted)
    pub name: Option<String>,
    /// Snapshot description
    pub description: Option<String>,
    /// Tags (key-value pairs)
    #[serde(default)]
    pub tags: Vec<String>,
    /// Snapshot type (full, incremental, memory_only, checkpoint)
    #[serde(default = "default_snapshot_type")]
    pub snapshot_type: String,
    /// Parent snapshot ID (for incremental)
    pub parent_id: Option<u64>,
}

fn default_snapshot_type() -> String {
    "full".into()
}

/// Snapshot info response
#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotResponse {
    /// Snapshot ID
    pub id: u64,
    /// Name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// State
    pub state: String,
    /// Type
    pub snapshot_type: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// vCPU count
    pub vcpu_count: u32,
    /// Created timestamp
    pub created_at: String,
    /// Tags
    pub tags: Vec<String>,
    /// Parent ID (if incremental)
    pub parent_id: Option<u64>,
}

/// List snapshots response
#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotListResponse {
    /// VM ID
    pub vm_id: String,
    /// Snapshots
    pub snapshots: Vec<SnapshotResponse>,
    /// Total count
    pub total: usize,
}

/// Restore response
#[derive(Debug, Serialize, Deserialize)]
pub struct RestoreResponse {
    /// VM ID
    pub vm_id: String,
    /// Snapshot ID that was restored
    pub snapshot_id: u64,
    /// Status
    pub status: String,
}

/// Error response
#[derive(Debug, Serialize)]
struct SnapshotErrorResponse {
    error: String,
}

fn snapshot_info_to_response(info: &hv2_core::SnapshotInfo) -> SnapshotResponse {
    SnapshotResponse {
        id: info.id.value(),
        name: info.name.clone(),
        description: info.description.clone(),
        state: format!("{:?}", info.state),
        snapshot_type: format!("{:?}", info.snapshot_type),
        size_bytes: info.size_bytes,
        vcpu_count: info.vcpu_count,
        created_at: format!("{:?}", info.created_at),
        tags: info.tags.keys().cloned().collect(),
        parent_id: info.parent_id.map(|id| id.value()),
    }
}

fn parse_snapshot_type(s: &str) -> SnapshotType {
    match s.to_lowercase().as_str() {
        "incremental" => SnapshotType::Incremental,
        "memory_only" | "memoryonly" => SnapshotType::MemoryOnly,
        "checkpoint" => SnapshotType::Checkpoint,
        _ => SnapshotType::Full,
    }
}

// ============================================================================
// Path extractors
// ============================================================================

/// Path parameters for VM-scoped routes
#[derive(Debug, Deserialize)]
struct VmPath {
    vm_id: String,
}

/// Path parameters for snapshot-scoped routes
#[derive(Debug, Deserialize)]
struct SnapshotPath {
    vm_id: String,
    snap_id: u64,
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /api/v1/vms/:vm_id/snapshots — create a new snapshot
async fn create_snapshot(
    State(state): State<Arc<SnapshotAppState>>,
    Path(VmPath { vm_id }): Path<VmPath>,
    Json(req): Json<CreateSnapshotRequest>,
) -> impl IntoResponse {
    state.ensure_manager(&vm_id);

    let mut options = CreateSnapshotOptions {
        snapshot_type: parse_snapshot_type(&req.snapshot_type),
        ..Default::default()
    };

    if let Some(name) = req.name {
        options.name = Some(name);
    }
    if let Some(desc) = req.description {
        options.description = Some(desc);
    }
    for tag in req.tags {
        options.tags.push(tag);
    }
    if let Some(parent) = req.parent_id {
        options.parent_id = Some(SnapshotId::new(parent));
    }

    let mut managers = state.managers.write();
    let manager = managers.get_mut(&vm_id).unwrap();

    match manager.begin_snapshot(options) {
        Ok(snap_id) => {
            // Complete the snapshot immediately (in a real system, this
            // would be async with actual CPU/memory/device capture)
            match manager.complete_snapshot() {
                Ok(_) => {
                    let info = manager.get_snapshot(&snap_id).unwrap();
                    let resp = snapshot_info_to_response(info);
                    (StatusCode::CREATED, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/v1/vms/:vm_id/snapshots — list all snapshots for a VM
async fn list_snapshots(
    State(state): State<Arc<SnapshotAppState>>,
    Path(VmPath { vm_id }): Path<VmPath>,
) -> impl IntoResponse {
    state.ensure_manager(&vm_id);
    let managers = state.managers.read();
    let manager = managers.get(&vm_id).unwrap();

    let snapshots: Vec<SnapshotResponse> = manager
        .list_snapshots()
        .iter()
        .map(|info| snapshot_info_to_response(info))
        .collect();

    let total = snapshots.len();
    Json(SnapshotListResponse {
        vm_id,
        snapshots,
        total,
    })
}

/// GET /api/v1/vms/:vm_id/snapshots/:snap_id — get snapshot details
async fn get_snapshot(
    State(state): State<Arc<SnapshotAppState>>,
    Path(SnapshotPath { vm_id, snap_id }): Path<SnapshotPath>,
) -> impl IntoResponse {
    state.ensure_manager(&vm_id);
    let managers = state.managers.read();
    let manager = managers.get(&vm_id).unwrap();

    let snapshot_id = SnapshotId::new(snap_id);
    match manager.get_snapshot(&snapshot_id) {
        Some(info) => {
            let resp = snapshot_info_to_response(info);
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Snapshot not found"})),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/vms/:vm_id/snapshots/:snap_id — delete a snapshot
async fn delete_snapshot(
    State(state): State<Arc<SnapshotAppState>>,
    Path(SnapshotPath { vm_id, snap_id }): Path<SnapshotPath>,
) -> impl IntoResponse {
    state.ensure_manager(&vm_id);
    let mut managers = state.managers.write();
    let manager = managers.get_mut(&vm_id).unwrap();

    let snapshot_id = SnapshotId::new(snap_id);
    match manager.delete_snapshot(&snapshot_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(hv2_core::SnapshotError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Snapshot not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/v1/vms/:vm_id/snapshots/:snap_id/restore — restore from snapshot
async fn restore_snapshot(
    State(state): State<Arc<SnapshotAppState>>,
    Path(SnapshotPath { vm_id, snap_id }): Path<SnapshotPath>,
) -> impl IntoResponse {
    state.ensure_manager(&vm_id);
    let mut managers = state.managers.write();
    let manager = managers.get_mut(&vm_id).unwrap();

    let snapshot_id = SnapshotId::new(snap_id);

    match manager.begin_restore(&snapshot_id) {
        Ok(_info) => {
            // Complete restore (in real system, would replay state)
            match manager.complete_restore() {
                Ok(()) => Json(RestoreResponse {
                    vm_id,
                    snapshot_id: snap_id,
                    status: "restored".into(),
                })
                .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response(),
            }
        }
        Err(hv2_core::SnapshotError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Snapshot not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ============================================================================
// Router
// ============================================================================

/// Create the snapshot/restore router
pub fn create_snapshot_router(state: Arc<SnapshotAppState>) -> Router {
    Router::new()
        .route("/api/v1/vms/:vm_id/snapshots", post(create_snapshot))
        .route("/api/v1/vms/:vm_id/snapshots", get(list_snapshots))
        .route(
            "/api/v1/vms/:vm_id/snapshots/:snap_id",
            get(get_snapshot),
        )
        .route(
            "/api/v1/vms/:vm_id/snapshots/:snap_id",
            delete(delete_snapshot),
        )
        .route(
            "/api/v1/vms/:vm_id/snapshots/:snap_id/restore",
            post(restore_snapshot),
        )
        .with_state(state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> Arc<SnapshotAppState> {
        Arc::new(SnapshotAppState::new())
    }

    #[tokio::test]
    async fn test_create_and_list_snapshots() {
        let state = test_state();
        let app = create_snapshot_router(state);

        // Create a snapshot
        let body = serde_json::json!({
            "name": "pre-upgrade",
            "description": "Snapshot before upgrade",
            "snapshot_type": "full"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/vms/vm-001/snapshots")
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
        let snap: SnapshotResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(snap.name, "pre-upgrade");
        assert_eq!(snap.state, "Valid");

        // List snapshots
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms/vm-001/snapshots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: SnapshotListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.vm_id, "vm-001");
    }

    #[tokio::test]
    async fn test_get_snapshot_not_found() {
        let state = test_state();
        let app = create_snapshot_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms/vm-001/snapshots/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_snapshot() {
        let state = test_state();
        let app = create_snapshot_router(state.clone());

        // Create a snapshot first
        let body = serde_json::json!({
            "name": "to-delete",
            "snapshot_type": "full"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/vms/vm-002/snapshots")
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
        let snap: SnapshotResponse = serde_json::from_slice(&body).unwrap();
        let snap_id = snap.id;

        // Delete it
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(&format!("/api/v1/vms/vm-002/snapshots/{snap_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_restore_snapshot() {
        let state = test_state();
        let app = create_snapshot_router(state);

        // Create
        let body = serde_json::json!({
            "name": "restore-test",
            "snapshot_type": "full"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/vms/vm-003/snapshots")
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
        let snap: SnapshotResponse = serde_json::from_slice(&body).unwrap();
        let snap_id = snap.id;

        // Restore
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&format!("/api/v1/vms/vm-003/snapshots/{snap_id}/restore"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let restore: RestoreResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(restore.status, "restored");
        assert_eq!(restore.vm_id, "vm-003");
    }

    #[tokio::test]
    async fn test_delete_nonexistent_snapshot() {
        let state = test_state();
        let app = create_snapshot_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/vms/vm-001/snapshots/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_restore_nonexistent_snapshot() {
        let state = test_state();
        let app = create_snapshot_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/vms/vm-001/snapshots/99999/restore")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_checkpoint() {
        let state = test_state();
        let app = create_snapshot_router(state);

        let body = serde_json::json!({
            "name": "checkpoint-1",
            "snapshot_type": "checkpoint",
            "tags": ["before-migration", "production"]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/vms/vm-004/snapshots")
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
        let snap: SnapshotResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(snap.name, "checkpoint-1");
        assert_eq!(snap.snapshot_type, "Checkpoint");
    }
}
