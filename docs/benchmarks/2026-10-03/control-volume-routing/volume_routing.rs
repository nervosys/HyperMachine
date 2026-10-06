//! Existing volume routes exercised over real HTTP with owned protocol nodes.
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use axum::{body::{to_bytes, Body}, extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
use hv2_cluster::{control::{self, ControlConfig, ControlPlane, CLUSTER_TOKEN_HEADER},
    model::{now_ms, volume_id, NodeInfo}, store::{ClusterStore, MemoryStore}};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn volume_routes_preserve_placement_query_binary_and_credentials() {
    let store = Arc::new(MemoryStore::new());
    let payload = Arc::new(Mutex::new(Vec::<u8>::new()));
    let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
    let id = volume_id("owned");
    let mut tasks = Vec::new();
    for node_id in ["volume-a", "volume-b"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        store.put_node(&NodeInfo { id: node_id.into(), api: format!("http://{addr}"), proxy: addr,
            capacity: 4, running: 0, heartbeat_ms: now_ms(), version: "fixture".into(),
            jwk: None, templates: vec!["base".into()], template_metadata: BTreeMap::new() },
            Duration::from_secs(60)).await.unwrap();
        let data = payload.clone();
        let observed = seen.clone();
        let volume = id.clone();
        let app = Router::new().fallback(move |request: Request| {
            let data = data.clone();
            let observed = observed.clone();
            let volume = volume.clone();
            async move {
                assert_eq!(request.headers()[CLUSTER_TOKEN_HEADER], "owned-cluster-token");
                assert!(!request.headers().contains_key("x-api-key"));
                let path = request.uri().path().to_string();
                let method = request.method().to_string();
                if path.starts_with("/volumecontent/") && request.headers().get("authorization")
                    .and_then(|v| v.to_str().ok()) != Some("Bearer owned-volume-token") {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                observed.lock().push((node_id.into(), method.clone(), request.uri().to_string()));
                if path.starts_with("/volumecontent/") {
                    if method == "PUT" {
                        let bytes = to_bytes(request.into_body(), 2 << 20).await.unwrap();
                        let size = bytes.len();
                        *data.lock() = bytes.to_vec();
                        return (StatusCode::CREATED, Json(json!({"size":size}))).into_response();
                    }
                    return Body::from(data.lock().clone()).into_response();
                }
                let metadata = json!({"volumeID":volume,"name":"owned","token":"owned-volume-token"});
                if path == "/volumes" && method == "GET" {
                    Json(json!([metadata])).into_response()
                } else if method == "POST" {
                    (StatusCode::CREATED, Json(metadata)).into_response()
                } else {
                    Json(metadata).into_response()
                }
            }
        });
        tasks.push(tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let plane = ControlPlane::new(store, ControlConfig { api_key: Some("owned-api-key".into()),
        api_keys: vec![], access_audit: None, cluster_token: Some("owned-cluster-token".into()),
        proxy_port: 5981, create_timeout: Duration::from_secs(5), identity_issuer: None });
    tasks.push(tokio::spawn(async move { axum::serve(listener, control::router(plane)).await.unwrap() }));
    let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
    assert_eq!(http.post(format!("{base}/volumes")).json(&json!({"name":"owned"})).send().await.unwrap().status(), 401);
    assert!(seen.lock().is_empty());
    assert_eq!(http.post(format!("{base}/volumes")).header("x-api-key", "owned-api-key")
        .json(&json!({"name":"owned"})).send().await.unwrap().status(), 201);
    assert_eq!(http.get(format!("{base}/volumes/{id}")).header("x-api-key", "owned-api-key")
        .send().await.unwrap().status(), 200);
    let bytes: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    let route = format!("/volumecontent/{id}/file?path=%2Fnested%2Fa%20b.bin&atomic=true&force=true");
    let uploaded = http.put(format!("{base}{route}")).bearer_auth("owned-volume-token")
        .header(CLUSTER_TOKEN_HEADER, "client-must-not-select-node-credential")
        .header("content-type", "application/octet-stream").body(bytes.clone()).send().await.unwrap();
    assert_eq!(uploaded.status(), 201);
    let stat: serde_json::Value = uploaded.json().await.unwrap();
    assert_eq!(stat["size"], bytes.len());
    assert_eq!(http.get(format!("{base}{route}")).bearer_auth("owned-volume-token")
        .send().await.unwrap().bytes().await.unwrap().as_ref(), bytes.as_slice());
    assert_eq!(http.put(format!("{base}{route}")).bearer_auth("wrong")
        .body("refused").send().await.unwrap().status(), 401);
    assert_eq!(*payload.lock(), bytes);
    {
        let rows = seen.lock();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.0 == rows[0].0));
        assert_eq!(rows[2].2, route);
        assert_eq!(rows[3].2, route);
    }
    let listed: serde_json::Value = http.get(format!("{base}/volumes")).header("x-api-key", "owned-api-key")
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    for task in tasks { task.abort(); assert!(task.await.unwrap_err().is_cancelled()); }
}
