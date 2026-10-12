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
                ("POST", "/machines/web-01/runs") => (
                    StatusCode::CREATED,
                    Json(json!({"runID": 3, "state": "running"})),
                )
                    .into_response(),
                ("GET", "/machines/web-01/runs") => Json(json!([{"runID": 3}])).into_response(),
                ("GET", "/machines/web-01/runs/3?stdout=0&stderr=0") => {
                    Json(json!({"runID": 3, "stdout": {"text": "built\n"}})).into_response()
                }
                ("POST", "/machines/web-01/prompts") => (
                    StatusCode::CREATED,
                    Json(json!({"promptID": 5, "state": "running"})),
                )
                    .into_response(),
                ("GET", "/machines/web-01/prompts/5?after=0") => Json(json!({
                    "state": "running", "exitCode": null, "next": 17,
                    "events": [{"offset": 0, "event": {"type": "start"}}],
                }))
                .into_response(),
                ("GET", "/machines/web-01/prompts/5?after=17") => Json(json!({
                    "state": "exited", "exitCode": 4, "next": 40,
                    "events": [
                        {"offset": 17, "text": "plain"},
                        {"offset": 23, "event": {"type": "result"}},
                    ],
                }))
                .into_response(),
                ("PUT", "/machines/web-01/files?path=%2Fsrv%2Fapp%2Fa+file.bin&user=dev")
                | ("PUT", "/machines/web-01/files?path=%2Ftmp%2Fplain") => {
                    StatusCode::NO_CONTENT.into_response()
                }
                ("GET", "/machines/web-01/files?path=%2Fetc%2Fhostname") => {
                    b"web-01\n\x00\xff".to_vec().into_response()
                }
                ("GET", "/machines/web-01/files?path=%2Fnope") => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"code": 404, "message": "reading /nope: No such file"})),
                )
                    .into_response(),
                ("GET", "/machines/web-01/snapshots/snap-0123456789abcde/disk") => {
                    let mut disk = vec![0u8; 3 << 20];
                    disk[..5].copy_from_slice(b"start");
                    disk[(2 << 20) + 7] = 9;
                    disk.into_response()
                }
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
                .env("HM_TEST_AGENT_KEY", "a-key-for-the-agent")
                .env_remove("HM_TEST_UNSET")
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

    // A run is started and left: the command quoted for the guest's shell,
    // and only the variables that were named.
    let output = hm(
        vec![
            "run",
            "web-01",
            "--cwd",
            "/srv",
            "--user",
            "dev",
            "--pass-env",
            "HM_TEST_AGENT_KEY",
            "--",
            "make",
            "it's",
        ],
        "owned-machine-key",
    )
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        observed.lock().unwrap().last().unwrap().2,
        json!({"cmd": r"exec 'make' 'it'\''s'", "cwd": "/srv", "user": "dev",
               "env": {"HM_TEST_AGENT_KEY": "a-key-for-the-agent"}})
    );
    for (arguments, path) in [
        (vec!["runs", "web-01"], "/machines/web-01/runs"),
        (
            vec!["runs", "web-01", "3"],
            "/machines/web-01/runs/3?stdout=0&stderr=0",
        ),
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(output.status.success(), "{arguments:?}");
        assert_eq!(observed.lock().unwrap().last().unwrap().1, path);
    }

    // A prompt is followed to its end: each event on a line as the agent
    // printed it, read on from where the last read stopped, and the agent's
    // exit code kept.
    let output = hm(
        vec![
            "prompt",
            "web-01",
            "--provider",
            "claude",
            "--model",
            "opus",
            "--unattended",
            "--user",
            "dev",
            "--pass-env",
            "HM_TEST_AGENT_KEY",
            "fix the $(tests)",
        ],
        "owned-machine-key",
    )
    .await;
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "{\"type\":\"start\"}\nplain\n{\"type\":\"result\"}\n"
    );
    {
        let requests = observed.lock().unwrap();
        let last: Vec<(&str, &str)> = requests[requests.len() - 3..]
            .iter()
            .map(|(method, path, _)| (method.as_str(), path.as_str()))
            .collect();
        assert_eq!(
            last,
            [
                ("POST", "/machines/web-01/prompts"),
                ("GET", "/machines/web-01/prompts/5?after=0"),
                ("GET", "/machines/web-01/prompts/5?after=17"),
            ]
        );
        assert_eq!(
            requests[requests.len() - 3].2,
            json!({"provider": "claude", "prompt": "fix the $(tests)", "model": "opus",
                   "unattended": true, "user": "dev",
                   "env": {"HM_TEST_AGENT_KEY": "a-key-for-the-agent"}})
        );
    }
    // Detached, it is started and not followed.
    let output = hm(
        vec![
            "prompt",
            "web-01",
            "--provider",
            "custom",
            "--command",
            "my-agent",
            "--detach",
            "hello",
        ],
        "owned-machine-key",
    )
    .await;
    assert!(output.status.success());
    {
        let requests = observed.lock().unwrap();
        let (method, path, body) = requests.last().unwrap();
        assert_eq!(
            (method.as_str(), path.as_str()),
            ("POST", "/machines/web-01/prompts")
        );
        assert_eq!(
            body,
            &json!({"provider": "custom", "prompt": "hello", "command": "my-agent"})
        );
    }

    // A file goes in as the bytes it is, to the path and for the user asked;
    // and comes out as the bytes it is, never over a file that is there.
    let scratch = std::env::temp_dir().join(format!("hm-machine-files-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let local = scratch.join("in.bin");
    std::fs::write(&local, b"bytes \x00\xff in").unwrap();
    let local_arg: &'static str = Box::leak(local.to_str().unwrap().to_owned().into_boxed_str());
    for (arguments, path) in [
        (
            vec![
                "upload",
                "web-01",
                local_arg,
                "/srv/app/a file.bin",
                "--user",
                "dev",
            ],
            "/machines/web-01/files?path=%2Fsrv%2Fapp%2Fa+file.bin&user=dev",
        ),
        (
            vec!["upload", "web-01", local_arg, "/tmp/plain"],
            "/machines/web-01/files?path=%2Ftmp%2Fplain",
        ),
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = observed.lock().unwrap();
        let (method, seen, body) = requests.last().unwrap();
        assert_eq!((method.as_str(), seen.as_str()), ("PUT", path));
        // The fixture keeps a body that is not JSON as lossy text.
        assert_eq!(body, &Value::String("bytes \0\u{fffd} in".to_owned()));
    }
    let fetched = scratch.join("out.bin");
    let fetched_arg: &'static str =
        Box::leak(fetched.to_str().unwrap().to_owned().into_boxed_str());
    let output = hm(
        vec!["download", "web-01", "/etc/hostname", fetched_arg],
        "owned-machine-key",
    )
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&fetched).unwrap(), b"web-01\n\x00\xff");
    // Again, onto the file that is now there: refused, and it is unchanged.
    std::fs::write(&fetched, b"mine").unwrap();
    let output = hm(
        vec!["download", "web-01", "/etc/hostname", fetched_arg],
        "owned-machine-key",
    )
    .await;
    assert!(!output.status.success());
    assert_eq!(std::fs::read(&fetched).unwrap(), b"mine");
    let output = hm(
        vec!["download", "web-01", "/etc/hostname", "-"],
        "owned-machine-key",
    )
    .await;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"web-01\n\x00\xff");
    // What the node refuses is said, and nothing is written for it.
    let missing = scratch.join("missing.bin");
    let missing_arg: &'static str =
        Box::leak(missing.to_str().unwrap().to_owned().into_boxed_str());
    let output = hm(
        vec!["download", "web-01", "/nope", missing_arg],
        "owned-machine-key",
    )
    .await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No such file"));
    assert!(!missing.exists());
    // A local file that is not there never reaches the API.
    let sent = observed.lock().unwrap().len();
    let output = hm(
        vec!["upload", "web-01", missing_arg, "/tmp/x"],
        "owned-machine-key",
    )
    .await;
    assert!(!output.status.success());
    assert_eq!(observed.lock().unwrap().len(), sent);
    std::fs::remove_dir_all(&scratch).unwrap();

    // Publishing sends the guest's port, and the node's only when chosen.
    for (arguments, method, path, body) in [
        (
            vec!["publish", "web-01", "80"],
            "POST",
            "/machines/web-01/ports",
            json!({"guestPort": 80}),
        ),
        (
            vec!["publish", "web-01", "443", "--host-port", "20443"],
            "POST",
            "/machines/web-01/ports",
            json!({"guestPort": 443, "hostPort": 20443}),
        ),
        (
            vec!["ports", "web-01"],
            "GET",
            "/machines/web-01/ports",
            Value::Null,
        ),
        (
            vec!["unpublish", "web-01", "20443"],
            "DELETE",
            "/machines/web-01/ports/20443",
            Value::Null,
        ),
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = observed.lock().unwrap();
        let (seen_method, seen_path, sent) = requests.last().unwrap();
        assert_eq!((seen_method.as_str(), seen_path.as_str()), (method, path));
        assert_eq!(sent, &body);
    }

    // Snapshots: what is asked goes where it should.
    for (arguments, method, path, body) in [
        (
            vec!["snapshot", "take", "web-01", "--name", "before upgrade"],
            "POST",
            "/machines/web-01/snapshots",
            json!({"name": "before upgrade"}),
        ),
        (
            vec!["snapshot", "take", "web-01"],
            "POST",
            "/machines/web-01/snapshots",
            json!({}),
        ),
        (
            vec!["snapshot", "list", "web-01"],
            "GET",
            "/machines/web-01/snapshots",
            Value::Null,
        ),
        (
            vec!["snapshot", "restore", "web-01", "snap-0123456789abcde"],
            "POST",
            "/machines/web-01/snapshots/snap-0123456789abcde/restore",
            json!({}),
        ),
        (
            vec!["snapshot", "delete", "web-01", "snap-0123456789abcde"],
            "DELETE",
            "/machines/web-01/snapshots/snap-0123456789abcde",
            Value::Null,
        ),
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = observed.lock().unwrap();
        let (seen_method, seen_path, sent) = requests.last().unwrap();
        assert_eq!((seen_method.as_str(), seen_path.as_str()), (method, path));
        assert_eq!(sent, &body);
    }
    // A download is the disk, byte for byte, and never over a file.
    let scratch = std::env::temp_dir().join(format!("hm-machine-disk-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let image = scratch.join("disk.img");
    let image_arg: &'static str = Box::leak(image.to_str().unwrap().to_owned().into_boxed_str());
    let download = vec![
        "snapshot",
        "download",
        "web-01",
        "snap-0123456789abcde",
        image_arg,
    ];
    let output = hm(download.clone(), "owned-machine-key").await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let disk = std::fs::read(&image).unwrap();
    assert_eq!(disk.len(), 3 << 20);
    assert_eq!(&disk[..5], b"start");
    assert_eq!(disk[(2 << 20) + 7], 9);
    assert_eq!(disk.iter().filter(|b| **b != 0).count(), 6);
    let output = hm(download, "owned-machine-key").await;
    assert!(!output.status.success());
    std::fs::remove_dir_all(&scratch).unwrap();

    // A resize sends only the sizes that were given.
    for (arguments, body) in [
        (
            vec!["resize", "web-01", "--cpus", "4"],
            json!({"cpuCount": 4}),
        ),
        (
            vec![
                "resize",
                "web-01",
                "--memory-mb",
                "8192",
                "--disk-gib",
                "40",
            ],
            json!({"memoryMB": 8192, "diskGiB": 40}),
        ),
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = observed.lock().unwrap();
        let (method, path, sent) = requests.last().unwrap();
        assert_eq!(
            (method.as_str(), path.as_str()),
            ("PATCH", "/machines/web-01")
        );
        assert_eq!(sent, &body);
    }

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
        vec!["run", "web-01"],
        vec!["resize", "web-01"],
        vec!["publish", "web-01", "0"],
        vec!["publish", "web-01", "70000"],
        // A variable that is not set is not sent as an empty one.
        vec!["run", "web-01", "--pass-env", "HM_TEST_UNSET", "--", "true"],
        vec!["prompt", "web-01", "hello"],
        vec![
            "prompt",
            "web-01",
            "--provider",
            "claude",
            "--pass-env",
            "HM_TEST_UNSET",
            "hello",
        ],
    ] {
        let output = hm(arguments.clone(), "owned-machine-key").await;
        assert!(!output.status.success(), "{arguments:?}");
    }
    assert_eq!(observed.lock().unwrap().len(), before + 2);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
