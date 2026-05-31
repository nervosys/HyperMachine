//! Integration tests for the MCP tool surface used by agents to run VM
//! workloads. These lock in the contract that the *published* tool schema
//! (what an LLM sees) matches what the handler actually reads — a mismatch
//! would silently give agents wrong results (e.g. a default-sized VM).

use hv2_agent::{AgentCapabilities, McpServer};
use serde_json::json;

/// `vm.create` must honor the `cpu_cores` / `memory_gb` parameters exactly as
/// advertised in its schema (regression guard against the old `cpus`/`memory_mb`
/// handler names that were silently ignored).
#[tokio::test]
async fn vm_create_honors_advertised_params() {
    let server = McpServer::new();
    let session = server
        .create_session("test-agent", AgentCapabilities::full())
        .unwrap();

    let resp = session
        .call_tool(
            &server,
            "vm.create",
            json!({ "name": "sized", "cpu_cores": 8, "memory_gb": 32 }),
        )
        .await;

    assert!(resp.success, "vm.create failed: {:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result["cpu_cores"], 8, "cpu_cores not applied");
    assert_eq!(result["memory_gb"], 32, "memory_gb not applied");

    // The same values must be observable via vm.status.
    let vm_id = result["vm_id"].as_str().unwrap();
    let status = session
        .call_tool(&server, "vm.status", json!({ "vm_id": vm_id }))
        .await;
    assert!(status.success);
    let s = status.result.unwrap();
    assert_eq!(s["cpu_cores"], 8);
    assert_eq!(s["memory_gb"], 32);
}

/// Every required parameter named in a tool's JSON schema must be a parameter
/// the dispatcher accepts. We assert it for the common-workload tools by
/// driving the full lifecycle and checking each call succeeds.
#[tokio::test]
async fn full_workload_lifecycle_succeeds() {
    let server = McpServer::new();
    let session = server
        .create_session("test-agent", AgentCapabilities::full())
        .unwrap();

    let vm_id = session
        .call_tool(
            &server,
            "vm.create",
            json!({ "name": "wl", "cpu_cores": 4, "memory_gb": 8, "gpu_enabled": true }),
        )
        .await
        .result
        .unwrap()["vm_id"]
        .as_str()
        .unwrap()
        .to_string();

    for (tool, params) in [
        ("vm.start", json!({ "vm_id": vm_id })),
        ("vm.metrics", json!({ "vm_id": vm_id })),
        ("vm.status", json!({ "vm_id": vm_id })),
        (
            "vm.resize",
            json!({ "vm_id": vm_id, "cpu_cores": 8, "memory_gb": 16 }),
        ),
    ] {
        let r = session.call_tool(&server, tool, params).await;
        assert!(r.success, "{tool} failed: {:?}", r.error);
    }

    // Snapshot create -> restore by the returned id.
    let snap = session
        .call_tool(
            &server,
            "snapshot.create",
            json!({ "vm_id": vm_id, "snapshot_name": "s1" }),
        )
        .await;
    assert!(snap.success, "snapshot.create failed: {:?}", snap.error);
    let snapshot_id = snap.result.unwrap()["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();

    let restore = session
        .call_tool(
            &server,
            "snapshot.restore",
            json!({ "snapshot_id": snapshot_id }),
        )
        .await;
    assert!(
        restore.success,
        "snapshot.restore failed: {:?}",
        restore.error
    );

    for (tool, params) in [
        ("vm.stop", json!({ "vm_id": vm_id })),
        ("vm.delete", json!({ "vm_id": vm_id })),
    ] {
        let r = session.call_tool(&server, tool, params).await;
        assert!(r.success, "{tool} failed: {:?}", r.error);
    }

    // All actions were audited.
    assert!(server.get_audit_log(100).len() >= 9);
}

/// A guest command can be submitted, the result delivered by the guest-agent
/// channel, and then polled to completion via `guest.exec.status`.
#[tokio::test]
async fn guest_exec_round_trip() {
    let server = McpServer::new();
    let session = server
        .create_session("test-agent", AgentCapabilities::full())
        .unwrap();

    let vm_id = session
        .call_tool(
            &server,
            "vm.create",
            json!({ "name": "g", "cpu_cores": 2, "memory_gb": 4 }),
        )
        .await
        .result
        .unwrap()["vm_id"]
        .as_str()
        .unwrap()
        .to_string();
    session
        .call_tool(&server, "vm.start", json!({ "vm_id": vm_id }))
        .await;
    session.set_state(
        &format!("guest_agent:{vm_id}"),
        json!({ "connected": true }),
    );

    // Submit a command -> get a request_id, initially pending.
    let submitted = session
        .call_tool(
            &server,
            "guest.exec",
            json!({ "vm_id": vm_id, "command": "echo", "args": ["hi"] }),
        )
        .await;
    assert!(submitted.success);
    let request_id = submitted.result.unwrap()["request_id"]
        .as_str()
        .unwrap()
        .to_string();

    let pending = session
        .call_tool(
            &server,
            "guest.exec.status",
            json!({ "request_id": request_id }),
        )
        .await;
    assert_eq!(pending.result.unwrap()["status"], "pending");

    // Guest agent delivers the result; status now reports completion.
    session.deliver_guest_response(
        &request_id,
        json!({ "exit_code": 0, "stdout": "hi\n", "stderr": "" }),
    );
    let done = session
        .call_tool(
            &server,
            "guest.exec.status",
            json!({ "request_id": request_id }),
        )
        .await;
    assert!(done.success);
    let r = done.result.unwrap();
    assert_eq!(r["status"], "completed");
    assert_eq!(r["exit_code"], 0);
    assert_eq!(r["stdout"], "hi\n");

    // An unknown request id is an error.
    let unknown = session
        .call_tool(
            &server,
            "guest.exec.status",
            json!({ "request_id": "nope" }),
        )
        .await;
    assert!(!unknown.success);
}
