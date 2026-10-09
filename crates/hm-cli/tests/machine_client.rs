//! Shipped CLI machine management against an owned HTTP protocol fixture.
use axum::{extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn machine_commands_authenticate_send_what_was_asked_and_keep_exit_codes() {
    let observed = Arc::new(Mutex::new(Vec::<(String, String, Value)>::new()));
    let seen = observed.clone();
    let app = Router::new().fallback(move |request: Request| {
        let seen = seen.clone();
        async move {
            if request
                .headers()
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                != Some("owned-machine-key")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let method = request.method().to_string();
            // The query too, where there is one, so a flag that becomes one
            // is seen.
            let path = match request.uri().query() {
                Some(query) => format!("{}?{query}", request.uri().path()),
                None => request.uri().path().to_string(),
            };
            let body = axum::body::to_bytes(request.into_body(), 65536)
                .await
                .unwrap();
            // JSON where it is JSON; typed console input as the text it is.
            let body = serde_json::from_slice(&body).unwrap_or_else(|_| {
                if body.is_empty() {
                    Value::Null
                } else {
                    Value::String(String::from_utf8_lossy(&body).into_owned())
                }
            });
            seen.lock()
                .unwrap()
                .push((method.clone(), path.clone(), body));
            let machine = json!({"machineID":"vm-owned","name":"web-01","state":"running"});
            match (method.as_str(), path.as_str()) {
                ("GET", "/machines") => Json(json!([machine])).into_response(),
                ("POST", "/machines") => (StatusCode::CREATED, Json(machine)).into_response(),
                ("DELETE", "/machines/running") => StatusCode::CONFLICT.into_response(),
                ("DELETE", _) => StatusCode::NO_CONTENT.into_response(),
                ("POST", "/machines/web-01/exec") => Json(json!({
                    "exit_code": 7, "signal": null, "stdout": "guest-out\n", "stderr": "guest-err\n"
                }))
                .into_response(),
                ("GET", "/machines/web-01/console") => "[    0.1] booted\n".into_response(),
                ("GET", "/machines/web-01/console?tail=64") => "login: ".into_response(),
                ("POST", "/machines/web-01/console") => StatusCode::NO_CONTENT.into_response(),
                ("GET", "/machines/missing") => StatusCode::NOT_FOUND.into_response(),
                _ => Json(machine).into_response(),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let hm = |arguments: Vec<&'static str>, key: &'static str| {
        let endpoint = endpoint.clone();
        async move {
            tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
                .args(["sandbox", "vm", "--endpoint", &endpoint, "machine"])
                .args(arguments)
                .env("HV2_API_KEY", key)
                .output()
                .await
                .unwrap()
        }
    };

    for arguments in [
        vec!["create", "web-01"],
        vec![
            "create",
            "web-02",
            "--template",
            "ubuntu",
            "--cpus",
            "2",
            "--memory-mb",
            "2048",
            "--disk-gib",
            "20",
            "--node",
            "node-a",
            "--no-autostart",
            "--restart",
            "never",
            "--no-start",
            "--allow-out",
            "api.example.com",
            "--allow-out",
            "10.0.0.0/8",
            "--deny-out",
            "10.9.0.0/16",
            "--no-internet",
        ],
        vec!["create", "web-03", "--network"],
        vec!["list"],
        vec!["inspect", "web-01"],
        vec!["start", "web-01"],
        vec!["stop", "web-01"],
        vec!["restart", "web-01"],
        vec!["decisions", "web-01"],
        vec!["delete", "web-01"],
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    {
        let requests = observed.lock().unwrap();
        let seen: Vec<(&str, &str)> = requests
            .iter()
            .map(|(method, path, _)| (method.as_str(), path.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                ("POST", "/machines"),
                ("POST", "/machines"),
                ("POST", "/machines"),
                ("GET", "/machines"),
                ("GET", "/machines/web-01"),
                ("POST", "/machines/web-01/start"),
                ("POST", "/machines/web-01/stop"),
                ("POST", "/machines/web-01/restart"),
                ("GET", "/machines/web-01/network/decisions"),
                ("DELETE", "/machines/web-01"),
            ]
        );
        // The defaults: no NIC, and nothing the node should decide.
        assert_eq!(
            requests[0].2,
            json!({"name":"web-01","templateID":"base","autostart":true,
                   "restartPolicy":"always","start":true})
        );
        assert_eq!(
            requests[1].2,
            json!({"name":"web-02","templateID":"ubuntu","diskGiB":20,"autostart":false,
                   "restartPolicy":"never","start":false,"cpuCount":2,"memoryMB":2048,
                   "nodeID":"node-a",
                   "network":{"allowOut":["api.example.com","10.0.0.0/8"],
                              "denyOut":["10.9.0.0/16"],"allowInternetAccess":false}})
        );
        // `--network` alone: a NIC with the node's default policy.
        assert_eq!(
            requests[2].2["network"],
            json!({"allowOut":[],"denyOut":[]})
        );
    }

    // A guest command's streams and exit code are the command's own.
    let output = hm(
        vec!["exec", "web-01", "--", "sh", "-c", "it's"],
        "owned-machine-key",
    )
    .await;
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"guest-out\n");
    assert_eq!(output.stderr, b"guest-err\n");
    assert_eq!(
        observed.lock().unwrap().last().unwrap().2,
        json!({"cmd":"exec 'sh' '-c' 'it'\\''s'","timeout_secs":60})
    );
    // The console is the guest's text, printed as it is.
    let output = hm(vec!["console", "web-01"], "owned-machine-key").await;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"[    0.1] booted\n");

    // With --tail, the console's last bytes as the guest wrote them.
    let output = hm(
        vec!["console", "web-01", "--tail", "64"],
        "owned-machine-key",
    )
    .await;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"login: ");
    // Typing sends the text and Enter, or the text alone.
    for (arguments, typed) in [
        (vec!["type", "web-01", "sudo reboot"], "sudo reboot\n"),
        (vec!["type", "web-01", "y", "--no-enter"], "y"),
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = observed.lock().unwrap();
        let (method, path, body) = requests.last().unwrap();
        assert_eq!(
            (method.as_str(), path.as_str()),
            ("POST", "/machines/web-01/console")
        );
        assert_eq!(body, &Value::String(typed.to_owned()));
    }
    // From an image: no template is sent, and no disk size unless asked.
    let output = hm(
        vec!["create", "cloud-01", "--image", "cirros.raw"],
        "owned-machine-key",
    )
    .await;
    assert!(output.status.success());
    assert_eq!(
        observed.lock().unwrap().last().unwrap().2,
        json!({"name":"cloud-01","image":"cirros.raw","autostart":true,
               "restartPolicy":"always","start":true})
    );

    // Refusals fail, and a bad name never reaches the API.
    let before = observed.lock().unwrap().len();
    for (arguments, key) in [
        (vec!["inspect", "missing"], "owned-machine-key"),
        (vec!["delete", "running"], "owned-machine-key"),
        (vec!["list"], "another-key"),
    ] {
        let output = hm(arguments.clone(), key).await;
        assert!(!output.status.success(), "{arguments:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains(key));
    }
    assert_eq!(observed.lock().unwrap().len(), before + 2);
    for arguments in [
        vec!["inspect", "../etc"],
        vec!["create", "has space"],
        vec!["create", "web-04", "--disk-gib", "0"],
        vec!["create", "web-05", "--image", "a.raw", "--template", "base"],
        vec!["type", "web-01", "--no-enter", ""],
        vec!["exec", "web-01"],
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(!output.status.success(), "{arguments:?}");
    }
    assert_eq!(observed.lock().unwrap().len(), before + 2);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
