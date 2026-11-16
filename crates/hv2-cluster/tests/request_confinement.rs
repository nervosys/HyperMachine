//! What a control plane will and will not repeat to a node on a client's
//! behalf: a client's text goes into a node URL only as the one path segment
//! it is, a session credential never goes at all, and the node inventory a
//! key may read is the inventory it may see.
//!
//! The nodes here are fakes that record every request they receive, so each
//! test asserts what *reached* a node, which is the thing that matters: a node
//! trusts the control plane's cluster token.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, Method, StatusCode};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::model::{now_ms, NodeInfo, TemplateInfo};
use hv2_cluster::ownership::TeamId;
use hv2_cluster::store::{ClusterStore, MemoryStore};

const TOKEN: &str = "cluster-secret";

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// One request a node received: method, request target, and whether it
/// carried an `Authorization` header (with its value).
type Seen = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

fn digest(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct World {
    base: String,
    seen: Seen,
    _node: Owned,
    _server: Owned,
}

fn team(name: &str) -> TeamId {
    TeamId::parse(name).unwrap()
}

/// A control plane over one node, which holds red's snapshot `snap`, blue's
/// snapshot `blue-private`, and the operator's `base`.
async fn world() -> World {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let seen: Seen = Arc::default();
    let record = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let node = Owned(tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(
                move |method: Method, uri: axum::http::Uri, headers: HeaderMap| {
                    let record = record.clone();
                    async move {
                        record.lock().push((
                            method.to_string(),
                            uri.to_string(),
                            headers
                                .get("authorization")
                                .map(|v| v.to_str().unwrap().to_owned()),
                        ));
                        (StatusCode::OK, Json(json!({})))
                    }
                },
            ),
        )
        .await
        .unwrap();
    }));
    let snapshot = |owner: &str| TemplateInfo {
        snapshot: true,
        cpu_count: 1,
        memory_mb: 512,
        team: Some(team(owner)),
        shared_with: Vec::new(),
    };
    store
        .put_node(
            &NodeInfo {
                id: "n1".into(),
                api: format!("http://{address}"),
                proxy: address,
                capacity: 8,
                running: 2,
                heartbeat_ms: now_ms(),
                version: "fixture".into(),
                jwk: Some(json!({"kty": "EC", "kid": "node-signing-key"})),
                templates: vec!["base".into(), "snap".into(), "blue-private".into()],
                template_metadata: [
                    ("snap".to_owned(), snapshot("red")),
                    ("blue-private".to_owned(), snapshot("blue")),
                ]
                .into(),
            },
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    let expires = chrono::Utc::now().timestamp() + 600;
    let policies = json!([
        {"sha256": digest("red-key"), "expires_at": expires,
         "scopes": ["templates", "volumes", "inventory", "sandboxes"],
         "principal_id": "alice", "team_id": "red"},
        {"sha256": digest("admin-key"), "expires_at": expires, "scopes": ["admin"]},
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
    let server = Owned(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    World {
        base,
        seen,
        _node: node,
        _server: server,
    }
}

impl World {
    /// Send `request_target` (as written, so percent-encoding and dot
    /// segments survive) over a raw connection: a client library would
    /// tidy the very thing under test.
    async fn raw(&self, method: &str, request_target: &str, key: &str, extra: &str) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let address = self.base.trim_start_matches("http://");
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "{method} {request_target} HTTP/1.1\r\nHost: {address}\r\nx-api-key: {key}\r\n\
             {extra}Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).await.unwrap();
        let answer = String::from_utf8_lossy(&answer);
        answer
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status in {answer:?}"))
    }

    fn taken(&self) -> Vec<(String, String, Option<String>)> {
        std::mem::take(&mut *self.seen.lock())
    }
}

/// `DELETE /templates/{own}/shares/{with}` is repeated to nodes as
/// `/templates/{own}/shares/{with}`, with `with` a team and nothing else. A
/// `with` that decodes to `x/../../../../sandboxes/victim` -- which a URL
/// parser resolves to another route -- reached `DELETE /sandboxes/victim` on
/// the node, with the cluster token, for any key that could share a snapshot.
#[tokio::test]
async fn a_share_target_cannot_climb_out_of_its_route() {
    let w = world().await;
    let traversal = "x%2F..%2F..%2F..%2F..%2Fsandboxes%2Fvictim";
    for method in ["DELETE", "PUT"] {
        for with in [
            traversal,
            "..",
            ".",
            "%2E%2E",
            "a%2Fb",
            "a%5Cb",
            "a%3Fb%3Dc",
            "a%23b",
            "a%00b",
        ] {
            let target = format!("/templates/snap/shares/{with}");
            let status = w.raw(method, &target, "red-key", "").await;
            assert_eq!(status, 400, "{method} {target}");
        }
        // An administrator is held to the same: it is the URL, not the key.
        let status = w
            .raw(
                method,
                &format!("/templates/snap/shares/{traversal}"),
                "admin-key",
                "",
            )
            .await;
        assert_eq!(status, 400, "admin {method}");
        // Nor can the snapshot name be a path.
        for name in ["..", "%2e%2e", "a%2F.."] {
            let target = format!("/templates/{name}/shares/blue");
            let status = w.raw(method, &target, "admin-key", "").await;
            assert!(
                status == 400 || status == 404,
                "admin {method} {target}: {status}"
            );
        }
    }
    assert_eq!(
        w.taken(),
        Vec::new(),
        "nothing a client wrote as a target reached a node"
    );

    // The route as designed still works, and reaches the node as itself.
    assert_eq!(
        w.raw("PUT", "/templates/snap/shares/blue", "red-key", "")
            .await,
        200
    );
    assert_eq!(
        w.raw("DELETE", "/templates/snap/shares/blue", "red-key", "")
            .await,
        200
    );
    assert_eq!(
        w.taken(),
        vec![
            ("PUT".into(), "/templates/snap/shares/blue".into(), None),
            ("DELETE".into(), "/templates/snap/shares/blue".into(), None),
        ]
    );
    // Deleting a snapshot is one path segment too.
    assert_eq!(
        w.raw("DELETE", "/templates/x%2F..%2F..", "admin-key", "")
            .await,
        400
    );
    assert_eq!(w.taken(), Vec::new());
}

/// A path repeated to a node as the client wrote it must mean there what it
/// meant here: `.` and `..` segments in any spelling, and `\`, are resolved by
/// the HTTP client, so they are refused before they are sent.
#[tokio::test]
async fn a_path_repeated_verbatim_has_no_dot_segments() {
    let w = world().await;
    for target in [
        "/volumes/%2e%2e",
        "/volumes/%2E%2e",
        "/volumes/..",
        "/volumes/.",
        "/volumes/.%2e",
        "/sandboxes/%2e%2e/exec",
    ] {
        let status = w.raw("GET", target, "red-key", "").await;
        assert!(
            (400..500).contains(&status),
            "{target} was answered {status}"
        );
    }
    assert_eq!(w.taken(), Vec::new(), "nothing reached the node");
    // An ordinary volume is repeated as written.
    assert_eq!(w.raw("GET", "/volumes/vol-1", "red-key", "").await, 200);
    assert_eq!(
        w.taken(),
        vec![("GET".into(), "/volumes/vol-1".into(), None)]
    );
}

/// A node gets the `Authorization` it needs -- the volume's own bearer on the
/// content API -- and nothing else of the caller's. Above all it never gets a
/// signed-in member's session token, which it could replay here.
#[tokio::test]
async fn a_session_bearer_is_never_repeated_to_a_node() {
    let w = world().await;
    let sent = |token: &str| format!("Authorization: Bearer {token}\r\n");
    // The volume's token, to the volume content API: passed on.
    assert_eq!(
        w.raw(
            "GET",
            "/volumecontent/vol-1/file?path=a",
            "red-key",
            &sent("volume-token")
        )
        .await,
        200
    );
    // A session token, to the same: not.
    for token in ["hms1.eyJ.abc", "HMS1.x"] {
        let _ = w
            .raw(
                "GET",
                "/volumecontent/vol-1/file?path=a",
                "red-key",
                &sent(token),
            )
            .await;
    }
    // Any other route that relays: not, whatever it holds.
    assert_eq!(
        w.raw("GET", "/volumes/vol-1", "red-key", &sent("volume-token"))
            .await,
        200
    );
    assert_eq!(
        w.raw("GET", "/volumes/vol-2", "red-key", &sent("hms1.eyJ.abc"))
            .await,
        200
    );
    let seen = w.taken();
    assert_eq!(seen.len(), 5, "{seen:?}");
    for (method, target, authorization) in &seen {
        let leaked = authorization
            .as_deref()
            .is_some_and(|v| v.to_ascii_lowercase().contains("hms1."));
        assert!(!leaked, "{method} {target} carried a session token");
    }
    assert_eq!(
        seen[0],
        (
            "GET".into(),
            "/volumecontent/vol-1/file?path=a".into(),
            Some("Bearer volume-token".into())
        )
    );
    for (_, target, authorization) in &seen[1..] {
        assert!(
            authorization.is_none(),
            "{target}: only the volume content API takes the caller's bearer"
        );
    }
}

async fn get_json(w: &World, path: &str, key: &str) -> (u16, Value) {
    let response = reqwest::Client::new()
        .get(format!("{}{path}", w.base))
        .header("x-api-key", key)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// `GET /cluster/nodes`: an administrator sees nodes as they reported
/// themselves; a team key that may read the inventory sees load and the
/// templates it may start, and not where the nodes are, their signing keys, or
/// another team's snapshot names.
#[tokio::test]
async fn the_node_inventory_is_redacted_for_everyone_but_an_administrator() {
    let w = world().await;
    let (status, full) = get_json(&w, "/cluster/nodes", "admin-key").await;
    assert_eq!(status, 200);
    let node = &full[0];
    assert!(node["api"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    assert!(node["jwk"].is_object() && node["proxy"].is_string());
    assert!(node["templates"].to_string().contains("blue-private"));

    let (status, redacted) = get_json(&w, "/cluster/nodes", "red-key").await;
    assert_eq!(status, 200, "{redacted}");
    let node = &redacted[0];
    assert_eq!(node["id"], "n1");
    assert_eq!(
        (node["capacity"].as_u64(), node["running"].as_u64()),
        (Some(8), Some(2))
    );
    assert_eq!(node["templates"], json!(["base", "snap"]));
    let text = redacted.to_string();
    for secret in ["127.0.0.1", "node-signing-key", "blue-private"] {
        assert!(!text.contains(secret), "{secret} is in {text}");
    }
    assert!(node.get("api").is_none() && node.get("proxy").is_none());
    assert!(node.get("jwk").is_none() && node.get("template_metadata").is_none());
}
