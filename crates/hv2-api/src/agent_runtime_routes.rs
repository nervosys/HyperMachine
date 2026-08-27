//! Agent Runtime REST API
//!
//! Operate the [`AgentRuntime`] over HTTP: spawn copy-on-write agents from a
//! warm baseline, list/release/reap them, and read fleet memory stats.
//!
//! | Method | Path                              | Description                 |
//! |--------|-----------------------------------|-----------------------------|
//! | POST   | `/api/v1/agents`                  | Spawn an agent              |
//! | GET    | `/api/v1/agents`                  | List the tenant's agents    |
//! | GET    | `/api/v1/agents/fleet`            | Fleet memory / count stats  |
//! | POST   | `/api/v1/agents/reap`             | Reap idle agents            |
//! | DELETE | `/api/v1/agents/{session_id}`     | Release an agent            |
//!
//! ## Tenancy
//!
//! Each request is scoped to the tenant in the `X-Tenant-Id` header (default
//! `"default"`). An agent is owned by the tenant that spawned it: `list` and
//! `fleet` only show that tenant's agents, and only the owning tenant may
//! release one.
//!
//! ## Auth
//!
//! If the state is built with auth tokens, every request must carry a matching
//! `Authorization: Bearer <token>`; otherwise the endpoints are open.
//!
//! ## Session record (`context.*`)
//!
//! The MCP server behind this runtime dispatches its `context.*` tools against
//! an installed host and refuses when there is none. A deployment gets one
//! here, from the environment, so that running the server is enough --
//! see [`ContextConfig`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use hv2_agent::context_host::LocalContextHost;
use hv2_agent::{AgentCapabilities, AgentRuntime};
use hv2_context::{Budget, SandboxRuntime};
use hv2_sandbox::ProcessSandbox;

/// Where a deployment keeps its session record, and where computation over it
/// is allowed to write.
///
/// Read from the environment by [`ContextConfig::from_env`], which is what
/// [`AgentRuntimeAppState::new`] uses:
///
/// | Variable                | Meaning                                                                  |
/// |-------------------------|--------------------------------------------------------------------------|
/// | `HV2_CONTEXT_ROOT`      | directory holding the append-only event log. Set it to the empty string to run with no record at all, in which case the `context.*` tools refuse. Defaults to `<platform data dir>/hypermachine/context`. |
/// | `HV2_CONTEXT_SESSION`   | session id the log is opened under. Defaults to `default`.               |
/// | `HV2_CONTEXT_WORKSPACE` | working directory for the confined runtime `context.exec` computes in. Defaults to `<root>/workspace`. |
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Root of the session record. `None` means keep no record.
    pub root: Option<PathBuf>,
    /// Session id the record is opened under.
    pub session: String,
    /// Workspace for the confined runtime; `None` means `<root>/workspace`.
    pub workspace: Option<PathBuf>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            root: Some(default_context_root()),
            session: "default".to_string(),
            workspace: None,
        }
    }
}

impl ContextConfig {
    /// Read the record's location from the `HV2_CONTEXT_*` environment.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(val) = std::env::var("HV2_CONTEXT_ROOT") {
            // An empty value is the opt-out, not a path: a deployment that
            // wants no record on disk should get tools that refuse, not a log
            // written to the process's working directory.
            config.root = if val.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(val))
            };
        }
        if let Ok(val) = std::env::var("HV2_CONTEXT_SESSION") {
            if !val.trim().is_empty() {
                config.session = val;
            }
        }
        if let Ok(val) = std::env::var("HV2_CONTEXT_WORKSPACE") {
            if !val.trim().is_empty() {
                config.workspace = Some(PathBuf::from(val));
            }
        }
        config
    }
}

/// Per-platform data directory for a record that has to outlive a reboot.
///
/// Deliberately not the temp directory: the whole arrangement depends on an
/// address meaning the same thing forever, and a record the OS may sweep
/// breaks that in a way no error ever reports.
fn default_context_root() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\ProgramData"))
    } else {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|_| PathBuf::from("/var/lib"))
    };
    base.join("hypermachine").join("context")
}

/// Install a session record on `runtime`'s MCP server, per `config`.
///
/// Failures are logged and leave the tools refusing rather than escalating to
/// a panic: an API server that cannot open its record is still able to serve
/// every other route, and a refusal tells the agent the truth.
fn install_context_host(runtime: &AgentRuntime, config: &ContextConfig) {
    let Some(root) = config.root.as_ref() else {
        tracing::info!("no session record configured (HV2_CONTEXT_ROOT is empty)");
        return;
    };

    let host = match LocalContextHost::open(root, config.session.clone(), Budget::default()) {
        Ok(host) => host,
        Err(e) => {
            tracing::error!(
                "session record at {} could not be opened ({e}); context.* tools will refuse",
                root.display()
            );
            return;
        }
    };

    // The runtime is built separately from the host rather than through
    // `with_sandbox_runtime`, which consumes the host on failure: an
    // unusable workspace must not also cost the deployment its record, since
    // search and record work perfectly well without anywhere to compute.
    let workspace = config
        .workspace
        .clone()
        .unwrap_or_else(|| root.join("workspace"));
    let host = match SandboxRuntime::new(Box::new(ProcessSandbox::new()), &workspace) {
        Ok(rt) => host.with_runtime(Box::new(rt)),
        Err(e) => {
            tracing::warn!(
                "confined runtime workspace {} is unusable ({e}); context.exec will refuse",
                workspace.display()
            );
            host
        }
    };

    runtime.server().set_context_host(Arc::new(host));
}

/// Shared state for the agent-runtime routes.
pub struct AgentRuntimeAppState {
    /// The runtime that owns the agent fleet.
    pub runtime: Arc<AgentRuntime>,
    /// session_id -> owning tenant.
    tenants: Mutex<HashMap<String, String>>,
    /// Accepted bearer tokens; empty means auth is disabled.
    auth_tokens: Vec<String>,
}

impl AgentRuntimeAppState {
    /// Build state over a warm baseline image (auth disabled), with the
    /// session record configured from the environment.
    pub fn new(baseline: &[u8]) -> Self {
        Self::with_context_config(baseline, &ContextConfig::from_env())
    }

    /// Build state over a warm baseline image with an explicit record location.
    pub fn with_context_config(baseline: &[u8], context: &ContextConfig) -> Self {
        let runtime = Arc::new(AgentRuntime::new(baseline));
        // Without this the seven `context.*` tools are advertised and then
        // refuse every call, because nothing else in a deployment ever
        // installs a host.
        install_context_host(&runtime, context);
        Self {
            runtime,
            tenants: Mutex::new(HashMap::new()),
            auth_tokens: Vec::new(),
        }
    }

    /// Build state requiring one of `auth_tokens` as a bearer token.
    pub fn with_auth_tokens(baseline: &[u8], auth_tokens: Vec<String>) -> Self {
        Self {
            auth_tokens,
            ..Self::new(baseline)
        }
    }

    /// Drop tenancy records for agents the runtime no longer has (e.g. reaped).
    fn reconcile(&self) {
        let live: HashSet<String> = self.runtime.agent_session_ids().into_iter().collect();
        self.tenants.lock().retain(|sid, _| live.contains(sid));
    }
}

fn tenant_of(headers: &HeaderMap) -> String {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string()
}

/// `None` if the request is authorized, otherwise the `401` response to return.
fn check_auth(state: &AgentRuntimeAppState, headers: &HeaderMap) -> Option<Response> {
    if state.auth_tokens.is_empty() {
        return None;
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|h| h.strip_prefix("Bearer ").unwrap_or(h));
    match token {
        Some(t) if state.auth_tokens.iter().any(|k| k == t) => None,
        _ => Some(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "missing or invalid bearer token" })),
            )
                .into_response(),
        ),
    }
}

// ── DTOs ──

#[derive(Debug, Deserialize)]
pub struct SpawnAgentRequest {
    /// Caller-chosen agent identifier.
    pub agent_id: String,
    /// Capability set: `full`, `operator` (default), `read_only`, or `none`.
    pub capabilities: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub session_id: String,
    pub agent_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentListResponse {
    pub agents: Vec<String>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FleetResponse {
    /// Live agents owned by the requesting tenant.
    pub live_agents: usize,
    /// Shared baseline size (whole fleet).
    pub baseline_bytes: usize,
    /// Total fleet memory across all tenants (shared baseline + private pages).
    pub fleet_memory_bytes: usize,
}

#[derive(Debug, Deserialize)]
pub struct ReapRequest {
    pub max_idle_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReapResponse {
    pub reaped: usize,
}

fn parse_capabilities(spec: Option<&str>) -> Result<AgentCapabilities, String> {
    Ok(match spec {
        None | Some("operator") => AgentCapabilities::operator(),
        Some("full") => AgentCapabilities::full(),
        Some("read_only") | Some("readonly") => AgentCapabilities::read_only(),
        Some("none") => AgentCapabilities::none(),
        Some(other) => return Err(format!("unknown capabilities: {other}")),
    })
}

// ── Handlers ──

/// POST /api/v1/agents — spawn an agent for the requesting tenant.
async fn spawn_agent(
    State(state): State<Arc<AgentRuntimeAppState>>,
    headers: HeaderMap,
    Json(req): Json<SpawnAgentRequest>,
) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    let tenant = tenant_of(&headers);
    let caps = match parse_capabilities(req.capabilities.as_deref()) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    };
    match state.runtime.spawn_agent(&req.agent_id, caps) {
        Ok(handle) => {
            let session_id = handle.session_id().to_string();
            state.tenants.lock().insert(session_id.clone(), tenant);
            (
                StatusCode::CREATED,
                Json(AgentResponse {
                    session_id,
                    agent_id: req.agent_id,
                }),
            )
                .into_response()
        }
        // At capacity (after idle reclamation) — back-pressure to the caller.
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e }))).into_response(),
    }
}

/// GET /api/v1/agents — list the requesting tenant's live agents.
async fn list_agents(
    State(state): State<Arc<AgentRuntimeAppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    let tenant = tenant_of(&headers);
    state.reconcile();
    let agents: Vec<String> = state
        .tenants
        .lock()
        .iter()
        .filter(|(_, t)| **t == tenant)
        .map(|(sid, _)| sid.clone())
        .collect();
    Json(AgentListResponse {
        total: agents.len(),
        agents,
    })
    .into_response()
}

/// GET /api/v1/agents/fleet — fleet stats (tenant agent count + global memory).
async fn fleet_stats(
    State(state): State<Arc<AgentRuntimeAppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    let tenant = tenant_of(&headers);
    state.reconcile();
    let tenant_agents = state
        .tenants
        .lock()
        .values()
        .filter(|t| **t == tenant)
        .count();
    Json(FleetResponse {
        live_agents: tenant_agents,
        baseline_bytes: state.runtime.baseline_bytes(),
        fleet_memory_bytes: state.runtime.fleet_memory_bytes(),
    })
    .into_response()
}

/// POST /api/v1/agents/reap — reap agents idle beyond `max_idle_secs`.
async fn reap_idle(
    State(state): State<Arc<AgentRuntimeAppState>>,
    headers: HeaderMap,
    Json(req): Json<ReapRequest>,
) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    let reaped = state
        .runtime
        .reap_idle(Duration::from_secs(req.max_idle_secs));
    state.reconcile();
    Json(ReapResponse { reaped }).into_response()
}

/// DELETE /api/v1/agents/{session_id} — release an agent (owning tenant only).
async fn release_agent(
    State(state): State<Arc<AgentRuntimeAppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    let tenant = tenant_of(&headers);
    // Only the owning tenant may release; others get a 404 (no existence leak).
    let owned = state
        .tenants
        .lock()
        .get(&session_id)
        .map(|t| *t == tenant)
        .unwrap_or(false);
    if !owned {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found" })),
        )
            .into_response();
    }
    state.runtime.release_agent(&session_id);
    state.tenants.lock().remove(&session_id);
    StatusCode::NO_CONTENT.into_response()
}

/// Build the agent-runtime API router.
pub fn create_agent_runtime_router(state: Arc<AgentRuntimeAppState>) -> Router {
    Router::new()
        .route("/api/v1/agents", post(spawn_agent).get(list_agents))
        .route("/api/v1/agents/fleet", get(fleet_stats))
        .route("/api/v1/agents/reap", post(reap_idle))
        .route("/api/v1/agents/{session_id}", delete(release_agent))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn app_with(state: AgentRuntimeAppState) -> Router {
        create_agent_runtime_router(Arc::new(state))
    }

    /// These routes are about tenancy and auth, so they run with no record:
    /// otherwise every `cargo test` would append to the developer's real
    /// append-only log, which is by design never truncated.
    fn no_record() -> ContextConfig {
        ContextConfig {
            root: None,
            ..ContextConfig::default()
        }
    }

    fn test_app() -> Router {
        app_with(AgentRuntimeAppState::with_context_config(
            &vec![0u8; 4096],
            &no_record(),
        ))
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    fn post_req(
        uri: &str,
        tenant: Option<&str>,
        token: Option<&str>,
        body: serde_json::Value,
    ) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(t) = tenant {
            b = b.header("x-tenant-id", t);
        }
        if let Some(tok) = token {
            b = b.header("authorization", format!("Bearer {tok}"));
        }
        b.body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    fn get_req(uri: &str, tenant: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().uri(uri);
        if let Some(t) = tenant {
            b = b.header("x-tenant-id", t);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn spawn_list_release_lifecycle() {
        let app = test_app();
        let (s1, b1) = send(
            &app,
            post_req("/api/v1/agents", None, None, json!({"agent_id":"a1"})),
        )
        .await;
        assert_eq!(s1, StatusCode::CREATED);
        let a1: AgentResponse = serde_json::from_slice(&b1).unwrap();
        send(
            &app,
            post_req("/api/v1/agents", None, None, json!({"agent_id":"a2"})),
        )
        .await;

        let (sl, bl) = send(&app, get_req("/api/v1/agents", None)).await;
        assert_eq!(sl, StatusCode::OK);
        let list: AgentListResponse = serde_json::from_slice(&bl).unwrap();
        assert_eq!(list.total, 2);

        let (sf, bf) = send(&app, get_req("/api/v1/agents/fleet", None)).await;
        assert_eq!(sf, StatusCode::OK);
        let fleet: FleetResponse = serde_json::from_slice(&bf).unwrap();
        assert_eq!(fleet.live_agents, 2);
        assert_eq!(fleet.fleet_memory_bytes, 4096);

        let (sd, _) = send(
            &app,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/agents/{}", a1.session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(sd, StatusCode::NO_CONTENT);

        let (_, bl2) = send(&app, get_req("/api/v1/agents", None)).await;
        let list2: AgentListResponse = serde_json::from_slice(&bl2).unwrap();
        assert_eq!(list2.total, 1);
    }

    #[tokio::test]
    async fn tenants_are_isolated() {
        let app = test_app();
        // Tenant A spawns an agent.
        let (_, ba) = send(
            &app,
            post_req(
                "/api/v1/agents",
                Some("acme"),
                None,
                json!({"agent_id":"x"}),
            ),
        )
        .await;
        let a: AgentResponse = serde_json::from_slice(&ba).unwrap();

        // Tenant B sees none of A's agents.
        let (_, bl) = send(&app, get_req("/api/v1/agents", Some("globex"))).await;
        let list: AgentListResponse = serde_json::from_slice(&bl).unwrap();
        assert_eq!(list.total, 0);

        // Tenant B cannot release A's agent (404, no existence leak).
        let (sd, _) = send(
            &app,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/agents/{}", a.session_id))
                .header("x-tenant-id", "globex")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(sd, StatusCode::NOT_FOUND);

        // Tenant A still sees its agent.
        let (_, bla) = send(&app, get_req("/api/v1/agents", Some("acme"))).await;
        let lista: AgentListResponse = serde_json::from_slice(&bla).unwrap();
        assert_eq!(lista.total, 1);
    }

    #[tokio::test]
    async fn auth_is_enforced_when_configured() {
        let mut state = AgentRuntimeAppState::with_context_config(&vec![0u8; 4096], &no_record());
        state.auth_tokens = vec!["secret".to_string()];
        let app = app_with(state);
        // No token -> 401.
        let (s_no, _) = send(
            &app,
            post_req("/api/v1/agents", None, None, json!({"agent_id":"a"})),
        )
        .await;
        assert_eq!(s_no, StatusCode::UNAUTHORIZED);
        // Wrong token -> 401.
        let (s_bad, _) = send(
            &app,
            post_req(
                "/api/v1/agents",
                None,
                Some("nope"),
                json!({"agent_id":"a"}),
            ),
        )
        .await;
        assert_eq!(s_bad, StatusCode::UNAUTHORIZED);
        // Correct token -> 201.
        let (s_ok, _) = send(
            &app,
            post_req(
                "/api/v1/agents",
                None,
                Some("secret"),
                json!({"agent_id":"a"}),
            ),
        )
        .await;
        assert_eq!(s_ok, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn reap_releases_idle_agents() {
        let app = test_app();
        send(
            &app,
            post_req("/api/v1/agents", None, None, json!({"agent_id":"a"})),
        )
        .await;
        send(
            &app,
            post_req("/api/v1/agents", None, None, json!({"agent_id":"b"})),
        )
        .await;

        let (s, b) = send(
            &app,
            post_req(
                "/api/v1/agents/reap",
                None,
                None,
                json!({"max_idle_secs":0}),
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let reap: ReapResponse = serde_json::from_slice(&b).unwrap();
        assert_eq!(reap.reaped, 2);

        // Tenancy records were reconciled away.
        let (_, bl) = send(&app, get_req("/api/v1/agents", None)).await;
        let list: AgentListResponse = serde_json::from_slice(&bl).unwrap();
        assert_eq!(list.total, 0);
    }

    #[tokio::test]
    async fn unknown_capabilities_is_bad_request() {
        let app = test_app();
        let (s, _) = send(
            &app,
            post_req(
                "/api/v1/agents",
                None,
                None,
                json!({"agent_id":"a","capabilities":"wizard"}),
            ),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
