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

    /// Answer a request here instead of forwarding it: a sign-in redirect, or
    /// the step that turns a sign-in handoff into a cookie for this host. Sees
    /// the URI, which admission does not. `None`, the default, forwards.
    async fn intercept(
        &self,
        _sandbox: &str,
        _port: u16,
        _uri: &hyper::Uri,
        _headers: &hyper::HeaderMap,
    ) -> Option<ProxyAnswer> {
        None
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

    /// The authentication line to send on each new connection to a backend,
    /// before any request bytes: a control plane relaying to a node's proxy
    /// proves it holds the cluster token this way. `None`, the default, sends
    /// nothing -- right for a loopback listener of a sandbox's own.
    fn backend_preamble(&self) -> Option<Vec<u8>> {
        None
    }

    /// What this proxy's own listener does with a preamble on an accepted
    /// connection. The default expects none: a public-facing proxy has no use
    /// for the line, and must not treat client bytes as one.
    fn preamble_policy(&self) -> PreamblePolicy {
        PreamblePolicy::Ignore
    }
}

/// The tag a proxy authentication line starts with.
pub const PREAMBLE_TAG: &str = "HV2-PROXY";
/// The longest authentication line a proxy will read, line ending included.
pub const PREAMBLE_MAX_LEN: usize = 256;
/// How long a connection has to deliver its authentication line.
pub const PREAMBLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// How far a line's timestamp may be from the verifier's clock, either way.
pub const PREAMBLE_SKEW_SECS: i64 = 60;
/// How many nonces a verifier remembers; a full memory refuses rather than
/// forgets, because forgetting would let a captured line be replayed.
const PREAMBLE_NONCE_CAPACITY: usize = 100_000;

/// What a listener does with the authentication line a client may send first.
#[derive(Clone)]
pub enum PreamblePolicy {
    /// Do not look for one: the bytes are the client's own.
    Ignore,
    /// Accept a line if one is sent and drop it unchecked; a connection
    /// without one proceeds. For a node with no token to check against, or one
    /// started with `--proxy-open`, that a current control plane still relays
    /// to.
    Strip,
    /// Refuse every connection that does not begin with a valid line.
    Require(Arc<PreambleVerifier>),
}

/// Checks the authentication lines of one node's proxy.
pub struct PreambleVerifier {
    token: String,
    /// Nonces accepted within the skew window, by the second they were sent.
    seen: Mutex<HashMap<String, i64>>,
}

impl PreambleVerifier {
    #[must_use]
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Whether `line` -- without its line ending -- is an authentication line
    /// made with this verifier's token, recently, and not seen before.
    #[must_use]
    pub fn verify(&self, line: &str, now: i64) -> bool {
        use subtle::ConstantTimeEq;

        let mut parts = line.split(' ');
        let (Some(tag), Some(mac), Some(nonce), Some(ts), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return false;
        };
        if tag != PREAMBLE_TAG
            || nonce.len() < 16
            || nonce.len() > 64
            || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return false;
        }
        let Ok(ts) = ts.parse::<i64>() else {
            return false;
        };
        if ts.saturating_sub(now).abs() > PREAMBLE_SKEW_SECS {
            return false;
        }
        let expected = preamble_mac(&self.token, nonce, ts);
        let genuine =
            mac.len() == expected.len() && bool::from(mac.as_bytes().ct_eq(expected.as_bytes()));
        if !genuine {
            return false;
        }
        // Only a genuine line is remembered, so a stranger cannot fill the
        // memory with nonces of their own choosing.
        let mut seen = self.seen.lock();
        if seen.len() >= PREAMBLE_NONCE_CAPACITY / 2 {
            seen.retain(|_, sent| sent.saturating_sub(now).abs() <= PREAMBLE_SKEW_SECS);
        }
        if seen.len() >= PREAMBLE_NONCE_CAPACITY || seen.contains_key(nonce) {
            return false;
        }
        seen.insert(nonce.to_owned(), ts);
        true
    }
}

fn preamble_mac(token: &str, nonce: &str, ts: i64) -> String {
    crate::middleware::hmac_sha256(
        token.as_bytes(),
        format!("hv2-proxy-v1;{nonce};{ts}").as_bytes(),
    )
}

/// The authentication line for one new connection to a node's proxy:
/// `HV2-PROXY <hmac> <nonce> <unix-time>\r\n`, the HMAC-SHA256 (hex) of
/// `hv2-proxy-v1;<nonce>;<unix-time>` under the cluster token.
#[must_use]
pub fn build_preamble(token: &str) -> Vec<u8> {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    build_preamble_at(token, &nonce, chrono::Utc::now().timestamp())
}

/// [`build_preamble`] with the nonce and time given.
#[must_use]
pub fn build_preamble_at(token: &str, nonce: &str, ts: i64) -> Vec<u8> {
    format!(
        "{PREAMBLE_TAG} {} {nonce} {ts}\r\n",
        preamble_mac(token, nonce, ts)
    )
    .into_bytes()
}

/// A stream with some bytes already read from it, handed back first.
struct Replay<S> {
    prefix: Vec<u8>,
    pos: usize,
    inner: S,
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Replay<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.pos < self.prefix.len() {
            let n = (self.prefix.len() - self.pos).min(buf.remaining());
            let start = self.pos;
            buf.put_slice(&self.prefix[start..start + n]);
            self.pos += n;
            return std::task::Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Replay<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, data)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Read the authentication line off a new connection, per `policy`.
///
/// One byte at a time, so that nothing past the line is consumed: what the
/// client sent after it is the HTTP connection and goes to hyper untouched.
/// `Err` is a connection to close.
async fn read_preamble<S>(
    mut stream: S,
    policy: &PreamblePolicy,
) -> Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static, &'static str>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::AsyncReadExt;

    let tag = format!("{PREAMBLE_TAG} ");
    let required = matches!(policy, PreamblePolicy::Require(_));
    let mut line: Vec<u8> = Vec::with_capacity(64);
    let body = async {
        loop {
            let mut byte = [0u8; 1];
            if stream.read_exact(&mut byte).await.is_err() {
                return Err("the connection ended before an authentication line");
            }
            line.push(byte[0]);
            if line.len() <= tag.len() && !tag.as_bytes().starts_with(&line) {
                // Not an authentication line. A listener that does not insist
                // on one hands what it read back to the client's own protocol.
                return if required {
                    Err("the connection did not start with an authentication line")
                } else {
                    Ok(false)
                };
            }
            if byte[0] == b'\n' {
                return Ok(true);
            }
            if line.len() >= PREAMBLE_MAX_LEN {
                return Err("the authentication line is too long");
            }
        }
    };
    // Waiting for the first byte is not the client's fault when the policy
    // does not insist: an HTTP client that connects and waits is still one.
    let found = if required {
        tokio::time::timeout(PREAMBLE_TIMEOUT, body)
            .await
            .map_err(|_| "the authentication line did not arrive in time")??
    } else {
        body.await?
    };
    if !found {
        return Ok(Replay {
            prefix: line,
            pos: 0,
            inner: stream,
        });
    }
    if let PreamblePolicy::Require(verifier) = policy {
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if !verifier.verify(text, chrono::Utc::now().timestamp()) {
            return Err("the authentication line is not valid");
        }
    }
    Ok(Replay {
        prefix: Vec::new(),
        pos: 0,
        inner: stream,
    })
}

/// A route owner's own answer to a request, sent instead of forwarding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyAnswer {
    pub status: u16,
    /// Where to send the browser, for a redirect.
    pub location: Option<String>,
    /// A cookie to set on the way.
    pub set_cookie: Option<String>,
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
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    serve_listener(listener, routes, shutdown).await
}

/// [`serve`] on a listener already bound.
///
/// For a caller that must hold the port from the moment it learns it: binding,
/// reading the address and dropping the listener to bind it again leaves a
/// window in which another process -- or another test -- takes the port.
///
/// # Errors
///
/// Fails if the listener cannot accept.
pub async fn serve_listener(
    listener: TcpListener,
    routes: Arc<dyn SandboxRoutes>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    if let Ok(listen) = listener.local_addr() {
        tracing::info!("sandbox proxy listening on {listen}");
    }

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
    match routes.preamble_policy() {
        PreamblePolicy::Ignore => serve_http(stream, routes, peer).await,
        // Strip: a line is dropped unchecked if sent; Require: it must verify.
        policy @ (PreamblePolicy::Strip | PreamblePolicy::Require(_)) => {
            match read_preamble(stream, &policy).await {
                Ok(stream) => serve_http(stream, routes, peer).await,
                Err(why) => {
                    tracing::warn!("sandbox proxy: refused a connection from {peer}: {why}");
                }
            }
        }
    }
}

/// The HTTP side of [`serve_one`], once any authentication line is read.
async fn serve_http<S>(stream: S, routes: Arc<dyn SandboxRoutes>, peer: SocketAddr)
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
    hv2_tls::install_default();

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

    if let Some(answer) = routes
        .intercept(sandbox, port, req.uri(), req.headers())
        .await
    {
        let mut response = Response::builder()
            .status(StatusCode::from_u16(answer.status).unwrap_or(StatusCode::FORBIDDEN))
            .header(hyper::header::CACHE_CONTROL, "no-store");
        if let Some(location) = &answer.location {
            response = response.header(hyper::header::LOCATION, location.as_str());
        }
        if let Some(cookie) = &answer.set_cookie {
            response = response.header(hyper::header::SET_COOKIE, cookie.as_str());
        }
        return Ok(response
            .body(
                http_body_util::Empty::<Bytes>::new()
                    .map_err(|never| match never {})
                    .boxed(),
            )
            .unwrap_or_else(|_| {
                refuse(grpc, StatusCode::BAD_GATEWAY, 13, "proxy answer invalid")
            }));
    }

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
    // A node that holds a cluster token wants proof of it before anything
    // else: the line goes first on the connection, ahead of any request.
    let mut stream = stream;
    if let Some(line) = routes.backend_preamble() {
        use tokio::io::AsyncWriteExt;
        if let Err(e) = stream.write_all(&line).await {
            tracing::warn!("sandbox proxy: {target} did not take the authentication line: {e}");
            return Ok(refuse(
                grpc,
                StatusCode::BAD_GATEWAY,
                grpc_status::UNAVAILABLE,
                "the sandbox's listener did not accept a connection",
            ));
        }
    }
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

    // ---- the proxy authentication line -------------------------------------

    const TOKEN: &str = "cluster-secret-for-the-proxy-tests";
    const NONCE: &str = "0123456789abcdef0123456789abcdef";

    fn line_text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    }

    #[test]
    fn a_genuine_line_verifies_once_and_a_replay_does_not() {
        let now = 1_700_000_000;
        let verifier = PreambleVerifier::new(TOKEN);
        let line = line_text(&build_preamble_at(TOKEN, NONCE, now));
        assert!(verifier.verify(&line, now));
        assert!(
            !verifier.verify(&line, now),
            "the same nonce twice is a replay"
        );
    }

    #[test]
    fn forged_stale_and_malformed_lines_are_refused() {
        let now = 1_700_000_000;
        let verifier = PreambleVerifier::new(TOKEN);
        let wrong_key = line_text(&build_preamble_at("another-token", NONCE, now));
        assert!(!verifier.verify(&wrong_key, now));
        let stale = line_text(&build_preamble_at(TOKEN, NONCE, now - 61));
        assert!(!verifier.verify(&stale, now));
        let future = line_text(&build_preamble_at(TOKEN, NONCE, now + 61));
        assert!(!verifier.verify(&future, now));
        // A good MAC for one timestamp does not carry to another.
        let good = line_text(&build_preamble_at(TOKEN, NONCE, now));
        let moved = good.replacen(&now.to_string(), &(now + 1).to_string(), 1);
        assert!(!verifier.verify(&moved, now));
        for bad in [
            "",
            "HV2-PROXY",
            "HV2-PROXY a b",
            "HV2-PROXY a b c d",
            "GET / HTTP/1.1",
        ] {
            assert!(!verifier.verify(bad, now), "{bad}");
        }
        // Within the window on both sides is fine.
        let early = line_text(&build_preamble_at(TOKEN, "aaaaaaaaaaaaaaaa", now - 60));
        assert!(verifier.verify(&early, now));
    }

    /// A routes with a policy of its own that answers every request with the
    /// proxy's "no such sandbox" -- reaching that answer means the connection
    /// was admitted and its HTTP bytes were intact.
    struct Gate(PreamblePolicy);

    #[async_trait::async_trait]
    impl SandboxRoutes for Gate {
        async fn resolve(&self, _sandbox: &str, _port: u16) -> Option<SocketAddr> {
            None
        }
        fn preamble_policy(&self) -> PreamblePolicy {
            self.0.clone()
        }
    }

    const REQUEST: &[u8] =
        b"GET / HTTP/1.1\r\nHost: 9000-sbx_one.test\r\nConnection: close\r\n\r\n";

    /// What a client gets back for `bytes` sent as one write to a proxy.
    async fn ask(policy: PreamblePolicy, bytes: &[u8]) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let routes: Arc<dyn SandboxRoutes> = Arc::new(Gate(policy));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (_stop, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(serve_listener(listener, routes, rx));
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(bytes).await.unwrap();
        let mut answer = Vec::new();
        // A refused connection is closed or reset; either ends the read.
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.read_to_end(&mut answer),
        )
        .await;
        String::from_utf8_lossy(&answer).into_owned()
    }

    fn required() -> PreamblePolicy {
        PreamblePolicy::Require(Arc::new(PreambleVerifier::new(TOKEN)))
    }

    #[tokio::test]
    async fn a_tokened_proxy_refuses_a_connection_with_no_line() {
        let answer = ask(required(), REQUEST).await;
        assert_eq!(answer, "", "no line, no answer: {answer}");
    }

    #[tokio::test]
    async fn a_tokened_proxy_refuses_forged_and_stale_lines() {
        let now = chrono::Utc::now().timestamp();
        for line in [
            build_preamble_at("not-the-token", NONCE, now),
            build_preamble_at(TOKEN, NONCE, now - 3600),
            b"HV2-PROXY nonsense\r\n".to_vec(),
        ] {
            let mut bytes = line;
            bytes.extend_from_slice(REQUEST);
            let answer = ask(required(), &bytes).await;
            assert_eq!(answer, "", "a bad line gets no answer: {answer}");
        }
    }

    #[tokio::test]
    async fn a_tokened_proxy_accepts_a_valid_line_and_passes_the_request_on_unchanged() {
        let mut bytes = build_preamble(TOKEN);
        bytes.extend_from_slice(REQUEST);
        let answer = ask(required(), &bytes).await;
        assert!(
            answer.starts_with("HTTP/1.1 404"),
            "the request after the line reached the proxy intact: {answer}"
        );
    }

    #[tokio::test]
    async fn an_open_proxy_accepts_plain_requests_and_drops_a_line_if_sent() {
        let plain = ask(PreamblePolicy::Strip, REQUEST).await;
        assert!(plain.starts_with("HTTP/1.1 404"), "{plain}");
        // A control plane with the token in front of a node without one.
        let mut with_line = build_preamble(TOKEN);
        with_line.extend_from_slice(REQUEST);
        let stripped = ask(PreamblePolicy::Strip, &with_line).await;
        assert!(stripped.starts_with("HTTP/1.1 404"), "{stripped}");
        // Bytes that merely begin like the tag are the client's own.
        let hv = ask(
            PreamblePolicy::Strip,
            b"HEAD / HTTP/1.1\r\nHost: 9000-sbx_one.test\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(hv.starts_with("HTTP/1.1 404"), "{hv}");
    }

    #[tokio::test]
    async fn a_proxy_relaying_with_a_line_sends_it_first_and_the_request_after() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        struct Relay(SocketAddr);
        #[async_trait::async_trait]
        impl SandboxRoutes for Relay {
            async fn resolve(&self, _sandbox: &str, _port: u16) -> Option<SocketAddr> {
                Some(self.0)
            }
            fn backend_preamble(&self) -> Option<Vec<u8>> {
                Some(build_preamble(TOKEN))
            }
        }
        // A node's proxy, here a socket that reads what arrives first.
        let node = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let node_address = node.local_addr().unwrap();
        let node_side = tokio::spawn(async move {
            let (mut stream, _) = node.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut byte = [0u8; 1];
            while !seen.ends_with(b"\r\n") {
                stream.read_exact(&mut byte).await.unwrap();
                seen.push(byte[0]);
            }
            let mut rest = vec![0u8; 4];
            stream.read_exact(&mut rest).await.unwrap();
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            (seen, rest)
        });
        let routes: Arc<dyn SandboxRoutes> = Arc::new(Relay(node_address));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (_stop, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(serve_listener(listener, routes, rx));
        let mut client = TcpStream::connect(address).await.unwrap();
        // Not envd's port, so it is relayed as HTTP/1.1.
        client.write_all(REQUEST).await.unwrap();
        let mut answer = Vec::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.read_to_end(&mut answer),
        )
        .await;
        let (line, rest) = tokio::time::timeout(std::time::Duration::from_secs(3), node_side)
            .await
            .unwrap()
            .unwrap();
        let verifier = PreambleVerifier::new(TOKEN);
        assert!(
            verifier.verify(&line_text(&line), chrono::Utc::now().timestamp()),
            "the first bytes to the node are a valid line: {}",
            String::from_utf8_lossy(&line)
        );
        assert_eq!(&rest, b"GET ", "the request follows the line");
    }
}
