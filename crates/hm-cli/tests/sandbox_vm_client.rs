//! Exercise the shipped binary against a protocol fixture, not a real guest.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use axum::{
    extract::{Json, State},
    routing::{delete, post},
    Router,
};
use serde_json::{json, Value};

async fn server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (endpoint, task)
}

#[tokio::test]
async fn binary_preserves_guest_arguments_streams_and_failure() {
    let app = Router::new().route("/sandboxes/test-vm/exec", post(|Json(body): Json<Value>| async move {
                    assert_eq!(body["cmd"], "exec '/bin/sh' '-c' 'printf hello; exit 7'");
                    assert!(body["args"].is_null());
        Json(json!({"stdout": "hello", "stderr": "guest diagnostic", "exit_code": 7, "timed_out": false}))
    }));
    let (endpoint, server) = server(app).await;
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args([
            "sandbox",
            "vm",
            "--endpoint",
            &endpoint,
            "exec",
            "test-vm",
            "--",
            "/bin/sh",
            "-c",
            "printf hello; exit 7",
        ])
        .env_remove("HV2_API_KEY")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"hello");
    assert_eq!(output.stderr, b"guest diagnostic");
    server.abort();
}

#[tokio::test]
async fn benchmark_does_not_report_wrong_output_as_a_fast_success() {
    let deleted = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route("/v2/sandboxes", post(|| async { Json(json!({"sandboxID": "test-vm"})) }))
        .route("/sandboxes/test-vm/exec", post(|| async { Json(json!({"stdout": "not-ready", "stderr": "", "exit_code": 0, "timed_out": false})) }))
        .route("/sandboxes/test-vm", delete(|State(deleted): State<Arc<AtomicBool>>| async move {
            deleted.store(true, Ordering::SeqCst);
            axum::http::StatusCode::NO_CONTENT
        })).with_state(deleted.clone());
    let (endpoint, server) = server(app).await;
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args([
            "sandbox",
            "vm",
            "--endpoint",
            &endpoint,
            "benchmark",
            "--samples",
            "1",
            "--environment",
            "protocol fixture, not a performance result",
        ])
        .env_remove("HV2_API_KEY")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["successful_samples"], 0);
    assert_eq!(report["failed_samples"], 1);
    assert!(report["ready_ms"].is_null());
    assert!(deleted.load(Ordering::SeqCst));
    server.abort();
}
