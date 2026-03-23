//! Image Registry REST API Routes
//!
//! Exposes the fleet image allowlist registry — registration, approval,
//! admission checks, and lifecycle management — as REST endpoints
//! under `/api/v1/images/...`.
//!
//! ## Route Groups
//!
//! | Method   | Path                                  | Description               |
//! |----------|---------------------------------------|---------------------------|
//! | `GET`    | `/api/v1/images`                      | List images (optional filter) |
//! | `GET`    | `/api/v1/images/:reference`           | Get image detail          |
//! | `POST`   | `/api/v1/images`                      | Register a new image      |
//! | `POST`   | `/api/v1/images/:reference/approve`   | Approve an image          |
//! | `POST`   | `/api/v1/images/:reference/deny`      | Deny an image             |
//! | `POST`   | `/api/v1/images/:reference/revoke`    | Revoke an image           |
//! | `POST`   | `/api/v1/images/:reference/deprecate` | Deprecate an image        |
//! | `POST`   | `/api/v1/images/check-admission`      | Check admission for an image |
//! | `GET`    | `/api/v1/images/config`               | Get registry configuration |

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use hv2_core::security::image_registry::{
    AdmissionDecision, ApprovalStatus, EnforcementMode, ImageEntry, ImageKind, ImageRegistry,
    ImageSignature, RegistryConfig,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;

// ============================================================================
// Shared State
// ============================================================================

/// Application state wrapping the image registry.
#[derive(Clone)]
pub struct ImageRegistryAppState {
    pub registry: Arc<ImageRegistry>,
}

impl Default for ImageRegistryAppState {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageRegistryAppState {
    /// Create with default enforce + require-signature config.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(ImageRegistry::new(RegistryConfig::default())),
        }
    }

    /// Create with a custom config.
    pub fn with_config(config: RegistryConfig) -> Self {
        Self {
            registry: Arc::new(ImageRegistry::new(config)),
        }
    }
}

// ============================================================================
// DTOs
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RegisterImageRequest {
    pub reference: String,
    pub kind: String,
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub signatures: Vec<SignatureDto>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SignatureDto {
    pub signer: String,
    pub algorithm: String,
    pub signature_hex: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageResponse {
    pub reference: String,
    pub kind: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub status: String,
    pub labels: std::collections::HashMap<String, String>,
    pub signature_count: usize,
    pub has_verified_signature: bool,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct ReviewRequest {
    pub reviewer: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct AdmissionCheckRequest {
    pub reference: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdmissionCheckResponse {
    pub reference: String,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListImagesQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageListResponse {
    pub count: usize,
    pub images: Vec<ImageResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryConfigResponse {
    pub mode: String,
    pub require_signature: bool,
    pub trusted_signers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RegistryErrorResponse {
    pub error: String,
    pub code: String,
}

// ============================================================================
// Conversions
// ============================================================================

fn image_kind_from_str(s: &str) -> Option<ImageKind> {
    match s {
        "vm_image" => Some(ImageKind::VmImage),
        "container" => Some(ImageKind::Container),
        "kernel" => Some(ImageKind::Kernel),
        "initramfs" => Some(ImageKind::Initramfs),
        "firmware" => Some(ImageKind::Firmware),
        other => Some(ImageKind::Custom(other.to_string())),
    }
}

fn image_kind_to_str(kind: &ImageKind) -> String {
    match kind {
        ImageKind::VmImage => "vm_image".to_string(),
        ImageKind::Container => "container".to_string(),
        ImageKind::Kernel => "kernel".to_string(),
        ImageKind::Initramfs => "initramfs".to_string(),
        ImageKind::Firmware => "firmware".to_string(),
        ImageKind::Custom(s) => s.clone(),
    }
}

fn approval_status_to_str(s: ApprovalStatus) -> &'static str {
    match s {
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::PendingReview => "pending_review",
        ApprovalStatus::Denied => "denied",
        ApprovalStatus::Deprecated => "deprecated",
        ApprovalStatus::Revoked => "revoked",
    }
}

fn approval_status_from_str(s: &str) -> Option<ApprovalStatus> {
    match s {
        "approved" => Some(ApprovalStatus::Approved),
        "pending_review" => Some(ApprovalStatus::PendingReview),
        "denied" => Some(ApprovalStatus::Denied),
        "deprecated" => Some(ApprovalStatus::Deprecated),
        "revoked" => Some(ApprovalStatus::Revoked),
        _ => None,
    }
}

fn enforcement_mode_to_str(m: EnforcementMode) -> &'static str {
    match m {
        EnforcementMode::Audit => "audit",
        EnforcementMode::Enforce => "enforce",
        EnforcementMode::Disabled => "disabled",
    }
}

fn entry_to_response(entry: &ImageEntry) -> ImageResponse {
    ImageResponse {
        reference: entry.reference.clone(),
        kind: image_kind_to_str(&entry.kind),
        sha256: entry.sha256.clone(),
        size_bytes: entry.size_bytes,
        status: approval_status_to_str(entry.status).to_string(),
        labels: entry.labels.clone(),
        signature_count: entry.signatures.len(),
        has_verified_signature: entry.has_verified_signature(),
        notes: entry.notes.clone(),
    }
}

// ============================================================================
// Router
// ============================================================================

/// Create the image registry API router.
pub fn create_image_registry_router(state: Arc<ImageRegistryAppState>) -> Router {
    Router::new()
        .route("/api/v1/images", get(list_images).post(register_image))
        .route("/api/v1/images/config", get(get_config))
        .route("/api/v1/images/check-admission", post(check_admission))
        .route("/api/v1/images/:reference", get(get_image))
        .route("/api/v1/images/:reference/approve", post(approve_image))
        .route("/api/v1/images/:reference/deny", post(deny_image))
        .route("/api/v1/images/:reference/revoke", post(revoke_image))
        .route("/api/v1/images/:reference/deprecate", post(deprecate_image))
        .with_state(state)
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/v1/images
async fn list_images(
    State(state): State<Arc<ImageRegistryAppState>>,
    Query(query): Query<ListImagesQuery>,
) -> impl IntoResponse {
    // Filter by status if provided
    if let Some(ref status_str) = query.status {
        if let Some(status) = approval_status_from_str(status_str) {
            let images = state.registry.list_by_status(status);
            return Json(ImageListResponse {
                count: images.len(),
                images: images.iter().map(entry_to_response).collect(),
            })
            .into_response();
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(RegistryErrorResponse {
                error: format!("Invalid status filter: {status_str}"),
                code: "INVALID_STATUS".into(),
            }),
        )
            .into_response();
    }

    // Filter by kind if provided
    if let Some(ref kind_str) = query.kind {
        if let Some(kind) = image_kind_from_str(kind_str) {
            let images = state.registry.list_by_kind(&kind);
            return Json(ImageListResponse {
                count: images.len(),
                images: images.iter().map(entry_to_response).collect(),
            })
            .into_response();
        }
    }

    // No filter — return count summary
    let count = state.registry.image_count();
    Json(ImageListResponse {
        count,
        images: Vec::new(),
    })
    .into_response()
}

/// GET /api/v1/images/:reference
async fn get_image(
    State(state): State<Arc<ImageRegistryAppState>>,
    Path(reference): Path<String>,
) -> impl IntoResponse {
    match state.registry.get(&reference) {
        Some(entry) => Json(entry_to_response(&entry)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(RegistryErrorResponse {
                error: format!("Image not found: {reference}"),
                code: "NOT_FOUND".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/images
async fn register_image(
    State(state): State<Arc<ImageRegistryAppState>>,
    Json(req): Json<RegisterImageRequest>,
) -> impl IntoResponse {
    let kind = match image_kind_from_str(&req.kind) {
        Some(k) => k,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RegistryErrorResponse {
                    error: format!("Unknown image kind: {}", req.kind),
                    code: "INVALID_KIND".into(),
                }),
            )
                .into_response();
        }
    };

    let mut entry = ImageEntry::new(&req.reference, kind, &req.sha256).size(req.size_bytes);

    for (k, v) in &req.labels {
        entry = entry.label(k, v);
    }

    for sig in &req.signatures {
        entry = entry.signature(ImageSignature {
            signer: sig.signer.clone(),
            algorithm: sig.algorithm.clone(),
            signature_hex: sig.signature_hex.clone(),
            signed_at: SystemTime::now(),
            verified: sig.verified,
        });
    }

    match state.registry.register(entry) {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(RegistryErrorResponse {
                error: e.to_string(),
                code: "REGISTER_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/images/:reference/approve
async fn approve_image(
    State(state): State<Arc<ImageRegistryAppState>>,
    Path(reference): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> impl IntoResponse {
    match state.registry.approve(&reference, &req.reviewer) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            let status = if e.to_string().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (
                status,
                Json(RegistryErrorResponse {
                    error: e.to_string(),
                    code: "APPROVE_FAILED".into(),
                }),
            )
                .into_response()
        }
    }
}

/// POST /api/v1/images/:reference/deny
async fn deny_image(
    State(state): State<Arc<ImageRegistryAppState>>,
    Path(reference): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> impl IntoResponse {
    match state.registry.deny(&reference, &req.reviewer, &req.reason) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(RegistryErrorResponse {
                error: e.to_string(),
                code: "DENY_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/images/:reference/revoke
async fn revoke_image(
    State(state): State<Arc<ImageRegistryAppState>>,
    Path(reference): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> impl IntoResponse {
    match state
        .registry
        .revoke(&reference, &req.reviewer, &req.reason)
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(RegistryErrorResponse {
                error: e.to_string(),
                code: "REVOKE_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/images/:reference/deprecate
async fn deprecate_image(
    State(state): State<Arc<ImageRegistryAppState>>,
    Path(reference): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> impl IntoResponse {
    match state
        .registry
        .deprecate(&reference, &req.reviewer, &req.reason)
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(RegistryErrorResponse {
                error: e.to_string(),
                code: "DEPRECATE_FAILED".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/images/check-admission
async fn check_admission(
    State(state): State<Arc<ImageRegistryAppState>>,
    Json(req): Json<AdmissionCheckRequest>,
) -> impl IntoResponse {
    let decision = state.registry.check_admission(&req.reference);
    let (decision_str, message) = match decision {
        AdmissionDecision::Allowed => ("allowed", None),
        AdmissionDecision::AllowedWithWarning(msg) => ("allowed_with_warning", Some(msg)),
        AdmissionDecision::Denied(msg) => ("denied", Some(msg)),
    };
    Json(AdmissionCheckResponse {
        reference: req.reference,
        decision: decision_str.to_string(),
        message,
    })
}

/// GET /api/v1/images/config
async fn get_config(State(state): State<Arc<ImageRegistryAppState>>) -> impl IntoResponse {
    let config = state.registry.config();
    Json(RegistryConfigResponse {
        mode: enforcement_mode_to_str(config.mode).to_string(),
        require_signature: config.require_signature,
        trusted_signers: config.trusted_signers.clone(),
    })
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
        let state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        }));
        create_image_registry_router(state)
    }

    fn signed_app() -> Router {
        let state = Arc::new(ImageRegistryAppState::new());
        create_image_registry_router(state)
    }

    #[tokio::test]
    async fn list_images_empty() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/images")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: ImageListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 0);
    }

    #[tokio::test]
    async fn register_and_get_image() {
        let state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        }));
        let app = create_image_registry_router(state);

        // Register
        let body = serde_json::json!({
            "reference": "ubuntu:22.04",
            "kind": "vm_image",
            "sha256": "abc123",
            "size_bytes": 1048576,
            "labels": {"os": "ubuntu"}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Get
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/images/ubuntu:22.04")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let img: ImageResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(img.reference, "ubuntu:22.04");
        assert_eq!(img.kind, "vm_image");
        assert_eq!(img.status, "pending_review");
    }

    #[tokio::test]
    async fn register_duplicate_returns_conflict() {
        let state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        }));
        let app = create_image_registry_router(state);
        let body = serde_json::json!({
            "reference": "img:1",
            "kind": "container",
            "sha256": "sha1"
        });
        let req_fn = || {
            Request::builder()
                .method("POST")
                .uri("/api/v1/images")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        };
        let resp = app.clone().oneshot(req_fn()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app.oneshot(req_fn()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn approve_and_check_admission() {
        let state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        }));
        let app = create_image_registry_router(state);

        // Register
        let reg = serde_json::json!({
            "reference": "img:1",
            "kind": "vm_image",
            "sha256": "sha1"
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&reg).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Approve
        let review = serde_json::json!({"reviewer": "admin"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images/img:1/approve")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&review).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Check admission
        let check = serde_json::json!({"reference": "img:1"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images/check-admission")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&check).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: AdmissionCheckResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.decision, "allowed");
    }

    #[tokio::test]
    async fn deny_blocks_admission() {
        let state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        }));
        let app = create_image_registry_router(state);

        // Register
        let reg = serde_json::json!({
            "reference": "bad:1",
            "kind": "container",
            "sha256": "sha1"
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&reg).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Deny
        let review = serde_json::json!({"reviewer": "admin", "reason": "CVE"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images/bad:1/deny")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&review).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Check admission
        let check = serde_json::json!({"reference": "bad:1"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images/check-admission")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&check).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: AdmissionCheckResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.decision, "denied");
    }

    #[tokio::test]
    async fn get_config_returns_ok() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/images/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let config: RegistryConfigResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(config.mode, "enforce");
    }

    #[tokio::test]
    async fn not_found_image_returns_404() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/images/nonexistent:1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn deprecate_warns_on_admission() {
        let state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        }));
        let app = create_image_registry_router(state);

        // Register
        let reg = serde_json::json!({
            "reference": "old:1",
            "kind": "vm_image",
            "sha256": "sha1"
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&reg).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Deprecate
        let review = serde_json::json!({"reviewer": "admin", "reason": "EOL"});
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images/old:1/deprecate")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&review).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Check admission
        let check = serde_json::json!({"reference": "old:1"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/images/check-admission")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&check).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: AdmissionCheckResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.decision, "allowed_with_warning");
    }
}
