//! Approving an image over REST decides whether a VM can boot it.
//!
//! `VM::provision` gained an admission check, but the `/api/v1/images` routes
//! and the VMs the API creates held *separate* registry instances — so an
//! operator could revoke an image through the API and watch a VM boot it
//! anyway. These tests pin that the two now share one registry.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hv2_api::rest::{create_router_with_state, AppState};
use hv2_core::security::image_registry::{
    EnforcementMode, ImageEntry, ImageKind, ImageRegistry, RegistryConfig,
};
use serde_json::json;
use tower::ServiceExt;

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

fn bzimage(salt: u8) -> Vec<u8> {
    let mut image = vec![0u8; 8192];
    image[0x1F1] = 4; // setup_sects
    image[0x1FE] = 0x55;
    image[0x1FF] = 0xAA;
    image[0x202..0x206].copy_from_slice(b"HdrS");
    image[0x206] = 0x0C; // protocol 2.12
    image[0x207] = 0x02;
    image[0x1000] = salt;
    image
}

fn temp_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("hv2-rest-admission-{name}"));
    std::fs::write(&path, bytes).expect("write temp boot image");
    path
}

fn digest_of(kernel: &std::path::Path) -> String {
    hv2_core::boot::source::BootSource::linux(kernel)
        .load()
        .expect("load")
        .primary_image_digest()
        .expect("digest")
}

/// A registry that enforces but does not demand signatures, so a test can
/// approve an image without standing up a signing chain.
fn enforcing_registry() -> Arc<ImageRegistry> {
    Arc::new(ImageRegistry::new(RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: false,
        trusted_signers: Vec::new(),
    }))
}

async fn create_vm(
    app: axum::Router,
    name: &str,
    kernel: &std::path::Path,
) -> (StatusCode, String, axum::Router) {
    let body = json!({
        "name": name,
        "vcpu_count": 1,
        "memory_gb": 1,
        "boot": { "type": "linux", "kernel": kernel, "cmdline": "" }
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/vms")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&bytes).into_owned();

    (status, body, app)
}

/// Whether VM creation failed because the machine has no usable hypervisor.
///
/// A runner with /dev/kvm present but not accessible fails here, which says
/// nothing about image admission -- the subject of these tests. Callers skip
/// on this, matching how the hv2-core tests already skip without a backend.
fn no_hypervisor(status: StatusCode, body: &str) -> bool {
    status != StatusCode::CREATED
        && (body.contains("/dev/kvm")
            || body.contains("Hypervisor error")
            || body.contains("hypervisor"))
}

/// Start a VM and return the response body.
///
/// The status alone cannot decide these tests: starting a Linux guest fails
/// with 500 on a host with no usable hypervisor whether or not admission
/// refused it. The body distinguishes the two — an admission refusal names the
/// image registry.
async fn start_vm(app: &axum::Router, name: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/vms/{name}/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether a start response was refused by image admission specifically.
fn refused_by_admission(body: &str) -> bool {
    body.contains("image registry")
}

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn without_a_registry_the_api_boots_any_image() {
    // The default has to stay open, or every existing deployment breaks.
    let kernel = temp_image("open", &bzimage(1));
    let app = create_router_with_state(AppState::new());

    let (status, body, app) = create_vm(app, "open-vm", &kernel).await;
    if no_hypervisor(status, &body) {
        eprintln!("skipping: no usable hypervisor ({body})");
        return;
    }
    assert_eq!(status, StatusCode::CREATED, "creation should succeed");

    let body = start_vm(&app, "open-vm").await;
    assert!(
        !refused_by_admission(&body),
        "no registry installed, so admission must not refuse: {body}"
    );
}

#[tokio::test]
async fn an_approved_image_starts() {
    let kernel = temp_image("rest-approved", &bzimage(2));
    let registry = enforcing_registry();

    registry
        .register(ImageEntry::new(
            "internal/kernels/rest-ok:1",
            ImageKind::Kernel,
            digest_of(&kernel),
        ))
        .unwrap();
    registry
        .approve("internal/kernels/rest-ok:1", "reviewer")
        .unwrap();

    let app = create_router_with_state(AppState::new().with_image_registry(registry));

    let (status, body, app) = create_vm(app, "approved-vm", &kernel).await;
    if no_hypervisor(status, &body) {
        eprintln!("skipping: no usable hypervisor ({body})");
        return;
    }
    assert_eq!(status, StatusCode::CREATED);

    let body = start_vm(&app, "approved-vm").await;
    assert!(
        !refused_by_admission(&body),
        "an approved image must not be refused by admission: {body}"
    );
}

#[tokio::test]
async fn an_unapproved_image_cannot_start() {
    // The whole point: the registry the operator manages decides whether the
    // VM boots, not merely what a query returns.
    let kernel = temp_image("rest-unapproved", &bzimage(3));
    let app = create_router_with_state(AppState::new().with_image_registry(enforcing_registry()));

    let (status, body, app) = create_vm(app, "denied-vm", &kernel).await;
    if no_hypervisor(status, &body) {
        eprintln!("skipping: no usable hypervisor ({body})");
        return;
    }
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creation succeeds; admission is a boot-time decision"
    );

    let body = start_vm(&app, "denied-vm").await;
    assert!(
        refused_by_admission(&body),
        "an image the registry does not admit must be refused by admission: {body}"
    );
}

#[tokio::test]
async fn revoking_an_image_stops_it_booting() {
    let kernel = temp_image("rest-revoked", &bzimage(4));
    let registry = enforcing_registry();
    let reference = "internal/kernels/rest-revoked:1";

    registry
        .register(ImageEntry::new(
            reference,
            ImageKind::Kernel,
            digest_of(&kernel),
        ))
        .unwrap();
    registry.approve(reference, "reviewer").unwrap();

    let app = create_router_with_state(AppState::new().with_image_registry(Arc::clone(&registry)));
    let (status, body, app) = create_vm(app, "revoked-vm", &kernel).await;
    if no_hypervisor(status, &body) {
        eprintln!("skipping: no usable hypervisor ({body})");
        return;
    }
    assert_eq!(status, StatusCode::CREATED);

    // Revoke through the same registry the routes serve.
    registry
        .revoke(reference, "reviewer", "CVE-2026-0002")
        .unwrap();

    let body = start_vm(&app, "revoked-vm").await;
    assert!(
        refused_by_admission(&body),
        "revocation must take effect on the boot path: {body}"
    );
    assert!(
        body.contains("revoked"),
        "the refusal should say why: {body}"
    );
}
