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
use axum::extract::{MatchedPath, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Extension, Json, Router};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domains::{DomainBinding, DomainName};
use crate::metrics::{self, Counter, Exposition, Histogram};
use crate::model::{metadata_matches, parse_metadata_query, ClusterEvent, NodeInfo, SandboxRecord};
use crate::names::{NameReservation, SandboxName, NAME_OPERATION_HEADER};
use crate::scheduler::candidates;
use crate::store::{ClusterStore, DomainClaim};

/// The header a node reads the cluster token from.
pub const CLUSTER_TOKEN_HEADER: &str = "x-hv2-cluster-token";

/// How a control plane runs.
#[derive(Debug, Clone)]
pub struct ControlConfig {
    /// Required from clients as `X-API-Key`, when set.
    pub api_key: Option<String>,
    /// Additional operator-provisioned expiring keys with capability scopes.
    pub api_keys: Vec<crate::keys::ApiKeyPolicy>,
    /// Optional synced admission/completion log for protected API routes.
    pub access_audit: Option<Arc<crate::audit::AccessAudit>>,
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
    tcp_http: reqwest::Client,
    config: ControlConfig,
    api_keys: parking_lot::RwLock<Vec<crate::keys::ApiKeyPolicy>>,
    domain_verification:
        parking_lot::RwLock<Option<Arc<crate::domain_verification::DomainVerification>>>,
    native_port_range: parking_lot::RwLock<Option<crate::ports::PublicPortRange>>,
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
    /// Enable owner-only reservation APIs for one fixed operator allocation window.
    /// The gateway publishes these reservations asynchronously.
    pub fn configure_public_ports(
        &self,
        range: crate::ports::PublicPortRange,
    ) -> Result<(), String> {
        let mut active = self.native_port_range.write();
        if active.is_some() {
            return Err("public port range already configured".into());
        }
        *active = Some(range);
        Ok(())
    }

    /// Enable DNS ownership verification before serving domain management requests.
    /// The policy cannot be disabled or replaced while this instance is running.
    ///
    /// # Errors
    /// Reject a second installation; use a coordinated restart for key rotation.
    pub fn require_domain_verification(
        &self,
        policy: Arc<crate::domain_verification::DomainVerification>,
    ) -> Result<(), String> {
        let mut active = self.domain_verification.write();
        if active.is_some() {
            return Err("DNS verification policy is already installed".into());
        }
        *active = Some(policy);
        Ok(())
    }
    /// Atomically replace scoped policies after validating the full JSON array.
    /// Rejected replacements leave active policies unchanged. In-flight requests
    /// retain their original authorization; new requests use the replacement.
    ///
    /// # Errors
    /// Reject invalid policies or a collision with the legacy admin credential.
    pub fn replace_api_key_policies(&self, json: &str) -> Result<(), String> {
        let policies = crate::keys::ApiKeyPolicy::from_json(json)?;
        crate::keys::ApiKeyPolicy::validate_legacy_admin(
            &policies,
            self.config.api_key.as_deref(),
        )?;
        *self.api_keys.write() = policies;
        Ok(())
    }

    #[must_use]
    pub fn new(store: Arc<dyn ClusterStore>, config: ControlConfig) -> Arc<Self> {
        // No global timeout: a streaming or long request is the caller's
        // business. Creation gets its own below.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        let tcp_http = reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("HTTP/1 TCP tunnel client configuration");
        Self::with_clients(store, config, http, tcp_http)
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
        Self::with_clients(store, config, http.clone(), http)
    }

    /// Custom clients, including an HTTP/1-only client for TCP upgrades.
    /// Supply the same TLS identity and trust policy to both clients.
    #[must_use]
    pub fn with_clients(
        store: Arc<dyn ClusterStore>,
        config: ControlConfig,
        http: reqwest::Client,
        tcp_http: reqwest::Client,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            http,
            tcp_http,
            api_keys: parking_lot::RwLock::new(config.api_keys.clone()),
            domain_verification: parking_lot::RwLock::new(None),
            config,
            native_port_range: parking_lot::RwLock::new(None),
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
        .route("/sandbox-names/{name}", get(resolve_sandbox_name))
        .route(
            "/sandboxes/{id}/names/{name}",
            axum::routing::put(assign_sandbox_name),
        )
        .route("/sandboxes", post(create_v1).get(list_v1))
        .route("/v2/sandboxes", post(create_v2).get(list_v2))
        .route("/sandboxes/{id}", get(detail).delete(forward))
        .route("/sandboxes/{id}/connect", post(forward))
        .route("/v2/sandboxes/{id}/connect", post(forward))
        .route("/sandboxes/{id}/timeout", post(forward))
        .route("/sandboxes/{id}/pause", post(forward))
        .route("/sandboxes/{id}/resume", post(forward))
        .route("/sandboxes/{id}/fork", post(forward))
        .route("/sandboxes/{id}/owner", post(adopt_owner))
        .route(
            "/sandboxes/{id}/private-networks",
            get(get_private_membership)
                .put(update_private_membership)
                .layer(axum::extract::DefaultBodyLimit::max(4096)),
        )
        .route(
            "/sandboxes/{id}/web-sharing",
            get(get_web_sharing)
                .put(update_web_sharing)
                .layer(axum::extract::DefaultBodyLimit::max(65536)),
        )
        .route("/sandboxes/{id}/public-ports", get(list_public_ports))
        .route(
            "/sandboxes/{id}/public-ports/{port}",
            axum::routing::put(reserve_public_port).delete(remove_public_port),
        )
        .route("/sandboxes/{id}/domains", get(list_domains))
        .route(
            "/sandboxes/{id}/domains/{domain}/challenge",
            get(domain_challenge),
        )
        .route(
            "/sandboxes/{id}/domains/{domain}",
            axum::routing::put(bind_domain).delete(unbind_domain),
        )
        .route("/sandboxes/{id}/checkpoints", get(forward).post(forward))
        .route(
            "/sandboxes/{id}/checkpoints/{name}",
            axum::routing::delete(forward),
        )
        .route("/sandboxes/{id}/checkpoints/{name}/restore", post(forward))
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
        .merge(crate::events::router(Arc::clone(&control.store)))
        .route("/volumes", get(list_volumes).post(to_volume_node))
        .route("/volumes/{id}", get(to_volume_node).delete(to_volume_node))
        .route("/sandboxes/{id}/refreshes", post(forward))
        .route("/sandboxes/{id}/network", any(forward))
        .route("/sandboxes/{id}/network/decisions", get(forward))
        .route("/sandboxes/{id}/exec", post(forward))
        .route("/sandboxes/{id}/ports/{port}/tcp", get(tcp_tunnel))
        .route("/sandboxes/{id}/ports/{port}/udp", get(udp_tunnel))
        .route("/sandboxes/{id}/ports/{port}/udp6", get(udp_tunnel_ipv6))
        .route("/cluster/nodes", get(cluster_nodes))
        .route(
            "/cluster/nodes/{node}/registrations/pending",
            get(pending_on_node),
        )
        .route(
            "/cluster/nodes/{node}/sandboxes/{id}/registration/reconcile",
            post(reconcile_on_node),
        )
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
        .route(
            "/.well-known/openid-configuration",
            get(openid_configuration),
        )
        .route("/ui", get(ui))
        // Without the key: the SDK sends none with an upload. The node
        // checks the token its authenticated link carried.
        .route(
            "/templates/{id}/files/{hash}",
            axum::routing::put(to_builder),
        )
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainPort {
    port: u16,
    #[serde(default)]
    challenge_expires_at: Option<u64>,
}

async fn domain_challenge(
    State(control): State<Arc<ControlPlane>>,
    Path((id, domain)): Path<(String, String)>,
) -> Response {
    let domain = match DomainName::parse(&domain) {
        Ok(domain) => domain,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    match control.store.sandbox(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
    let policy = control.domain_verification.read().clone();
    let Some(policy) = policy else {
        return api_error(
            StatusCode::NOT_IMPLEMENTED,
            "DNS ownership verification is not configured",
        );
    };
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(now) => now.as_secs(),
        Err(_) => return api_error(StatusCode::SERVICE_UNAVAILABLE, "system clock is invalid"),
    };
    match policy.challenge(&domain, &id, now) {
        Ok(challenge) => Json(challenge).into_response(),
        Err(e) => api_error(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrivateMembershipRequest {
    expected_revision: Option<String>,
    revision: String,
    tags: Vec<crate::private_networks::NetworkTag>,
}
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
fn private_membership_context(
    principal: &CreatorPrincipal,
    id: &str,
) -> Result<crate::ownership::OwnerId, Response> {
    let owner = principal.0.clone().ok_or_else(|| {
        api_error(
            StatusCode::FORBIDDEN,
            "configured creator identity required",
        )
    })?;
    crate::ports::validate_request(id, 1, owner.as_str())
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid sandbox ID"))?;
    Ok(owner)
}
fn private_membership_json(
    state: Option<&crate::private_networks::NetworkMembershipState>,
) -> Value {
    let tags: Vec<_> = state
        .and_then(|state| state.membership())
        .map(|member| member.tags().iter().map(|tag| tag.as_str()).collect())
        .unwrap_or_default();
    json!({"revision":state.map(|state|state.revision()),"tags":tags})
}
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
async fn private_membership_store<T>(
    future: impl std::future::Future<Output = crate::store::Result<T>>,
) -> Result<T, Response> {
    match tokio::time::timeout(Duration::from_secs(5), future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "private membership store unavailable; read current state before retrying",
        )),
        Err(_) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "private membership outcome uncertain; retry the exact revision and request",
        )),
    }
}
async fn get_private_membership(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path(id): Path<String>,
) -> Response {
    use crate::private_networks::MembershipAccess;
    let owner = match private_membership_context(&principal, &id) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match private_membership_store(control.store.private_membership(&id, &owner)).await {
        Ok(MembershipAccess::Granted(state)) => {
            Json(private_membership_json(state.as_ref())).into_response()
        }
        Ok(MembershipAccess::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(MembershipAccess::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(response) => response,
    }
}
async fn update_private_membership(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path(id): Path<String>,
    Json(body): Json<PrivateMembershipRequest>,
) -> Response {
    use crate::private_networks::{MembershipChange, NetworkMembershipState};
    let owner = match private_membership_context(&principal, &id) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    if body.expected_revision.as_ref().is_some_and(|revision| {
        !uuid::Uuid::parse_str(revision)
            .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == *revision)
    }) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid expected membership revision",
        );
    }
    let record = match private_membership_store(control.store.sandbox(&id)).await {
        Ok(Some(record)) if record.sandbox_id != id => {
            return api_error(StatusCode::SERVICE_UNAVAILABLE, "sandbox identity mismatch")
        }
        Ok(Some(record)) if record.owner_id.as_ref() == Some(&owner) => record,
        Ok(Some(_)) => {
            return api_error(
                StatusCode::FORBIDDEN,
                "sandbox belongs to a different or unassigned creator",
            )
        }
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
        Err(response) => return response,
    };
    let tags = if body.tags.is_empty() {
        None
    } else {
        Some(body.tags)
    };
    let next = match NetworkMembershipState::with_revision(&record, tags, &body.revision) {
        Ok(next) => next,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error),
    };
    match private_membership_store(
        control
            .store
            .compare_private_membership(body.expected_revision.as_deref(), &next),
    )
    .await
    {
        Ok(MembershipChange::Applied) => Json(private_membership_json(Some(&next))).into_response(),
        Ok(MembershipChange::RevisionConflict | MembershipChange::RecordChanged) => api_error(
            StatusCode::CONFLICT,
            "membership revision or sandbox incarnation changed; read current state",
        ),
        Ok(MembershipChange::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(MembershipChange::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(response) => response,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WebSharingRequest {
    expected_revision: Option<String>,
    revision: String,
    grants: Vec<crate::web_sharing::WebGrant>,
}
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
fn web_sharing_context(
    principal: &CreatorPrincipal,
    id: &str,
) -> Result<crate::ownership::OwnerId, Response> {
    let owner = principal.0.clone().ok_or_else(|| {
        api_error(
            StatusCode::FORBIDDEN,
            "configured creator identity required",
        )
    })?;
    crate::ports::validate_request(id, 1, owner.as_str())
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid sandbox ID"))?;
    Ok(owner)
}
fn web_sharing_json(state: Option<&crate::web_sharing::WebSharingState>) -> Value {
    json!({"revision":state.map(|state|state.revision()),
        "grants":state.map(|state|state.grants()).unwrap_or_default()})
}
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
async fn web_sharing_store<T>(
    future: impl std::future::Future<Output = crate::store::Result<T>>,
) -> Result<T, Response> {
    match tokio::time::timeout(Duration::from_secs(5), future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "web sharing store unavailable; read current state before retrying",
        )),
        Err(_) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "web sharing outcome uncertain; retry the exact revision and request",
        )),
    }
}
async fn get_web_sharing(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path(id): Path<String>,
) -> Response {
    use crate::web_sharing::SharingAccess;
    let owner = match web_sharing_context(&principal, &id) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match web_sharing_store(control.store.web_sharing(&id, &owner)).await {
        Ok(SharingAccess::Granted(state)) => Json(web_sharing_json(state.as_ref())).into_response(),
        Ok(SharingAccess::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(SharingAccess::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(response) => response,
    }
}
async fn update_web_sharing(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path(id): Path<String>,
    Json(body): Json<WebSharingRequest>,
) -> Response {
    use crate::web_sharing::{SharingChange, WebSharingState};
    let owner = match web_sharing_context(&principal, &id) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    if body.expected_revision.as_ref().is_some_and(|revision| {
        !uuid::Uuid::parse_str(revision)
            .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == *revision)
    }) {
        return api_error(StatusCode::BAD_REQUEST, "invalid expected sharing revision");
    }
    let record = match web_sharing_store(control.store.sandbox(&id)).await {
        Ok(Some(record)) if record.sandbox_id != id => {
            return api_error(StatusCode::SERVICE_UNAVAILABLE, "sandbox identity mismatch")
        }
        Ok(Some(record)) if record.owner_id.as_ref() == Some(&owner) => record,
        Ok(Some(_)) => {
            return api_error(
                StatusCode::FORBIDDEN,
                "sandbox belongs to a different or unassigned creator",
            )
        }
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
        Err(response) => return response,
    };
    let next = match WebSharingState::with_revision(&record, body.grants, &body.revision) {
        Ok(next) => next,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error),
    };
    match web_sharing_store(
        control
            .store
            .compare_web_sharing(body.expected_revision.as_deref(), &next),
    )
    .await
    {
        Ok(SharingChange::Applied) => Json(web_sharing_json(Some(&next))).into_response(),
        Ok(SharingChange::RevisionConflict | SharingChange::RecordChanged) => api_error(
            StatusCode::CONFLICT,
            "sharing revision or sandbox incarnation changed; read current state",
        ),
        Ok(SharingChange::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(SharingChange::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(response) => response,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicPortRequest {
    #[serde(default = "default_public_protocol")]
    protocol: crate::ports::PortProtocol,
}
fn default_public_protocol() -> crate::ports::PortProtocol {
    crate::ports::PortProtocol::Tcp
}
fn public_port_json(row: &crate::ports::PortAllocation) -> Value {
    json!({"machinePort":row.machine_port(),"publicPort":row.public_port(),"protocol":row.protocol()})
}
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
fn public_port_context(
    control: &ControlPlane,
    principal: &CreatorPrincipal,
    id: &str,
    port: u16,
) -> Result<(crate::ownership::OwnerId, crate::ports::PublicPortRange), Response> {
    let owner = principal.0.clone().ok_or_else(|| {
        api_error(
            StatusCode::FORBIDDEN,
            "configured creator identity required",
        )
    })?;
    crate::ports::validate_request(id, port, owner.as_str()).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid sandbox or destination port",
        )
    })?;
    let range = (*control.native_port_range.read()).ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "public port management is not enabled",
        )
    })?;
    Ok((owner, range))
}
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
async fn public_port_store<T>(
    future: impl std::future::Future<Output = crate::store::Result<T>>,
) -> Result<T, Response> {
    match tokio::time::timeout(Duration::from_secs(5), future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "public port store unavailable",
        )),
        Err(_) => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "public port operation timed out; outcome may be committed; retry the same request",
        )),
    }
}
async fn reserve_public_port(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path((id, port)): Path<(String, u16)>,
    Json(body): Json<PublicPortRequest>,
) -> Response {
    use crate::ports::PortClaim;
    let (owner, range) = match public_port_context(&control, &principal, &id, port) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match public_port_store(control.store.claim_owned_port(
        &id,
        port,
        owner.as_str(),
        body.protocol,
        range,
    ))
    .await
    {
        Ok(PortClaim::Allocated(row)) => {
            (StatusCode::ACCEPTED, Json(public_port_json(&row))).into_response()
        }
        Ok(PortClaim::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(PortClaim::SandboxMissing) => api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
        Ok(PortClaim::LimitReached) => {
            api_error(StatusCode::CONFLICT, "sandbox public port limit reached")
        }
        Ok(PortClaim::PoolExhausted) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "public port pool exhausted",
        ),
        Err(response) => response,
    }
}
async fn list_public_ports(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path(id): Path<String>,
) -> Response {
    use crate::ports::OwnedPortAccess;
    let (owner, _) = match public_port_context(&control, &principal, &id, 1) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match public_port_store(control.store.owned_ports(&id, owner.as_str())).await {
        Ok(OwnedPortAccess::Granted(rows)) => {
            Json(rows.iter().map(public_port_json).collect::<Vec<_>>()).into_response()
        }
        Ok(OwnedPortAccess::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(OwnedPortAccess::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(response) => response,
    }
}
async fn remove_public_port(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Path((id, port)): Path<(String, u16)>,
) -> Response {
    use crate::ports::OwnedPortAccess;
    let (owner, _) = match public_port_context(&control, &principal, &id, port) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match public_port_store(control.store.delete_owned_port(&id, port, owner.as_str())).await {
        Ok(OwnedPortAccess::Granted(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(OwnedPortAccess::Granted(false)) => api_error(
            StatusCode::NOT_FOUND,
            "public port reservation does not exist",
        ),
        Ok(OwnedPortAccess::OwnerConflict) => api_error(
            StatusCode::FORBIDDEN,
            "sandbox belongs to a different or unassigned creator",
        ),
        Ok(OwnedPortAccess::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(response) => response,
    }
}

async fn list_domains(
    State(control): State<Arc<ControlPlane>>,
    Path(id): Path<String>,
) -> Response {
    match control.store.sandbox(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
    match control.store.domains(&id).await {
        Ok(bindings) => Json(bindings).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn bind_domain(
    State(control): State<Arc<ControlPlane>>,
    Path((id, domain)): Path<(String, String)>,
    Json(body): Json<DomainPort>,
) -> Response {
    let binding = match DomainBinding::new(&domain, &id, body.port) {
        Ok(binding) => binding,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    let policy = control.domain_verification.read().clone();
    if let Some(policy) = policy {
        match control.store.sandbox(&id).await {
            Ok(Some(_)) => {}
            Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
            Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
        let Some(expires) = body.challenge_expires_at else {
            return api_error(
                StatusCode::PRECONDITION_REQUIRED,
                "DNS ownership challenge is required",
            );
        };
        if let Err(e) = policy.verify(binding.domain(), &id, expires).await {
            let status = match e {
                crate::domain_verification::VerificationError::InvalidProof(_) => {
                    StatusCode::FORBIDDEN
                }
                crate::domain_verification::VerificationError::Unavailable(_) => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            };
            return api_error(status, e);
        }
    }
    match control.store.claim_domain(&binding).await {
        Ok(DomainClaim::Claimed) => Json(binding).into_response(),
        Ok(DomainClaim::Conflict) => api_error(
            StatusCode::CONFLICT,
            "domain is already bound to another sandbox",
        ),
        Ok(DomainClaim::SandboxMissing) => {
            api_error(StatusCode::NOT_FOUND, "sandbox does not exist")
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn unbind_domain(
    State(control): State<Arc<ControlPlane>>,
    Path((id, domain)): Path<(String, String)>,
) -> Response {
    let domain = match DomainName::parse(&domain) {
        Ok(domain) => domain,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    match control.store.delete_domain(&domain, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => api_error(
            StatusCode::NOT_FOUND,
            "domain binding does not exist for this sandbox",
        ),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn assign_sandbox_name(
    State(control): State<Arc<ControlPlane>>,
    Path((id, raw_name)): Path<(String, String)>,
) -> Response {
    use crate::names::{NameReservation, SandboxName};
    let name = match SandboxName::parse(&raw_name) {
        Ok(name) => name,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error),
    };
    match control.store.sandbox(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox is missing"),
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "sandbox lookup unavailable",
            )
        }
    }
    match control.store.sandboxes().await {
        Ok(records)
            if records.iter().any(|record| {
                record.sandbox_id != id
                    && record
                        .metadata
                        .get("hm.name")
                        .is_some_and(|value| value == name.as_str())
            }) =>
        {
            return api_error(
                StatusCode::CONFLICT,
                "name conflicts with legacy sandbox metadata",
            );
        }
        Ok(_) => {}
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "legacy name lookup unavailable",
            )
        }
    }
    let reservation = NameReservation::pending(name.clone());
    match control.store.reserve_name(&reservation).await {
        Ok(true) => {}
        Ok(false) => {
            return match control.store.name_reservation(&name).await {
                Ok(Some(existing))
                    if existing.name() == &name && existing.sandbox_id() == Some(id.as_str()) =>
                {
                    Json(json!({"name":name.as_str(),"sandboxID":id})).into_response()
                }
                Ok(_) => api_error(
                    StatusCode::CONFLICT,
                    "name is owned or requires reconciliation",
                ),
                Err(_) => api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "name ownership lookup unavailable",
                ),
            };
        }
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "name reservation unavailable",
            )
        }
    }
    match control
        .store
        .bind_name(&name, reservation.operation_token(), &id)
        .await
    {
        Ok(true) => Json(json!({"name":name.as_str(),"sandboxID":id})).into_response(),
        Ok(false) => {
            // A false atomic bind did not assign ownership. Conditional release
            // cannot remove a binding if another operation has since completed it.
            match control
                .store
                .release_pending_name(&name, reservation.operation_token())
                .await
            {
                Ok(_) => api_error(StatusCode::CONFLICT, "sandbox or name ownership changed"),
                Err(_) => api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "name reconciliation required",
                ),
            }
        }
        // A failed store call has unknown commit outcome; retain ownership.
        Err(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "name reconciliation required",
        ),
    }
}

async fn resolve_sandbox_name(
    State(control): State<Arc<ControlPlane>>,
    Extension(access): Extension<SandboxAccess>,
    Path(name): Path<String>,
) -> Response {
    let name = match crate::names::SandboxName::parse(&name) {
        Ok(name) => name,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error),
    };
    let reservation = match control.store.name_reservation(&name).await {
        Ok(Some(reservation)) if reservation.name() == &name => reservation,
        Ok(Some(_)) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "name ownership record mismatch",
            )
        }
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "name is not reserved"),
        Err(_) => return api_error(StatusCode::SERVICE_UNAVAILABLE, "name lookup unavailable"),
    };
    let Some(id) = reservation.sandbox_id() else {
        return api_error(
            StatusCode::CONFLICT,
            "name creation outcome requires reconciliation",
        );
    };
    // Legacy metadata is still writable until creation migration is enforced.
    // Refuse observed ambiguity rather than hiding it behind a reservation.
    match control.store.sandboxes().await {
        Ok(records)
            if records.iter().any(|record| {
                record.sandbox_id != id
                    && record
                        .metadata
                        .get("hm.name")
                        .is_some_and(|value| value == name.as_str())
            }) =>
        {
            return api_error(
                StatusCode::CONFLICT,
                "name conflicts with legacy sandbox metadata",
            );
        }
        Ok(_) => {}
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "legacy name lookup unavailable",
            )
        }
    }
    match control.store.sandbox(id).await {
        Ok(Some(record)) if access.allows(&record) => {
            Json(json!({"name": name.as_str(), "sandboxID": id})).into_response()
        }
        Ok(Some(_)) => api_error(StatusCode::FORBIDDEN, "sandbox owner required"),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "named sandbox is missing"),
        Err(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "named sandbox lookup unavailable",
        ),
    }
}

#[derive(Clone)]
struct CreatorPrincipal(Option<crate::ownership::OwnerId>);
/// The creating key's team, recorded on what it creates.
#[derive(Clone)]
struct CreatorTeam(Option<crate::ownership::TeamId>);
/// Which sandboxes a caller may reach.
#[derive(Clone)]
enum SandboxAccess {
    /// Every sandbox: an administrator, or a deployment without owners.
    All,
    /// Only those it created: a key with a principal and no team.
    Owner(crate::ownership::OwnerId),
    /// Every sandbox in its team, whoever created it.
    Team(crate::ownership::TeamId),
}
impl SandboxAccess {
    fn allows(&self, record: &SandboxRecord) -> bool {
        match self {
            Self::All => true,
            Self::Owner(owner) => record.owner_id.as_ref() == Some(owner),
            Self::Team(team) => record.team_id.as_ref() == Some(team),
        }
    }
}

/// Routes a team key may not use until their resources are partitioned by
/// team: each would read or write a namespace every tenant shares.
fn shared_across_teams(route: &str, method: &axum::http::Method) -> bool {
    use axum::http::Method;
    match route {
        "/templates" => *method != Method::GET && *method != Method::HEAD,
        "/sandboxes/{id}/snapshots" | "/cluster/events" => true,
        _ => [
            "/snapshots",
            "/templates/{id}",
            "/v2/templates",
            "/v3/templates",
            "/volumes",
        ]
        .iter()
        .any(|prefix| route.starts_with(prefix)),
    }
}
#[derive(Clone, Copy)]
struct AdministratorContext(bool);

fn audit_event(mut event: Value) -> Value {
    if event["sandbox_ref"].is_null() {
        event.as_object_mut().unwrap().remove("sandbox_ref");
    }
    event
}

async fn require_api_key(
    State(control): State<Arc<ControlPlane>>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let request_id = uuid::Uuid::new_v4();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unknown", MatchedPath::as_str)
        .to_owned();
    let method = match request.method().as_str() {
        "GET" => "GET",
        "HEAD" => "HEAD",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "PATCH" => "PATCH",
        "OPTIONS" => "OPTIONS",
        "CONNECT" => "CONNECT",
        "TRACE" => "TRACE",
        _ => "OTHER",
    };
    let (kind, key_id, mut rejection, principal, team, administrator) = {
        let policies = control.api_keys.read();
        let (kind, key_id, rejection) = authorize(&control.config, &policies, &request);
        let matched = (kind == "scoped" && rejection.is_none())
            .then(|| {
                use sha2::{Digest, Sha256};
                let digest = Sha256::digest(
                    request
                        .headers()
                        .get("x-api-key")
                        .map(HeaderValue::as_bytes)
                        .unwrap_or_default(),
                )
                .into();
                policies
                    .iter()
                    .find(|policy| policy.has_digest(&digest))
                    .cloned()
            })
            .flatten();
        let principal = matched.as_ref().and_then(|p| p.principal_id()).cloned();
        let team = matched.as_ref().and_then(|p| p.team_id()).cloned();
        let administrator = rejection.is_none()
            && (kind == "legacy_admin" || matched.as_ref().is_some_and(|p| p.is_administrator()));
        (kind, key_id, rejection, principal, team, administrator)
    };
    // Authentication supplies this context. A client cannot choose its owner
    // or its team.
    request.headers_mut().remove(crate::ownership::OWNER_HEADER);
    request.headers_mut().remove(crate::ownership::TEAM_HEADER);
    let access = match (administrator, &team, &principal) {
        (true, _, _) => SandboxAccess::All,
        (false, Some(team), _) => SandboxAccess::Team(team.clone()),
        (false, None, Some(owner)) => SandboxAccess::Owner(owner.clone()),
        (false, None, None) => SandboxAccess::All,
    };
    if rejection.is_none()
        && matches!(access, SandboxAccess::Team(_))
        && shared_across_teams(&route, request.method())
    {
        rejection = Some(api_error(
            StatusCode::FORBIDDEN,
            "this resource is shared by every team and is not available to team keys",
        ));
    }
    // A teammate acts on a sandbox as its creator, which is who the store's
    // own ownership checks (ports, sharing, private networks) are written
    // for: authorization is the team check below, and those checks still
    // fence an ownership change mid-request.
    let mut acting_owner = None;
    if rejection.is_none()
        && !matches!(access, SandboxAccess::All)
        && (route.starts_with("/sandboxes/{id}")
            || route.starts_with("/v2/sandboxes/{id}")
            || route == "/events/sandboxes/{id}")
    {
        use axum::extract::FromRequestParts;
        let (mut parts, body) = request.into_parts();
        let id = Path::<HashMap<String, String>>::from_request_parts(&mut parts, &())
            .await
            .ok()
            .and_then(|Path(params)| params.get("id").cloned());
        request = Request::from_parts(parts, body);
        rejection = match id {
            Some(id) if crate::ports::validate_request(&id, 1, "access").is_ok() => {
                match tokio::time::timeout(Duration::from_secs(5), control.store.sandbox(&id)).await
                {
                    Ok(Ok(Some(record))) if record.sandbox_id != id => Some(api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "sandbox ownership lookup unavailable",
                    )),
                    Ok(Ok(Some(record))) if access.allows(&record) => {
                        acting_owner = Some(record.owner_id);
                        None
                    }
                    Ok(Ok(Some(_))) => {
                        Some(api_error(StatusCode::FORBIDDEN, "sandbox owner required"))
                    }
                    Ok(Ok(None)) => {
                        Some(api_error(StatusCode::NOT_FOUND, "sandbox does not exist"))
                    }
                    _ => Some(api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "sandbox ownership lookup unavailable",
                    )),
                }
            }
            _ => Some(api_error(StatusCode::BAD_REQUEST, "invalid sandbox ID")),
        };
    }
    let principal = match (&access, acting_owner) {
        (SandboxAccess::Team(_), Some(owner)) => owner,
        _ => principal,
    };
    request.extensions_mut().insert(access);
    request.extensions_mut().insert(CreatorPrincipal(principal));
    request
        .extensions_mut()
        .insert(crate::ownership::RequestTeam(team.clone()));
    request.extensions_mut().insert(CreatorTeam(team));
    request
        .extensions_mut()
        .insert(AdministratorContext(administrator));
    let allowed = rejection.is_none();
    let sandbox_ref = if let Some(audit) = &control.config.access_audit {
        if audit.resource_attribution_enabled()
            && (route.starts_with("/sandboxes/{id}") || route.starts_with("/v2/sandboxes/{id}"))
        {
            use axum::extract::FromRequestParts;
            let (mut parts, body) = request.into_parts();
            let target = Path::<HashMap<String, String>>::from_request_parts(&mut parts, &())
                .await
                .ok()
                .and_then(|Path(params)| {
                    params.get("id").and_then(|id| audit.sandbox_reference(id))
                });
            request = Request::from_parts(parts, body);
            target
        } else {
            None
        }
    } else {
        None
    };

    if let Some(audit) = &control.config.access_audit {
        if audit
            .append(audit_event(
                json!({"phase":"admission", "request_id":request_id,
            "route":route, "method":method, "principal":kind, "key_id":key_id,
            "allowed":allowed, "sandbox_ref":sandbox_ref}),
            ))
            .await
            .is_err()
        {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "access audit unavailable; request was not dispatched",
            );
        }
    }
    let response = match rejection {
        Some(response) => response,
        None => {
            tracing::info!(target: "hv2_cluster::access", %request_id, route, method,
                principal = kind, key_id, "control-plane access started");
            next.run(request).await
        }
    };
    tracing::info!(target: "hv2_cluster::access", %request_id, route, method,
        principal = kind, key_id, allowed, status = response.status().as_u16(),
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        "control-plane access completed");
    if let Some(audit) = &control.config.access_audit {
        if audit
            .append(audit_event(
                json!({"phase":"completion", "request_id":request_id,
            "route":route, "method":method, "principal":kind, "key_id":key_id,
            "allowed":allowed, "sandbox_ref":sandbox_ref, "status":response.status().as_u16(),
            "elapsed_us":u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)}),
            ))
            .await
            .is_err()
        {
            return api_error(StatusCode::SERVICE_UNAVAILABLE, "access audit unavailable after dispatch; operation outcome may already be committed");
        }
    }
    response
}

/// Identity is a category plus a short digest of a configured credential.
/// Never log unknown credential digests, headers, bodies, query strings or IDs
/// supplied in paths. Library callers get scoped precedence for collisions.
fn authorize(
    config: &ControlConfig,
    api_keys: &[crate::keys::ApiKeyPolicy],
    request: &Request,
) -> (&'static str, String, Option<Response>) {
    use sha2::{Digest, Sha256};
    use subtle::ConstantTimeEq;
    let fingerprint = |digest: &[u8; 32]| -> String {
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    if config.api_key.is_some() || !api_keys.is_empty() {
        let sent = request
            .headers()
            .get("x-api-key")
            .map(HeaderValue::as_bytes)
            .unwrap_or_default();
        let digest: [u8; 32] = Sha256::digest(sent).into();
        // Library callers can construct a conflicting configuration without
        // the binary's startup validation. Never turn a scoped key into an
        // unrestricted, non-expiring credential in that case.
        let now = chrono::Utc::now().timestamp();
        let policy = api_keys.iter().find(|policy| policy.has_digest(&digest));
        match policy {
            Some(policy) if !sent.is_empty() => {
                if !policy.matches(&digest, now) {
                    return (
                        "expired",
                        fingerprint(&digest),
                        Some(api_error(
                            StatusCode::UNAUTHORIZED,
                            "missing, expired or wrong X-API-Key",
                        )),
                    );
                }
                if !policy.permits(request.method(), request.uri().path()) {
                    return (
                        "scoped",
                        fingerprint(&digest),
                        Some(api_error(
                            StatusCode::FORBIDDEN,
                            "API key scope does not permit this operation",
                        )),
                    );
                }
                return ("scoped", fingerprint(&digest), None);
            }
            _ => {
                if let Some(key) = &config.api_key {
                    if !sent.is_empty() && bool::from(sent.ct_eq(key.as_bytes())) {
                        return ("legacy_admin", fingerprint(&digest), None);
                    }
                }
                return (
                    "unauthenticated",
                    "none".into(),
                    Some(api_error(
                        StatusCode::UNAUTHORIZED,
                        "missing, expired or wrong X-API-Key",
                    )),
                );
            }
        }
    }
    if matches!(
        request
            .uri()
            .path()
            .trim_start_matches('/')
            .split('/')
            .collect::<Vec<_>>()
            .as_slice(),
        ["sandboxes", _, "owner"]
    ) {
        return (
            "anonymous",
            "none".into(),
            Some(api_error(
                StatusCode::FORBIDDEN,
                "administrator credential required",
            )),
        );
    }
    ("anonymous", "none".into(), None)
}

async fn health(State(control): State<Arc<ControlPlane>>) -> Response {
    match control.store.nodes().await {
        Ok(nodes) => Json(json!({ "status": "ok", "nodes": nodes.len() })).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

// ── Creation ────────────────────────────────────────────────────────────────

async fn create_v1(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Extension(team): Extension<CreatorTeam>,
    body: Bytes,
) -> Response {
    create(
        &control,
        "/sandboxes",
        body,
        principal.0.as_ref(),
        team.0.as_ref(),
    )
    .await
}

async fn create_v2(
    State(control): State<Arc<ControlPlane>>,
    Extension(principal): Extension<CreatorPrincipal>,
    Extension(team): Extension<CreatorTeam>,
    body: Bytes,
) -> Response {
    create(
        &control,
        "/v2/sandboxes",
        body,
        principal.0.as_ref(),
        team.0.as_ref(),
    )
    .await
}

/// Point a node's descriptor at this control plane's proxy.
fn rewrite_descriptor(control: &ControlPlane, descriptor: &mut Value, node: &str) {
    if let Some(object) = descriptor.as_object_mut() {
        object.insert("proxyPort".into(), json!(control.config.proxy_port));
        object.insert("nodeID".into(), json!(node));
    }
}

async fn create(
    control: &ControlPlane,
    path: &str,
    body: Bytes,
    principal: Option<&crate::ownership::OwnerId>,
    team: Option<&crate::ownership::TeamId>,
) -> Response {
    let started = Instant::now();
    let response = create_inner(control, path, body, principal, team).await;
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

async fn create_inner(
    control: &ControlPlane,
    path: &str,
    body: Bytes,
    principal: Option<&crate::ownership::OwnerId>,
    team: Option<&crate::ownership::TeamId>,
) -> Response {
    if (principal.is_some() || team.is_some())
        && control
            .config
            .cluster_token
            .as_ref()
            .is_none_or(String::is_empty)
    {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "owner attribution requires authenticated cluster creation",
        );
    }
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

    // Ownership must exist before a named request can reach a node. A lost
    // response cannot establish that no VM was created, so pending ownership
    // is deliberately retained without a TTL or automatic cross-node retry.
    let name = match serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("metadata")
                .and_then(|metadata| metadata.get("hm.name"))
                .cloned()
        }) {
        None => None,
        Some(Value::String(value)) => match SandboxName::parse(&value) {
            Ok(name) => Some(name),
            Err(error) => return api_error(StatusCode::BAD_REQUEST, error),
        },
        Some(_) => return api_error(StatusCode::BAD_REQUEST, "hm.name must be a string"),
    };
    let reservation = if let Some(name) = name {
        // This refuses observed legacy conflicts. An atomic migration gate is
        // still required to exclude direct-node and older-control-plane races.
        match control.store.sandboxes().await {
            Ok(records)
                if records.iter().any(|record| {
                    record
                        .metadata
                        .get("hm.name")
                        .is_some_and(|value| value == name.as_str())
                }) =>
            {
                return api_error(
                    StatusCode::CONFLICT,
                    "name conflicts with legacy sandbox metadata",
                )
            }
            Ok(_) => {}
            Err(_) => {
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "name inventory unavailable",
                )
            }
        }
        let reservation = NameReservation::pending(name);
        match control.store.reserve_name(&reservation).await {
            Ok(true) => Some(reservation),
            Ok(false) => return api_error(StatusCode::CONFLICT, "name is already reserved"),
            Err(_) => {
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "name reservation unavailable",
                )
            }
        }
    } else {
        None
    };
    let mut refusals = Vec::new();
    for node in order {
        let mut request = control
            .http
            .post(format!("{}{path}", node.api))
            .timeout(control.config.create_timeout)
            .header("content-type", "application/json")
            .body(body.clone());
        if let Some(principal) = principal {
            let mut value = HeaderValue::from_str(principal.as_str()).expect("validated owner ID");
            value.set_sensitive(true);
            request = request.header(crate::ownership::OWNER_HEADER, value);
        }
        if let Some(team) = team {
            let mut value = HeaderValue::from_str(team.as_str()).expect("validated team ID");
            value.set_sensitive(true);
            request = request.header(crate::ownership::TEAM_HEADER, value);
        }
        if let Some(operation) = &reservation {
            let mut value = HeaderValue::from_str(operation.operation_token())
                .expect("validated operation UUID");
            value.set_sensitive(true);
            request = request.header(NAME_OPERATION_HEADER, value);
        }
        if let Some(token) = &control.config.cluster_token {
            request = request.header(CLUSTER_TOKEN_HEADER, token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                if reservation.is_some() {
                    return api_error(
                        StatusCode::BAD_GATEWAY,
                        "named creation outcome requires reconciliation",
                    );
                }
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
            if reservation.is_some() {
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "named creation outcome requires reconciliation",
                );
            }
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
        if let Some(reservation) = &reservation {
            let Some(id) = descriptor.get("sandboxID").and_then(Value::as_str) else {
                return api_error(
                    StatusCode::BAD_GATEWAY,
                    "named creation returned an invalid descriptor",
                );
            };
            match control.store.sandbox(id).await {
                Ok(Some(record))
                    if record.node_id == node.id
                        && record
                            .metadata
                            .get("hm.name")
                            .is_some_and(|value| value == reservation.name().as_str()) => {}
                Ok(_) => {
                    return api_error(
                        StatusCode::BAD_GATEWAY,
                        "named creation record does not match its response",
                    )
                }
                Err(_) => {
                    return api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "named creation record unavailable",
                    )
                }
            }
            match control
                .store
                .bind_name(reservation.name(), reservation.operation_token(), id)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    return api_error(
                        StatusCode::CONFLICT,
                        "named creation outcome requires reconciliation",
                    )
                }
                Err(_) => {
                    return api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "named creation outcome requires reconciliation",
                    )
                }
            }
        }
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
    access: &SandboxAccess,
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
        .filter(|r| access.allows(r) && metadata_matches(r, &wanted))
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
    Extension(access): Extension<SandboxAccess>,
    Query(query): Query<ListQuery>,
) -> Response {
    match matching(&control, &query, &access).await {
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
    Extension(access): Extension<SandboxAccess>,
    Query(query): Query<ListQuery>,
) -> Response {
    let records = match matching(&control, &query, &access).await {
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

async fn detail(
    State(control): State<Arc<ControlPlane>>,
    Extension(access): Extension<SandboxAccess>,
    Path(id): Path<String>,
) -> Response {
    match control.store.sandbox(&id).await {
        Ok(Some(record)) if access.allows(&record) => Json(record.detail()).into_response(),
        Ok(Some(_)) => api_error(StatusCode::FORBIDDEN, "sandbox owner required"),
        Ok(None) => api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}")),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

// ── Everything else goes to the sandbox's node ──────────────────────────────

async fn tcp_tunnel(
    State(control): State<Arc<ControlPlane>>,
    Path((id, port)): Path<(String, u16)>,
    request: Request,
) -> Response {
    port_tunnel(control, id, port, request, false, false).await
}

async fn udp_tunnel(
    State(control): State<Arc<ControlPlane>>,
    Path((id, port)): Path<(String, u16)>,
    request: Request,
) -> Response {
    port_tunnel(control, id, port, request, true, false).await
}

async fn udp_tunnel_ipv6(
    State(control): State<Arc<ControlPlane>>,
    Path((id, port)): Path<(String, u16)>,
    request: Request,
) -> Response {
    port_tunnel(control, id, port, request, true, true).await
}

async fn port_tunnel(
    control: Arc<ControlPlane>,
    id: String,
    port: u16,
    request: Request,
    udp: bool,
    ipv6: bool,
) -> Response {
    let protocol = if ipv6 {
        hv2_api::udp_tunnel::PROTOCOL_IPV6
    } else if udp {
        hv2_api::udp_tunnel::PROTOCOL
    } else {
        hv2_api::tcp_tunnel::PROTOCOL
    };
    let negotiation = if ipv6 {
        hv2_api::udp_tunnel::validate_ipv6(&request)
    } else if udp {
        hv2_api::udp_tunnel::validate(&request)
    } else {
        hv2_api::tcp_tunnel::validate(&request)
    };
    if let Err(message) = negotiation {
        return api_error(StatusCode::BAD_REQUEST, message);
    }
    if port == 0 {
        return api_error(StatusCode::BAD_REQUEST, "guest port must be nonzero");
    }
    let Some(access) = request.extensions().get::<SandboxAccess>() else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sandbox access context unavailable",
        );
    };
    let record = match control.store.sandbox(&id).await {
        Ok(Some(record)) if !access.allows(&record) => {
            return api_error(StatusCode::FORBIDDEN, "sandbox owner required")
        }
        Ok(Some(record)) if !record.paused => record,
        Ok(Some(_)) => {
            return api_error(
                StatusCode::CONFLICT,
                "resume the sandbox before opening a tunnel",
            )
        }
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "sandbox does not exist"),
        Err(error) => return api_error(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let node = match control.store.node(&record.node_id).await {
        Ok(Some(node)) => node,
        Ok(None) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "sandbox node is unavailable",
            )
        }
        Err(error) => return api_error(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let path = request.uri().path();
    let mut upstream = control
        .tcp_http
        .get(format!("{}{path}", node.api))
        .version(reqwest::Version::HTTP_11)
        .header("connection", "upgrade")
        .header("upgrade", protocol);
    if let Some(token) = &control.config.cluster_token {
        upstream = upstream.header(CLUSTER_TOKEN_HEADER, token);
    }
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let response = upstream.send().await?;
        let status = response.status();
        if status != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            return Ok::<_, reqwest::Error>(Err(status));
        }
        if response
            .headers()
            .get("upgrade")
            .and_then(|v| v.to_str().ok())
            != Some(protocol)
        {
            return Ok(Err(reqwest::StatusCode::BAD_GATEWAY));
        }
        Ok(Ok(response.upgrade().await?))
    })
    .await;
    match result {
        Ok(Ok(Ok(stream))) => {
            if ipv6 {
                hv2_api::udp_tunnel::accept_ipv6(request, stream)
            } else if udp {
                hv2_api::udp_tunnel::accept(request, stream)
            } else {
                hv2_api::tcp_tunnel::accept(request, stream)
            }
        }
        Ok(Ok(Err(status))) => api_error(status, "node could not open the guest port"),
        Ok(Err(error)) => {
            tracing::debug!(%id, %error, "port tunnel node connection failed");
            api_error(
                StatusCode::BAD_GATEWAY,
                "port tunnel node connection failed",
            )
        }
        Err(_) => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "port tunnel node connection timed out",
        ),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AdoptOwnerRequest {
    principal_id: crate::ownership::OwnerId,
}

async fn adopt_owner(
    State(control): State<Arc<ControlPlane>>,
    Extension(admin): Extension<AdministratorContext>,
    Path(parameters): Path<BTreeMap<String, String>>,
    method: Method,
    uri: axum::http::Uri,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !admin.0 {
        return api_error(StatusCode::FORBIDDEN, "administrator credential required");
    }
    if control
        .config
        .cluster_token
        .as_ref()
        .is_none_or(|token| token.is_empty())
    {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "owner adoption requires authenticated cluster nodes",
        );
    }
    let parsed: AdoptOwnerRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "valid principalId required; unknown fields refused",
            )
        }
    };
    let Some(id) = parameters.get("id") else {
        return api_error(StatusCode::BAD_REQUEST, "missing sandbox ID");
    };
    if crate::ports::validate_request(id, 1, parsed.principal_id.as_str()).is_err() {
        return api_error(StatusCode::BAD_REQUEST, "invalid sandbox ID");
    }
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    forward(
        State(control),
        Extension(SandboxAccess::All),
        Path(parameters),
        method,
        uri,
        headers,
        body,
    )
    .await
}

async fn forward(
    State(control): State<Arc<ControlPlane>>,
    Extension(access): Extension<SandboxAccess>,
    Path(parameters): Path<BTreeMap<String, String>>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(id) = parameters.get("id").cloned() else {
        return api_error(StatusCode::BAD_REQUEST, "missing sandbox ID");
    };
    let record = match control.store.sandbox(&id).await {
        Ok(Some(record)) => record,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}")),
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    if !access.allows(&record) {
        return api_error(StatusCode::FORBIDDEN, "sandbox owner required");
    }
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

    // Older nodes remove the VM record without knowing about reserved names.
    // Repeat the atomic store cleanup here before acknowledging deletion; it
    // also clears ownership when the node has already removed the record.
    if method == Method::DELETE && uri.path() == format!("/sandboxes/{id}") && status.is_success() {
        if let Err(error) = control.store.delete_sandbox(&id).await {
            tracing::warn!(%id, %error, "sandbox deletion store cleanup failed");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "sandbox deletion store cleanup failed",
            );
        }
    }

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

fn valid_pending_page(value: &Value, after: Option<&str>) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 2 || !object.contains_key("nextCursor") {
        return false;
    }
    let Some(rows) = object.get("registrations").and_then(Value::as_array) else {
        return false;
    };
    if rows.len() > 32 {
        return false;
    }
    let mut previous = after;
    for row in rows {
        let Some(object) = row.as_object() else {
            return false;
        };
        if object.len() != 2 {
            return false;
        }
        let Some(id) = object.get("sandboxID").and_then(Value::as_str) else {
            return false;
        };
        if crate::ports::validate_request(id, 1, "discovery").is_err()
            || previous.is_some_and(|previous| id <= previous)
        {
            return false;
        }
        if !matches!(
            object.get("kind").and_then(Value::as_str),
            Some("named" | "unnamed")
        ) {
            return false;
        }
        previous = Some(id);
    }
    match &value["nextCursor"] {
        Value::Null => true,
        Value::String(cursor) => rows.len() == 32 && previous == Some(cursor.as_str()),
        _ => false,
    }
}

async fn pending_on_node(
    State(control): State<Arc<ControlPlane>>,
    Extension(admin): Extension<AdministratorContext>,
    Path(node_id): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if !admin.0 {
        return api_error(StatusCode::FORBIDDEN, "administrator credential required");
    }
    let Some(token) = control
        .config
        .cluster_token
        .as_ref()
        .filter(|token| !token.is_empty())
    else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "authenticated cluster required",
        );
    };
    if query.keys().any(|key| key != "after")
        || query
            .get("after")
            .is_some_and(|id| crate::ports::validate_request(id, 1, "cursor").is_err())
    {
        return api_error(StatusCode::BAD_REQUEST, "invalid registration cursor");
    }
    let node = match control.store.node(&node_id).await {
        Ok(Some(node)) => node,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "no registered node"),
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "node inventory unavailable",
            )
        }
    };
    let mut response = match control
        .http
        .get(format!("{}/registrations/pending", node.api))
        .query(&query)
        .header(CLUSTER_TOKEN_HEADER, token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return api_error(StatusCode::BAD_GATEWAY, "node discovery unavailable"),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        return api_error(status, "node discovery refused");
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if chunk.len() <= 16384usize.saturating_sub(bytes.len()) => {
                bytes.extend_from_slice(&chunk);
            }
            Ok(Some(_)) | Err(_) => {
                return api_error(StatusCode::BAD_GATEWAY, "invalid discovery response")
            }
            Ok(None) => break,
        }
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) if valid_pending_page(&value, query.get("after").map(String::as_str)) => {
            Json(value).into_response()
        }
        _ => api_error(StatusCode::BAD_GATEWAY, "invalid discovery response"),
    }
}

/// Recovery targets a registered node explicitly: initial publication may have no record.
async fn reconcile_on_node(
    State(control): State<Arc<ControlPlane>>,
    Extension(admin): Extension<AdministratorContext>,
    Path((node_id, id)): Path<(String, String)>,
) -> Response {
    if !admin.0 {
        return api_error(StatusCode::FORBIDDEN, "administrator credential required");
    }
    let Some(token) = control
        .config
        .cluster_token
        .as_ref()
        .filter(|token| !token.is_empty())
    else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "authenticated cluster required",
        );
    };
    if crate::ports::validate_request(&id, 1, "reconciliation").is_err() {
        return api_error(StatusCode::BAD_REQUEST, "invalid sandbox ID");
    }
    let node = match control.store.node(&node_id).await {
        Ok(Some(node)) => node,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "no registered node"),
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "node inventory unavailable",
            )
        }
    };
    let mut response = match control
        .http
        .post(format!(
            "{}/sandboxes/{id}/registration/reconcile",
            node.api
        ))
        .header(CLUSTER_TOKEN_HEADER, token)
        .json(&serde_json::json!({}))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return api_error(StatusCode::BAD_GATEWAY, "node reconciliation unavailable"),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        return api_error(status, "node reconciliation refused or remains uncertain");
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= 16384 => {
                bytes.extend_from_slice(&chunk);
            }
            Ok(Some(_)) | Err(_) => {
                return api_error(StatusCode::BAD_GATEWAY, "invalid reconciliation response")
            }
            Ok(None) => break,
        }
    }
    let mut descriptor: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return api_error(StatusCode::BAD_GATEWAY, "invalid reconciliation descriptor"),
    };
    if descriptor.get("sandboxID").and_then(Value::as_str) != Some(id.as_str()) {
        return api_error(
            StatusCode::BAD_GATEWAY,
            "reconciliation descriptor identity differs",
        );
    }
    rewrite_descriptor(&control, &mut descriptor, &node.id);
    (status, Json(descriptor)).into_response()
}

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
    let mut offered: BTreeMap<String, Vec<(&NodeInfo, Option<&crate::model::TemplateInfo>)>> =
        BTreeMap::new();
    for node in &nodes {
        let names = if node.templates.is_empty() {
            vec!["base".to_string()]
        } else {
            node.templates.clone()
        };
        for name in names {
            let metadata = node.template_metadata.get(&name);
            offered.entry(name).or_default().push((node, metadata));
        }
    }
    Json(
        offered
            .into_iter()
            .map(|(name, nodes)| {
                let snapshot = if nodes
                    .iter()
                    .all(|(_, metadata)| metadata.is_some_and(|m| m.snapshot))
                {
                    Some(true)
                } else if nodes
                    .iter()
                    .any(|(_, metadata)| metadata.is_some_and(|m| !m.snapshot))
                {
                    Some(false)
                } else {
                    None
                };
                let common_size =
                    nodes
                        .first()
                        .and_then(|(_, metadata)| *metadata)
                        .filter(|first| {
                            nodes.iter().all(|(_, metadata)| {
                                metadata.is_some_and(|m| {
                                    m.cpu_count == first.cpu_count && m.memory_mb == first.memory_mb
                                })
                            })
                        });
                json!({
                    "templateID": name,
                    "buildID": name,
                    "aliases": [name],
                    "public": false,
                    "buildStatus": "ready",
                    "nodeIDs": nodes.iter().map(|(node, _)| &node.id).collect::<Vec<_>>(),
                    "snapshot": snapshot,
                    "cpuCount": common_size.map(|m| m.cpu_count),
                    "memoryMB": common_size.map(|m| m.memory_mb),
                    "nodes": nodes.iter().map(|(node, metadata)| json!({
                        "nodeID": node.id,
                        "snapshot": metadata.map(|m| m.snapshot),
                        "cpuCount": metadata.map(|m| m.cpu_count),
                        "memoryMB": metadata.map(|m| m.memory_mb),
                    })).collect::<Vec<_>>(),
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
    Extension(access): Extension<SandboxAccess>,
    uri: axum::http::Uri,
) -> Response {
    let path = uri
        .path_and_query()
        .map_or("/sandboxes/metrics", axum::http::uri::PathAndQuery::as_str);
    // Only the sandboxes this caller may reach. Nodes report every sandbox
    // they run, so the store says which those are.
    let reachable: Option<std::collections::HashSet<String>> = match &access {
        SandboxAccess::All => None,
        _ => match control.store.sandboxes().await {
            Ok(records) => Some(
                records
                    .into_iter()
                    .filter(|record| access.allows(record))
                    .map(|record| record.sandbox_id)
                    .collect(),
            ),
            Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
        },
    };
    let answers = match on_every_node(&control, Method::GET, path).await {
        Ok(answers) => answers,
        Err((status, message)) => return api_error(status, message),
    };
    let mut merged = serde_json::Map::new();
    for (_, status, answer) in answers {
        if status == 200 {
            if let Some(found) = answer["sandboxes"].as_object() {
                merged.extend(
                    found
                        .iter()
                        .filter(|(id, _)| reachable.as_ref().is_none_or(|ids| ids.contains(*id)))
                        .map(|(id, value)| (id.clone(), value.clone())),
                );
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
    web_access: Option<Arc<crate::web_access::WebAccessPolicy>>,
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
            web_access: None,
            cache: Mutex::new(HashMap::new()),
            ttl,
            backend_tls: None,
        }
    }

    /// Require dedicated browser credentials on guest application URLs.
    #[must_use]
    pub fn with_web_access(mut self, policy: Arc<crate::web_access::WebAccessPolicy>) -> Self {
        self.web_access = Some(policy);
        self
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
    fn prepare_response(&self, _sandbox: &str, port: u16, headers: &mut HeaderMap) {
        if self.web_access.is_some() && port != hv2_api::sandbox_proxy::ENVD_PORT {
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
        }
    }
    fn authorize_request(
        &self,
        sandbox: &str,
        port: u16,
        headers: &mut HeaderMap,
    ) -> Result<(), hv2_api::sandbox_proxy::ProxyAccessDenied> {
        let Some(policy) = &self.web_access else {
            return Ok(());
        };
        headers.remove(crate::web_access::IDENTITY_HEADER);
        // Envd already requires its per-sandbox access token. Browser policy
        // protects guest application URLs without changing E2B SDK transport.
        if port == hv2_api::sandbox_proxy::ENVD_PORT {
            return Ok(());
        }
        let denied = || hv2_api::sandbox_proxy::ProxyAccessDenied {
            challenge: Some("Basic realm=\"HyperMachine sandbox\", charset=\"UTF-8\""),
        };
        let subject = policy
            .identity(headers, sandbox, chrono::Utc::now().timestamp())
            .ok_or_else(denied)?;
        headers.remove(axum::http::header::AUTHORIZATION);
        headers.insert(
            crate::web_access::IDENTITY_HEADER,
            subject.parse().map_err(|_| denied())?,
        );
        Ok(())
    }
    async fn admit_request(
        &self,
        sandbox: &str,
        port: u16,
        headers: &mut HeaderMap,
    ) -> Result<(), hv2_api::sandbox_proxy::ProxyAccessDenied> {
        // Existing operator scopes and envd token transport retain their policy.
        let denied = match self.authorize_request(sandbox, port, headers) {
            Ok(()) => return Ok(()),
            Err(denied) => denied,
        };
        let Some(policy) = &self.web_access else {
            return Err(denied);
        };
        let Some(subject) = policy.grant_identity(headers, chrono::Utc::now().timestamp()) else {
            return Err(denied);
        };
        let snapshot = tokio::time::timeout(
            Duration::from_secs(5),
            self.store.web_sharing_snapshot(sandbox),
        )
        .await;
        let Ok(Ok(Some((record, sharing)))) = snapshot else {
            return Err(denied);
        };
        let now = chrono::Utc::now().timestamp();
        // Rotation, expiry and delegation changes during the await must deny.
        if policy.grant_identity(headers, now).as_deref() != Some(subject.as_str())
            || !sharing.allows(&record, &subject, now)
        {
            return Err(denied);
        }
        let identity = subject
            .parse()
            .map_err(|_| hv2_api::sandbox_proxy::ProxyAccessDenied {
                challenge: Some("Basic realm=\"HyperMachine sandbox\", charset=\"UTF-8\""),
            })?;
        headers.remove(axum::http::header::AUTHORIZATION);
        headers.insert(crate::web_access::IDENTITY_HEADER, identity);
        Ok(())
    }
    async fn resolve_hostname(&self, authority: &str) -> Option<(u16, String)> {
        if authority.contains('@') {
            return None;
        }
        let authority = authority.parse::<axum::http::uri::Authority>().ok()?;
        let name = DomainName::parse(authority.host()).ok()?;
        let binding = self.store.domain(&name).await.ok()??;
        // Do not let the node-address cache keep a deleted sandbox's alias alive.
        self.store.sandbox(binding.sandbox_id()).await.ok()??;
        Some((binding.port(), binding.sandbox_id().to_owned()))
    }

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

#[cfg(test)]
mod access_audit_tests {
    use super::*;
    #[test]
    fn pending_page_refuses_capabilities_and_malformed_pagination() {
        let page =
            json!({"registrations":[{"sandboxID":"sbx-001","kind":"unnamed"}],"nextCursor":null});
        assert!(valid_pending_page(&page, None));
        assert!(valid_pending_page(
            &json!({"registrations":[],"nextCursor":null}),
            None
        ));
        let mut cases = Vec::new();
        let mut value = page.clone();
        value["envdAccessToken"] = json!("secret");
        cases.push(value);
        let mut value = page.clone();
        value["registrations"][0]["envdAccessToken"] = json!("secret");
        cases.push(value);
        let mut value = page.clone();
        value["registrations"][0]["kind"] = json!("other");
        cases.push(value);
        let mut value = page.clone();
        value["registrations"][0]["sandboxID"] = json!("bad/id");
        cases.push(value);
        let mut value = page.clone();
        value["nextCursor"] = json!("sbx-001");
        cases.push(value);
        cases.push(json!({"registrations":[]}));
        cases.push(json!({"registrations":[{"sandboxID":"sbx-001","kind":"unnamed"},{"sandboxID":"sbx-001","kind":"named"}],"nextCursor":null}));
        cases.push(json!({"registrations":(0..33).map(|i|json!({"sandboxID":format!("sbx-{i:03}"),"kind":"unnamed"})).collect::<Vec<_>>(),"nextCursor":null}));
        for value in cases {
            assert!(!valid_pending_page(&value, None));
        }
        assert!(!valid_pending_page(&page, Some("sbx-001")));
        let full = json!({"registrations":(0..32).map(|i|json!({"sandboxID":format!("sbx-{i:03}"),"kind":"unnamed"})).collect::<Vec<_>>(),"nextCursor":"sbx-031"});
        assert!(valid_pending_page(&full, None));
        let mut wrong = full;
        wrong["nextCursor"] = json!("sbx-030");
        assert!(!valid_pending_page(&wrong, None));
    }

    #[tokio::test]
    async fn reconciliation_requires_administrator_cluster_auth_and_registered_node() {
        for (admin, token, expected) in [
            (false, Some("cluster"), StatusCode::FORBIDDEN),
            (true, None, StatusCode::SERVICE_UNAVAILABLE),
            (true, Some("cluster"), StatusCode::NOT_FOUND),
        ] {
            let control = ControlPlane::new(
                Arc::new(crate::store::MemoryStore::new()),
                ControlConfig {
                    api_key: Some("admin".into()),
                    api_keys: Vec::new(),
                    access_audit: None,
                    cluster_token: token.map(str::to_owned),
                    proxy_port: 5981,
                    create_timeout: Duration::from_secs(1),
                    identity_issuer: None,
                },
            );
            let response = reconcile_on_node(
                State(control),
                Extension(AdministratorContext(admin)),
                Path(("missing-node".into(), "sandbox-a".into())),
            )
            .await;
            assert_eq!(response.status(), expected);
            let control = ControlPlane::new(
                Arc::new(crate::store::MemoryStore::new()),
                ControlConfig {
                    api_key: Some("admin".into()),
                    api_keys: Vec::new(),
                    access_audit: None,
                    cluster_token: token.map(str::to_owned),
                    proxy_port: 5981,
                    create_timeout: Duration::from_secs(1),
                    identity_issuer: None,
                },
            );
            let response = pending_on_node(
                State(control),
                Extension(AdministratorContext(admin)),
                Path("missing-node".into()),
                Query(BTreeMap::new()),
            )
            .await;
            assert_eq!(response.status(), expected);
        }
    }

    use hv2_core::security::audit_chain::{AuditChain, AuditSink};
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailAt {
        writes: usize,
        fail_at: usize,
        lines: Arc<Mutex<Vec<String>>>,
    }
    impl AuditSink for FailAt {
        fn write_line(&mut self, line: &str) -> io::Result<()> {
            if self.writes == self.fail_at {
                return Err(io::Error::other("injected audit storage failure"));
            }
            self.writes += 1;
            self.lines.lock().push(line.to_owned());
            Ok(())
        }
    }

    #[tokio::test]
    async fn audit_failure_before_dispatch_blocks_mutation_and_after_dispatch_preserves_uncertainty(
    ) {
        for fail_at in [0, 1] {
            let lines = Arc::new(Mutex::new(Vec::new()));
            let audit = crate::audit::AccessAudit::with_test_chain(AuditChain::new(
                [42; 32],
                Box::new(FailAt {
                    writes: 0,
                    fail_at,
                    lines: lines.clone(),
                }),
            ));
            let control = ControlPlane::new(
                Arc::new(crate::store::MemoryStore::new()),
                ControlConfig {
                    api_key: Some("test-key".into()),
                    api_keys: Vec::new(),
                    access_audit: Some(audit),
                    cluster_token: None,
                    proxy_port: 5981,
                    create_timeout: Duration::from_secs(1),
                    identity_issuer: None,
                },
            );
            let mutations = Arc::new(AtomicUsize::new(0));
            let handler_mutations = mutations.clone();
            let app = Router::new()
                .route(
                    "/mutation",
                    post(move || {
                        let mutations = handler_mutations.clone();
                        async move {
                            mutations.fetch_add(1, Ordering::SeqCst);
                            StatusCode::CREATED
                        }
                    }),
                )
                .route_layer(axum::middleware::from_fn_with_state(
                    control,
                    require_api_key,
                ));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/mutation", listener.local_addr().unwrap());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = reqwest::Client::new();
            let response = client
                .post(&url)
                .header("x-api-key", "test-key")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let text = response.text().await.unwrap();
            assert!(text.contains(if fail_at == 0 {
                "not dispatched"
            } else {
                "may already be committed"
            }));
            assert_eq!(mutations.load(Ordering::SeqCst), fail_at);
            let response = client
                .post(&url)
                .header("x-api-key", "test-key")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(mutations.load(Ordering::SeqCst), fail_at);
            assert_eq!(lines.lock().len(), fail_at);
            task.abort();
            let _ = task.await;
        }
    }
}

#[cfg(test)]
mod private_membership_http_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn put(
        client: &reqwest::Client,
        url: &str,
        key: &str,
        body: &Value,
    ) -> reqwest::Response {
        client
            .put(url)
            .header("x-api-key", key)
            .header(crate::ownership::OWNER_HEADER, "network-alice")
            .json(body)
            .send()
            .await
            .unwrap()
    }
    async fn http_contract(store: Arc<dyn ClusterStore>) {
        let mut record = crate::store::tests::sandbox("network-api", "network-node");
        record.owner_id = Some(crate::ownership::OwnerId::parse("network-alice").unwrap());
        store.put_sandbox(&record).await.unwrap();
        let legacy = crate::store::tests::sandbox("network-legacy", "network-node");
        store.put_sandbox(&legacy).await.unwrap();
        let expires = (crate::model::now_ms() / 1000 + 3600) as i64;
        let entry = |key: &str, owner: Option<&str>, scope: &str, role: &str| {
            json!({
            "sha256":Sha256::digest(key.as_bytes()).iter().map(|b|format!("{b:02x}")).collect::<String>(),
            "expires_at":expires,"scopes":[scope],"role":role,"principal_id":owner})
        };
        let policies = crate::keys::ApiKeyPolicy::from_json(
            &json!([
                entry("alice", Some("network-alice"), "sandboxes", "operator"),
                entry(
                    "alice-rotated",
                    Some("network-alice"),
                    "sandboxes",
                    "operator"
                ),
                entry("bob", Some("network-bob"), "sandboxes", "operator"),
                entry("observer", Some("network-alice"), "admin", "observer"),
                entry("inventory", Some("network-alice"), "inventory", "operator"),
                entry("unassigned", None, "sandboxes", "operator"),
            ])
            .to_string(),
        )
        .unwrap();
        let control = ControlPlane::new(
            store.clone(),
            ControlConfig {
                api_key: Some("legacy-admin".into()),
                api_keys: policies,
                access_audit: None,
                cluster_token: Some("owned-cluster".into()),
                proxy_port: 5981,
                create_timeout: Duration::from_secs(1),
                identity_issuer: None,
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(control);
        let mut server = Server(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url = format!("http://{addr}/sandboxes/network-api/private-networks");
        let revision = uuid::Uuid::new_v4().to_string();
        let initial = json!({"expectedRevision":null,"revision":revision,"tags":["team"]});
        for (key, code) in [
            ("bad-key", 401),
            ("bob", 403),
            ("observer", 403),
            ("inventory", 403),
            ("unassigned", 403),
            ("legacy-admin", 403),
        ] {
            assert_eq!(
                put(&client, &url, key, &initial).await.status().as_u16(),
                code,
                "{key} PUT"
            );
            assert_eq!(
                client
                    .get(&url)
                    .header("x-api-key", key)
                    .header(crate::ownership::OWNER_HEADER, "network-alice")
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                code,
                "{key} GET"
            );
        }
        assert_eq!(
            client
                .get(&url)
                .header("x-api-key", "alice")
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap(),
            json!({"revision":null,"tags":[]})
        );
        assert_eq!(
            put(
                &client,
                &url.replace("network-api", "network-legacy"),
                "alice",
                &initial
            )
            .await
            .status()
            .as_u16(),
            403
        );
        assert_eq!(
            put(
                &client,
                &url.replace("network-api", "missing"),
                "alice",
                &initial
            )
            .await
            .status()
            .as_u16(),
            404
        );
        assert_eq!(
            put(&client, &url, "alice", &initial)
                .await
                .status()
                .as_u16(),
            200
        );
        let replay = put(&client, &url, "alice-rotated", &initial).await;
        assert_eq!(replay.status().as_u16(), 200);
        assert_eq!(
            replay.json::<Value>().await.unwrap(),
            json!({"revision":revision,"tags":["team"]})
        );
        // Strict JSON, canonical revisions and bounded/tag-validated input.
        for (body, code) in [
            (
                json!({"expectedRevision":revision,"revision":"invalid","tags":["team"]}),
                400,
            ),
            (
                json!({"expectedRevision":"invalid","revision":uuid::Uuid::new_v4().to_string(),"tags":["team"]}),
                400,
            ),
            (
                json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"tags":["team","team"]}),
                400,
            ),
            (
                json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"tags":["a","b","c","d","e","f","g","h","i"]}),
                400,
            ),
            (json!({"revision":uuid::Uuid::new_v4().to_string()}), 422),
            (
                json!({"revision":uuid::Uuid::new_v4().to_string(),"tags":["TEAM"]}),
                422,
            ),
            (
                json!({"revision":uuid::Uuid::new_v4().to_string(),"tags":["team"],"owner_id":"network-bob"}),
                422,
            ),
        ] {
            assert_eq!(
                put(&client, &url, "alice", &body).await.status().as_u16(),
                code
            );
        }
        assert_eq!(
            client
                .put(&url)
                .header("x-api-key", "alice")
                .header("content-type", "application/json")
                .body("x".repeat(5000))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            413
        );
        let left = json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"tags":["left"]});
        let right = json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"tags":["right"]});
        let (a, b) = tokio::join!(
            put(&client, &url, "alice", &left),
            put(&client, &url, "alice-rotated", &right)
        );
        let (a, b) = (a.status().as_u16(), b.status().as_u16());
        assert!(matches!((a, b), (200, 409) | (409, 200)), "{a}/{b}");
        let winner = if a == 200 { &left } else { &right };
        let state = client
            .get(&url)
            .header("x-api-key", "alice")
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(
            state,
            json!({"revision":winner["revision"],"tags":winner["tags"]})
        );
        let removed = json!({"expectedRevision":winner["revision"],"revision":uuid::Uuid::new_v4().to_string(),"tags":[]});
        assert_eq!(
            put(&client, &url, "alice", &removed)
                .await
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            put(&client, &url, "alice", &initial)
                .await
                .status()
                .as_u16(),
            409
        );
        assert_eq!(
            put(&client, &url, "alice-rotated", &removed)
                .await
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            client
                .get(&url)
                .header("x-api-key", "alice")
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap(),
            json!({"revision":removed["revision"],"tags":[]})
        );
        store.delete_sandbox(&record.sandbox_id).await.unwrap();
        assert_eq!(
            put(&client, &url, "alice", &removed)
                .await
                .status()
                .as_u16(),
            404
        );
        store.delete_sandbox(&legacy.sandbox_id).await.unwrap();
        server.0.abort();
        let _ = (&mut server.0).await;
    }
    #[tokio::test]
    async fn memory_membership_api_enforces_owners_revisions_and_request_bounds() {
        http_contract(Arc::new(crate::store::MemoryStore::new())).await;
    }
    #[tokio::test]
    async fn redis_membership_api_enforces_owners_revisions_and_request_bounds() {
        let Ok(url) = std::env::var("HV2_TEST_REDIS") else {
            eprintln!("skipped: set HV2_TEST_REDIS for owned Redis HTTP checks");
            return;
        };
        let namespace = format!("private-http-{}", uuid::Uuid::new_v4().simple());
        let store = crate::store::RedisStore::connect(&url, &namespace)
            .await
            .unwrap();
        http_contract(Arc::new(store)).await;
    }
}

#[cfg(test)]
mod web_sharing_http_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn put(
        client: &reqwest::Client,
        url: &str,
        key: &str,
        body: &Value,
    ) -> reqwest::Response {
        client
            .put(url)
            .header("x-api-key", key)
            .header(crate::ownership::OWNER_HEADER, "network-alice")
            .json(body)
            .send()
            .await
            .unwrap()
    }
    async fn http_contract(store: Arc<dyn ClusterStore>) {
        let mut record = crate::store::tests::sandbox("sharing-api", "network-node");
        record.owner_id = Some(crate::ownership::OwnerId::parse("network-alice").unwrap());
        store.put_sandbox(&record).await.unwrap();
        let legacy = crate::store::tests::sandbox("sharing-legacy", "network-node");
        store.put_sandbox(&legacy).await.unwrap();
        let expires = (crate::model::now_ms() / 1000 + 3600) as i64;
        let entry = |key: &str, owner: Option<&str>, scope: &str, role: &str| {
            json!({
            "sha256":Sha256::digest(key.as_bytes()).iter().map(|b|format!("{b:02x}")).collect::<String>(),
            "expires_at":expires,"scopes":[scope],"role":role,"principal_id":owner})
        };
        let policies = crate::keys::ApiKeyPolicy::from_json(
            &json!([
                entry("alice", Some("network-alice"), "sandboxes", "operator"),
                entry(
                    "alice-rotated",
                    Some("network-alice"),
                    "sandboxes",
                    "operator"
                ),
                entry("bob", Some("network-bob"), "sandboxes", "operator"),
                entry("observer", Some("network-alice"), "admin", "observer"),
                entry("inventory", Some("network-alice"), "inventory", "operator"),
                entry("unassigned", None, "sandboxes", "operator"),
            ])
            .to_string(),
        )
        .unwrap();
        let control = ControlPlane::new(
            store.clone(),
            ControlConfig {
                api_key: Some("legacy-admin".into()),
                api_keys: policies,
                access_audit: None,
                cluster_token: Some("owned-cluster".into()),
                proxy_port: 5981,
                create_timeout: Duration::from_secs(1),
                identity_issuer: None,
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(control);
        let mut server = Server(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url = format!("http://{addr}/sandboxes/sharing-api/web-sharing");
        let revision = uuid::Uuid::new_v4().to_string();
        let initial = json!({"expectedRevision":null,"revision":revision,"grants":[{"subject":"alice","expires_at":100}]});
        for (key, code) in [
            ("bad-key", 401),
            ("bob", 403),
            ("observer", 403),
            ("inventory", 403),
            ("unassigned", 403),
            ("legacy-admin", 403),
        ] {
            assert_eq!(
                put(&client, &url, key, &initial).await.status().as_u16(),
                code,
                "{key} PUT"
            );
            assert_eq!(
                client
                    .get(&url)
                    .header("x-api-key", key)
                    .header(crate::ownership::OWNER_HEADER, "network-alice")
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                code,
                "{key} GET"
            );
        }
        assert_eq!(
            client
                .get(&url)
                .header("x-api-key", "alice")
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap(),
            json!({"revision":null,"grants":[]})
        );
        assert_eq!(
            put(
                &client,
                &url.replace("sharing-api", "sharing-legacy"),
                "alice",
                &initial
            )
            .await
            .status()
            .as_u16(),
            403
        );
        assert_eq!(
            put(
                &client,
                &url.replace("sharing-api", "missing"),
                "alice",
                &initial
            )
            .await
            .status()
            .as_u16(),
            404
        );
        assert_eq!(
            put(&client, &url, "alice", &initial)
                .await
                .status()
                .as_u16(),
            200
        );
        let replay = put(&client, &url, "alice-rotated", &initial).await;
        assert_eq!(replay.status().as_u16(), 200);
        assert_eq!(
            replay.json::<Value>().await.unwrap(),
            json!({"revision":revision,"grants":[{"subject":"alice","expires_at":100}]})
        );
        // Strict JSON, canonical revisions and bounded validated grant input.
        for (body, code) in [
            (
                json!({"expectedRevision":revision,"revision":"invalid","grants":[{"subject":"alice","expires_at":100}]}),
                400,
            ),
            (
                json!({"expectedRevision":"invalid","revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"alice","expires_at":100}]}),
                400,
            ),
            (
                json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"alice","expires_at":100},{"subject":"alice","expires_at":200}]}),
                400,
            ),
            (
                json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"grants":(0..257).map(|i|json!({"subject":format!("user{i}"),"expires_at":100})).collect::<Vec<_>>()}),
                400,
            ),
            (json!({"revision":uuid::Uuid::new_v4().to_string()}), 422),
            (
                json!({"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"*","expires_at":100}]}),
                422,
            ),
            (
                json!({"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"alice","expires_at":100}],"owner_id":"network-bob"}),
                422,
            ),
        ] {
            assert_eq!(
                put(&client, &url, "alice", &body).await.status().as_u16(),
                code
            );
        }
        assert_eq!(
            client
                .put(&url)
                .header("x-api-key", "alice")
                .header("content-type", "application/json")
                .body("x".repeat(65537))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            413
        );
        let changed = json!({"expectedRevision":revision,"revision":revision,"grants":[]});
        assert_eq!(
            put(&client, &url, "alice", &changed)
                .await
                .status()
                .as_u16(),
            409
        );
        for expiry in [0, -1] {
            let invalid = json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"alice","expires_at":expiry}]});
            assert_eq!(
                put(&client, &url, "alice", &invalid)
                    .await
                    .status()
                    .as_u16(),
                422
            );
        }
        let left = json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"left","expires_at":100}]});
        let right = json!({"expectedRevision":revision,"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"right","expires_at":100}]});
        let (a, b) = tokio::join!(
            put(&client, &url, "alice", &left),
            put(&client, &url, "alice-rotated", &right)
        );
        let (a, b) = (a.status().as_u16(), b.status().as_u16());
        assert!(matches!((a, b), (200, 409) | (409, 200)), "{a}/{b}");
        let winner = if a == 200 { &left } else { &right };
        let state = client
            .get(&url)
            .header("x-api-key", "alice")
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(
            state,
            json!({"revision":winner["revision"],"grants":winner["grants"]})
        );
        let removed = json!({"expectedRevision":winner["revision"],"revision":uuid::Uuid::new_v4().to_string(),"grants":[]});
        assert_eq!(
            put(&client, &url, "alice", &removed)
                .await
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            put(&client, &url, "alice", &initial)
                .await
                .status()
                .as_u16(),
            409
        );
        assert_eq!(
            put(&client, &url, "alice-rotated", &removed)
                .await
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            client
                .get(&url)
                .header("x-api-key", "alice")
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap(),
            json!({"revision":removed["revision"],"grants":[]})
        );
        store.delete_sandbox(&record.sandbox_id).await.unwrap();
        assert_eq!(
            put(&client, &url, "alice", &removed)
                .await
                .status()
                .as_u16(),
            404
        );
        store.delete_sandbox(&legacy.sandbox_id).await.unwrap();
        server.0.abort();
        let _ = (&mut server.0).await;
    }
    #[tokio::test]
    async fn memory_sharing_api_enforces_owners_revisions_and_request_bounds() {
        http_contract(Arc::new(crate::store::MemoryStore::new())).await;
    }
    #[tokio::test]
    async fn redis_sharing_api_enforces_owners_revisions_and_request_bounds() {
        let Ok(url) = std::env::var("HV2_TEST_REDIS") else {
            eprintln!("skipped: set HV2_TEST_REDIS for owned Redis HTTP checks");
            return;
        };
        let namespace = format!("sharing-http-{}", uuid::Uuid::new_v4().simple());
        let store = crate::store::RedisStore::connect(&url, &namespace)
            .await
            .unwrap();
        http_contract(Arc::new(store)).await;
    }
}

#[cfg(test)]
mod private_membership_failure_tests {
    use super::*;
    use crate::domains::{DomainBinding, DomainName};
    use crate::model::{ClusterEvent, Delivery, Webhook};
    use crate::private_networks::{MembershipAccess, MembershipChange, NetworkMembershipState};
    use crate::store::{MemoryStore, Result as StoreResult, StoreError};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    struct FaultStore {
        inner: MemoryStore,
        mode: AtomicU8,
        cancelled: Arc<AtomicUsize>,
    }
    struct Pending(Arc<AtomicUsize>);
    impl Drop for Pending {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl FaultStore {
        async fn hang<T>(&self) -> T {
            let _pending = Pending(self.cancelled.clone());
            std::future::pending().await
        }
        fn error<T>() -> StoreResult<T> {
            Err(StoreError(
                "injected store fault with private internal detail".into(),
            ))
        }
    }
    #[async_trait::async_trait]
    impl ClusterStore for FaultStore {
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
        async fn register_named_sandbox(
            &self,
            record: &SandboxRecord,
            reservation: &NameReservation,
        ) -> StoreResult<bool> {
            self.inner.register_named_sandbox(record, reservation).await
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
        async fn name_reservation(
            &self,
            name: &SandboxName,
        ) -> StoreResult<Option<NameReservation>> {
            self.inner.name_reservation(name).await
        }
        async fn bind_name(
            &self,
            name: &SandboxName,
            token: &str,
            sandbox: &str,
        ) -> StoreResult<bool> {
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
        async fn sandbox(&self, id: &str) -> StoreResult<Option<SandboxRecord>> {
            match self.mode.load(Ordering::SeqCst) {
                3 => Self::error(),
                4 => self.hang().await,
                _ => self.inner.sandbox(id).await,
            }
        }
        async fn private_membership(
            &self,
            sandbox: &str,
            owner: &crate::ownership::OwnerId,
        ) -> StoreResult<MembershipAccess<Option<NetworkMembershipState>>> {
            match self.mode.load(Ordering::SeqCst) {
                1 => Self::error(),
                2 => self.hang().await,
                _ => self.inner.private_membership(sandbox, owner).await,
            }
        }
        async fn compare_private_membership(
            &self,
            expected: Option<&str>,
            next: &NetworkMembershipState,
        ) -> StoreResult<MembershipChange> {
            let mode = self.mode.load(Ordering::SeqCst);
            match mode {
                5 => return Self::error(),
                6 => return self.hang().await,
                _ => {}
            }
            let result = self
                .inner
                .compare_private_membership(expected, next)
                .await?;
            if result == MembershipChange::Applied {
                match mode {
                    7 => return self.hang().await,
                    8 => return Self::error(),
                    _ => {}
                }
            }
            Ok(result)
        }
    }
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    #[tokio::test]
    async fn http_store_failures_preserve_uncertainty_and_exact_replay_after_commit() {
        let store = Arc::new(FaultStore {
            inner: MemoryStore::new(),
            mode: AtomicU8::new(0),
            cancelled: Arc::new(AtomicUsize::new(0)),
        });
        let owner = crate::ownership::OwnerId::parse("fault-owner").unwrap();
        let mut record = crate::store::tests::sandbox("fault-member", "fault-node");
        record.owner_id = Some(owner.clone());
        store.inner.put_sandbox(&record).await.unwrap();
        let hash = Sha256::digest(b"owned-fault-key")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let policies=crate::keys::ApiKeyPolicy::from_json(&json!([{
            "sha256":hash,"expires_at":crate::model::now_ms()/1000+3600,"scopes":["sandboxes"],"principal_id":"fault-owner"
        }]).to_string()).unwrap();
        let control = ControlPlane::new(
            store.clone(),
            ControlConfig {
                api_key: Some("fault-admin".into()),
                api_keys: policies,
                access_audit: None,
                cluster_token: None,
                proxy_port: 5981,
                create_timeout: Duration::from_secs(1),
                identity_issuer: None,
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(control);
        let mut server = Server(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap();
        let url = format!("http://{address}/sandboxes/fault-member/private-networks");
        let request = json!({"expectedRevision":null,"revision":uuid::Uuid::new_v4().to_string(),"tags":["team"]});
        for mode in 1..=6 {
            store.mode.store(mode, Ordering::SeqCst);
            let started = Instant::now();
            let response = if mode <= 2 {
                client
                    .get(&url)
                    .header("x-api-key", "owned-fault-key")
                    .send()
                    .await
                    .unwrap()
            } else {
                client
                    .put(&url)
                    .header("x-api-key", "owned-fault-key")
                    .json(&request)
                    .send()
                    .await
                    .unwrap()
            };
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "mode {mode}"
            );
            eprintln!(
                "fault mode={mode} status=503 elapsed_ms={}",
                started.elapsed().as_millis()
            );
            let text = response.text().await.unwrap();
            assert!(!text.contains("private internal detail"));
            if [2, 4, 6].contains(&mode) {
                assert!(started.elapsed() >= Duration::from_secs(5));
            }
            // A hung store call may have committed, so its outcome is
            // uncertain. A hung ownership lookup (mode 4) comes before any
            // write, so nothing is uncertain: the lookup was unavailable.
            if [2, 6].contains(&mode) {
                assert!(text.contains("outcome uncertain"), "mode {mode}: {text}");
            }
            if mode == 4 {
                assert!(text.contains("lookup unavailable"), "mode {mode}: {text}");
            }
            assert_eq!(
                store
                    .inner
                    .private_membership(&record.sandbox_id, &owner)
                    .await
                    .unwrap(),
                MembershipAccess::Granted(None)
            );
        }
        let mut expected: Option<String> = None;
        for mode in [8, 7] {
            let revision = uuid::Uuid::new_v4().to_string();
            let request = json!({"expectedRevision":expected,"revision":revision,"tags":["team"]});
            store.mode.store(mode, Ordering::SeqCst);
            let started = Instant::now();
            let response = client
                .put(&url)
                .header("x-api-key", "owned-fault-key")
                .json(&request)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let text = response.text().await.unwrap();
            assert!(!text.contains("private internal detail"));
            if mode == 7 {
                assert!(started.elapsed() >= Duration::from_secs(5));
                assert!(text.contains("outcome uncertain"));
            }
            eprintln!(
                "fault mode={mode} status=503 elapsed_ms={} committed=true",
                started.elapsed().as_millis()
            );
            let MembershipAccess::Granted(Some(committed)) = store
                .inner
                .private_membership(&record.sandbox_id, &owner)
                .await
                .unwrap()
            else {
                panic!("committed membership missing");
            };
            assert_eq!(committed.revision(), revision);
            store.mode.store(0, Ordering::SeqCst);
            let mut changed = request.clone();
            changed["tags"] = json!(["changed"]);
            assert_eq!(
                client
                    .put(&url)
                    .header("x-api-key", "owned-fault-key")
                    .json(&changed)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::CONFLICT
            );
            let mut changed_revision = request.clone();
            changed_revision["revision"] = json!(uuid::Uuid::new_v4().to_string());
            assert_eq!(
                client
                    .put(&url)
                    .header("x-api-key", "owned-fault-key")
                    .json(&changed_revision)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::CONFLICT
            );
            let replay = client
                .put(&url)
                .header("x-api-key", "owned-fault-key")
                .json(&request)
                .send()
                .await
                .unwrap();
            assert_eq!(replay.status(), StatusCode::OK);
            assert_eq!(
                replay.json::<Value>().await.unwrap(),
                json!({"revision":revision,"tags":["team"]})
            );
            assert_eq!(
                store
                    .inner
                    .private_membership(&record.sandbox_id, &owner)
                    .await
                    .unwrap(),
                MembershipAccess::Granted(Some(committed))
            );
            expected = Some(revision);
        }
        assert_eq!(store.cancelled.load(Ordering::SeqCst), 4);
        eprintln!("fault recovery exact_replays=2 changed_payload_refusals=2 changed_revision_refusals=2 cancelled_store_futures=4");
        assert!(store
            .inner
            .delete_sandbox(&record.sandbox_id)
            .await
            .unwrap());
        server.0.abort();
        let _ = (&mut server.0).await;
    }
}

#[cfg(test)]
mod web_sharing_failure_tests {
    use super::*;
    use crate::domains::{DomainBinding, DomainName};
    use crate::model::{ClusterEvent, Delivery, Webhook};
    use crate::store::{MemoryStore, Result as StoreResult, StoreError};
    use crate::web_sharing::{SharingAccess, SharingChange, WebSharingState};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    struct FaultStore {
        inner: MemoryStore,
        mode: AtomicU8,
        cancelled: Arc<AtomicUsize>,
        entered: tokio::sync::Notify,
        release: tokio::sync::Notify,
    }
    struct Pending(Arc<AtomicUsize>);
    impl Drop for Pending {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl FaultStore {
        async fn hang<T>(&self) -> T {
            let _pending = Pending(self.cancelled.clone());
            std::future::pending().await
        }
        fn error<T>() -> StoreResult<T> {
            Err(StoreError(
                "injected store fault with private internal detail".into(),
            ))
        }
    }
    #[async_trait::async_trait]
    impl ClusterStore for FaultStore {
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
        async fn register_named_sandbox(
            &self,
            record: &SandboxRecord,
            reservation: &NameReservation,
        ) -> StoreResult<bool> {
            self.inner.register_named_sandbox(record, reservation).await
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
        async fn name_reservation(
            &self,
            name: &SandboxName,
        ) -> StoreResult<Option<NameReservation>> {
            self.inner.name_reservation(name).await
        }
        async fn bind_name(
            &self,
            name: &SandboxName,
            token: &str,
            sandbox: &str,
        ) -> StoreResult<bool> {
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
        async fn sandbox(&self, id: &str) -> StoreResult<Option<SandboxRecord>> {
            match self.mode.load(Ordering::SeqCst) {
                3 => Self::error(),
                4 => self.hang().await,
                _ => self.inner.sandbox(id).await,
            }
        }
        async fn web_sharing_snapshot(
            &self,
            sandbox: &str,
        ) -> StoreResult<Option<(SandboxRecord, WebSharingState)>> {
            match self.mode.load(Ordering::SeqCst) {
                9 => return Self::error(),
                10 => return self.hang().await,
                11 => {
                    self.entered.notify_one();
                    self.release.notified().await;
                }
                _ => {}
            }
            self.inner.web_sharing_snapshot(sandbox).await
        }
        async fn web_sharing(
            &self,
            sandbox: &str,
            owner: &crate::ownership::OwnerId,
        ) -> StoreResult<SharingAccess<Option<WebSharingState>>> {
            match self.mode.load(Ordering::SeqCst) {
                1 => Self::error(),
                2 => self.hang().await,
                _ => self.inner.web_sharing(sandbox, owner).await,
            }
        }
        async fn compare_web_sharing(
            &self,
            expected: Option<&str>,
            next: &WebSharingState,
        ) -> StoreResult<SharingChange> {
            let mode = self.mode.load(Ordering::SeqCst);
            match mode {
                5 => return Self::error(),
                6 => return self.hang().await,
                _ => {}
            }
            let result = self.inner.compare_web_sharing(expected, next).await?;
            if result == SharingChange::Applied {
                match mode {
                    7 => return self.hang().await,
                    8 => return Self::error(),
                    _ => {}
                }
            }
            Ok(result)
        }
    }
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    #[tokio::test]
    async fn http_store_failures_preserve_uncertainty_and_exact_replay_after_commit() {
        let store = Arc::new(FaultStore {
            inner: MemoryStore::new(),
            mode: AtomicU8::new(0),
            cancelled: Arc::new(AtomicUsize::new(0)),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let owner = crate::ownership::OwnerId::parse("fault-owner").unwrap();
        let mut record = crate::store::tests::sandbox("fault-sharing", "fault-node");
        record.owner_id = Some(owner.clone());
        store.inner.put_sandbox(&record).await.unwrap();
        let hash = Sha256::digest(b"owned-fault-key")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let policies=crate::keys::ApiKeyPolicy::from_json(&json!([{
            "sha256":hash,"expires_at":crate::model::now_ms()/1000+3600,"scopes":["sandboxes"],"principal_id":"fault-owner"
        }]).to_string()).unwrap();
        let control = ControlPlane::new(
            store.clone(),
            ControlConfig {
                api_key: Some("fault-admin".into()),
                api_keys: policies,
                access_audit: None,
                cluster_token: None,
                proxy_port: 5981,
                create_timeout: Duration::from_secs(1),
                identity_issuer: None,
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(control);
        let mut server = Server(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap();
        let url = format!("http://{address}/sandboxes/fault-sharing/web-sharing");
        let request = json!({"expectedRevision":null,"revision":uuid::Uuid::new_v4().to_string(),"grants":[{"subject":"alice","expires_at":100}]});
        for mode in 1..=6 {
            store.mode.store(mode, Ordering::SeqCst);
            let started = Instant::now();
            let response = if mode <= 2 {
                client
                    .get(&url)
                    .header("x-api-key", "owned-fault-key")
                    .send()
                    .await
                    .unwrap()
            } else {
                client
                    .put(&url)
                    .header("x-api-key", "owned-fault-key")
                    .json(&request)
                    .send()
                    .await
                    .unwrap()
            };
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "mode {mode}"
            );
            eprintln!(
                "fault mode={mode} status=503 elapsed_ms={}",
                started.elapsed().as_millis()
            );
            let text = response.text().await.unwrap();
            assert!(!text.contains("private internal detail"));
            if [2, 4, 6].contains(&mode) {
                assert!(started.elapsed() >= Duration::from_secs(5));
            }
            // A hung store call may have committed, so its outcome is
            // uncertain. A hung ownership lookup (mode 4) comes before any
            // write, so nothing is uncertain: the lookup was unavailable.
            if [2, 6].contains(&mode) {
                assert!(text.contains("outcome uncertain"), "mode {mode}: {text}");
            }
            if mode == 4 {
                assert!(text.contains("lookup unavailable"), "mode {mode}: {text}");
            }
            assert_eq!(
                store
                    .inner
                    .web_sharing(&record.sandbox_id, &owner)
                    .await
                    .unwrap(),
                SharingAccess::Granted(None)
            );
        }
        let mut expected: Option<String> = None;
        for mode in [8, 7] {
            let revision = uuid::Uuid::new_v4().to_string();
            let request = json!({"expectedRevision":expected,"revision":revision,"grants":[{"subject":"alice","expires_at":100}]});
            store.mode.store(mode, Ordering::SeqCst);
            let started = Instant::now();
            let response = client
                .put(&url)
                .header("x-api-key", "owned-fault-key")
                .json(&request)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let text = response.text().await.unwrap();
            assert!(!text.contains("private internal detail"));
            if mode == 7 {
                assert!(started.elapsed() >= Duration::from_secs(5));
                assert!(text.contains("outcome uncertain"));
            }
            eprintln!(
                "fault mode={mode} status=503 elapsed_ms={} committed=true",
                started.elapsed().as_millis()
            );
            let SharingAccess::Granted(Some(committed)) = store
                .inner
                .web_sharing(&record.sandbox_id, &owner)
                .await
                .unwrap()
            else {
                panic!("committed membership missing");
            };
            assert_eq!(committed.revision(), revision);
            store.mode.store(0, Ordering::SeqCst);
            let mut changed = request.clone();
            changed["grants"] = json!([{"subject":"changed","expires_at":100}]);
            assert_eq!(
                client
                    .put(&url)
                    .header("x-api-key", "owned-fault-key")
                    .json(&changed)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::CONFLICT
            );
            let mut changed_revision = request.clone();
            changed_revision["revision"] = json!(uuid::Uuid::new_v4().to_string());
            assert_eq!(
                client
                    .put(&url)
                    .header("x-api-key", "owned-fault-key")
                    .json(&changed_revision)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::CONFLICT
            );
            let replay = client
                .put(&url)
                .header("x-api-key", "owned-fault-key")
                .json(&request)
                .send()
                .await
                .unwrap();
            assert_eq!(replay.status(), StatusCode::OK);
            assert_eq!(
                replay.json::<Value>().await.unwrap(),
                json!({"revision":revision,"grants":[{"subject":"alice","expires_at":100}]})
            );
            assert_eq!(
                store
                    .inner
                    .web_sharing(&record.sandbox_id, &owner)
                    .await
                    .unwrap(),
                SharingAccess::Granted(Some(committed))
            );
            expected = Some(revision);
        }
        assert_eq!(store.cancelled.load(Ordering::SeqCst), 4);
        eprintln!("fault recovery exact_replays=2 changed_payload_refusals=2 changed_revision_refusals=2 cancelled_store_futures=4");
        assert!(store
            .inner
            .delete_sandbox(&record.sandbox_id)
            .await
            .unwrap());
        server.0.abort();
        let _ = (&mut server.0).await;
    }

    #[tokio::test]
    async fn stored_grant_admission_denies_store_faults_and_credential_changes_during_lookup() {
        use base64::Engine;
        use hv2_api::sandbox_proxy::SandboxRoutes;
        let store = Arc::new(FaultStore {
            inner: MemoryStore::new(),
            mode: AtomicU8::new(0),
            cancelled: Arc::new(AtomicUsize::new(0)),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let mut record = crate::store::tests::sandbox("admission-guest", "node");
        record.owner_id = Some(crate::ownership::OwnerId::parse("owner").unwrap());
        store.inner.put_sandbox(&record).await.unwrap();
        let expiry = chrono::Utc::now().timestamp() + 600;
        let sharing = WebSharingState::new(
            &record,
            vec![crate::web_sharing::WebGrant::new("alice", expiry).unwrap()],
        )
        .unwrap();
        assert_eq!(
            store
                .inner
                .compare_web_sharing(None, &sharing)
                .await
                .unwrap(),
            SharingChange::Applied
        );
        let document = |password: &str, expires: i64, delegation: bool| {
            json!([{
            "subject":"alice","sha256":Sha256::digest(password.as_bytes()).iter().map(|b|format!("{b:02x}")).collect::<String>(),
            "expires_at":expires,"sandboxes":[],"allow_owner_grants":delegation
        }]).to_string()
        };
        let policy = Arc::new(
            crate::web_access::WebAccessPolicy::from_json(&document("secret", expiry, true))
                .unwrap(),
        );
        let routes = Arc::new(
            ClusterRoutes::new(store.clone(), Duration::from_secs(30))
                .with_web_access(policy.clone()),
        );
        let headers = || {
            let mut headers = HeaderMap::new();
            headers.insert(
                "authorization",
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode("alice:secret")
                )
                .parse()
                .unwrap(),
            );
            headers.insert(
                crate::web_access::IDENTITY_HEADER,
                "forged".parse().unwrap(),
            );
            headers
        };
        for mode in [9, 10] {
            store.mode.store(mode, Ordering::SeqCst);
            let mut h = headers();
            let started = Instant::now();
            assert!(routes
                .admit_request(&record.sandbox_id, 8080, &mut h)
                .await
                .is_err());
            assert!(!h.contains_key(crate::web_access::IDENTITY_HEADER));
            if mode == 10 {
                assert!(started.elapsed() >= Duration::from_secs(5));
            }
        }
        assert_eq!(store.cancelled.load(Ordering::SeqCst), 1);
        for replacement in [
            document("rotated", expiry, true),
            document("secret", 1, true),
            document("secret", expiry, false),
        ] {
            policy.replace(&document("secret", expiry, true)).unwrap();
            store.mode.store(11, Ordering::SeqCst);
            let r = routes.clone();
            let id = record.sandbox_id.clone();
            let mut h = headers();
            let pending = tokio::spawn(async move {
                let result = r.admit_request(&id, 8080, &mut h).await;
                (result, h)
            });
            tokio::time::timeout(Duration::from_secs(1), store.entered.notified())
                .await
                .unwrap();
            assert!(!pending.is_finished());
            policy.replace(&replacement).unwrap();
            store.release.notify_one();
            let (result, h) = pending.await.unwrap();
            assert!(result.is_err());
            assert!(!h.contains_key(crate::web_access::IDENTITY_HEADER));
        }
        policy.replace(&document("secret", expiry, true)).unwrap();
        store.mode.store(0, Ordering::SeqCst);
        let mut h = headers();
        routes
            .admit_request(&record.sandbox_id, 8080, &mut h)
            .await
            .unwrap();
        assert_eq!(h[crate::web_access::IDENTITY_HEADER], "alice");
        assert!(!h.contains_key("authorization"));
        eprintln!("stored grant admission: store error and stall deny; cancelled=1; rotation expiry delegation races deny; current grant admits");
    }
}
