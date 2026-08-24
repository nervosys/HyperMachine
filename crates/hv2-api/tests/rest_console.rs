//! `GET /api/v1/vms/{id}/console` — reading a guest's boot log over HTTP.
//!
//! The route exists so an operator does not have to attach a debugger to find
//! out what a guest printed. What these tests pin is the honesty of the
//! answer: an unknown VM is a 404 rather than an empty log, and a VM with no
//! console device says so through `attached` instead of returning "" and
//! letting the caller assume the guest booted silently.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hv2_api::rest::{create_router_with_state, AppState};
use serde_json::json;
use tower::ServiceExt;

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

/// Create a VM with no boot source, or return `None` when this machine has no
/// usable hypervisor — `AgentVM` allocates a real VM underneath.
async fn create_vm(app: &axum::Router, name: &str) -> Option<String> {
    let body = json!({ "name": name, "vcpu_count": 1, "memory_gb": 1 });
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
    if status != StatusCode::CREATED {
        eprintln!(
            "skipping: could not create VM ({status}): {}",
            String::from_utf8_lossy(&bytes)
        );
        return None;
    }

    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    Some(created["id"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn the_console_of_an_unknown_vm_is_a_404() {
    let app = create_router_with_state(AppState::new());

    let (status, body) = get(&app, "/api/v1/vms/ghost/console").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], json!("VM_NOT_FOUND"));
}

#[tokio::test]
async fn a_vm_with_no_console_device_says_nothing_is_attached() {
    let app = create_router_with_state(AppState::new());
    let Some(id) = create_vm(&app, "console-none").await else {
        return;
    };

    let (status, body) = get(&app, &format!("/api/v1/vms/{id}/console")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], json!(id));
    assert_eq!(
        body["attached"],
        json!(false),
        "nothing registers a serial device automatically; the caller must learn that"
    );
    assert_eq!(body["output"], json!(""));
}

#[tokio::test]
async fn reading_the_console_twice_returns_the_same_log() {
    let app = create_router_with_state(AppState::new());
    let Some(id) = create_vm(&app, "console-poll").await else {
        return;
    };

    let uri = format!("/api/v1/vms/{id}/console");
    let (_, first) = get(&app, &uri).await;
    let (_, second) = get(&app, &uri).await;

    // Polling is the expected access pattern, so the route must not drain.
    assert_eq!(first, second);
}
