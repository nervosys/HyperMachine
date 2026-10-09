//! `hm sandbox exec`: the shipped binary, one JSON request in and one JSON
//! response out.
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

/// Run `hm sandbox exec` with `request` on its standard input.
fn exec(request: &str) -> (Option<i32>, Value, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hm"))
        .args(["sandbox", "exec"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start hm");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write the request");
    let output = child.wait_with_output().expect("wait for hm");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let response = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    (
        output.status.code(),
        response,
        format!("{stdout}{}", String::from_utf8_lossy(&output.stderr)),
    )
}

/// A shell line, as this platform runs one, with the host's network so the
/// request asks for nothing a test machine may be unable to enforce.
fn shell(line: &str) -> Value {
    if cfg!(windows) {
        json!({
            "version": 1,
            "command": [r"C:\Windows\System32\cmd.exe", "/c", line],
            // The whole environment: without a PATH here, there is none.
            "env": { "SystemRoot": r"C:\Windows", "PATH": r"C:\Windows\System32" },
            "network": { "egress": "host" },
        })
    } else {
        json!({
            "version": 1,
            "command": ["/bin/sh", "-c", line],
            "env": { "PATH": "/usr/bin:/bin" },
            "network": { "egress": "host" },
        })
    }
}

#[test]
fn a_request_runs_and_the_response_carries_the_workloads_exit_and_output() {
    let line = if cfg!(windows) {
        "echo out& echo err 1>&2& exit 3"
    } else {
        "echo out; echo err >&2; exit 3"
    };
    let (code, response, raw) = exec(&shell(line).to_string());
    // The run happened, so `hm` itself succeeds; the workload's own exit is
    // in the response.
    assert_eq!(code, Some(0), "{raw}");
    assert_eq!(response["version"], 1, "{raw}");
    assert_eq!(response["exitCode"], 3, "{raw}");
    assert_eq!(
        response["stdout"].as_str().map(str::trim),
        Some("out"),
        "{raw}"
    );
    assert_eq!(
        response["stderr"].as_str().map(str::trim),
        Some("err"),
        "{raw}"
    );
    assert_eq!(response["killedBy"], Value::Null);
    assert_eq!(response["unenforced"], json!([]));
    assert_eq!(response["backend"], "process");
    assert!(response.get("error").is_none(), "{raw}");
    assert!(response.get("stdoutBase64").is_none(), "{raw}");
}

#[test]
fn standard_input_and_the_deadline_are_the_requests() {
    let mut request = shell(if cfg!(windows) { "findstr x" } else { "grep x" });
    request["stdin"] = json!("axb\nnone\n");
    let (code, response, raw) = exec(&request.to_string());
    assert_eq!(code, Some(0), "{raw}");
    assert_eq!(
        response["stdout"].as_str().map(str::trim),
        Some("axb"),
        "{raw}"
    );

    let mut request = shell(if cfg!(windows) {
        "for /l %i in () do @rem"
    } else {
        "sleep 30"
    });
    request["limits"] = json!({ "timeoutMs": 500 });
    let (code, response, raw) = exec(&request.to_string());
    assert_eq!(code, Some(0), "{raw}");
    assert_eq!(response["killedBy"], "wall-clock deadline", "{raw}");
}

#[test]
fn a_request_that_does_not_run_answers_with_why_and_exits_two() {
    for (request, kind, says) in [
        ("not json".to_string(), "invalid", "request:"),
        (
            json!({"version": 9, "command": ["x"]}).to_string(),
            "invalid",
            "version 9",
        ),
        (
            json!({"version": 1, "command": ["x"], "netwrok": {}}).to_string(),
            "invalid",
            "netwrok",
        ),
        (
            json!({"version": 1, "command": ["x"], "network": {"egress": "host"},
                   "filesystem": {"readOnly": ["relative/path"]}})
            .to_string(),
            "invalid",
            "granted path",
        ),
        (
            json!({"version": 1, "command": ["/no/such/program-anywhere"],
                   "network": {"egress": "host"}})
            .to_string(),
            "spawn",
            "program-anywhere",
        ),
    ] {
        let (code, response, raw) = exec(&request);
        assert_eq!(code, Some(2), "{request}: {raw}");
        assert_eq!(response["version"], 1, "{request}: {raw}");
        assert_eq!(response["error"]["kind"], kind, "{request}: {raw}");
        let message = response["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains(says), "{request}: {message}");
        assert!(response.get("exitCode").is_none(), "{request}: {raw}");
    }
}

/// A request that asks for no network and does not say best effort is run
/// with none or refused, never run with the host's: whichever this machine
/// does, the response says so.
#[test]
fn no_network_is_enforced_or_refused_never_dropped_quietly() {
    let mut request = shell("echo ran");
    request["network"] = json!({ "egress": "deny" });
    let (code, response, raw) = exec(&request.to_string());
    match code {
        Some(0) => {
            assert_eq!(response["unenforced"], json!([]), "{raw}");
            let enforced = response["controls"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|c| c["control"] == "network isolation" && c["enforced"] == true);
            assert!(enforced, "{raw}");
        }
        Some(2) => {
            assert_eq!(response["error"]["kind"], "unsupported", "{raw}");
            let message = response["error"]["message"].as_str().unwrap_or_default();
            assert!(message.contains("network isolation"), "{message}");
        }
        other => panic!("exit {other:?}: {raw}"),
    }
}
