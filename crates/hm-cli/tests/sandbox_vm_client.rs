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
    assert!(report["template_metadata"].is_null());
    assert!(report["template_metadata_error"]
        .as_str()
        .unwrap()
        .contains("404"));
    assert!(deleted.load(Ordering::SeqCst));
    server.abort();
}

#[tokio::test]
async fn benchmark_snapshot_gate_rejects_cold_boot_before_creating_a_guest() {
    let created = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route(
            "/templates",
            axum::routing::get(|| async {
                Json(json!([{"templateID": "base", "snapshot": false, "buildStatus": "ready"}]))
            }),
        )
        .route(
            "/v2/sandboxes",
            post(|State(created): State<Arc<AtomicBool>>| async move {
                created.store(true, Ordering::SeqCst);
                Json(json!({"sandboxID": "unexpected"}))
            }),
        )
        .with_state(created.clone());
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
            "protocol fixture",
            "--require-snapshot",
        ])
        .env_remove("HV2_API_KEY")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--require-snapshot"));
    assert!(!created.load(Ordering::SeqCst));
    server.abort();
}

#[tokio::test]
async fn benchmark_bounds_live_sandboxes_and_fails_a_missed_latency_gate() {
    use std::sync::atomic::AtomicUsize;
    #[derive(Default)]
    struct Counts {
        created: AtomicUsize,
        live: AtomicUsize,
        peak: AtomicUsize,
    }
    let counts = Arc::new(Counts::default());
    let app = Router::new()
        .route("/templates", axum::routing::get(|| async {
            Json(json!([{"templateID": "base", "snapshot": true, "cpuCount": 1, "memoryMB": 1024}]))
        }))
        .route(
            "/v2/sandboxes",
            post(|State(counts): State<Arc<Counts>>| async move {
                let id = counts.created.fetch_add(1, Ordering::SeqCst);
                let live = counts.live.fetch_add(1, Ordering::SeqCst) + 1;
                counts.peak.fetch_max(live, Ordering::SeqCst);
                Json(json!({"sandboxID": format!("fixture-{id}")}))
            }),
        )
        .route(
            "/sandboxes/{id}/exec",
            post(|Json(body): Json<Value>| async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let marker = body["cmd"]
                    .as_str()
                    .unwrap()
                    .split('\'')
                    .rev()
                    .nth(1)
                    .unwrap();
                Json(json!({"stdout": marker, "stderr": "", "exit_code": 0, "timed_out": false}))
            }),
        )
        .route(
            "/sandboxes/{id}",
            delete(|State(counts): State<Arc<Counts>>| async move {
                counts.live.fetch_sub(1, Ordering::SeqCst);
                axum::http::StatusCode::NO_CONTENT
            }),
        )
        .with_state(counts.clone());
    let (endpoint, server) = server(app).await;
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args([
            "sandbox",
            "vm",
            "--endpoint",
            &endpoint,
            "benchmark",
            "--samples",
            "4",
            "--concurrency",
            "2",
            "--max-p99-ready-ms",
            "1",
            "--require-snapshot",
            "--environment",
            "delayed protocol fixture, not a performance result",
        ])
        .env_remove("HV2_API_KEY")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["successful_samples"], 4);
    assert_eq!(report["failed_samples"], 0);
    assert_eq!(report["threshold_passed"], false);
    assert_eq!(report["template_metadata"]["snapshot"], true);
    assert_eq!(report["template_metadata"]["memoryMB"], 1024);
    assert!(report["template_metadata_error"].is_null());
    assert_eq!(report["samples"].as_array().unwrap().len(), 4);
    assert_eq!(counts.created.load(Ordering::SeqCst), 4);
    assert_eq!(counts.live.load(Ordering::SeqCst), 0);
    assert!(counts.peak.load(Ordering::SeqCst) <= 2);
    server.abort();
}
