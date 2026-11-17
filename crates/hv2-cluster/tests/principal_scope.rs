//! A key with a principal and no team sees only what its principal created.
//!
//! Such a key acts for a team of one ([`TeamId::personal`]): the control
//! plane sends that label to nodes in place of a team, nodes record it on
//! what the key creates, and every team check fences principals as it fences
//! tenants. Each test has two teamless keys with different principals, a
//! team key and an administrator, and what each resource kind shows each.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, Method, StatusCode};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::events::Dispatcher;
use hv2_cluster::model::{now_ms, ClusterEvent, NodeInfo, SandboxRecord, TemplateInfo};
use hv2_cluster::ownership::{OwnerId, TeamId, OWNER_HEADER, TEAM_HEADER};
use hv2_cluster::store::{ClusterStore, MemoryStore};

const TOKEN: &str = "cluster-secret";

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn me(principal: &str) -> TeamId {
    TeamId::personal(&OwnerId::parse(principal).unwrap())
}

fn digest(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Plane {
    base: String,
    control: Arc<ControlPlane>,
    /// The secret of `red-key`, a managed key: a policy file that names a
    /// team may not also hold a key without one.
    red: String,
    _server: Owned,
}

impl Plane {
    /// Keys: `alice-key` and `bob-key` (principals, no team), `admin-key`
    /// (all in the policy file), and `red-key` (a team's, made through the
    /// key API).
    async fn new(store: Arc<dyn ClusterStore>) -> Self {
        let expires = chrono::Utc::now().timestamp() + 600;
        let scopes = [
            "sandboxes",
            "inventory",
            "templates",
            "volumes",
            "events",
            "machines",
            "telemetry",
        ];
        let policies = json!([
            {"sha256":digest("alice-key"),"expires_at":expires,"scopes":scopes,
             "role":"operator","principal_id":"alice"},
            {"sha256":digest("bob-key"),"expires_at":expires,"scopes":scopes,
             "role":"operator","principal_id":"bob"},
            {"sha256":digest("admin-key"),"expires_at":expires,"scopes":["admin"],
             "role":"operator"},
        ]);
        let control = ControlPlane::new(
            store.clone(),
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
            control,
            red: made["key"].as_str().expect("a managed key").to_owned(),
            _server: server,
        }
    }

    fn call(&self, method: Method, path: &str, key: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .header("x-api-key", if key == "red-key" { &self.red } else { key })
            // Never believed: the control plane takes both from the key.
            .header(TEAM_HEADER, "red")
            .header(OWNER_HEADER, "alice")
    }

    async fn status(&self, method: Method, path: &str, key: &str) -> u16 {
        self.call(method, path, key)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    async fn json(&self, path: &str, key: &str) -> Value {
        let response = self.call(Method::GET, path, key).send().await.unwrap();
        assert_eq!(response.status(), 200, "{key} GET {path}");
        response.json().await.unwrap()
    }
}

fn column(rows: &Value, field: &str) -> BTreeSet<String> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| row[field].as_str().unwrap_or("").to_owned())
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn node(name: &str, address: std::net::SocketAddr) -> NodeInfo {
    NodeInfo {
        id: name.into(),
        api: format!("http://{address}"),
        proxy: address,
        capacity: 8,
        running: 0,
        heartbeat_ms: now_ms(),
        version: "fixture".into(),
        jwk: None,
        templates: vec!["base".into()],
        template_metadata: Default::default(),
    }
}

/// What a node does with the creating key's owner and team: it records them.
fn record(id: &str, owner: Option<&str>, team: Option<TeamId>) -> SandboxRecord {
    serde_json::from_value(json!({
        "sandbox_id": id, "node_id": "missing-node", "template_id": "base",
        "started_at_ms": 1, "end_at_ms": 600000, "cpu_count": 1, "memory_mb": 1024,
        "envd_version": "fixture", "descriptor": {},
        "owner_id": owner, "team_id": team.map(String::from),
    }))
    .unwrap()
}

async fn seed_sandboxes(store: &Arc<dyn ClusterStore>) {
    for rec in [
        record("alice-sbx", Some("alice"), Some(me("alice"))),
        record("bob-sbx", Some("bob"), Some(me("bob"))),
        record("red-sbx", Some("rita"), Some(TeamId::parse("red").unwrap())),
        // Made before principals were scoped: an owner and no team.
        record("alice-old", Some("alice"), None),
        record("legacy", None, None),
    ] {
        store.put_sandbox(&rec).await.unwrap();
    }
}

/// A node that answers GET `listing_path` with `rows` and records the
/// owner/team headers of every other call.
type Seen = Arc<Mutex<Vec<(String, String, Vec<String>, Option<String>)>>>;
async fn fake_node(
    store: &Arc<dyn ClusterStore>,
    name: &str,
    listing_path: &'static str,
    rows: Value,
    seen: &Seen,
) -> Owned {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = seen.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(move |method: Method, uri: axum::http::Uri, h: HeaderMap| {
                let seen = seen.clone();
                let rows = rows.clone();
                async move {
                    if method == Method::GET && uri.path() == listing_path {
                        return (StatusCode::OK, Json(rows));
                    }
                    seen.lock().push((
                        method.to_string(),
                        uri.path().to_owned(),
                        h.get_all(TEAM_HEADER)
                            .iter()
                            .map(|v| v.to_str().unwrap().to_owned())
                            .collect(),
                        h.get(OWNER_HEADER).map(|v| v.to_str().unwrap().to_owned()),
                    ));
                    (StatusCode::CREATED, Json(json!({"sandboxID":"made"})))
                }
            }),
        )
        .await
        .unwrap();
    });
    store
        .put_node(&node(name, address), Duration::from_secs(60))
        .await
        .unwrap();
    Owned(server)
}

#[tokio::test]
async fn what_a_teamless_key_creates_is_made_for_its_principals_scope() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let seen: Seen = Arc::default();
    let _node = fake_node(&store, "n", "/machines", json!([]), &seen).await;
    let plane = Plane::new(store).await;
    for (key, path) in [
        ("alice-key", "/sandboxes"),
        ("bob-key", "/v2/sandboxes"),
        ("red-key", "/sandboxes"),
        ("admin-key", "/sandboxes"),
        ("alice-key", "/machines"),
        ("bob-key", "/volumes"),
    ] {
        let status = plane
            .call(Method::POST, path, key)
            .json(&json!({"templateID":"base","name":"thing"}))
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, 201, "{key} {path}");
    }
    let creates: Vec<_> = seen
        .lock()
        .iter()
        .map(|(_, path, team, owner)| (path.clone(), team.clone(), owner.clone()))
        .collect();
    let team = |t: TeamId| vec![t.as_str().to_owned()];
    assert_eq!(
        creates,
        vec![
            ("/sandboxes".into(), team(me("alice")), Some("alice".into())),
            ("/v2/sandboxes".into(), team(me("bob")), Some("bob".into())),
            (
                "/sandboxes".into(),
                vec!["red".to_owned()],
                Some("rita".into())
            ),
            ("/sandboxes".into(), Vec::new(), None),
            ("/machines".into(), team(me("alice")), None),
            ("/volumes".into(), team(me("bob")), None),
        ]
    );
    assert_ne!(me("alice"), me("bob"));
}

#[tokio::test]
async fn a_real_team_may_not_take_a_principals_label() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let expires = chrono::Utc::now().timestamp() + 600;
    let policies = json!([
        {"sha256":digest("squatter"),"expires_at":expires,"scopes":["events"],
         "role":"operator","principal_id":"mallory","team_id":me("alice").as_str()},
    ]);
    let control = ControlPlane::new(
        store,
        ControlConfig {
            api_key: None,
            api_keys: hv2_cluster::keys::ApiKeyPolicy::from_json(&policies.to_string()).unwrap(),
            access_audit: None,
            cluster_token: Some(TOKEN.into()),
            proxy_port: 3000,
            create_timeout: Duration::from_secs(5),
            identity_issuer: None,
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = control::router(control);
    let _server = Owned(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let response = reqwest::Client::new()
        .get(format!("{base}/events/sandboxes"))
        .header("x-api-key", "squatter")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn events_and_webhooks_belong_to_the_principal_that_made_them() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    seed_sandboxes(&store).await;
    for id in ["alice-sbx", "bob-sbx", "red-sbx", "alice-old", "legacy"] {
        let rec = store.sandbox(id).await.unwrap().unwrap();
        store
            .publish(&ClusterEvent::new("sandbox-created", "n", Some(id)).with_record(&rec))
            .await
            .unwrap();
    }
    let plane = Plane::new(store.clone()).await;

    // Each key reads its own sandboxes' events: a teamless key not a
    // team's, nor another principal's, nor a record that has no owner scope.
    for (key, expected) in [
        ("alice-key", vec!["alice-sbx"]),
        ("bob-key", vec!["bob-sbx"]),
        ("red-key", vec!["red-sbx"]),
        (
            "admin-key",
            vec!["alice-sbx", "bob-sbx", "red-sbx", "alice-old", "legacy"],
        ),
    ] {
        let rows = plane.json("/events/sandboxes", key).await;
        assert_eq!(column(&rows, "sandboxId"), set(&expected), "{key}");
    }
    // The cluster-wide event log is filtered by nobody's scope.
    for key in ["alice-key", "bob-key", "red-key"] {
        assert_eq!(
            plane.status(Method::GET, "/cluster/events", key).await,
            403,
            "{key}"
        );
    }
    assert_eq!(
        plane
            .status(Method::GET, "/cluster/events", "admin-key")
            .await,
        200
    );
    // Another principal's sandbox's events are not readable at all.
    assert_eq!(
        plane
            .status(Method::GET, "/events/sandboxes/bob-sbx", "alice-key")
            .await,
        403
    );

    // Webhooks: each key makes one, sees one.
    let mut ids = Vec::new();
    for key in ["alice-key", "bob-key", "red-key", "admin-key"] {
        let response = plane
            .call(Method::POST, "/events/webhooks", key)
            .json(&json!({"name": key, "url": "https://example.com/hook",
                "signatureSecret": "s".repeat(16)}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201, "{key}");
        let made: Value = response.json().await.unwrap();
        ids.push(made["id"].as_str().unwrap().to_owned());
    }
    for (key, expected) in [
        ("alice-key", vec!["alice-key"]),
        ("bob-key", vec!["bob-key"]),
        ("red-key", vec!["red-key"]),
        (
            "admin-key",
            vec!["alice-key", "bob-key", "red-key", "admin-key"],
        ),
    ] {
        let rows = plane.json("/events/webhooks", key).await;
        assert_eq!(column(&rows, "name"), set(&expected), "{key}");
    }
    // Another principal's webhook, or a team's, is not there to read,
    // change, remove or ask about.
    for other in [&ids[1], &ids[2], &ids[3]] {
        for (method, suffix) in [
            (Method::GET, ""),
            (Method::PATCH, ""),
            (Method::DELETE, ""),
            (Method::GET, "/deliveries"),
            (Method::GET, "/stats"),
        ] {
            let mut request = plane.call(
                method.clone(),
                &format!("/events/webhooks/{other}{suffix}"),
                "alice-key",
            );
            if method == Method::PATCH {
                request = request.json(&json!({"enabled": false}));
            }
            assert_eq!(
                request.send().await.unwrap().status(),
                404,
                "{method} {other}{suffix}"
            );
        }
    }
    assert_eq!(store.webhooks().await.unwrap().len(), 4, "none was removed");
    assert_eq!(
        plane
            .status(
                Method::GET,
                &format!("/events/webhooks/{}", ids[0]),
                "alice-key"
            )
            .await,
        200
    );
    // The record names its principal's scope; the administrator's names none.
    let hooks = store.webhooks().await.unwrap();
    let scope_of = |id: &str| hooks.iter().find(|h| h.id == id).unwrap().team_id.clone();
    assert_eq!(scope_of(&ids[0]), Some(me("alice")));
    assert_eq!(scope_of(&ids[1]), Some(me("bob")));
    assert_eq!(scope_of(&ids[2]), Some(TeamId::parse("red").unwrap()));
    assert_eq!(scope_of(&ids[3]), None);
    // Its owner reaches its own.
    assert_eq!(
        plane
            .status(
                Method::DELETE,
                &format!("/events/webhooks/{}", ids[0]),
                "alice-key"
            )
            .await,
        204
    );
}

#[tokio::test]
async fn a_principals_webhook_is_delivered_only_events_about_its_resources() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    seed_sandboxes(&store).await;
    let plane = Plane::new(store.clone()).await;

    let received: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let sink = received.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let receiver = listener.local_addr().unwrap();
    let _receiver = Owned(tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(move |uri: axum::http::Uri, body: String| {
                let sink = sink.clone();
                async move {
                    let id = serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|v| v["sandbox_id"].as_str().map(str::to_owned))
                        .unwrap_or_default();
                    sink.lock().push((uri.path().to_owned(), id));
                    StatusCode::OK
                }
            }),
        )
        .await
        .unwrap();
    }));
    for (key, path) in [
        ("alice-key", "/alice"),
        ("bob-key", "/bob"),
        ("red-key", "/red"),
        ("admin-key", "/admin"),
    ] {
        let response = plane
            .call(Method::POST, "/events/webhooks", key)
            .json(
                &json!({"name": key, "url": format!("http://{receiver}{path}"),
                "signatureSecret": "s".repeat(16)}),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201, "{key}");
    }
    let dispatcher = Dispatcher::new(store.clone(), true);
    for id in ["alice-sbx", "bob-sbx", "red-sbx", "legacy"] {
        let rec = store.sandbox(id).await.unwrap().unwrap();
        dispatcher.deliver(&ClusterEvent::new("sandbox-created", "n", Some(id)).with_record(&rec));
    }
    let wanted = 7;
    for _ in 0..100 {
        if received.lock().len() >= wanted {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut got = received.lock().clone();
    got.sort();
    let expected: Vec<(String, String)> = [
        ("/admin", "alice-sbx"),
        ("/admin", "bob-sbx"),
        ("/admin", "legacy"),
        ("/admin", "red-sbx"),
        ("/alice", "alice-sbx"),
        ("/bob", "bob-sbx"),
        ("/red", "red-sbx"),
    ]
    .iter()
    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
    .collect();
    assert_eq!(got, expected);
}

#[tokio::test]
async fn machines_and_volumes_are_listed_and_reached_by_their_principal_only() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let seen: Seen = Arc::default();
    let a = me("alice");
    let b = me("bob");
    let machines = json!([
        {"machineID":"vm-0000000000000001","name":"m","teamID":a.as_str()},
        {"machineID":"vm-0000000000000002","name":"m","teamID":b.as_str()},
        {"machineID":"vm-0000000000000003","name":"m","teamID":"red"},
        {"machineID":"vm-0000000000000004","name":"old"},
    ]);
    let _machines = fake_node(&store, "machine-node", "/machines", machines, &seen).await;
    let plane = Plane::new(store.clone()).await;

    for (key, expected) in [
        ("alice-key", vec!["vm-0000000000000001"]),
        ("bob-key", vec!["vm-0000000000000002"]),
        ("red-key", vec!["vm-0000000000000003"]),
        (
            "admin-key",
            vec![
                "vm-0000000000000001",
                "vm-0000000000000002",
                "vm-0000000000000003",
                "vm-0000000000000004",
            ],
        ),
    ] {
        let rows = plane.json("/machines", key).await;
        assert_eq!(column(&rows, "machineID"), set(&expected), "{key}");
    }
    // Someone else's machine, by ID, and a machine with no owner, are not
    // there; a name means the caller's own.
    for id in [
        "vm-0000000000000002",
        "vm-0000000000000003",
        "vm-0000000000000004",
    ] {
        for method in [Method::GET, Method::DELETE, Method::PATCH] {
            assert_eq!(
                plane
                    .status(method.clone(), &format!("/machines/{id}"), "alice-key")
                    .await,
                404,
                "{method} {id}"
            );
        }
        assert_eq!(
            plane
                .status(Method::POST, &format!("/machines/{id}/start"), "alice-key")
                .await,
            404,
            "{id}"
        );
    }
    assert!(seen.lock().is_empty(), "nothing reached a node");
    assert_eq!(
        plane
            .status(Method::GET, "/machines/vm-0000000000000001", "alice-key")
            .await,
        201,
        "its own is reached (the fixture node answers every call 201)"
    );
    assert_eq!(
        plane.status(Method::GET, "/machines/m", "bob-key").await,
        201
    );
    let calls = seen.lock().clone();
    assert_eq!(
        calls
            .iter()
            .map(|(_, _, team, _)| team.clone())
            .collect::<Vec<_>>(),
        vec![vec![a.as_str().to_owned()], vec![b.as_str().to_owned()]]
    );

    // Volumes.
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let seen: Seen = Arc::default();
    let volumes = json!([
        {"volumeID":"vol-a","name":"d","teamID":a.as_str()},
        {"volumeID":"vol-b","name":"d","teamID":b.as_str()},
        {"volumeID":"vol-red","name":"d","teamID":"red"},
        {"volumeID":"vol-old","name":"d"},
    ]);
    let _volumes = fake_node(&store, "volume-node", "/volumes", volumes, &seen).await;
    let plane = Plane::new(store).await;
    for (key, expected) in [
        ("alice-key", vec!["vol-a"]),
        ("bob-key", vec!["vol-b"]),
        ("red-key", vec!["vol-red"]),
        ("admin-key", vec!["vol-a", "vol-b", "vol-old", "vol-red"]),
    ] {
        let rows = plane.json("/volumes", key).await;
        assert_eq!(column(&rows, "volumeID"), set(&expected), "{key}");
    }
    // The node enforces ownership of a volume by its ID, from the team it is
    // told: each principal's calls carry its own scope, never the other's.
    for (key, path) in [
        ("alice-key", "/volumes/vol-b"),
        ("bob-key", "/volumes/vol-a"),
    ] {
        plane.call(Method::DELETE, path, key).send().await.unwrap();
    }
    let calls = seen.lock().clone();
    assert_eq!(
        calls
            .iter()
            .map(|(_, path, team, _)| (path.clone(), team.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("/volumes/vol-b".to_owned(), vec![a.as_str().to_owned()]),
            ("/volumes/vol-a".to_owned(), vec![b.as_str().to_owned()]),
        ]
    );
}

#[tokio::test]
async fn snapshots_and_templates_belong_to_their_principal() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let a = me("alice");
    let b = me("bob");
    let seen: Seen = Arc::default();
    let snapshots = json!([
        {"snapshotID":"snap-a","teamID":a.as_str()},
        {"snapshotID":"snap-b","teamID":b.as_str()},
        {"snapshotID":"snap-red","teamID":"red"},
        {"snapshotID":"snap-old"},
        {"snapshotID":"snap-shared","teamID":b.as_str(),"sharedWith":[a.as_str()]},
    ]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let node_seen = seen.clone();
    let _node = Owned(tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(move |method: Method, uri: axum::http::Uri| {
                let seen = node_seen.clone();
                let snapshots = snapshots.clone();
                async move {
                    if method == Method::GET && uri.path() == "/snapshots" {
                        return (StatusCode::OK, Json(snapshots));
                    }
                    seen.lock()
                        .push((method.to_string(), uri.path().to_owned(), vec![], None));
                    (StatusCode::NO_CONTENT, Json(json!({})))
                }
            }),
        )
        .await
        .unwrap();
    }));
    let mut info = node("snap-node", address);
    let meta = |team: Option<TeamId>, shared: Vec<TeamId>| TemplateInfo {
        snapshot: true,
        cpu_count: 1,
        memory_mb: 512,
        team,
        shared_with: shared,
    };
    info.templates = vec![
        "base".into(),
        "snap-a".into(),
        "snap-b".into(),
        "snap-red".into(),
        "snap-old".into(),
    ];
    info.template_metadata = [
        ("snap-a".to_owned(), meta(Some(a.clone()), vec![])),
        ("snap-b".to_owned(), meta(Some(b.clone()), vec![])),
        (
            "snap-red".to_owned(),
            meta(Some(TeamId::parse("red").unwrap()), vec![]),
        ),
        ("snap-old".to_owned(), meta(None, vec![])),
    ]
    .into();
    store
        .put_node(&info, Duration::from_secs(60))
        .await
        .unwrap();
    let plane = Plane::new(store).await;

    // Snapshots: its own, those shared with it, and the operator's (no
    // owner), as for a team. Never another principal's.
    for (key, expected) in [
        ("alice-key", vec!["snap-a", "snap-old", "snap-shared"]),
        ("bob-key", vec!["snap-b", "snap-old", "snap-shared"]),
        ("red-key", vec!["snap-old", "snap-red"]),
    ] {
        let rows = plane.json("/snapshots", key).await;
        assert_eq!(column(&rows, "snapshotID"), set(&expected), "{key}");
    }
    let rows = plane.json("/snapshots", "admin-key").await;
    assert_eq!(rows.as_array().unwrap().len(), 5);
    // Templates, the same.
    for (key, expected) in [
        ("alice-key", vec!["base", "snap-a", "snap-old"]),
        ("bob-key", vec!["base", "snap-b", "snap-old"]),
    ] {
        let rows = plane.json("/templates", key).await;
        assert_eq!(column(&rows, "templateID"), set(&expected), "{key}");
    }
    // Shares: read and written by the owning principal only.
    assert_eq!(
        plane
            .status(Method::GET, "/templates/snap-a/shares", "alice-key")
            .await,
        200
    );
    for (method, path) in [
        (Method::GET, "/templates/snap-b/shares"),
        (Method::PUT, "/templates/snap-b/shares/red"),
        (Method::DELETE, "/templates/snap-b/shares/red"),
        (Method::GET, "/templates/snap-old/shares"),
        (Method::PUT, "/templates/snap-old/shares/red"),
        (Method::DELETE, "/templates/snap-b"),
        // An operator's template every principal uses is not its to delete.
        (Method::DELETE, "/templates/snap-old"),
        (Method::DELETE, "/templates/snap-red"),
    ] {
        assert_eq!(
            plane.status(method.clone(), path, "alice-key").await,
            404,
            "{method} {path}"
        );
    }
    assert!(seen.lock().is_empty(), "nothing reached a node");
    assert_eq!(
        plane
            .status(Method::DELETE, "/templates/snap-a", "alice-key")
            .await,
        204
    );
    assert_eq!(seen.lock().len(), 1);
}

#[tokio::test]
async fn telemetry_destinations_are_a_principals_own() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let plane = Plane::new(store).await;
    plane
        .control
        .allow_team_telemetry(vec!["https://otel.example/".into()]);
    for (key, endpoint) in [
        ("alice-key", "https://otel.example/alice"),
        ("bob-key", "https://otel.example/bob"),
        ("red-key", "https://otel.example/red"),
    ] {
        let status = plane
            .call(Method::PUT, "/telemetry", key)
            .json(&json!({"endpoint": endpoint}))
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, 201, "{key}");
    }
    for (key, endpoint) in [
        ("alice-key", "https://otel.example/alice"),
        ("bob-key", "https://otel.example/bob"),
        ("red-key", "https://otel.example/red"),
    ] {
        let seen = plane.json("/telemetry", key).await;
        assert_eq!(seen["endpoint"], endpoint, "{key}");
    }
    // Nobody's destination is the administrator's, which is its own.
    assert_eq!(
        plane.status(Method::GET, "/telemetry", "admin-key").await,
        404
    );
    // Deleting is of one's own.
    assert_eq!(
        plane
            .status(Method::DELETE, "/telemetry", "alice-key")
            .await,
        204
    );
    assert_eq!(
        plane.status(Method::GET, "/telemetry", "alice-key").await,
        404
    );
    assert_eq!(
        plane.status(Method::GET, "/telemetry", "bob-key").await,
        200
    );
}
