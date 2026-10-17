//! Machines through the control plane, against several nodes that answer as
//! `hv2-sandboxd` does without booting anything: where a new machine is
//! placed, how an existing one is found, what each team sees, and what
//! happens while a node is not answering.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, Method, StatusCode};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::model::{now_ms, NodeInfo};
use hv2_cluster::ownership::TEAM_HEADER;
use hv2_cluster::store::{ClusterStore, MemoryStore};

const TOKEN: &str = "cluster-secret";

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// What a node was asked: its name, the method, the path, the teams sent.
type Calls = Arc<Mutex<Vec<(String, String, String, Vec<String>)>>>;

/// A node holding `machines`, recording every call but the inventory's.
async fn node(
    store: &Arc<dyn ClusterStore>,
    name: &'static str,
    running: u32,
    machines: Value,
    calls: &Calls,
) -> Owned {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = calls.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(
                move |method: Method, uri: axum::http::Uri, headers: HeaderMap| {
                    let calls = calls.clone();
                    let machines = machines.clone();
                    async move {
                        if method == Method::GET && uri.path() == "/machines" {
                            return (StatusCode::OK, Json(machines));
                        }
                        let teams = headers
                            .get_all(TEAM_HEADER)
                            .iter()
                            .map(|value| value.to_str().unwrap().to_owned())
                            .collect();
                        calls.lock().push((
                            name.to_owned(),
                            method.to_string(),
                            uri.path().to_owned(),
                            teams,
                        ));
                        let status = if method == Method::POST && uri.path() == "/machines" {
                            StatusCode::CREATED
                        } else {
                            StatusCode::OK
                        };
                        (status, Json(json!({ "servedBy": name })))
                    }
                },
            ),
        )
        .await
        .unwrap();
    });
    store
        .put_node(
            &NodeInfo {
                id: name.into(),
                api: format!("http://{address}"),
                proxy: address,
                capacity: 8,
                running,
                heartbeat_ms: now_ms(),
                version: "fixture".into(),
                jwk: None,
                templates: vec!["base".into()],
                template_metadata: Default::default(),
            },
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    Owned(server)
}

#[tokio::test]
async fn machines_are_placed_found_and_kept_to_their_team_across_nodes() {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    // `full` has one slot left and holds red's machine; `roomy` has seven
    // and holds blue's machine of the same name.
    let _full = node(
        &store,
        "full",
        7,
        json!([{"machineID":"vm-red","name":"web-01","teamID":"red"}]),
        &calls,
    )
    .await;
    let _roomy = node(
        &store,
        "roomy",
        1,
        json!([{"machineID":"vm-blue","name":"web-01","teamID":"blue"}]),
        &calls,
    )
    .await;
    let digest = |key: &str| {
        Sha256::digest(key.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let expires = chrono::Utc::now().timestamp() + 60;
    let policies = json!([
        {"sha256":digest("red-key"),"expires_at":expires,"scopes":["machines"],
         "principal_id":"alice","team_id":"red"},
        {"sha256":digest("admin-key"),"expires_at":expires,"scopes":["admin"]},
    ]);
    let control = ControlPlane::new(
        store.clone(),
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
    let router = control::router(control.clone());
    let _server = Owned(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let client = reqwest::Client::new();
    // Every request also claims to be blue: the control plane must not
    // forward a client's team.
    let send = |method: reqwest::Method, path: &str, key: &str, body: Option<Value>| {
        let mut request = client
            .request(method, format!("{base}{path}"))
            .header("x-api-key", key)
            .header(TEAM_HEADER, "blue");
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send()
    };
    let taken = |calls: &Calls| std::mem::take(&mut *calls.lock());

    // Each caller's list, with the node holding each machine.
    for (key, expected) in [
        ("red-key", vec![("vm-red", "full")]),
        ("admin-key", vec![("vm-blue", "roomy"), ("vm-red", "full")]),
    ] {
        let response = send(reqwest::Method::GET, "/machines", key, None)
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{key}");
        let rows: Value = response.json().await.unwrap();
        let mut listed: Vec<(String, String)> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["machineID"].as_str().unwrap().to_owned(),
                    row["nodeID"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        listed.sort();
        let expected: Vec<(String, String)> = expected
            .into_iter()
            .map(|(id, node)| (id.to_owned(), node.to_owned()))
            .collect();
        assert_eq!(listed, expected, "{key}");
    }
    assert!(taken(&calls).is_empty(), "a list asks only for inventories");

    // A team's machine by name goes to the node holding it, as that team.
    let response = send(
        reqwest::Method::POST,
        "/machines/web-01/exec",
        "red-key",
        Some(json!({"cmd":"true"})),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    // Another team's is not there, by name's ID or otherwise; an
    // administrator reaches it by ID, with no team, and has no `web-01`.
    for (key, path, status) in [
        ("red-key", "/machines/vm-blue", 404),
        ("red-key", "/machines/nothing", 404),
        ("admin-key", "/machines/web-01", 404),
        ("admin-key", "/machines/vm-blue", 200),
    ] {
        let response = send(reqwest::Method::GET, path, key, None).await.unwrap();
        assert_eq!(response.status(), status, "{key} {path}");
    }
    assert_eq!(
        taken(&calls),
        vec![
            (
                "full".to_owned(),
                "POST".to_owned(),
                "/machines/web-01/exec".to_owned(),
                vec!["red".to_owned()]
            ),
            (
                "roomy".to_owned(),
                "GET".to_owned(),
                "/machines/vm-blue".to_owned(),
                Vec::new()
            ),
        ]
    );

    // A new machine goes to the node with the most room, unless one is named;
    // a name the team already holds is refused before any node is asked.
    for (body, status, placed) in [
        (json!({"name":"new-01"}), 201, Some("roomy")),
        (json!({"name":"new-02","nodeID":"full"}), 201, Some("full")),
        (json!({"name":"web-01"}), 409, None),
        (json!({"name":"new-03","nodeID":"absent"}), 503, None),
        (json!({"name":"new-04","templateID":"unknown"}), 503, None),
    ] {
        let response = send(
            reqwest::Method::POST,
            "/machines",
            "red-key",
            Some(body.clone()),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), status, "{body}");
        let node = response
            .headers()
            .get("x-hv2-node")
            .map(|value| value.to_str().unwrap().to_owned());
        assert_eq!(node.as_deref(), placed, "{body}");
    }
    assert_eq!(
        taken(&calls),
        vec![
            (
                "roomy".to_owned(),
                "POST".to_owned(),
                "/machines".to_owned(),
                vec!["red".to_owned()]
            ),
            (
                "full".to_owned(),
                "POST".to_owned(),
                "/machines".to_owned(),
                vec!["red".to_owned()]
            ),
        ]
    );

    // A node that is alive in the store but not answering: a machine found
    // elsewhere is still served, one not found may be there (503, not 404),
    // and nothing is created, since its name may be taken there.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = closed.local_addr().unwrap();
    drop(closed);
    store
        .put_node(
            &NodeInfo {
                id: "silent".into(),
                api: format!("http://{silent}"),
                proxy: silent,
                capacity: 8,
                running: 0,
                heartbeat_ms: now_ms(),
                version: "fixture".into(),
                jwk: None,
                templates: vec!["base".into()],
                template_metadata: Default::default(),
            },
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    for (method, path, body, status) in [
        (reqwest::Method::GET, "/machines/web-01", None, 200),
        (reqwest::Method::GET, "/machines/nothing", None, 503),
        (
            reqwest::Method::POST,
            "/machines",
            Some(json!({"name":"new-05"})),
            503,
        ),
    ] {
        let response = send(method.clone(), path, "red-key", body).await.unwrap();
        assert_eq!(response.status(), status, "{method} {path}");
    }
    assert_eq!(
        taken(&calls),
        vec![(
            "full".to_owned(),
            "GET".to_owned(),
            "/machines/web-01".to_owned(),
            vec!["red".to_owned()]
        )]
    );
}

/// A machine's reverse port through the control plane: found as the
/// machine's other routes are, carried as the node's upgraded stream, and
/// sent to the node as the machine's team and no one else.
#[tokio::test]
async fn a_machines_reverse_port_is_carried_to_its_node_as_its_team() {
    use axum::extract::{Path, Request};
    use axum::response::IntoResponse;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    // What the node saw of each reverse call: the name, the teams and the
    // token sent.
    type Seen = Arc<Mutex<Vec<(String, Vec<String>, Option<String>)>>>;
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let machines = json!([
        {"machineID":"vm-0000000000000001","name":"web-01","teamID":"red"},
        {"machineID":"vm-0000000000000002","name":"web-01","teamID":"blue"},
    ]);
    let app = Router::new()
        .route(
            "/machines",
            axum::routing::get({
                let machines = machines.clone();
                move || {
                    let machines = machines.clone();
                    async move { Json(machines) }
                }
            }),
        )
        .route(
            "/machines/{name}/ports/{port}/reverse",
            axum::routing::get({
                let seen = seen.clone();
                move |Path((name, port)): Path<(String, u16)>, request: Request| {
                    let seen = seen.clone();
                    async move {
                        seen.lock().push((
                            name,
                            request
                                .headers()
                                .get_all(TEAM_HEADER)
                                .iter()
                                .map(|v| v.to_str().unwrap().to_owned())
                                .collect(),
                            request
                                .headers()
                                .get(hv2_cluster::control::CLUSTER_TOKEN_HEADER)
                                .map(|v| v.to_str().unwrap().to_owned()),
                        ));
                        hv2_api::tcp_tunnel::validate_reverse(&request).unwrap();
                        if port == 9001 {
                            return StatusCode::CONFLICT.into_response();
                        }
                        let (relay, mut service) = tokio::io::duplex(4096);
                        tokio::spawn(async move {
                            // As the node does: one byte when a program has
                            // connected, then the bytes both ways.
                            service.write_all(b"+").await.unwrap();
                            let mut bytes = Vec::new();
                            service.read_to_end(&mut bytes).await.unwrap();
                            service.write_all(&bytes).await.unwrap();
                            service.shutdown().await.unwrap();
                        });
                        hv2_api::tcp_tunnel::accept_reverse(request, relay)
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let _node = Owned(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    store
        .put_node(
            &NodeInfo {
                id: "only".into(),
                api: format!("http://{address}"),
                proxy: address,
                capacity: 8,
                running: 0,
                heartbeat_ms: now_ms(),
                version: "fixture".into(),
                jwk: None,
                templates: vec!["base".into()],
                template_metadata: Default::default(),
            },
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    let digest = |key: &str| {
        Sha256::digest(key.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let expires = chrono::Utc::now().timestamp() + 60;
    let policies = json!([
        {"sha256":digest("red-key"),"expires_at":expires,"scopes":["machines"],
         "principal_id":"alice","team_id":"red"},
        {"sha256":digest("sandbox-key"),"expires_at":expires,"scopes":["sandboxes"],
         "principal_id":"bob","team_id":"red"},
    ]);
    let control = ControlPlane::new(
        store.clone(),
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
    let router = control::router(control.clone());
    let _server = Owned(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let http = reqwest::Client::builder().http1_only().build().unwrap();
    // Every request also claims to be blue: the control plane must not
    // forward a client's team.
    let upgrade = |path: &str, key: &str| {
        http.get(format!("{base}{path}"))
            .header("x-api-key", key)
            .header(TEAM_HEADER, "blue")
            .header("connection", "upgrade")
            .header("upgrade", hv2_api::tcp_tunnel::REVERSE_PROTOCOL)
    };

    // Refused before any node is asked.
    for (path, key, status) in [
        ("/machines/web-01/ports/9000/reverse", "wrong", 401),
        ("/machines/web-01/ports/9000/reverse", "sandbox-key", 403),
        ("/machines/web-01/ports/0/reverse", "red-key", 400),
    ] {
        assert_eq!(
            upgrade(path, key).send().await.unwrap().status(),
            status,
            "{path} {key}"
        );
    }
    assert_eq!(
        http.get(format!("{base}/machines/web-01/ports/9000/reverse"))
            .header("x-api-key", "red-key")
            .send()
            .await
            .unwrap()
            .status(),
        400,
        "no upgrade asked for"
    );
    assert!(seen.lock().is_empty(), "{:?}", seen.lock());

    // Another team's machine, by its ID, and a machine that is not there.
    for path in [
        "/machines/vm-0000000000000002/ports/9000/reverse",
        "/machines/nothing/ports/9000/reverse",
    ] {
        assert_eq!(
            upgrade(path, "red-key").send().await.unwrap().status(),
            404,
            "{path}"
        );
    }
    assert!(seen.lock().is_empty(), "{:?}", seen.lock());

    // The team's own, by name: the node sees red and the cluster's token,
    // and the stream carries the node's first byte and then both ways.
    let response = upgrade("/machines/web-01/ports/9000/reverse", "red-key")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 101);
    let mut stream = response.upgrade().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut first = [0u8; 1];
        stream.read_exact(&mut first).await.unwrap();
        assert_eq!(&first, b"+");
        let bytes: Vec<u8> = (0..65536).map(|n| (n % 251) as u8).collect();
        stream.write_all(&bytes).await.unwrap();
        stream.shutdown().await.unwrap();
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, bytes);
    })
    .await
    .unwrap();
    assert_eq!(
        *seen.lock(),
        vec![(
            "web-01".to_owned(),
            vec!["red".to_owned()],
            Some(TOKEN.to_owned())
        )]
    );

    // A machine that is not running answers as the node does.
    assert_eq!(
        upgrade("/machines/web-01/ports/9001/reverse", "red-key")
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
}
