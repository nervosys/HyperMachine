//! `/environments` and `/usage` through the real control-plane router.
//!
//! A team key reaches both through its scopes (`sandboxes` for environments,
//! `events` for usage, both in the `developer` preset) and sees only its own
//! team's. An administrator sees usage of every team. A key without the
//! scope, and an observer, are refused with 403 before any handler runs.

use std::sync::Arc;
use std::time::Duration;

use axum::http::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::model::{now_ms, ClusterEvent};
use hv2_cluster::ownership::TeamId;
use hv2_cluster::store::{ClusterStore, MemoryStore};

const TOKEN: &str = "cluster-secret";

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn digest(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Keys are built at runtime, so no literal credential sits in the source.
struct Keys {
    red: String,
    blue: String,
    plain: String,
    observer: String,
    admin: String,
}

impl Keys {
    fn new() -> Self {
        let make = |label: &str| format!("{label}-{}", uuid::Uuid::new_v4());
        Self {
            red: make("red"),
            blue: make("blue"),
            plain: make("plain"),
            observer: make("observer"),
            admin: make("admin"),
        }
    }
}

struct Plane {
    base: String,
    keys: Keys,
    store: Arc<dyn ClusterStore>,
    _server: Owned,
}

impl Plane {
    async fn new() -> Self {
        let keys = Keys::new();
        let expires = chrono::Utc::now().timestamp() + 600;
        let policies = json!([
            // A team's developer: sandboxes, events (and the rest of the preset).
            {"sha256": digest(&keys.red), "expires_at": expires, "preset": "developer",
             "team_id": "red", "principal_id": "rita"},
            {"sha256": digest(&keys.blue), "expires_at": expires, "preset": "developer",
             "team_id": "blue", "principal_id": "bob"},
            // A team key without the sandboxes or events scope.
            {"sha256": digest(&keys.plain), "expires_at": expires,
             "scopes": ["machines", "inventory"], "role": "operator",
             "team_id": "red", "principal_id": "pat"},
            // A team's read-only key: the observer role refuses both routes.
            {"sha256": digest(&keys.observer), "expires_at": expires, "preset": "read-only",
             "team_id": "red", "principal_id": "ola"},
            {"sha256": digest(&keys.admin), "expires_at": expires,
             "scopes": ["admin"], "role": "operator"},
        ]);
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
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
        Self {
            base,
            keys,
            store,
            _server: server,
        }
    }

    fn request(&self, method: Method, path: &str, key: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .header("x-api-key", key)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        key: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = self.request(method, path, key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let body = response.json().await.unwrap_or(Value::Null);
        (status, body)
    }

    /// A usage event for `sandbox` of `team`, as a node publishes one: a
    /// running sandbox from a minute ago, still running now.
    async fn seed_usage(&self, sandbox: &str, team: &str) {
        let mut event = ClusterEvent::new("sandbox-created", "node-a", Some(sandbox));
        event.at_ms = now_ms() - 60_000;
        event.template_id = Some("base".into());
        event.team_id = Some(TeamId::parse(team).unwrap());
        event.data = Some(json!({"hm_usage": {
            "vcpuCount": 2, "memoryMB": 512, "running": true}}));
        self.store.publish(&event).await.unwrap();
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap().to_owned())
        .collect()
}

fn env_body(var: &str) -> Value {
    json!({"envVars": {var: format!("value-{}", uuid::Uuid::new_v4())},
           "files": [{"path": "/home/user/.npmrc", "content": "registry=x\n"}]})
}

/// Usage groups by sandbox key, for a window that covers every seeded event.
async fn usage_sandboxes(plane: &Plane, key: &str) -> (u16, Vec<String>) {
    let from = now_ms() - 3_600_000;
    let (status, body) = plane
        .send(
            Method::GET,
            &format!("/usage?from={from}&by=sandbox"),
            key,
            None,
        )
        .await;
    let groups = body["groups"]
        .as_array()
        .map(|groups| {
            groups
                .iter()
                .map(|g| g["key"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default();
    (status, groups)
}

#[tokio::test]
async fn a_team_key_manages_its_own_environments_and_no_others() {
    let plane = Plane::new().await;
    let red = plane.keys.red.clone();
    let blue = plane.keys.blue.clone();

    let (status, _) = plane
        .send(
            Method::PUT,
            "/environments/dev",
            &red,
            Some(env_body("RED_VAR")),
        )
        .await;
    assert_eq!(status, 201, "a team's developer makes an environment");
    let (status, _) = plane
        .send(
            Method::PUT,
            "/environments/dev",
            &blue,
            Some(env_body("BLUE_VAR")),
        )
        .await;
    assert_eq!(status, 201, "another team may hold one of the same name");
    let (status, _) = plane
        .send(
            Method::PUT,
            "/environments/red-only",
            &red,
            Some(env_body("RED_ONLY")),
        )
        .await;
    assert_eq!(status, 201);

    let (status, listed) = plane.send(Method::GET, "/environments", &red, None).await;
    assert_eq!(status, 200);
    assert_eq!(names(&listed), vec!["dev", "red-only"]);
    assert_eq!(listed[0]["envVarNames"], json!(["RED_VAR"]));

    let (status, one) = plane
        .send(Method::GET, "/environments/dev", &red, None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        one["envVarNames"],
        json!(["RED_VAR"]),
        "its own dev, not blue's"
    );

    // Blue sees only its own, and cannot read, replace or delete red's.
    let (_, listed) = plane.send(Method::GET, "/environments", &blue, None).await;
    assert_eq!(names(&listed), vec!["dev"]);
    assert_eq!(listed[0]["envVarNames"], json!(["BLUE_VAR"]));
    let (status, _) = plane
        .send(Method::GET, "/environments/red-only", &blue, None)
        .await;
    assert_eq!(status, 404, "another team's environment is not found");
    let (status, _) = plane
        .send(Method::DELETE, "/environments/red-only", &blue, None)
        .await;
    assert_eq!(status, 404);
    let (status, _) = plane
        .send(Method::GET, "/environments/red-only", &red, None)
        .await;
    assert_eq!(status, 200, "red's environment survives blue's delete");

    // An administrator has no team: it lists the environments no team holds.
    let (status, listed) = plane
        .send(Method::GET, "/environments", &plane.keys.admin, None)
        .await;
    assert_eq!(status, 200);
    assert!(
        names(&listed).is_empty(),
        "no team's environment is an administrator's"
    );
}

#[tokio::test]
async fn a_team_key_reads_only_its_own_teams_usage() {
    let plane = Plane::new().await;
    plane.seed_usage("red-sbx", "red").await;
    plane.seed_usage("blue-sbx", "blue").await;

    let (status, groups) = usage_sandboxes(&plane, &plane.keys.red).await;
    assert_eq!(status, 200, "a team's developer reads usage");
    assert_eq!(groups, vec!["red-sbx"]);

    let (status, groups) = usage_sandboxes(&plane, &plane.keys.blue).await;
    assert_eq!(status, 200);
    assert_eq!(groups, vec!["blue-sbx"]);

    let (status, groups) = usage_sandboxes(&plane, &plane.keys.admin).await;
    assert_eq!(status, 200);
    assert_eq!(
        groups,
        vec!["blue-sbx", "red-sbx"],
        "an administrator sees every team"
    );
}

#[tokio::test]
async fn a_key_without_the_scope_or_the_observer_role_is_refused() {
    let plane = Plane::new().await;
    for key in [&plane.keys.plain, &plane.keys.observer] {
        let (status, _) = plane.send(Method::GET, "/environments", key, None).await;
        assert_eq!(status, 403, "list environments");
        let (status, _) = plane
            .send(Method::GET, "/environments/dev", key, None)
            .await;
        assert_eq!(status, 403, "read an environment");
        let (status, _) = plane
            .send(Method::PUT, "/environments/dev", key, Some(env_body("X")))
            .await;
        assert_eq!(status, 403, "write an environment");
        let (status, _) = plane.send(Method::GET, "/usage", key, None).await;
        assert_eq!(status, 403, "read usage");
    }
}
