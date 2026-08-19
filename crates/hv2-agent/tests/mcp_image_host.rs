//! The `image.*` tools, backed by a real allowlist.
//!
//! Admission is enforced at `VM::provision` regardless of these tools. They
//! exist so an agent can ask what it may boot *before* composing a plan around
//! an image, rather than discovering the answer when the VM refuses to start.

use std::sync::Arc;

use hv2_agent::image_host::RegistryImageHost;
use hv2_agent::mcp::{AgentCapabilities, AgentSession, McpServer};
use hv2_core::security::image_registry::{
    EnforcementMode, ImageEntry, ImageKind, ImageRegistry, RegistryConfig,
};
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

/// A registry that enforces but does not demand signatures.
fn registry() -> Arc<ImageRegistry> {
    Arc::new(ImageRegistry::new(RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: false,
        trusted_signers: Vec::new(),
    }))
}

#[tokio::test]
async fn without_a_host_the_tools_say_so() {
    // Not an empty list: an agent must be able to tell "no images are approved"
    // from "nobody is tracking images at all".
    let server = McpServer::new();
    assert!(server.image_host().is_none());

    let session = session(&server);
    let err = call(&server, &session, "image.list", json!({}))
        .await
        .expect_err("no registry installed");
    assert!(err.contains("No image registry"), "got: {err}");
}

#[tokio::test]
async fn listing_reports_status_and_is_ordered() {
    let reg = registry();
    reg.register(ImageEntry::new("b/kernel:2", ImageKind::Kernel, "bbb"))
        .unwrap();
    reg.register(ImageEntry::new("a/kernel:1", ImageKind::Kernel, "aaa"))
        .unwrap();
    reg.approve("a/kernel:1", "reviewer").unwrap();

    let server = McpServer::new();
    server.set_image_host(Arc::new(RegistryImageHost::new(reg)));
    let session = session(&server);

    let out = call(&server, &session, "image.list", json!({}))
        .await
        .unwrap();
    assert_eq!(out["total"], 2);

    // Sorted, because the registry is a HashMap and an agent diffing two
    // listings should not see changes that are only hash order.
    assert_eq!(out["images"][0]["reference"], "a/kernel:1");
    assert_eq!(out["images"][0]["status"], "approved");
    assert_eq!(out["images"][1]["reference"], "b/kernel:2");
    assert_eq!(out["images"][1]["status"], "pending_review");
}

#[tokio::test]
async fn check_answers_the_same_question_provision_asks() {
    let reg = registry();
    reg.register(ImageEntry::new(
        "ok/kernel:1",
        ImageKind::Kernel,
        "cafebabe",
    ))
    .unwrap();
    reg.approve("ok/kernel:1", "reviewer").unwrap();

    let server = McpServer::new();
    server.set_image_host(Arc::new(RegistryImageHost::new(Arc::clone(&reg))));
    let session = session(&server);

    let ok = call(
        &server,
        &session,
        "image.check",
        json!({"sha256": "cafebabe"}),
    )
    .await
    .unwrap();
    assert_eq!(ok["admitted"], true);

    let unknown = call(
        &server,
        &session,
        "image.check",
        json!({"sha256": "deadbeef"}),
    )
    .await
    .unwrap();
    assert_eq!(unknown["admitted"], false);
    assert!(unknown["reason"].as_str().unwrap().contains("deadbeef"));
}

#[tokio::test]
async fn a_revoked_image_is_reported_as_inadmissible() {
    // The case that matters: an agent should be able to see that an image it
    // used yesterday is no longer bootable, without trying to boot it.
    let reg = registry();
    reg.register(ImageEntry::new(
        "old/kernel:1",
        ImageKind::Kernel,
        "feedface",
    ))
    .unwrap();
    reg.approve("old/kernel:1", "reviewer").unwrap();
    reg.revoke("old/kernel:1", "reviewer", "CVE-2026-0001")
        .unwrap();

    let server = McpServer::new();
    server.set_image_host(Arc::new(RegistryImageHost::new(reg)));
    let session = session(&server);

    let verdict = call(
        &server,
        &session,
        "image.check",
        json!({"sha256": "feedface"}),
    )
    .await
    .unwrap();
    assert_eq!(verdict["admitted"], false);
    assert!(verdict["reason"].as_str().unwrap().contains("revoked"));

    let image = call(
        &server,
        &session,
        "image.get",
        json!({"reference": "old/kernel:1"}),
    )
    .await
    .unwrap();
    assert_eq!(image["status"], "revoked");
    assert!(image["notes"].as_str().unwrap().contains("CVE-2026-0001"));
}

#[tokio::test]
async fn a_missing_parameter_is_reported_not_guessed() {
    let server = McpServer::new();
    server.set_image_host(Arc::new(RegistryImageHost::new(registry())));
    let session = session(&server);

    let err = call(&server, &session, "image.get", json!({}))
        .await
        .expect_err("reference is required");
    assert!(err.contains("reference"), "got: {err}");
}
