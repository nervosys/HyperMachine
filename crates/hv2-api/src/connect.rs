//! The [Connect protocol](https://connectrpc.com/docs/protocol) for the two
//! envd services, because that is what E2B's real client speaks.
//!
//! # Why this exists
//!
//! `envd_process` and `envd_filesystem` are tonic services, so they serve
//! gRPC: HTTP/2, `application/grpc`, length-prefixed binary protobuf. The E2B
//! SDK does not speak that. Captured from the real client (`e2b` 2.50.0,
//! unmodified):
//!
//! ```text
//! POST /filesystem.Filesystem/ListDir HTTP/1.1
//!   user-agent: connectrpc/0.11.1
//!   connect-protocol-version: 1
//!   content-type: application/json
//!   [body] {"path": "/", "depth": 1}
//! ```
//!
//! The service and method names are ours exactly -- the protos are right and
//! only the transport differs. Handed a gRPC reply, the SDK says so itself:
//! `invalid content-type: 'application/grpc'; expecting 'application/json'`.
//!
//! So this is a second surface over the *same* service objects, not a second
//! implementation. `EnvdProcess` and `EnvdFilesystem` clone into shared state,
//! so a process started over gRPC is visible to a `List` over Connect and the
//! other way round.
//!
//! # What the protocol is
//!
//! A unary call is an ordinary POST whose body is the request message, in
//! protobuf-JSON (`application/json`) or binary protobuf
//! (`application/proto`). Success is `200` with the response message in the
//! same codec. An error is a non-200 status with a JSON body
//! `{"code": "...", "message": "..."}` -- and the status *and* the code both
//! have to be right, since the client reads the code.
//!
//! A streaming call uses `application/connect+json` (or `+proto`) and frames
//! every message in a 5-byte envelope: one flags byte, then a big-endian
//! `u32` length. The last frame of a response has flag `0x02` and carries
//! end-of-stream metadata -- `{}` for success, `{"error": {...}}` for
//! failure. A streaming call that fails *after* the first message cannot
//! change the HTTP status, which is exactly why the protocol puts the error
//! there instead.
//!
//! # What is not implemented
//!
//! Compression. `connect-accept-encoding` is ignored and nothing is ever
//! compressed, which is allowed -- identity is always acceptable -- and costs
//! bandwidth on large `ListDir` replies.
//!
//! GET for side-effect-free unary calls. The SDK does not use it, and
//! advertising it would mean deciding which of these RPCs is cacheable.

use std::sync::Arc;

use bytes::{BufMut, Bytes, BytesMut};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use tokio_stream::{Stream, StreamExt};
use tonic::Status;

use crate::envd_filesystem::filesystem_proto;
use crate::envd_filesystem::filesystem_proto::filesystem_server::Filesystem;
use crate::envd_filesystem::EnvdFilesystem;
use crate::envd_process::process_proto;
use crate::envd_process::process_proto::process_server::Process;
use crate::envd_process::EnvdProcess;

/// The body this module returns.
///
/// Boxed because a unary reply is one buffer and a streaming reply is a
/// channel, and both leave through the same function.
///
/// `UnsyncBoxBody` rather than `BoxBody`: axum's body -- which the gRPC arm
/// produces -- is `Send` but not `Sync`, and requiring `Sync` here would rule
/// out passing a tonic response through unchanged. Nothing needs to share a
/// body between threads; it is moved to the connection task and read there.
pub type ConnectBody = http_body_util::combinators::UnsyncBoxBody<Bytes, std::io::Error>;

/// How messages are encoded on this request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Codec {
    /// protobuf-JSON, per protobuf's own JSON mapping. What the SDK uses.
    Json,
    /// Binary protobuf.
    Proto,
}

/// What a request's content-type says it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Wire {
    codec: Codec,
    /// `application/connect+*`, meaning every message is enveloped. Set for
    /// streaming calls, and the client picks it, not the method: a client may
    /// use it for a unary call too.
    enveloped: bool,
}

impl Wire {
    /// Read the content-type, or `None` if this is not a Connect request.
    fn of(content_type: &str) -> Option<Self> {
        // Parameters like `; charset=utf-8` are allowed and carry nothing.
        let media = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match media.as_str() {
            "application/json" => Some(Wire {
                codec: Codec::Json,
                enveloped: false,
            }),
            "application/proto" | "application/protobuf" | "application/x-protobuf" => Some(Wire {
                codec: Codec::Proto,
                enveloped: false,
            }),
            "application/connect+json" => Some(Wire {
                codec: Codec::Json,
                enveloped: true,
            }),
            "application/connect+proto" | "application/connect+protobuf" => Some(Wire {
                codec: Codec::Proto,
                enveloped: true,
            }),
            _ => None,
        }
    }

    /// The content-type a reply to this request carries.
    fn content_type(self, streaming: bool) -> &'static str {
        match (self.codec, streaming || self.enveloped) {
            (Codec::Json, false) => "application/json",
            (Codec::Proto, false) => "application/proto",
            (Codec::Json, true) => "application/connect+json",
            (Codec::Proto, true) => "application/connect+proto",
        }
    }
}

/// Connect's name for a gRPC status code, and the HTTP status that carries it.
///
/// From the protocol's own table. Both matter: the client reads the code out
/// of the body, and anything sitting between client and server reads the
/// status.
fn code_and_status(code: tonic::Code) -> (&'static str, StatusCode) {
    use tonic::Code;
    match code {
        Code::Ok => ("ok", StatusCode::OK),
        Code::Cancelled => ("canceled", StatusCode::from_u16(499).expect("499")),
        Code::Unknown => ("unknown", StatusCode::INTERNAL_SERVER_ERROR),
        Code::InvalidArgument => ("invalid_argument", StatusCode::BAD_REQUEST),
        Code::DeadlineExceeded => ("deadline_exceeded", StatusCode::GATEWAY_TIMEOUT),
        Code::NotFound => ("not_found", StatusCode::NOT_FOUND),
        Code::AlreadyExists => ("already_exists", StatusCode::CONFLICT),
        Code::PermissionDenied => ("permission_denied", StatusCode::FORBIDDEN),
        Code::ResourceExhausted => ("resource_exhausted", StatusCode::TOO_MANY_REQUESTS),
        Code::FailedPrecondition => ("failed_precondition", StatusCode::PRECONDITION_FAILED),
        Code::Aborted => ("aborted", StatusCode::CONFLICT),
        Code::OutOfRange => ("out_of_range", StatusCode::BAD_REQUEST),
        Code::Unimplemented => ("unimplemented", StatusCode::NOT_IMPLEMENTED),
        Code::Internal => ("internal", StatusCode::INTERNAL_SERVER_ERROR),
        Code::Unavailable => ("unavailable", StatusCode::SERVICE_UNAVAILABLE),
        Code::DataLoss => ("data_loss", StatusCode::INTERNAL_SERVER_ERROR),
        Code::Unauthenticated => ("unauthenticated", StatusCode::UNAUTHORIZED),
    }
}

/// The JSON body of an error, as the protocol spells it.
fn error_json(status: &Status) -> Vec<u8> {
    let (code, _) = code_and_status(status.code());
    serde_json::to_vec(&serde_json::json!({
        "code": code,
        "message": status.message(),
    }))
    .unwrap_or_else(|_| br#"{"code":"internal","message":"error could not be encoded"}"#.to_vec())
}

fn full(bytes: Vec<u8>) -> ConnectBody {
    Full::new(Bytes::from(bytes))
        .map_err(|never| match never {})
        .boxed_unsync()
}

/// A unary error response.
fn fail(status: &Status) -> Response<ConnectBody> {
    let (_, http) = code_and_status(status.code());
    Response::builder()
        .status(http)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(full(error_json(status)))
        .expect("a static response builds")
}

/// One enveloped frame: flags, big-endian length, payload.
fn envelope(flags: u8, payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(5 + payload.len());
    buf.put_u8(flags);
    // `as` is safe here: a payload is one protobuf message, and nothing this
    // service produces approaches 4 GiB.
    buf.put_u32(payload.len() as u32);
    buf.put_slice(payload);
    buf.freeze()
}

/// The end-of-stream flag. Its payload is metadata, not a message.
const END_STREAM: u8 = 0x02;

/// Split whole envelopes off the front of a buffer, leaving any partial one.
fn take_envelopes(buffer: &mut BytesMut) -> Vec<(u8, Bytes)> {
    let mut out = Vec::new();
    loop {
        if buffer.len() < 5 {
            return out;
        }
        let flags = buffer[0];
        let length = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]) as usize;
        if buffer.len() < 5 + length {
            // The rest is still arriving. Leaving it whole is the point: a
            // half-read message decoded as if complete is a parse error
            // reported as the client's fault.
            return out;
        }
        let _ = buffer.split_to(5);
        out.push((flags, buffer.split_to(length).freeze()));
    }
}

/// Decode a request message.
fn decode<T>(wire: Wire, body: &[u8]) -> Result<T, Status>
where
    T: prost::Message + Default + serde::de::DeserializeOwned,
{
    // An empty body is a message with every field defaulted. The SDK sends
    // one for `List`, which takes no arguments, and rejecting it would break
    // the simplest call there is.
    if body.is_empty() {
        return Ok(T::default());
    }
    match wire.codec {
        Codec::Json => serde_json::from_slice(body)
            .map_err(|e| Status::invalid_argument(format!("could not decode JSON request: {e}"))),
        Codec::Proto => T::decode(body)
            .map_err(|e| Status::invalid_argument(format!("could not decode request: {e}"))),
    }
}

/// Encode a response message.
fn encode<T>(wire: Wire, message: &T) -> Result<Vec<u8>, Status>
where
    T: prost::Message + serde::Serialize,
{
    match wire.codec {
        Codec::Json => serde_json::to_vec(message)
            .map_err(|e| Status::internal(format!("could not encode JSON response: {e}"))),
        Codec::Proto => Ok(message.encode_to_vec()),
    }
}

/// Run one unary call: decode, dispatch, encode.
async fn unary<Req, Res, Fut>(
    wire: Wire,
    body: Bytes,
    call: impl FnOnce(tonic::Request<Req>) -> Fut,
) -> Response<ConnectBody>
where
    Req: prost::Message + Default + serde::de::DeserializeOwned,
    Res: prost::Message + serde::Serialize,
    Fut: std::future::Future<Output = Result<tonic::Response<Res>, Status>>,
{
    // A client may send a unary call enveloped. One message in, so one
    // envelope, and its payload is the message.
    let payload = if wire.enveloped {
        let mut buffer = BytesMut::from(&body[..]);
        match take_envelopes(&mut buffer).into_iter().next() {
            Some((_, payload)) => payload,
            None => return fail(&Status::invalid_argument("expected one enveloped message")),
        }
    } else {
        body
    };

    let request = match decode::<Req>(wire, &payload) {
        Ok(message) => message,
        Err(status) => return fail(&status),
    };

    let response = match call(tonic::Request::new(request)).await {
        Ok(response) => response.into_inner(),
        Err(status) => return fail(&status),
    };

    match encode(wire, &response) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, wire.content_type(false))
            .body(full(bytes))
            .expect("a checked response builds"),
        Err(status) => fail(&status),
    }
}

/// Run one server-streaming call.
///
/// The HTTP status is `200` whatever happens: the protocol cannot report a
/// later failure any other way, so a failure becomes the end-of-stream
/// envelope instead. Sending a non-200 here would be a client-visible lie for
/// any stream that produced a message first.
async fn server_stream<Req, Res, S, Fut>(
    wire: Wire,
    body: Bytes,
    call: impl FnOnce(tonic::Request<Req>) -> Fut,
) -> Response<ConnectBody>
where
    Req: prost::Message + Default + serde::de::DeserializeOwned,
    Res: prost::Message + serde::Serialize + Send + 'static,
    S: Stream<Item = Result<Res, Status>> + Send + Unpin + 'static,
    Fut: std::future::Future<Output = Result<tonic::Response<S>, Status>>,
{
    let payload = if wire.enveloped {
        let mut buffer = BytesMut::from(&body[..]);
        take_envelopes(&mut buffer)
            .into_iter()
            .next()
            .map_or_else(Bytes::new, |(_, payload)| payload)
    } else {
        body
    };

    let request = match decode::<Req>(wire, &payload) {
        Ok(message) => message,
        Err(status) => return fail(&status),
    };

    // A failure *before* the stream exists has not sent anything yet, so it
    // can still be an ordinary error response, which is a clearer thing for a
    // client to receive than a 200 with an error envelope.
    let mut stream = match call(tonic::Request::new(request)).await {
        Ok(response) => response.into_inner(),
        Err(status) => return fail(&status),
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(16);
    tokio::spawn(async move {
        let mut ending = Vec::from(&b"{}"[..]);
        while let Some(item) = stream.next().await {
            match item {
                Ok(message) => match encode(wire, &message) {
                    Ok(bytes) => {
                        if tx.send(envelope(0, &bytes)).await.is_err() {
                            return;
                        }
                    }
                    Err(status) => {
                        ending = end_with_error(&status);
                        break;
                    }
                },
                Err(status) => {
                    ending = end_with_error(&status);
                    break;
                }
            }
        }
        let _ = tx.send(envelope(END_STREAM, &ending)).await;
    });

    let body = StreamBody::new(
        tokio_stream::wrappers::ReceiverStream::new(rx).map(|chunk| Ok(Frame::data(chunk))),
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, wire.content_type(true))
        .body(body.boxed_unsync())
        .expect("a checked response builds")
}

/// The end-of-stream payload for a failed stream.
fn end_with_error(status: &Status) -> Vec<u8> {
    let (code, _) = code_and_status(status.code());
    serde_json::to_vec(&serde_json::json!({
        "error": { "code": code, "message": status.message() }
    }))
    .unwrap_or_else(|_| br#"{"error":{"code":"internal","message":"unencodable"}}"#.to_vec())
}

/// Is this request for the Connect protocol rather than gRPC?
///
/// gRPC and Connect share the same paths, so the content-type is what tells
/// them apart -- there is nothing in the URL to dispatch on.
#[must_use]
pub fn is_connect(headers: &hyper::HeaderMap) -> bool {
    headers
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| Wire::of(value).is_some())
}

/// Serve one Connect request against these services.
///
/// Returns `501` for a path neither service has, rather than `404`: the
/// endpoint exists, the method does not, and Connect carries that distinction
/// in the code a client reads.
pub async fn handle(
    process: EnvdProcess,
    filesystem: EnvdFilesystem,
    request: Request<Incoming>,
) -> Response<ConnectBody> {
    let content_type = request
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();

    let Some(wire) = Wire::of(&content_type) else {
        return fail(&Status::invalid_argument(format!(
            "content-type {content_type:?} is not one this endpoint speaks"
        )));
    };

    if request.method() != hyper::Method::POST {
        // GET is legal in Connect for calls marked side-effect-free; none
        // here are marked, so anything but POST is a mistake rather than a
        // capability worth advertising.
        return fail(&Status::unimplemented(
            "this endpoint accepts POST only; Connect GET is not implemented",
        ));
    }

    let path = request.uri().path().to_owned();

    // `StreamInput` reads its request as a stream, so it must not have the
    // body collected out from under it.
    if path == "/process.Process/StreamInput" {
        return stream_input(process, wire, request).await;
    }

    let body = match request.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            return fail(&Status::internal(format!(
                "could not read the request: {e}"
            )))
        }
    };

    dispatch(process, filesystem, &path, wire, body).await
}

/// Route one collected request to the method it names.
async fn dispatch(
    process: EnvdProcess,
    filesystem: EnvdFilesystem,
    path: &str,
    wire: Wire,
    body: Bytes,
) -> Response<ConnectBody> {
    use filesystem_proto as fsp;
    use process_proto as pp;

    match path {
        // ---------------------------------------------------------- process
        "/process.Process/List" => {
            unary::<pp::ListRequest, pp::ListResponse, _>(wire, body, |r| async move {
                Process::list(&process, r).await
            })
            .await
        }
        "/process.Process/Update" => {
            unary::<pp::UpdateRequest, pp::UpdateResponse, _>(wire, body, |r| async move {
                Process::update(&process, r).await
            })
            .await
        }
        "/process.Process/SendInput" => {
            unary::<pp::SendInputRequest, pp::SendInputResponse, _>(wire, body, |r| async move {
                Process::send_input(&process, r).await
            })
            .await
        }
        "/process.Process/SendSignal" => {
            unary::<pp::SendSignalRequest, pp::SendSignalResponse, _>(wire, body, |r| async move {
                Process::send_signal(&process, r).await
            })
            .await
        }
        "/process.Process/CloseStdin" => {
            unary::<pp::CloseStdinRequest, pp::CloseStdinResponse, _>(wire, body, |r| async move {
                Process::close_stdin(&process, r).await
            })
            .await
        }
        "/process.Process/Start" => {
            server_stream::<pp::StartRequest, pp::StartResponse, _, _>(wire, body, |r| async move {
                Process::start(&process, r).await
            })
            .await
        }
        "/process.Process/Connect" => {
            server_stream::<pp::ConnectRequest, pp::ConnectResponse, _, _>(
                wire,
                body,
                |r| async move { Process::connect(&process, r).await },
            )
            .await
        }

        // ------------------------------------------------------- filesystem
        "/filesystem.Filesystem/Stat" => {
            unary::<fsp::StatRequest, fsp::StatResponse, _>(wire, body, |r| async move {
                Filesystem::stat(&filesystem, r).await
            })
            .await
        }
        "/filesystem.Filesystem/MakeDir" => {
            unary::<fsp::MakeDirRequest, fsp::MakeDirResponse, _>(wire, body, |r| async move {
                Filesystem::make_dir(&filesystem, r).await
            })
            .await
        }
        "/filesystem.Filesystem/Move" => {
            unary::<fsp::MoveRequest, fsp::MoveResponse, _>(wire, body, |r| async move {
                Filesystem::r#move(&filesystem, r).await
            })
            .await
        }
        "/filesystem.Filesystem/Remove" => {
            unary::<fsp::RemoveRequest, fsp::RemoveResponse, _>(wire, body, |r| async move {
                Filesystem::remove(&filesystem, r).await
            })
            .await
        }
        "/filesystem.Filesystem/ListDir" => {
            unary::<fsp::ListDirRequest, fsp::ListDirResponse, _>(wire, body, |r| async move {
                Filesystem::list_dir(&filesystem, r).await
            })
            .await
        }
        "/filesystem.Filesystem/CreateWatcher" => {
            unary::<fsp::CreateWatcherRequest, fsp::CreateWatcherResponse, _>(
                wire,
                body,
                |r| async move { Filesystem::create_watcher(&filesystem, r).await },
            )
            .await
        }
        "/filesystem.Filesystem/GetWatcherEvents" => {
            unary::<fsp::GetWatcherEventsRequest, fsp::GetWatcherEventsResponse, _>(
                wire,
                body,
                |r| async move { Filesystem::get_watcher_events(&filesystem, r).await },
            )
            .await
        }
        "/filesystem.Filesystem/RemoveWatcher" => {
            unary::<fsp::RemoveWatcherRequest, fsp::RemoveWatcherResponse, _>(
                wire,
                body,
                |r| async move { Filesystem::remove_watcher(&filesystem, r).await },
            )
            .await
        }
        "/filesystem.Filesystem/WatchDir" => {
            server_stream::<fsp::WatchDirRequest, fsp::WatchDirResponse, _, _>(
                wire,
                body,
                |r| async move { Filesystem::watch_dir(&filesystem, r).await },
            )
            .await
        }

        _ => fail(&Status::unimplemented(format!(
            "no method {path} on this endpoint"
        ))),
    }
}

/// `StreamInput`, whose request is the stream.
///
/// Read as it arrives rather than collected: the whole point of the RPC is
/// ordered writes to a running program's stdin, and collecting the body would
/// hold every keystroke until the client closed the stream -- which an
/// interactive client never does.
async fn stream_input(
    process: EnvdProcess,
    wire: Wire,
    request: Request<Incoming>,
) -> Response<ConnectBody> {
    use process_proto::stream_input_request::Event;
    use process_proto::{StreamInputRequest, StreamInputResponse};

    if !wire.enveloped {
        return fail(&Status::invalid_argument(
            "a client-streaming call must use application/connect+json or +proto",
        ));
    }

    let mut body = request.into_body();
    let mut buffer = BytesMut::new();
    let mut pid: Option<u32> = None;

    loop {
        let frame = match body.frame().await {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                return fail(&Status::internal(format!("reading the request: {e}")));
            }
            None => break,
        };
        let Ok(data) = frame.into_data() else {
            // Trailers. Nothing here reads them.
            continue;
        };
        buffer.extend_from_slice(&data);

        for (flags, payload) in take_envelopes(&mut buffer) {
            if flags & END_STREAM != 0 {
                break;
            }
            let message: StreamInputRequest = match decode(wire, &payload) {
                Ok(message) => message,
                Err(status) => return fail(&status),
            };
            match message.event {
                Some(Event::Start(start)) => match process.resolve(start.process) {
                    Ok(resolved) => pid = Some(resolved),
                    Err(status) => return fail(&status),
                },
                Some(Event::Data(data)) => {
                    let Some(pid) = pid else {
                        return fail(&Status::failed_precondition(
                            "the first message on this stream must name the process",
                        ));
                    };
                    if let Err(status) = process.write_input(pid, data.input).await {
                        return fail(&status);
                    }
                }
                Some(Event::Keepalive(_)) | None => {}
            }
        }
    }

    let response = StreamInputResponse {};
    match encode(wire, &response) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, wire.content_type(true))
            .body(full(
                [envelope(0, &bytes), envelope(END_STREAM, b"{}")].concat(),
            ))
            .expect("a checked response builds"),
        Err(status) => fail(&status),
    }
}

/// One request's worth of work in [`shared_service`].
///
/// Boxed because the two arms are different futures -- tonic's router and
/// this module's handler -- and a `tower::Service` has exactly one future
/// type.
type SharedFuture = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<Response<ConnectBody>, std::convert::Infallible>>
            + Send,
    >,
>;

/// Serve both services, on both protocols, on one connection.
///
/// gRPC and Connect use the same paths, so this dispatches on content-type:
/// `application/grpc*` goes to tonic, everything Connect-shaped comes here,
/// and anything else is refused in Connect's terms because that is the only
/// error shape either client understands.
pub fn shared_service(
    process: EnvdProcess,
    filesystem: EnvdFilesystem,
) -> impl tower::Service<
    Request<Incoming>,
    Response = Response<ConnectBody>,
    Error = std::convert::Infallible,
    Future = SharedFuture,
> + Clone {
    use crate::envd_filesystem::FilesystemServer;
    use crate::envd_process::ProcessServer;

    let grpc = tonic::service::Routes::new(ProcessServer::new(process.clone()))
        .add_service(FilesystemServer::new(filesystem.clone()))
        .prepare()
        .into_axum_router();

    tower::service_fn(move |request: Request<Incoming>| {
        let process = process.clone();
        let filesystem = filesystem.clone();
        let grpc = grpc.clone();
        Box::pin(async move {
            let is_grpc = request
                .headers()
                .get(hyper::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("application/grpc"));

            if is_grpc {
                use tower::ServiceExt;
                let response = grpc
                    .oneshot(request.map(axum::body::Body::new))
                    .await
                    .expect("axum router is infallible");
                Ok(response.map(|body| body.map_err(std::io::Error::other).boxed_unsync()))
            } else {
                Ok(handle(process, filesystem, request).await)
            }
        }) as SharedFuture
    })
}

/// Serve `process.Process` and `filesystem.Filesystem` for `vm` on `addr`,
/// speaking gRPC and Connect, until `shutdown` fires.
///
/// One listener for both, because a sandbox has one port and a client picks
/// its own protocol.
///
/// # Errors
///
/// Fails if `addr` cannot be bound. A failure on one connection is logged and
/// dropped: one client's mistake must not close the endpoint for the others.
pub async fn serve(
    vm: Arc<hv2_agent::AgentVM>,
    addr: std::net::SocketAddr,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_on(
        listener,
        EnvdProcess::new(Arc::clone(&vm)),
        EnvdFilesystem::new(vm),
        shutdown,
    )
    .await
}

/// As [`serve`], on a listener the caller already holds.
///
/// Split out so a test can bind port 0 and still know where to connect: the
/// kernel picks the port, and only the listener knows which.
///
/// # Errors
///
/// Fails only if accepting stops working. A failure on one connection is
/// logged and dropped.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    process: EnvdProcess,
    filesystem: EnvdFilesystem,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    let service = shared_service(process, filesystem);
    let mut shutdown = shutdown;

    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => accepted?,
            _ = &mut shutdown => return Ok(()),
        };

        let service = service.clone();
        tokio::spawn(async move {
            // HTTP/1.1 and HTTP/2 both: gRPC needs h2, and the Connect client
            // uses h1.
            let served =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(
                        hyper_util::rt::TokioIo::new(stream),
                        hyper::service::service_fn(move |request| {
                            let mut service = service.clone();
                            async move {
                                use tower::Service;
                                std::future::poll_fn(|cx| service.poll_ready(cx)).await?;
                                service.call(request).await
                            }
                        }),
                    )
                    .await;
            if let Err(e) = served {
                tracing::debug!("envd endpoint: connection from {peer} ended: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sdks_content_types_are_recognised() {
        // Exactly what the real client sends: JSON for unary, connect+json
        // for streaming.
        assert_eq!(
            Wire::of("application/json"),
            Some(Wire {
                codec: Codec::Json,
                enveloped: false
            })
        );
        assert_eq!(
            Wire::of("application/connect+json"),
            Some(Wire {
                codec: Codec::Json,
                enveloped: true
            })
        );
        // Parameters are allowed and mean nothing to the dispatch.
        assert_eq!(
            Wire::of("application/json; charset=utf-8"),
            Some(Wire {
                codec: Codec::Json,
                enveloped: false
            })
        );
    }

    #[test]
    fn grpc_is_not_mistaken_for_connect() {
        // The two share every path, so this is the only thing keeping them
        // apart. If it ever returned Some, gRPC clients would be answered in
        // Connect's error shape and could not read it.
        assert_eq!(Wire::of("application/grpc"), None);
        assert_eq!(Wire::of("application/grpc+proto"), None);
        assert!(!is_connect(&hyper::HeaderMap::new()));
    }

    #[test]
    fn an_envelope_round_trips() {
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(&envelope(0, b"first"));
        buffer.extend_from_slice(&envelope(END_STREAM, b"{}"));
        let taken = take_envelopes(&mut buffer);
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0], (0, Bytes::from_static(b"first")));
        assert_eq!(taken[1], (END_STREAM, Bytes::from_static(b"{}")));
        assert!(buffer.is_empty());
    }

    #[test]
    fn a_partial_envelope_waits_for_the_rest() {
        // A message split across two reads is the normal case on a stream,
        // not an error. Decoding half of one would report the client's
        // perfectly good request as malformed.
        let whole = envelope(0, b"a longer message");
        for cut in 0..whole.len() {
            let mut buffer = BytesMut::from(&whole[..cut]);
            assert!(
                take_envelopes(&mut buffer).is_empty(),
                "{cut} of {} bytes should not yield a message",
                whole.len()
            );
            assert_eq!(buffer.len(), cut, "the partial frame must be kept");
        }
    }

    #[test]
    fn every_code_has_a_status_a_client_can_read() {
        // The client reads the code from the body, but proxies and browsers
        // read the status. Both come from one table so they cannot disagree.
        use tonic::Code;
        assert_eq!(
            code_and_status(Code::NotFound),
            ("not_found", StatusCode::NOT_FOUND)
        );
        assert_eq!(
            code_and_status(Code::Unimplemented),
            ("unimplemented", StatusCode::NOT_IMPLEMENTED)
        );
        assert_eq!(
            code_and_status(Code::InvalidArgument),
            ("invalid_argument", StatusCode::BAD_REQUEST)
        );
        // Not 200: an OK never travels this path, but mapping it to anything
        // else would be a lie if it ever did.
        assert_eq!(code_and_status(Code::Ok).1, StatusCode::OK);
    }

    #[test]
    fn an_error_body_is_what_the_protocol_says() {
        let body = error_json(&Status::not_found("no such path: /nope"));
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert_eq!(parsed["code"], "not_found");
        assert_eq!(parsed["message"], "no such path: /nope");
    }

    #[test]
    fn a_stream_reports_its_failure_in_the_last_envelope() {
        // Not in the HTTP status: by the time a stream fails, 200 has already
        // been sent. This is the only place the client can learn of it.
        let body = end_with_error(&Status::aborted("the watcher stopped"));
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert_eq!(parsed["error"]["code"], "aborted");
        assert_eq!(parsed["error"]["message"], "the watcher stopped");
    }

    #[test]
    fn the_json_is_protobuf_json_not_whatever_serde_derives() {
        // The mapping protobuf specifies, which is not what a plain derive
        // produces: lowerCamelCase names, 64-bit integers as strings, enums
        // by name. A client reading `size` would otherwise get nothing.
        let entry = filesystem_proto::EntryInfo {
            name: "a.txt".to_string(),
            r#type: filesystem_proto::FileType::File as i32,
            path: "/w/a.txt".to_string(),
            size: 4,
            mode: 0o644,
            permissions: "644".to_string(),
            owner: "root".to_string(),
            group: "root".to_string(),
            modified_time: Some(pbjson_types::Timestamp {
                seconds: 100,
                nanos: 0,
            }),
            symlink_target: None,
            metadata: Default::default(),
        };
        let json: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&entry).expect("encode")).expect("parse");

        assert_eq!(json["type"], "FILE_TYPE_FILE", "enums travel by name");
        assert_eq!(json["size"], "4", "64-bit integers travel as strings");
        assert!(
            json.get("modifiedTime").is_some(),
            "field names are lowerCamelCase, not snake_case: {json}"
        );
        // RFC 3339, but `+00:00` where protobuf-JSON's spec says `Z`. That
        // is `pbjson-types`' choice, not ours, and both spell the same
        // instant; a strict protobuf-JSON parser could still object. The real
        // SDK does not -- checked against it -- so this records the deviation
        // rather than pretending it is not there.
        assert_eq!(json["modifiedTime"], "1970-01-01T00:01:40+00:00");
    }

    #[test]
    fn a_request_decodes_from_what_the_sdk_actually_sent() {
        // Copied from the wire capture in this module's doc comment.
        let wire = Wire {
            codec: Codec::Json,
            enveloped: false,
        };
        let request: filesystem_proto::ListDirRequest =
            decode(wire, br#"{"path": "/", "depth": 1}"#).expect("decode");
        assert_eq!(request.path, "/");
        assert_eq!(request.depth, 1);
    }

    #[test]
    fn an_empty_body_is_a_defaulted_message() {
        // `List` takes no arguments and the SDK sends no body for it.
        let wire = Wire {
            codec: Codec::Json,
            enveloped: false,
        };
        assert!(decode::<process_proto::ListRequest>(wire, b"").is_ok());
    }
}
