//! Unified health check endpoint that aggregates status from all subsystems.
//!
//! Provides `/api/v1/health/full` which combines:
//! - Core API (always present)
//! - GPU Fabric topology / fleet / capacity
//! - Image Registry

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::gpu_fabric_routes::GpuFabricAppState;
use crate::image_registry_routes::ImageRegistryAppState;

// ============================================================================
// Types
// ============================================================================

/// Shared state for the unified health endpoint.
pub struct UnifiedHealthState {
    start_time: Instant,
    pub gpu_fabric: Option<Arc<GpuFabricAppState>>,
    pub image_registry: Option<Arc<ImageRegistryAppState>>,
    pub runtime_enabled: bool,
    pub events_enabled: bool,
}

impl UnifiedHealthState {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            gpu_fabric: None,
            image_registry: None,
            runtime_enabled: false,
            events_enabled: false,
        }
    }

    pub fn with_gpu_fabric(mut self, state: Arc<GpuFabricAppState>) -> Self {
        self.gpu_fabric = Some(state);
        self
    }

    pub fn with_image_registry(mut self, state: Arc<ImageRegistryAppState>) -> Self {
        self.image_registry = Some(state);
        self
    }

    pub fn with_runtime(mut self, enabled: bool) -> Self {
        self.runtime_enabled = enabled;
        self
    }

    pub fn with_events(mut self, enabled: bool) -> Self {
        self.events_enabled = enabled;
        self
    }
}

/// Full health response combining all subsystems.
#[derive(Debug, Serialize, Deserialize)]
pub struct UnifiedHealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub subsystems: Vec<SubsystemHealth>,
}

/// Individual subsystem health entry.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubsystemHealth {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

// ============================================================================
// Router
// ============================================================================

pub fn create_health_router(state: Arc<UnifiedHealthState>) -> Router {
    Router::new()
        .route("/api/v1/health/full", get(unified_health))
        .with_state(state)
}

// ============================================================================
// Handler
// ============================================================================

async fn unified_health(State(state): State<Arc<UnifiedHealthState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    let mut subsystems = Vec::new();

    // Core API — always up if we are responding
    subsystems.push(SubsystemHealth {
        name: "api".to_string(),
        status: "up".to_string(),
        detail: None,
    });

    // Runtime
    subsystems.push(SubsystemHealth {
        name: "runtime".to_string(),
        status: if state.runtime_enabled {
            "up".to_string()
        } else {
            "disabled".to_string()
        },
        detail: None,
    });

    // Events
    subsystems.push(SubsystemHealth {
        name: "events".to_string(),
        status: if state.events_enabled {
            "up".to_string()
        } else {
            "disabled".to_string()
        },
        detail: None,
    });

    // GPU Fabric
    if let Some(ref gpu) = state.gpu_fabric {
        let topo = gpu.topology.read();
        let device_count = topo.device_count();
        let host_count = topo.hosts().len();
        drop(topo);

        let class_count = gpu.capacity.list_classes().len();
        let reservation_count = gpu.capacity.active_reservations().len();
        let fleet_hosts = gpu.fleet.list_hosts().len();

        subsystems.push(SubsystemHealth {
            name: "gpu_fabric".to_string(),
            status: "up".to_string(),
            detail: Some(serde_json::json!({
                "devices": device_count,
                "hosts": host_count,
                "fleet_hosts": fleet_hosts,
                "vm_classes": class_count,
                "active_reservations": reservation_count,
            })),
        });
    } else {
        subsystems.push(SubsystemHealth {
            name: "gpu_fabric".to_string(),
            status: "disabled".to_string(),
            detail: None,
        });
    }

    // Image Registry
    if let Some(ref reg) = state.image_registry {
        let count = reg.registry.image_count();
        let mode = format!("{:?}", reg.registry.config().mode);
        subsystems.push(SubsystemHealth {
            name: "image_registry".to_string(),
            status: "up".to_string(),
            detail: Some(serde_json::json!({
                "image_count": count,
                "enforcement_mode": mode,
            })),
        });
    } else {
        subsystems.push(SubsystemHealth {
            name: "image_registry".to_string(),
            status: "disabled".to_string(),
            detail: None,
        });
    }

    let all_ok = subsystems
        .iter()
        .all(|s| s.status == "up" || s.status == "disabled");

    (
        if all_ok {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(UnifiedHealthResponse {
            status: if all_ok {
                "healthy".to_string()
            } else {
                "degraded".to_string()
            },
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: uptime,
            subsystems,
        }),
    )
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

    fn test_request(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn health_full_all_disabled() {
        let state = Arc::new(UnifiedHealthState::new());
        let app = create_health_router(state);

        let resp = app
            .oneshot(test_request("/api/v1/health/full"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: UnifiedHealthResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.status, "healthy");
        assert_eq!(body.subsystems.len(), 5);
        assert_eq!(body.subsystems[0].name, "api");
        assert_eq!(body.subsystems[0].status, "up");
        assert_eq!(body.subsystems[3].name, "gpu_fabric");
        assert_eq!(body.subsystems[3].status, "disabled");
    }

    #[tokio::test]
    async fn health_full_with_gpu_and_registry() {
        let gpu = Arc::new(GpuFabricAppState::new());
        let reg = Arc::new(ImageRegistryAppState::new());
        let state = Arc::new(
            UnifiedHealthState::new()
                .with_runtime(true)
                .with_events(true)
                .with_gpu_fabric(gpu)
                .with_image_registry(reg),
        );
        let app = create_health_router(state);

        let resp = app
            .oneshot(test_request("/api/v1/health/full"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], "healthy");
        assert_eq!(body["subsystems"][3]["name"], "gpu_fabric");
        assert_eq!(body["subsystems"][3]["status"], "up");
        assert_eq!(body["subsystems"][3]["detail"]["devices"], 0);
        assert_eq!(body["subsystems"][4]["name"], "image_registry");
        assert_eq!(body["subsystems"][4]["status"], "up");
    }
}
