//! One port in front of every sandbox, routed by the name the client asks for.
//!
//! # Why this exists
//!
//! `hv2-sandboxd`'s `POST /sandboxes` returns a `processPort`, and the sandbox's
//! gRPC service really is listening there. But no E2B SDK reads a field called
//! `processPort`: E2B addresses a sandbox by *hostname*, as
//! `{port}-{sandboxID}.{domain}`, and its own proxy resolves that to the right
//! envd. An SDK pointed at this server would go looking for a host name and
//! find nothing, which is why the parity roadmap lists domain routing as the
//! thing that actually blocks a real client rather than `grpcurl`.
//!
//! So this is that resolution step: accept HTTP/2 on one address, read the
//! authority the client asked for, and forward the request to whichever local
//! listener belongs to that sandbox.
//!
//! # Why it terminates the connection rather than forwarding bytes
//!
//! The authority is inside the HEADERS frame of an HTTP/2 stream, compressed
//! with HPACK against a table that is per-connection state. There is no way to
//! read it without being an HTTP/2 endpoint, so a TCP-level forwarder cannot
//! route on it. That is the whole reason this is a real proxy and not a socket
//! splice.
//!
//! Request and response bodies are passed through as streams, not buffered,
//! because gRPC's interesting methods are streaming ones -- `process.Process`'s
//! `Start` sends its output as it arrives, and a proxy that collected the body
//! before forwarding would turn that into one message at the end.
//!
//! # TLS
//!
//! [`serve_tls`] takes a certificate and key and speaks HTTPS, advertising
//! `h2` over ALPN because a gRPC client will not negotiate anything else. The
//! hop to the sandbox stays plaintext on loopback: the listener is a local
//! port that only this process routes to, and terminating TLS twice on one
//! machine buys nothing.
//!
//! # What it does not do
//!
//! No connection reuse -- each proxied request opens its own connection to the
//! backend, which is a cost a busy proxy would not pay and is not worth hiding
//! behind a pool until something measures it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use parking_lot::Mutex;
use tokio::net::{TcpListener, TcpStream};

/// The body this proxy returns: either the backend's, streamed through, or a
/// short message of its own.
type ProxyBody =
    http_body_util::combinators::BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

/// A backend's response body, holding the request's [`InFlight`] guard until
/// the body is finished with -- fully sent, or dropped by a client that left.
struct GuardedBody {
    inner: ProxyBody,
    _in_flight: InFlight,
}

impl hyper::body::Body for GuardedBody {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        std::pin::Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

/// Refuse a request in the shape its client can read.
///
/// A gRPC client does not interpret HTTP status codes: a 404 reaches it as
/// `Unimplemented ... malformed header: missing HTTP content-type`, which
/// names neither the problem nor the sandbox. gRPC carries its own status in
/// headers on a 200, so a request that arrived as gRPC is refused that way and
/// everything else gets the HTTP code.
///
/// Observed rather than assumed: deleting a sandbox and calling its old
/// hostname produced exactly that `Unimplemented` from `grpcurl`, which is why
/// this distinction exists.
fn refuse(is_grpc: bool, status: StatusCode, grpc_status: u8, text: &str) -> Response<ProxyBody> {
    let body = Full::new(Bytes::from(text.to_owned()))
        .map_err(|never| match never {})
        .boxed();

    if is_grpc {
        // "Trailers-only": status in the headers and *no body at all*. A gRPC
        // client reads a body as length-prefixed messages, so prose sent under
        // `application/grpc` is decoded as a frame header -- grpcurl reported
        // `received message larger than max (1864397665 vs 4194304)`, which is
        // this sentence's first four bytes read as a length. Found by running
        // it; the first version of the test checked the headers and not
        // whether a client could read the response.
        return Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "application/grpc")
            .header("grpc-status", grpc_status.to_string())
            .header("grpc-message", text)
            .body(
                http_body_util::Empty::<Bytes>::new()
                    .map_err(|never| match never {})
                    .boxed(),
            )
            .unwrap_or_else(|_| {
                let mut fallback = Response::new(
                    Full::new(Bytes::from_static(b"proxy error"))
                        .map_err(|never| match never {})
                        .boxed(),
                );
                *fallback.status_mut() = status;
                fallback
            });
    }

    let mut response = Response::new(body);
    *response.status_mut() = status;
    response
}

/// Whether a request arrived as gRPC, by the content type it declared.
fn is_grpc(req: &Request<Incoming>) -> bool {
    req.headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/grpc"))
}

/// gRPC status codes, from the canonical list, for the three refusals here.
mod grpc_status {
    /// The authority was missing or not a sandbox name.
    pub const INVALID_ARGUMENT: u8 = 3;
    /// No sandbox is serving that name.
    pub const NOT_FOUND: u8 = 5;
    /// The sandbox is known but its listener did not answer.
    pub const UNAVAILABLE: u8 = 14;
}

/// Where a sandbox's listener actually is.
///
/// A trait rather than a concrete map because the thing that knows this is the
/// control plane -- a node daemon holds its own sandboxes, a cluster's control
/// plane asks a shared store -- and a proxy that owned the registry would have
/// to be told about every creation and deletion. This way it asks. Async,
/// because the cluster's answer is a network round trip away.
#[async_trait::async_trait]
pub trait SandboxRoutes: Send + Sync + 'static {
    /// Authenticate or sanitize a request before opening a route or waking a VM.
    fn authorize_request(
        &self,
        _sandbox: &str,
        _port: u16,
        _headers: &mut hyper::HeaderMap,
    ) -> Result<(), ProxyAccessDenied> {
        Ok(())
    }

    /// Complete admission before backend lookup or guest wakeup. Implementations
    /// may await durable access checks here; denial keeps the route unopened.
    /// The default preserves existing synchronous authorization policies.
    async fn admit_request(
        &self,
        sandbox: &str,
        port: u16,
        headers: &mut hyper::HeaderMap,
    ) -> Result<(), ProxyAccessDenied> {
        self.authorize_request(sandbox, port, headers)
    }

    /// Apply route-owned policy to guest HTTP application response headers.
    fn prepare_response(&self, _sandbox: &str, _port: u16, _headers: &mut hyper::HeaderMap) {}
    /// The address serving `port` for `sandbox`, if that sandbox exists.
    async fn resolve(&self, sandbox: &str, port: u16) -> Option<SocketAddr>;

    /// Resolve an operator-bound hostname to a sandbox and guest port.
    /// The authority may include the public proxy's port.
    async fn resolve_hostname(&self, authority: &str) -> Option<(u16, String)> {
        let _ = authority;
        None
    }

    /// [`Self::resolve`] for a request about to be sent, with a guard the
    /// proxy holds until that request's exchange is over -- response body
    /// included, which for a streamed command is the whole of its run.
    ///
    /// What lets the owner of the routes tell a sandbox that is idle from one
    /// with a request in flight, which a last-request timestamp cannot: a
    /// command started a minute ago and still streaming is not idle. The
    /// default holds nothing.
    async fn open(&self, sandbox: &str, port: u16) -> Option<(SocketAddr, InFlight)> {
        self.resolve(sandbox, port)
            .await
            .map(|addr| (addr, InFlight::none()))
    }

    /// The last answer for `sandbox` could not be connected to: drop
    /// whatever made it, so the next [`Self::open`] asks afresh. Nothing to
    /// drop by default.
    async fn forget(&self, sandbox: &str) {
        let _ = sandbox;
    }

    /// Speak TLS to the addresses this resolves to: the client config, and
    /// the name their certificates must carry. `None`, the default, is
    /// plaintext -- right for a loopback listener, wrong across a network.
    fn backend_tls(
        &self,
    ) -> Option<(
        Arc<rustls::ClientConfig>,
        rustls::pki_types::ServerName<'static>,
    )> {
        None
    }
}

/// A route owner refused authentication before backend activity.
#[derive(Debug)]
pub struct ProxyAccessDenied {
    /// Optional HTTP authentication challenge for browser clients.
    pub challenge: Option<&'static str>,
}

/// What the proxy relays over: a TCP stream, or TLS on one.
trait BackendIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> BackendIo for T {}

/// Held for as long as a request to a sandbox is in flight; see
/// [`SandboxRoutes::open`]. Whatever it wraps is dropped when it is.
pub struct InFlight(Option<Box<dyn Send + Sync>>);

impl InFlight {
    /// A guard that tracks nothing.
    #[must_use]
    pub fn none() -> Self {
        Self(None)
    }

    /// A guard whose end is `guard`'s drop.
    #[must_use]
    pub fn new(guard: impl Send + Sync + 'static) -> Self {
        Self(Some(Box::new(guard)))
    }
}

/// A registry that can be handed around and updated as sandboxes come and go.
///
/// Enough for the control plane's needs, and the obvious implementation, so
/// that a caller with nothing more complicated does not have to write one.
#[derive(Default)]
pub struct PortMap {
    routes: Mutex<HashMap<(String, u16), SocketAddr>>,
}

impl PortMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Point `{port}-{sandbox}` at `addr`.
    pub fn insert(&self, sandbox: &str, port: u16, addr: SocketAddr) {
        self.routes.lock().insert((sandbox.to_owned(), port), addr);
    }

    /// Forget every route for one sandbox, as its VM goes away.
    pub fn remove_sandbox(&self, sandbox: &str) {
        self.routes.lock().retain(|(id, _), _| id != sandbox);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.routes.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait::async_trait]
impl SandboxRoutes for PortMap {
    async fn resolve(&self, sandbox: &str, port: u16) -> Option<SocketAddr> {
        self.routes.lock().get(&(sandbox.to_owned(), port)).copied()
    }
}

/// Split `{port}-{sandboxID}.{domain}` into its port and sandbox.
///
/// E2B's shape, and only the first label is looked at: the domain after it is
/// whatever the deployment is called and carries no routing information. A
/// port is required, because a sandbox runs more than one service and an
/// unqualified name would have to guess which.
///
/// Returns `None` for anything that does not have that shape, including a bare
/// host with no port prefix -- which is a request for the control plane, not
/// for a sandbox, and should not be silently routed to one.
#[must_use]
pub fn route_of(authority: &str) -> Option<(u16, &str)> {
    // An authority may carry a port of its own (`host:8080`); that is the
    // proxy's own port and says nothing about which sandbox is wanted.
    let host = authority.split(':').next()?;
    let first_label = host.split('.').next()?;
    let (port, sandbox) = first_label.split_once('-')?;
    if sandbox.is_empty() {
        return None;
    }
    Some((port.parse().ok()?, sandbox))
}

/// The port a sandbox's envd listens on, when a client names a sandbox but
/// not a port. E2B's own default, and what its SDK assumes.
pub const ENVD_PORT: u16 = 49983;

/// The sandbox a request names through E2B's own headers.
///
/// Captured from the real Python SDK (`connectrpc/0.11.1`), which sends
/// `e2b-sandbox-id` and `e2b-sandbox-port` on every request to a sandbox --
/// including when it addresses the sandbox as plain `localhost:49983` and
/// there is no routable hostname at all. Routing on the headers is what lets
/// one proxy serve an SDK in that mode, which [`route_of`] alone cannot.
///
/// The port defaults to [`ENVD_PORT`] rather than failing: a client that
/// names a sandbox and no port means its envd, which is the only service the
/// SDK addresses this way.
///
/// This is client-supplied, exactly as the hostname is. Neither is an
/// authorization boundary -- the proxy resolves a name to a local listener
/// and nothing more, so anything that must not be reachable must not be in
/// the route table.
#[must_use]
pub fn route_of_headers(headers: &hyper::HeaderMap) -> Option<(u16, &str)> {
    let sandbox = headers.get("e2b-sandbox-id")?.to_str().ok()?;
    if sandbox.is_empty() {
        return None;
    }
    let port = headers
        .get("e2b-sandbox-port")
        .and_then(|value| value.to_str().ok())
        .map_or(Some(ENVD_PORT), |value| value.parse().ok())?;
    Some((port, sandbox))
}

/// Serve the proxy on `listen` until `shutdown` fires.
///
/// # Errors
///
/// Fails if `listen` cannot be bound. A failure on one connection is logged
/// and dropped rather than ending the proxy: one client sending nonsense must
/// not take the endpoint away from the others.
pub async fn serve(
    listen: SocketAddr,
    routes: Arc<dyn SandboxRoutes>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    tracing::info!("sandbox proxy listening on {listen}");

    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => accepted?,
            _ = &mut shutdown => {
                tracing::info!("sandbox proxy shutting down");
                return Ok(());
            }
        };

        let routes = Arc::clone(&routes);
        tokio::spawn(async move {
            serve_one(stream, routes, peer).await;
        });
    }
}

/// Serve one already-accepted stream, HTTP/1.1 or HTTP/2.
///
/// Shared by the plaintext and TLS listeners, which differ only in what they
/// hand over here.
async fn serve_one<S>(stream: S, routes: Arc<dyn SandboxRoutes>, peer: SocketAddr)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req| proxy(req, Arc::clone(&routes)));
    // Both HTTP/1.1 and HTTP/2, sniffed from the connection preface.
    //
    // This was h2-only, on the reasoning that gRPC clients send the h2
    // preface directly. That reasoning was wrong about the client that
    // matters: E2B's own SDK speaks the Connect protocol over HTTP/1.1, so
    // every request it made was rejected at the preface, before any routing
    // ran -- the connection log said `http2 error` and nothing else.
    if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .serve_connection_with_upgrades(TokioIo::new(stream), service)
        .await
    {
        tracing::debug!("sandbox proxy: connection from {peer} ended: {e}");
    }
}

/// Load a PEM certificate chain and private key into a rustls server config.
///
/// ALPN advertises `h2` and nothing else: a gRPC client offers `h2` and will
/// not fall back, and a server that negotiates `http/1.1` with one produces a
/// connection that handshakes and then cannot carry a single call.
///
/// # Errors
///
/// Reports a missing or unreadable file, a PEM that contains no certificate,
/// or a key rustls will not accept.
pub fn tls_config(
    cert_pem: &std::path::Path,
    key_pem: &std::path::Path,
) -> std::io::Result<rustls::ServerConfig> {
    use std::io::{Error, ErrorKind};

    // `rustls_pki_types`, not `rustls_pemfile`: that crate is unmaintained
    // (RUSTSEC-2025-0134), and this manifest already carried a comment saying
    // so and naming this replacement. The first version of this function used
    // it anyway, adding the dependency immediately above the comment warning
    // against it. Caught by running `cargo audit`, which is what an audit is
    // for.
    use rustls_pki_types::pem::PemObject;

    let certs = rustls_pki_types::CertificateDer::pem_file_iter(cert_pem)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
    if certs.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("{} contains no certificate", cert_pem.display()),
        ));
    }

    let key = rustls_pki_types::PrivateKeyDer::from_pem_file(key_pem)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;

    // rustls 0.23 will not pick a crypto provider for you when the crate is
    // built with explicit features: without this it panics at the first
    // `ServerConfig::builder()` with "Could not automatically determine the
    // process-level CryptoProvider". The workspace enables `ring` and only
    // `ring`, so that is the one to install. Idempotent -- a second call
    // returns Err because one is already installed, which is not a failure.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(config)
}

/// Serve the proxy over TLS on `listen` until `shutdown` fires.
///
/// The same routing as [`serve`]; only the transport differs.
///
/// # Errors
///
/// Fails if `listen` cannot be bound. A handshake that fails is logged and
/// dropped: one client offering the wrong protocol must not take the endpoint
/// away from the others.
pub async fn serve_tls(
    listen: SocketAddr,
    routes: Arc<dyn SandboxRoutes>,
    config: rustls::ServerConfig,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(listen).await?;
    tracing::info!("sandbox proxy listening on {listen} (TLS)");

    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => accepted?,
            _ = &mut shutdown => {
                tracing::info!("sandbox proxy shutting down");
                return Ok(());
            }
        };

        let routes = Arc::clone(&routes);
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls) => serve_one(tls, routes, peer).await,
                Err(e) => tracing::debug!("sandbox proxy: TLS handshake with {peer} failed: {e}"),
            }
        });
    }
}

/// Route one request and stream it through.
async fn proxy(
    mut req: Request<Incoming>,
    routes: Arc<dyn SandboxRoutes>,
) -> Result<Response<ProxyBody>, hyper::Error> {
    // `:authority` on an h2 request; `Host` if a client sent one instead.
    let authority = req
        .uri()
        .authority()
        .map(|a| a.as_str().to_owned())
        .or_else(|| {
            req.headers()
                .get(hyper::header::HOST)
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned)
        });

    let grpc = is_grpc(&req);

    // Headers first. A client that sends them has said outright which
    // sandbox it wants, while a hostname has to be parsed and may just be
    // whatever name the proxy was reached by -- `localhost:49983` in the
    // SDK's own default mode, which names no sandbox at all.
    let mut route = route_of_headers(req.headers())
        .map(|(port, sandbox)| (port, sandbox.to_owned()))
        .or_else(|| {
            authority
                .as_deref()
                .and_then(route_of)
                .map(|(port, sandbox)| (port, sandbox.to_owned()))
        });

    if route.is_none() {
        if let Some(authority) = authority.as_deref() {
            route = routes.resolve_hostname(authority).await;
        }
    }

    let Some((port, sandbox)) = route else {
        return Ok(refuse(
            grpc,
            StatusCode::BAD_REQUEST,
            grpc_status::INVALID_ARGUMENT,
            "nothing to route on: no e2b-sandbox-id header, and the authority \
             is neither {port}-{sandboxID}.{domain} nor a bound custom domain",
        ));
    };
    let sandbox = sandbox.as_str();

    if let Err(denied) = routes.admit_request(sandbox, port, req.headers_mut()).await {
        let mut response = refuse(
            grpc,
            StatusCode::UNAUTHORIZED,
            16,
            "sandbox URL authentication required",
        );
        if let Some(challenge) = denied.challenge {
            response.headers_mut().insert(
                hyper::header::WWW_AUTHENTICATE,
                hyper::header::HeaderValue::from_static(challenge),
            );
        }
        response.headers_mut().insert(
            hyper::header::CACHE_CONTROL,
            hyper::header::HeaderValue::from_static("no-store"),
        );
        return Ok(response);
    }

    let Some((mut target, mut in_flight)) = routes.open(sandbox, port).await else {
        return Ok(refuse(
            grpc,
            StatusCode::NOT_FOUND,
            grpc_status::NOT_FOUND,
            "no sandbox is serving that name and port",
        ));
    };

    // One retry, after telling the routes their answer failed: a cached
    // route can name a node that has gone while the sandbox moved on --
    // paused into shared storage when the node drained, resumed by another.
    let mut retried = false;
    let stream = loop {
        match TcpStream::connect(target).await {
            Ok(stream) => break stream,
            Err(e) if !retried => {
                tracing::debug!("sandbox proxy: {sandbox} at {target}: {e}; asking again");
                retried = true;
                routes.forget(sandbox).await;
                match routes.open(sandbox, port).await {
                    Some((again, guard)) => {
                        target = again;
                        in_flight = guard;
                    }
                    None => {
                        return Ok(refuse(
                            grpc,
                            StatusCode::NOT_FOUND,
                            grpc_status::NOT_FOUND,
                            "no sandbox is serving that name and port",
                        ))
                    }
                }
            }
            Err(e) => {
                tracing::warn!("sandbox proxy: {sandbox} port {port} at {target}: {e}");
                return Ok(refuse(
                    grpc,
                    StatusCode::BAD_GATEWAY,
                    grpc_status::UNAVAILABLE,
                    "the sandbox's listener did not accept a connection",
                ));
            }
        }
    };

    // Over TLS when the routes say so -- a control plane relaying to a
    // node's proxy, with a client certificate -- and plain otherwise, as a
    // node relaying to its own sandbox's loopback listener is.
    let stream: Box<dyn BackendIo> = match routes.backend_tls() {
        None => Box::new(stream),
        Some((config, name)) => {
            match tokio_rustls::TlsConnector::from(config)
                .connect(name, stream)
                .await
            {
                Ok(tls) => Box::new(tls),
                Err(e) => {
                    tracing::warn!("sandbox proxy: TLS with {target} failed: {e}");
                    return Ok(refuse(
                        grpc,
                        StatusCode::BAD_GATEWAY,
                        grpc_status::UNAVAILABLE,
                        "the sandbox's node refused this proxy's certificate, or presented one \
                         this proxy does not trust",
                    ));
                }
            }
        }
    };
    // envd's port is gRPC and Connect over HTTP/2; every other port is
    // whatever the sandbox serves there, which is HTTP/1.1 far more often,
    // and WebSockets need its upgrades.
    if port != ENVD_PORT {
        let mut response = relay_http1(req, stream, target, sandbox, port, in_flight).await;
        routes.prepare_response(sandbox, port, response.headers_mut());
        return Ok(response);
    }
    let (mut sender, connection) =
        match hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
        {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("sandbox proxy: h2 handshake with {target} failed: {e}");
                return Ok(refuse(
                    grpc,
                    StatusCode::BAD_GATEWAY,
                    grpc_status::UNAVAILABLE,
                    "the sandbox's listener is not speaking HTTP/2",
                ));
            }
        };

    // The connection drives the stream; dropping this task would stall the
    // body mid-transfer.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!("sandbox proxy: backend connection ended: {e}");
        }
    });

    // Rewrite the authority to the backend's own, and keep everything else --
    // path, method, and the headers gRPC carries its metadata in.
    let (mut parts, body) = req.into_parts();
    // Retain the public hostname for virtual-host applications even when
    // HTTP/2's authority is rewritten for the next proxy hop.
    if !parts.headers.contains_key(hyper::header::HOST) {
        if let Some(value) = parts
            .uri
            .authority()
            .and_then(|authority| hyper::header::HeaderValue::from_str(authority.as_str()).ok())
        {
            parts.headers.insert(hyper::header::HOST, value);
        }
    }
    // The route goes with the request as headers, because rewriting the
    // authority erases a route that was read from it: a proxy in front of
    // another proxy -- a cluster's control plane in front of a node -- would
    // otherwise hand the second one a request it cannot route.
    if !parts.headers.contains_key("e2b-sandbox-id") {
        if let Ok(value) = hyper::header::HeaderValue::from_str(sandbox) {
            parts.headers.insert("e2b-sandbox-id", value);
            parts
                .headers
                .insert("e2b-sandbox-port", hyper::header::HeaderValue::from(port));
        }
    }
    let path = parts
        .uri
        .path_and_query()
        .map_or_else(|| "/".to_owned(), ToString::to_string);
    parts.uri = match format!("http://{target}{path}").parse() {
        Ok(uri) => uri,
        Err(e) => {
            tracing::warn!("sandbox proxy: could not build a target URI: {e}");
            return Ok(refuse(
                grpc,
                StatusCode::BAD_REQUEST,
                grpc_status::INVALID_ARGUMENT,
                "unroutable request path",
            ));
        }
    };

    match sender.send_request(Request::from_parts(parts, body)).await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            let body = body
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                .boxed();
            // The guard goes with the response body, which is the exchange's
            // last part to finish: a streamed command's output is still
            // arriving long after the headers. Not with the connection task,
            // where it once was -- that future resolves as soon as the
            // request's sender is dropped, while hyper goes on driving the
            // stream by itself, so a command 40 ms into its run already
            // counted as finished and its sandbox was paused under it.
            Ok(Response::from_parts(
                parts,
                GuardedBody {
                    inner: body,
                    _in_flight: in_flight,
                }
                .boxed(),
            ))
        }
        Err(e) => {
            tracing::warn!("sandbox proxy: forwarding to {target} failed: {e}");
            Ok(refuse(
                grpc,
                StatusCode::BAD_GATEWAY,
                grpc_status::UNAVAILABLE,
                "the sandbox's listener did not answer",
            ))
        }
    }
}

/// One request to a sandbox's own port, over HTTP/1.1. An upgrade -- a
/// WebSocket -- is answered with the backend's `101` and then spliced: bytes
/// both ways between the two upgraded connections until either closes, the
/// in-flight guard held all the while.
async fn relay_http1(
    mut req: Request<Incoming>,
    stream: Box<dyn BackendIo>,
    target: SocketAddr,
    sandbox: &str,
    port: u16,
    in_flight: InFlight,
) -> Response<ProxyBody> {
    let (mut sender, connection) =
        match hyper::client::conn::http1::handshake(TokioIo::new(stream)).await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("sandbox proxy: http/1.1 with {target} failed: {e}");
                return refuse(
                    false,
                    StatusCode::BAD_GATEWAY,
                    grpc_status::UNAVAILABLE,
                    "the sandbox is not serving HTTP on that port",
                );
            }
        };
    tokio::spawn(async move {
        if let Err(e) = connection.with_upgrades().await {
            tracing::debug!("sandbox proxy: backend connection ended: {e}");
        }
    });
    let client_upgrade = req
        .headers()
        .contains_key(hyper::header::UPGRADE)
        .then(|| hyper::upgrade::on(&mut req));
    let authority = req.uri().authority().map(|a| a.as_str().to_owned());
    let (mut parts, body) = req.into_parts();
    if !parts.headers.contains_key("e2b-sandbox-id") {
        if let Ok(value) = hyper::header::HeaderValue::from_str(sandbox) {
            parts.headers.insert("e2b-sandbox-id", value);
            parts
                .headers
                .insert("e2b-sandbox-port", hyper::header::HeaderValue::from(port));
        }
    }
    // HTTP/1.1 carries the authority as `Host`, which a request that came in
    // over HTTP/2 had as `:authority` instead.
    if !parts.headers.contains_key(hyper::header::HOST) {
        if let Some(value) = authority.and_then(|a| hyper::header::HeaderValue::from_str(&a).ok()) {
            parts.headers.insert(hyper::header::HOST, value);
        }
    }
    parts.version = hyper::Version::HTTP_11;
    parts.uri = match parts
        .uri
        .path_and_query()
        .map_or("/", hyper::http::uri::PathAndQuery::as_str)
        .parse()
    {
        Ok(uri) => uri,
        Err(_) => {
            return refuse(
                false,
                StatusCode::BAD_REQUEST,
                grpc_status::INVALID_ARGUMENT,
                "unroutable request path",
            )
        }
    };
    let mut response = match sender.send_request(Request::from_parts(parts, body)).await {
        Ok(response) => response,
        Err(e) => {
            tracing::warn!("sandbox proxy: forwarding to {target} failed: {e}");
            return refuse(
                false,
                StatusCode::BAD_GATEWAY,
                grpc_status::UNAVAILABLE,
                "the sandbox's port did not answer",
            );
        }
    };
    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        if let Some(client) = client_upgrade {
            let server = hyper::upgrade::on(&mut response);
            tokio::spawn(async move {
                let _held = in_flight;
                if let (Ok(client), Ok(server)) = tokio::join!(client, server) {
                    let (mut client, mut server) = (TokioIo::new(client), TokioIo::new(server));
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                }
            });
            let (parts, body) = response.into_parts();
            let body = body
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                .boxed();
            return Response::from_parts(parts, body);
        }
    }
    let (parts, body) = response.into_parts();
    let body = body
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .boxed();
    Response::from_parts(
        parts,
        GuardedBody {
            inner: body,
            _in_flight: in_flight,
        }
        .boxed(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn asynchronous_admission_finishes_before_any_backend_open() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        struct DelayedDeny {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            resolves: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl SandboxRoutes for DelayedDeny {
            async fn admit_request(
                &self,
                sandbox: &str,
                port: u16,
                _headers: &mut hyper::HeaderMap,
            ) -> Result<(), ProxyAccessDenied> {
                assert_eq!(sandbox, "sbx_owned");
                assert_eq!(port, 9000);
                self.entered.notify_one();
                self.release.notified().await;
                Err(ProxyAccessDenied {
                    challenge: Some("Basic realm=\"owned\""),
                })
            }
            async fn resolve(&self, _sandbox: &str, _port: u16) -> Option<SocketAddr> {
                self.resolves.fetch_add(1, Ordering::SeqCst);
                None
            }
        }
        for grpc in [false, true] {
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let resolves = Arc::new(AtomicUsize::new(0));
            let routes: Arc<dyn SandboxRoutes> = Arc::new(DelayedDeny {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                resolves: Arc::clone(&resolves),
            });
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, peer) = listener.accept().await.unwrap();
                serve_one(stream, routes, peer).await;
            });
            let request = tokio::spawn(async move {
                let mut request = reqwest::Client::new()
                    .get(format!("http://{address}/"))
                    .header("host", "9000-sbx_owned.test");
                if grpc {
                    request = request.header("content-type", "application/grpc");
                }
                request.send().await.unwrap()
            });
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            assert_eq!(
                resolves.load(Ordering::SeqCst),
                0,
                "pending admission must not open a route"
            );
            assert!(
                !request.is_finished(),
                "pending admission must not answer early"
            );
            release.notify_one();
            let response = tokio::time::timeout(Duration::from_secs(2), request)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.status().as_u16(), if grpc { 200 } else { 401 });
            if grpc {
                assert_eq!(response.headers()["grpc-status"], "16");
            }
            assert_eq!(
                response.headers()["www-authenticate"],
                "Basic realm=\"owned\""
            );
            assert_eq!(
                resolves.load(Ordering::SeqCst),
                0,
                "denied admission must not open a route"
            );
            server.abort();
            let _ = server.await;
        }
    }

    #[test]
    fn an_e2b_style_host_names_a_port_and_a_sandbox() {
        assert_eq!(
            route_of("9000-sbx_abc123.e2b.dev"),
            Some((9000, "sbx_abc123"))
        );
    }

    /// The proxy's own port is not the sandbox's.
    #[test]
    fn a_port_on_the_authority_itself_is_ignored() {
        assert_eq!(
            route_of("49983-sbx_abc.localhost:8080"),
            Some((49983, "sbx_abc"))
        );
    }

    /// A sandbox id may contain the separator; only the first one splits.
    #[test]
    fn only_the_first_hyphen_separates() {
        assert_eq!(
            route_of("80-sbx-with-hyphens.dev"),
            Some((80, "sbx-with-hyphens"))
        );
    }

    /// A bare host is a request for the control plane. Routing it to some
    /// sandbox because it happened to parse would be worse than refusing.
    #[test]
    fn a_host_with_no_port_prefix_is_not_a_sandbox() {
        assert_eq!(route_of("api.e2b.dev"), None);
        assert_eq!(route_of("localhost"), None);
        assert_eq!(route_of("localhost:3980"), None);
    }

    #[test]
    fn a_malformed_prefix_is_refused_rather_than_guessed() {
        assert_eq!(route_of("notaport-sbx.dev"), None, "port must be a number");
    }

    fn headers(pairs: &[(&str, &str)]) -> hyper::HeaderMap {
        let mut map = hyper::HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                hyper::header::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                hyper::header::HeaderValue::from_str(value).expect("value"),
            );
        }
        map
    }

    #[test]
    fn the_sdks_own_headers_name_a_sandbox() {
        // Exactly what e2b's Python SDK sends (connectrpc/0.11.1), captured
        // from a real client: it addresses the sandbox as localhost:49983 and
        // puts the identity in headers, so there is no hostname to route on.
        assert_eq!(
            route_of_headers(&headers(&[
                ("e2b-sandbox-id", "sbx_test"),
                ("e2b-sandbox-port", "49983"),
            ])),
            Some((49983, "sbx_test"))
        );
    }

    #[test]
    fn a_sandbox_named_without_a_port_means_its_envd() {
        assert_eq!(
            route_of_headers(&headers(&[("e2b-sandbox-id", "sbx_test")])),
            Some((ENVD_PORT, "sbx_test"))
        );
    }

    #[test]
    fn a_header_route_that_is_not_one_is_refused_rather_than_guessed() {
        // No id at all: this request is not for a sandbox.
        assert_eq!(route_of_headers(&headers(&[])), None);
        assert_eq!(route_of_headers(&headers(&[("e2b-sandbox-id", "")])), None);
        // A port that is not a number is a malformed request, not a request
        // for the default port -- guessing would send it to the wrong place.
        assert_eq!(
            route_of_headers(&headers(&[
                ("e2b-sandbox-id", "sbx_test"),
                ("e2b-sandbox-port", "http"),
            ])),
            None
        );
        assert_eq!(route_of("9000-.dev"), None, "sandbox must not be empty");
        assert_eq!(route_of("99999-sbx.dev"), None, "port must fit in u16");
    }

    #[tokio::test]
    async fn a_port_map_answers_only_for_what_it_holds() {
        let map = PortMap::new();
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        map.insert("sbx_a", 49983, addr);

        assert_eq!(map.resolve("sbx_a", 49983).await, Some(addr));
        assert_eq!(
            map.resolve("sbx_a", 22).await,
            None,
            "a port it does not serve"
        );
        assert_eq!(
            map.resolve("sbx_b", 49983).await,
            None,
            "a sandbox it has never seen"
        );
    }

    /// A destroyed sandbox takes all of its ports with it, or the next request
    /// is routed at a VM that is gone.
    #[tokio::test]
    async fn removing_a_sandbox_removes_every_port_it_had() {
        let map = PortMap::new();
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        map.insert("sbx_a", 49983, addr);
        map.insert("sbx_a", 8080, addr);
        map.insert("sbx_b", 49983, addr);

        map.remove_sandbox("sbx_a");
        assert_eq!(map.resolve("sbx_a", 49983).await, None);
        assert_eq!(map.resolve("sbx_a", 8080).await, None);
        assert_eq!(
            map.resolve("sbx_b", 49983).await,
            Some(addr),
            "and only that one"
        );
    }

    /// The in-flight guard lasts exactly as long as the response body: held
    /// while it is still being read, released once it is done with. It was
    /// once tied to the backend connection's future, which ends when the
    /// request's sender is dropped -- long before a streamed body is -- so a
    /// sandbox mid-command counted as idle and was paused under it.
    #[tokio::test]
    async fn the_in_flight_guard_lives_as_long_as_the_body() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct Released(Arc<AtomicBool>);
        impl Drop for Released {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let released = Arc::new(AtomicBool::new(false));
        let (tx, rx) = tokio::sync::mpsc::channel::<
            Result<hyper::body::Frame<Bytes>, Box<dyn std::error::Error + Send + Sync>>,
        >(4);
        let inner =
            http_body_util::StreamBody::new(tokio_stream::wrappers::ReceiverStream::new(rx))
                .boxed();
        let body = GuardedBody {
            inner,
            _in_flight: InFlight::new(Released(Arc::clone(&released))),
        };
        let reading = tokio::spawn(body.collect());

        tx.send(Ok(hyper::body::Frame::data(Bytes::from_static(b"first"))))
            .await
            .expect("the body is reading");
        tokio::task::yield_now().await;
        assert!(
            !released.load(Ordering::SeqCst),
            "a body still streaming holds its guard"
        );

        drop(tx);
        let collected = reading.await.expect("joined").expect("a clean body");
        assert_eq!(collected.to_bytes(), Bytes::from_static(b"first"));
        assert!(
            released.load(Ordering::SeqCst),
            "a finished body releases it"
        );
    }
}
