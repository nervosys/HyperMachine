//! Existing volume routes exercised over real HTTP with owned protocol nodes.
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use axum::{body::{to_bytes, Body}, extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
use hv2_cluster::{control::{self, ControlConfig, ControlPlane, CLUSTER_TOKEN_HEADER},
    model::{now_ms, volume_id, NodeInfo}, store::{ClusterStore, MemoryStore}};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn volume_routes_preserve_placement_query_binary_and_credentials() {
    let store = Arc::new(MemoryStore::new());
    let payload = Arc::new(Mutex::new(Vec::<u8>::new()));
    let seen = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
    let id = volume_id("owned");
    let mut tasks = Vec::new();
    for node_id in ["volume-a", "volume-b"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        store.put_node(&NodeInfo { id: node_id.into(), api: format!("http://{addr}"), proxy: addr,
            capacity: 4, running: 0, heartbeat_ms: now_ms(), version: "fixture".into(),
            jwk: None, templates: vec!["base".into()], template_metadata: BTreeMap::new() },
            Duration::from_secs(60)).await.unwrap();
        let data = payload.clone();
        let observed = seen.clone();
        let volume = id.clone();
        let app = Router::new().fallback(move |request: Request| {
            let data = data.clone();
            let observed = observed.clone();
            let volume = volume.clone();
            async move {
                assert_eq!(request.headers()[CLUSTER_TOKEN_HEADER], "owned-cluster-token");
                assert!(!request.headers().contains_key("x-api-key"));
                let path = request.uri().path().to_string();
                let method = request.method().to_string();
                if path.starts_with("/volumecontent/") && request.headers().get("authorization")
                    .and_then(|v| v.to_str().ok()) != Some("Bearer owned-volume-token") {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                observed.lock().push((node_id.into(), method.clone(), request.uri().to_string()));
                if path.starts_with("/volumecontent/") {
                    if method == "PUT" {
                        let bytes = to_bytes(request.into_body(), 2 << 20).await.unwrap();
                        let size = bytes.len();
                        *data.lock() = bytes.to_vec();
                        return (StatusCode::CREATED, Json(json!({"size":size}))).into_response();
                    }
                    return Body::from(data.lock().clone()).into_response();
                }
                let metadata = json!({"volumeID":volume,"name":"owned","token":"owned-volume-token"});
                if path == "/volumes" && method == "GET" {
                    Json(json!([metadata])).into_response()
                } else if method == "POST" {
                    (StatusCode::CREATED, Json(metadata)).into_response()
                } else {
                    Json(metadata).into_response()
                }
            }
        });
        tasks.push(tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let plane = ControlPlane::new(store, ControlConfig { api_key: Some("owned-api-key".into()),
        api_keys: vec![], access_audit: None, cluster_token: Some("owned-cluster-token".into()),
        proxy_port: 5981, create_timeout: Duration::from_secs(5), identity_issuer: None });
    tasks.push(tokio::spawn(async move { axum::serve(listener, control::router(plane)).await.unwrap() }));
    let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
    assert_eq!(http.post(format!("{base}/volumes")).json(&json!({"name":"owned"})).send().await.unwrap().status(), 401);
    assert!(seen.lock().is_empty());
    assert_eq!(http.post(format!("{base}/volumes")).header("x-api-key", "owned-api-key")
        .json(&json!({"name":"owned"})).send().await.unwrap().status(), 201);
    assert_eq!(http.get(format!("{base}/volumes/{id}")).header("x-api-key", "owned-api-key")
        .send().await.unwrap().status(), 200);
    let bytes: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    let route = format!("/volumecontent/{id}/file?path=%2Fnested%2Fa%20b.bin&atomic=true&force=true");
    let uploaded = http.put(format!("{base}{route}")).bearer_auth("owned-volume-token")
        .header(CLUSTER_TOKEN_HEADER, "client-must-not-select-node-credential")
        .header("content-type", "application/octet-stream").body(bytes.clone()).send().await.unwrap();
    assert_eq!(uploaded.status(), 201);
    let stat: serde_json::Value = uploaded.json().await.unwrap();
    assert_eq!(stat["size"], bytes.len());
    assert_eq!(http.get(format!("{base}{route}")).bearer_auth("owned-volume-token")
        .send().await.unwrap().bytes().await.unwrap().as_ref(), bytes.as_slice());
    assert_eq!(http.put(format!("{base}{route}")).bearer_auth("wrong")
        .body("refused").send().await.unwrap().status(), 401);
    assert_eq!(*payload.lock(), bytes);
    {
        let rows = seen.lock();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.0 == rows[0].0));
        assert_eq!(rows[2].2, route);
        assert_eq!(rows[3].2, route);
    }
    let listed: serde_json::Value = http.get(format!("{base}/volumes")).header("x-api-key", "owned-api-key")
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    for task in tasks { task.abort(); assert!(task.await.unwrap_err().is_cancelled()); }
}

#[tokio::test]
#[ignore = "requires HM_VOLUME_TEST_DAEMON, HM_VOLUME_TEST_KERNEL and HM_VOLUME_TEST_INITRD"]
async fn real_shared_volume_daemons_remain_accessible_after_routing_membership_changes() {
    struct Children(Vec<std::process::Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 { let _ = child.kill(); let _ = child.wait(); }
        }
    }
    let daemon = std::env::var("HM_VOLUME_TEST_DAEMON").unwrap();
    let kernel = std::env::var("HM_VOLUME_TEST_KERNEL").unwrap();
    let initrd = std::env::var("HM_VOLUME_TEST_INITRD").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut children = Children(Vec::new());
    let store = Arc::new(MemoryStore::new());
    let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
    let mut nodes = Vec::new();
    for index in 0..2 {
        let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = reservation.local_addr().unwrap();
        drop(reservation);
        let child = std::process::Command::new(&daemon)
            .args(["--no-template", "--port", &addr.port().to_string(), "--proxy-port", "0", "--volume-dir"])
            .arg(directory.path().join("volumes")).arg("--snapshot-store")
            .arg(directory.path().join(format!("snapshots-{index}")))
            .env("HV2_KERNEL", &kernel).env("HV2_INITRD", &initrd)
            .env("HV2_CLUSTER_TOKEN", "owned-cluster-token")
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        children.0.push(child);
        let api = format!("http://{addr}");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            assert!(children.0[index].try_wait().unwrap().is_none());
            if http.get(format!("{api}/sandboxes")).header(CLUSTER_TOKEN_HEADER, "owned-cluster-token")
                .send().await.is_ok_and(|response| response.status() == 200) { break; }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let node = NodeInfo { id: format!("real-volume-{index}"), api, proxy: addr,
            capacity: 4, running: 0, heartbeat_ms: now_ms(), version: "owned-real-daemon".into(),
            jwk: None, templates: vec!["base".into()], template_metadata: BTreeMap::new() };
        store.put_node(&node, Duration::from_secs(60)).await.unwrap();
        nodes.push(node);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let plane = ControlPlane::new(store.clone(), ControlConfig { api_key: Some("owned-api-key".into()),
        api_keys: vec![], access_audit: None, cluster_token: Some("owned-cluster-token".into()),
        proxy_port: 5981, create_timeout: Duration::from_secs(5), identity_issuer: None });
    let task = tokio::spawn(async move { axum::serve(listener, control::router(plane)).await.unwrap() });
    let response = http.post(format!("{base}/volumes")).header("x-api-key", "owned-api-key")
        .json(&json!({"name":"owned-real"})).send().await.unwrap();
    assert_eq!(response.status(), 201);
    let metadata: serde_json::Value = response.json().await.unwrap();
    if let Ok(cli) = std::env::var("HM_VOLUME_TEST_CLI") {
        let output = tokio::process::Command::new(&cli)
            .args(["sandbox", "vm", "--endpoint", &base, "volume", "create", "cli-owned"])
            .env("HV2_API_KEY", "owned-api-key").output().await.unwrap();
        assert!(output.status.success());
        let cli_volume: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let cli_id = cli_volume["volumeID"].as_str().unwrap();
        let cli_bytes: Vec<u8> = (0..=255).cycle().take(512 * 1024).collect();
        let cli_source = directory.path().join("cli-source.bin");
        std::fs::write(&cli_source, &cli_bytes).unwrap();
        for first_upload in [true, false] {
            let output = tokio::process::Command::new(&cli)
                .args(["sandbox", "vm", "--endpoint", &base, "volume", "upload", cli_id])
                .arg(&cli_source).args(["--path", "/cli.bin", "--no-clobber"])
                .env("HV2_VOLUME_TOKEN", cli_volume["token"].as_str().unwrap())
                .env("HV2_API_KEY", "invalid\nunused-management-key").output().await.unwrap();
            assert_eq!(output.status.success(), first_upload);
            assert_eq!(std::fs::read(directory.path().join("volumes").join(cli_id).join("data/cli.bin")).unwrap(), cli_bytes);
        }
        let cli_destination = directory.path().join("cli-download.bin");
        for first_download in [true, false] {
            let output = tokio::process::Command::new(&cli)
                .args(["sandbox", "vm", "--endpoint", &base, "volume", "download", cli_id])
                .arg(&cli_destination).args(["--path", "/cli.bin"])
                .env("HV2_VOLUME_TOKEN", cli_volume["token"].as_str().unwrap())
                .env("HV2_API_KEY", "invalid\nunused-management-key").output().await.unwrap();
            assert_eq!(output.status.success(), first_download);
            assert_eq!(std::fs::read(&cli_destination).unwrap(), cli_bytes);
        }
        for operation in ["list", "inspect", "delete"] {
            let mut command = tokio::process::Command::new(&cli);
            command.args(["sandbox", "vm", "--endpoint", &base, "volume", operation]);
            if operation != "list" { command.arg(cli_id); }
            let output = command.env("HV2_API_KEY", "owned-api-key").output().await.unwrap();
            assert!(output.status.success());
            if operation == "inspect" {
                assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(), cli_volume);
            } else if operation == "list" {
                assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap().as_array().unwrap().len(), 2);
            }
        }
        assert!(!directory.path().join("volumes").join(cli_id).exists());
    }
    let id = metadata["volumeID"].as_str().unwrap();
    let token = metadata["token"].as_str().unwrap();
    let bytes: Vec<u8> = (0..=255).cycle().take(1024 * 1024).collect();
    let route = format!("/volumecontent/{id}/file?path=%2Fnested%2Ffile.bin&force=true&atomic=true");
    assert_eq!(http.put(format!("{base}{route}")).bearer_auth(token).body(bytes.clone())
        .send().await.unwrap().status(), 201);
    assert_eq!(std::fs::read(directory.path().join("volumes").join(id).join("data/nested/file.bin")).unwrap(), bytes);
    for removed in &nodes {
        store.remove_node(&removed.id).await.unwrap();
        // Only the other node is eligible: the shared root must preserve access.
        assert_eq!(http.get(format!("{base}{route}")).bearer_auth(token).send().await.unwrap()
            .bytes().await.unwrap().as_ref(), bytes.as_slice());
        let inspected: serde_json::Value = http.get(format!("{base}/volumes/{id}"))
            .header("x-api-key", "owned-api-key").send().await.unwrap().json().await.unwrap();
        assert_eq!(inspected, metadata);
        store.put_node(removed, Duration::from_secs(60)).await.unwrap();
    }
    // Force the unfinished upload to node zero, then terminate that real
    // process only after its private staging file contains request bytes.
    store.remove_node(&nodes[1].id).await.unwrap();
    let address = reqwest::Url::parse(&base).unwrap();
    let mut unfinished = tokio::net::TcpStream::connect(("127.0.0.1", address.port().unwrap())).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = format!("PUT {route} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Length: 1048576\r\nConnection: close\r\n\r\n");
    unfinished.write_all(request.as_bytes()).await.unwrap();
    unfinished.write_all(&vec![42; 65536]).await.unwrap();
    let parent = directory.path().join("volumes").join(id).join("data/nested");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let staging = loop {
        let staged = std::fs::read_dir(&parent).unwrap().filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with(".hm-upload-")
                && entry.metadata().is_ok_and(|meta| meta.len() > 0));
        if let Some(entry) = staged { break entry.path(); }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    children.0[0].kill().unwrap(); children.0[0].wait().unwrap();
    unfinished.shutdown().await.unwrap();
    let mut refusal = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), unfinished.read_to_end(&mut refusal)).await.unwrap().unwrap();
    assert!(refusal.starts_with(b"HTTP/1.1 502 "));
    assert!(staging.exists()); // Hard termination cannot run the RAII cleanup.
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&staging).unwrap().permissions().mode() & 0o777, 0o600);
    store.remove_node(&nodes[0].id).await.unwrap();
    store.put_node(&nodes[1], Duration::from_secs(60)).await.unwrap();
    assert_eq!(http.get(format!("{base}{route}")).bearer_auth(token).send().await.unwrap()
        .bytes().await.unwrap().as_ref(), bytes.as_slice());
    for (index, node) in nodes.iter().enumerate() {
        if children.0[index].try_wait().unwrap().is_none() {
            children.0[index].kill().unwrap(); children.0[index].wait().unwrap();
        }
        let port = reqwest::Url::parse(&node.api).unwrap().port().unwrap().to_string();
        children.0[index] = std::process::Command::new(&daemon)
            .args(["--no-template", "--port", &port, "--proxy-port", "0", "--volume-dir"])
            .arg(directory.path().join("volumes")).arg("--snapshot-store")
            .arg(directory.path().join(format!("snapshots-{index}")))
            .env("HV2_KERNEL", &kernel).env("HV2_INITRD", &initrd)
            .env("HV2_CLUSTER_TOKEN", "owned-cluster-token")
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            assert!(children.0[index].try_wait().unwrap().is_none());
            if http.get(format!("{}/volumes/{id}", node.api)).header(CLUSTER_TOKEN_HEADER, "owned-cluster-token")
                .send().await.is_ok_and(|response| response.status() == 200) { break; }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        store.put_node(node, Duration::from_secs(60)).await.unwrap();
    }
    assert_eq!(http.get(format!("{base}{route}")).bearer_auth(token).send().await.unwrap()
        .bytes().await.unwrap().as_ref(), bytes.as_slice());
    assert_eq!(http.put(format!("{base}{route}")).bearer_auth("wrong").body("refused")
        .send().await.unwrap().status(), 401);
    let listed: serde_json::Value = http.get(format!("{base}/volumes"))
        .header("x-api-key", "owned-api-key").send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(http.delete(format!("{base}/volumes/{id}")).header("x-api-key", "owned-api-key")
        .send().await.unwrap().status(), 204);
    assert!(!directory.path().join("volumes").join(id).exists());
    task.abort(); assert!(task.await.unwrap_err().is_cancelled());
    for child in &mut children.0 { child.kill().unwrap(); child.wait().unwrap(); }
}
