//! Does the proxy actually carry an HTTP/2 request to the right sandbox?
//!
//! The unit tests next to `sandbox_proxy` check the parsing, which is the part
//! that is easy to check and not the part that would embarrass anyone. This
//! runs the thing: a real HTTP/2 backend on one port, the proxy on another,
//! and a real h2 client asking for a hostname.
//!
//! No VM and no gRPC service, deliberately. What is under test is the routing
//! and the forwarding; putting a guest behind it would make the test need
//! `/dev/kvm` and a kernel image to tell us something about `hyper`.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::{TcpListener, TcpStream};

use hv2_api::sandbox_proxy::{serve, PortMap};

/// A backend that answers every request by describing what it received, so the
/// test can assert on what actually arrived rather than on a status code.
async fn spawn_backend() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind backend");
    let addr = listener.local_addr().expect("backend addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let service = service_fn(|req: Request<Incoming>| async move {
                    let authority = req
                        .uri()
                        .authority()
                        .map_or_else(|| "(none)".to_owned(), ToString::to_string);
                    let body = format!(
                        "method={} path={} authority={} grpc={}",
                        req.method(),
                        req.uri().path(),
                        authority,
                        req.headers()
                            .get("x-grpc-like")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("(absent)")
                    );
                    Ok::<_, hyper::Error>(Response::new(Full::new(Bytes::from(body))))
                });
                let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });

    addr
}

/// Send one h2 request to `proxy` asking for `authority`, declaring itself
/// gRPC, and return the status plus the gRPC status header if there is one.
async fn grpc_through_proxy(
    proxy: SocketAddr,
    authority: &str,
) -> (StatusCode, Option<String>, Option<String>, usize) {
    let stream = TcpStream::connect(proxy).await.expect("connect to proxy");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2 handshake with proxy");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("http://{authority}/process.Process/Start"))
        .header("content-type", "application/grpc")
        .body(Empty::<Bytes>::new())
        .expect("build request");

    let response = sender.send_request(request).await.expect("send");
    let status = response.status();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let grpc_status = header("grpc-status");
    let grpc_message = header("grpc-message");
    // The body length matters: a gRPC client decodes a body as length-prefixed
    // messages, so an error must carry none.
    let body_len = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes()
        .len();
    (status, grpc_status, grpc_message, body_len)
}

/// Send one h2 request to `proxy` asking for `authority`.
async fn through_proxy(proxy: SocketAddr, authority: &str, path: &str) -> (StatusCode, String) {
    let stream = TcpStream::connect(proxy).await.expect("connect to proxy");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2 handshake with proxy");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("http://{authority}{path}"))
        .header("x-grpc-like", "carried-through")
        .body(Empty::<Bytes>::new())
        .expect("build request");

    let response = sender
        .send_request(request)
        .await
        .expect("send through proxy");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// Start the proxy on an ephemeral port; returns where it is listening and the
/// shutdown sender.
///
/// The sender comes back rather than being dropped here because dropping it
/// *is* the shutdown signal -- a oneshot receiver resolves when its sender
/// goes away. The first version of this helper bound it as `_tx` and let it
/// fall out of scope, so every proxy stopped before the test could connect and
/// every test failed with a connection reset.
async fn spawn_proxy(routes: Arc<PortMap>) -> (SocketAddr, tokio::sync::oneshot::Sender<()>) {
    // Bind first to learn the port, drop, and let the proxy rebind it. A race
    // in principle; in a test on loopback with an ephemeral port, the kernel
    // does not hand the same one out again in the microseconds between.
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("probe bind");
    let addr = probe.local_addr().expect("probe addr");
    drop(probe);

    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = serve(addr, routes, rx).await;
    });

    // Wait for it to be accepting rather than sleeping a fixed time.
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return (addr, tx);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("proxy never started listening on {addr}");
}

/// The whole point: a request for `{port}-{sandbox}` reaches that sandbox's
/// listener, with its path and headers intact.
#[tokio::test]
async fn a_request_for_a_sandbox_hostname_reaches_that_sandbox() {
    let backend = spawn_backend().await;

    let routes = Arc::new(PortMap::new());
    routes.insert("sbx_test", 49983, backend);
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    let (status, body) = through_proxy(
        proxy,
        "49983-sbx_test.hypermachine.local",
        "/process.Process/Start",
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(
        body.contains("path=/process.Process/Start"),
        "the path must survive the hop, or gRPC cannot name a method: {body}"
    );
    assert!(
        body.contains("grpc=carried-through"),
        "headers must survive too, since gRPC metadata rides in them: {body}"
    );
    assert!(
        body.contains(&format!("authority={backend}")),
        "the authority should be rewritten to the backend's own: {body}"
    );
}

/// A name that parses but belongs to no sandbox is refused, rather than being
/// sent to whichever sandbox happens to be first.
#[tokio::test]
async fn an_unknown_sandbox_is_not_routed_anywhere() {
    let routes = Arc::new(PortMap::new());
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    let (status, _) = through_proxy(proxy, "49983-sbx_nothere.local", "/x").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A bare hostname is a request for the control plane. It must not be guessed
/// into a sandbox.
#[tokio::test]
async fn a_hostname_with_no_sandbox_in_it_is_refused() {
    let routes = Arc::new(PortMap::new());
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    let (status, body) = through_proxy(proxy, "api.hypermachine.local", "/sandboxes").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// A route whose backend is not listening answers 502 rather than hanging or
/// dying: the sandbox's VM may have gone away between the map and the dial.
#[tokio::test]
async fn a_sandbox_whose_listener_is_gone_answers_bad_gateway() {
    // Bind and drop, so the address is real and refuses connections.
    let dead = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dead_addr = dead.local_addr().expect("addr");
    drop(dead);

    let routes = Arc::new(PortMap::new());
    routes.insert("sbx_dead", 49983, dead_addr);
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    let (status, _) = through_proxy(proxy, "49983-sbx_dead.local", "/x").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

/// A gRPC client cannot read an HTTP status. Refusing one with a bare 404
/// reaches it as `Unimplemented ... malformed header: missing HTTP
/// content-type`, which names neither the problem nor the sandbox -- observed
/// with grpcurl against a sandbox that had just been deleted.
#[tokio::test]
async fn a_grpc_client_is_refused_in_grpc_terms() {
    let routes = Arc::new(PortMap::new());
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    // Unknown sandbox: NOT_FOUND is 5.
    let (status, grpc_status, message, body_len) =
        grpc_through_proxy(proxy, "49983-sbx_gone.local").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "gRPC carries its status in a header"
    );
    assert_eq!(grpc_status.as_deref(), Some("5"));
    assert!(
        message.unwrap_or_default().contains("no sandbox"),
        "and says which problem it was"
    );
    assert_eq!(
        body_len, 0,
        "a gRPC error carries no body: a client decodes one as a length-prefixed          message and reports a nonsense length"
    );

    // A name that is not a sandbox at all: INVALID_ARGUMENT is 3.
    let (_, grpc_status, _, _) = grpc_through_proxy(proxy, "api.local").await;
    assert_eq!(grpc_status.as_deref(), Some("3"));
}

/// A sandbox whose listener has gone is UNAVAILABLE rather than NOT_FOUND:
/// the name resolved, so telling a client the sandbox does not exist would
/// send it to create another one.
#[tokio::test]
async fn a_dead_listener_is_unavailable_not_missing() {
    let dead = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dead_addr = dead.local_addr().expect("addr");
    drop(dead);

    let routes = Arc::new(PortMap::new());
    routes.insert("sbx_dead", 49983, dead_addr);
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    let (_, grpc_status, _, _) = grpc_through_proxy(proxy, "49983-sbx_dead.local").await;
    assert_eq!(grpc_status.as_deref(), Some("14"));
}

/// A non-gRPC client still gets the HTTP status, since that is what it reads.
#[tokio::test]
async fn a_plain_http_client_still_gets_an_http_status() {
    let routes = Arc::new(PortMap::new());
    let (proxy, _shutdown) = spawn_proxy(Arc::clone(&routes)).await;

    let (status, _) = through_proxy(proxy, "49983-sbx_gone.local", "/x").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
