//! `vm.console` over the MCP tool surface.
//!
//! An agent that boots a guest and then has to reason about what happened
//! needs the console. The thing these tests pin is the distinction the tool
//! exists to preserve: "no console attached" and "console attached, guest
//! silent" both carry an empty log, and an agent that cannot tell them apart
//! will go looking for a boot failure that is really a missing serial device.

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
    call(server, session, "vm.create", json!({"name": "console-vm"}))
        .await
        .unwrap()["vm_id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn the_tool_is_advertised() {
    let server = McpServer::new();
    let names: Vec<String> = server
        .list_tools(&AgentCapabilities::full())
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(
        names.contains(&"vm.console".to_string()),
        "an agent that cannot discover the tool cannot use it"
    );
}

#[tokio::test]
async fn without_a_host_the_console_reports_nothing_attached() {
    // The hostless server is a state machine over JSON records; there is no
    // guest, so there is no console. Reporting `attached: false` says that,
    // where an empty string alone would read as a guest that booted silently.
    let server = McpServer::new();
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    let console = call(&server, &session, "vm.console", json!({"vm_id": vm_id}))
        .await
        .unwrap();

    assert_eq!(console["attached"], json!(false));
    assert_eq!(console["output"], json!(""));
}

#[tokio::test]
async fn an_unknown_vm_is_an_error_not_an_empty_console() {
    let server = McpServer::new();
    let session = session(&server);

    let err = call(&server, &session, "vm.console", json!({"vm_id": "ghost"}))
        .await
        .expect_err("a typo must not look like a quiet guest");
    assert!(err.contains("not found"), "got: {err}");
}

#[tokio::test]
async fn a_missing_vm_id_is_rejected() {
    let server = McpServer::new();
    let session = session(&server);

    assert!(call(&server, &session, "vm.console", json!({}))
        .await
        .is_err());
}

#[tokio::test]
async fn one_session_cannot_read_another_session_s_console() {
    // Console output is guest output. Ownership is checked before dispatch for
    // every other `vm.*` tool; this one leaks a boot log if it is not.
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));

    let owner = session(&server);
    let vm_id = create_vm(&server, &owner).await;

    let stranger = server
        .create_session("other-agent", AgentCapabilities::full())
        .unwrap();

    let err = call(&server, &stranger, "vm.console", json!({"vm_id": vm_id}))
        .await
        .expect_err("a session must not read a VM it does not own");
    assert!(!err.is_empty());
}

#[tokio::test]
async fn a_created_but_unstarted_vm_has_no_console() {
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));
    let session = session(&server);
    let vm_id = create_vm(&server, &session).await;

    let console = call(&server, &session, "vm.console", json!({"vm_id": vm_id}))
        .await
        .unwrap();

    assert_eq!(console["vm_id"], json!(vm_id));
    assert_eq!(
        console["attached"],
        json!(false),
        "nothing is running, so there are no devices to have a console"
    );
    assert_eq!(console["output"], json!(""));
}

#[tokio::test]
async fn reading_the_console_requires_the_vm_read_capability() {
    let server = McpServer::new();
    let full = session(&server);
    let vm_id = create_vm(&server, &full).await;

    // A second agent holding no capabilities at all.
    let limited = server
        .create_session("limited", AgentCapabilities::none())
        .unwrap();

    let err = call(&server, &limited, "vm.console", json!({"vm_id": vm_id}))
        .await
        .expect_err("a capability-less session must be refused");
    assert!(!err.is_empty());
}
