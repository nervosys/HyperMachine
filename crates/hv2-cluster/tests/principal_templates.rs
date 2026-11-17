//! Template builds by a key with a principal and no team are made under that
//! principal's scope.
//!
//! A node keeps one template namespace and records no owner on a build, so the
//! control plane puts the scope in the template's name: what alice builds as
//! `app` is `scope.<hash>--app` on the node, and is hers alone to see, start,
//! rebuild or delete. These tests run the control plane against a node that
//! keeps builds as the real one does -- by name, and by build ID alone.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::model::{now_ms, NodeInfo};
use hv2_cluster::ownership::{OwnerId, TeamId};
use hv2_cluster::store::{ClusterStore, MemoryStore};

const TOKEN: &str = "cluster-secret";

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The prefix a principal's templates have on the node.
fn scope(principal: &str) -> String {
    TeamId::personal(&OwnerId::parse(principal).unwrap())
        .template_prefix()
        .expect("a principal's scope has a prefix")
}

fn digest(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What the node was asked: method, path and query, body.
type Calls = Arc<Mutex<Vec<(String, String, String)>>>;

/// A node that keeps what the real one keeps of a build: its template's name,
/// found by build ID alone. Builds are named `build-N`.
struct FakeNode {
    calls: Calls,
    address: std::net::SocketAddr,
    _server: Owned,
}

impl FakeNode {
    async fn start() -> Self {
        let calls: Calls = Arc::default();
        let builds: Arc<Mutex<BTreeMap<String, String>>> = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (seen, kept) = (calls.clone(), builds.clone());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(move |method: Method, uri: axum::http::Uri, body: Bytes| {
                    let (seen, kept) = (seen.clone(), kept.clone());
                    async move {
                        let path = uri.path().to_owned();
                        let text = String::from_utf8_lossy(&body).into_owned();
                        seen.lock().push((
                            method.to_string(),
                            uri.path_and_query().map_or("", |p| p.as_str()).to_owned(),
                            text.clone(),
                        ));
                        let parts: Vec<&str> = path.split('/').collect();
                        match (method.as_str(), parts.as_slice()) {
                            ("POST", ["", "v3", "templates"]) => {
                                let body: Value = serde_json::from_str(&text).unwrap();
                                let requested = body["name"]
                                    .as_str()
                                    .or_else(|| body["alias"].as_str())
                                    .unwrap();
                                let name = requested.split(':').next().unwrap().to_owned();
                                let id = format!("build-{}", kept.lock().len());
                                kept.lock().insert(id.clone(), name.clone());
                                (
                                    StatusCode::ACCEPTED,
                                    Json(json!({"templateID": name, "buildID": id})),
                                )
                            }
                            ("GET", ["", "templates", _, "builds", build, "status"]) => {
                                match kept.lock().get(*build) {
                                    Some(name) => (
                                        StatusCode::OK,
                                        Json(json!({"templateID": name, "buildID": build,
                                                "status": "ready", "logEntries": []})),
                                    ),
                                    None => (StatusCode::NOT_FOUND, Json(json!({}))),
                                }
                            }
                            ("POST", ["", "v2", "templates", _, "builds", _]) => {
                                (StatusCode::ACCEPTED, Json(json!({})))
                            }
                            ("GET", ["", "templates", _, "files", _]) => {
                                (StatusCode::CREATED, Json(json!({"present": false})))
                            }
                            ("DELETE", ["", "templates", _]) => {
                                (StatusCode::NO_CONTENT, Json(json!({})))
                            }
                            ("POST", ["", "templates"]) => (StatusCode::ACCEPTED, Json(json!({}))),
                            ("POST", ["", "sandboxes"]) => {
                                (StatusCode::CREATED, Json(json!({"sandboxID": "made"})))
                            }
                            _ => (StatusCode::NOT_FOUND, Json(json!({}))),
                        }
                    }
                }),
            )
            .await
            .unwrap();
        });
        Self {
            calls,
            address,
            _server: Owned(server),
        }
    }

    /// Say which templates the node offers, as its heartbeat does.
    async fn offer(&self, store: &Arc<dyn ClusterStore>, templates: &[String]) {
        let mut names = vec!["base".to_owned()];
        names.extend(templates.iter().cloned());
        store
            .put_node(
                &NodeInfo {
                    id: "n".into(),
                    api: format!("http://{}", self.address),
                    proxy: self.address,
                    capacity: 8,
                    running: 0,
                    heartbeat_ms: now_ms(),
                    version: "fixture".into(),
                    jwk: None,
                    templates: names,
                    template_metadata: Default::default(),
                },
                Duration::from_secs(60),
            )
            .await
            .unwrap();
    }

    /// Calls made so far.
    fn calls(&self) -> Vec<(String, String, String)> {
        self.calls.lock().clone()
    }
}

struct Plane {
    base: String,
    red: String,
    _server: Owned,
}

impl Plane {
    /// `alice-key` and `bob-key` have a principal and no team, `admin-key` is
    /// an administrator, and `red-key` is a team's.
    async fn new(store: Arc<dyn ClusterStore>) -> Self {
        let expires = chrono::Utc::now().timestamp() + 600;
        let scopes = ["sandboxes", "inventory", "templates", "machines"];
        let policies = json!([
            {"sha256":digest("alice-key"),"expires_at":expires,"scopes":scopes,
             "role":"operator","principal_id":"alice"},
            {"sha256":digest("bob-key"),"expires_at":expires,"scopes":scopes,
             "role":"operator","principal_id":"bob"},
            {"sha256":digest("admin-key"),"expires_at":expires,"scopes":["admin"],
             "role":"operator"},
        ]);
        let control = ControlPlane::new(
            store,
            ControlConfig {
                api_key: None,
                api_keys: hv2_cluster::keys::ApiKeyPolicy::from_json(&policies.to_string())
                    .unwrap(),
                access_audit: None,
                cluster_token: Some(TOKEN.into()),
                proxy_port: 3000,
                create_timeout: Duration::from_secs(5),
                identity_issuer: None,
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = control::router(control.clone());
        let server = Owned(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }));
        control.set_managed_key_secret(vec![0x5a; 48]).unwrap();
        let made: Value = reqwest::Client::new()
            .post(format!("{base}/keys"))
            .header("x-api-key", "admin-key")
            .json(
                &json!({"label":"red","expiresInSeconds":600,"scopes":scopes,
                "role":"operator","teamId":"red","principalId":"rita"}),
            )
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        Self {
            base,
            red: made["key"].as_str().expect("a managed key").to_owned(),
            _server: server,
        }
    }

    fn call(&self, method: Method, path: &str, key: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .header("x-api-key", if key == "red-key" { &self.red } else { key })
    }

    async fn status(&self, method: Method, path: &str, key: &str) -> u16 {
        self.call(method, path, key)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    async fn post(&self, path: &str, key: &str, body: Value) -> (u16, Value) {
        let response = self
            .call(Method::POST, path, key)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn ids(&self, key: &str) -> Vec<String> {
        let response = self
            .call(Method::GET, "/templates", key)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{key}");
        let rows: Value = response.json().await.unwrap();
        let mut ids: Vec<String> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["templateID"].as_str().unwrap().to_owned())
            .collect();
        ids.sort();
        ids
    }
}

/// The whole of it: alice builds `app`, and bob and a team can do nothing
/// with it, while alice builds, reads, starts from and deletes it as she did
/// before, and an administrator sees it.
#[tokio::test]
async fn a_principals_template_is_its_own() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let node = FakeNode::start().await;
    node.offer(&store, &[]).await;
    let plane = Plane::new(store.clone()).await;
    let (a, b) = (scope("alice"), scope("bob"));
    assert_ne!(a, b);

    // Alice names a build; the node is told her scope's name, tag kept.
    let (status, made) = plane
        .post("/v3/templates", "alice-key", json!({"name": "app:v1"}))
        .await;
    assert_eq!(status, 202, "{made}");
    assert_eq!(made["templateID"], format!("{a}app"));
    assert_eq!(made["buildID"], "build-0");
    assert!(
        node.calls().iter().any(|(m, p, body)| m == "POST"
            && p == "/v3/templates"
            && body.contains(&format!("{a}app:v1"))),
        "{:?}",
        node.calls()
    );
    // Bob builds one of the same name: his own, beside it.
    let (status, made) = plane
        .post("/v3/templates", "bob-key", json!({"alias": "app"}))
        .await;
    assert_eq!(status, 202, "{made}");
    assert_eq!(made["templateID"], format!("{b}app"));
    assert_eq!(made["buildID"], "build-1");

    // Alice's calls, by the plain name and by the ID the build answered with.
    let canonical = format!("{a}app");
    for id in ["app", canonical.as_str()] {
        assert_eq!(
            plane
                .status(
                    Method::GET,
                    &format!("/templates/{id}/files/abc"),
                    "alice-key"
                )
                .await,
            201,
            "{id}"
        );
        assert_eq!(
            plane
                .status(
                    Method::GET,
                    &format!("/templates/{id}/builds/build-0/status"),
                    "alice-key"
                )
                .await,
            200,
            "{id}"
        );
    }
    let before = node.calls().len();
    assert_eq!(
        plane
            .post(
                "/v2/templates/app/builds/build-0",
                "alice-key",
                json!({"fromImage": "alpine"})
            )
            .await
            .0,
        202
    );
    assert_eq!(node.calls().len(), before + 2, "the check, then the start");

    // Bob: another's build ID under any name, another's name, and another's
    // template ID are not there, and nothing reaches a build of alice's.
    let before = node.calls().len();
    for (method, path) in [
        (
            Method::GET,
            "/templates/app/builds/build-0/status".to_owned(),
        ),
        (
            Method::GET,
            format!("/templates/{a}app/builds/build-0/status"),
        ),
        (Method::GET, format!("/templates/{a}app/files/abc")),
        (Method::POST, "/v2/templates/app/builds/build-0".to_owned()),
        (Method::POST, format!("/v2/templates/{a}app/builds/build-0")),
    ] {
        let status = plane.status(method.clone(), &path, "bob-key").await;
        assert_eq!(status, 404, "{method} {path}");
    }
    let touched: Vec<_> = node.calls().split_off(before);
    assert!(
        touched
            .iter()
            .all(|(m, p, _)| m == "GET" && p.ends_with("/status")),
        "only the ownership check reached the node: {touched:?}"
    );
    assert!(
        !touched.iter().any(|(_, p, _)| p.contains(&a)),
        "{touched:?}"
    );
    // Bob cannot name alice's template to build over it.
    let before = node.calls().len();
    for name in [format!("{a}app"), format!("{a}app:v2")] {
        for field in ["name", "alias"] {
            let (status, _) = plane
                .post("/v3/templates", "bob-key", json!({ (field): name }))
                .await;
            assert_eq!(status, 403, "{field} {name}");
        }
    }
    let (status, _) = plane
        .post(
            "/templates",
            "bob-key",
            json!({"templateID": format!("{a}img"), "image": "alpine"}),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(node.calls().len(), before, "nothing reached the node");
    // A team's key has no business with builds at all.
    for path in ["/v3/templates", "/templates"] {
        assert_eq!(
            plane
                .post(path, "red-key", json!({"name": "app", "templateID": "app"}))
                .await
                .0,
            403,
            "{path}"
        );
    }

    // The node now offers all three; who sees which.
    node.offer(
        &store,
        &[format!("{a}app"), format!("{b}app"), format!("{a}img")],
    )
    .await;
    assert_eq!(
        plane.ids("alice-key").await,
        ["base".to_owned(), format!("{a}app"), format!("{a}img")]
    );
    assert_eq!(
        plane.ids("bob-key").await,
        ["base".to_owned(), format!("{b}app")]
    );
    assert_eq!(plane.ids("red-key").await, ["base"]);
    assert_eq!(plane.ids("admin-key").await.len(), 4);
    let rows: Value = plane
        .call(Method::GET, "/templates", "alice-key")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let app = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["templateID"] == format!("{a}app"))
        .unwrap();
    assert_eq!(app["aliases"], json!(["app"]), "alice knows it as app");
    let nodes: Value = plane
        .call(Method::GET, "/cluster/nodes", "bob-key")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        nodes[0]["templates"],
        json!(["base", format!("{b}app")]),
        "{nodes}"
    );

    // Aliases.
    for (key, path, expected) in [
        ("alice-key", "app".to_owned(), Some(format!("{a}app"))),
        ("bob-key", "app".to_owned(), Some(format!("{b}app"))),
        ("bob-key", format!("{a}app"), None),
        ("red-key", format!("{a}app"), None),
        ("red-key", "app".to_owned(), None),
        ("admin-key", format!("{a}app"), Some(format!("{a}app"))),
    ] {
        let response = plane
            .call(Method::GET, &format!("/templates/aliases/{path}"), key)
            .send()
            .await
            .unwrap();
        match expected {
            Some(id) => {
                assert_eq!(response.status(), 200, "{key} {path}");
                let found: Value = response.json().await.unwrap();
                assert_eq!(found["templateID"], id, "{key} {path}");
            }
            None => assert_eq!(response.status(), 404, "{key} {path}"),
        }
    }

    // Starting a sandbox: the plain name is the caller's own, and another
    // scope's name is no template.
    let before = node.calls().len();
    for (key, template, expected) in [
        ("alice-key", "app".to_owned(), 201),
        ("alice-key", "app:v1".to_owned(), 201),
        ("bob-key", "app".to_owned(), 201),
        ("bob-key", format!("{a}app"), 404),
        ("red-key", format!("{a}app"), 404),
        ("red-key", "app".to_owned(), 404),
        ("admin-key", format!("{a}app"), 201),
    ] {
        let (status, _) = plane
            .post("/sandboxes", key, json!({"templateID": template}))
            .await;
        assert_eq!(status, expected, "{key} {template}");
    }
    let started: Vec<String> = node
        .calls()
        .split_off(before)
        .into_iter()
        .filter(|(m, p, _)| m == "POST" && p == "/sandboxes")
        .map(|(_, _, body)| {
            serde_json::from_str::<Value>(&body).unwrap()["templateID"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        started,
        [
            format!("{a}app"),
            format!("{a}app"),
            format!("{b}app"),
            format!("{a}app")
        ]
    );

    // A build that starts from a template: the caller's own, by its plain
    // name; never another's.
    let before = node.calls().len();
    assert_eq!(
        plane
            .post(
                "/v2/templates/app/builds/build-0",
                "alice-key",
                json!({"fromTemplate": "app"})
            )
            .await
            .0,
        202
    );
    let sent = node.calls().split_off(before);
    assert!(
        sent.iter()
            .any(|(m, _, body)| m == "POST" && body.contains(&format!("\"{a}app\""))),
        "{sent:?}"
    );
    let before = node.calls().len();
    assert_eq!(
        plane
            .post(
                "/v2/templates/app/builds/build-1",
                "bob-key",
                json!({"fromTemplate": format!("{a}app")})
            )
            .await
            .0,
        400
    );
    assert!(
        !node
            .calls()
            .split_off(before)
            .iter()
            .any(|(m, _, _)| m == "POST"),
        "no build was started"
    );

    // Deleting: one's own, by either name; never another's.
    let before = node.calls().len();
    for path in [format!("{a}app"), format!("{a}img")] {
        assert_eq!(
            plane
                .status(Method::DELETE, &format!("/templates/{path}"), "bob-key")
                .await,
            404,
            "{path}"
        );
    }
    assert_eq!(
        plane
            .status(Method::DELETE, &format!("/templates/{a}app"), "red-key")
            .await,
        404
    );
    assert_eq!(
        plane
            .status(Method::DELETE, "/templates/base", "bob-key")
            .await,
        404,
        "an operator's template is not his to delete"
    );
    assert_eq!(node.calls().len(), before, "nothing reached the node");
    assert_eq!(
        plane
            .status(Method::DELETE, "/templates/app", "bob-key")
            .await,
        204
    );
    assert_eq!(
        plane
            .status(Method::DELETE, &format!("/templates/{a}img"), "alice-key")
            .await,
        204
    );
    let deleted: Vec<String> = node
        .calls()
        .split_off(before)
        .into_iter()
        .filter(|(m, _, _)| m == "DELETE")
        .map(|(_, p, _)| p)
        .collect();
    assert!(deleted
        .iter()
        .all(|p| !p.contains(&a) || p.ends_with("img")));
    assert!(
        deleted.contains(&format!("/templates/{b}app")),
        "{deleted:?}"
    );
    // The administrator sees and removes anyone's.
    assert_eq!(
        plane
            .status(Method::DELETE, &format!("/templates/{a}app"), "admin-key")
            .await,
        204
    );
}

/// The image build `POST /templates` takes its name from the body.
#[tokio::test]
async fn an_image_build_is_made_under_the_principals_scope() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let node = FakeNode::start().await;
    node.offer(&store, &[]).await;
    let plane = Plane::new(store).await;
    for (key, name) in [("alice-key", "img"), ("admin-key", "img")] {
        let (status, _) = plane
            .post(
                "/templates",
                key,
                json!({"templateID": name, "image": "alpine"}),
            )
            .await;
        assert_eq!(status, 202, "{key}");
    }
    let named: Vec<String> = node
        .calls()
        .into_iter()
        .filter(|(m, p, _)| m == "POST" && p == "/templates")
        .map(|(_, _, body)| {
            serde_json::from_str::<Value>(&body).unwrap()["templateID"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(named, [format!("{}img", scope("alice")), "img".to_owned()]);
}
