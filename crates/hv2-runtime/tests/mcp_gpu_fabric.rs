//! The MCP `gpu.*` tools driving the real GPU fabric.
//!
//! This is the end the dependency inversion exists for: the `GpuHost` trait
//! lives in `hv2-agent`, the topology-backed implementation lives here, and
//! this test is the only place both are visible at once — so it is where the
//! seam is verified.

use std::sync::Arc;

use hv2_agent::mcp::{AgentCapabilities, McpServer};
use hv2_runtime::{AgentGpuHost, GpuDevice, GpuTopologyMap};
use serde_json::json;

const GB: u64 = 1024 * 1024 * 1024;

fn fleet() -> AgentGpuHost {
    let mut topology = GpuTopologyMap::new();
    topology.add_device(GpuDevice::new("gpu-0", "node-1", "H100").vram(80 * GB));
    topology.add_device(GpuDevice::new("gpu-1", "node-1", "H100").vram(80 * GB));
    topology.add_device(GpuDevice::new("gpu-2", "node-2", "A100").vram(40 * GB));
    AgentGpuHost::new(topology)
}

/// Create a session and a VM it owns, returning the session and the VM id.
async fn session_with_vm(
    server: &McpServer,
    agent: &str,
) -> (Arc<hv2_agent::mcp::AgentSession>, String) {
    let session = server
        .create_session(agent, AgentCapabilities::full())
        .unwrap();
    let response = session
        .call_tool(server, "vm.create", json!({"name": agent}))
        .await;
    let vm_id = response.result.unwrap()["vm_id"]
        .as_str()
        .unwrap()
        .to_string();
    (session, vm_id)
}

#[tokio::test]
async fn gpu_list_enumerates_the_real_fleet() {
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host);

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let response = session.call_tool(&server, "gpu.list", json!({})).await;
    assert!(response.success, "{:?}", response.error);

    let result = response.result.unwrap();
    assert_eq!(
        result["total"], 3,
        "the agent should see the fleet, not its own empty session"
    );
    assert_eq!(result["available"], 3);
    assert_eq!(result["devices"][0]["model"], "H100");
    assert_eq!(result["devices"][2]["vram_gb"], 40);
}

#[tokio::test]
async fn attaching_a_gpu_claims_it_from_the_placement_pool() {
    // Without this, the scheduler would keep offering a GPU an agent has
    // already taken — the exact failure the in-memory tools could not prevent.
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host.clone());

    let (session, vm_id) = session_with_vm(&server, "claimer").await;

    let response = session
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_id, "device_id": "gpu-0"}),
        )
        .await;
    assert!(response.success, "{:?}", response.error);

    assert_eq!(
        host.with_topology(|t| t.available_count()),
        2,
        "the fabric must see the device as allocated"
    );
}

#[tokio::test]
async fn two_sessions_cannot_hold_the_same_gpu() {
    // A physical device attached to one VM is unavailable to every other VM in
    // the fleet — across sessions, which per-session bookkeeping cannot model.
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host.clone());

    let (session_a, vm_a) = session_with_vm(&server, "agent-a").await;
    let (session_b, vm_b) = session_with_vm(&server, "agent-b").await;

    let first = session_a
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_a, "device_id": "gpu-0"}),
        )
        .await;
    assert!(first.success);

    let second = session_b
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_b, "device_id": "gpu-0"}),
        )
        .await;
    assert!(
        !second.success,
        "a second session must not be able to take an allocated GPU"
    );
    assert!(
        second.error.unwrap().contains("already allocated"),
        "the refusal should say why"
    );
}

#[tokio::test]
async fn detaching_returns_the_gpu_to_the_fleet() {
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host.clone());

    let (session, vm_id) = session_with_vm(&server, "borrower").await;

    session
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_id, "device_id": "gpu-1"}),
        )
        .await;
    let response = session
        .call_tool(
            &server,
            "gpu.detach",
            json!({"vm_id": vm_id, "device_id": "gpu-1"}),
        )
        .await;

    assert!(response.success, "{:?}", response.error);
    assert_eq!(host.with_topology(|t| t.available_count()), 3);
}

#[tokio::test]
async fn a_session_cannot_attach_a_gpu_to_another_sessions_vm() {
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host.clone());

    let (_owner, vm_id) = session_with_vm(&server, "owner").await;
    let intruder = server
        .create_session("intruder", AgentCapabilities::full())
        .unwrap();

    let response = intruder
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_id, "device_id": "gpu-0"}),
        )
        .await;

    assert!(!response.success, "ownership must be enforced");
    assert_eq!(
        host.with_topology(|t| t.available_count()),
        3,
        "a refused attach must not consume a device"
    );
}

#[tokio::test]
async fn only_available_filters_out_allocated_devices() {
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host);

    let (session, vm_id) = session_with_vm(&server, "filterer").await;
    session
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_id, "device_id": "gpu-0"}),
        )
        .await;

    let result = session
        .call_tool(&server, "gpu.list", json!({"only_available": true}))
        .await
        .result
        .unwrap();

    assert_eq!(result["total"], 2);
    let ids: Vec<&str> = result["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["device_id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&"gpu-0"));
}

#[tokio::test]
async fn an_attached_gpu_reports_its_owner() {
    let host = Arc::new(fleet());
    let server = McpServer::new();
    server.set_gpu_host(host);

    let (session, vm_id) = session_with_vm(&server, "owner-report").await;
    session
        .call_tool(
            &server,
            "gpu.attach",
            json!({"vm_id": vm_id, "device_id": "gpu-2"}),
        )
        .await;

    let result = session
        .call_tool(&server, "gpu.list", json!({}))
        .await
        .result
        .unwrap();

    let gpu2 = result["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_id"] == "gpu-2")
        .unwrap();
    assert_eq!(gpu2["allocated_to"], vm_id);
}

#[tokio::test]
async fn without_a_gpu_host_the_tools_keep_their_own_records() {
    // The default behaviour must survive: agent logic stays testable with no
    // fabric present.
    let server = McpServer::new();
    assert!(server.gpu_host().is_none());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let registered = session
        .call_tool(
            &server,
            "gpu.register",
            json!({"device_id": "local-0", "model": "H100"}),
        )
        .await;
    assert!(registered.success);

    let listed = session
        .call_tool(&server, "gpu.list", json!({}))
        .await
        .result
        .unwrap();
    assert_eq!(listed["total"], 1);
}
