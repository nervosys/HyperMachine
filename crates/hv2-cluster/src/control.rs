//! The control plane: E2B's API for a whole cluster.
//!
//! Stateless, in CubeMaster's sense: everything it knows is in the
//! [`ClusterStore`], so any number of instances can run behind a load
//! balancer, any of them can serve any request, and losing one loses
//! nothing. Creation is scheduled onto a node and forwarded there; every
//! per-sandbox call is forwarded to the node that owns the sandbox; listing
//! and detail are answered from the store. Envd traffic goes through
//! [`ClusterRoutes`], which routes a sandbox's calls to its node's proxy.
//!
//! # Trust
//!
//! Clients authenticate with `X-API-Key`, as E2B's SDK sends it. The control
//! plane authenticates to nodes with a shared cluster token, and a node in a
//! cluster refuses its API to anything without it -- so reaching a node's
//! port directly does not bypass the key.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::metrics::{self, Counter, Exposition, Histogram};
use crate::model::{metadata_matches, parse_metadata_query, ClusterEvent, SandboxRecord};
use crate::scheduler::candidates;
use crate::store::ClusterStore;

/// The header a node reads the cluster token from.
pub const CLUSTER_TOKEN_HEADER: &str = "x-hv2-cluster-token";

/// How a control plane runs.
#[derive(Debug, Clone)]
pub struct ControlConfig {
    /// Required from clients as `X-API-Key`, when set.
    pub api_key: Option<String>,
    /// Sent to nodes, when set.
    pub cluster_token: Option<String>,
    /// This instance's envd proxy port, written into descriptors so a client
    /// told where to send envd traffic is told this instance, not a node.
    pub proxy_port: u16,
    /// How long a node may take to boot a sandbox.
    pub create_timeout: Duration,
}

pub struct ControlPlane {
    store: Arc<dyn ClusterStore>,
    http: reqwest::Client,
    config: ControlConfig,
    metrics: ControlMetrics,
}

/// What `/metrics` reports beyond the store's gauges.
#[derive(Default)]
struct ControlMetrics {
    creates_ok: Counter,
    /// No node had room.
    creates_full: Counter,
    /// The request was refused as the client's fault (4xx).
    creates_rejected: Counter,
    creates_error: Counter,
    create_latency: Histogram,
    reaped: Counter,
}

impl ControlPlane {
    #[must_use]
    pub fn new(store: Arc<dyn ClusterStore>, config: ControlConfig) -> Arc<Self> {
        Arc::new(Self {
            store,
            // No global timeout: a streaming or long request is the caller's
            // business. Creation gets its own below.
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            config,
            metrics: ControlMetrics::default(),
        })
    }
}

/// E2B's error shape, which the SDK parses before it looks at the status.
fn api_error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({ "code": status.as_u16(), "message": message.to_string() })),
    )
        .into_response()
}

/// The routes.
pub fn router(control: Arc<ControlPlane>) -> Router {
    let e2b = Router::new()
        .route("/sandboxes", post(create_v1).get(list_v1))
        .route("/v2/sandboxes", post(create_v2).get(list_v2))
        .route("/sandboxes/{id}", get(detail).delete(forward))
        .route("/sandboxes/{id}/connect", post(forward))
        .route("/v2/sandboxes/{id}/connect", post(forward))
        .route("/sandboxes/{id}/timeout", post(forward))
        .route("/sandboxes/{id}/refreshes", post(forward))
        .route("/sandboxes/{id}/network", any(forward))
        .route("/sandboxes/{id}/network/decisions", get(forward))
        .route("/sandboxes/{id}/exec", post(forward))
        .route("/cluster/nodes", get(cluster_nodes))
        .route("/cluster/events", get(cluster_events))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&control),
            require_api_key,
        ));
    Router::new()
        .route("/health", get(health))
        // Outside the key, like /health: a scraper holds no API key, and
        // counts are all this says.
        .route("/metrics", get(metrics))
        .route("/ui", get(ui))
        .merge(e2b)
        .fallback(|uri: axum::http::Uri| async move {
            api_error(StatusCode::NOT_FOUND, format!("no route {uri}"))
        })
        .with_state(control)
}

async fn require_api_key(
    State(control): State<Arc<ControlPlane>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(key) = &control.config.api_key {
        use subtle::ConstantTimeEq;
        let sent = request
            .headers()
            .get("x-api-key")
            .map(HeaderValue::as_bytes)
            .unwrap_or_default();
        if !bool::from(sent.ct_eq(key.as_bytes())) {
            return api_error(StatusCode::UNAUTHORIZED, "missing or wrong X-API-Key");
        }
    }
    next.run(request).await
}

async fn health(State(control): State<Arc<ControlPlane>>) -> Response {
    match control.store.nodes().await {
        Ok(nodes) => Json(json!({ "status": "ok", "nodes": nodes.len() })).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

// ── Creation ────────────────────────────────────────────────────────────────

async fn create_v1(State(control): State<Arc<ControlPlane>>, body: Bytes) -> Response {
    create(&control, "/sandboxes", body).await
}

async fn create_v2(State(control): State<Arc<ControlPlane>>, body: Bytes) -> Response {
    create(&control, "/v2/sandboxes", body).await
}

/// Point a node's descriptor at this control plane's proxy.
fn rewrite_descriptor(control: &ControlPlane, descriptor: &mut Value, node: &str) {
    if let Some(object) = descriptor.as_object_mut() {
        object.insert("proxyPort".into(), json!(control.config.proxy_port));
        object.insert("nodeID".into(), json!(node));
    }
}

async fn create(control: &ControlPlane, path: &str, body: Bytes) -> Response {
    let started = Instant::now();
    let response = create_inner(control, path, body).await;
    let m = &control.metrics;
    match response.status() {
        StatusCode::CREATED => {
            m.creates_ok.inc();
            m.create_latency.observe(started.elapsed());
        }
        StatusCode::SERVICE_UNAVAILABLE => m.creates_full.inc(),
        s if s.is_client_error() => m.creates_rejected.inc(),
        _ => m.creates_error.inc(),
    }
    response
}

async fn create_inner(control: &ControlPlane, path: &str, body: Bytes) -> Response {
    let nodes = match control.store.nodes().await {
        Ok(nodes) => nodes,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let order = candidates(&nodes);
    if order.is_empty() {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("no node has room ({} alive)", nodes.len()),
        );
    }

    let mut refusals = Vec::new();
    for node in order {
        let mut request = control
            .http
            .post(format!("{}{path}", node.api))
            .timeout(control.config.create_timeout)
            .header("content-type", "application/json")
            .body(body.clone());
        if let Some(token) = &control.config.cluster_token {
            request = request.header(CLUSTER_TOKEN_HEADER, token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                refusals.push(format!("{}: {e}", node.id));
                continue;
            }
        };
        let status = response.status();
        // Full, or overloaded: the node is the authority on its capacity,
        // and a refusal here is the race between two control planes
        // resolving itself. Try the next.
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            refusals.push(format!("{}: {status}", node.id));
            continue;
        }
        let bytes = response.bytes().await.unwrap_or_default();
        if !status.is_success() {
            // A client error is the client's, whichever node answered it.
            return (
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                [("content-type", "application/json")],
                bytes,
            )
                .into_response();
        }
        let mut descriptor: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => return api_error(StatusCode::BAD_GATEWAY, format!("{}: {e}", node.id)),
        };
        rewrite_descriptor(control, &mut descriptor, &node.id);
        return (StatusCode::CREATED, Json(descriptor)).into_response();
    }
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        format!("every node refused: {}", refusals.join("; ")),
    )
}

// ── Listing and detail, from the store ──────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ListQuery {
    metadata: Option<String>,
    state: Option<String>,
    limit: Option<usize>,
    #[serde(rename = "nextToken")]
    next_token: Option<String>,
    order: Option<String>,
}

async fn matching(
    control: &ControlPlane,
    query: &ListQuery,
) -> crate::store::Result<Vec<SandboxRecord>> {
    let wanted = query
        .metadata
        .as_deref()
        .map(parse_metadata_query)
        .unwrap_or_default();
    let mut records: Vec<SandboxRecord> = control
        .store
        .sandboxes()
        .await?
        .into_iter()
        .filter(|r| metadata_matches(r, &wanted))
        .collect();
    // Every sandbox here is running; a query for only `paused` ones gets none.
    if let Some(state) = &query.state {
        if !state.split(',').any(|s| s == "running") {
            records.clear();
        }
    }
    if query.order.as_deref() == Some("desc") {
        records.reverse();
    }
    Ok(records)
}

async fn list_v1(
    State(control): State<Arc<ControlPlane>>,
    Query(query): Query<ListQuery>,
) -> Response {
    match matching(&control, &query).await {
        Ok(records) => Json(
            records
                .iter()
                .map(SandboxRecord::listed)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

/// `GET /v2/sandboxes`, paginated by `nextToken` / `x-next-token` -- the
/// token is the last sandbox ID of the page, so a page is stable against
/// sandboxes created after it was served.
async fn list_v2(
    State(control): State<Arc<ControlPlane>>,
    Query(query): Query<ListQuery>,
) -> Response {
    let records = match matching(&control, &query).await {
        Ok(records) => records,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let start = query
        .next_token
        .as_deref()
        .and_then(|token| records.iter().position(|r| r.sandbox_id == token))
        .map_or(0, |i| i + 1);
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let page: Vec<_> = records.iter().skip(start).take(limit).collect();
    let mut response = Json(page.iter().map(|r| r.listed()).collect::<Vec<_>>()).into_response();
    if start + page.len() < records.len() {
        if let Some(last) = page.last() {
            if let Ok(value) = HeaderValue::from_str(&last.sandbox_id) {
                response.headers_mut().insert("x-next-token", value);
            }
        }
    }
    response
}

async fn detail(State(control): State<Arc<ControlPlane>>, Path(id): Path<String>) -> Response {
    match control.store.sandbox(&id).await {
        Ok(Some(record)) => Json(record.detail()).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}")),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

// ── Everything else goes to the sandbox's node ──────────────────────────────

async fn forward(
    State(control): State<Arc<ControlPlane>>,
    Path(id): Path<String>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let record = match control.store.sandbox(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}")),
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let node = match control.store.node(&record.node_id).await {
        Ok(Some(node)) => node,
        Ok(None) => {
            // Its node is gone, and its VM with it. The reaper would get to
            // it; this request already has.
            if control.store.delete_sandbox(&id).await.unwrap_or(false) {
                let _ = control
                    .store
                    .publish(
                        &ClusterEvent::new("sandbox-lost", &record.node_id, Some(&id))
                            .with_detail("its node stopped heartbeating"),
                    )
                    .await;
            }
            return if method == Method::DELETE {
                StatusCode::NO_CONTENT.into_response()
            } else {
                api_error(
                    StatusCode::NOT_FOUND,
                    format!("sandbox {id} was lost with its node"),
                )
            };
        }
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };

    let path_and_query = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
    let reqwest_method =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut request = control
        .http
        .request(reqwest_method, format!("{}{path_and_query}", node.api))
        .body(body);
    if let Some(content_type) = headers.get("content-type") {
        request = request.header("content-type", content_type.as_bytes());
    }
    if let Some(token) = &control.config.cluster_token {
        request = request.header(CLUSTER_TOKEN_HEADER, token);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            return api_error(
                StatusCode::BAD_GATEWAY,
                format!("node {} did not answer: {e}", node.id),
            )
        }
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let bytes = response.bytes().await.unwrap_or_default();

    // `connect` answers with the descriptor, which has to point here too.
    if uri.path().ends_with("/connect") && status.is_success() {
        if let Ok(mut descriptor) = serde_json::from_slice::<Value>(&bytes) {
            rewrite_descriptor(&control, &mut descriptor, &node.id);
            return (status, Json(descriptor)).into_response();
        }
    }
    let mut out = Response::new(Body::from(bytes));
    *out.status_mut() = status;
    if let Ok(value) = HeaderValue::from_str(&content_type) {
        out.headers_mut().insert("content-type", value);
    }
    out
}

// ── Operations ──────────────────────────────────────────────────────────────

async fn cluster_nodes(State(control): State<Arc<ControlPlane>>) -> Response {
    match control.store.nodes().await {
        Ok(nodes) => Json(nodes).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn cluster_events(
    State(control): State<Arc<ControlPlane>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let count = query
        .get("count")
        .and_then(|c| c.parse().ok())
        .unwrap_or(100usize)
        .min(crate::store::EVENT_TAIL);
    match control.store.events(count).await {
        Ok(events) => Json(events).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

/// Remove the records of sandboxes whose node has stopped heartbeating.
///
/// Safe to run on every control-plane instance at once: a record is deleted
/// by whichever instance gets there first, and only that one reports it.
/// Returns how many this call reaped.
///
/// # Errors
///
/// The store could not be reached.
pub async fn reap(store: &dyn ClusterStore) -> crate::store::Result<usize> {
    let live: std::collections::HashSet<String> =
        store.nodes().await?.into_iter().map(|n| n.id).collect();
    let mut reaped = 0;
    for record in store.sandboxes().await? {
        if !live.contains(&record.node_id) && store.delete_sandbox(&record.sandbox_id).await? {
            store
                .publish(
                    &ClusterEvent::new("sandbox-lost", &record.node_id, Some(&record.sandbox_id))
                        .with_detail("its node stopped heartbeating"),
                )
                .await?;
            reaped += 1;
        }
    }
    Ok(reaped)
}

/// Run [`reap`] every `interval`, forever.
pub async fn reaper(control: Arc<ControlPlane>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        match reap(control.store.as_ref()).await {
            Ok(0) => {}
            Ok(n) => {
                for _ in 0..n {
                    control.metrics.reaped.inc();
                }
                tracing::info!("reaped {n} sandbox(es) whose node is gone");
            }
            Err(e) => tracing::warn!("reaper: {e}"),
        }
    }
}

/// `GET /metrics`: Prometheus text format.
async fn metrics(State(control): State<Arc<ControlPlane>>) -> Response {
    let m = &control.metrics;
    let mut e = Exposition::new();
    // The cluster as the store sees it -- the same from every instance.
    if let (Ok(nodes), Ok(sandboxes)) =
        (control.store.nodes().await, control.store.sandboxes().await)
    {
        e.gauge(
            "hv2_cluster_nodes",
            "Nodes whose heartbeat is current.",
            nodes.len() as f64,
        );
        e.gauge(
            "hv2_cluster_capacity",
            "Sandboxes the live nodes will run at once, summed.",
            nodes.iter().map(|n| f64::from(n.capacity)).sum(),
        );
        e.gauge(
            "hv2_cluster_sandboxes",
            "Sandboxes recorded in the store.",
            sandboxes.len() as f64,
        );
    }
    // This instance's own work.
    e.counters(
        "hv2_control_creates_total",
        "Creations handled by this control plane, by outcome.",
        "result",
        &[
            ("ok", m.creates_ok.get()),
            ("full", m.creates_full.get()),
            ("rejected", m.creates_rejected.get()),
            ("error", m.creates_error.get()),
        ],
    );
    e.histogram(
        "hv2_control_create_seconds",
        "Time to a created sandbox, through this control plane.",
        &m.create_latency,
    );
    e.counters(
        "hv2_control_reaped_total",
        "Sandboxes this instance removed because their node died.",
        "reason",
        &[("node-gone", m.reaped.get())],
    );
    ([("content-type", metrics::CONTENT_TYPE)], e.finish()).into_response()
}

/// `GET /ui`: the operator's view -- nodes, sandboxes, events. One static
/// page that calls this API with a key the viewer enters.
async fn ui() -> Response {
    (
        [
            ("content-type", "text/html; charset=utf-8"),
            // The page talks only to this origin and loads nothing else.
            (
                "content-security-policy",
                "default-src 'none'; script-src 'self' 'unsafe-inline';                  style-src 'unsafe-inline'; connect-src 'self'; img-src data:",
            ),
        ],
        include_str!("ui.html"),
    )
        .into_response()
}

// ── Envd routing ────────────────────────────────────────────────────────────

/// Routes a sandbox's envd traffic to its node's proxy.
///
/// Answers from a short cache, because an SDK makes a call per operation and
/// a store round trip per call is the latency of every command. Short
/// because a stale answer routes to a node that no longer has the sandbox,
/// which then says so -- a failed call, not a misdirected one.
pub struct ClusterRoutes {
    store: Arc<dyn ClusterStore>,
    cache: Mutex<HashMap<String, (SocketAddr, Instant)>>,
    ttl: Duration,
}

impl ClusterRoutes {
    #[must_use]
    pub fn new(store: Arc<dyn ClusterStore>, ttl: Duration) -> Self {
        Self {
            store,
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }
}

#[async_trait::async_trait]
impl hv2_api::sandbox_proxy::SandboxRoutes for ClusterRoutes {
    async fn resolve(&self, sandbox: &str, _port: u16) -> Option<SocketAddr> {
        if let Some((addr, at)) = self.cache.lock().get(sandbox) {
            if at.elapsed() < self.ttl {
                return Some(*addr);
            }
        }
        let record = self.store.sandbox(sandbox).await.ok()??;
        let node = self.store.node(&record.node_id).await.ok()??;
        let mut cache = self.cache.lock();
        if cache.len() > 10_000 {
            cache.clear();
        }
        cache.insert(sandbox.to_string(), (node.proxy, Instant::now()));
        Some(node.proxy)
    }
}
