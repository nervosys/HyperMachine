//! Exercise the shipped binary against a protocol fixture, not a real guest.

#[tokio::test]
async fn tcp_stdio_is_binary_clean_and_exits_when_guest_closes_with_stdin_open() {
    use axum::{extract::Request, http::StatusCode, response::IntoResponse};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let app = Router::new().route(
        "/sandboxes/{id}/ports/22/tcp",
        axum::routing::get(|mut request: Request| async move {
            if request
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                != Some("stdio-fixture-key")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            assert_eq!(request.headers()["upgrade"], "hv2-tcp/1");
            let close = request.uri().path().contains("/close/");
            let upgraded = hyper::upgrade::on(&mut request);
            tokio::spawn(async move {
                let mut stream = hyper_util::rt::TokioIo::new(upgraded.await.unwrap());
                if close {
                    stream.write_all(b"SSH-2.0-fixture\r\n").await.unwrap();
                } else {
                    let mut bytes = Vec::new();
                    stream.read_to_end(&mut bytes).await.unwrap();
                    stream.write_all(&bytes).await.unwrap();
                }
                stream.shutdown().await.unwrap();
            });
            (
                StatusCode::SWITCHING_PROTOCOLS,
                [("connection", "upgrade"), ("upgrade", "hv2-tcp/1")],
            )
                .into_response()
        }),
    );
    let (endpoint, server) = server(app).await;
    let bytes: Vec<u8> = (0..65536).map(|n| (n % 251) as u8).collect();
    for id in ["echo", "close"] {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args(["sandbox", "vm", "--endpoint", &endpoint, "tcp-stdio", id])
            .env("HV2_API_KEY", "stdio-fixture-key")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take();
        if id == "echo" {
            stdin.as_mut().unwrap().write_all(&bytes).await.unwrap();
            drop(stdin.take());
        }
        // Keep the parent's stdin pipe open for the guest-close case.
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
                .await
                .unwrap()
                .unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            output.stdout,
            if id == "echo" {
                bytes.as_slice()
            } else {
                b"SSH-2.0-fixture\r\n"
            }
        );
        drop(stdin);
    }
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args([
            "sandbox",
            "vm",
            "--endpoint",
            &endpoint,
            "tcp-stdio",
            "echo",
        ])
        .env("HV2_API_KEY", "wrong")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    server.abort();
}

#[tokio::test]
async fn tcp_command_authenticates_and_preserves_binary_replies_after_client_eof() {
    use axum::{extract::Request, http::StatusCode, response::IntoResponse};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let app = Router::new().route(
        "/sandboxes/test-vm/ports/41000/tcp",
        axum::routing::get(|mut request: Request| async move {
            if request
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                != Some("tcp-cli-fixture-key")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            assert_eq!(request.headers()["upgrade"], "hv2-tcp/1");
            assert_eq!(request.version(), axum::http::Version::HTTP_11);
            let upgraded = hyper::upgrade::on(&mut request);
            tokio::spawn(async move {
                let mut stream = hyper_util::rt::TokioIo::new(upgraded.await.unwrap());
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).await.unwrap();
                stream.write_all(&bytes).await.unwrap();
                stream.shutdown().await.unwrap();
            });
            (
                StatusCode::SWITCHING_PROTOCOLS,
                [("connection", "upgrade"), ("upgrade", "hv2-tcp/1")],
            )
                .into_response()
        }),
    );
    let (endpoint, server) = server(app).await;
    let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args([
            "sandbox",
            "vm",
            "--endpoint",
            &endpoint,
            "--request-timeout",
            "1",
            "tcp",
            "test-vm",
            "--port",
            "41000",
        ])
        .env("HV2_API_KEY", "tcp-cli-fixture-key")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = tokio::io::BufReader::new(process.stdout.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        output.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    let bound: Value = serde_json::from_str(&line).unwrap();
    let address = bound["listen"].as_str().unwrap();
    assert!(address.starts_with("127.0.0.1:"));
    let bytes: Vec<u8> = (0..65536).map(|n| (n % 251) as u8).collect();
    for iteration in 0..2 {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            if iteration == 0 {
                // The handshake deadline must not become the tunnel lifetime.
                tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            }
            client.write_all(&bytes).await.unwrap();
            client.shutdown().await.unwrap();
            let mut reply = Vec::new();
            client.read_to_end(&mut reply).await.unwrap();
            assert_eq!(reply, bytes);
        })
        .await
        .unwrap();
    }
    process.kill().await.unwrap();
    let _ = process.wait().await;
    for (extra, key, code) in [
        (vec!["--port", "0"], "tcp-cli-fixture-key", 2),
        (
            vec!["--port", "41000", "--listen", "0.0.0.0:0"],
            "tcp-cli-fixture-key",
            1,
        ),
        (vec!["--port", "41000"], "wrong", 1),
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args(["sandbox", "vm", "--endpoint", &endpoint, "tcp", "test-vm"])
            .args(extra)
            .env("HV2_API_KEY", key)
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert!(output.stdout.is_empty());
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn domain_commands_send_authenticated_claim_list_and_release_requests() {
    use axum::extract::Path;
    use axum::http::{HeaderMap, StatusCode};
    let app = Router::new()
        .route(
            "/sandboxes/{id}/domains",
            axum::routing::get(|Path(id): Path<String>, headers: HeaderMap| async move {
                assert_eq!(headers["x-api-key"], "domain-fixture-key");
                assert_eq!(id, "test-vm");
                Json(json!([{"domain":"app.example.com","sandbox_id":id,"port":8080}]))
            }),
        )
        .route(
            "/sandboxes/{id}/domains/{domain}",
            axum::routing::put(
                |Path((id, domain)): Path<(String, String)>,
                 headers: HeaderMap,
                 Json(body): Json<Value>| async move {
                    assert_eq!(headers["x-api-key"], "domain-fixture-key");
                    assert_eq!(id, "test-vm");
                    assert_eq!(domain, "app.example.com");
                    assert_eq!(body, json!({"port":8080}));
                    Json(json!({"domain":domain,"sandbox_id":id,"port":8080}))
                },
            )
            .delete(
                |Path((id, domain)): Path<(String, String)>, headers: HeaderMap| async move {
                    assert_eq!(headers["x-api-key"], "domain-fixture-key");
                    assert_eq!(
                        (id.as_str(), domain.as_str()),
                        ("test-vm", "app.example.com")
                    );
                    StatusCode::NO_CONTENT
                },
            ),
        );
    let (endpoint, task) = server(app).await;
    for arguments in [
        vec!["bind", "test-vm", "app.example.com", "--port", "8080"],
        vec!["list", "test-vm"],
        vec!["unbind", "test-vm", "app.example.com"],
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args(["sandbox", "vm", "--endpoint", &endpoint, "domain"])
            .args(&arguments)
            .env("HV2_API_KEY", "domain-fixture-key")
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if arguments[0] != "unbind" {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(value.is_object() || value.is_array());
        }
    }
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args([
            "sandbox",
            "vm",
            "domain",
            "bind",
            "test-vm",
            "app.example.com",
            "--port",
            "0",
        ])
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn failed_downloads_and_destination_races_do_not_publish_or_overwrite_files() {
    use axum::{extract::Query, response::IntoResponse};
    use std::collections::HashMap;
    let directory = tempfile::tempdir().unwrap();
    let destination = Arc::new(directory.path().join("destination.bin"));
    let api = Router::new().route(
        "/sandboxes/test-vm/connect",
        post(|| async { Json(json!({"envdAccessToken": "sandbox-fixture-token"})) }),
    );
    let envd = Router::new()
        .route(
            "/files",
            axum::routing::get(
                |State(destination): State<Arc<std::path::PathBuf>>,
                 Query(query): Query<HashMap<String, String>>| async move {
                    if query["path"] == "/failed" {
                        return (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            "transfer unavailable",
                        )
                            .into_response();
                    }
                    std::fs::write(destination.as_ref(), b"created concurrently").unwrap();
                    axum::body::Bytes::from_static(b"download bytes").into_response()
                },
            ),
        )
        .with_state(destination.clone());
    let (api_endpoint, api_task) = server(api).await;
    let (envd_endpoint, envd_task) = server(envd).await;
    for path in ["/failed", "/race"] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args([
                "sandbox",
                "vm",
                "--endpoint",
                &api_endpoint,
                "files",
                "test-vm",
                "--envd-endpoint",
                &envd_endpoint,
                "download",
                path,
                destination.to_str().unwrap(),
            ])
            .env_remove("HV2_API_KEY")
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        if path == "/failed" {
            assert!(!destination.exists());
            assert!(String::from_utf8_lossy(&output.stderr).contains("503"));
        } else {
            assert_eq!(
                std::fs::read(destination.as_ref()).unwrap(),
                b"created concurrently"
            );
        }
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    api_task.abort();
    envd_task.abort();
}

#[tokio::test]
async fn file_transfers_preserve_binary_bytes_and_separate_credentials() {
    use axum::{extract::Query, http::HeaderMap};
    use std::collections::HashMap;
    let bytes = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let path = "/root/a'&query=literal.bin";
    let api = Router::new().route(
        "/sandboxes/test-vm/connect",
        post(|headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers["x-api-key"], "platform-fixture-secret");
            assert_eq!(body["timeout"], 300);
            Json(json!({"envdAccessToken": "sandbox-fixture-token"}))
        }),
    );
    let envd = Router::new()
        .route(
            "/files",
            axum::routing::get(
                |State(bytes): State<Arc<std::sync::Mutex<Vec<u8>>>>,
                 headers: HeaderMap,
                 Query(query): Query<HashMap<String, String>>| async move {
                    assert_eq!(headers["x-access-token"], "sandbox-fixture-token");
                    assert_eq!(headers["host"], "proxy-fixture.local");
                    assert!(headers.get("x-api-key").is_none());
                    assert_eq!(query["path"], "/root/a'&query=literal.bin");
                    axum::body::Bytes::from(bytes.lock().unwrap().clone())
                },
            )
            .post(
                |State(bytes): State<Arc<std::sync::Mutex<Vec<u8>>>>,
                 headers: HeaderMap,
                 Query(query): Query<HashMap<String, String>>,
                 body: axum::body::Bytes| async move {
                    assert_eq!(headers["x-access-token"], "sandbox-fixture-token");
                    assert_eq!(headers["host"], "proxy-fixture.local");
                    assert!(headers.get("x-api-key").is_none());
                    assert_eq!(headers["content-type"], "application/octet-stream");
                    assert_eq!(query["path"], "/root/a'&query=literal.bin");
                    *bytes.lock().unwrap() = body.to_vec();
                    Json(json!([{"path": query["path"]}]))
                },
            ),
        )
        .with_state(bytes.clone());
    let (api_endpoint, api_task) = server(api).await;
    let (envd_endpoint, envd_task) = server(envd).await;
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.bin");
    let destination = directory.path().join("destination.bin");
    let original = vec![0, 255, 128, 13, 10, 39, 36, 0];
    std::fs::write(&source, &original).unwrap();
    let common = [
        "sandbox",
        "vm",
        "--endpoint",
        &api_endpoint,
        "files",
        "test-vm",
        "--envd-endpoint",
        &envd_endpoint,
        "--envd-host",
        "proxy-fixture.local",
    ];
    let upload = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args(common)
        .args(["upload", source.to_str().unwrap(), path])
        .env("HV2_API_KEY", "platform-fixture-secret")
        .output()
        .await
        .unwrap();
    assert!(
        upload.status.success(),
        "{}",
        String::from_utf8_lossy(&upload.stderr)
    );
    assert_eq!(*bytes.lock().unwrap(), original);
    let download = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args(common)
        .args(["download", path, destination.to_str().unwrap()])
        .env("HV2_API_KEY", "platform-fixture-secret")
        .output()
        .await
        .unwrap();
    assert!(
        download.status.success(),
        "{}",
        String::from_utf8_lossy(&download.stderr)
    );
    assert_eq!(std::fs::read(&destination).unwrap(), original);
    let repeated = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args(common)
        .args(["download", path, destination.to_str().unwrap()])
        .env("HV2_API_KEY", "platform-fixture-secret")
        .output()
        .await
        .unwrap();
    assert_eq!(repeated.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already exists"));
    assert_eq!(std::fs::read(&destination).unwrap(), original);
    api_task.abort();
    envd_task.abort();
}

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
