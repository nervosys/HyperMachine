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
async fn tcp_tunnels_route_authenticated_binary_streams_and_both_half_closes() {
    use axum::extract::Request;
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_, heartbeat) = fake_node(store.clone(), "tcp-node", 4, Duration::from_secs(30)).await;
    let hash: String = Sha256::digest(b"tcp-inventory-key")
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
    let policies = hv2_cluster::keys::ApiKeyPolicy::from_json(
        &json!([
            {"sha256":hash,"expires_at":chrono::Utc::now().timestamp()+600,"scopes":["inventory"]}
        ])
        .to_string(),
    )
    .unwrap();
    let base = control_plane_with_keys(store.clone(), Some(KEY), policies).await;
    let (_, created) = create(&base, json!({"templateID":"base"})).await;
    let id = created["sandboxID"].as_str().unwrap().to_owned();
    heartbeat.abort();
    let _ = heartbeat.await;
    let received = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new().route(
        "/sandboxes/{id}/ports/{port}/tcp",
        axum::routing::get({
            let received = received.clone();
            let calls = calls.clone();
            let expected_id = id.clone();
            move |Path((id, port)): Path<(String, u16)>, request: Request| {
                let received = received.clone();
                let calls = calls.clone();
                let expected_id = expected_id.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert_eq!(id, expected_id);
                    assert_eq!(request.headers()[CLUSTER_TOKEN_HEADER], TOKEN);
                    assert!(!request.headers().contains_key("x-api-key"));
                    hv2_api::tcp_tunnel::validate(&request).unwrap();
                    if port == 8082 {
                        return StatusCode::BAD_GATEWAY.into_response();
                    }
                    let (relay, mut service) = tokio::io::duplex(4096);
                    tokio::spawn(async move {
                        if port == 8081 {
                            service.write_all(b"ready\0\xff").await.unwrap();
                            service.shutdown().await.unwrap();
                        }
                        let mut bytes = Vec::new();
                        service.read_to_end(&mut bytes).await.unwrap();
                        if port == 8080 {
                            service.write_all(&bytes).await.unwrap();
                            service.shutdown().await.unwrap();
                        } else {
                            *received.lock() = bytes;
                        }
                    });
                    hv2_api::tcp_tunnel::accept(request, relay)
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let node_url = format!("http://{}", listener.local_addr().unwrap());
    let node_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut node = store.node("tcp-node").await.unwrap().unwrap();
    node.api = node_url;
    store
        .put_node(&node, Duration::from_secs(60))
        .await
        .unwrap();
    let http = reqwest::Client::builder().http1_only().build().unwrap();
    let upgrade = |path: String, key: &str| {
        http.get(path)
            .header("x-api-key", key)
            .header("connection", "upgrade")
            .header("upgrade", hv2_api::tcp_tunnel::PROTOCOL)
    };
    let url = format!("{base}/sandboxes/{id}/ports/8080/tcp");
    for (key, status) in [("wrong", 401), ("tcp-inventory-key", 403)] {
        assert_eq!(
            upgrade(url.clone(), key).send().await.unwrap().status(),
            status
        );
    }
    assert_eq!(
        http.get(&url)
            .header("x-api-key", KEY)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        upgrade(format!("{base}/sandboxes/{id}/ports/0/tcp"), KEY)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let bytes: Vec<u8> = (0..131072).map(|n| (n % 251) as u8).collect();
    for port in [8080, 8081] {
        let response = upgrade(format!("{base}/sandboxes/{id}/ports/{port}/tcp"), KEY)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 101);
        let mut stream = response.upgrade().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut reply = Vec::new();
            if port == 8081 {
                stream.read_to_end(&mut reply).await.unwrap();
                assert_eq!(reply, b"ready\0\xff");
            }
            stream.write_all(&bytes).await.unwrap();
            stream.shutdown().await.unwrap();
            if port == 8080 {
                stream.read_to_end(&mut reply).await.unwrap();
                assert_eq!(reply, bytes);
            } else {
                while received.lock().len() != bytes.len() {
                    tokio::task::yield_now().await;
                }
                assert_eq!(*received.lock(), bytes);
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(
        upgrade(format!("{base}/sandboxes/{id}/ports/8082/tcp"), KEY)
            .send()
            .await
            .unwrap()
            .status(),
        502
    );
    let mut record = store.sandbox(&id).await.unwrap().unwrap();
    record.paused = true;
    store.put_sandbox(&record).await.unwrap();
    assert_eq!(
        upgrade(url.clone(), KEY).send().await.unwrap().status(),
        409
    );
    record.paused = false;
    store.put_sandbox(&record).await.unwrap();
    store.remove_node("tcp-node").await.unwrap();
    assert_eq!(
        upgrade(url.clone(), KEY).send().await.unwrap().status(),
        503
    );
    store.delete_sandbox(&id).await.unwrap();
    assert_eq!(upgrade(url, KEY).send().await.unwrap().status(), 404);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    node_task.abort();
    let _ = node_task.await;
}

#[tokio::test]
async fn custom_domain_https_reaches_the_node_with_guest_routing_and_host_intact() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_, heartbeat) = fake_node(store.clone(), "domain-tls", 4, Duration::from_secs(30)).await;
    let base = control_plane(store.clone(), Some(KEY)).await;
    let (_, created) = create(&base, json!({"templateID":"base"})).await;
    let id = created["sandboxID"].as_str().unwrap().to_owned();
    heartbeat.abort();
    let _ = heartbeat.await;
    let backend = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut node = store.node("domain-tls").await.unwrap().unwrap();
    node.proxy = backend.local_addr().unwrap();
    store
        .put_node(&node, Duration::from_secs(60))
        .await
        .unwrap();
    let backend_task = tokio::spawn(async move {
        axum::serve(
            backend,
            Router::new().fallback(
                |headers: HeaderMap, uri: axum::http::Uri, body: String| async move {
                    Json(json!({"host":headers["host"].to_str().unwrap(),
                "sandbox":headers["e2b-sandbox-id"].to_str().unwrap(),
                "port":headers["e2b-sandbox-port"].to_str().unwrap(),
                "path":uri.path_and_query().unwrap().as_str(), "body":body}))
                },
            ),
        )
        .await
        .unwrap();
    });
    let binding_url = format!("{base}/sandboxes/{id}/domains/app.example.com");
    assert_eq!(
        client()
            .put(&binding_url)
            .header("x-api-key", KEY)
            .json(&json!({"port":8080}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let cert = rcgen::generate_simple_self_signed(vec!["app.example.com".into()]).unwrap();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
        )
        .unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let (shutdown, rx) = tokio::sync::oneshot::channel();
    let routes = Arc::new(ClusterRoutes::new(store.clone(), Duration::from_secs(30)));
    let proxy_task = tokio::spawn(hv2_api::sandbox_proxy::serve_tls(addr, routes, tls, rx));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let builder = || {
        reqwest::Client::builder()
            .no_proxy()
            .resolve("app.example.com", addr)
            .add_root_certificate(reqwest::Certificate::from_der(cert.cert.der()).unwrap())
            .timeout(Duration::from_secs(5))
    };
    let clients = [
        (
            builder().http1_only().build().unwrap(),
            reqwest::Version::HTTP_11,
        ),
        (
            builder().http2_prior_knowledge().build().unwrap(),
            reqwest::Version::HTTP_2,
        ),
    ];
    let url = format!("https://app.example.com:{}/echo?literal=1", addr.port());
    for port in [8080, 3000] {
        assert_eq!(
            client()
                .put(&binding_url)
                .header("x-api-key", KEY)
                .json(&json!({"port":port}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        for (http, version) in &clients {
            let response = http
                .post(&url)
                .body("body survives alias routing")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.version(), *version);
            let echoed: Value = response.json().await.unwrap();
            assert_eq!(echoed["sandbox"], id);
            assert_eq!(echoed["port"], port.to_string());
            assert_eq!(echoed["host"], format!("app.example.com:{}", addr.port()));
            assert_eq!(echoed["path"], "/echo?literal=1");
            assert_eq!(echoed["body"], "body survives alias routing");
        }
    }
    store.delete_sandbox(&id).await.unwrap();
    for (http, _) in &clients {
        assert_eq!(http.get(&url).send().await.unwrap().status(), 400);
    }
    shutdown.send(()).unwrap();
    proxy_task.await.unwrap().unwrap();
    backend_task.abort();
    let _ = backend_task.await;
}

#[tokio::test]
async fn custom_domains_are_authenticated_owned_and_follow_port_updates() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_node, _) = fake_node(store.clone(), "domain-node", 4, Duration::from_secs(30)).await;
    let base = control_plane(store.clone(), Some(KEY)).await;
    let (_, a) = create(&base, json!({"templateID": "base"})).await;
    let (_, b) = create(&base, json!({"templateID": "base"})).await;
    let a = a["sandboxID"].as_str().unwrap();
    let b = b["sandboxID"].as_str().unwrap();
    let path = format!("{base}/sandboxes/{a}/domains/App.Example.com.");
    for method in [
        reqwest::Method::GET,
        reqwest::Method::PUT,
        reqwest::Method::DELETE,
    ] {
        let url = if method == reqwest::Method::GET {
            format!("{base}/sandboxes/{a}/domains")
        } else {
            path.clone()
        };
        assert_eq!(
            client()
                .request(method, url)
                .json(&json!({"port":8080}))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    for port in [8080, 3000] {
        let response = client()
            .put(&path)
            .header("x-api-key", KEY)
            .json(&json!({"port":port}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let binding: Value = response.json().await.unwrap();
        assert_eq!(binding["domain"], "app.example.com");
        assert_eq!(binding["port"], port);
    }
    let other = format!("{base}/sandboxes/{b}/domains/app.example.com");
    assert_eq!(
        client()
            .put(&other)
            .header("x-api-key", KEY)
            .json(&json!({"port":9000}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client()
            .delete(&other)
            .header("x-api-key", KEY)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let listed: Value = client()
        .get(format!("{base}/sandboxes/{a}/domains"))
        .header("x-api-key", KEY)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let routes = ClusterRoutes::new(store.clone(), Duration::from_secs(30));
    assert_eq!(
        routes.resolve_hostname("APP.EXAMPLE.COM.:443").await,
        Some((3000, a.into()))
    );
    for bad in [
        "127.0.0.1",
        "9000-sandbox.example.com",
        "user@app.example.com",
        "unknown.example.com",
    ] {
        assert!(routes.resolve_hostname(bad).await.is_none());
    }
    assert_eq!(
        client()
            .put(&path)
            .header("x-api-key", KEY)
            .json(&json!({"port":0}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client()
            .put(format!(
                "{base}/sandboxes/missing/domains/missing.example.com"
            ))
            .header("x-api-key", KEY)
            .json(&json!({"port":80}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client()
            .delete(&path)
            .header("x-api-key", KEY)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert!(routes.resolve_hostname("app.example.com").await.is_none());
    assert_eq!(
        client()
            .put(&path)
            .header("x-api-key", KEY)
            .json(&json!({"port":8080}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    store.delete_sandbox(a).await.unwrap();
    assert!(routes.resolve_hostname("app.example.com").await.is_none());
}

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
    control_plane_with_timeout(store, api_key, api_keys, Duration::from_secs(10)).await
}

async fn control_plane_with_timeout(
    store: Arc<dyn ClusterStore>,
    api_key: Option<&str>,
    api_keys: Vec<hv2_cluster::keys::ApiKeyPolicy>,
    create_timeout: Duration,
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
            create_timeout,
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
        format!("/sandboxes/{id}/domains"),
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
    let domain_url = format!("{base}/sandboxes/{id}/domains/scoped.example.com");
    for (key, status) in [
        ("inventory-fixture", 403),
        ("expired-fixture", 401),
        ("sandbox-fixture", 200),
    ] {
        assert_eq!(
            http.put(&domain_url)
                .header("x-api-key", key)
                .json(&json!({"port":8080}))
                .send()
                .await
                .unwrap()
                .status(),
            status
        );
    }
    assert_eq!(
        http.delete(&domain_url)
            .header("x-api-key", "sandbox-fixture")
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
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

#[tokio::test]
async fn policy_replacement_revokes_old_keys_and_rejects_invalid_updates() {
    use sha2::{Digest, Sha256};
    let policies = |key: &str| {
        json!([{"sha256":Sha256::digest(key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "expires_at":chrono::Utc::now().timestamp()+600,"scopes":["inventory"]}]).to_string()
    };
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let control = ControlPlane::new(
        store,
        ControlConfig {
            api_key: Some(KEY.into()),
            api_keys: hv2_cluster::keys::ApiKeyPolicy::from_json(&policies("old-key")).unwrap(),
            cluster_token: Some(TOKEN.into()),
            proxy_port: 5981,
            create_timeout: Duration::from_secs(10),
            identity_issuer: None,
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = control::router(control.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let status = |key: &str| {
        client()
            .get(format!("{base}/sandboxes"))
            .header("x-api-key", key)
            .send()
    };
    assert_eq!(status("old-key").await.unwrap().status(), 200);
    control
        .replace_api_key_policies(&policies("new-key"))
        .unwrap();
    assert_eq!(status("old-key").await.unwrap().status(), 401);
    assert_eq!(status("new-key").await.unwrap().status(), 200);
    for invalid in [
        "[]".to_owned(),
        "malformed-secret".to_owned(),
        policies(KEY),
    ] {
        assert!(control.replace_api_key_policies(&invalid).is_err());
        assert_eq!(status("new-key").await.unwrap().status(), 200);
        assert_eq!(status("old-key").await.unwrap().status(), 401);
    }
    assert_eq!(status(KEY).await.unwrap().status(), 200);
    assert_eq!(
        client()
            .post(format!("{base}/sandboxes"))
            .header("x-api-key", "new-key")
            .json(&json!({"templateID":"base"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn reserved_name_lookup_authenticates_and_never_returns_operation_tokens() {
    use hv2_cluster::names::{NameReservation, SandboxName};
    use sha2::{Digest, Sha256};
    let policy = |key: &str, scope: &str| {
        json!({"sha256":Sha256::digest(key.as_bytes()).iter()
        .map(|byte| format!("{byte:02x}")).collect::<String>(),
        "expires_at":chrono::Utc::now().timestamp()+600,"scopes":[scope]})
    };
    let policies = hv2_cluster::keys::ApiKeyPolicy::from_json(
        &json!([
            policy("inventory-fixture", "inventory"),
            policy("sandbox-fixture", "sandboxes")
        ])
        .to_string(),
    )
    .unwrap();
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_, heartbeat) = fake_node(store.clone(), "name-node", 4, Duration::from_secs(30)).await;
    let base = control_plane_with_keys(store.clone(), Some(KEY), policies).await;
    let request = |name: &str, key: &str| {
        client()
            .get(format!("{base}/sandbox-names/{name}"))
            .header("x-api-key", key)
            .send()
    };
    assert_eq!(request("guest", "").await.unwrap().status(), 401);
    assert_eq!(
        request("guest", "inventory-fixture")
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(request("bad%20name", KEY).await.unwrap().status(), 400);
    assert_eq!(
        request("guest", "sandbox-fixture").await.unwrap().status(),
        404
    );
    let name = SandboxName::parse("guest").unwrap();
    let reservation = NameReservation::pending(name.clone());
    assert!(store.reserve_name(&reservation).await.unwrap());
    let pending = request("guest", "sandbox-fixture").await.unwrap();
    assert_eq!(pending.status(), 409);
    assert!(!pending
        .text()
        .await
        .unwrap()
        .contains(reservation.operation_token()));
    let (_, created) = create(&base, json!({"templateID":"base"})).await;
    let id = created["sandboxID"].as_str().unwrap();
    for (key, expected) in [("inventory-fixture", 403), ("sandbox-fixture", 409)] {
        assert_eq!(
            client()
                .put(format!("{base}/sandboxes/{id}/names/guest"))
                .header("x-api-key", key)
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert!(store
        .bind_name(&name, reservation.operation_token(), id)
        .await
        .unwrap());
    let response = request("guest", "sandbox-fixture").await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"name":"guest","sandboxID":id})
    );
    let (_, duplicate) = create(&base, json!({"templateID":"base"})).await;
    let duplicate_id = duplicate["sandboxID"].as_str().unwrap();
    let mut legacy = store.sandbox(duplicate_id).await.unwrap().unwrap();
    legacy.metadata.insert("hm.name".into(), "guest".into());
    store.put_sandbox(&legacy).await.unwrap();
    assert_eq!(
        request("guest", "sandbox-fixture").await.unwrap().status(),
        409
    );
    assert!(store.delete_sandbox(duplicate_id).await.unwrap());
    assert_eq!(
        request("guest", "sandbox-fixture").await.unwrap().status(),
        200
    );
    assert!(store.delete_sandbox(id).await.unwrap());
    assert_eq!(
        request("guest", "sandbox-fixture").await.unwrap().status(),
        404
    );
    heartbeat.abort();
    let _ = heartbeat.await;
}

#[tokio::test]
async fn name_assignment_is_authenticated_exclusive_and_reusable_after_deletion() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (_, heartbeat) = fake_node(store.clone(), "assign-node", 4, Duration::from_secs(30)).await;
    let base = control_plane(store.clone(), Some(KEY)).await;
    let (_, a) = create(&base, json!({"templateID":"base"})).await;
    let (_, b) = create(&base, json!({"templateID":"base"})).await;
    let a = a["sandboxID"].as_str().unwrap();
    let b = b["sandboxID"].as_str().unwrap();
    let assign = |id: &str, name: &str, key: &str| {
        client()
            .put(format!("{base}/sandboxes/{id}/names/{name}"))
            .header("x-api-key", key)
            .send()
    };
    assert_eq!(assign(a, "alias", "").await.unwrap().status(), 401);
    assert_eq!(assign(a, "bad%20name", KEY).await.unwrap().status(), 400);
    assert_eq!(assign("absent", "alias", KEY).await.unwrap().status(), 404);
    let (ra, rb) = tokio::join!(assign(a, "alias", KEY), assign(b, "alias", KEY));
    let ra = ra.unwrap();
    let rb = rb.unwrap();
    assert!(matches!(
        (ra.status().as_u16(), rb.status().as_u16()),
        (200, 409) | (409, 200)
    ));
    let (winner, loser) = if ra.status().is_success() {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(
        assign(winner, "alias", KEY)
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap(),
        json!({"name":"alias","sandboxID":winner})
    );
    assert_eq!(assign(loser, "alias", KEY).await.unwrap().status(), 409);
    // A successful legacy node response does not clean the shared name store.
    // The control plane must perform cleanup itself before acknowledging it.
    heartbeat.abort();
    let _ = heartbeat.await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut node = store.node("assign-node").await.unwrap().unwrap();
    node.api = format!("http://{}", listener.local_addr().unwrap());
    store
        .put_node(&node, Duration::from_secs(30))
        .await
        .unwrap();
    let app = Router::new().route(
        "/sandboxes/{id}",
        delete(|| async { StatusCode::NO_CONTENT }),
    );
    let legacy_node = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    assert_eq!(
        client()
            .delete(format!("{base}/sandboxes/{winner}"))
            .header("x-api-key", KEY)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert!(store.sandbox(winner).await.unwrap().is_none());
    assert_eq!(assign(loser, "alias", KEY).await.unwrap().status(), 200);
    assert!(!store.delete_sandbox(winner).await.unwrap());
    let resolved = client()
        .get(format!("{base}/sandbox-names/alias"))
        .header("x-api-key", KEY)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(resolved, json!({"name":"alias","sandboxID":loser}));
    legacy_node.abort();
    let _ = legacy_node.await;
}

#[tokio::test]
async fn named_creation_reserves_before_forwarding_across_control_planes() {
    use hv2_cluster::names::SandboxName;
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (node, heartbeat) = fake_node(store.clone(), "named", 8, Duration::from_secs(30)).await;
    let a = control_plane(store.clone(), Some(KEY)).await;
    let b = control_plane(store.clone(), Some(KEY)).await;
    let body = json!({"templateID":"base", "metadata":{"hm.name":"exclusive"}});
    let request = |base: &str, path: &str, body: Value| {
        client()
            .post(format!("{base}{path}"))
            .header("x-api-key", KEY)
            .json(&body)
            .send()
    };
    let (ra, rb) = tokio::join!(
        request(&a, "/sandboxes", body.clone()),
        request(&b, "/v2/sandboxes", body.clone())
    );
    let ra = ra.unwrap();
    let rb = rb.unwrap();
    assert!(matches!(
        (ra.status().as_u16(), rb.status().as_u16()),
        (201, 409) | (409, 201)
    ));
    let winner = if ra.status().is_success() { ra } else { rb };
    let descriptor = winner.json::<Value>().await.unwrap();
    let id = descriptor["sandboxID"].as_str().unwrap();
    assert_eq!(node.running.lock().len(), 1);
    let name = SandboxName::parse("exclusive").unwrap();
    let reservation = store.name_reservation(&name).await.unwrap().unwrap();
    assert_eq!(reservation.sandbox_id(), Some(id));
    assert!(!descriptor
        .to_string()
        .contains(reservation.operation_token()));
    assert_eq!(
        request(&a, "/sandboxes", body.clone())
            .await
            .unwrap()
            .status(),
        409
    );
    for invalid in [json!("bad name"), json!(""), json!(12), Value::Null] {
        assert_eq!(
            request(
                &a,
                "/sandboxes",
                json!({"templateID":"base","metadata":{"hm.name":invalid}})
            )
            .await
            .unwrap()
            .status(),
            400
        );
    }
    assert_eq!(node.running.lock().len(), 1);
    assert_eq!(
        client()
            .delete(format!("{a}/sandboxes/{id}"))
            .header("x-api-key", KEY)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert!(store.name_reservation(&name).await.unwrap().is_none());
    assert_eq!(
        request(&b, "/v2/sandboxes", body).await.unwrap().status(),
        201
    );
    heartbeat.abort();
    let _ = heartbeat.await;
}

#[tokio::test]
async fn uncertain_named_creation_keeps_ownership_and_never_tries_another_node() {
    use hv2_cluster::names::SandboxName;
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let (a, ha) = fake_node(store.clone(), "a", 8, Duration::from_secs(30)).await;
    let (b, hb) = fake_node(store.clone(), "b", 8, Duration::from_secs(30)).await;
    ha.abort();
    let _ = ha.await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut info = store.node("a").await.unwrap().unwrap();
    info.api = format!("http://{}", listener.local_addr().unwrap());
    // Keep the first node preferred even after its created() announcement.
    let mut other = store.node("b").await.unwrap().unwrap();
    other.running = 7;
    hb.abort();
    let _ = hb.await;
    store
        .put_node(&other, Duration::from_secs(30))
        .await
        .unwrap();
    let app =
        Router::new()
            .route(
                "/v2/sandboxes",
                post(
                    |State(node): State<Arc<FakeNode>>,
                     headers: HeaderMap,
                     Json(body): Json<Value>| async move {
                        let mode = body["metadata"]["test.mode"].as_str().unwrap().to_owned();
                        let result = node_create(State(node), headers, Json(body)).await;
                        assert_eq!(result.status(), StatusCode::CREATED);
                        match mode.as_str() {
                            "unavailable" => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                            "malformed" => (StatusCode::CREATED, "invalid JSON").into_response(),
                            "timeout" => {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                result
                            }
                            _ => (
                                StatusCode::CREATED,
                                Json(json!({"sandboxID":"wrong-target"})),
                            )
                                .into_response(),
                        }
                    },
                ),
            )
            .with_state(a.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = control_plane_with_timeout(
        store.clone(),
        Some(KEY),
        Vec::new(),
        Duration::from_millis(500),
    )
    .await;
    for (mode, status) in [
        ("unavailable", 503),
        ("malformed", 502),
        ("mismatch", 502),
        ("timeout", 502),
    ] {
        store
            .put_node(&info, Duration::from_secs(30))
            .await
            .unwrap();
        let body = json!({"templateID":"base","metadata":{"hm.name":mode,"test.mode":mode}});
        let (actual, _) = create(&base, body.clone()).await;
        assert_eq!(actual, status);
        let name = SandboxName::parse(mode).unwrap();
        let reservation = store.name_reservation(&name).await.unwrap().unwrap();
        assert!(reservation.sandbox_id().is_none());
        assert_eq!(create(&base, body).await.0, 409);
        assert!(b.running.lock().is_empty());
    }
    assert_eq!(a.running.lock().len(), 4);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let another = control_plane(store.clone(), Some(KEY)).await;
    assert_eq!(
        create(
            &another,
            json!({"templateID":"base","metadata":{"hm.name":"timeout"}})
        )
        .await
        .0,
        409
    );
    assert!(store
        .name_reservation(&SandboxName::parse("timeout").unwrap())
        .await
        .unwrap()
        .unwrap()
        .sandbox_id()
        .is_none());
    task.abort();
    let _ = task.await;
    // An unreachable node also leaves an uncertain reservation, without fallback.
    store
        .put_node(&info, Duration::from_secs(30))
        .await
        .unwrap();
    let (status, _) = create(
        &base,
        json!({"templateID":"base","metadata":{"hm.name":"unreachable"}}),
    )
    .await;
    assert_eq!(status, 502);
    assert!(store
        .name_reservation(&SandboxName::parse("unreachable").unwrap())
        .await
        .unwrap()
        .unwrap()
        .sandbox_id()
        .is_none());
    assert!(b.running.lock().is_empty());
}
