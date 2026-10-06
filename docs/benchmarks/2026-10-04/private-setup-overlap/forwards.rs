//! A sandbox's own ports, reached through the proxy: E2B's
//! `sandbox.get_host(port)` -- a web server, a dev server, the Code
//! Interpreter's Jupyter -- at `{port}-{sandboxID}.{domain}`.
//!
//! The first request for a port makes a loopback listener for it, which
//! the proxy is routed to like envd's. Each connection it accepts is
//! carried into the guest over a vsock connection of its own: the agent
//! connects to the port there and copies bytes both ways
//! ([`hv2_guest_agent::Operation::Forward`]). No network interface is
//! involved, so a sandbox without one serves its ports all the same, and
//! nothing reaches a guest port unless a request through the proxy -- with
//! the proxy's own checks -- asked for it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use hv2_agent::{AgentVM, VsockStream};

use super::{AppState, Arc};

/// How long the guest has to answer that its port is open.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(5);

/// Open the guest port before accepting an API upgrade. Holding the lifecycle
/// lock through registration prevents pause/delete from missing this stream.
pub(crate) async fn tcp_tunnel(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((sandbox, port)): axum::extract::Path<(String, u16)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    port_tunnel(state, sandbox, port, request, false, false, None).await
}

pub(crate) async fn udp_tunnel(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((sandbox, port)): axum::extract::Path<(String, u16)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    port_tunnel(state, sandbox, port, request, true, false, None).await
}

pub(crate) async fn udp_tunnel_ipv6(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((sandbox, port)): axum::extract::Path<(String, u16)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    port_tunnel(state, sandbox, port, request, true, true, None).await
}

use hv2_cluster::private_networks::{
    PRIVATE_ROUTE_HEADER as PRIVATE_CLAIM_HEADER, PRIVATE_SOURCE_NODE_HEADER,
};
pub(super) fn private_transport_enabled(clustered: bool, token: Option<&str>, tls: [bool; 3]) -> bool {
    clustered
        && token.is_some_and(|token| !token.trim().is_empty() && token.len() <= 4096)
        && tls.into_iter().all(|enabled| enabled)
}
#[derive(Clone)]
struct PrivateIncoming {
    claim: hv2_cluster::private_networks::PrivateRouteClaim,
    source_node: String,
}
fn private_incoming(
    headers: &axum::http::HeaderMap,
    sandbox: &str,
    port: u16,
) -> Result<PrivateIncoming, &'static str> {
    fn single<'a>(
        headers: &'a axum::http::HeaderMap,
        name: &str,
        limit: usize,
    ) -> Result<&'a str, &'static str> {
        let mut values = headers.get_all(name).iter();
        let value = values.next().ok_or("private route context required")?;
        if values.next().is_some() || value.as_bytes().len() > limit {
            return Err("ambiguous or oversized private route context");
        }
        value.to_str().map_err(|_| "invalid private route context")
    }
    let value = single(headers, PRIVATE_CLAIM_HEADER, 2048)?;
    let claim: hv2_cluster::private_networks::PrivateRouteClaim =
        serde_json::from_str(value).map_err(|_| "invalid private route context")?;
    let source_node = single(headers, PRIVATE_SOURCE_NODE_HEADER, 128)?;
    claim.validate()?;
    if source_node.is_empty()
        || !source_node
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("invalid private source node");
    }
    if port == 0 || claim.port != port || claim.destination_id != sandbox {
        return Err("private route destination mismatch");
    }
    Ok(PrivateIncoming {
        claim,
        source_node: source_node.into(),
    })
}
/// The source label is trusted only inside the operator's mutually authenticated
/// cluster fabric. Shared cluster credentials are administrator-level trust;
/// this is not per-node certificate-to-VM attribution. Source gateway code must
/// derive context from its own live VM, never accept it from guest traffic.
pub(crate) async fn private_tcp_tunnel(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((sandbox, port)): axum::extract::Path<(String, u16)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    private_port_tunnel(state, sandbox, port, request, false).await
}

/// Authenticated private IPv4 UDP uses the same bounded framing and lifecycle
/// authorization as private TCP. Source gateway UDP routing is separate.
pub(crate) async fn private_udp_tunnel(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((sandbox, port)): axum::extract::Path<(String, u16)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    private_port_tunnel(state, sandbox, port, request, true).await
}

async fn private_port_tunnel(
    state: Arc<AppState>,
    sandbox: String,
    port: u16,
    request: axum::extract::Request,
    udp: bool,
) -> axum::response::Response {
    use axum::http::StatusCode;
    if !private_transport_enabled(
        state.node.is_some(),
        state.opts.cluster_token.as_deref(),
        [
            state.opts.mtls_ca.is_some(),
            state.opts.mtls_cert.is_some(),
            state.opts.mtls_key.is_some(),
        ],
    ) {
        return super::api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "private routes require an authenticated mutual-TLS cluster",
        );
    }
    // Reject duplicate credential fields even though ordinary middleware checks
    // the first one. That middleware performs the constant-time token check.
    if request
        .headers()
        .get_all(super::CLUSTER_TOKEN_HEADER)
        .iter()
        .count()
        != 1
    {
        return super::api_error(
            StatusCode::UNAUTHORIZED,
            "unambiguous cluster authentication required",
        );
    }
    let incoming = match private_incoming(request.headers(), &sandbox, port) {
        Ok(incoming) => incoming,
        Err(error) => return super::api_error(StatusCode::BAD_REQUEST, error),
    };
    port_tunnel(state, sandbox, port, request, udp, false, Some(incoming)).await
}
async fn authorize_private(
    state: &AppState,
    incoming: &PrivateIncoming,
) -> Result<(), (axum::http::StatusCode, &'static str)> {
    use axum::http::StatusCode;
    let refused = (StatusCode::FORBIDDEN, "private route refused");
    let unavailable = (
        StatusCode::SERVICE_UNAVAILABLE,
        "private route state unavailable",
    );
    let node = state.node.as_ref().ok_or(unavailable)?;
    let lookup = async {
        // Redis reads all route and live-node records in one atomic command.
        // Each setup barrier still requests a fresh view, as does revocation.
        let snapshot = node.store().private_route_snapshot_with_live_nodes(
            &incoming.claim.source_id, &incoming.claim.destination_id,
            &incoming.source_node, node.id(),
        ).await.map_err(|_| unavailable)?.ok_or(refused)?;
        if snapshot.source_record.node_id != incoming.source_node
            || snapshot.destination_record.node_id != node.id()
        {
            return Err(refused);
        }
        let local = state.sandboxes.lock();
        let matches = |record: &hv2_cluster::model::SandboxRecord| {
            local.get(&record.sandbox_id).is_some_and(|live| {
                live.pending_registration.is_none()
                    && live.record.owner_id == record.owner_id
                    && live.record.started_at_ms == record.started_at_ms
                    && live.record.node_id == record.node_id
                    && live.record.end_at_ms > super::now_ms()
            })
        };
        if !matches(&snapshot.destination_record)
            || (incoming.source_node == node.id() && !matches(&snapshot.source_record))
        {
            return Err(refused);
        }
        snapshot
            .authorize(
                &incoming.claim.source_id,
                &incoming.claim,
                super::now_ms(),
                false,
                false,
            )
            .map_err(|_| refused)
    };
    tokio::time::timeout(Duration::from_secs(3), lookup)
        .await
        .map_err(|_| unavailable)?
}
async fn private_revoked(state: &AppState, incoming: &PrivateIncoming) {
    let mut ticks = tokio::time::interval(Duration::from_secs(1));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let lifetime = tokio::time::sleep(Duration::from_secs(600));
    tokio::pin!(lifetime);
    loop {
        tokio::select! { _=&mut lifetime=>return, _=ticks.tick()=>{} }
        if authorize_private(state, incoming).await.is_err() {
            return;
        }
    }
}
/// A port opened before registration is closed if setup is cancelled or refused.
struct UnregisteredStream(Option<VsockStream>);
impl Drop for UnregisteredStream {
    fn drop(&mut self) {
        if let Some(stream) = self.0.take() {
            stream.close();
        }
    }
}

async fn port_tunnel(
    state: Arc<AppState>,
    sandbox: String,
    port: u16,
    request: axum::extract::Request,
    udp: bool,
    ipv6: bool,
    private: Option<PrivateIncoming>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    let negotiation = if ipv6 {
        hv2_api::udp_tunnel::validate_ipv6(&request)
    } else if udp {
        hv2_api::udp_tunnel::validate(&request)
    } else {
        hv2_api::tcp_tunnel::validate(&request)
    };
    if let Err(message) = negotiation {
        return super::api_error(StatusCode::BAD_REQUEST, message);
    }
    if port == 0 {
        return super::api_error(StatusCode::BAD_REQUEST, "guest port must be nonzero");
    }
    let private_permit = if private.is_some() {
        match Arc::clone(&state.forwards.private_slots).try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                return super::api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "private route connection capacity reached",
                );
            }
        }
    } else {
        None
    };
    // Unknown names must not allocate entries in the lifecycle lock table.
    if !state.sandboxes.lock().contains_key(&sandbox) {
        return super::api_error(StatusCode::NOT_FOUND, "sandbox is not running on this node");
    }
    let lock = super::transition_lock(&state, &sandbox);
    let _held = lock.lock().await;
    if let Some(incoming) = &private {
        if let Err((status, error)) = authorize_private(&state, incoming).await {
            return super::api_error(status, error);
        }
    }
    let vm = state.sandboxes.lock().get(&sandbox).map(|live| {
        (
            Arc::clone(&live.vm),
            super::ActivityGuard::enter(&live.activity),
        )
    });
    let Some((vm, activity)) = vm else {
        return super::api_error(StatusCode::NOT_FOUND, "sandbox is not running on this node");
    };
    let source_activity = private
        .as_ref()
        .filter(|incoming| incoming.source_node == state.node_id)
        .and_then(|incoming| {
            state
                .sandboxes
                .lock()
                .get(&incoming.claim.source_id)
                .map(|live| super::ActivityGuard::enter(&live.activity))
        });
    // Guard the guest stream inside its future: cancellation while the
    // independent socket pair is still pending must close an opened port.
    let opening = async {
        let opened = if ipv6 {
            vm.forward_udp_port_ipv6(port, FORWARD_TIMEOUT).await
        } else if udp {
            vm.forward_udp_port(port, FORWARD_TIMEOUT).await
        } else {
            vm.forward_port(port, FORWARD_TIMEOUT).await
        };
        opened.map(|(stream, early)| {
            let guard = UnregisteredStream(Some(stream.clone()));
            (stream, early, guard)
        })
    };
    // Neither the temporary loopback address nor a listener is published.
    let pairing = async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let client = tokio::net::TcpStream::connect(listener.local_addr()?).await?;
        let (server, _) = listener.accept().await?;
        Ok::<_, std::io::Error>((client, server))
    };
    let (opened, pair) = tokio::join!(opening, pairing);
    let (stream, early, mut unregistered) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            tracing::debug!(%sandbox, port, udp, %error, "guest port unavailable");
            return super::api_error(StatusCode::BAD_GATEWAY, "guest port unavailable");
        }
    };
    let (client, server) = match pair {
        Ok(pair) => pair,
        Err(error) => {
            return super::api_error(StatusCode::INTERNAL_SERVER_ERROR, error);
        }
    };
    if let Some(incoming) = &private {
        if let Err((status, error)) = authorize_private(&state, incoming).await {
            return super::api_error(status, error);
        }
    }
    state
        .forwards
        .open
        .lock()
        .entry(sandbox.clone())
        .or_default()
        .push(stream.clone());
    unregistered.0.take();
    let relay_state = Arc::clone(&state);
    tokio::spawn(async move {
        let _private_permit = private_permit;
        let _active = activity;
        let _source_active = source_activity;
        if let Some(incoming) = private {
            let _cancelled = UnregisteredStream(Some(stream.clone()));
            let closing = stream.clone();
            let carry = carry_opened(&relay_state, &sandbox, stream, early, server);
            tokio::pin!(carry);
            tokio::select! {
                _=&mut carry=>{},
                _=private_revoked(&relay_state,&incoming)=>{ closing.close(); (&mut carry).await; }
            }
        } else {
            carry_opened(&relay_state, &sandbox, stream, early, server).await;
        }
    });
    if ipv6 {
        hv2_api::udp_tunnel::accept_ipv6(request, client)
    } else if udp {
        hv2_api::udp_tunnel::accept(request, client)
    } else {
        hv2_api::tcp_tunnel::accept(request, client)
    }
}

pub(crate) struct Forwards {
    private_slots: Arc<tokio::sync::Semaphore>,
    listeners: parking_lot::Mutex<HashMap<(String, u16), (SocketAddr, tokio::task::AbortHandle)>>,
    /// Connections carried now, by sandbox, to close when it stops.
    open: parking_lot::Mutex<HashMap<String, Vec<VsockStream>>>,
}

impl Default for Forwards {
    fn default() -> Self {
        Self {
            private_slots: Arc::new(tokio::sync::Semaphore::new(128)),
            listeners: Default::default(),
            open: Default::default(),
        }
    }
}

/// The listener for `port` of `sandbox`: made now if there is none, and
/// the proxy routed to it. `None` if the sandbox is not running here.
pub(crate) async fn listen(state: &Arc<AppState>, sandbox: &str, port: u16) -> Option<SocketAddr> {
    let key = (sandbox.to_string(), port);
    if let Some((addr, _)) = state.forwards.listeners.lock().get(&key) {
        return Some(*addr);
    }
    let vm = state
        .sandboxes
        .lock()
        .get(sandbox)
        .map(|live| Arc::clone(&live.vm))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
    let addr = listener.local_addr().ok()?;
    let accepting = {
        let state = Arc::clone(state);
        let sandbox = sandbox.to_string();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let vm = Arc::clone(&vm);
                let state = Arc::clone(&state);
                let sandbox = sandbox.clone();
                tokio::spawn(async move { carry(&state, &sandbox, &vm, port, tcp).await });
            }
        })
    };
    let mut listeners = state.forwards.listeners.lock();
    // Two first requests at once: the first listener made stays.
    if let Some((existing, _)) = listeners.get(&key) {
        accepting.abort();
        return Some(*existing);
    }
    listeners.insert(key, (addr, accepting.abort_handle()));
    drop(listeners);
    state.routes.insert(sandbox, port, addr);
    Some(addr)
}

/// One connection: into the guest over vsock, and back.
async fn carry(
    state: &AppState,
    sandbox: &str,
    vm: &AgentVM,
    port: u16,
    tcp: tokio::net::TcpStream,
) {
    let lock = super::transition_lock(state, sandbox);
    let held = lock.lock().await;
    let current = state
        .sandboxes
        .lock()
        .get(sandbox)
        .is_some_and(|live| std::ptr::eq(live.vm.as_ref(), vm));
    if !current {
        return;
    }
    let (stream, early) = match vm.forward_port(port, FORWARD_TIMEOUT).await {
        Ok(opened) => opened,
        Err(e) => {
            tracing::debug!("{sandbox}: port {port}: {e}");
            return; // the proxy answers 502 as the connection closes
        }
    };
    state
        .forwards
        .open
        .lock()
        .entry(sandbox.to_string())
        .or_default()
        .push(stream.clone());
    drop(held);
    carry_opened(state, sandbox, stream, early, tcp).await;
}

async fn carry_opened(
    state: &AppState,
    sandbox: &str,
    stream: VsockStream,
    early: Vec<u8>,
    tcp: tokio::net::TcpStream,
) {
    let Ok(tcp) = tcp.into_std() else {
        stream.close();
        return;
    };
    if tcp.set_nonblocking(false).is_err() {
        stream.close();
        return;
    }
    let _ = tcp.set_nodelay(true);
    let done = tokio::task::spawn_blocking(move || splice(&stream, tcp, &early));
    let _ = done.await;
    // Forget streams that have closed; each is closed once both sides are.
    if let Some(open) = state.forwards.open.lock().get_mut(sandbox) {
        open.retain(VsockStream::is_open);
    }
}

/// Bytes both ways between the proxy's connection and the guest's, until
/// either side is done.
fn splice(stream: &VsockStream, tcp: std::net::TcpStream, early: &[u8]) {
    use std::io::{Read, Write};
    let Ok(mut to_proxy) = tcp.try_clone() else {
        return;
    };
    let mut from_proxy = tcp;
    if !early.is_empty() && to_proxy.write_all(early).is_err() {
        return;
    }
    let upstream = {
        let stream = stream.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match from_proxy.read(&mut buf) {
                    Ok(0) => {
                        let _ = stream.shutdown_write();
                        return;
                    }
                    Err(_) => break,
                    Ok(n) => {
                        if stream.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            // An error aborts the relay; orderly EOF only ends its direction.
            stream.close();
        })
    };
    let clean_eof = loop {
        match stream.read() {
            Ok(data) if !data.is_empty() => {
                if to_proxy.write_all(&data).is_err() {
                    break false;
                }
            }
            Ok(_) => break true,
            Err(_) => break false,
        }
    };
    if clean_eof && stream.is_open() {
        let _ = to_proxy.shutdown(std::net::Shutdown::Write);
    } else {
        let _ = to_proxy.shutdown(std::net::Shutdown::Both);
        stream.close();
    }
    let _ = upstream.join();
    let _ = to_proxy.shutdown(std::net::Shutdown::Both);
    stream.close();
}

/// A sandbox stopped or paused: its listeners and every connection close.
pub(crate) fn stop(state: &AppState, sandbox: &str) {
    state
        .forwards
        .listeners
        .lock()
        .retain(|(id, _), (_, accepting)| {
            if id == sandbox {
                accepting.abort();
                false
            } else {
                true
            }
        });
    if let Some(open) = state.forwards.open.lock().remove(sandbox) {
        for stream in open {
            stream.close();
        }
    }
}

#[cfg(test)]
mod private_tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        let claim = hv2_cluster::private_networks::PrivateRouteClaim {
            source_id: "source".into(),
            source_generation: uuid::Uuid::new_v4().to_string(),
            destination_id: "destination".into(),
            destination_generation: uuid::Uuid::new_v4().to_string(),
            network: hv2_cluster::private_networks::NetworkTag::parse("team").unwrap(),
            port: 8080,
        };
        headers.insert(
            PRIVATE_CLAIM_HEADER,
            HeaderValue::from_str(&serde_json::to_string(&claim).unwrap()).unwrap(),
        );
        headers.insert(
            PRIVATE_SOURCE_NODE_HEADER,
            HeaderValue::from_static("source-node"),
        );
        headers
    }
    #[test]
    fn transport_requires_cluster_token_and_all_mutual_tls_inputs() {
        assert!(private_transport_enabled(
            true,
            Some("owned-token"),
            [true; 3]
        ));
        for token in [None, Some(""), Some("  ")] {
            assert!(!private_transport_enabled(true, token, [true; 3]));
        }
        assert!(!private_transport_enabled(
            true,
            Some(&"a".repeat(4097)),
            [true; 3]
        ));
        assert!(!private_transport_enabled(
            false,
            Some("owned-token"),
            [true; 3]
        ));
        for tls in [
            [false, true, true],
            [true, false, true],
            [true, true, false],
        ] {
            assert!(!private_transport_enabled(true, Some("owned-token"), tls));
        }
    }
    #[test]
    fn context_is_single_bounded_strict_and_bound_to_the_path() {
        let good = headers();
        assert!(private_incoming(&good, "destination", 8080).is_ok());
        assert!(private_incoming(&good, "other", 8080).is_err());
        assert!(private_incoming(&good, "destination", 8081).is_err());
        assert!(private_incoming(&good, "destination", 0).is_err());
        for key in [PRIVATE_CLAIM_HEADER, PRIVATE_SOURCE_NODE_HEADER] {
            let mut invalid = good.clone();
            invalid.remove(key);
            assert!(private_incoming(&invalid, "destination", 8080).is_err());
            let mut invalid = good.clone();
            invalid.append(key, good[key].clone());
            assert!(private_incoming(&invalid, "destination", 8080).is_err());
        }
        let mut invalid = good.clone();
        invalid.insert(
            PRIVATE_CLAIM_HEADER,
            HeaderValue::from_str(&"x".repeat(2049)).unwrap(),
        );
        assert!(private_incoming(&invalid, "destination", 8080).is_err());
        let mut invalid = good.clone();
        invalid.insert(
            PRIVATE_SOURCE_NODE_HEADER,
            HeaderValue::from_static("bad/node"),
        );
        assert!(private_incoming(&invalid, "destination", 8080).is_err());
        let value: serde_json::Value =
            serde_json::from_str(good[PRIVATE_CLAIM_HEADER].to_str().unwrap()).unwrap();
        for (key, new_value) in [
            ("unknown", serde_json::json!(true)),
            ("source_id", serde_json::json!("bad/source")),
            ("network", serde_json::json!("TEAM")),
        ] {
            let mut value = value.clone();
            value[key] = new_value;
            let mut invalid = good.clone();
            invalid.insert(
                PRIVATE_CLAIM_HEADER,
                HeaderValue::from_str(&value.to_string()).unwrap(),
            );
            assert!(private_incoming(&invalid, "destination", 8080).is_err());
        }
    }
    #[test]
    fn private_connections_have_a_bounded_reusable_budget() {
        let forwards = Forwards::default();
        let mut permits = Vec::new();
        for _ in 0..128 {
            permits.push(forwards.private_slots.clone().try_acquire_owned().unwrap());
        }
        assert!(forwards.private_slots.clone().try_acquire_owned().is_err());
        permits.pop();
        assert!(forwards.private_slots.clone().try_acquire_owned().is_ok());
        drop(permits);
        assert_eq!(forwards.private_slots.available_permits(), 128);
    }
}
