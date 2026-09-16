//! Does the Connect surface actually answer a real HTTP client?
//!
//! The unit tests beside `connect.rs` check the codec, the envelope and the
//! error table in isolation. This runs the whole thing: a real listener, a
//! real HTTP/1.1 client, real requests with the content-types and bodies the
//! E2B SDK sends -- captured from it, not guessed.
//!
//! No guest and no KVM, deliberately. What is under test is the protocol
//! surface: that a request is decoded, dispatched to the right method, and
//! answered in the shape a Connect client can read. `process.Process/List`
//! reads an in-memory table and needs no guest at all, so there is one real
//! success path here and not only error paths.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;

use hv2_agent::{AgentVM, Capability, CapabilitySet};
use hv2_api::connect::serve_on;
use hv2_api::envd_filesystem::EnvdFilesystem;
use hv2_api::envd_process::EnvdProcess;

/// A VM that was never started.
///
/// Enough to construct the services, which is all the protocol layer needs:
/// anything that would actually reach a guest fails, and failing in Connect's
/// error shape is itself worth asserting.
async fn unstarted_vm() -> Arc<AgentVM> {
    let mut capabilities = CapabilitySet::new();
    capabilities.add(Capability::GuestExec);
    Arc::new(
        AgentVM::builder()
            .name("connect-protocol-test")
            .capabilities(capabilities)
            .build()
            .await
            .expect("building a VM allocates; it does not need KVM"),
    )
}

/// Start the endpoint on an ephemeral port.
///
/// The shutdown sender comes back rather than being dropped: dropping it *is*
/// the shutdown signal, so letting it fall out of scope would stop the server
/// before the test could connect.
async fn spawn() -> (SocketAddr, tokio::sync::oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let vm = unstarted_vm().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = serve_on(
            listener,
            EnvdProcess::new(Arc::clone(&vm)),
            EnvdFilesystem::new(vm),
            rx,
        )
        .await;
    });
    (addr, tx)
}

/// One HTTP/1.1 POST, as the SDK makes them.
async fn post(
    addr: SocketAddr,
    path: &str,
    content_type: &str,
    body: &'static [u8],
) -> (StatusCode, String, String) {
    request(addr, "POST", path, content_type, body).await
}

async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    content_type: &str,
    body: &'static [u8],
) -> (StatusCode, String, String) {
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(hyper::header::HOST, "localhost")
        .header(hyper::header::CONTENT_TYPE, content_type)
        .header("connect-protocol-version", "1")
        .body(Full::new(Bytes::from_static(body)))
        .expect("build");

    let response = sender.send_request(request).await.expect("send");
    let status = response.status();
    let reply_type = response
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        reply_type,
        String::from_utf8_lossy(&body).into_owned(),
    )
}

#[tokio::test]
async fn a_unary_call_answers_in_the_codec_it_was_asked_in() {
    // `List` reads a table, so this is a genuine success path with no guest:
    // decoded, dispatched, encoded, 200.
    let (addr, _shutdown) = spawn().await;
    let (status, content_type, body) =
        post(addr, "/process.Process/List", "application/json", b"{}").await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(content_type, "application/json");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON: {body}");
    // Protobuf-JSON omits an empty repeated field rather than writing `[]`.
    assert!(
        parsed
            .get("processes")
            .is_none_or(|p| p.as_array().is_some_and(std::vec::Vec::is_empty)),
        "nothing is running, so the list is empty: {body}"
    );
}

#[tokio::test]
async fn an_empty_body_is_a_defaulted_message_over_the_wire_too() {
    // The SDK sends no body at all for a request with no fields. Rejecting
    // that would break the simplest call there is.
    let (addr, _shutdown) = spawn().await;
    let (status, _, body) = post(addr, "/process.Process/List", "application/json", b"").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

#[tokio::test]
async fn a_method_that_does_not_exist_is_unimplemented_not_missing() {
    // 501 and `unimplemented`, not 404: the endpoint is there, the method is
    // not, and Connect carries that difference in the code the client reads.
    let (addr, _shutdown) = spawn().await;
    let (status, content_type, body) = post(
        addr,
        "/process.Process/NoSuchMethod",
        "application/json",
        b"{}",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(content_type, "application/json");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["code"], "unimplemented");
    assert!(
        parsed["message"]
            .as_str()
            .is_some_and(|m| m.contains("NoSuchMethod")),
        "the message should name what was asked for: {body}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_the_message_is_the_clients_fault() {
    // 400 and `invalid_argument`. Reporting a malformed request as `internal`
    // tells the client to retry something that can never work.
    let (addr, _shutdown) = spawn().await;
    let (status, _, body) = post(
        addr,
        "/filesystem.Filesystem/ListDir",
        "application/json",
        b"{ this is not json",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["code"], "invalid_argument");
}

#[tokio::test]
async fn a_content_type_this_endpoint_does_not_speak_is_refused_readably() {
    let (addr, _shutdown) = spawn().await;
    let (status, content_type, body) =
        post(addr, "/process.Process/List", "text/plain", b"hello").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Answered as JSON whatever was asked for, because a client that sent
    // nonsense still has to be able to read why.
    assert_eq!(content_type, "application/json");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["code"], "invalid_argument");
}

#[tokio::test]
async fn a_get_is_refused_rather_than_half_supported() {
    // Connect allows GET for methods marked side-effect-free. None here are,
    // and answering one anyway would advertise a guarantee nothing checks.
    let (addr, _shutdown) = spawn().await;
    let (status, _, body) = request(
        addr,
        "GET",
        "/process.Process/List",
        "application/json",
        b"",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["code"], "unimplemented");
}

#[tokio::test]
async fn a_streaming_call_that_fails_before_it_starts_says_so_in_the_status() {
    // `Connect` on a pid nothing is running fails before any message exists,
    // so it can still be an ordinary error response. Once a stream has begun,
    // 200 has been sent and the error has to go in the last envelope instead
    // -- which is why this distinction is worth a test.
    let (addr, _shutdown) = spawn().await;
    let (status, _, body) = post(
        addr,
        "/process.Process/Connect",
        "application/connect+json",
        // Enveloped: flags 0, then a big-endian length, then the message.
        b"\x00\x00\x00\x00\x19{\"process\":{\"pid\":99999}}",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["code"], "not_found");
}

#[tokio::test]
async fn a_call_that_needs_a_guest_fails_in_connects_terms_not_by_hanging_up() {
    // This VM was never started, so there is no guest channel. What matters is
    // that the failure arrives as a Connect error a client can read, rather
    // than a dropped connection or a gRPC-shaped reply it would reject.
    let (addr, _shutdown) = spawn().await;
    let (status, content_type, body) = post(
        addr,
        "/filesystem.Filesystem/Stat",
        "application/json",
        br#"{"path":"/tmp"}"#,
    )
    .await;

    assert!(status.is_server_error(), "status {status}: {body}");
    assert_eq!(content_type, "application/json");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert!(
        parsed.get("code").is_some() && parsed.get("message").is_some(),
        "a Connect error carries both: {body}"
    );
}

#[tokio::test]
async fn grpc_on_the_same_port_still_reaches_tonic() {
    // The two protocols share every path, so the content-type is the only
    // thing keeping them apart. If Connect ever claimed a gRPC request, the
    // client would get a JSON error body it cannot parse -- and every test
    // above would still pass, because none of them speak gRPC.
    let (addr, _shutdown) = spawn().await;

    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2 handshake");
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = Request::builder()
        .method("POST")
        .uri("/process.Process/List")
        .header(hyper::header::CONTENT_TYPE, "application/grpc")
        .header("te", "trailers")
        // A gRPC frame: one compression byte, a big-endian length, then an
        // empty message.
        .body(Full::new(Bytes::from_static(b"\x00\x00\x00\x00\x00")))
        .expect("build");

    let response = sender.send_request(request).await.expect("send");
    let content_type = response
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();

    assert!(
        content_type.starts_with("application/grpc"),
        "a gRPC request must be answered by the gRPC service, not the Connect \
         one; got {content_type:?}"
    );
}
