//! `vm.exec` over the MCP tool surface.
//!
//! The tool that finally makes "run a command in the VM" a thing an agent can
//! ask for. What these tests pin is mostly what it refuses: every way of not
//! having a guest to run in has to be reported as itself, because an agent
//! that receives a timeout when the real problem was a missing device will
//! retry forever, and one that receives an empty success will believe a
//! command ran that never did.

use std::sync::Arc;

use hv2_agent::mcp::{AgentCapabilities, AgentSession, McpServer};
use hv2_agent::vm_host::LocalVmHost;
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

async fn create_vm(server: &McpServer, session: &AgentSession) -> String {
    call(server, session, "vm.create", json!({"name": "exec-vm"}))
        .await
        .unwrap()["vm_id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn the_tool_is_advertised_and_says_how_it_differs_from_the_script_tool() {
    let server = McpServer::new();
    let tools = server.list_tools(&AgentCapabilities::full());
    let exec = tools
        .iter()
        .find(|t| t.name == "vm.exec")
        .expect("an agent that cannot discover the tool cannot use it");

    // The whole reason this tool exists is that another one was described as
    // doing what it does. The description has to keep them apart, or an agent
    // picks the wrong one for the same reason a reader did.
    assert!(
        exec.description.contains("guest"),
        "description should say it runs in the guest: {}",
        exec.description
    );
    assert!(
        exec.description.contains("execute_script"),
        "description should distinguish it from the host-side script tool: {}",
        exec.description
    );
}

#[tokio::test]
async fn without_a_host_there_is_no_guest_and_the_tool_says_so() {
    // The hostless server is a state machine over JSON records. An empty
    // successful result here would be a fabricated measurement, which is the
    // defect execute_plan had before it was fixed.
    let server = McpServer::new();
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    let err = call(
        &server,
        &session,
        "vm.exec",
        json!({"vm_id": vm_id, "program": "uname"}),
    )
    .await
    .expect_err("no host means no guest");
    assert!(err.contains("no VM host"), "got: {err}");
}

#[tokio::test]
async fn an_unknown_vm_is_an_error_rather_than_a_command_that_ran_nowhere() {
    let server = McpServer::new();
    let session = session(&server);

    let err = call(
        &server,
        &session,
        "vm.exec",
        json!({"vm_id": "ghost", "program": "uname"}),
    )
    .await
    .expect_err("an unknown VM is not a place to run a command");
    assert!(
        err.contains("ghost") || err.contains("not found"),
        "got: {err}"
    );
}

#[tokio::test]
async fn a_created_but_unstarted_vm_has_no_guest_to_run_in() {
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    // "Not started" is a different problem from "no agent answered", and an
    // operator sent to the wrong one wastes the debugging session.
    let err = call(
        &server,
        &session,
        "vm.exec",
        json!({"vm_id": vm_id, "program": "uname", "timeout_seconds": 1}),
    )
    .await
    .expect_err("an unstarted VM has no guest");
    assert!(
        err.contains("not been started") || err.contains("guest channel"),
        "got: {err}"
    );
}

#[tokio::test]
async fn one_session_cannot_run_a_command_in_another_session_s_vm() {
    // Ownership matters more here than for any other vm.* tool: this one runs
    // code inside someone else's guest.
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));

    let owner = session(&server);
    let vm_id = create_vm(&server, &owner).await;

    let stranger = server
        .create_session("other-agent", AgentCapabilities::full())
        .unwrap();

    let err = call(
        &server,
        &stranger,
        "vm.exec",
        json!({"vm_id": vm_id, "program": "uname"}),
    )
    .await
    .expect_err("a session must not run commands in a VM it does not own");
    assert!(!err.is_empty());
}

#[tokio::test]
async fn running_a_command_requires_the_guest_exec_capability() {
    let server = McpServer::new();
    let full = session(&server);
    let vm_id = create_vm(&server, &full).await;

    let limited = server
        .create_session("limited", AgentCapabilities::none())
        .unwrap();

    let err = call(
        &server,
        &limited,
        "vm.exec",
        json!({"vm_id": vm_id, "program": "uname"}),
    )
    .await
    .expect_err("a capability-less session must be refused");
    assert!(!err.is_empty());
}

#[tokio::test]
async fn a_missing_program_is_refused_before_anything_is_dispatched() {
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    let err = call(&server, &session, "vm.exec", json!({"vm_id": vm_id}))
        .await
        .expect_err("there is no default program");
    assert!(err.contains("program"), "got: {err}");
}

#[tokio::test]
async fn args_must_be_strings_rather_than_whatever_arrived() {
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    // A number here would otherwise be stringified somewhere downstream, and
    // the caller would never learn its argument was not what it wrote.
    let err = call(
        &server,
        &session,
        "vm.exec",
        json!({"vm_id": vm_id, "program": "echo", "args": [1, 2]}),
    )
    .await
    .expect_err("args are strings");
    assert!(err.contains("array of strings"), "got: {err}");
}

#[tokio::test]
async fn a_zero_timeout_is_refused_rather_than_meaning_forever() {
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    let err = call(
        &server,
        &session,
        "vm.exec",
        json!({"vm_id": vm_id, "program": "sleep", "timeout_seconds": 0}),
    )
    .await
    .expect_err("zero is not a deadline");
    assert!(err.contains("timeout_seconds"), "got: {err}");
}
