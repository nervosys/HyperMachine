//! The control plane over real HTTP, against nodes that behave like
//! `hv2-sandboxd` without booting anything: they enforce their capacity,
//! check the cluster token, and record what they run through the same
//! `NodeAgent` the real daemon uses.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde_json::{json, Value};

use hv2_api::sandbox_proxy::SandboxRoutes;
use hv2_cluster::control::{
    self, ClusterRoutes, ControlConfig, ControlPlane, CLUSTER_TOKEN_HEADER,
};
use hv2_cluster::model::{now_ms, SandboxRecord};
use hv2_cluster::node::{NodeAgent, NodeConfig};
use hv2_cluster::store::{ClusterStore, MemoryStore};

const TOKEN: &str = "cluster-secret";
const KEY: &str = "e2b_test_key";

#[tokio::test]
async fn template_metadata_distinguishes_snapshot_cold_legacy_and_heterogeneous_nodes() {
    use hv2_cluster::model::TemplateInfo;
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (a, heart_a) = fake_node(store.clone(), "a", 4, Duration::from_secs(30)).await;
    let (b, heart_b) = fake_node(store.clone(), "b", 4, Duration::from_secs(30)).await;
    heart_a.abort();
    heart_b.abort();
    let _ = heart_a.await;
    let _ = heart_b.await;
    let metadata = |snapshot, cpu_count, memory_mb| {
        [(
            "base".to_string(),
            TemplateInfo {
                snapshot,
                cpu_count,
                memory_mb,
            },
        )]
        .into_iter()
        .collect()
    };
    a.agent.set_template_metadata(metadata(true, 1, 128));
    b.agent.set_template_metadata(metadata(true, 1, 128));
    a.agent.announce(0).await.unwrap();
    b.agent.announce(0).await.unwrap();
    let base = control_plane(store, None).await;
    async fn listed(base: &str) -> Value {
        let entries: Value = client()
            .get(format!("{base}/templates"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        entries[0].clone()
    }
    let all_ready = listed(&base).await;
    assert_eq!(all_ready["snapshot"], true);
    assert_eq!(all_ready["cpuCount"], 1);
    assert_eq!(all_ready["memoryMB"], 128);
    assert_eq!(all_ready["nodeIDs"].as_array().unwrap().len(), 2);
    b.agent.set_template_metadata(metadata(false, 1, 128));
    b.agent.announce(0).await.unwrap();
    assert_eq!(listed(&base).await["snapshot"], false);
    b.agent.set_template_metadata(metadata(true, 2, 256));
    b.agent.announce(0).await.unwrap();
    let heterogeneous = listed(&base).await;
    assert_eq!(heterogeneous["snapshot"], true);
    assert!(heterogeneous["cpuCount"].is_null());
    assert!(heterogeneous["memoryMB"].is_null());
    let node_b = heterogeneous["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["nodeID"] == "b")
        .unwrap();
    assert_eq!(node_b["memoryMB"], 256);
    b.agent.set_templates(vec!["base".into()]);
    b.agent.announce(0).await.unwrap();
    let legacy = listed(&base).await;
    assert!(legacy["snapshot"].is_null());
    let node_b = legacy["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["nodeID"] == "b")
        .unwrap();
    assert!(node_b["snapshot"].is_null());
}

struct FakeNode {
    agent: NodeAgent,
    capacity: usize,
    running: Mutex<HashMap<String, Value>>,
    seen_tokens: Mutex<Vec<Option<String>>>,
}

fn token_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CLUSTER_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

async fn node_create(
    State(node): State<Arc<FakeNode>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    node.seen_tokens.lock().push(token_of(&headers));
    if token_of(&headers).as_deref() != Some(TOKEN) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if node.running.lock().len() >= node.capacity {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"code": 503, "message": "full"})),
        )
            .into_response();
    }
    let id = format!("sbx-{}-{}", node.agent.id(), node.running.lock().len());
    let descriptor = json!({
        "templateID": body["templateID"],
        "sandboxID": id,
        "clientID": id,
        "envdVersion": "0.6.3",
        "envdAccessToken": format!("tok-{id}"),
        "proxyPort": 1,
    });
    let metadata = body["metadata"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let running = {
        let mut running = node.running.lock();
        running.insert(id.clone(), descriptor.clone());
        running.len() as u32
    };
    node.agent
        .created(
            &SandboxRecord {
                sandbox_id: id.clone(),
                node_id: node.agent.id().to_string(),
                template_id: "base".into(),
                started_at_ms: now_ms(),
                end_at_ms: now_ms() + 300_000,
                cpu_count: 1,
                memory_mb: 1024,
                metadata,
                envd_version: "0.6.3".into(),
                descriptor: descriptor.clone(),
                paused: false,
                portable: false,
                volume_mounts: Vec::new(),
            },
            running,
        )
        .await
        .unwrap();
    (StatusCode::CREATED, Json(descriptor)).into_response()
}

async fn node_delete(
    State(node): State<Arc<FakeNode>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if token_of(&headers).as_deref() != Some(TOKEN) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let running = {
        let mut running = node.running.lock();
        if running.remove(&id).is_none() {
            return StatusCode::NOT_FOUND.into_response();
        }
        running.len() as u32
    };
    node.agent
        .ended(&id, None, "sandbox-deleted", running)
        .await
        .unwrap();
    StatusCode::NO_CONTENT.into_response()
}

async fn node_connect(State(node): State<Arc<FakeNode>>, Path(id): Path<String>) -> Response {
    match node.running.lock().get(&id) {
        Some(d) => Json(d.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Start a fake node, joined to `store`, with a short TTL so a test can
/// watch it die.
async fn fake_node(
    store: Arc<dyn ClusterStore>,
    id: &str,
    capacity: usize,
    ttl: Duration,
) -> (Arc<FakeNode>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let agent = NodeAgent::new(
        store,
        NodeConfig {
            id: id.into(),
            api: format!("http://{addr}"),
            proxy: SocketAddr::from(([127, 0, 0, 1], 40000 + addr.port() % 1000)),
            capacity: capacity as u32,
            ttl,
            // Nodes a and b share a key; any other has its own.
            jwk: Some(json!({
                "kty": "EC",
                "kid": if id == "a" || id == "b" { "shared".to_string() } else { format!("key-{id}") },
            })),
            // Node c alone offers the python template.
            templates: if id == "c" {
                vec!["base".into(), "python".into()]
            } else {
                Vec::new()
            },
        },
    );
    agent.join().await.unwrap();
    let node = Arc::new(FakeNode {
        agent: agent.clone(),
        capacity,
        running: Mutex::new(HashMap::new()),
        seen_tokens: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/v2/sandboxes", post(node_create))
        .route("/sandboxes", post(node_create))
        .route("/sandboxes/{id}", delete(node_delete))
        .route("/v2/sandboxes/{id}/connect", post(node_connect))
        .route(
            "/sandboxes/{id}/checkpoints",
            axum::routing::any(echo_checkpoint),
        )
        .route(
            "/sandboxes/{id}/checkpoints/{name}",
            axum::routing::any(echo_checkpoint),
        )
        .route(
            "/sandboxes/{id}/checkpoints/{name}/restore",
            axum::routing::any(echo_checkpoint),
        )
        .with_state(Arc::clone(&node));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let beating = Arc::clone(&node);
    let heart = tokio::spawn(agent.heartbeat(move || beating.running.lock().len() as u32));
    (node, heart)
}

async fn echo_checkpoint(
    headers: HeaderMap,
    method: axum::http::Method,
    uri: axum::http::Uri,
    body: axum::body::Bytes,
) -> Response {
    if token_of(&headers).as_deref() != Some(TOKEN) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({"path": uri.path(), "method": method.as_str(), "body": String::from_utf8_lossy(&body)})).into_response()
}

#[tokio::test]
async fn checkpoints_forward_collection_and_named_operations_to_the_owner() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_node, _heart) = fake_node(store.clone(), "a", 4, Duration::from_secs(30)).await;
    let base = control_plane(store, Some(KEY)).await;
    let (_, created) = create(&base, json!({"templateID": "base"})).await;
    let id = created["sandboxID"].as_str().unwrap();
    for (method, suffix) in [
        ("POST", "/checkpoints"),
        ("GET", "/checkpoints"),
        ("POST", "/checkpoints/before/restore"),
        ("DELETE", "/checkpoints/before"),
    ] {
        let path = format!("/sandboxes/{id}{suffix}");
        let denied = client()
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                format!("{base}{path}"),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 401);
        let response = client()
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                format!("{base}{path}"),
            )
            .header("x-api-key", KEY)
            .json(&json!({"name": "before"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let echoed: Value = response.json().await.unwrap();
        assert_eq!(echoed["path"], path);
        assert_eq!(echoed["method"], method);
        assert_eq!(
            serde_json::from_str::<Value>(echoed["body"].as_str().unwrap()).unwrap(),
            json!({"name": "before"})
        );
    }
}

async fn control_plane(store: Arc<dyn ClusterStore>, api_key: Option<&str>) -> String {
    control_plane_with_keys(store, api_key, Vec::new()).await
}

async fn control_plane_with_keys(
    store: Arc<dyn ClusterStore>,
    api_key: Option<&str>,
    api_keys: Vec<hv2_cluster::keys::ApiKeyPolicy>,
) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let control = ControlPlane::new(
        store,
        ControlConfig {
            api_key: api_key.map(str::to_string),
            api_keys,
            cluster_token: Some(TOKEN.into()),
            proxy_port: 5981,
            create_timeout: Duration::from_secs(10),
            identity_issuer: Some("https://issuer.test".into()),
        },
    );
    tokio::spawn(async move {
        axum::serve(listener, control::router(control))
            .await
            .unwrap();
    });
    format!("http://{addr}")
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn embedded_admin_collision_never_bypasses_scope_or_expiry() {
    use sha2::{Digest, Sha256};
    let key = "collision-fixture";
    let hash: String = Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let now = chrono::Utc::now().timestamp();
    for (expiry, expected) in [(now + 60, 200), (now - 60, 401)] {
        let policies = hv2_cluster::keys::ApiKeyPolicy::from_json(
            &json!([
                {"sha256": hash, "expires_at": expiry, "scopes": ["inventory"]}
            ])
            .to_string(),
        )
        .unwrap();
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
        let base = control_plane_with_keys(store, Some(key), policies).await;
        assert_eq!(
            client()
                .get(format!("{base}/sandboxes"))
                .header("x-api-key", key)
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
        assert_eq!(
            client()
                .post(format!("{base}/sandboxes"))
                .header("x-api-key", key)
                .json(&json!({"templateID": "base"}))
                .send()
                .await
                .unwrap()
                .status(),
            if expected == 200 { 403 } else { 401 }
        );
    }
}

#[tokio::test]
async fn scoped_keys_expire_and_inventory_cannot_leak_guest_credentials() {
    use sha2::{Digest, Sha256};
    let now = chrono::Utc::now().timestamp();
    let hash = |key: &[u8]| -> String {
        Sha256::digest(key)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    let policies = hv2_cluster::keys::ApiKeyPolicy::from_json(
        &json!([
            {"sha256": hash(b"inventory-fixture"), "expires_at": now + 60, "scopes": ["inventory"]},
            {"sha256": hash(b"sandbox-fixture"), "expires_at": now + 60, "scopes": ["sandboxes"]},
            {"sha256": hash(b"expired-fixture"), "expires_at": now - 60, "scopes": ["admin"]}
        ])
        .to_string(),
    )
    .unwrap();
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (node, _) = fake_node(store.clone(), "scope-node", 4, Duration::from_secs(30)).await;
    let base = control_plane_with_keys(store, Some(KEY), policies).await;
    let http = client();
    let created = http
        .post(format!("{base}/sandboxes"))
        .header("x-api-key", "sandbox-fixture")
        .json(&json!({"templateID": "base"}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let descriptor: Value = created.json().await.unwrap();
    let id = descriptor["sandboxID"].as_str().unwrap();
    let listed: Value = http
        .get(format!("{base}/sandboxes"))
        .header("x-api-key", "inventory-fixture")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(listed[0].get("envdAccessToken").is_none());
    for path in [
        format!("/sandboxes/{id}"),
        "/volumes".into(),
        "/events/webhooks".into(),
    ] {
        assert_eq!(
            http.get(format!("{base}{path}"))
                .header("x-api-key", "inventory-fixture")
                .send()
                .await
                .unwrap()
                .status(),
            403,
            "{path}"
        );
    }
    assert_eq!(
        http.post(format!("{base}/sandboxes"))
            .header("x-api-key", "inventory-fixture")
            .json(&json!({"templateID": "base"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(node.running.lock().len(), 1);
    assert_eq!(
        http.get(format!("{base}/volumes"))
            .header("x-api-key", "sandbox-fixture")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    for key in ["expired-fixture", "wrong-fixture", ""] {
        assert_eq!(
            http.get(format!("{base}/sandboxes"))
                .header("x-api-key", key)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        http.get(format!("{base}/sandboxes/{id}"))
            .header("x-api-key", KEY)
            .send()
            .await
            .unwrap()
            .status(),
        200,
        "legacy admin key remains usable"
    );
}

async fn create(base: &str, body: Value) -> (reqwest::StatusCode, Value) {
    let r = client()
        .post(format!("{base}/v2/sandboxes"))
        .header("x-api-key", KEY)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status();
    (status, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn creations_spread_across_nodes_and_answer_with_this_proxy() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (a, _) = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(30)).await;
    let (b, _) = fake_node(Arc::clone(&store), "b", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), None).await;

    for _ in 0..4 {
        let (status, descriptor) = create(&base, json!({"templateID": "base"})).await;
        assert_eq!(status, 201, "{descriptor}");
        assert_eq!(
            descriptor["proxyPort"], 5981,
            "rewritten to the control plane's"
        );
        // Heartbeats are slower than this loop; let the store catch up so
        // the spread is visible rather than raced.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (on_a, on_b) = (a.running.lock().len(), b.running.lock().len());
    assert_eq!(on_a + on_b, 4);
    assert!(
        on_a >= 1 && on_b >= 1,
        "spread, not packed: a={on_a} b={on_b}"
    );
    assert!(
        a.seen_tokens
            .lock()
            .iter()
            .all(|t| t.as_deref() == Some(TOKEN)),
        "every call to a node carried the cluster token"
    );
}

/// The node is the authority on its capacity. A full node refuses, and the
/// control plane moves on -- which is how two control planes racing for one
/// node's last slot resolve without a lock.
#[tokio::test]
async fn a_full_node_refuses_and_the_next_one_takes_it() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (a, _) = fake_node(Arc::clone(&store), "a", 1, Duration::from_secs(30)).await;
    let (b, _) = fake_node(Arc::clone(&store), "b", 1, Duration::from_secs(30)).await;
    let first = control_plane(Arc::clone(&store), None).await;
    let second = control_plane(Arc::clone(&store), None).await;

    // Two control planes, each believing both nodes empty.
    let (s1, _) = create(&first, json!({"templateID": "base"})).await;
    let (s2, _) = create(&second, json!({"templateID": "base"})).await;
    assert_eq!((s1.as_u16(), s2.as_u16()), (201, 201));
    assert_eq!(a.running.lock().len(), 1);
    assert_eq!(b.running.lock().len(), 1);

    let (s3, body) = create(&first, json!({"templateID": "base"})).await;
    assert_eq!(s3, 503, "{body}");
}

#[tokio::test]
async fn listing_detail_and_pagination_come_from_the_store() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let _a = fake_node(Arc::clone(&store), "a", 8, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), None).await;
    for team in ["red", "red", "blue"] {
        create(
            &base,
            json!({"templateID": "base", "metadata": {"team": team}}),
        )
        .await;
    }

    let all: Vec<Value> = client()
        .get(format!("{base}/sandboxes"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all.len(), 3);

    let red: Vec<Value> = client()
        .get(format!("{base}/v2/sandboxes?metadata=team%3Dred"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(red.len(), 2);

    let page = client()
        .get(format!("{base}/v2/sandboxes?limit=2"))
        .send()
        .await
        .unwrap();
    let token = page.headers()["x-next-token"].to_str().unwrap().to_string();
    let first: Vec<Value> = page.json().await.unwrap();
    let rest: Vec<Value> = client()
        .get(format!("{base}/v2/sandboxes?limit=2&nextToken={token}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!((first.len(), rest.len()), (2, 1));
    assert_ne!(first[1]["sandboxID"], rest[0]["sandboxID"]);

    let id = rest[0]["sandboxID"].as_str().unwrap();
    let detail: Value = client()
        .get(format!("{base}/sandboxes/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["envdAccessToken"], format!("tok-{id}"));
    assert_eq!(detail["state"], "running");
}

#[tokio::test]
async fn per_sandbox_calls_reach_the_owning_node() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (a, _) = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), None).await;
    let (_, descriptor) = create(&base, json!({"templateID": "base"})).await;
    let id = descriptor["sandboxID"].as_str().unwrap().to_string();

    let connected: Value = client()
        .post(format!("{base}/v2/sandboxes/{id}/connect"))
        .json(&json!({"timeout": 60}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(connected["sandboxID"], id.as_str());
    assert_eq!(connected["proxyPort"], 5981);

    let deleted = client()
        .delete(format!("{base}/sandboxes/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 204);
    assert!(a.running.lock().is_empty());
    assert!(
        store.sandbox(&id).await.unwrap().is_none(),
        "the node removed its record"
    );
}

/// A node that stops heartbeating takes its sandboxes with it: the reaper
/// removes their records, and a call to one says it is gone rather than
/// hanging on a dead address.
#[tokio::test]
async fn a_dead_nodes_sandboxes_are_reaped() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_a, heart) = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(1)).await;
    let _b = fake_node(Arc::clone(&store), "b", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), None).await;
    let mut on_a = None;
    for _ in 0..4 {
        let (_, d) = create(&base, json!({"templateID": "base"})).await;
        if d["nodeID"] == "a" {
            on_a = d["sandboxID"].as_str().map(str::to_string);
        }
    }
    let on_a = on_a.expect("something landed on a");

    heart.abort(); // a stops heartbeating
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let reaped = control::reap(store.as_ref()).await.unwrap();
    assert!(reaped >= 1);
    assert!(store.sandbox(&on_a).await.unwrap().is_none());
    let events = store.events(50).await.unwrap();
    assert!(events
        .iter()
        .any(|e| e.kind == "sandbox-lost" && e.sandbox_id.as_deref() == Some(&on_a)));

    let gone = client()
        .post(format!("{base}/v2/sandboxes/{on_a}/connect"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 404);

    // New work goes only to the node that is left.
    let (status, d) = create(&base, json!({"templateID": "base"})).await;
    assert_eq!((status.as_u16(), d["nodeID"].as_str()), (201, Some("b")));
}

#[tokio::test]
async fn the_api_key_is_required_when_set() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let _a = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), Some(KEY)).await;

    let without = client()
        .post(format!("{base}/v2/sandboxes"))
        .json(&json!({"templateID": "base"}))
        .send()
        .await
        .unwrap();
    assert_eq!(without.status(), 401);
    let wrong = client()
        .get(format!("{base}/sandboxes"))
        .header("x-api-key", "e2b_wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    let (status, _) = create(&base, json!({"templateID": "base"})).await;
    assert_eq!(status, 201);
    let health = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(health.status(), 200, "health needs no key");
}

#[tokio::test]
async fn envd_routes_go_to_the_owning_nodes_proxy() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let _a = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), None).await;
    let (_, d) = create(&base, json!({"templateID": "base"})).await;
    let id = d["sandboxID"].as_str().unwrap();
    let node = store.node("a").await.unwrap().unwrap();

    let routes = ClusterRoutes::new(Arc::clone(&store), Duration::from_secs(2));
    assert_eq!(routes.resolve(id, 49983).await, Some(node.proxy));
    assert_eq!(routes.resolve("sbx-nope", 49983).await, None);
}

/// The cluster's JWKS is every live node's workload-token key, each once,
/// and discovery points at it -- what a cloud verifying a sandbox's token
/// fetches, without an API key.
#[tokio::test]
async fn the_jwks_lists_each_nodes_key_once_and_discovery_points_at_it() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let _a = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(30)).await;
    let _b = fake_node(Arc::clone(&store), "b", 4, Duration::from_secs(30)).await;
    let _c = fake_node(Arc::clone(&store), "c", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), Some("the-key")).await;

    let jwks: Value = client()
        .get(format!("{base}/.well-known/jwks.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut kids: Vec<String> = jwks["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["kid"].as_str().unwrap().to_string())
        .collect();
    kids.sort();
    assert_eq!(kids, ["key-c", "shared"]);

    let discovery: Value = client()
        .get(format!("{base}/.well-known/openid-configuration"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(discovery["issuer"], "https://issuer.test");
    assert_eq!(
        discovery["jwks_uri"],
        "https://issuer.test/.well-known/jwks.json"
    );
}

/// A create for a template goes only to a node that offers it; one no node
/// offers is refused as not found, and the templates are listed.
#[tokio::test]
async fn creates_go_to_a_node_with_the_template() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (a, _) = fake_node(Arc::clone(&store), "a", 4, Duration::from_secs(30)).await;
    let (c, _) = fake_node(Arc::clone(&store), "c", 4, Duration::from_secs(30)).await;
    let base = control_plane(Arc::clone(&store), None).await;

    for _ in 0..3 {
        let (status, body) = create(&base, json!({"templateID": "python"})).await;
        assert_eq!(status, 201, "{body}");
    }
    assert_eq!(
        c.running.lock().len(),
        3,
        "all on the node with the template"
    );
    assert_eq!(a.running.lock().len(), 0);

    let (status, body) = create(&base, json!({"templateID": "rust"})).await;
    assert_eq!(status, 404, "{body}");

    let listed: Value = client()
        .get(format!("{base}/templates"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["templateID"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["base", "python"]);
}
