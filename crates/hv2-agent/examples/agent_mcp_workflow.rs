//! Example: an AI agent driving a complete VM workload via MCP tools.
//!
//! This is the control loop HyperMachine is built for. An agent is given a
//! capability-scoped session, discovers the typed tools available to it, and
//! then provisions, boots, runs a workload, snapshots, scales, restores, and
//! tears down a VM — entirely through structured tool calls. Every call is
//! recorded in a tamper-evident audit log.
//!
//! These are the exact tools an LLM drives through the OpenAI / Anthropic /
//! Gemini tool-use formats (see the `llm_tool_schemas` example for how the
//! same surface is exported to each provider).
//!
//! Run with:
//! ```bash
//! cargo run -p hv2-agent --example agent_mcp_workflow
//! ```

use hv2_agent::{AgentCapabilities, McpServer};
use serde_json::{json, Value};

/// Invoke an MCP tool, print the call, and assert it succeeded.
///
/// Written as a macro so it expands inline in the async `main` (keeping the
/// `.await` in scope) without needing to name internal session types.
macro_rules! call {
    ($server:expr, $session:expr, $tool:expr, $params:expr) => {{
        let response = $session.call_tool(&$server, $tool, $params).await;
        assert!(
            response.success,
            "tool `{}` failed: {:?}",
            $tool, response.error
        );
        let result = response.result.unwrap_or(Value::Null);
        println!("  → {:<16} {}", $tool, result);
        result
    }};
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🤖 HyperMachine — agent-driven VM workload over MCP\n{}",
        "=".repeat(60)
    );

    // The MCP server hosts the tool registry and the audit log.
    let server = McpServer::new();

    // The agent is issued a *capability-scoped* session. `full()` permits guest
    // execution and networking in addition to VM management; a read-only monitor
    // agent would get `AgentCapabilities::read_only()` and never see write tools.
    let session = server
        .create_session("workload-agent", AgentCapabilities::full())
        .map_err(|e| format!("failed to create session: {e}"))?;

    // 1. Discovery — exactly the tool list an LLM receives up front.
    let tools = session.list_tools(&server);
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    println!(
        "\n[discovery] {} tools available to this agent:",
        tools.len()
    );
    println!("  {}", names.join(", "));

    // 2. Provision a GPU-backed sandbox.
    println!("\n[provision]");
    let created = call!(
        server,
        session,
        "vm.create",
        json!({ "name": "ml-sandbox", "cpu_cores": 8, "memory_gb": 32, "gpu_enabled": true })
    );
    let vm_id = created["vm_id"]
        .as_str()
        .expect("vm.create returns a vm_id")
        .to_string();

    // 3. Boot it.
    println!("\n[boot]");
    call!(server, session, "vm.start", json!({ "vm_id": vm_id }));

    // 4. Run a workload in the guest. In a real deployment the guest agent is
    //    installed in the image; here we mark its channel connected so the call
    //    is dispatched rather than rejected.
    println!("\n[run workload]");
    session.set_state(
        &format!("guest_agent:{vm_id}"),
        json!({ "connected": true }),
    );
    call!(
        server,
        session,
        "guest.exec",
        json!({ "vm_id": vm_id, "command": "python", "args": ["train.py", "--epochs", "1"] })
    );

    // 5. Observe.
    println!("\n[observe]");
    call!(server, session, "vm.metrics", json!({ "vm_id": vm_id }));
    call!(server, session, "vm.status", json!({ "vm_id": vm_id }));

    // 6. Snapshot before a risky change.
    println!("\n[snapshot]");
    let snap = call!(
        server,
        session,
        "snapshot.create",
        json!({ "vm_id": vm_id, "snapshot_name": "pre-scale" })
    );
    let snapshot_id = snap["snapshot_id"]
        .as_str()
        .expect("snapshot.create returns a snapshot_id")
        .to_string();

    // 7. Scale up, then roll back to the snapshot — demonstrating safe,
    //    reversible agent-driven changes.
    println!("\n[scale + rollback]");
    call!(
        server,
        session,
        "vm.resize",
        json!({ "vm_id": vm_id, "cpu_cores": 16, "memory_gb": 64 })
    );
    call!(
        server,
        session,
        "snapshot.restore",
        json!({ "snapshot_id": snapshot_id })
    );

    // 8. Tear down.
    println!("\n[teardown]");
    call!(server, session, "vm.stop", json!({ "vm_id": vm_id }));
    call!(server, session, "vm.delete", json!({ "vm_id": vm_id }));

    // 9. Audit trail — every action the agent took, in order.
    let audit = server.get_audit_log(100);
    println!("\n[audit] {} recorded actions:", audit.len());
    for entry in &audit {
        println!(
            "  {:<14} {:<16} ok={}",
            entry.agent_id, entry.tool, entry.success
        );
    }

    println!("\n✅ workload complete — provisioned, ran, snapshotted, scaled, rolled back, and cleaned up.");
    Ok(())
}
