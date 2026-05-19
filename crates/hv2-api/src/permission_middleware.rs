//! Permission middleware — wires the graph-based permission system into the API.
//!
//! When enabled, each request is checked against the [`PermissionGraph`] before
//! being forwarded to the handler. The middleware:
//!
//! 1. Extracts the API key from the `Authorization` header.
//! 2. Looks up the associated [`PrincipalId`] in the key→principal map.
//! 3. Determines the required [`Permission`] from the request method + path.
//! 4. Resolves effective permissions via [`ResolutionEngine`].
//! 5. Returns 403 if the permission is denied.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use hv2_agent::permissions::{
    Permission, PermissionGraph, PrincipalId, ResolutionEngine, ResourceScope,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for the permission middleware.
#[derive(Clone)]
pub struct PermissionMiddlewareConfig {
    /// Map from API key → principal ID.
    pub key_principal_map: HashMap<String, PrincipalId>,
    /// The permission graph (shared, thread-safe via internal RwLock).
    pub graph: Arc<PermissionGraph>,
    /// Default scope for permission checks (configurable per-deployment).
    pub default_scope: ResourceScope,
    /// Path prefixes excluded from permission checks.
    pub excluded_paths: Vec<String>,
}

impl PermissionMiddlewareConfig {
    /// Check if a path is excluded from permission enforcement.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Error response body for permission denials.
#[derive(Serialize)]
struct PermDenied {
    error: String,
    code: &'static str,
}

/// Determine the required [`Permission`] for a given HTTP method + path.
///
/// Returns `None` if the route doesn't require a specific permission
/// (e.g. health checks, ontology endpoints).
pub fn required_permission(method: &axum::http::Method, path: &str) -> Option<Permission> {
    use axum::http::Method;

    // Normalize trailing slashes
    let path = path.trim_end_matches('/');

    // VM lifecycle routes
    if path.ends_with("/start") {
        return Some(Permission::VmStart);
    }
    if path.ends_with("/stop") {
        return Some(Permission::VmStop);
    }
    if path.ends_with("/pause") {
        return Some(Permission::VmPause);
    }
    if path.ends_with("/resume") {
        return Some(Permission::VmResume);
    }
    if path.ends_with("/script") {
        return Some(Permission::GuestExec);
    }

    // Snapshot routes
    if path.contains("/snapshots") {
        if path.ends_with("/restore") {
            return Some(Permission::SnapshotRestore);
        }
        return match *method {
            Method::POST => Some(Permission::SnapshotCreate),
            Method::DELETE => Some(Permission::SnapshotDelete),
            _ => Some(Permission::VmRead),
        };
    }

    // Metrics (check before generic VM CRUD)
    if path.contains("/metrics") {
        return Some(Permission::MetricsRead);
    }

    // VM CRUD
    if path.starts_with("/api/v1/vms") {
        return match *method {
            Method::POST => Some(Permission::VmCreate),
            Method::DELETE => Some(Permission::VmDelete),
            Method::GET => Some(Permission::VmRead),
            Method::PUT | Method::PATCH => Some(Permission::VmConfigure),
            _ => None,
        };
    }

    // Runtime fleet
    if path.starts_with("/api/v1/runtime") {
        return match *method {
            Method::GET => Some(Permission::MetricsRead),
            _ => Some(Permission::AdminConfig),
        };
    }

    // GPU fabric
    if path.starts_with("/api/v1/gpu-fabric") {
        return match *method {
            Method::GET => Some(Permission::GpuConfigure),
            _ => Some(Permission::GpuConfigure),
        };
    }

    // Image registry
    if path.starts_with("/api/v1/images") {
        return match *method {
            Method::GET => Some(Permission::VmRead),
            _ => Some(Permission::AdminConfig),
        };
    }

    None
}

/// Axum middleware handler for permission enforcement.
///
/// This is called by a closure from [`MiddlewareConfig::apply`].
pub fn permission_handler(
    config: PermissionMiddlewareConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        // Determine required permission
        let required = match required_permission(request.method(), &path) {
            Some(p) => p,
            None => return next.run(request).await,
        };

        // Extract API key from Authorization header
        let auth = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.strip_prefix("Bearer ").unwrap_or(v));

        let key = match auth {
            Some(k) => k,
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(PermDenied {
                        error: "Missing authorization".to_string(),
                        code: "UNAUTHORIZED",
                    }),
                )
                    .into_response();
            }
        };

        // Look up principal
        let principal_id = match config.key_principal_map.get(key) {
            Some(pid) => pid.clone(),
            None => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(PermDenied {
                        error: "Unknown API key".to_string(),
                        code: "FORBIDDEN",
                    }),
                )
                    .into_response();
            }
        };

        // Resolve effective permissions
        match ResolutionEngine::resolve(&config.graph, &principal_id, &config.default_scope) {
            Ok(effective) => {
                if effective.allows(&required) {
                    next.run(request).await
                } else {
                    (
                        StatusCode::FORBIDDEN,
                        Json(PermDenied {
                            error: format!("Permission denied: {required}"),
                            code: "PERMISSION_DENIED",
                        }),
                    )
                        .into_response()
                }
            }
            Err(_) => (
                StatusCode::FORBIDDEN,
                Json(PermDenied {
                    error: "Permission resolution failed".to_string(),
                    code: "PERMISSION_ERROR",
                }),
            )
                .into_response(),
        }
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[test]
    fn test_required_permission_vm_crud() {
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms"),
            Some(Permission::VmCreate)
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/vms"),
            Some(Permission::VmRead)
        );
        assert_eq!(
            required_permission(&Method::DELETE, "/api/v1/vms/123"),
            Some(Permission::VmDelete)
        );
        assert_eq!(
            required_permission(&Method::PUT, "/api/v1/vms/123"),
            Some(Permission::VmConfigure)
        );
    }

    #[test]
    fn test_required_permission_lifecycle() {
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms/123/start"),
            Some(Permission::VmStart)
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms/123/stop"),
            Some(Permission::VmStop)
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms/123/pause"),
            Some(Permission::VmPause)
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms/123/resume"),
            Some(Permission::VmResume)
        );
    }

    #[test]
    fn test_required_permission_snapshots() {
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms/123/snapshots"),
            Some(Permission::SnapshotCreate)
        );
        assert_eq!(
            required_permission(&Method::DELETE, "/api/v1/vms/123/snapshots/s1"),
            Some(Permission::SnapshotDelete)
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/vms/123/snapshots/s1/restore"),
            Some(Permission::SnapshotRestore)
        );
    }

    #[test]
    fn test_required_permission_none_for_health() {
        assert_eq!(required_permission(&Method::GET, "/health"), None);
        assert_eq!(required_permission(&Method::GET, "/agentic/ontology"), None);
    }

    #[test]
    fn test_required_permission_metrics() {
        assert_eq!(
            required_permission(&Method::GET, "/metrics"),
            Some(Permission::MetricsRead)
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/vms/123/metrics"),
            Some(Permission::MetricsRead)
        );
    }

    #[test]
    fn test_required_permission_runtime() {
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/runtime/status"),
            Some(Permission::MetricsRead)
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/runtime/maintenance"),
            Some(Permission::AdminConfig)
        );
    }

    #[test]
    fn test_excluded_paths() {
        let config = PermissionMiddlewareConfig {
            key_principal_map: HashMap::new(),
            graph: Arc::new(PermissionGraph::new()),
            default_scope: ResourceScope::Root,
            excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
        };
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/live"));
        assert!(config.is_excluded("/agentic/ontology"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_permission_resolution_integration() {
        use hv2_agent::permissions::{PermissionSet, PrincipalKind};

        let graph = PermissionGraph::new();
        let pid = PrincipalId("agent-1".to_string());
        graph
            .add_principal(pid.clone(), PrincipalKind::Agent, "Test Agent".to_string())
            .expect("add_principal");

        let perms = PermissionSet::new()
            .with(Permission::VmCreate)
            .with(Permission::VmRead)
            .with(Permission::VmStart)
            .with(Permission::VmStop);

        graph
            .grant(pid.clone(), perms, ResourceScope::Root, None, 0, None)
            .expect("grant");

        let effective = ResolutionEngine::resolve(&graph, &pid, &ResourceScope::Root).unwrap();

        assert!(effective.allows(&Permission::VmCreate));
        assert!(effective.allows(&Permission::VmRead));
        assert!(!effective.allows(&Permission::AdminConfig));
    }
}
