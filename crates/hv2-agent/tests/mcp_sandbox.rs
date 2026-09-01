//! `sandbox.*` over the MCP tool surface.
//!
//! This is the first tool that runs a program on the machine the server runs
//! on, so most of what these tests pin is refusal: no sandbox installed, no
//! capability, confinement the host cannot provide. The one rule underneath
//! all of them is that the alternative to confinement is not running the
//! program unconfined — it is not running it.

use std::sync::Arc;

use hv2_agent::mcp::{AgentCapabilities, AgentCapability, AgentSession, McpServer};
use hv2_agent::sandbox_host::LocalSandboxHost;
use hv2_sandbox::{Controls, ProcessSandbox};
use serde_json::json;

fn session(server: &McpServer) -> Arc<AgentSession> {
    server
        .create_session("agent", AgentCapabilities::full())
        .unwrap()
}

async fn call(
    server: &McpServer,
    session: &AgentSession,
    tool: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = session.call_tool(server, tool, params).await;
    if response.success {
        Ok(response.result.unwrap_or(serde_json::Value::Null))
    } else {
        Err(response.error.unwrap_or_default())
    }
}

/// A server with a working sandbox installed.
fn server_with_sandbox() -> McpServer {
    let server = McpServer::new();
    server.set_sandbox_host(Arc::new(LocalSandboxHost::new()));
    server
}

/// A program that exists everywhere CI runs. The environment is empty by
/// design, so anything the workload needs to find its tools is handed to it.
fn echo_params(vm_extra: serde_json::Value) -> serde_json::Value {
    let mut params = if cfg!(windows) {
        json!({
            "program": "cmd.exe",
            "args": ["/C", "echo", "hello"],
            "env": { "SystemRoot": "C:\\Windows", "PATH": "C:\\Windows\\System32" }
        })
    } else {
        json!({ "program": "/bin/echo", "args": ["hello"] })
    };
    if let (Some(base), Some(extra)) = (params.as_object_mut(), vm_extra.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    params
}

#[tokio::test]
async fn the_tools_are_advertised_and_say_where_the_program_runs() {
    let server = McpServer::new();
    let tools = server.list_tools(&AgentCapabilities::full());

    let run = tools
        .iter()
        .find(|t| t.name == "sandbox.run")
        .expect("an agent that cannot discover the tool cannot use it");
    // Two tools now run programs. An agent picking between them has to be able
    // to tell which machine each one runs on.
    assert!(run.description.contains("host"), "got: {}", run.description);
    assert!(
        run.description.contains("vm.exec"),
        "should point at the in-guest tool: {}",
        run.description
    );
    assert!(tools.iter().any(|t| t.name == "sandbox.capabilities"));
}

#[tokio::test]
async fn without_a_sandbox_host_the_tool_refuses_rather_than_running_unconfined() {
    // The whole posture of this surface in one assertion.
    let server = McpServer::new();
    let session = session(&server);

    let err = call(&server, &session, "sandbox.run", echo_params(json!({})))
        .await
        .expect_err("no sandbox means no run");
    assert!(err.contains("no sandbox host"), "got: {err}");
    assert!(
        err.contains("not the fallback"),
        "the message should say why it did not just run it: {err}"
    );
}

#[tokio::test]
async fn running_a_program_on_the_host_requires_host_exec() {
    let server = server_with_sandbox();
    let limited = server
        .create_session("limited", AgentCapabilities::read_only())
        .unwrap();

    let err = call(&server, &limited, "sandbox.run", echo_params(json!({})))
        .await
        .expect_err("a read-only session must be refused");
    assert!(!err.is_empty());
}

#[tokio::test]
async fn admin_alone_does_not_grant_host_execution() {
    // Admin implies every other capability. Letting it imply this one would
    // have handed host execution to every existing admin session the moment
    // the tool shipped — a privilege expansion nobody wrote down.
    let mut admin_only = AgentCapabilities::none();
    admin_only.add(AgentCapability::Admin);
    assert!(admin_only.has(AgentCapability::VmManage));
    assert!(!admin_only.has(AgentCapability::HostExec));

    let mut explicit = AgentCapabilities::none();
    explicit.add(AgentCapability::HostExec);
    assert!(explicit.has(AgentCapability::HostExec));
}

#[tokio::test]
async fn capabilities_report_what_this_host_can_and_cannot_confine() {
    let server = server_with_sandbox();
    let session = session(&server);

    let caps = call(&server, &session, "sandbox.capabilities", json!({}))
        .await
        .expect("capabilities");

    assert_eq!(caps["backend"], json!("process"));
    assert!(caps["enforced"].is_array());
    // Every control this host cannot enforce carries a reason, so an operator
    // learns what to change rather than only that something failed.
    for (_, reason) in caps["unavailable"].as_object().expect("an object") {
        assert!(!reason.as_str().unwrap_or_default().is_empty());
    }
}

#[tokio::test]
async fn a_request_for_confinement_this_host_lacks_is_refused() {
    // A sandbox that claims nothing, so every strict request is refused. On a
    // host that genuinely enforces everything this test would otherwise pass
    // for the wrong reason.
    let server = McpServer::new();
    server.set_sandbox_host(Arc::new(LocalSandboxHost::with_sandbox(Arc::new(
        ProcessSandbox::with_controls(Controls::none()),
    ))));
    let session = session(&server);

    let err = call(&server, &session, "sandbox.run", echo_params(json!({})))
        .await
        .expect_err("confinement that cannot be provided must refuse");
    assert!(err.contains("cannot enforce"), "got: {err}");
}

#[tokio::test]
async fn best_effort_runs_and_reports_what_was_dropped() {
    let server = McpServer::new();
    server.set_sandbox_host(Arc::new(LocalSandboxHost::with_sandbox(Arc::new(
        ProcessSandbox::with_controls(Controls::none()),
    ))));
    let session = session(&server);

    let run = call(
        &server,
        &session,
        "sandbox.run",
        echo_params(json!({ "best_effort": true })),
    )
    .await
    .expect("best effort runs");

    let unenforced = run["unenforced"].as_array().expect("an array");
    assert!(
        !unenforced.is_empty(),
        "opting into best-effort must not mean opting out of knowing: {run}"
    );
}

#[tokio::test]
async fn a_program_runs_and_its_output_comes_back() {
    let server = server_with_sandbox();
    let session = session(&server);

    let run = call(
        &server,
        &session,
        "sandbox.run",
        echo_params(json!({ "best_effort": true })),
    )
    .await
    .expect("run");

    assert!(
        run["stdout"].as_str().unwrap_or_default().contains("hello"),
        "got: {run}"
    );
    assert_eq!(run["exit_code"], json!(0));
    assert_eq!(run["killed_by"], json!(null));
}

#[tokio::test]
async fn a_misspelled_field_is_refused_rather_than_ignored() {
    let server = server_with_sandbox();
    let session = session(&server);

    // Ignoring this would give the caller the strict default while it believed
    // it had asked for the network — the failure runs in the dangerous
    // direction just as easily as the safe one.
    let err = call(
        &server,
        &session,
        "sandbox.run",
        echo_params(json!({ "allow_netwrok": true })),
    )
    .await
    .expect_err("an unknown field is a request this surface does not understand");
    assert!(err.contains("invalid sandbox request"), "got: {err}");
}

#[tokio::test]
async fn a_request_with_no_program_is_refused() {
    let server = server_with_sandbox();
    let session = session(&server);

    let err = call(&server, &session, "sandbox.run", json!({}))
        .await
        .expect_err("there is no default program");
    assert!(!err.is_empty());
}
