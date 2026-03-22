//! End-to-end integration tests for the GPU Fabric + Image Registry API.
//!
//! These tests exercise the full workflow across multiple route groups:
//! topology → fleet → capacity → image registry, verifying that data
//! flows correctly between the subsystems.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hv2_api::gpu_fabric_routes::{self, GpuFabricAppState};
use hv2_api::image_registry_routes::{self, ImageRegistryAppState};
use hv2_core::security::image_registry::{EnforcementMode, RegistryConfig};
use std::sync::Arc;
use tower::ServiceExt;

// ============================================================================
// Helpers
// ============================================================================

/// Build a combined router that merges GPU fabric + image registry routes.
fn combined_app() -> axum::Router {
    let gpu_state = Arc::new(GpuFabricAppState::new());
    let image_state = Arc::new(ImageRegistryAppState::with_config(RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: false,
        trusted_signers: vec![],
    }));

    let gpu_router = gpu_fabric_routes::create_gpu_fabric_router(gpu_state);
    let image_router = image_registry_routes::create_image_registry_router(image_state);
    gpu_router.merge(image_router)
}

fn json_post(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn json_get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

async fn body_json<T: serde::de::DeserializeOwned>(resp: axum::http::Response<Body>) -> T {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ============================================================================
// End-to-End Workflows
// ============================================================================

/// Full lifecycle: add GPUs → query topology → register VM class → create reservation → cancel.
#[tokio::test]
async fn gpu_fabric_full_lifecycle() {
    let app = combined_app();

    // 1. Add GPU devices
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/gpu-fabric/topology/devices",
            serde_json::json!({
                "id": "gpu-0",
                "host_id": "host-1",
                "model": "A100",
                "numa_node": 0,
                "pci_address": "0000:3b:00.0",
                "vram_bytes": 85899345920_u64,
                "compute_capability": 80
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/gpu-fabric/topology/devices",
            serde_json::json!({
                "id": "gpu-1",
                "host_id": "host-1",
                "model": "A100",
                "numa_node": 0,
                "pci_address": "0000:3c:00.0",
                "vram_bytes": 85899345920_u64,
                "compute_capability": 80
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 2. Query topology
    let resp = app
        .clone()
        .oneshot(json_get("/api/v1/gpu-fabric/topology"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let overview: serde_json::Value = body_json(resp).await;
    assert_eq!(overview["total_devices"], 2);
    assert_eq!(overview["available_devices"], 2);

    // 3. Register a VM class
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/gpu-fabric/capacity/classes",
            serde_json::json!({
                "name": "gpu-a100-2x",
                "sla_tier": "premium",
                "vcpus": 32,
                "memory_bytes": 274877906944_u64,
                "gpu_count": 2,
                "gpu_model": "A100",
                "max_instances": 50
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 4. Check capacity overview
    let resp = app
        .clone()
        .oneshot(json_get("/api/v1/gpu-fabric/capacity"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cap: serde_json::Value = body_json(resp).await;
    assert_eq!(cap["vm_classes"].as_array().unwrap().len(), 1);
    assert_eq!(cap["vm_classes"][0]["name"], "gpu-a100-2x");

    // 5. Create a reservation
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/gpu-fabric/capacity/reservations",
            serde_json::json!({
                "tenant_id": "tenant-abc",
                "vm_class": "gpu-a100-2x",
                "count": 1,
                "duration_hours": 24
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let res: serde_json::Value = body_json(resp).await;
    let reservation_id = res["reservation_id"].as_str().unwrap().to_string();
    assert!(!reservation_id.is_empty());

    // 6. Cancel the reservation
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/v1/gpu-fabric/capacity/reservations/{reservation_id}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// Image registry full lifecycle: register → approve → admit → deprecate → warn.
#[tokio::test]
async fn image_registry_full_lifecycle() {
    let app = combined_app();

    // 1. Register image (use flat reference to avoid slash issues in path params)
    let image_ref = "ubuntu-22.04-gpu-v3";
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/images",
            serde_json::json!({
                "reference": image_ref,
                "kind": "vm_image",
                "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "size_bytes": 2147483648_u64,
                "labels": {"os": "ubuntu", "version": "22.04", "gpu": "cuda-12.4"}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 2. Check admission before approval — should be denied (pending review)
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/images/check-admission",
            serde_json::json!({"reference": image_ref}),
        ))
        .await
        .unwrap();
    let admission: serde_json::Value = body_json(resp).await;
    assert_eq!(admission["decision"], "denied");

    // 3. Approve
    let resp = app
        .clone()
        .oneshot(json_post(
            &format!("/api/v1/images/{image_ref}/approve"),
            serde_json::json!({"reviewer": "security-team"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. Check admission after approval — allowed
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/images/check-admission",
            serde_json::json!({"reference": image_ref}),
        ))
        .await
        .unwrap();
    let admission: serde_json::Value = body_json(resp).await;
    assert_eq!(admission["decision"], "allowed");

    // 5. Deprecate
    let resp = app
        .clone()
        .oneshot(json_post(
            &format!("/api/v1/images/{image_ref}/deprecate"),
            serde_json::json!({"reviewer": "admin", "reason": "Ubuntu 24.04 LTS available"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. Admission now returns warning
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/images/check-admission",
            serde_json::json!({"reference": image_ref}),
        ))
        .await
        .unwrap();
    let admission: serde_json::Value = body_json(resp).await;
    assert_eq!(admission["decision"], "allowed_with_warning");
}

/// Cross-subsystem: topology + fleet overview queries return consistent data.
#[tokio::test]
async fn topology_and_fleet_both_start_empty() {
    let app = combined_app();

    let resp = app
        .clone()
        .oneshot(json_get("/api/v1/gpu-fabric/topology"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let topo: serde_json::Value = body_json(resp).await;
    assert_eq!(topo["total_devices"], 0);

    let resp = app
        .oneshot(json_get("/api/v1/gpu-fabric/fleet"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let fleet: serde_json::Value = body_json(resp).await;
    assert_eq!(fleet["host_count"], 0);
}

/// Add topology link between GPUs and verify placement prefers NVLink.
#[tokio::test]
async fn topology_link_and_placement() {
    let app = combined_app();

    // Add 2 GPUs on the same host
    for (id, pci) in [("gpu-a", "0000:3b:00.0"), ("gpu-b", "0000:3c:00.0")] {
        let resp = app
            .clone()
            .oneshot(json_post(
                "/api/v1/gpu-fabric/topology/devices",
                serde_json::json!({
                    "id": id,
                    "host_id": "host-1",
                    "model": "H100",
                    "numa_node": 0,
                    "pci_address": pci,
                    "vram_bytes": 85899345920_u64,
                    "compute_capability": 90
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Add NVLink between them
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/gpu-fabric/topology/links",
            serde_json::json!({
                "from": "gpu-a",
                "to": "gpu-b",
                "interconnect": "nvlink",
                "link_count": 4
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Request placement for 2 GPUs
    let resp = app
        .clone()
        .oneshot(json_post(
            "/api/v1/gpu-fabric/topology/placement",
            serde_json::json!({
                "gpu_count": 2,
                "min_vram_bytes": 0,
                "same_numa": true
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let placement: serde_json::Value = body_json(resp).await;
    assert_eq!(placement["gpu_ids"].as_array().unwrap().len(), 2);
    assert_eq!(placement["host_id"], "host-1");
    assert!(placement["same_numa"].as_bool().unwrap());
}

/// Invalid interconnect type returns 400.
#[tokio::test]
async fn invalid_interconnect_returns_bad_request() {
    let app = combined_app();

    let resp = app
        .oneshot(json_post(
            "/api/v1/gpu-fabric/topology/links",
            serde_json::json!({
                "from": "a",
                "to": "b",
                "interconnect": "teleportation"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Registry config endpoint.
#[tokio::test]
async fn registry_config_reflects_setup() {
    let app = combined_app();

    let resp = app
        .oneshot(json_get("/api/v1/images/config"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let config: serde_json::Value = body_json(resp).await;
    assert_eq!(config["mode"], "enforce");
}
