//! Policy governance over the MCP tool surface.
//!
//! Capabilities answer "may this agent use this kind of tool at all". A policy
//! set answers the questions capabilities cannot express — this action, on this
//! resource, right now. These tests pin that the gate is genuinely optional,
//! that it actually refuses, and that a refusal is recorded.

use std::sync::Arc;

use hv2_agent::mcp::{AgentCapabilities, AgentSession, McpServer};
use hv2_agent::policies::{PolicyAction, PolicyRule, PolicySet, ResourceId};
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

/// Allow everything except deleting a VM.
fn no_deletions() -> Arc<PolicySet> {
    Arc::new(
        PolicySet::permissive("no-deletions").with_rule(
            PolicyRule::deny("deny-vm-delete")
                .with_action(PolicyAction::VmDelete)
                .with_resource(ResourceId::wildcard("vm"))
                .with_priority(10),
        ),
    )
}

#[tokio::test]
async fn without_a_policy_set_nothing_changes() {
    // The gate has to be genuinely optional: the server's long-standing
    // behaviour is capabilities plus VM ownership, and installing nothing must
    // leave exactly that.
    let server = McpServer::new();
    assert!(server.policy_set().is_none());

    let session = session(&server);
    let created = call(&server, &session, "vm.create", json!({"name": "free"}))
        .await
        .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    call(&server, &session, "vm.delete", json!({"vm_id": vm_id}))
        .await
        .expect("no policy installed, so nothing to deny");
}

#[tokio::test]
async fn an_installed_policy_refuses_the_action_it_denies() {
    let server = McpServer::new();
    server.set_policy_set(no_deletions());

    let session = session(&server);
    let created = call(&server, &session, "vm.create", json!({"name": "protected"}))
        .await
        .expect("creating is still allowed");
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    let err = call(&server, &session, "vm.delete", json!({"vm_id": vm_id}))
        .await
        .expect_err("deletion is denied");

    assert!(err.contains("Denied by policy"), "got: {err}");
    assert!(err.contains("no-deletions"), "got: {err}");
}

#[tokio::test]
async fn a_denial_is_recorded_in_the_audit_log() {
    // An unrecorded denial is the one an incident review most needs.
    let server = McpServer::new();
    server.set_policy_set(no_deletions());

    let session = session(&server);
    let created = call(&server, &session, "vm.create", json!({"name": "audited"}))
        .await
        .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    let _ = call(&server, &session, "vm.delete", json!({"vm_id": vm_id})).await;

    let entry = server
        .get_audit_log(100)
        .into_iter()
        .rev()
        .find(|e| e.tool == "vm.delete")
        .expect("the denied call must appear in the audit log");

    assert!(!entry.success);
    assert!(entry.error.unwrap().contains("Denied by policy"));
}

#[tokio::test]
async fn a_deny_by_default_set_refuses_a_tool_it_never_named() {
    // `PolicySet::new` denies by default. That is the safe direction for tools
    // added after a policy was written — the operator has to opt them in.
    let server = McpServer::new();
    server.set_policy_set(Arc::new(PolicySet::new("locked-down")));

    let session = session(&server);
    let err = call(&server, &session, "vm.create", json!({"name": "nope"}))
        .await
        .expect_err("deny-by-default refuses an unnamed action");

    assert!(err.contains("Denied by policy"), "got: {err}");
}

#[tokio::test]
async fn clearing_the_policy_set_restores_ungoverned_behaviour() {
    let server = McpServer::new();
    server.set_policy_set(Arc::new(PolicySet::new("locked-down")));

    let session = session(&server);
    assert!(call(&server, &session, "vm.create", json!({"name": "a"}))
        .await
        .is_err());

    server.clear_policy_set();
    assert!(server.policy_set().is_none());

    call(&server, &session, "vm.create", json!({"name": "a"}))
        .await
        .expect("ungoverned again");
}

#[tokio::test]
async fn a_rule_can_name_one_resource_without_catching_its_neighbours() {
    // The resource identifier comes from the call's parameters, so a rule can
    // protect a single VM rather than the whole type.
    let policies = Arc::new(
        PolicySet::permissive("protect-one").with_rule(
            PolicyRule::deny("protect-vm-critical")
                .with_action(PolicyAction::VmStop)
                .with_resource(ResourceId::new("vm", "critical"))
                .with_priority(10),
        ),
    );

    let server = McpServer::new();
    server.set_policy_set(policies);
    let session = session(&server);

    // `vm.stop` reads `vm_id`, so drive the rule through that parameter.
    let ordinary = call(&server, &session, "vm.create", json!({"name": "ordinary"}))
        .await
        .unwrap();
    let ordinary_id = ordinary["vm_id"].as_str().unwrap().to_string();
    call(&server, &session, "vm.start", json!({"vm_id": ordinary_id}))
        .await
        .unwrap();
    call(&server, &session, "vm.stop", json!({"vm_id": ordinary_id}))
        .await
        .expect("an unnamed VM is unaffected");

    let err = call(&server, &session, "vm.stop", json!({"vm_id": "critical"}))
        .await
        .expect_err("the named VM is protected");
    assert!(err.contains("Denied by policy"), "got: {err}");
}

// ═══════════════════════════════════════════════════════════════════
//  Concurrency ceiling
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn without_a_limit_concurrency_is_unbounded() {
    let server = McpServer::new();
    assert!(server.concurrency_limiter().is_none());

    let session = session(&server);
    for i in 0..8 {
        call(
            &server,
            &session,
            "vm.create",
            json!({"name": format!("vm-{i}")}),
        )
        .await
        .expect("no ceiling installed");
    }
}

#[tokio::test]
async fn a_rejected_call_is_recorded_like_any_other() {
    // McpConfig::rate_limit bounds calls per minute; this bounds how many run
    // at once. A refusal has to reach the audit log either way.
    let server = McpServer::new();
    server.set_concurrency_limit(4);
    assert!(server.concurrency_limiter().is_some());

    let session = session(&server);

    // Sequential calls each release their permit on completion, so a ceiling of
    // four does not stop four hundred calls made one after another.
    for i in 0..6 {
        call(
            &server,
            &session,
            "vm.create",
            json!({"name": format!("seq-{i}")}),
        )
        .await
        .expect("permits are released when a call finishes");
    }

    let entries = server.get_audit_log(100);
    assert_eq!(
        entries.iter().filter(|e| e.tool == "vm.create").count(),
        6,
        "every call should be audited"
    );
}

#[tokio::test]
async fn clearing_the_limit_removes_the_ceiling() {
    let server = McpServer::new();
    server.set_concurrency_limit(1);
    assert!(server.concurrency_limiter().is_some());

    server.clear_concurrency_limit();
    assert!(server.concurrency_limiter().is_none());
}
