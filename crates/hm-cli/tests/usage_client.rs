//! `hm sandbox vm usage` against an owned HTTP fixture.
use axum::{extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn usage_asks_for_the_window_and_grouping_given_and_prints_the_answer() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = Arc::clone(&seen);
    let app = Router::new().fallback(move |request: Request| {
        let kept = Arc::clone(&kept);
        async move {
            if request
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                != Some("usage-key")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let asked = match request.uri().query() {
                Some(query) => format!("{}?{query}", request.uri().path()),
                None => request.uri().path().to_string(),
            };
            kept.lock().unwrap().push(asked);
            Json(json!({"total": {"sandboxes": 2, "runningSeconds": 12.5}, "groups": []}))
                .into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let hm = |arguments: Vec<&'static str>| {
        let endpoint = endpoint.clone();
        async move {
            tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
                .args(["sandbox", "vm", "--endpoint", &endpoint, "usage"])
                .args(arguments)
                .env("HV2_API_KEY", "usage-key")
                .output()
                .await
                .unwrap()
        }
    };
    for (arguments, asked) in [
        (vec![], "/usage?by=sandbox"),
        (
            vec![
                "--by",
                "team",
                "--from",
                "2026-10-01T00:00:00Z",
                "--to",
                "1791000000000",
            ],
            "/usage?by=team&from=2026-10-01T00%3A00%3A00Z&to=1791000000000",
        ),
    ] {
        let output = hm(arguments.clone()).await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(seen.lock().unwrap().last().unwrap(), asked);
        let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(printed["total"]["runningSeconds"], 12.5);
    }
    // A grouping that is not one never reaches the node.
    let before = seen.lock().unwrap().len();
    let output = hm(vec!["--by", "colour"]).await;
    assert!(!output.status.success());
    assert_eq!(seen.lock().unwrap().len(), before);
    server.abort();
}
