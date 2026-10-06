//! Shipped CLI volume management against an owned HTTP protocol fixture.
use std::sync::{Arc, Mutex};
use axum::{extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
use serde_json::json;

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
