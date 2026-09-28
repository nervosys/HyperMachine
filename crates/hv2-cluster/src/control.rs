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
    /// The URL sandboxes' workload tokens name as their issuer, and where
    /// this serves OIDC discovery for them; nodes must be given the same.
    pub identity_issuer: Option<String>,
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
        // No global timeout: a streaming or long request is the caller's
        // business. Creation gets its own below.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self::with_client(store, config, http)
    }

    /// [`Self::new`], reaching nodes through `http` -- one built by
    /// [`crate::mtls::Mtls::http_client`], for nodes that require a client
    /// certificate.
    #[must_use]
    pub fn with_client(
        store: Arc<dyn ClusterStore>,
        config: ControlConfig,
        http: reqwest::Client,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            http,
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
        .route("/sandboxes/{id}/pause", post(forward))
        .route("/sandboxes/{id}/resume", post(forward))
        .route("/sandboxes/{id}/fork", post(forward))
        .route("/sandboxes/{id}/snapshots", post(forward))
        .route("/sandboxes/metrics", get(sandboxes_metrics))
        .route("/sandboxes/{id}/metrics", get(forward))
        .route("/sandboxes/{id}/logs", get(forward))
        .route("/v2/sandboxes/{id}/logs", get(forward))
        .route("/snapshots", get(snapshots))
        .route("/templates/{id}", axum::routing::delete(delete_template))
        .route("/v3/templates", post(to_builder))
        .route("/templates/{id}/files/{hash}", get(to_builder))
        .route("/v2/templates/{id}/builds/{build}", post(to_builder))
        .route("/templates/{id}/builds/{build}/status", get(to_builder))
        .route("/templates/aliases/{alias}", get(template_alias))
        .route("/volumes", get(list_volumes).post(to_volume_node))
        .route("/volumes/{id}", get(to_volume_node).delete(to_volume_node))
        .route("/sandboxes/{id}/refreshes", post(forward))
        .route("/sandboxes/{id}/network", any(forward))
        .route("/sandboxes/{id}/network/decisions", get(forward))
        .route("/sandboxes/{id}/exec", post(forward))
        .route("/cluster/nodes", get(cluster_nodes))
        .route("/templates", get(templates).post(build_templates))
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
        // Public, like any OIDC issuer's: what a cloud verifying a
        // sandbox's workload token fetches.
        .route("/.well-known/jwks.json", get(jwks))
        .route("/.well-known/openid-configuration", get(openid_configuration))
        .route("/ui", get(ui))
        // Without the key: the SDK sends none with an upload. The node
        // checks the token its authenticated link carried.
        .route("/templates/{id}/files/{hash}", axum::routing::put(to_builder))
        // Volume content: each volume's bearer token, which its node checks.
        .route("/volumecontent/{id}/file", any(to_volume_node))
        .route("/volumecontent/{id}/dir", any(to_volume_node))
        .route("/volumecontent/{id}/path", any(to_volume_node))
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
    // Only nodes with the template asked for. The body is the node's to
    // validate; this reads only which template, and a body that does not
    // parse goes on for the node to refuse.
    let template = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|b| {
            b.get("templateID")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "base".to_string());
    let nodes: Vec<_> = nodes.into_iter().filter(|n| n.offers(&template)).collect();
    if nodes.is_empty() {
        return api_error(
            StatusCode::NOT_FOUND,
            format!("template {template:?} not found on any live node"),
        );
    }
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
    if let Some(state) = &query.state {
        records.retain(|r| state.split(',').any(|s| s == r.state()));
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
    // Where to send it: the sandbox's node -- or, for one paused into
    // shared storage whose node is gone, any node with room, since any of
    // them can resume it.
    let targets = match control.store.node(&record.node_id).await {
        Ok(Some(node)) => vec![node],
        Ok(None) if record.survives_its_node() => match control.store.nodes().await {
            Ok(nodes) => {
                let mut order = candidates(&nodes);
                if order.is_empty() {
                    // Full everywhere: a node may still make room.
                    order = nodes;
                }
                if order.is_empty() {
                    return api_error(StatusCode::SERVICE_UNAVAILABLE, "no node is alive");
                }
                order
            }
            Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
        },
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
    let mut answered = None;
    let mut refusals = Vec::new();
    let last = targets.len() - 1;
    for (i, node) in targets.iter().enumerate() {
        let mut request = control
            .http
            .request(
                reqwest_method.clone(),
                format!("{}{path_and_query}", node.api),
            )
            .body(body.clone());
        if let Some(content_type) = headers.get("content-type") {
            request = request.header("content-type", content_type.as_bytes());
        }
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
        // A full node, when there are others to ask: as for a create.
        if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE && i < last {
            refusals.push(format!("{}: {}", node.id, response.status()));
            continue;
        }
        answered = Some((node, response));
        break;
    }
    let Some((node, response)) = answered else {
        return api_error(
            StatusCode::BAD_GATEWAY,
            format!("no node answered: {}", refusals.join("; ")),
        );
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

    // `connect` and `resume` answer with the descriptor, which has to point
    // here too; `fork` with one per fork, beside an error or not.
    let path = uri.path();
    if (path.ends_with("/connect") || path.ends_with("/resume")) && status.is_success() {
        if let Ok(mut descriptor) = serde_json::from_slice::<Value>(&bytes) {
            rewrite_descriptor(&control, &mut descriptor, &node.id);
            return (status, Json(descriptor)).into_response();
        }
    }
    if path.ends_with("/fork") && status.is_success() {
        if let Ok(mut results) = serde_json::from_slice::<Vec<Value>>(&bytes) {
            for result in &mut results {
                if let Some(sandbox) = result.get_mut("sandbox") {
                    rewrite_descriptor(&control, sandbox, &node.id);
                }
            }
            return (status, Json(results)).into_response();
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
        // Paused into shared storage: nothing of it was on the node, so
        // nothing of it went with the node.
        if record.survives_its_node() {
            continue;
        }
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

/// `GET /templates`: every template a live node offers, E2B's `Template`
/// shape as far as the cluster knows it, with the nodes offering each.
async fn templates(State(control): State<Arc<ControlPlane>>) -> Response {
    let nodes = match control.store.nodes().await {
        Ok(nodes) => nodes,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let mut offered: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in &nodes {
        let names = if node.templates.is_empty() {
            vec!["base".to_string()]
        } else {
            node.templates.clone()
        };
        for name in names {
            offered.entry(name).or_default().push(node.id.clone());
        }
    }
    Json(
        offered
            .into_iter()
            .map(|(name, nodes)| {
                json!({
                    "templateID": name,
                    "buildID": name,
                    "aliases": [name],
                    "public": false,
                    "buildStatus": "ready",
                    "nodeIDs": nodes,
                })
            })
            .collect::<Vec<_>>(),
    )
    .into_response()
}

/// `POST /templates`: have every live node build a template from an image.
/// Answered with each node's answer; 202 if any node took the build.
///
/// Every node, not one: without a shared snapshot store a template exists
/// only where it was built. With one, nodes adopt what another built, and
/// the extra builds converge on the same content-addressed files.
async fn build_templates(State(control): State<Arc<ControlPlane>>, body: Bytes) -> Response {
    let nodes = match control.store.nodes().await {
        Ok(nodes) => nodes,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let mut answers = Vec::new();
    let mut accepted = false;
    for node in &nodes {
        let mut request = control
            .http
            .post(format!("{}/templates", node.api))
            .header("content-type", "application/json")
            .body(body.clone());
        if let Some(token) = &control.config.cluster_token {
            request = request.header(CLUSTER_TOKEN_HEADER, token);
        }
        let (status, answer) = match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let answer = response.json::<Value>().await.unwrap_or(Value::Null);
                (status, answer)
            }
            Err(e) => (502, json!({ "message": e.to_string() })),
        };
        accepted |= status == 202;
        answers.push(json!({ "nodeID": node.id, "status": status, "answer": answer }));
    }
    let status = if accepted {
        StatusCode::ACCEPTED
    } else {
        StatusCode::BAD_GATEWAY
    };
    (status, Json(answers)).into_response()
}

/// `method path` on every live node: each node's ID, status and JSON answer.
async fn on_every_node(
    control: &ControlPlane,
    method: Method,
    path: &str,
) -> Result<Vec<(String, u16, Value)>, (StatusCode, String)> {
    let nodes = control
        .store
        .nodes()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    let mut answers = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let mut request = control
            .http
            .request(method.clone(), format!("{}{path}", node.api));
        if let Some(token) = &control.config.cluster_token {
            request = request.header(CLUSTER_TOKEN_HEADER, token);
        }
        answers.push(match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let answer = response.json::<Value>().await.unwrap_or(Value::Null);
                (node.id.clone(), status, answer)
            }
            Err(e) => (node.id.clone(), 502, json!({ "message": e.to_string() })),
        });
    }
    Ok(answers)
}

/// `GET /snapshots`: every live node's snapshots, once each -- nodes that
/// share a snapshot store offer the same ones -- with the nodes offering it.
async fn snapshots(State(control): State<Arc<ControlPlane>>, uri: axum::http::Uri) -> Response {
    let path = uri
        .path_and_query()
        .map_or("/snapshots", axum::http::uri::PathAndQuery::as_str);
    let answers = match on_every_node(&control, Method::GET, path).await {
        Ok(answers) => answers,
        Err((status, message)) => return api_error(status, message),
    };
    let mut merged: BTreeMap<String, Value> = BTreeMap::new();
    for (node, status, answer) in answers {
        if status != 200 {
            continue;
        }
        for mut snapshot in answer.as_array().cloned().unwrap_or_default() {
            let Some(id) = snapshot["snapshotID"].as_str().map(str::to_string) else {
                continue;
            };
            let entry = merged.entry(id).or_insert_with(|| {
                snapshot["nodeIDs"] = json!([]);
                snapshot
            });
            if let Some(nodes) = entry["nodeIDs"].as_array_mut() {
                nodes.push(json!(node));
            }
        }
    }
    Json(merged.into_values().collect::<Vec<_>>()).into_response()
}

/// `DELETE /templates/{id}`: delete a snapshot wherever it is offered.
async fn delete_template(
    State(control): State<Arc<ControlPlane>>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/templates/{}", crate::model::untagged(&id));
    let answers = match on_every_node(&control, Method::DELETE, &path).await {
        Ok(answers) => answers,
        Err((status, message)) => return api_error(status, message),
    };
    if answers.iter().any(|(_, status, _)| *status == 204) {
        StatusCode::NO_CONTENT.into_response()
    } else if let Some((_, _, answer)) = answers.iter().find(|(_, s, _)| *s == 409) {
        (StatusCode::CONFLICT, Json(answer.clone())).into_response()
    } else {
        api_error(StatusCode::NOT_FOUND, format!("no snapshot {id}"))
    }
}

/// The node a template's builds run on: chosen from the template's name by
/// rendezvous hashing over the live nodes, so every control plane chooses
/// the same one, and every call of one build -- its upload links, uploads,
/// start and status -- reaches the node that holds it.
async fn builder(
    control: &ControlPlane,
    template: &str,
) -> Result<crate::model::NodeInfo, (StatusCode, String)> {
    let nodes = control
        .store
        .nodes()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    let template = crate::model::untagged(template);
    nodes
        .into_iter()
        .max_by_key(|node| fnv1a(&[template.as_bytes(), b"\0", node.id.as_bytes()]))
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "no node is alive".to_string(),
            )
        })
}

/// FNV-1a: stable across processes and releases, as a choice every
/// control plane must agree on has to be.
fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// E2B's template build calls, to the template's build node. The body is
/// streamed through, not buffered: an upload may be large. The host the
/// caller reached is passed on, so the upload links a node makes point
/// back here.
async fn to_builder(
    State(control): State<Arc<ControlPlane>>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = uri.path();
    let (template, body) = if path == "/v3/templates" {
        // The name is in the body, which is small.
        let bytes = match axum::body::to_bytes(body, 1 << 20).await {
            Ok(bytes) => bytes,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        };
        let parsed = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
        let name = parsed["name"]
            .as_str()
            .or_else(|| parsed["alias"].as_str())
            .unwrap_or_default()
            .to_string();
        (name, reqwest::Body::from(bytes))
    } else {
        let mut segments = path.trim_start_matches("/v2").split('/').skip(2);
        let name = segments.next().unwrap_or_default().to_string();
        (name, reqwest::Body::wrap_stream(body.into_data_stream()))
    };
    let node = match builder(&control, &template).await {
        Ok(node) => node,
        Err((status, message)) => return api_error(status, message),
    };
    relay(&control, &node, &method, &uri, &headers, body).await
}

/// Pass one request to `node`, both bodies streamed: an upload or a
/// download may be large. The host the caller reached goes along, so links
/// a node makes point back here; so does a bearer token, which the volume
/// content API authenticates with.
async fn relay(
    control: &ControlPlane,
    node: &crate::model::NodeInfo,
    method: &Method,
    uri: &axum::http::Uri,
    headers: &HeaderMap,
    body: reqwest::Body,
) -> Response {
    let path_and_query = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
    let reqwest_method =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut request = control
        .http
        .request(reqwest_method, format!("{}{path_and_query}", node.api))
        .body(body);
    for name in ["content-type", "content-length", "authorization"] {
        if let Some(value) = headers.get(name) {
            request = request.header(name, value.as_bytes());
        }
    }
    if let Some(host) = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
    {
        request = request.header("x-forwarded-host", host.as_bytes());
    }
    if let Some(proto) = headers.get("x-forwarded-proto") {
        request = request.header("x-forwarded-proto", proto.as_bytes());
    }
    if let Some(token) = &control.config.cluster_token {
        request = request.header(CLUSTER_TOKEN_HEADER, token);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => return api_error(StatusCode::BAD_GATEWAY, format!("{}: {e}", node.id)),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response.headers().get("content-type").cloned();
    let mut out = Response::new(Body::from_stream(response.bytes_stream()));
    *out.status_mut() = status;
    if let Some(value) = content_type {
        out.headers_mut().insert("content-type", value);
    }
    out
}

/// A volume's calls -- its API and its content API -- to the node chosen
/// for it by rendezvous over its ID, which its name determines: a create
/// and every call after it meet on the same node. With a shared snapshot
/// store every node holds every volume, and the choice only spreads load.
async fn to_volume_node(
    State(control): State<Arc<ControlPlane>>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = uri.path();
    let (id, body) = if path == "/volumes" {
        let bytes = match axum::body::to_bytes(body, 1 << 16).await {
            Ok(bytes) => bytes,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        };
        let parsed = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
        let name = parsed["name"].as_str().unwrap_or_default();
        (crate::model::volume_id(name), reqwest::Body::from(bytes))
    } else {
        // /volumes/{id} and /volumecontent/{id}/...
        let id = path.split('/').nth(2).unwrap_or_default().to_string();
        (id, reqwest::Body::wrap_stream(body.into_data_stream()))
    };
    let node = match builder(&control, &id).await {
        Ok(node) => node,
        Err((status, message)) => return api_error(status, message),
    };
    relay(&control, &node, &method, &uri, &headers, body).await
}

/// `GET /sandboxes/metrics`: each sandbox's latest sample, from whichever
/// node runs it.
async fn sandboxes_metrics(
    State(control): State<Arc<ControlPlane>>,
    uri: axum::http::Uri,
) -> Response {
    let path = uri
        .path_and_query()
        .map_or("/sandboxes/metrics", axum::http::uri::PathAndQuery::as_str);
    let answers = match on_every_node(&control, Method::GET, path).await {
        Ok(answers) => answers,
        Err((status, message)) => return api_error(status, message),
    };
    let mut merged = serde_json::Map::new();
    for (_, status, answer) in answers {
        if status == 200 {
            if let Some(found) = answer["sandboxes"].as_object() {
                merged.extend(found.clone());
            }
        }
    }
    Json(json!({ "sandboxes": merged })).into_response()
}

/// `GET /volumes`: every live node's volumes, once each.
async fn list_volumes(State(control): State<Arc<ControlPlane>>) -> Response {
    let answers = match on_every_node(&control, Method::GET, "/volumes").await {
        Ok(answers) => answers,
        Err((status, message)) => return api_error(status, message),
    };
    let mut merged: BTreeMap<String, Value> = BTreeMap::new();
    for (_, status, answer) in answers {
        if status == 200 {
            for volume in answer.as_array().cloned().unwrap_or_default() {
                if let Some(id) = volume["volumeID"].as_str() {
                    merged.entry(id.to_string()).or_insert(volume);
                }
            }
        }
    }
    Json(merged.into_values().collect::<Vec<_>>()).into_response()
}

/// `GET /templates/aliases/{alias}`: whether a live node offers a template
/// of that name.
async fn template_alias(
    State(control): State<Arc<ControlPlane>>,
    Path(alias): Path<String>,
) -> Response {
    let nodes = match control.store.nodes().await {
        Ok(nodes) => nodes,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let name = crate::model::untagged(&alias);
    if nodes.iter().any(|n| n.offers(name)) {
        Json(json!({ "templateID": name, "public": false })).into_response()
    } else {
        api_error(StatusCode::NOT_FOUND, format!("no template {name}"))
    }
}

/// `GET /.well-known/jwks.json`: every live node's workload-token key, once
/// each -- nodes sharing a key publish the same `kid`.
async fn jwks(State(control): State<Arc<ControlPlane>>) -> Response {
    match control.store.nodes().await {
        Ok(nodes) => {
            let mut seen = std::collections::BTreeSet::new();
            let keys: Vec<Value> = nodes
                .into_iter()
                .filter_map(|n| n.jwk)
                .filter(|jwk| seen.insert(jwk["kid"].to_string()))
                .collect();
            Json(json!({ "keys": keys })).into_response()
        }
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

/// `GET /.well-known/openid-configuration`, when an issuer is configured.
async fn openid_configuration(State(control): State<Arc<ControlPlane>>) -> Response {
    let Some(issuer) = &control.config.identity_issuer else {
        return api_error(
            StatusCode::NOT_FOUND,
            "no --identity-issuer: this control plane is not an OIDC issuer",
        );
    };
    let base = issuer.trim_end_matches('/');
    Json(json!({
        "issuer": issuer,
        "jwks_uri": format!("{base}/.well-known/jwks.json"),
        "response_types_supported": ["id_token"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
    }))
    .into_response()
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
    backend_tls: Option<(
        Arc<rustls::ClientConfig>,
        rustls::pki_types::ServerName<'static>,
    )>,
}

impl ClusterRoutes {
    #[must_use]
    pub fn new(store: Arc<dyn ClusterStore>, ttl: Duration) -> Self {
        Self {
            store,
            cache: Mutex::new(HashMap::new()),
            ttl,
            backend_tls: None,
        }
    }

    /// Relay to nodes' proxies over mutual TLS.
    ///
    /// # Errors
    ///
    /// The certificate and key do not go together.
    pub fn with_mtls(mut self, mtls: &crate::mtls::Mtls) -> std::io::Result<Self> {
        self.backend_tls = Some((Arc::new(mtls.client_config()?), mtls.node_name().clone()));
        Ok(self)
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
        let node = match self.store.node(&record.node_id).await.ok()? {
            Some(node) => node,
            // Paused into shared storage, on a node that has gone: any node
            // resumes it, and its proxy does so for the request that asks.
            None if record.survives_its_node() => {
                let nodes = self.store.nodes().await.ok()?;
                let chosen = candidates(&nodes).into_iter().next();
                chosen.or_else(|| nodes.into_iter().next())?
            }
            None => return None,
        };
        let mut cache = self.cache.lock();
        if cache.len() > 10_000 {
            cache.clear();
        }
        cache.insert(sandbox.to_string(), (node.proxy, Instant::now()));
        Some(node.proxy)
    }

    /// The cached node did not answer -- gone, with the sandbox paused into
    /// shared storage for another to resume -- so the next look goes to the
    /// store rather than waiting out the cache.
    async fn forget(&self, sandbox: &str) {
        self.cache.lock().remove(sandbox);
    }

    fn backend_tls(
        &self,
    ) -> Option<(
        Arc<rustls::ClientConfig>,
        rustls::pki_types::ServerName<'static>,
    )> {
        self.backend_tls.clone()
    }
}
