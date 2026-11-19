//! API keys carry a role and, optionally, a tenant; the routes that scope by
//! tenant take it from the key rather than from a header the caller wrote.
//!
//! Each behaviour is checked in both directions: the request that must be
//! allowed, and the one that must be refused. A middleware that denied
//! everything or allowed everything would fail one half of each pair.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use hv2_api::middleware::{KeyRole, MiddlewareConfig};
use hv2_api::server::{Server, ServerConfig};
use tower::ServiceExt;

/// Not literals: a fixed secret-looking string in a test is what a scanner
/// exists to find.
fn key(name: &str) -> String {
    format!("{name}-{}", std::process::id())
}

fn app() -> Router {
    // No session record: these tests must not append to a developer's real log.
    std::env::set_var("HV2_CONTEXT_ROOT", "");
    let middleware = MiddlewareConfig::none()
        .api_keys(vec![key("root")])
        .scoped_api_key(key("viewer"), KeyRole::Viewer, None)
        .scoped_api_key(key("acme"), KeyRole::Operator, Some("acme"))
        .scoped_api_key(key("globex"), KeyRole::Operator, Some("globex"));
    let config = ServerConfig {
        middleware,
        enable_events: false,
        pre_warm_count: 0,
        ..ServerConfig::default()
    };
    Server::new(config).build_router()
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    tenant: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(k) = bearer {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    if let Some(t) = tenant {
        b = b.header("x-tenant-id", t);
    }
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&v).unwrap()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn a_missing_or_unknown_key_is_refused() {
    let app = app();
    let (s, _) = send(&app, "GET", "/api/v1/agents", None, None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = send(&app, "GET", "/api/v1/agents", Some("nope"), None, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &app,
        "GET",
        "/api/v1/agents",
        Some(&key("root")),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_viewer_reads_but_cannot_write() {
    let app = app();
    let viewer = key("viewer");
    let (s, _) = send(&app, "GET", "/api/v1/agents", Some(&viewer), None, None).await;
    assert_eq!(s, StatusCode::OK, "reads are allowed");
    let body = serde_json::json!({"agent_id": "a"});
    let (s, v) = send(
        &app,
        "POST",
        "/api/v1/agents",
        Some(&viewer),
        None,
        Some(body.clone()),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(v["code"], "ROLE_DENIED");
    // The same write with an operator key goes through.
    let (s, _) = send(
        &app,
        "POST",
        "/api/v1/agents",
        Some(&key("acme")),
        None,
        Some(body),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn an_operator_cannot_touch_the_admin_only_routes_but_an_admin_can() {
    let app = app();
    let review = serde_json::json!({"reviewer": "someone"});
    for uri in [
        "/api/v1/images/x:1/approve",
        "/api/v1/images/x:1/revoke",
        "/api/v1/runtime/maintenance",
    ] {
        let (s, v) = send(
            &app,
            "POST",
            uri,
            Some(&key("acme")),
            None,
            Some(review.clone()),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(v["code"], "ROLE_DENIED", "{uri}");
    }
    // An admin gets past the role gate: the handler answers (the image does
    // not exist), where the operator was stopped before reaching it.
    let (s, v) = send(
        &app,
        "POST",
        "/api/v1/images/x:1/approve",
        Some(&key("root")),
        None,
        Some(review),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(v["code"], "APPROVE_FAILED");
}

#[tokio::test]
async fn the_tenant_comes_from_the_key_not_the_header() {
    let app = app();
    let acme = key("acme");
    let globex = key("globex");
    let spawn = serde_json::json!({"agent_id": "a"});

    // acme spawns with no header at all, and the agent is acme's.
    let (s, _) = send(
        &app,
        "POST",
        "/api/v1/agents",
        Some(&acme),
        None,
        Some(spawn),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, v) = send(&app, "GET", "/api/v1/agents", Some(&acme), None, None).await;
    assert_eq!(v["total"], 1);

    // globex cannot see it, and cannot claim to be acme to see it.
    let (_, v) = send(&app, "GET", "/api/v1/agents", Some(&globex), None, None).await;
    assert_eq!(v["total"], 0);
    let (s, v) = send(
        &app,
        "GET",
        "/api/v1/agents",
        Some(&globex),
        Some("acme"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(v["code"], "TENANT_MISMATCH");

    // Naming its own tenant is fine.
    let (s, _) = send(
        &app,
        "GET",
        "/api/v1/agents",
        Some(&globex),
        Some("globex"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // globex's reap does not end acme's agent; acme's does.
    let reap = serde_json::json!({"max_idle_secs": 0});
    let (_, v) = send(
        &app,
        "POST",
        "/api/v1/agents/reap",
        Some(&globex),
        None,
        Some(reap.clone()),
    )
    .await;
    assert_eq!(v["reaped"], 0);
    let (_, v) = send(
        &app,
        "POST",
        "/api/v1/agents/reap",
        Some(&acme),
        None,
        Some(reap),
    )
    .await;
    assert_eq!(v["reaped"], 1);
}

#[tokio::test]
async fn a_reservation_can_only_be_cancelled_by_its_tenant_or_an_admin() {
    let app = app();
    let root = key("root");
    let class = serde_json::json!({
        "name": "c1", "sla_tier": "premium", "vcpus": 4, "memory_bytes": 1u64 << 30,
        "gpu_count": 1, "gpu_model": "A100", "max_instances": 10
    });
    let (s, _) = send(
        &app,
        "POST",
        "/api/v1/gpu-fabric/capacity/classes",
        Some(&root),
        None,
        Some(class),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    // An operator cannot register classes, which is the admin's.
    let (s, _) = send(
        &app,
        "POST",
        "/api/v1/gpu-fabric/capacity/classes",
        Some(&key("acme")),
        None,
        Some(serde_json::json!({"name": "c2"})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let reserve = |tenant: &str| serde_json::json!({"tenant_id": tenant, "vm_class": "c1", "count": 1, "duration_hours": 1});
    // acme cannot reserve in globex's name.
    let (s, v) = send(
        &app,
        "POST",
        "/api/v1/gpu-fabric/capacity/reservations",
        Some(&key("acme")),
        None,
        Some(reserve("globex")),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(v["code"], "TENANT_MISMATCH");

    let mut ids = Vec::new();
    for _ in 0..2 {
        let (s, v) = send(
            &app,
            "POST",
            "/api/v1/gpu-fabric/capacity/reservations",
            Some(&key("acme")),
            None,
            Some(reserve("acme")),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED);
        ids.push(v["reservation_id"].as_str().unwrap().to_string());
    }
    let cancel = |id: &str| format!("/api/v1/gpu-fabric/capacity/reservations/{id}");

    // globex: not theirs.
    let (s, _) = send(
        &app,
        "DELETE",
        &cancel(&ids[0]),
        Some(&key("globex")),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // The owner, and an unscoped admin, can.
    let (s, _) = send(
        &app,
        "DELETE",
        &cancel(&ids[0]),
        Some(&key("acme")),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = send(&app, "DELETE", &cancel(&ids[1]), Some(&root), None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
}
