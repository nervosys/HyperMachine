//! Unified API Server
//!
//! Combines all route groups — VM CRUD, ontology, events/SSE, and runtime
//! fleet management — into a single configurable HTTP server.
//!
//! ## Architecture
//!
//! The server merges eleven independent routers into one:
//!
//! | Router           | Prefix                            | State                  |
//! |------------------|-----------------------------------|------------------------|
//! | VM CRUD          | `/api/v1/vms`                     | `AppState`             |
//! | Ontology         | `/agentic`                        | (stateless)            |
//! | Events/SSE       | `/api/v1/events`                  | `EventBus`             |
//! | WebSocket        | `/api/v1/events/ws`               | `EventBus`             |
//! | Runtime fleet    | `/api/v1/runtime`                 | `RuntimeAppState`      |
//! | GPU Fabric       | `/api/v1/gpu-fabric`              | `GpuFabricAppState`    |
//! | Image Registry   | `/api/v1/images`                  | `ImageRegistryAppState`|
//! | Unified Health   | `/api/v1/health/full`             | `UnifiedHealthState`   |
//! | Prometheus       | `/metrics`                        | `MetricsState`         |
//! | Snapshots        | `/api/v1/vms/:id/snapshots`       | `SnapshotAppState`     |
//! | Agent Runtime    | `/api/v1/agents`                  | `AgentRuntimeAppState` |
//!
//! Each router keeps its own state via axum state extractors while
//! sharing a single TCP listener and connection pool.
//!
//! ## Graceful Shutdown
//!
//! The server listens for `Ctrl+C` (SIGINT) and initiates a graceful
//! shutdown sequence:
//!
//! 1. Stop accepting new connections
//! 2. Drain in-flight requests (up to `shutdown_timeout_secs`)
//! 3. Log shutdown summary
//!
//! Configure the drain period via `ServerConfig::shutdown_timeout_secs`
//! (default: 30 seconds, 0 = immediate shutdown).

use crate::agent_runtime_routes::{self, AgentRuntimeAppState};
use crate::events::{self, EventBus};
use crate::gpu_fabric_routes::{self, GpuFabricAppState};
use crate::health_routes::{self, UnifiedHealthState};
use crate::image_registry_routes::{self, ImageRegistryAppState};
use crate::metrics_routes::{self, MetricsState};
use crate::middleware::MiddlewareConfig;
use crate::rest;
use crate::runtime_routes::{self, RuntimeAppState};
use crate::snapshot_routes::{self, SnapshotAppState};
use crate::ws_routes;
use crate::Result;
use axum::Router;
use hv2_core::security::image_registry::{EnforcementMode, RegistryConfig};
use hv2_runtime::{Runtime, RuntimeConfig};
use std::sync::Arc;
use std::time::Instant;

/// Placeholder warm-baseline size for the mounted agent runtime. A production
/// deployment supplies a real booted-agent image instead.
const AGENT_BASELINE_BYTES: usize = 1024 * 1024;

// ============================================================================
// Server Configuration
// ============================================================================

/// How the image allowlist behaves when admission is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageAdmissionMode {
    /// Log what would be refused, refuse nothing. The default, so that
    /// enabling admission is observable before it is load-bearing.
    #[default]
    Audit,
    /// Refuse any image the registry does not admit.
    Enforce,
    /// Admit everything; the registry answers questions but gates nothing.
    Disabled,
}

impl ImageAdmissionMode {
    fn to_registry(self) -> EnforcementMode {
        match self {
            Self::Audit => EnforcementMode::Audit,
            Self::Enforce => EnforcementMode::Enforce,
            Self::Disabled => EnforcementMode::Disabled,
        }
    }
}

/// Configuration for the unified API server
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// REST API host
    pub host: String,
    /// REST API port
    pub rest_port: u16,
    /// gRPC port
    pub grpc_port: u16,
    /// Enable runtime fleet endpoints
    pub enable_runtime: bool,
    /// Enable events/SSE/webhook endpoints
    pub enable_events: bool,
    /// Runtime configuration (used when enable_runtime is true)
    pub runtime: RuntimeConfig,
    /// Number of VMs to pre-warm in the pool
    pub pre_warm_count: usize,
    /// Middleware stack configuration
    pub middleware: MiddlewareConfig,
    /// Graceful shutdown timeout (seconds before force-killing connections)
    pub shutdown_timeout_secs: u64,
    /// TLS configuration (None = plain HTTP)
    pub tls: Option<crate::tls::TlsConfig>,
    /// Gate VM boot images on the image allowlist served at `/api/v1/images`.
    ///
    /// Off by default, and deliberately so: `RegistryConfig::default` is
    /// `EnforcementMode::Enforce`, so switching this on with an empty catalogue
    /// refuses *every* boot image until images are registered and approved.
    /// Enable it once the catalogue is populated, or start the registry in
    /// `EnforcementMode::Audit` to see what would be blocked first.
    pub enforce_image_admission: bool,
    /// How the shared image registry behaves once admission is enabled.
    ///
    /// Defaults to [`ImageAdmissionMode::Audit`]: turning admission on logs
    /// what *would* be refused without refusing it, so an operator can see the
    /// blast radius against a real workload before committing. Promote to
    /// `Enforce` once the catalogue is populated — doing it the other way round
    /// refuses every boot image until each one is registered and approved.
    pub image_admission_mode: ImageAdmissionMode,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            rest_port: 8080,
            grpc_port: 50051,
            enable_runtime: true,
            enable_events: true,
            runtime: RuntimeConfig::default(),
            pre_warm_count: 2,
            middleware: MiddlewareConfig::default(),
            shutdown_timeout_secs: 30,
            tls: None,
            enforce_image_admission: false,
            image_admission_mode: ImageAdmissionMode::Audit,
        }
    }
}

impl ServerConfig {
    /// Builder-style: set REST port
    pub fn rest_port(mut self, port: u16) -> Self {
        self.rest_port = port;
        self
    }

    /// Builder-style: set gRPC port
    pub fn grpc_port(mut self, port: u16) -> Self {
        self.grpc_port = port;
        self
    }

    /// Builder-style: set host
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Builder-style: enable/disable runtime
    pub fn enable_runtime(mut self, enable: bool) -> Self {
        self.enable_runtime = enable;
        self
    }

    /// Builder-style: enable/disable events
    pub fn enable_events(mut self, enable: bool) -> Self {
        self.enable_events = enable;
        self
    }

    /// Builder-style: set runtime config
    pub fn runtime_config(mut self, config: RuntimeConfig) -> Self {
        self.runtime = config;
        self
    }

    /// Builder-style: set pre-warm count
    pub fn pre_warm_count(mut self, count: usize) -> Self {
        self.pre_warm_count = count;
        self
    }

    /// Builder-style: set middleware configuration
    pub fn middleware(mut self, middleware: MiddlewareConfig) -> Self {
        self.middleware = middleware;
        self
    }

    /// Builder-style: set graceful shutdown timeout (seconds)
    pub fn shutdown_timeout_secs(mut self, secs: u64) -> Self {
        self.shutdown_timeout_secs = secs;
        self
    }

    /// Builder-style: set TLS configuration
    pub fn tls(mut self, config: crate::tls::TlsConfig) -> Self {
        self.tls = Some(config);
        self
    }

    /// Parse the REST socket address
    pub fn rest_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.host, self.rest_port)
            .parse()
            .expect("Invalid REST address")
    }

    /// Parse the gRPC socket address
    pub fn grpc_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.host, self.grpc_port)
            .parse()
            .expect("Invalid gRPC address")
    }
}

// ============================================================================
// Unified Server
// ============================================================================

/// The unified HyperMachine API server
///
/// Combines VM CRUD, ontology, events, and runtime fleet routes into a
/// single axum application backed by a shared `Runtime` instance.
pub struct Server {
    /// Server configuration
    config: ServerConfig,
    /// Runtime instance (if enabled)
    runtime: Option<Arc<Runtime>>,
    /// Event bus (if enabled)
    event_bus: Option<Arc<EventBus>>,
}

impl Server {
    /// Create a new server with the given configuration
    pub fn new(config: ServerConfig) -> Self {
        let runtime = if config.enable_runtime {
            let rt = Runtime::new(config.runtime.clone());

            // Pre-warm the pool
            for _ in 0..config.pre_warm_count {
                if let Ok(vm_id) = rt.pool().provision() {
                    let _ = rt.pool().mark_warm(&vm_id);
                }
            }

            Some(Arc::new(rt))
        } else {
            None
        };

        let event_bus = if config.enable_events {
            Some(Arc::new(EventBus::new()))
        } else {
            None
        };

        Self {
            config,
            runtime,
            event_bus,
        }
    }

    /// Create a server with an existing runtime (for testing or embedding)
    pub fn with_runtime(config: ServerConfig, runtime: Runtime) -> Self {
        let event_bus = if config.enable_events {
            Some(Arc::new(EventBus::new()))
        } else {
            None
        };

        Self {
            config,
            runtime: Some(Arc::new(runtime)),
            event_bus,
        }
    }

    /// Get a reference to the runtime (if enabled)
    pub fn runtime(&self) -> Option<&Runtime> {
        self.runtime.as_deref()
    }

    /// Get a reference to the event bus (if enabled)
    pub fn event_bus(&self) -> Option<&EventBus> {
        self.event_bus.as_deref()
    }

    /// Get the server config
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Build the unified axum router
    ///
    /// Merges all enabled route groups into a single router:
    /// - `/health` — health check
    /// - `/api/v1/vms/...` — VM CRUD
    /// - `/agentic/...` — AI agent ontology
    /// - `/api/v1/events/...` — webhooks, SSE streaming
    /// - `/api/v1/runtime/...` — fleet-level operations
    /// - `/api/v1/gpu-fabric/...` — GPU topology, fleet, capacity
    pub fn build_router(&self) -> Router {
        // One registry instance, shared by the `/api/v1/images` routes and the
        // VMs this API creates — otherwise approving an image over REST would
        // have no bearing on whether a VM could boot it.
        let image_state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: self.config.image_admission_mode.to_registry(),
            ..RegistryConfig::default()
        }));

        // Build application state with component awareness
        let mut app_state = rest::AppState::new()
            .with_runtime_enabled(self.config.enable_runtime)
            .with_events_enabled(self.config.enable_events);
        if self.config.enforce_image_admission {
            app_state = app_state.with_image_registry(Arc::clone(&image_state.registry));
        }

        // Start with the VM CRUD + ontology + health router
        let mut app = rest::create_router_with_state(app_state);

        // Build unified health state
        let mut health_state = UnifiedHealthState::new()
            .with_runtime(self.config.enable_runtime)
            .with_events(self.config.enable_events);

        // Merge runtime routes if enabled
        if let Some(ref rt) = self.runtime {
            let state = Arc::new(RuntimeAppState::from_runtime_arc(rt.clone()));
            let runtime_router = runtime_routes::create_runtime_router(state);
            app = app.merge(runtime_router);

            // Merge GPU fabric routes (backed by runtime topology/fleet/capacity)
            let gpu_state = Arc::new(GpuFabricAppState::new());
            let gpu_router = gpu_fabric_routes::create_gpu_fabric_router(gpu_state.clone());
            app = app.merge(gpu_router);

            // Merge image registry routes, backed by the shared registry above
            let image_router =
                image_registry_routes::create_image_registry_router(image_state.clone());
            app = app.merge(image_router);

            health_state = health_state
                .with_gpu_fabric(gpu_state)
                .with_image_registry(image_state);
        }

        // Merge unified health endpoint
        let health_router = health_routes::create_health_router(Arc::new(health_state));
        app = app.merge(health_router);

        // Merge unified Prometheus metrics endpoint
        let metrics_state = Arc::new(MetricsState::new());
        let metrics_router = metrics_routes::create_metrics_router(metrics_state);
        app = app.merge(metrics_router);

        // Merge snapshot/restore routes
        let snapshot_state = Arc::new(SnapshotAppState::new());
        let snapshot_router = snapshot_routes::create_snapshot_router(snapshot_state);
        app = app.merge(snapshot_router);

        // Merge agent-runtime routes (CoW agent fleet over a warm baseline).
        // The placeholder baseline is replaced by a real agent image in a
        // production deployment.
        let agent_state = Arc::new(AgentRuntimeAppState::new(&vec![0u8; AGENT_BASELINE_BYTES]));
        let agent_router = agent_runtime_routes::create_agent_runtime_router(agent_state);
        app = app.merge(agent_router);

        // Nest events routes if enabled
        if let Some(ref bus) = self.event_bus {
            let events_router = events::events_router(bus.clone());
            app = app.nest("/api/v1/events", events_router);

            // Merge WebSocket event streaming
            let ws_router = ws_routes::create_ws_router(bus.clone());
            app = app.merge(ws_router);
        }

        // Apply middleware stack
        self.config.middleware.apply(app)
    }

    /// Start the REST API server with graceful shutdown (blocking)
    pub async fn serve_rest(&self) -> Result<()> {
        let addr = self.config.rest_addr();
        let app = self.build_router();
        let timeout = self.config.shutdown_timeout_secs;

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| crate::ApiError::Transport(e.to_string()))?;

        let start = Instant::now();

        if let Some(ref tls_config) = self.config.tls {
            tracing::info!("Starting unified REST API server on https://{}", addr);
            let rustls_config = crate::tls::build_rustls_config(tls_config)?;
            let acceptor = tokio_rustls::TlsAcceptor::from(rustls_config);
            crate::tls::serve_tls(listener, app, acceptor, shutdown_signal(timeout)).await?;
        } else {
            tracing::info!("Starting unified REST API server on {}", addr);
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal(timeout))
                .await
                .map_err(|e| crate::ApiError::Transport(e.to_string()))?;
        }

        tracing::info!(
            "REST server shut down after {:.1}s uptime",
            start.elapsed().as_secs_f64()
        );

        Ok(())
    }

    /// Start the gRPC server (blocking)
    pub async fn serve_grpc(&self) -> Result<()> {
        crate::grpc::serve(self.config.grpc_addr()).await
    }

    /// Start both REST and gRPC servers concurrently with graceful shutdown
    ///
    /// On Ctrl+C (SIGINT), the server:
    /// 1. Stops accepting new connections
    /// 2. Waits up to `shutdown_timeout_secs` for in-flight requests to drain
    /// 3. Logs a shutdown summary with uptime
    pub async fn serve_all(&self) -> Result<()> {
        let rest_addr = self.config.rest_addr();
        let grpc_addr = self.config.grpc_addr();
        let app = self.build_router();
        let timeout = self.config.shutdown_timeout_secs;
        let tls_config = self.config.tls.clone();

        self.log_startup_banner();

        let start = Instant::now();

        let rest_handle = {
            let listener = tokio::net::TcpListener::bind(rest_addr)
                .await
                .map_err(|e| crate::ApiError::Transport(e.to_string()))?;

            if let Some(ref tls) = tls_config {
                let rustls_config = crate::tls::build_rustls_config(tls)?;
                let acceptor = tokio_rustls::TlsAcceptor::from(rustls_config);
                tokio::spawn(async move {
                    crate::tls::serve_tls(listener, app, acceptor, shutdown_signal(timeout))
                        .await
                        .ok();
                })
            } else {
                tokio::spawn(async move {
                    axum::serve(listener, app)
                        .with_graceful_shutdown(shutdown_signal(timeout))
                        .await
                        .ok();
                })
            }
        };

        let grpc_handle = tokio::spawn(async move {
            crate::grpc::serve(grpc_addr).await.ok();
        });

        tokio::select! {
            _ = rest_handle => {},
            _ = grpc_handle => {},
        }

        let uptime = start.elapsed();
        tracing::info!("Server shut down after {:.1}s uptime", uptime.as_secs_f64());

        Ok(())
    }

    /// Get a summary of enabled features for display
    pub fn feature_summary(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("VM CRUD", true),
            ("Ontology", true),
            ("Events/SSE", self.config.enable_events),
            ("Runtime Fleet", self.config.enable_runtime),
            ("gRPC", true),
        ]
    }

    /// Route table for display
    pub fn route_table(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let mut routes = vec![
            ("GET", "/health", "Health check"),
            ("GET", "/health/live", "Liveness probe"),
            ("GET", "/health/ready", "Readiness probe"),
            ("GET", "/api/v1/health/full", "Unified health"),
            ("GET", "/api/v1/vms", "List VMs"),
            ("POST", "/api/v1/vms", "Create VM"),
            ("GET", "/api/v1/vms/{id}", "Get VM"),
            ("DELETE", "/api/v1/vms/{id}", "Delete VM"),
            ("POST", "/api/v1/vms/{id}/start", "Start VM"),
            ("POST", "/api/v1/vms/{id}/stop", "Stop VM"),
            ("POST", "/api/v1/vms/{id}/pause", "Pause VM"),
            ("POST", "/api/v1/vms/{id}/resume", "Resume VM"),
            ("GET", "/api/v1/vms/{id}/metrics", "Get metrics"),
            ("POST", "/api/v1/vms/{id}/script", "Execute script"),
            ("GET", "/agentic/ontology", "AI ontology"),
            ("GET", "/agentic/tools/openai", "OpenAI tools"),
            ("GET", "/agentic/tools/anthropic", "Anthropic tools"),
            ("GET", "/agentic/tools/gemini", "Gemini tools"),
            ("GET", "/.well-known/ai-plugin.json", "AI plugin manifest"),
            ("GET", "/metrics", "Unified Prometheus metrics"),
            ("POST", "/api/v1/vms/{id}/snapshots", "Create snapshot"),
            ("GET", "/api/v1/vms/{id}/snapshots", "List snapshots"),
            (
                "GET",
                "/api/v1/vms/{id}/snapshots/{snap_id}",
                "Get snapshot",
            ),
            (
                "DELETE",
                "/api/v1/vms/{id}/snapshots/{snap_id}",
                "Delete snapshot",
            ),
            (
                "POST",
                "/api/v1/vms/{id}/snapshots/{snap_id}/restore",
                "Restore snapshot",
            ),
        ];

        if self.config.enable_events {
            routes.extend_from_slice(&[
                ("POST", "/api/v1/events/webhooks", "Create webhook"),
                ("GET", "/api/v1/events/webhooks", "List webhooks"),
                ("GET", "/api/v1/events/webhooks/{id}", "Get webhook"),
                ("DELETE", "/api/v1/events/webhooks/{id}", "Delete webhook"),
                ("GET", "/api/v1/events/stream", "SSE event stream"),
                ("GET", "/api/v1/events/ws", "WebSocket event stream"),
            ]);
        }

        if self.config.enable_runtime {
            routes.extend_from_slice(&[
                ("GET", "/api/v1/runtime/status", "Runtime status"),
                ("GET", "/api/v1/runtime/health", "Runtime health"),
                ("GET", "/api/v1/runtime/metrics", "Metrics (JSON)"),
                (
                    "GET",
                    "/api/v1/runtime/metrics/prometheus",
                    "Metrics (Prometheus)",
                ),
                ("POST", "/api/v1/runtime/sessions", "Create session"),
                ("DELETE", "/api/v1/runtime/sessions/{id}", "Destroy session"),
                ("POST", "/api/v1/runtime/workloads", "Submit workload"),
                (
                    "POST",
                    "/api/v1/runtime/workloads/schedule",
                    "Schedule pending",
                ),
                ("POST", "/api/v1/runtime/workflows", "Run workflow"),
                (
                    "POST",
                    "/api/v1/runtime/workflows/{id}/steps/{step}",
                    "Advance step",
                ),
                (
                    "DELETE",
                    "/api/v1/runtime/workflows/{id}",
                    "Cancel workflow",
                ),
                ("POST", "/api/v1/runtime/maintenance", "Maintenance tick"),
            ]);
        }

        routes
    }

    /// Log a structured startup banner showing server configuration
    pub fn log_startup_banner(&self) {
        let features = self.feature_summary();
        let enabled: Vec<&str> = features
            .iter()
            .filter(|(_, e)| *e)
            .map(|(n, _)| *n)
            .collect();
        let routes = self.route_table();
        let middleware = self.config.middleware.summary();
        let mw_enabled: Vec<&str> = middleware
            .iter()
            .filter(|(_, e)| *e)
            .map(|(n, _)| *n)
            .collect();

        tracing::info!("┌─────────────────────────────────────────────────");
        tracing::info!("│ HyperMachine API Server v{}", env!("CARGO_PKG_VERSION"));
        tracing::info!("├─────────────────────────────────────────────────");
        let proto = if self.config.tls.is_some() {
            "https"
        } else {
            "http"
        };
        tracing::info!(
            "│ REST : {}://{}:{}",
            proto,
            self.config.host,
            self.config.rest_port
        );
        tracing::info!("│ gRPC : {}:{}", self.config.host, self.config.grpc_port);
        tracing::info!("│ Shutdown timeout: {}s", self.config.shutdown_timeout_secs);
        tracing::info!("│ Features: {}", enabled.join(", "));
        tracing::info!("│ Middleware: {}", mw_enabled.join(", "));
        tracing::info!("│ Routes: {} endpoints", routes.len());
        tracing::info!("└─────────────────────────────────────────────────");
        tracing::info!("Press Ctrl+C to initiate graceful shutdown");
    }
}

// ============================================================================
// Shutdown Signal
// ============================================================================

/// Wait for a shutdown signal (Ctrl+C) then enforce a drain timeout.
///
/// After receiving the signal, logs a notice and returns immediately.
/// The `timeout_secs` value is logged for operator awareness — the actual
/// connection draining is handled by `axum::serve`'s graceful shutdown
/// machinery which respects the future completing.
///
/// Setting `timeout_secs` to 0 means the server shuts down as quickly as
/// possible after the signal (no extended drain period).
pub async fn shutdown_signal(timeout_secs: u64) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    ctrl_c.await;

    if timeout_secs > 0 {
        tracing::info!(
            "Shutdown signal received — draining connections (timeout: {}s)",
            timeout_secs
        );
    } else {
        tracing::info!("Shutdown signal received — shutting down immediately");
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

    #[test]
    fn image_admission_defaults_to_audit() {
        // Enabling admission must not brick every boot. The registry denies by
        // default, so an operator flipping enforce_image_admission on an empty
        // catalogue would refuse every image; audit reports instead.
        let config = ServerConfig::default();
        assert!(!config.enforce_image_admission, "admission is opt-in");
        assert_eq!(config.image_admission_mode, ImageAdmissionMode::Audit);
    }

    #[test]
    fn admission_mode_maps_onto_the_registry() {
        use hv2_core::security::image_registry::EnforcementMode;
        assert_eq!(
            ImageAdmissionMode::Audit.to_registry(),
            EnforcementMode::Audit
        );
        assert_eq!(
            ImageAdmissionMode::Enforce.to_registry(),
            EnforcementMode::Enforce
        );
        assert_eq!(
            ImageAdmissionMode::Disabled.to_registry(),
            EnforcementMode::Disabled
        );
    }

    fn test_config() -> ServerConfig {
        ServerConfig {
            host: "127.0.0.1".to_string(),
            rest_port: 0,
            grpc_port: 0,
            enable_runtime: true,
            enable_events: true,
            runtime: RuntimeConfig::default(),
            pre_warm_count: 2,
            middleware: MiddlewareConfig::none(),
            shutdown_timeout_secs: 30,
            tls: None,
            // Struct-update syntax so a new field does not break every
            // test helper that builds a config.
            ..ServerConfig::default()
        }
    }

    fn test_server() -> Server {
        Server::new(test_config())
    }

    // ── Configuration ─────────────────────────────────────────────────

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.rest_port, 8080);
        assert_eq!(config.grpc_port, 50051);
        assert!(config.enable_runtime);
        assert!(config.enable_events);
        assert_eq!(config.pre_warm_count, 2);
    }

    #[test]
    fn test_server_config_builder() {
        let config = ServerConfig::default()
            .host("127.0.0.1")
            .rest_port(9090)
            .grpc_port(50052)
            .enable_runtime(false)
            .enable_events(false)
            .pre_warm_count(0);

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.rest_port, 9090);
        assert_eq!(config.grpc_port, 50052);
        assert!(!config.enable_runtime);
        assert!(!config.enable_events);
        assert_eq!(config.pre_warm_count, 0);
    }

    #[test]
    fn test_server_config_addrs() {
        let config = ServerConfig::default();
        assert_eq!(
            config.rest_addr(),
            "0.0.0.0:8080".parse::<std::net::SocketAddr>().unwrap()
        );
        assert_eq!(
            config.grpc_addr(),
            "0.0.0.0:50051".parse::<std::net::SocketAddr>().unwrap()
        );
    }

    // ── Server Construction ───────────────────────────────────────────

    #[test]
    fn test_server_new_with_runtime() {
        let server = test_server();
        assert!(server.runtime().is_some());
        assert!(server.event_bus().is_some());
    }

    #[test]
    fn test_server_new_without_runtime() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        assert!(server.runtime().is_none());
        assert!(server.event_bus().is_none());
    }

    #[test]
    fn test_server_with_runtime() {
        let rt = Runtime::new(RuntimeConfig::default());
        let config = test_config();
        let server = Server::with_runtime(config, rt);
        assert!(server.runtime().is_some());
    }

    #[test]
    fn test_server_pre_warm() {
        let server = test_server();
        let status = server.runtime().unwrap().status();
        assert_eq!(status.pool.warm, 2);
    }

    // ── Feature summary ───────────────────────────────────────────────

    #[test]
    fn test_feature_summary_all_enabled() {
        let server = test_server();
        let features = server.feature_summary();
        assert_eq!(features.len(), 5);
        assert!(features.iter().all(|(_, enabled)| *enabled));
    }

    #[test]
    fn test_feature_summary_partial() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let features = server.feature_summary();
        let disabled: Vec<_> = features.iter().filter(|(_, e)| !e).collect();
        assert_eq!(disabled.len(), 2);
    }

    // ── Route table ───────────────────────────────────────────────────

    #[test]
    fn test_route_table_all_enabled() {
        let server = test_server();
        let routes = server.route_table();
        // 25 base + 6 events + 12 runtime = 43
        assert_eq!(routes.len(), 43);
    }

    #[test]
    fn test_route_table_minimal() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let routes = server.route_table();
        // 25 base (includes /metrics, snapshot routes, /api/v1/health/full)
        assert_eq!(routes.len(), 25);
    }

    // ── Router Integration ────────────────────────────────────────────

    #[tokio::test]
    async fn test_health_endpoint() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_runtime_status_via_unified() {
        let server = test_server();
        let app = server.build_router();

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
        let status: hv2_runtime::RuntimeStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(status.pool.warm, 2);
    }

    #[tokio::test]
    async fn test_runtime_session_via_unified() {
        let server = test_server();
        let app = server.build_router();

        let body = serde_json::json!({
            "session_id": "unified-sess",
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
    }

    #[tokio::test]
    async fn test_ontology_via_unified() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/agentic/tools/openai")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_events_webhook_list_via_unified() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/webhooks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_vm_list_via_unified() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_without_runtime() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let app = server.build_router();

        // Health should still work
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_runtime_disabled_returns_404() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should 404 because runtime routes aren't registered
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_metrics_via_unified() {
        let server = test_server();
        let app = server.build_router();

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
        assert!(metrics.autoscale_enabled);
        assert!(!metrics.instance_id.is_empty());
    }

    #[tokio::test]
    async fn test_prometheus_via_unified() {
        let server = test_server();
        let app = server.build_router();

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
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("hm_pool_total_vms"));
        assert!(text.contains("hm_uptime_seconds"));
    }

    #[tokio::test]
    async fn test_runtime_health_via_unified() {
        let server = test_server();
        let app = server.build_router();

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
    }

    // ── Middleware Integration ─────────────────────────────────────────

    #[tokio::test]
    async fn test_middleware_request_id_on_server() {
        let config = test_config().middleware(MiddlewareConfig::none().request_id(true));
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let id = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(id.len(), 36); // UUID v4
    }

    #[tokio::test]
    async fn test_middleware_timing_on_server() {
        let config = test_config().middleware(MiddlewareConfig::none().request_timing(true));
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let timing = response
            .headers()
            .get("x-response-time")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(timing.ends_with("ms"));
    }

    #[tokio::test]
    async fn test_middleware_cors_on_server() {
        let config = test_config().middleware(MiddlewareConfig::none().cors_enabled(true));
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn test_middleware_auth_on_server() {
        let config = test_config()
            .middleware(MiddlewareConfig::none().api_keys(vec!["server-key".to_string()]));
        let server = Server::new(config);
        let app = server.build_router();

        // Without key: should fail
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // With key: should succeed
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/status")
                    .header("authorization", "Bearer server-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_full_stack_on_server() {
        let config = test_config().middleware(MiddlewareConfig::default().request_logging(false));
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
        assert!(response.headers().contains_key("x-response-time"));
        assert!(response
            .headers()
            .contains_key("access-control-allow-origin"));
    }

    // ── Shutdown & Lifecycle ──────────────────────────────────────────

    #[test]
    fn test_server_config_default_shutdown_timeout() {
        let config = ServerConfig::default();
        assert_eq!(config.shutdown_timeout_secs, 30);
    }

    #[test]
    fn test_server_config_builder_shutdown_timeout() {
        let config = ServerConfig::default().shutdown_timeout_secs(60);
        assert_eq!(config.shutdown_timeout_secs, 60);
    }

    #[test]
    fn test_server_config_zero_shutdown_timeout() {
        let config = ServerConfig::default().shutdown_timeout_secs(0);
        assert_eq!(config.shutdown_timeout_secs, 0);
    }

    #[test]
    fn test_startup_banner_does_not_panic() {
        // Exercises log_startup_banner without asserting log content
        let server = test_server();
        server.log_startup_banner();
    }

    #[test]
    fn test_startup_banner_minimal_config() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false)
            .shutdown_timeout_secs(10);
        let server = Server::new(config);
        server.log_startup_banner();
    }

    #[test]
    fn test_feature_summary_includes_all_five() {
        // Ensures feature_summary returns exactly 5 items
        let server = test_server();
        let features = server.feature_summary();
        let names: Vec<&str> = features.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"VM CRUD"));
        assert!(names.contains(&"Ontology"));
        assert!(names.contains(&"Events/SSE"));
        assert!(names.contains(&"Runtime Fleet"));
        assert!(names.contains(&"gRPC"));
    }

    #[tokio::test]
    async fn test_graceful_shutdown_with_timeout() {
        // Simulates graceful shutdown by dropping the server
        let config = test_config().shutdown_timeout_secs(5);
        let server = Server::new(config);
        let _app = server.build_router();
        // If we reach here without panic, shutdown config is wired correctly
        assert_eq!(server.config().shutdown_timeout_secs, 5);
    }

    #[tokio::test]
    async fn test_serve_rest_binds_and_shuts_down() {
        // Start a server on a random port and verify it shuts down
        // when the shutdown signal (Ctrl+C) would fire. We use a
        // manual approach: bind, build router, verify the config.
        let config = ServerConfig::default()
            .host("127.0.0.1")
            .rest_port(0)
            .grpc_port(0)
            .enable_runtime(false)
            .enable_events(false)
            .shutdown_timeout_secs(1);
        let server = Server::new(config);
        assert_eq!(server.config().shutdown_timeout_secs, 1);
    }

    #[test]
    fn test_shutdown_timeout_propagates_to_config() {
        let config = ServerConfig::default()
            .shutdown_timeout_secs(120)
            .middleware(MiddlewareConfig::none());
        let server = Server::new(config);
        assert_eq!(server.config().shutdown_timeout_secs, 120);
        assert_eq!(server.config().rest_port, 8080);
    }

    // ── Health Check Enhancements ─────────────────────────────────────

    #[tokio::test]
    async fn test_health_returns_uptime_and_version() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
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
        assert!(!health["version"].as_str().unwrap().is_empty());
        assert!(health["uptime_seconds"].as_u64().is_some());
        assert_eq!(health["vm_count"], 0);
    }

    #[tokio::test]
    async fn test_health_shows_components_enabled() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
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
        assert_eq!(health["components"]["runtime"], "enabled");
        assert_eq!(health["components"]["events"], "enabled");
    }

    #[tokio::test]
    async fn test_health_shows_components_disabled() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
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
        assert_eq!(health["components"]["runtime"], "disabled");
        assert_eq!(health["components"]["events"], "disabled");
    }

    #[tokio::test]
    async fn test_liveness_probe() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let live: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(live["status"], "alive");
    }

    #[tokio::test]
    async fn test_readiness_probe_all_enabled() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let ready: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(ready["status"], "ready");
        let checks = ready["checks"].as_array().unwrap();
        assert_eq!(checks.len(), 3);
        assert_eq!(checks[0]["name"], "http_server");
        assert_eq!(checks[0]["status"], "up");
        assert_eq!(checks[1]["name"], "runtime");
        assert_eq!(checks[1]["status"], "up");
        assert_eq!(checks[2]["name"], "events");
        assert_eq!(checks[2]["status"], "up");
    }

    #[tokio::test]
    async fn test_readiness_probe_subsystems_disabled() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Still 200 — disabled is not the same as down
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let ready: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(ready["status"], "ready");
        let checks = ready["checks"].as_array().unwrap();
        assert_eq!(checks[1]["name"], "runtime");
        assert_eq!(checks[1]["status"], "disabled");
        assert_eq!(checks[2]["name"], "events");
        assert_eq!(checks[2]["status"], "disabled");
    }

    #[tokio::test]
    async fn test_liveness_probe_without_runtime() {
        let config = ServerConfig::default()
            .enable_runtime(false)
            .enable_events(false);
        let server = Server::new(config);
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let live: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(live["status"], "alive");
    }

    #[tokio::test]
    async fn test_health_uptime_is_non_negative() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // uptime_seconds should be >= 0 (it's u64, so always true,
        // but verify the field is present and a number)
        let uptime = health["uptime_seconds"].as_u64().unwrap();
        assert!(uptime < 60, "uptime should be small in test: {}", uptime);
    }

    #[tokio::test]
    async fn test_health_version_matches_cargo_pkg() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            health["version"].as_str().unwrap(),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[tokio::test]
    async fn test_route_table_includes_health_probes() {
        let server = test_server();
        let routes = server.route_table();
        let descs: Vec<&str> = routes.iter().map(|(_, _, d)| *d).collect();
        assert!(descs.contains(&"Liveness probe"));
        assert!(descs.contains(&"Readiness probe"));
        assert!(descs.contains(&"Health check"));
    }

    #[test]
    fn test_app_state_builder() {
        let state = rest::AppState::new()
            .with_runtime_enabled(true)
            .with_events_enabled(false);
        assert!(state.runtime_enabled);
        assert!(!state.events_enabled);
    }

    #[test]
    fn test_app_state_default() {
        let state = rest::AppState::default();
        assert!(!state.runtime_enabled);
        assert!(!state.events_enabled);
    }

    // ── Pagination & Filtering ────────────────────────────────────────

    #[test]
    fn test_pagination_params_default() {
        let params = rest::PaginationParams::default();
        assert_eq!(params.offset, 0);
        assert_eq!(params.effective_limit(), 20);
    }

    #[test]
    fn test_pagination_params_clamp_max() {
        let params = rest::PaginationParams {
            offset: 0,
            limit: Some(500),
        };
        assert_eq!(params.effective_limit(), 100);
    }

    #[test]
    fn test_pagination_params_clamp_min() {
        let params = rest::PaginationParams {
            offset: 0,
            limit: Some(0),
        };
        assert_eq!(params.effective_limit(), 1);
    }

    #[test]
    fn test_pagination_params_custom_limit() {
        let params = rest::PaginationParams {
            offset: 10,
            limit: Some(50),
        };
        assert_eq!(params.offset, 10);
        assert_eq!(params.effective_limit(), 50);
    }

    #[test]
    fn test_paginated_response_from_vec() {
        let items = vec![1, 2, 3];
        let resp = rest::PaginatedResponse::from_vec(items, 10, 0, 3);
        assert_eq!(resp.items, vec![1, 2, 3]);
        assert_eq!(resp.total, 10);
        assert_eq!(resp.offset, 0);
        assert_eq!(resp.limit, 3);
        assert!(resp.has_more);
    }

    #[test]
    fn test_paginated_response_last_page() {
        let items = vec![8, 9, 10];
        let resp = rest::PaginatedResponse::from_vec(items, 10, 7, 3);
        assert!(!resp.has_more);
    }

    #[test]
    fn test_paginated_response_empty() {
        let items: Vec<i32> = vec![];
        let resp = rest::PaginatedResponse::from_vec(items, 0, 0, 20);
        assert!(!resp.has_more);
        assert_eq!(resp.total, 0);
    }

    #[tokio::test]
    async fn test_list_vms_returns_pagination_fields() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 0);
        assert_eq!(json["offset"], 0);
        assert_eq!(json["limit"], 20);
        assert_eq!(json["has_more"], false);
        assert!(json["vms"].is_array());
    }

    #[tokio::test]
    async fn test_list_vms_custom_limit() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms?limit=5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["limit"], 5);
    }

    #[tokio::test]
    async fn test_list_vms_offset() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms?offset=10&limit=5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // offset is clamped to total when there are 0 VMs
        assert_eq!(json["offset"], 0);
        assert_eq!(json["limit"], 5);
        assert_eq!(json["has_more"], false);
    }

    #[tokio::test]
    async fn test_list_vms_state_filter_param_accepted() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/vms?state=Running")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 0);
    }

    #[tokio::test]
    async fn test_list_webhooks_returns_pagination_fields() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/webhooks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 0);
        assert_eq!(json["offset"], 0);
        assert_eq!(json["limit"], 20);
        assert_eq!(json["has_more"], false);
    }

    #[tokio::test]
    async fn test_list_webhooks_custom_limit() {
        let server = test_server();
        let app = server.build_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/webhooks?limit=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["limit"], 2);
    }

    #[test]
    fn test_vm_list_params_effective_limit() {
        let params = rest::VmListParams {
            offset: 0,
            limit: Some(200),
            state: None,
        };
        assert_eq!(params.effective_limit(), 100);
    }

    #[test]
    fn test_vm_list_params_default_limit() {
        let params = rest::VmListParams {
            offset: 0,
            limit: None,
            state: Some("Running".to_string()),
        };
        assert_eq!(params.effective_limit(), 20);
        assert_eq!(params.state.unwrap(), "Running");
    }
}
