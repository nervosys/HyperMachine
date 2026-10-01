use super::*;
use axum::{routing::get, Json, Router};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::{
    io::{DuplexStream, ReadHalf, WriteHalf},
    sync::Notify,
    task::JoinHandle,
    time::{timeout, Duration},
};

struct Peer {
    input: BufReader<ReadHalf<DuplexStream>>,
    output: WriteHalf<DuplexStream>,
    partial: Vec<u8>,
}
impl Peer {
    async fn send(&mut self, value: Value) {
        self.output
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }
    async fn receive(&mut self) -> Value {
        let frame = timeout(
            Duration::from_secs(2),
            read_frame(&mut self.input, &mut self.partial),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        serde_json::from_slice(&frame).unwrap()
    }
}

struct Fixture {
    peer: Peer,
    transport: JoinHandle<Result<()>>,
    server: JoinHandle<()>,
    started: Arc<Notify>,
    release: Arc<Notify>,
    calls: Arc<AtomicUsize>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.transport.abort();
        self.server.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (start, wait, count) = (started.clone(), release.clone(), calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Api::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            5,
            None,
        )
        .unwrap();
        let router = Router::new().route(
            "/sandboxes",
            get(move || {
                let (start, wait, count) = (start.clone(), wait.clone(), count.clone());
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    start.notify_one();
                    wait.notified().await;
                    Json(json!([]))
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let (client, remote) = tokio::io::duplex(2 * 1024 * 1024);
        let (input, output) = tokio::io::split(client);
        let transport = tokio::spawn(async move {
            let (input, mut output) = tokio::io::split(remote);
            super::transport(&api, 5, None, &mut BufReader::new(input), &mut output).await
        });
        let mut fixture = Self {
            peer: Peer {
                input: BufReader::new(input),
                output,
                partial: Vec::new(),
            },
            transport,
            server,
            started,
            release,
            calls,
        };
        fixture.peer.send(json!({"jsonrpc":"2.0","id":-1,"method":"initialize","params":{
            "protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).await;
        assert!(fixture.peer.receive().await.get("result").is_some());
        fixture
            .peer
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        fixture
    }
    async fn blocked(&mut self, id: Value) {
        self.peer.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"sandbox_list"}})).await;
        timeout(Duration::from_secs(2), self.started.notified())
            .await
            .unwrap();
    }
    async fn cancel(&mut self, id: Value) {
        self.peer.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}})).await;
    }
    async fn ping(&mut self, id: Value) {
        self.peer
            .send(json!({"jsonrpc":"2.0","id":id,"method":"ping"}))
            .await;
    }
}

#[tokio::test]
async fn cancellation_matches_id_type_and_keeps_the_session_usable() {
    let mut fixture = Fixture::new().await;
    fixture.blocked(json!("42")).await;
    fixture.cancel(json!(42)).await;
    fixture
        .peer
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled",
        "params":{"requestId":"42","reason":123}}))
        .await;
    fixture.ping(json!(9)).await;
    assert!(timeout(
        Duration::from_millis(30),
        read_frame(&mut fixture.peer.input, &mut fixture.peer.partial)
    )
    .await
    .is_err());
    fixture.cancel(json!("42")).await;
    assert_eq!(fixture.peer.receive().await["id"], 9);
    fixture.cancel(json!("42")).await; // Completed/unknown IDs do not cancel another request.
    fixture.ping(json!(10)).await;
    assert_eq!(fixture.peer.receive().await["id"], 10);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn queued_cancellation_prevents_a_second_api_call() {
    let mut fixture = Fixture::new().await;
    fixture.blocked(json!(7)).await;
    fixture
        .peer
        .send(
            json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"sandbox_list"}}),
        )
        .await;
    fixture.cancel(json!(8)).await;
    fixture.ping(json!(9)).await;
    fixture.cancel(json!(7)).await;
    assert_eq!(fixture.peer.receive().await["id"], 9);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn queued_requests_survive_completion_and_late_cancellation() {
    let mut fixture = Fixture::new().await;
    fixture.blocked(json!(7)).await;
    fixture.ping(json!(8)).await;
    fixture.release.notify_one();
    assert_eq!(fixture.peer.receive().await["id"], 7);
    assert_eq!(fixture.peer.receive().await["id"], 8);
    fixture.cancel(json!(7)).await;
    fixture.ping(json!(9)).await;
    assert_eq!(fixture.peer.receive().await["id"], 9);
}

#[tokio::test]
async fn partial_frames_survive_a_cancelled_read_and_keep_the_cumulative_limit() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    let mut reader = BufReader::new(reader);
    let mut partial = Vec::new();
    writer.write_all(b"{\"jsonrpc\":").await.unwrap();
    assert!(timeout(
        Duration::from_millis(30),
        read_frame(&mut reader, &mut partial)
    )
    .await
    .is_err());
    assert_eq!(partial, b"{\"jsonrpc\":");
    writer.write_all(b"\"2.0\"}\n").await.unwrap();
    assert_eq!(
        read_frame(&mut reader, &mut partial)
            .await
            .unwrap()
            .unwrap(),
        b"{\"jsonrpc\":\"2.0\"}\n"
    );
    partial = vec![b'x'; MAX_MESSAGE as usize - 4];
    writer.write_all(b"123456789\n").await.unwrap();
    assert!(read_frame(&mut reader, &mut partial)
        .await
        .unwrap_err()
        .to_string()
        .contains("1 MiB"));
}

#[tokio::test]
async fn read_ahead_is_bounded_by_count_and_bytes() {
    for oversized in [false, true] {
        let mut fixture = Fixture::new().await;
        fixture.blocked(json!(7)).await;
        if oversized {
            for id in [8, 9] {
                fixture.peer.send(json!({"jsonrpc":"2.0","id":id,"method":"ping","params":{"padding":"x".repeat(600000)}})).await;
            }
        } else {
            for id in 0..=MAX_QUEUED_MESSAGES {
                fixture.ping(json!(id + 10)).await;
            }
        }
        let error = timeout(Duration::from_secs(2), &mut fixture.transport)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("queue limit"));
    }
}

#[tokio::test]
async fn input_eof_drops_the_active_client_wait() {
    let mut fixture = Fixture::new().await;
    fixture.blocked(json!(7)).await;
    fixture.peer.output.shutdown().await.unwrap();
    timeout(Duration::from_secs(2), &mut fixture.transport)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[test]
fn initialize_and_malformed_cancellations_are_not_cancellable() {
    assert!(request_id(br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).is_none());
    for value in [
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":null}}),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1.5}}),
        json!({"jsonrpc":"2.0","id":1,"method":"notifications/cancelled","params":{"requestId":2}}),
        json!({"jsonrpc":"1.0","method":"notifications/cancelled","params":{"requestId":2}}),
    ] {
        assert!(cancellation_id(&serde_json::to_vec(&value).unwrap()).is_none());
    }
}
