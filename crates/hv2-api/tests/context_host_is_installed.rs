//! A deployment gets a session record without anyone writing code.
//!
//! `McpServer` advertises seven `context.*` tools and dispatches them against
//! an installed host. Nothing in the shipped server used to install one, so
//! every one of them refused in production while every unit test passed --
//! the tools were correct and the product was useless. These tests fail if
//! that regresses.
//!
//! Both live in one test binary on purpose: the first mutates the process
//! environment, which is shared by every thread in the binary, so nothing
//! else may be running alongside it.

use hv2_agent::AgentCapabilities;
use hv2_api::agent_runtime_routes::{AgentRuntimeAppState, ContextConfig};
use serde_json::json;

const BASELINE: usize = 4096;

/// Call a tool the way an agent does, returning the tool's own error text.
async fn call(
    state: &AgentRuntimeAppState,
    tool: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let handle = state
        .runtime
        .spawn_agent("agent", AgentCapabilities::full())
        .unwrap();
    let response = handle
        .session
        .call_tool(state.runtime.server(), tool, params)
        .await;
    if response.success {
        Ok(response.result.unwrap_or(serde_json::Value::Null))
    } else {
        Err(response.error.unwrap_or_default())
    }
}

#[tokio::test]
async fn a_server_built_the_normal_way_has_a_session_record() {
    // The regression that matters: an operator who runs the server as shipped
    // must not have to install a host for the memory surface to exist.
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("HV2_CONTEXT_ROOT", dir.path());
    std::env::set_var("HV2_CONTEXT_SESSION", "operator-session");

    let state = AgentRuntimeAppState::new(&vec![0u8; BASELINE]);
    let status = call(&state, "context.status", json!({})).await.unwrap();

    assert_eq!(
        status.get("session").and_then(|v| v.as_str()),
        Some("operator-session"),
        "status came back for the wrong session: {status}"
    );

    // And the record is durable, not an in-memory stand-in that would accept
    // every write and lose it.
    call(
        &state,
        "context.record",
        json!({ "role": "system", "kind": "note", "text": "a thing worth keeping" }),
    )
    .await
    .unwrap();
    let reopened = AgentRuntimeAppState::new(&vec![0u8; BASELINE]);
    let hits = call(
        &reopened,
        "context.search",
        json!({ "query": "worth keeping" }),
    )
    .await
    .unwrap();
    assert!(
        !hits.as_array().unwrap().is_empty(),
        "a record written before the restart was not found after it: {hits}"
    );

    std::env::remove_var("HV2_CONTEXT_ROOT");
    std::env::remove_var("HV2_CONTEXT_SESSION");
}

#[tokio::test]
async fn a_deployment_that_wants_no_record_gets_tools_that_refuse() {
    // Opting out has to mean refusal, not a silent stub: an agent told its
    // note was saved, with nothing on disk, is the failure this whole design
    // exists to avoid.
    let state = AgentRuntimeAppState::with_context_config(
        &vec![0u8; BASELINE],
        &ContextConfig {
            root: None,
            ..ContextConfig::default()
        },
    );
    let err = call(&state, "context.status", json!({})).await.unwrap_err();
    assert!(err.contains("no context host"), "got: {err}");
}
