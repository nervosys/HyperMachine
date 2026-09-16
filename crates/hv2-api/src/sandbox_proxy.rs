//! One port in front of every sandbox, routed by the name the client asks for.
//!
//! # Why this exists
//!
//! `e2b_compat`'s `POST /sandboxes` returns a `processPort`, and the sandbox's
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
//! # Why it terminates HTTP/2 rather than forwarding bytes
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
//! # What it does not do
//!
//! No TLS: E2B's real endpoints are HTTPS, and a client that insists on it
//! cannot use this yet. No connection reuse either -- each proxied request
//! opens its own connection to the backend, which is a cost a busy proxy would
//! not pay and is not worth hiding behind a pool until something measures it.

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
/// control plane -- `e2b_compat` holds the sandboxes -- and a proxy that owned
/// the registry would have to be told about every creation and deletion. This
/// way it asks.
pub trait SandboxRoutes: Send + Sync + 'static {
    /// The local address serving `port` for `sandbox`, if that sandbox exists.
    fn resolve(&self, sandbox: &str, port: u16) -> Option<SocketAddr>;
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

impl SandboxRoutes for PortMap {
    fn resolve(&self, sandbox: &str, port: u16) -> Option<SocketAddr> {
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
            let service = service_fn(move |req| proxy(req, Arc::clone(&routes)));
            // HTTP/2 without a prior upgrade: gRPC clients send the h2 preface
            // directly, and there is no h1 traffic to this port to negotiate
            // away from.
            if let Err(e) = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                tracing::debug!("sandbox proxy: connection from {peer} ended: {e}");
            }
        });
    }
}

/// Route one request and stream it through.
async fn proxy(
    req: Request<Incoming>,
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

    let Some(authority) = authority else {
        return Ok(refuse(
            grpc,
            StatusCode::BAD_REQUEST,
            grpc_status::INVALID_ARGUMENT,
            "no :authority and no Host header; nothing to route on",
        ));
    };

    let Some((port, sandbox)) = route_of(&authority) else {
        return Ok(refuse(
            grpc,
            StatusCode::BAD_REQUEST,
            grpc_status::INVALID_ARGUMENT,
            "authority is not {port}-{sandboxID}.{domain}",
        ));
    };

    let Some(target) = routes.resolve(sandbox, port) else {
        return Ok(refuse(
            grpc,
            StatusCode::NOT_FOUND,
            grpc_status::NOT_FOUND,
            "no sandbox is serving that name and port",
        ));
    };

    let stream = match TcpStream::connect(target).await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::warn!("sandbox proxy: {sandbox} port {port} at {target}: {e}");
            return Ok(refuse(
                grpc,
                StatusCode::BAD_GATEWAY,
                grpc_status::UNAVAILABLE,
                "the sandbox's listener did not accept a connection",
            ));
        }
    };

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
            Ok(Response::from_parts(
                parts,
                body.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(route_of("9000-.dev"), None, "sandbox must not be empty");
        assert_eq!(route_of("99999-sbx.dev"), None, "port must fit in u16");
    }

    #[test]
    fn a_port_map_answers_only_for_what_it_holds() {
        let map = PortMap::new();
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        map.insert("sbx_a", 49983, addr);

        assert_eq!(map.resolve("sbx_a", 49983), Some(addr));
        assert_eq!(map.resolve("sbx_a", 22), None, "a port it does not serve");
        assert_eq!(
            map.resolve("sbx_b", 49983),
            None,
            "a sandbox it has never seen"
        );
    }

    /// A destroyed sandbox takes all of its ports with it, or the next request
    /// is routed at a VM that is gone.
    #[test]
    fn removing_a_sandbox_removes_every_port_it_had() {
        let map = PortMap::new();
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        map.insert("sbx_a", 49983, addr);
        map.insert("sbx_a", 8080, addr);
        map.insert("sbx_b", 49983, addr);

        map.remove_sandbox("sbx_a");
        assert_eq!(map.resolve("sbx_a", 49983), None);
        assert_eq!(map.resolve("sbx_a", 8080), None);
        assert_eq!(map.resolve("sbx_b", 49983), Some(addr), "and only that one");
    }
}
