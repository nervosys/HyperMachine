//! Shipped CLI volume management against an owned HTTP protocol fixture.
use std::sync::{Arc, Mutex};
use axum::{extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
use serde_json::json;

#[tokio::test]
async fn volume_upload_refuses_over_limit_source_before_network_contact() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("over-limit.bin");
    std::fs::File::create(&source).unwrap().set_len(4 * 1024 * 1024 * 1024 + 1).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let output = tokio::time::timeout(std::time::Duration::from_secs(2),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .kill_on_drop(true)
            .args(["sandbox", "vm", "--endpoint", &endpoint, "volume", "upload", "vol-owned"])
            .arg(&source).args(["--path", "/file"])
            .env("HV2_VOLUME_TOKEN", "owned-content-token").output())
        .await.expect("over-limit source was not refused promptly").unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("at most 4 GiB"));
    assert!(tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept()).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn volume_upload_refuses_named_pipe_without_writer_or_network_contact() {
    use std::os::unix::ffi::OsStrExt;
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.fifo");
    let path = std::ffi::CString::new(source.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let output = tokio::time::timeout(std::time::Duration::from_secs(2),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .kill_on_drop(true)
            .args(["sandbox", "vm", "--endpoint", &endpoint, "volume", "upload", "vol-owned"])
            .arg(&source).args(["--path", "/file"])
            .env("HV2_VOLUME_TOKEN", "owned-content-token").output())
        .await.expect("FIFO open waited for a writer").unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("regular file"));
    assert!(tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept()).await.is_err());
}

#[tokio::test]
async fn volume_commands_authenticate_encode_requests_and_propagate_errors() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let seen = observed.clone();
    let app = Router::new().fallback(move |request: Request| {
        let seen = seen.clone();
        async move {
            if request.headers().get("x-api-key").and_then(|value| value.to_str().ok()) != Some("owned-volume-key") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let method = request.method().to_string();
            let path = request.uri().path().to_string();
            let body = axum::body::to_bytes(request.into_body(), 65536).await.unwrap();
            seen.lock().unwrap().push((method.clone(), path.clone(), body.to_vec()));
            if path.ends_with("/conflict") { return StatusCode::CONFLICT.into_response(); }
            if method == "DELETE" { return StatusCode::NO_CONTENT.into_response(); }
            let volume = json!({"volumeID":"vol-owned","name":"owned","token":"owned-content-token"});
            if method == "GET" && path == "/volumes" {
                Json(json!([volume])).into_response()
            } else {
                Json(volume).into_response()
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for arguments in [vec!["create", "owned"], vec!["list"], vec!["inspect", "vol-owned"], vec!["delete", "vol-owned"]] {
        let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args(["sandbox", "vm", "--endpoint", &endpoint, "volume"]).args(&arguments)
            .env("HV2_API_KEY", "owned-volume-key").output().await.unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("owned-content-token"));
        if arguments[0] == "delete" {
            assert!(result.stdout.is_empty());
        } else {
            let _: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        }
    }
    {
        let requests = observed.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&requests[0].2).unwrap(), json!({"name":"owned"}));
        assert_eq!(requests[1].0, "GET");
        assert_eq!(requests[2].1, "/volumes/vol-owned");
        assert_eq!(requests[3].0, "DELETE");
    }
    for (id, key) in [("conflict", "owned-volume-key"), ("vol-owned", "wrong"), ("../escape", "owned-volume-key")] {
        let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args(["sandbox", "vm", "--endpoint", &endpoint, "volume", "inspect", id])
            .env("HV2_API_KEY", key).output().await.unwrap();
        assert!(!result.status.success());
    }
    assert_eq!(observed.lock().unwrap().len(), 5);
    server.abort(); assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn volume_download_publishes_binary_and_refuses_existing_destination() {
    let bytes: Vec<u8> = (0..=255).cycle().take(512 * 1024).collect();
    let payload = bytes.clone();
    let contacts = Arc::new(Mutex::new(0usize));
    let seen = contacts.clone();
    let app = Router::new().fallback(move |request: Request| {
        let payload = payload.clone();
        let seen = seen.clone();
        async move {
            assert!(!request.headers().contains_key("x-api-key"));
            assert_eq!(request.headers()["authorization"], "Bearer owned-content-token");
            *seen.lock().unwrap() += 1;
            axum::body::Body::from(payload).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("download.bin");
    for first in [true, false] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
            .args(["sandbox", "vm", "--endpoint", &endpoint, "volume", "download", "vol-owned"])
            .arg(&destination).args(["--path", "/input.bin"])
            .env("HV2_VOLUME_TOKEN", "owned-content-token").env("HV2_API_KEY", "invalid\nunused-key")
            .output().await.unwrap();
        assert_eq!(output.status.success(), first);
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    }
    assert_eq!(*contacts.lock().unwrap(), 1);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    server.abort(); assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn truncated_volume_download_leaves_no_destination_or_staging() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(socket.read_u8().await.unwrap());
            assert!(headers.len() < 65536);
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial").await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("download.bin");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args(["sandbox", "vm", "--endpoint", &endpoint, "volume", "download", "vol-owned"])
        .arg(&destination).args(["--path", "/input.bin"])
        .env("HV2_VOLUME_TOKEN", "owned-content-token").output().await.unwrap();
    assert!(!output.status.success());
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    server.await.unwrap();
}

#[tokio::test]
async fn volume_upload_streams_binary_without_management_credentials() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let seen = recorded.clone();
    let app = Router::new().fallback(move |request: Request| {
        let seen = seen.clone();
        async move {
            assert!(!request.headers().contains_key("x-api-key"));
            assert_eq!(request.headers()["authorization"], "Bearer owned-content-token");
            let query = request.uri().query().unwrap().to_string();
            let bytes = axum::body::to_bytes(request.into_body(), 2 << 20).await.unwrap();
            let size = bytes.len();
            seen.lock().unwrap().push((query.clone(), bytes.to_vec()));
            if query.contains("overwrite=false") { return StatusCode::CONFLICT.into_response(); }
            (StatusCode::CREATED, Json(json!({"size":size}))).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.bin");
    let bytes: Vec<u8> = (0..=255).cycle().take(512 * 1024).collect();
    std::fs::write(&source, &bytes).unwrap();
    for no_clobber in [false, true] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"));
        command.args(["sandbox", "vm", "--endpoint", &endpoint, "volume", "upload", "vol-owned"])
            .arg(&source).args(["--path", "/nested/a b.bin", "--force"])
            .env("HV2_VOLUME_TOKEN", "owned-content-token")
            .env("HV2_API_KEY", "invalid\nmanagement-key-must-not-be-used");
        if no_clobber { command.arg("--no-clobber"); }
        let output = command.output().await.unwrap();
        assert_eq!(output.status.success(), !no_clobber);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("owned-content-token"));
    }
    {
        let rows = recorded.lock().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, bytes);
        assert!(rows[0].0.contains("atomic=true") && rows[0].0.contains("force=true"));
        assert!(rows[0].0.contains("path=%2Fnested%2Fa+b.bin"));
    }
    std::fs::remove_file(source).unwrap();
    server.abort(); assert!(server.await.unwrap_err().is_cancelled());
}
