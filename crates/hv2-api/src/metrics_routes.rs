//! Prometheus Metrics Endpoint
//!
//! Provides a unified `/metrics` endpoint in Prometheus text exposition format
//! (OpenMetrics 1.0.0 compatible). This endpoint aggregates metrics from all
//! subsystems:
//!
//! - **API**: Request counts, uptime
//! - **Runtime**: Pool, scheduler, billing, health (if enabled)
//! - **GPU Fabric**: Device count, host count, reservations (if enabled)
//! - **Image Registry**: Image counts, enforcement mode (if enabled)
//! - **Events**: Webhook count, event publish total (if enabled)
//!
//! ## Usage
//!
//! ```text
//! GET /metrics
//! Accept: text/plain
//! ```

use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{extract::State, Router};
use std::sync::Arc;
use std::time::Instant;

use crate::events::EventBus;
use crate::gpu_fabric_routes::GpuFabricAppState;
use crate::image_registry_routes::ImageRegistryAppState;
use hv2_runtime::Runtime;

// ============================================================================
// State
// ============================================================================

/// Aggregated state for the metrics endpoint
pub struct MetricsState {
    /// Server start time
    start_time: Instant,
    /// Runtime (if enabled)
    runtime: Option<Arc<Runtime>>,
    /// GPU fabric state (if enabled)
    gpu_fabric: Option<Arc<GpuFabricAppState>>,
    /// Image registry state (if enabled)
    image_registry: Option<Arc<ImageRegistryAppState>>,
    /// Event bus (if enabled)
    event_bus: Option<Arc<EventBus>>,
}

impl MetricsState {
    /// Create a new metrics state
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            runtime: None,
            gpu_fabric: None,
            image_registry: None,
            event_bus: None,
        }
    }

    /// Add runtime
    pub fn with_runtime(mut self, runtime: Arc<Runtime>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Add GPU fabric state
    pub fn with_gpu_fabric(mut self, gpu: Arc<GpuFabricAppState>) -> Self {
        self.gpu_fabric = Some(gpu);
        self
    }

    /// Add image registry state
    pub fn with_image_registry(mut self, registry: Arc<ImageRegistryAppState>) -> Self {
        self.image_registry = Some(registry);
        self
    }

    /// Add event bus
    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /metrics
///
/// Returns all metrics in Prometheus text exposition format.
async fn prometheus_metrics(State(state): State<Arc<MetricsState>>) -> impl IntoResponse {
    let mut lines = Vec::new();

    // ── API server metrics ────────────────────────────────────────────
    let uptime = state.start_time.elapsed().as_secs_f64();
    lines.push("# HELP hypermachine_api_uptime_seconds API server uptime in seconds.".into());
    lines.push("# TYPE hypermachine_api_uptime_seconds gauge".into());
    lines.push(format!("hypermachine_api_uptime_seconds {uptime:.1}"));

    lines.push("# HELP hypermachine_api_info API server build information.".into());
    lines.push("# TYPE hypermachine_api_info gauge".into());
    lines.push(format!(
        "hypermachine_api_info{{version=\"{}\"}} 1",
        env!("CARGO_PKG_VERSION")
    ));

    // ── Runtime metrics ───────────────────────────────────────────────
    if let Some(ref rt) = state.runtime {
        let m = rt.collect_metrics();

        lines.push("# HELP hypermachine_runtime_uptime_seconds Runtime uptime in seconds.".into());
        lines.push("# TYPE hypermachine_runtime_uptime_seconds gauge".into());
        lines.push(format!(
            "hypermachine_runtime_uptime_seconds {}",
            m.uptime_seconds
        ));

        // Pool
        lines.push("# HELP hypermachine_pool_total Total VMs in pool.".into());
        lines.push("# TYPE hypermachine_pool_total gauge".into());
        lines.push(format!("hypermachine_pool_total {}", m.pool_total));

        lines.push("# HELP hypermachine_pool_warm Warm (ready) VMs in pool.".into());
        lines.push("# TYPE hypermachine_pool_warm gauge".into());
        lines.push(format!("hypermachine_pool_warm {}", m.pool_warm));

        lines.push("# HELP hypermachine_pool_active Active (assigned) VMs.".into());
        lines.push("# TYPE hypermachine_pool_active gauge".into());
        lines.push(format!("hypermachine_pool_active {}", m.pool_assigned));

        lines.push("# HELP hypermachine_pool_failed Failed VMs.".into());
        lines.push("# TYPE hypermachine_pool_failed gauge".into());
        lines.push(format!("hypermachine_pool_failed {}", m.pool_failed));

        // Scheduler
        lines.push("# HELP hypermachine_scheduler_pending Pending workloads.".into());
        lines.push("# TYPE hypermachine_scheduler_pending gauge".into());
        lines.push(format!(
            "hypermachine_scheduler_pending {}",
            m.scheduler_pending
        ));

        lines.push("# HELP hypermachine_scheduler_placed_total Total placed workloads.".into());
        lines.push("# TYPE hypermachine_scheduler_placed_total counter".into());
        lines.push(format!(
            "hypermachine_scheduler_placed_total {}",
            m.sessions_created_total
        ));

        // Health
        lines.push("# HELP hypermachine_health_healthy Healthy VM count.".into());
        lines.push("# TYPE hypermachine_health_healthy gauge".into());
        lines.push(format!("hypermachine_health_healthy {}", m.health_healthy));

        lines.push("# HELP hypermachine_health_degraded Degraded VM count.".into());
        lines.push("# TYPE hypermachine_health_degraded gauge".into());
        lines.push(format!(
            "hypermachine_health_degraded {}",
            m.health_degraded
        ));

        lines.push("# HELP hypermachine_health_unhealthy Unhealthy VM count.".into());
        lines.push("# TYPE hypermachine_health_unhealthy gauge".into());
        lines.push(format!(
            "hypermachine_health_unhealthy {}",
            m.health_unhealthy
        ));

        // Billing
        lines.push("# HELP hypermachine_billing_sessions Active billing sessions.".into());
        lines.push("# TYPE hypermachine_billing_sessions gauge".into());
        lines.push(format!(
            "hypermachine_billing_sessions {}",
            m.billing_sessions
        ));

        lines.push(
            "# HELP hypermachine_billing_sessions_total Total billing sessions created.".into(),
        );
        lines.push("# TYPE hypermachine_billing_sessions_total counter".into());
        lines.push(format!(
            "hypermachine_billing_sessions_total {}",
            m.sessions_created_total
        ));
    }

    // ── GPU Fabric metrics ────────────────────────────────────────────
    if let Some(ref gpu) = state.gpu_fabric {
        let topo = gpu.topology.read();

        lines.push("# HELP hypermachine_gpu_devices_total Total GPU devices.".into());
        lines.push("# TYPE hypermachine_gpu_devices_total gauge".into());
        lines.push(format!(
            "hypermachine_gpu_devices_total {}",
            topo.device_count()
        ));

        lines.push("# HELP hypermachine_gpu_hosts_total Total GPU hosts.".into());
        lines.push("# TYPE hypermachine_gpu_hosts_total gauge".into());
        lines.push(format!(
            "hypermachine_gpu_hosts_total {}",
            topo.hosts().len()
        ));

        lines.push("# HELP hypermachine_gpu_fleet_hosts Fleet hosts registered.".into());
        lines.push("# TYPE hypermachine_gpu_fleet_hosts gauge".into());
        lines.push(format!(
            "hypermachine_gpu_fleet_hosts {}",
            gpu.fleet.list_hosts().len()
        ));

        lines.push("# HELP hypermachine_gpu_capacity_classes VM classes registered.".into());
        lines.push("# TYPE hypermachine_gpu_capacity_classes gauge".into());
        lines.push(format!(
            "hypermachine_gpu_capacity_classes {}",
            gpu.capacity.list_classes().len()
        ));

        lines.push("# HELP hypermachine_gpu_active_reservations Active GPU reservations.".into());
        lines.push("# TYPE hypermachine_gpu_active_reservations gauge".into());
        lines.push(format!(
            "hypermachine_gpu_active_reservations {}",
            gpu.capacity.active_reservations().len()
        ));
    }

    // ── Image Registry metrics ────────────────────────────────────────
    if let Some(ref reg) = state.image_registry {
        let registry = &reg.registry;

        lines.push("# HELP hypermachine_images_total Total registered images.".into());
        lines.push("# TYPE hypermachine_images_total gauge".into());
        lines.push(format!(
            "hypermachine_images_total {}",
            registry.image_count()
        ));

        let config = registry.config();
        lines
            .push("# HELP hypermachine_images_enforcement Image registry enforcement mode.".into());
        lines.push("# TYPE hypermachine_images_enforcement gauge".into());
        let enforcement_value = match config.mode {
            hv2_core::security::image_registry::EnforcementMode::Enforce => 2,
            hv2_core::security::image_registry::EnforcementMode::Audit => 1,
            hv2_core::security::image_registry::EnforcementMode::Disabled => 0,
        };
        lines.push(format!(
            "hypermachine_images_enforcement {enforcement_value}"
        ));
    }

    // ── Events metrics ────────────────────────────────────────────────
    if let Some(ref bus) = state.event_bus {
        let webhooks = bus.list_webhooks();

        lines.push("# HELP hypermachine_webhooks_total Registered webhook subscriptions.".into());
        lines.push("# TYPE hypermachine_webhooks_total gauge".into());
        lines.push(format!("hypermachine_webhooks_total {}", webhooks.len()));
    }

    // EOF marker
    lines.push("# EOF".into());

    let body = lines.join("\n");
    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

// ============================================================================
// Router
// ============================================================================

/// Create the metrics router
pub fn create_metrics_router(state: Arc<MetricsState>) -> Router {
    Router::new()
        .route("/metrics", get(prometheus_metrics))
        .with_state(state)
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

    fn test_state() -> Arc<MetricsState> {
        Arc::new(MetricsState::new())
    }

    #[tokio::test]
    async fn test_prometheus_metrics_minimal() {
        let state = test_state();
        let app = create_metrics_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
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

        assert!(text.contains("hypermachine_api_uptime_seconds"));
        assert!(text.contains("hypermachine_api_info"));
        assert!(text.contains("# EOF"));
        // No runtime lines in minimal mode
        assert!(!text.contains("hypermachine_runtime_uptime_seconds"));
    }

    #[tokio::test]
    async fn test_prometheus_metrics_with_runtime() {
        let rt = Arc::new(hv2_runtime::Runtime::new(
            hv2_runtime::RuntimeConfig::default(),
        ));
        let state = Arc::new(MetricsState::new().with_runtime(rt));
        let app = create_metrics_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(text.contains("hypermachine_runtime_uptime_seconds"));
        assert!(text.contains("hypermachine_pool_total"));
        assert!(text.contains("hypermachine_pool_warm"));
        assert!(text.contains("hypermachine_scheduler_pending"));
        assert!(text.contains("hypermachine_health_healthy"));
        assert!(text.contains("hypermachine_billing_sessions"));
    }

    #[tokio::test]
    async fn test_prometheus_metrics_with_gpu_fabric() {
        let gpu = Arc::new(GpuFabricAppState::new());
        let state = Arc::new(MetricsState::new().with_gpu_fabric(gpu));
        let app = create_metrics_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(text.contains("hypermachine_gpu_devices_total"));
        assert!(text.contains("hypermachine_gpu_hosts_total"));
        assert!(text.contains("hypermachine_gpu_fleet_hosts"));
        assert!(text.contains("hypermachine_gpu_capacity_classes"));
        assert!(text.contains("hypermachine_gpu_active_reservations"));
    }

    #[tokio::test]
    async fn test_prometheus_metrics_with_image_registry() {
        let reg = Arc::new(ImageRegistryAppState::new());
        let state = Arc::new(MetricsState::new().with_image_registry(reg));
        let app = create_metrics_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(text.contains("hypermachine_images_total"));
        assert!(text.contains("hypermachine_images_enforcement"));
    }

    #[tokio::test]
    async fn test_prometheus_metrics_with_events() {
        let bus = Arc::new(EventBus::new());
        let state = Arc::new(MetricsState::new().with_event_bus(bus));
        let app = create_metrics_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(text.contains("hypermachine_webhooks_total"));
    }
}
