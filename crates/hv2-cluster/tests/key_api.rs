//! Managed API keys over real HTTP: `POST /keys`, `GET /keys`,
//! `DELETE /keys/{id}` and `GET /teams/{team}/members`, and the
//! authentication of the keys they make.
//!
//! The store is a `MemoryStore` behind a wrapper that can be told to fail,
//! which is how the fail-closed paths are reached.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, StatusCode};
use axum::routing::any;
use axum::{Json, Router};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::domains::{DomainBinding, DomainName};
use hv2_cluster::keys::{ApiKeyPolicy, ApiRole, ApiScope, ManagedKey};
use hv2_cluster::model::{ClusterEvent, Delivery, NodeInfo, SandboxRecord, Webhook};
use hv2_cluster::names::{NameReservation, SandboxName};
use hv2_cluster::store::{
    ClusterStore, DomainClaim, MemoryStore, Result as StoreResult, StoreError,
};

const TOKEN: &str = "cluster-secret";
const ADMIN: &str = "legacy-admin-fixture";

/// A `MemoryStore` whose managed-key calls can be made to fail.
#[derive(Default)]
struct Flaky {
    inner: MemoryStore,
    fail_lookup: AtomicBool,
    fail_list: AtomicBool,
    fail_put: AtomicBool,
    lookups: std::sync::atomic::AtomicUsize,
}

impl Flaky {
    fn fault<T>() -> StoreResult<T> {
        Err(StoreError("injected fault".into()))
    }
}

#[async_trait::async_trait]
impl ClusterStore for Flaky {
    async fn put_node(&self, node: &NodeInfo, ttl: Duration) -> StoreResult<()> {
        self.inner.put_node(node, ttl).await
    }
    async fn nodes(&self) -> StoreResult<Vec<NodeInfo>> {
        self.inner.nodes().await
    }
    async fn node(&self, id: &str) -> StoreResult<Option<NodeInfo>> {
        self.inner.node(id).await
    }
    async fn remove_node(&self, id: &str) -> StoreResult<()> {
        self.inner.remove_node(id).await
    }
    async fn put_sandbox(&self, record: &SandboxRecord) -> StoreResult<()> {
        self.inner.put_sandbox(record).await
    }
    async fn adopt_sandbox_owner(
        &self,
        sandbox: &str,
        owner: &hv2_cluster::ownership::OwnerId,
    ) -> StoreResult<hv2_cluster::ownership::OwnerAdoption> {
        self.inner.adopt_sandbox_owner(sandbox, owner).await
    }
    async fn register_named_sandbox(
        &self,
        record: &SandboxRecord,
        reservation: &NameReservation,
    ) -> StoreResult<bool> {
        self.inner.register_named_sandbox(record, reservation).await
    }
    async fn sandbox(&self, id: &str) -> StoreResult<Option<SandboxRecord>> {
        self.inner.sandbox(id).await
    }
    async fn delete_sandbox(&self, id: &str) -> StoreResult<bool> {
        self.inner.delete_sandbox(id).await
    }
    async fn sandboxes(&self) -> StoreResult<Vec<SandboxRecord>> {
        self.inner.sandboxes().await
    }
    async fn reserve_name(&self, reservation: &NameReservation) -> StoreResult<bool> {
        self.inner.reserve_name(reservation).await
    }
    async fn name_reservation(&self, name: &SandboxName) -> StoreResult<Option<NameReservation>> {
        self.inner.name_reservation(name).await
    }
    async fn bind_name(&self, name: &SandboxName, token: &str, sandbox: &str) -> StoreResult<bool> {
        self.inner.bind_name(name, token, sandbox).await
    }
    async fn release_pending_name(&self, name: &SandboxName, token: &str) -> StoreResult<bool> {
        self.inner.release_pending_name(name, token).await
    }
    async fn claim_domain(&self, binding: &DomainBinding) -> StoreResult<DomainClaim> {
        self.inner.claim_domain(binding).await
    }
    async fn domain(&self, name: &DomainName) -> StoreResult<Option<DomainBinding>> {
        self.inner.domain(name).await
    }
    async fn domains(&self, sandbox: &str) -> StoreResult<Vec<DomainBinding>> {
        self.inner.domains(sandbox).await
    }
    async fn delete_domain(&self, name: &DomainName, sandbox: &str) -> StoreResult<bool> {
        self.inner.delete_domain(name, sandbox).await
    }
    async fn publish(&self, event: &ClusterEvent) -> StoreResult<()> {
        self.inner.publish(event).await
    }
    async fn events(&self, count: usize) -> StoreResult<Vec<ClusterEvent>> {
        self.inner.events(count).await
    }
    async fn put_webhook(&self, hook: &Webhook) -> StoreResult<()> {
        self.inner.put_webhook(hook).await
    }
    async fn webhooks(&self) -> StoreResult<Vec<Webhook>> {
        self.inner.webhooks().await
    }
    async fn delete_webhook(&self, id: &str) -> StoreResult<bool> {
        self.inner.delete_webhook(id).await
    }
    async fn record_delivery(&self, delivery: &Delivery) -> StoreResult<()> {
        self.inner.record_delivery(delivery).await
    }
    async fn deliveries(&self, webhook_id: &str, count: usize) -> StoreResult<Vec<Delivery>> {
        self.inner.deliveries(webhook_id, count).await
    }

    async fn put_managed_key(&self, key: &ManagedKey) -> StoreResult<()> {
        if self.fail_put.load(Ordering::SeqCst) {
            return Self::fault();
        }
        self.inner.put_managed_key(key).await
    }
    async fn managed_key_by_sha256(&self, sha256: &str) -> StoreResult<Option<ManagedKey>> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        if self.fail_lookup.load(Ordering::SeqCst) {
            return Self::fault();
        }
        self.inner.managed_key_by_sha256(sha256).await
    }
    async fn managed_keys(&self) -> StoreResult<Vec<ManagedKey>> {
        if self.fail_list.load(Ordering::SeqCst) {
            return Self::fault();
        }
        self.inner.managed_keys().await
    }
    async fn remove_managed_key(&self, id: &str) -> StoreResult<bool> {
        self.inner.remove_managed_key(id).await
    }
}

struct Owned(tokio::task::JoinHandle<()>);
impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn digest(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

struct World {
    base: String,
    store: Arc<Flaky>,
    control: Arc<ControlPlane>,
    _server: Owned,
}

/// The file's keys, by name: `file-admin` (admin scope), team-admins
/// `red-lead` (an hour), `red-short` (two minutes) and `blue-lead`, a red
/// `red-member`, `red-viewer` and `red-dev` (the developer preset).
fn file_policies() -> Value {
    let entry = |key: &str, extra: Value, exp: i64| {
        let mut entry = json!({"sha256": digest(key), "expires_at": now() + exp});
        entry
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        entry
    };
    json!([
        entry("file-admin", json!({"scopes": ["admin"]}), 3600),
        entry(
            "red-lead",
            json!({"scopes": ["sandboxes", "inventory"], "role": "team-admin",
                   "principal_id": "lead", "team_id": "red"}),
            3600
        ),
        entry(
            "red-short",
            json!({"scopes": ["sandboxes", "inventory"], "role": "team-admin",
                   "principal_id": "shorty", "team_id": "red"}),
            120
        ),
        entry(
            "blue-lead",
            json!({"scopes": ["sandboxes", "inventory"], "role": "team-admin",
                   "principal_id": "boss", "team_id": "blue"}),
            3600
        ),
        entry(
            "red-member",
            json!({"scopes": ["sandboxes", "inventory"], "role": "member",
                   "principal_id": "alice", "team_id": "red"}),
            3600
        ),
        entry(
            "red-viewer",
            json!({"scopes": ["inventory"], "role": "viewer",
                   "principal_id": "dash", "team_id": "red"}),
            3600
        ),
        entry(
            "red-dev",
            json!({"preset": "developer", "principal_id": "dev", "team_id": "red"}),
            3600
        ),
    ])
}

async fn world() -> World {
    world_on(Arc::new(Flaky::default()), file_policies()).await
}

async fn world_on(store: Arc<Flaky>, policies: Value) -> World {
    let control = ControlPlane::new(
        store.clone(),
        ControlConfig {
            api_key: Some(ADMIN.into()),
            api_keys: ApiKeyPolicy::from_json(&policies.to_string()).unwrap(),
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
    World {
        base,
        store,
        control,
        _server: server,
    }
}

async fn call(
    base: &str,
    method: reqwest::Method,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new().request(method, format!("{base}{path}"));
    if let Some(key) = key {
        request = request.header("x-api-key", key);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn post_key(base: &str, key: &str, body: Value) -> (u16, Value) {
    call(base, reqwest::Method::POST, "/keys", Some(key), Some(body)).await
}

async fn status(base: &str, method: reqwest::Method, path: &str, key: &str) -> u16 {
    call(base, method, path, Some(key), None).await.0
}

async fn listed(base: &str, key: &str) -> Vec<Value> {
    let (code, body) = call(base, reqwest::Method::GET, "/keys", Some(key), None).await;
    assert_eq!(code, 200, "{body}");
    body["keys"].as_array().unwrap().clone()
}

fn ids(keys: &[Value]) -> BTreeSet<String> {
    keys.iter()
        .map(|key| key["id"].as_str().unwrap().to_owned())
        .collect()
}

fn member_body(team: &str, principal: &str) -> Value {
    json!({"label": "ci", "expiresInSeconds": 3600, "preset": "developer",
           "teamId": team, "principalId": principal})
}

// ── Who may use the routes at all ───────────────────────────────────────────

/// Observers, operators, inventory keys and every other kind of non-admin key
/// get 403 on all four routes, and a missing or wrong key gets 401.
#[tokio::test]
async fn only_administrators_and_team_admins_reach_the_key_routes() {
    let w = world().await;
    let routes = [
        (
            reqwest::Method::POST,
            "/keys".to_string(),
            Some(member_body("red", "x")),
        ),
        (reqwest::Method::GET, "/keys".to_string(), None),
        (
            reqwest::Method::DELETE,
            "/keys/mk_anything".to_string(),
            None,
        ),
        (reqwest::Method::GET, "/teams/red/members".to_string(), None),
    ];
    for key in ["red-member", "red-viewer", "red-dev"] {
        for (method, path, body) in &routes {
            let (code, _) = call(&w.base, method.clone(), path, Some(key), body.clone()).await;
            assert_eq!(code, 403, "{key} {method} {path}");
        }
    }
    for (method, path, body) in &routes {
        for key in [None, Some("not-a-key")] {
            let (code, _) = call(&w.base, method.clone(), path, key, body.clone()).await;
            assert_eq!(code, 401, "{key:?} {method} {path}");
        }
    }
    // Both kinds of administrator, and a team-admin, are let in.
    for key in ["file-admin", ADMIN, "red-lead"] {
        assert_eq!(
            status(&w.base, reqwest::Method::GET, "/keys", key).await,
            200,
            "{key}"
        );
    }
    // A managed key that is only a member is no better off than a file member.
    let (code, made) = post_key(&w.base, "file-admin", member_body("red", "carol")).await;
    assert_eq!(code, 201, "{made}");
    let member = made["key"].as_str().unwrap().to_owned();
    for (method, path, body) in &routes {
        let (code, _) = call(&w.base, method.clone(), path, Some(&member), body.clone()).await;
        assert_eq!(code, 403, "managed member {method} {path}");
    }
}

/// With no key configured at all nobody is an administrator: the routes are
/// closed, and an unknown key does not break the open API.
#[tokio::test]
async fn an_open_deployment_has_no_key_administrator() {
    let control = ControlPlane::new(
        Arc::new(Flaky::default()),
        ControlConfig {
            api_key: None,
            api_keys: Vec::new(),
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
    for key in [None, Some("anything")] {
        let (code, _) = post_key_opt(&base, key, member_body("red", "x")).await;
        assert_eq!(code, 403, "{key:?}");
        let (code, _) = call(&base, reqwest::Method::GET, "/keys", key, None).await;
        assert_eq!(code, 403, "{key:?}");
    }
    let (code, _) = call(
        &base,
        reqwest::Method::GET,
        "/sandboxes",
        Some("anything"),
        None,
    )
    .await;
    assert_eq!(
        code, 200,
        "an unrecognised key is as unauthenticated as none"
    );
}

async fn post_key_opt(base: &str, key: Option<&str>, body: Value) -> (u16, Value) {
    call(base, reqwest::Method::POST, "/keys", key, Some(body)).await
}

// ── Making, listing and revoking ────────────────────────────────────────────

#[tokio::test]
async fn an_administrator_makes_lists_and_revokes_keys() {
    let w = world().await;
    let before = now();
    let (code, made) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    assert_eq!(code, 201, "{made}");
    let id = made["id"].as_str().unwrap().to_owned();
    let secret = made["key"].as_str().unwrap().to_owned();
    assert!(id.starts_with("mk_") && !secret.contains(&id), "{id}");
    assert!(secret.starts_with("hm_") && secret.len() >= 46, "{secret}");
    assert_eq!(made["label"], "ci");
    assert_eq!(made["teamId"], "red");
    assert_eq!(made["principalId"], "carol");
    assert_eq!(made["role"], "operator");
    let scopes: BTreeSet<&str> = made["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(
        scopes,
        BTreeSet::from(["sandboxes", "templates", "volumes", "events", "inventory"]),
        "the preset's scopes, expanded"
    );
    let (created, expires) = (
        made["createdAt"].as_i64().unwrap(),
        made["expiresAt"].as_i64().unwrap(),
    );
    assert!(created >= before && created <= now());
    assert_eq!(expires, created + 3600);

    // Two keys are two secrets and two ids.
    let (_, second) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    assert_ne!(second["key"], made["key"]);
    assert_ne!(second["id"], made["id"]);

    // The listing is metadata: never the secret, never a digest.
    let (code, body) = call(&w.base, reqwest::Method::GET, "/keys", Some(ADMIN), None).await;
    assert_eq!(code, 200);
    let text = body.to_string();
    assert!(!text.contains(&secret), "{text}");
    assert!(!text.contains(&digest(&secret)), "{text}");
    assert!(
        !text.contains("sha256") && !text.contains("\"key\""),
        "{text}"
    );
    let entry = body["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == id.as_str())
        .unwrap();
    assert_eq!(entry["createdBy"], "legacy-admin");
    assert_eq!(entry["teamId"], "red");
    assert_eq!(entry["expired"], false);
    assert_eq!(entry["expiresAt"], expires);

    // The secret is shown once, in the response to the creation, and the
    // store holds only its digest.
    let held = w.store.managed_keys().await.unwrap();
    assert_eq!(held.len(), 2);
    let record = held.iter().find(|k| k.id == id).unwrap();
    assert_eq!(record.sha256, digest(&secret));
    assert!(!format!("{record:?}").contains(&secret));
    assert!(!serde_json::to_string(record).unwrap().contains(&secret));

    // Revocation answers as documented; an unknown id is a 404.
    let (code, gone) = call(
        &w.base,
        reqwest::Method::DELETE,
        &format!("/keys/{id}"),
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!((code, gone), (200, json!({"deleted": id, "kind": "key"})));
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::DELETE,
            &format!("/keys/{id}"),
            ADMIN
        )
        .await,
        404
    );
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::DELETE,
            "/keys/mk_never_made",
            ADMIN
        )
        .await,
        404
    );
    assert_eq!(listed(&w.base, ADMIN).await.len(), 1);
}

/// The creation response says never to cache the secret.
#[tokio::test]
async fn the_secret_is_not_cacheable() {
    let w = world().await;
    let response = reqwest::Client::new()
        .post(format!("{}/keys", w.base))
        .header("x-api-key", ADMIN)
        .json(&member_body("red", "carol"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["cache-control"], "no-store");
}

/// A managed key acts under the same policy a file entry with its fields
/// would: its scopes gate routes, and its team and principal reach the node.
#[tokio::test]
async fn a_managed_key_behaves_like_a_file_policy() {
    use hv2_cluster::ownership::{OWNER_HEADER, TEAM_HEADER};
    let seen = Arc::new(parking_lot::Mutex::new(Vec::<(
        Option<String>,
        Option<String>,
    )>::new()));
    let calls = seen.clone();
    let node_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let node_address = node_listener.local_addr().unwrap();
    let _node = Owned(tokio::spawn(async move {
        axum::serve(
            node_listener,
            Router::new().fallback(any(move |headers: HeaderMap| {
                let calls = calls.clone();
                async move {
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .map(|value| value.to_str().unwrap().to_owned())
                    };
                    calls
                        .lock()
                        .push((header(TEAM_HEADER), header(OWNER_HEADER)));
                    (
                        StatusCode::CREATED,
                        Json(json!({"sandboxID": "from-managed"})),
                    )
                }
            })),
        )
        .await
        .unwrap();
    }));
    let w = world().await;
    w.store
        .put_node(
            &NodeInfo {
                id: "n".into(),
                api: format!("http://{node_address}"),
                proxy: node_address,
                capacity: 8,
                running: 0,
                heartbeat_ms: hv2_cluster::model::now_ms(),
                version: "fixture".into(),
                jwk: None,
                templates: vec!["base".into()],
                template_metadata: Default::default(),
            },
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    let (_, made) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    let key = made["key"].as_str().unwrap();
    let (code, created) = call(
        &w.base,
        reqwest::Method::POST,
        "/v2/sandboxes",
        Some(key),
        Some(json!({"templateID": "base"})),
    )
    .await;
    assert_eq!(code, 201, "{created}");
    assert_eq!(
        seen.lock().clone(),
        vec![(Some("red".to_owned()), Some("carol".to_owned()))],
        "the key's team and principal, from the store's record"
    );
    // Its scopes are the developer preset's, and nothing more.
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", key).await,
        200
    );
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/machines", key).await,
        403
    );
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/cluster/events", key).await,
        403
    );

    // A viewer key is an observer: it reads the inventory and nothing else.
    let (_, viewer) = post_key(
        &w.base,
        ADMIN,
        json!({"label": "dash", "expiresInSeconds": 3600, "scopes": ["inventory", "sandboxes"],
               "role": "viewer", "teamId": "red", "principalId": "dash-2"}),
    )
    .await;
    assert_eq!(viewer["role"], "observer");
    let viewer = viewer["key"].as_str().unwrap();
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", viewer).await,
        200
    );
    let (code, _) = call(
        &w.base,
        reqwest::Method::POST,
        "/v2/sandboxes",
        Some(viewer),
        Some(json!({"templateID": "base"})),
    )
    .await;
    assert_eq!(code, 403);
    assert_eq!(seen.lock().len(), 1);
}

/// An administrator key made through the API is an administrator, including
/// of the key routes, and needs no team.
#[tokio::test]
async fn a_managed_administrator_key_is_an_administrator() {
    let w = world().await;
    let (code, made) = post_key(
        &w.base,
        ADMIN,
        json!({"label": "root", "expiresInSeconds": 3600, "preset": "admin"}),
    )
    .await;
    assert_eq!(code, 201, "{made}");
    assert!(made["teamId"].is_null());
    let root = made["key"].as_str().unwrap();
    let (code, other) = post_key(&w.base, root, member_body("blue", "dana")).await;
    assert_eq!(code, 201, "{other}");
    assert_eq!(listed(&w.base, root).await.len(), 2);
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/cluster/events", root).await,
        200
    );
}

// ── Revocation, expiry, store failure ───────────────────────────────────────

/// A revoked key fails its next request with 401, on the control plane that
/// revoked it and on another one over the same store.
#[tokio::test]
async fn revoking_a_key_takes_effect_on_the_next_request_everywhere() {
    let w = world().await;
    let second = world_on(w.store.clone(), file_policies()).await;
    let (_, made) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    let (id, key) = (
        made["id"].as_str().unwrap().to_owned(),
        made["key"].as_str().unwrap().to_owned(),
    );
    for base in [&w.base, &second.base] {
        assert_eq!(
            status(base, reqwest::Method::GET, "/sandboxes", &key).await,
            200
        );
    }
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::DELETE,
            &format!("/keys/{id}"),
            "red-lead"
        )
        .await,
        200
    );
    for base in [&w.base, &second.base] {
        assert_eq!(
            status(base, reqwest::Method::GET, "/sandboxes", &key).await,
            401
        );
    }
}

#[tokio::test]
async fn an_expired_managed_key_is_refused_like_an_expired_file_key() {
    let w = world().await;
    let secret = format!("hm_{}", uuid::Uuid::new_v4().simple());
    let record = |id: &str, secret: &str, expires_at: i64| ManagedKey {
        id: id.into(),
        sha256: digest(secret),
        scopes: [ApiScope::Inventory].into(),
        role: ApiRole::Operator,
        team_id: Some(hv2_cluster::ownership::TeamId::parse("red").unwrap()),
        principal: Some(hv2_cluster::ownership::OwnerId::parse("old").unwrap()),
        label: "old".into(),
        created_by: "lead".into(),
        created_at: now() - 7200,
        expires_at,
    };
    w.store
        .put_managed_key(&record("mk_old", &secret, now() - 1))
        .await
        .unwrap();
    let live = format!("hm_{}", uuid::Uuid::new_v4().simple());
    w.store
        .put_managed_key(&record("mk_live", &live, now() + 600))
        .await
        .unwrap();
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", &live).await,
        200
    );
    let (code, body) = call(
        &w.base,
        reqwest::Method::GET,
        "/sandboxes",
        Some(&secret),
        None,
    )
    .await;
    assert_eq!(code, 401, "{body}");
    // The same words an expired file key gets.
    let file_expired = {
        let policies = json!([{"sha256": digest("old-file"), "expires_at": now() - 1,
                               "scopes": ["inventory"]}]);
        let w = world_on(Arc::new(Flaky::default()), policies).await;
        call(
            &w.base,
            reqwest::Method::GET,
            "/sandboxes",
            Some("old-file"),
            None,
        )
        .await
    };
    assert_eq!(file_expired.0, 401);
    assert_eq!(body["message"], file_expired.1["message"]);
    // And the listing says so.
    let all = listed(&w.base, ADMIN).await;
    let old = all.iter().find(|k| k["id"] == "mk_old").unwrap();
    assert_eq!(old["expired"], true);
}

/// A store that errors while authenticating a key fails closed with 503. It
/// is not a 401 (the key may be fine), and it never lets the request through.
#[tokio::test]
async fn a_store_error_while_authenticating_is_a_503_and_never_a_pass() {
    let w = world().await;
    let (_, made) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    let key = made["key"].as_str().unwrap().to_owned();
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", &key).await,
        200
    );
    w.store.fail_lookup.store(true, Ordering::SeqCst);
    for who in [key.as_str(), "some-key-nobody-made"] {
        let (code, body) = call(&w.base, reqwest::Method::GET, "/sandboxes", Some(who), None).await;
        assert_eq!(code, 503, "{body}");
        assert!(!body.to_string().contains("injected"), "{body}");
        let (code, _) = call(&w.base, reqwest::Method::GET, "/keys", Some(who), None).await;
        assert_eq!(code, 503);
    }
    // The policy file and the legacy key are answered without the store.
    for who in ["red-member", "file-admin", ADMIN] {
        assert_eq!(
            status(&w.base, reqwest::Method::GET, "/sandboxes", who).await,
            200,
            "{who}"
        );
    }
    // No key is no store call, and 401.
    let before = w.store.lookups.load(Ordering::SeqCst);
    assert_eq!(
        call(&w.base, reqwest::Method::GET, "/sandboxes", None, None)
            .await
            .0,
        401
    );
    assert_eq!(w.store.lookups.load(Ordering::SeqCst), before);
    // Recovery: the same key works again.
    w.store.fail_lookup.store(false, Ordering::SeqCst);
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", &key).await,
        200
    );
}

/// A store that cannot list or write answers 503 to the key routes, and a
/// failed write leaves no key.
#[tokio::test]
async fn the_key_routes_fail_closed_when_the_store_does() {
    let w = world().await;
    w.store.fail_put.store(true, Ordering::SeqCst);
    let (code, body) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    assert_eq!(code, 503, "{body}");
    assert!(!body.to_string().contains("injected") && body.get("key").is_none());
    w.store.fail_put.store(false, Ordering::SeqCst);
    assert!(w.store.managed_keys().await.unwrap().is_empty());
    let (_, made) = post_key(&w.base, ADMIN, member_body("red", "carol")).await;
    let id = made["id"].as_str().unwrap();
    w.store.fail_list.store(true, Ordering::SeqCst);
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/keys", ADMIN).await,
        503
    );
    assert_eq!(
        post_key(&w.base, ADMIN, member_body("red", "carol"))
            .await
            .0,
        503,
        "it cannot count what it holds"
    );
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::DELETE,
            &format!("/keys/{id}"),
            ADMIN
        )
        .await,
        503
    );
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/teams/red/members", ADMIN).await,
        503
    );
}

/// A policy-file key cannot be removed through the API: it has no id here,
/// and every id the store does not hold is a 404.
#[tokio::test]
async fn file_policy_keys_cannot_be_deleted_through_the_api() {
    let w = world().await;
    let policies = file_policies();
    for (index, who) in ["file-admin", "red-lead", "red-member"].iter().enumerate() {
        let sha = policies[index]["sha256"].as_str().unwrap();
        for id in [sha.to_owned(), sha[..16].to_owned(), (*who).to_owned()] {
            assert_eq!(
                status(
                    &w.base,
                    reqwest::Method::DELETE,
                    &format!("/keys/{id}"),
                    ADMIN
                )
                .await,
                404,
                "{id}"
            );
        }
        assert_eq!(
            status(&w.base, reqwest::Method::GET, "/sandboxes", who).await,
            200
        );
    }
    // Not even with a path that tries to be clever.
    for path in ["/keys/", "/keys/..%2fkeys", "/keys/a/b"] {
        let code = status(&w.base, reqwest::Method::DELETE, path, ADMIN).await;
        assert!(code == 404 || code == 405, "{path}: {code}");
    }
}

// ── Validation, shared with the policy file ─────────────────────────────────

#[tokio::test]
async fn requests_are_validated_by_the_policy_files_rules() {
    let w = world().await;
    let ok = |extra: Value| {
        let mut body = json!({"label": "x", "expiresInSeconds": 3600, "teamId": "red",
                              "principalId": "p"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let bad = [
        ("no scopes or preset", ok(json!({}))),
        ("empty scopes", ok(json!({"scopes": []}))),
        ("unknown scope", ok(json!({"scopes": ["root"]}))),
        ("unknown preset", ok(json!({"preset": "superuser"}))),
        ("null preset", ok(json!({"preset": null}))),
        (
            "null role",
            ok(json!({"scopes": ["inventory"], "role": null})),
        ),
        (
            "unknown role",
            ok(json!({"scopes": ["inventory"], "role": "god"})),
        ),
        (
            "read-only cannot be raised",
            ok(json!({"preset": "read-only", "role": "operator"})),
        ),
        (
            "read-only takes no more scopes",
            ok(json!({"preset": "read-only", "scopes": ["sandboxes"]})),
        ),
        (
            "team-admin cannot hold admin",
            ok(json!({"scopes": ["admin"], "role": "team-admin"})),
        ),
        (
            "unknown field",
            ok(json!({"scopes": ["inventory"], "sha256": "a".repeat(64)})),
        ),
        (
            "bad team",
            ok(json!({"scopes": ["inventory"], "teamId": "a b"})),
        ),
        (
            "bad principal",
            ok(json!({"scopes": ["inventory"], "principalId": "a/b"})),
        ),
        (
            "empty label",
            ok(json!({"scopes": ["inventory"], "label": ""})),
        ),
        (
            "control character in label",
            ok(json!({"scopes": ["inventory"], "label": "a\nb"})),
        ),
        (
            "long label",
            ok(json!({"scopes": ["inventory"], "label": "x".repeat(129)})),
        ),
        (
            "expiry too soon",
            ok(json!({"scopes": ["inventory"], "expiresInSeconds": 59})),
        ),
        (
            "expiry too late",
            ok(json!({"scopes": ["inventory"], "expiresInSeconds": 90 * 86400 + 1})),
        ),
        (
            "negative expiry",
            ok(json!({"scopes": ["inventory"], "expiresInSeconds": -5})),
        ),
        (
            "no team for a non-administrator",
            ok(json!({"scopes": ["inventory"], "teamId": null})),
        ),
    ];
    for (why, body) in bad {
        let (code, response) = post_key(&w.base, ADMIN, body).await;
        assert_eq!(code, 400, "{why}: {response}");
        assert!(response.get("key").is_none(), "{why}");
    }
    // Neither expiry nor label can be left out.
    for body in [
        json!({"label": "x", "scopes": ["inventory"], "teamId": "red"}),
        json!({"expiresInSeconds": 60, "scopes": ["inventory"], "teamId": "red"}),
    ] {
        assert_eq!(post_key(&w.base, ADMIN, body).await.0, 400);
    }
    // Not JSON at all.
    let code = reqwest::Client::new()
        .post(format!("{}/keys", w.base))
        .header("x-api-key", ADMIN)
        .body("nope")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(code, 400);
    // The limits of the range are accepted.
    for seconds in [60, 90 * 86400] {
        let (code, body) = post_key(
            &w.base,
            ADMIN,
            ok(json!({"scopes": ["inventory"], "expiresInSeconds": seconds})),
        )
        .await;
        assert_eq!(code, 201, "{body}");
    }
    // Nothing that was refused was kept.
    assert_eq!(w.store.managed_keys().await.unwrap().len(), 2);
}

#[tokio::test]
async fn the_longest_lifetime_is_configurable() {
    let w = world().await;
    w.control
        .set_managed_key_max_ttl(Duration::from_secs(600))
        .unwrap();
    let body = |seconds: u64| {
        json!({"label": "x", "expiresInSeconds": seconds, "scopes": ["inventory"],
               "teamId": "red", "principalId": "p"})
    };
    assert_eq!(post_key(&w.base, ADMIN, body(600)).await.0, 201);
    assert_eq!(post_key(&w.base, ADMIN, body(601)).await.0, 400);
    assert!(w
        .control
        .set_managed_key_max_ttl(Duration::from_secs(59))
        .is_err());
    assert!(w
        .control
        .set_managed_key_max_ttl(Duration::from_secs(3651 * 86400))
        .is_err());
}

#[tokio::test]
async fn the_number_of_live_keys_is_bounded() {
    let w = world().await;
    w.control.set_managed_key_limit(2);
    let body = member_body("red", "p");
    let (_, first) = post_key(&w.base, ADMIN, body.clone()).await;
    assert_eq!(post_key(&w.base, ADMIN, body.clone()).await.0, 201);
    assert_eq!(post_key(&w.base, ADMIN, body.clone()).await.0, 429);
    let id = first["id"].as_str().unwrap();
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::DELETE,
            &format!("/keys/{id}"),
            ADMIN
        )
        .await,
        200
    );
    assert_eq!(post_key(&w.base, ADMIN, body).await.0, 201);
}

// ── A team-admin may not escalate ───────────────────────────────────────────

/// One assertion per rule: a team-admin manages its own team, and nothing
/// that would raise a key above itself.
#[tokio::test]
async fn a_team_admin_cannot_escalate() {
    let w = world().await;
    let body = |extra: Value| {
        let mut body = json!({"label": "x", "expiresInSeconds": 600});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let lead = "red-lead";

    // Rule: teamId is its own team's. Another team's is refused...
    let (code, response) = post_key(
        &w.base,
        lead,
        body(json!({"scopes": ["sandboxes"], "teamId": "blue", "principalId": "p"})),
    )
    .await;
    assert_eq!(code, 403, "{response}");
    // ...and nothing was made for it.
    assert!(w.store.managed_keys().await.unwrap().is_empty());
    // Omitted, it is forced to its own; stated, it must equal it.
    let (code, forced) = post_key(&w.base, lead, body(json!({"scopes": ["sandboxes"]}))).await;
    assert_eq!(code, 201, "{forced}");
    assert_eq!(forced["teamId"], "red");
    assert!(
        forced["principalId"].as_str().unwrap().starts_with("mk_"),
        "a team's key always has a principal"
    );
    let (code, same) = post_key(
        &w.base,
        lead,
        body(json!({"scopes": ["sandboxes"], "teamId": "red", "principalId": "eve"})),
    )
    .await;
    assert_eq!(code, 201, "{same}");
    assert_eq!(same["principalId"], "eve");
    // Rule: a principal its own team uses is fine (a new key for a member)...
    let (code, again) = post_key(
        &w.base,
        lead,
        body(json!({"scopes": ["sandboxes"], "principalId": "alice"})),
    )
    .await;
    assert_eq!(code, 201, "{again}");
    // ...but never one another team's key carries (blue's "boss"), or an
    // administrator's: principals are global.
    let (code, response) = post_key(
        &w.base,
        lead,
        body(json!({"scopes": ["sandboxes"], "principalId": "boss"})),
    )
    .await;
    assert_eq!(code, 403, "{response}");
    let (code, admin_key) = post_key(
        &w.base,
        "file-admin",
        body(json!({"scopes": ["admin"], "principalId": "root-x"})),
    )
    .await;
    assert_eq!(code, 201, "{admin_key}");
    let (code, response) = post_key(
        &w.base,
        lead,
        body(json!({"scopes": ["sandboxes"], "principalId": "root-x"})),
    )
    .await;
    assert_eq!(code, 403, "{response}");

    // Rule: never a team-admin, by role or by preset.
    for extra in [
        json!({"scopes": ["sandboxes"], "role": "team-admin"}),
        json!({"preset": "developer", "role": "team-admin"}),
        json!({"scopes": ["sandboxes"], "role": "team-admin", "teamId": "red"}),
    ] {
        let (code, response) = post_key(&w.base, lead, body(extra.clone())).await;
        assert_eq!(code, 403, "{extra}: {response}");
    }
    // Rule: viewer/observer and member/operator are fine, under either name.
    for (name, role) in [
        ("viewer", "observer"),
        ("observer", "observer"),
        ("member", "operator"),
        ("operator", "operator"),
    ] {
        let (code, made) = post_key(
            &w.base,
            lead,
            body(json!({"scopes": ["inventory"], "role": name})),
        )
        .await;
        assert_eq!(code, 201, "{name}: {made}");
        assert_eq!(made["role"], role);
    }

    // Rule: no admin scope, however it is asked for.
    for extra in [
        json!({"scopes": ["admin"]}),
        json!({"preset": "admin"}),
        json!({"scopes": ["sandboxes", "admin"]}),
        json!({"preset": "developer", "scopes": ["admin"]}),
    ] {
        let (code, response) = post_key(&w.base, lead, body(extra.clone())).await;
        assert_eq!(code, 403, "{extra}: {response}");
    }

    // Rule: only scopes the team-admin itself holds (sandboxes and inventory).
    for extra in [
        json!({"scopes": ["templates"]}),
        json!({"scopes": ["sandboxes", "machines"]}),
        json!({"scopes": ["volumes"]}),
        json!({"scopes": ["events"]}),
        json!({"scopes": ["telemetry"]}),
        json!({"preset": "developer"}),
        json!({"preset": "machines"}),
    ] {
        let (code, response) = post_key(&w.base, lead, body(extra.clone())).await;
        assert_eq!(code, 403, "{extra}: {response}");
    }
    let (code, ok) = post_key(
        &w.base,
        lead,
        body(json!({"scopes": ["sandboxes", "inventory"]})),
    )
    .await;
    assert_eq!(code, 201, "{ok}");
    let (code, ok) = post_key(&w.base, lead, body(json!({"preset": "read-only"}))).await;
    assert_eq!(code, 201, "{ok}");

    // Rule: no key may outlive the team-admin's own (red-short has two minutes).
    let ttl = |seconds: u64| body(json!({"scopes": ["sandboxes"], "expiresInSeconds": seconds}));
    let (code, response) = post_key(&w.base, "red-short", ttl(3600)).await;
    assert_eq!(code, 403, "{response}");
    let (code, response) = post_key(&w.base, "red-short", ttl(600)).await;
    assert_eq!(code, 403, "{response}");
    let (code, response) = post_key(&w.base, "red-short", ttl(60)).await;
    assert_eq!(code, 201, "{response}");
    assert!(response["expiresAt"].as_i64().unwrap() <= now() + 120);
    // The same request from a team-admin with an hour is fine.
    assert_eq!(post_key(&w.base, lead, ttl(600)).await.0, 201);

    // Escalating does not partly succeed: count what was made.
    let made = w.store.managed_keys().await.unwrap();
    // (The administrator's key made for the principal rule above is not one of them.)
    assert!(made
        .iter()
        .filter(|k| k.principal.as_ref().map(|p| p.as_str()) != Some("root-x"))
        .all(|k| {
            k.team_id.as_ref().unwrap().as_str() == "red"
                && k.role != ApiRole::TeamAdmin
                && !k.scopes.contains(&ApiScope::Admin)
                && k.scopes
                    .is_subset(&[ApiScope::Sandboxes, ApiScope::Inventory].into())
        }));
}

/// A key a team-admin made cannot manage keys itself, whatever it asks.
#[tokio::test]
async fn keys_a_team_admin_makes_are_not_team_admins() {
    let w = world().await;
    let (_, made) = post_key(
        &w.base,
        "red-lead",
        json!({"label": "x", "expiresInSeconds": 600, "scopes": ["sandboxes", "inventory"]}),
    )
    .await;
    let child = made["key"].as_str().unwrap();
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/keys", child).await,
        403
    );
    assert_eq!(
        post_key(&w.base, child, member_body("red", "x")).await.0,
        403
    );
}

/// An administrator may make a team-admin (with a team), and that key is
/// held to the same rules as a file team-admin, including its own expiry.
#[tokio::test]
async fn a_managed_team_admin_is_bound_like_a_file_one() {
    let w = world().await;
    let (code, made) = post_key(
        &w.base,
        ADMIN,
        json!({"label": "lead-2", "expiresInSeconds": 120, "scopes": ["sandboxes", "inventory"],
               "role": "team-admin", "teamId": "red", "principalId": "lead-2"}),
    )
    .await;
    assert_eq!(code, 201, "{made}");
    assert_eq!(made["role"], "team-admin");
    let lead2 = made["key"].as_str().unwrap();
    let (code, child) = post_key(
        &w.base,
        lead2,
        json!({"label": "c", "expiresInSeconds": 600, "scopes": ["sandboxes"]}),
    )
    .await;
    assert_eq!(code, 403, "{child}");
    let (code, child) = post_key(
        &w.base,
        lead2,
        json!({"label": "c", "expiresInSeconds": 60, "scopes": ["sandboxes"]}),
    )
    .await;
    assert_eq!(code, 201, "{child}");
    assert_eq!(child["teamId"], "red");
    // It cannot reach another team, either.
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/teams/blue/members", lead2).await,
        403
    );
}

/// A team-admin lists and deletes its own team's keys, and another team's
/// (or an administrator's) do not exist to it.
#[tokio::test]
async fn a_team_admin_sees_and_revokes_only_its_teams_keys() {
    let w = world().await;
    let mut red = Vec::new();
    for principal in ["a", "b"] {
        let (_, made) = post_key(&w.base, ADMIN, member_body("red", principal)).await;
        red.push((
            made["id"].as_str().unwrap().to_owned(),
            made["key"].as_str().unwrap().to_owned(),
        ));
    }
    let (_, blue) = post_key(&w.base, ADMIN, member_body("blue", "c")).await;
    let (blue_id, blue_key) = (
        blue["id"].as_str().unwrap().to_owned(),
        blue["key"].as_str().unwrap().to_owned(),
    );
    let (_, root) = post_key(
        &w.base,
        ADMIN,
        json!({"label": "root", "expiresInSeconds": 3600, "preset": "admin"}),
    )
    .await;
    let root_id = root["id"].as_str().unwrap().to_owned();
    // An administrator's key that names the team is still not the team's.
    let (code, red_root) = post_key(
        &w.base,
        ADMIN,
        json!({"label": "red-root", "expiresInSeconds": 3600, "preset": "admin", "teamId": "red"}),
    )
    .await;
    assert_eq!(code, 201, "{red_root}");
    let red_root_id = red_root["id"].as_str().unwrap().to_owned();
    // Another team's team-admin made one more.
    let (_, blue_made) = post_key(
        &w.base,
        "blue-lead",
        json!({"label": "b", "expiresInSeconds": 600, "scopes": ["sandboxes"]}),
    )
    .await;
    let blue_made_id = blue_made["id"].as_str().unwrap().to_owned();

    // The listing: red's two member keys, nothing else.
    let seen = ids(&listed(&w.base, "red-lead").await);
    assert_eq!(seen, red.iter().map(|(id, _)| id.clone()).collect());
    assert_eq!(
        ids(&listed(&w.base, "blue-lead").await),
        [blue_id.clone(), blue_made_id.clone()]
            .into_iter()
            .collect()
    );
    assert_eq!(ids(&listed(&w.base, ADMIN).await).len(), 6);

    // Another team's key, an unrestricted one, and an administrator's key
    // that names the team: all 404, and all still work afterwards.
    for id in [&blue_id, &root_id, &red_root_id, &blue_made_id] {
        assert_eq!(
            status(
                &w.base,
                reqwest::Method::DELETE,
                &format!("/keys/{id}"),
                "red-lead"
            )
            .await,
            404,
            "{id}"
        );
    }
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", &blue_key).await,
        200
    );
    assert_eq!(ids(&listed(&w.base, ADMIN).await).len(), 6);

    // Its own: gone at once.
    let (id, key) = &red[0];
    let (code, body) = call(
        &w.base,
        reqwest::Method::DELETE,
        &format!("/keys/{id}"),
        Some("red-lead"),
        None,
    )
    .await;
    assert_eq!((code, body), (200, json!({"deleted": id, "kind": "key"})));
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", key).await,
        401
    );
    assert_eq!(listed(&w.base, "red-lead").await.len(), 1);
}

/// A key a team-admin made is the team's: it keeps working after the key
/// that made it is revoked from the policy file.
#[tokio::test]
async fn a_key_outlives_the_team_admin_that_made_it() {
    let w = world().await;
    let (_, made) = post_key(
        &w.base,
        "red-lead",
        json!({"label": "x", "expiresInSeconds": 600, "scopes": ["sandboxes", "inventory"]}),
    )
    .await;
    let key = made["key"].as_str().unwrap();
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", key).await,
        200
    );
    // Rotate the file without red-lead.
    let mut policies = file_policies();
    policies.as_array_mut().unwrap().remove(1);
    w.control
        .replace_api_key_policies(&policies.to_string())
        .unwrap();
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/keys", "red-lead").await,
        401
    );
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/sandboxes", key).await,
        200
    );
    // Its record still says who made it.
    let all = w.store.managed_keys().await.unwrap();
    assert_eq!(all[0].created_by, "lead");
}

/// An administrator manages every team's keys.
#[tokio::test]
async fn an_administrator_manages_any_teams_keys() {
    let w = world().await;
    let (_, red) = post_key(&w.base, "file-admin", member_body("red", "r")).await;
    let (_, blue) = post_key(&w.base, ADMIN, member_body("blue", "b")).await;
    let (_, brand_new) = post_key(&w.base, "file-admin", member_body("green", "g")).await;
    assert_eq!(brand_new["teamId"], "green");
    for admin in ["file-admin", ADMIN] {
        assert_eq!(listed(&w.base, admin).await.len(), 3, "{admin}");
    }
    let by = listed(&w.base, ADMIN).await;
    let creators: BTreeSet<_> = by
        .iter()
        .map(|k| k["createdBy"].as_str().unwrap().to_owned())
        .collect();
    assert!(creators.contains("legacy-admin"), "{creators:?}");
    assert!(
        creators.iter().any(|c| c.starts_with("scoped:")),
        "a file key without a principal is named by kind and key id: {creators:?}"
    );
    for made in [&red, &blue] {
        let id = made["id"].as_str().unwrap();
        assert_eq!(
            status(
                &w.base,
                reqwest::Method::DELETE,
                &format!("/keys/{id}"),
                "file-admin"
            )
            .await,
            200
        );
        assert_eq!(
            status(
                &w.base,
                reqwest::Method::GET,
                "/sandboxes",
                made["key"].as_str().unwrap()
            )
            .await,
            401
        );
    }
    assert_eq!(listed(&w.base, ADMIN).await.len(), 1);
}

// ── Members ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_member_listing_covers_file_and_managed_keys_of_one_team() {
    let w = world().await;
    // Managed: a second key for alice, a bot, one for blue, and one that expired.
    for (team, principal, role) in [
        ("red", "alice", "member"),
        ("red", "bot", "member"),
        ("blue", "carl", "viewer"),
    ] {
        let (code, made) = post_key(
            &w.base,
            ADMIN,
            json!({"label": "m", "expiresInSeconds": 3600, "scopes": ["inventory"],
                   "role": role, "teamId": team, "principalId": principal}),
        )
        .await;
        assert_eq!(code, 201, "{made}");
    }
    w.store
        .put_managed_key(&ManagedKey {
            id: "mk_gone".into(),
            sha256: digest("gone-secret"),
            scopes: [ApiScope::Inventory].into(),
            role: ApiRole::Operator,
            team_id: Some(hv2_cluster::ownership::TeamId::parse("red").unwrap()),
            principal: Some(hv2_cluster::ownership::OwnerId::parse("gone").unwrap()),
            label: "x".into(),
            created_by: "lead".into(),
            created_at: now() - 100,
            expires_at: now() - 1,
        })
        .await
        .unwrap();
    // A red administrator key: not a member of anything.
    post_key(
        &w.base,
        ADMIN,
        json!({"label": "r", "expiresInSeconds": 3600, "preset": "admin", "teamId": "red",
               "principalId": "root-red"}),
    )
    .await;

    let (code, body) = call(
        &w.base,
        reqwest::Method::GET,
        "/teams/red/members",
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["teamId"], "red");
    let members: Vec<(String, Vec<String>, u64)> = body["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["principalId"].as_str().unwrap().to_owned(),
                m["roles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_str().unwrap().to_owned())
                    .collect(),
                m["keyCount"].as_u64().unwrap(),
            )
        })
        .collect();
    let role = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        members,
        vec![
            // alice: the file's member key and a managed one.
            ("alice".to_owned(), role(&["operator"]), 2),
            ("bot".to_owned(), role(&["operator"]), 1),
            ("dash".to_owned(), role(&["observer"]), 1),
            ("dev".to_owned(), role(&["operator"]), 1),
            ("lead".to_owned(), role(&["team-admin"]), 1),
            ("shorty".to_owned(), role(&["team-admin"]), 1),
        ]
    );
    // No secret, digest, id or other team's name in it.
    let text = body.to_string();
    for forbidden in [
        "sha256", "mk_", "hm_", "carl", "boss", "blue", "root-red", "gone",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}: {text}");
    }
    let (_, blue) = call(
        &w.base,
        reqwest::Method::GET,
        "/teams/blue/members",
        Some(ADMIN),
        None,
    )
    .await;
    let names: Vec<_> = blue["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["principalId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["boss", "carl"]);
    // A team with nobody is an empty list, and a bad name is a 400.
    let (code, empty) = call(
        &w.base,
        reqwest::Method::GET,
        "/teams/nobody/members",
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!((code, empty["members"].clone()), (200, json!([])));
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/teams/a%20b/members", ADMIN).await,
        400
    );

    // A team-admin: its own team, and no other (403, not an empty list).
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::GET,
            "/teams/red/members",
            "red-lead"
        )
        .await,
        200
    );
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::GET,
            "/teams/blue/members",
            "red-lead"
        )
        .await,
        403
    );
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::GET,
            "/teams/nobody/members",
            "red-lead"
        )
        .await,
        403
    );
    assert_eq!(
        status(
            &w.base,
            reqwest::Method::GET,
            "/teams/red/members",
            "blue-lead"
        )
        .await,
        403
    );
}

// ── Events ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn creating_and_revoking_are_events_that_name_the_key_and_its_creator_only() {
    let w = world().await;
    let (_, made) = post_key(
        &w.base,
        "red-lead",
        json!({"label": "very-private-label",
        "expiresInSeconds": 600, "scopes": ["sandboxes"]}),
    )
    .await;
    let (id, key) = (made["id"].as_str().unwrap(), made["key"].as_str().unwrap());
    status(
        &w.base,
        reqwest::Method::DELETE,
        &format!("/keys/{id}"),
        ADMIN,
    )
    .await;
    let events = w.store.events(10).await.unwrap();
    let kinds: Vec<_> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["key-deleted", "key-created"]);
    assert_eq!(
        events[1].detail.as_deref(),
        Some(&*format!("id={id} by=lead"))
    );
    assert_eq!(
        events[0].detail.as_deref(),
        Some(&*format!("id={id} by=legacy-admin"))
    );
    let text = serde_json::to_string(&events).unwrap();
    assert!(
        !text.contains(key) && !text.contains(&digest(key)),
        "{text}"
    );
    assert!(!text.contains("very-private-label"), "{text}");
    // They are not sandbox events: webhooks and the E2B event feed ignore them.
    assert_eq!(
        status(&w.base, reqwest::Method::GET, "/events/sandboxes", ADMIN).await,
        200
    );
    let (_, feed) = call(
        &w.base,
        reqwest::Method::GET,
        "/events/sandboxes",
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(feed, json!([]));
}
