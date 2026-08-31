//! The MCP tool surface, backed by a real VM host.
//!
//! Without a host the `vm.*` tools keep their own session records. With one
//! installed the same calls reach a [`VmHost`] — these tests pin both halves of
//! that contract, including the isolation the server has to enforce once a host
//! is shared between sessions.

use std::sync::Arc;

use async_trait::async_trait;
use hv2_agent::mcp::{AgentCapabilities, AgentSession, McpServer};
use hv2_agent::vm_host::{VmDescriptor, VmHost, VmSpec};
use parking_lot::Mutex;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════
//  A host that records the calls it receives
// ═══════════════════════════════════════════════════════════════════

#[derive(Default)]
struct RecordingHost {
    calls: Mutex<Vec<String>>,
    vms: Mutex<Vec<VmDescriptor>>,
    /// The last spec `vm.create` handed over. A descriptor does not report
    /// everything a spec carries, so this is the only way to see what the tool
    /// surface actually asked for.
    specs: Mutex<Vec<VmSpec>>,
}

impl RecordingHost {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }

    fn last_spec(&self) -> VmSpec {
        self.specs.lock().last().cloned().expect("a spec")
    }

    fn find(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.vms
            .lock()
            .iter()
            .find(|v| v.vm_id == vm_id)
            .cloned()
            .ok_or_else(|| format!("VM not found: {vm_id}"))
    }

    fn set_status(&self, vm_id: &str, status: &str) -> Result<VmDescriptor, String> {
        let mut vms = self.vms.lock();
        let vm = vms
            .iter_mut()
            .find(|v| v.vm_id == vm_id)
            .ok_or_else(|| format!("VM not found: {vm_id}"))?;
        vm.status = status.to_string();
        Ok(vm.clone())
    }
}

#[async_trait]
impl VmHost for RecordingHost {
    async fn create(&self, spec: VmSpec) -> Result<VmDescriptor, String> {
        self.calls.lock().push(format!("create:{}", spec.name));
        self.specs.lock().push(spec.clone());
        let descriptor = VmDescriptor {
            vm_id: format!("host-vm-{}", self.vms.lock().len()),
            name: spec.name,
            cpu_cores: spec.cpu_cores,
            memory_gb: spec.memory_gb,
            status: "created".to_string(),
            boot_protocol: spec.boot.as_ref().map(|b| b.protocol().to_string()),
        };
        self.vms.lock().push(descriptor.clone());
        Ok(descriptor)
    }

    async fn start(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.calls.lock().push(format!("start:{vm_id}"));
        self.set_status(vm_id, "running")
    }

    async fn stop(&self, vm_id: &str, force: bool) -> Result<VmDescriptor, String> {
        self.calls.lock().push(format!("stop:{vm_id}:{force}"));
        self.set_status(vm_id, "stopped")
    }

    async fn pause(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.calls.lock().push(format!("pause:{vm_id}"));
        self.set_status(vm_id, "paused")
    }

    async fn resume(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.calls.lock().push(format!("resume:{vm_id}"));
        self.set_status(vm_id, "running")
    }

    async fn delete(&self, vm_id: &str) -> Result<(), String> {
        self.calls.lock().push(format!("delete:{vm_id}"));
        let mut vms = self.vms.lock();
        let before = vms.len();
        vms.retain(|v| v.vm_id != vm_id);
        if vms.len() == before {
            return Err(format!("VM not found: {vm_id}"));
        }
        Ok(())
    }

    async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.calls.lock().push(format!("status:{vm_id}"));
        self.find(vm_id)
    }

    async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
        self.calls.lock().push("list".to_string());
        Ok(self.vms.lock().clone())
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════
//  Dispatch
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn without_a_host_the_tools_keep_their_own_records() {
    // The default behaviour has to keep working: agent logic and tool schemas
    // must be exercisable with no hypervisor present.
    let server = McpServer::new();
    assert!(server.vm_host().is_none());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let created = call(&server, &session, "vm.create", json!({"name": "local"}))
        .await
        .expect("vm.create");
    let vm_id = created["vm_id"].as_str().expect("vm_id").to_string();

    let started = call(&server, &session, "vm.start", json!({"vm_id": vm_id}))
        .await
        .expect("vm.start");
    assert_eq!(started["status"], "running");
}

#[tokio::test]
async fn a_guest_cid_asked_for_at_create_reaches_the_host() {
    // Until this parameter existed there was no way to get a guest channel onto
    // a VM through the tool surface at all, which made vm.exec unreachable no
    // matter what was running inside the guest.
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host.clone());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    call(
        &server,
        &session,
        "vm.create",
        json!({"name": "talkative", "guest_cid": 42}),
    )
    .await
    .expect("vm.create");
    assert_eq!(host.last_spec().guest_cid, Some(42));

    // And a VM asked for without one still gets none: a channel nobody
    // requested is a device an agent cannot account for.
    call(&server, &session, "vm.create", json!({"name": "quiet"}))
        .await
        .expect("vm.create");
    assert_eq!(host.last_spec().guest_cid, None);

    // A CID that is not a number is a mistake. Dropping it would create the VM
    // without the channel and fail much later, at vm.exec.
    let err = call(
        &server,
        &session,
        "vm.create",
        json!({"name": "bad", "guest_cid": "three"}),
    )
    .await
    .expect_err("a string is not a CID");
    assert!(err.contains("guest_cid"), "got: {err}");
}

#[tokio::test]
async fn with_a_host_the_lifecycle_tools_reach_it() {
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host.clone());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let created = call(
        &server,
        &session,
        "vm.create",
        json!({"name": "hosted", "cpu_cores": 8, "memory_gb": 32}),
    )
    .await
    .expect("vm.create");

    let vm_id = created["vm_id"].as_str().expect("vm_id").to_string();
    assert_eq!(created["name"], "hosted");
    assert_eq!(
        created["cpu_cores"], 8,
        "the host must receive the requested shape, not the defaults"
    );
    assert_eq!(created["memory_gb"], 32);

    call(&server, &session, "vm.start", json!({"vm_id": vm_id}))
        .await
        .expect("vm.start");
    call(&server, &session, "vm.pause", json!({"vm_id": vm_id}))
        .await
        .expect("vm.pause");
    call(&server, &session, "vm.resume", json!({"vm_id": vm_id}))
        .await
        .expect("vm.resume");
    call(
        &server,
        &session,
        "vm.stop",
        json!({"vm_id": vm_id, "force": true}),
    )
    .await
    .expect("vm.stop");
    call(&server, &session, "vm.delete", json!({"vm_id": vm_id}))
        .await
        .expect("vm.delete");

    assert_eq!(
        host.calls(),
        vec![
            "create:hosted".to_string(),
            format!("start:{vm_id}"),
            format!("pause:{vm_id}"),
            format!("resume:{vm_id}"),
            format!("stop:{vm_id}:true"),
            format!("delete:{vm_id}"),
        ],
        "every lifecycle tool should reach the host, in order, with its arguments"
    );
}

#[tokio::test]
async fn a_boot_source_reaches_the_host() {
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host.clone());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let created = call(
        &server,
        &session,
        "vm.create",
        json!({
            "name": "booted",
            "boot": {"type": "linux", "kernel": "/boot/vmlinuz", "cmdline": "console=ttyS0"}
        }),
    )
    .await
    .expect("vm.create with a boot source");

    assert_eq!(
        created["boot_protocol"], "linux",
        "the agent should be able to see what the VM boots"
    );
}

#[tokio::test]
async fn a_malformed_boot_source_is_rejected_with_a_useful_message() {
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host.clone());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let err = call(
        &server,
        &session,
        "vm.create",
        json!({"name": "bad", "boot": {"type": "nonsense"}}),
    )
    .await
    .expect_err("an unknown boot type must be rejected");

    assert!(err.contains("boot source"), "got: {err}");
    assert!(
        host.calls().is_empty(),
        "a request that failed validation must not reach the host"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  Isolation
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_session_cannot_touch_another_sessions_vm() {
    // With a shared host the VM's existence is no longer proof of ownership,
    // so the server has to enforce it.
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host.clone());

    let owner = server
        .create_session("owner", AgentCapabilities::full())
        .unwrap();
    let intruder = server
        .create_session("intruder", AgentCapabilities::full())
        .unwrap();

    let created = call(&server, &owner, "vm.create", json!({"name": "private"}))
        .await
        .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    for tool in ["vm.start", "vm.stop", "vm.pause", "vm.resume", "vm.delete"] {
        let result = call(&server, &intruder, tool, json!({"vm_id": vm_id})).await;
        assert!(
            result.is_err(),
            "{tool} must be refused for a session that does not own the VM"
        );
    }

    // Every one of those must have been refused before reaching the host.
    assert_eq!(
        host.calls(),
        vec!["create:private".to_string()],
        "a non-owner's calls must never reach the host"
    );
}

#[tokio::test]
async fn probing_for_another_sessions_vm_is_indistinguishable_from_absence() {
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host);

    let owner = server
        .create_session("owner", AgentCapabilities::full())
        .unwrap();
    let intruder = server
        .create_session("intruder", AgentCapabilities::full())
        .unwrap();

    let created = call(&server, &owner, "vm.create", json!({"name": "secret"}))
        .await
        .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    let existing = call(&server, &intruder, "vm.status", json!({"vm_id": vm_id}))
        .await
        .expect_err("must be refused");
    let absent = call(
        &server,
        &intruder,
        "vm.status",
        json!({"vm_id": "no-such-vm"}),
    )
    .await
    .expect_err("must be refused");

    assert_eq!(
        existing.replace(&vm_id, "<id>"),
        absent.replace("no-such-vm", "<id>"),
        "the two errors must not let a session probe for others' VMs"
    );
}

#[tokio::test]
async fn vm_list_shows_only_the_calling_sessions_vms() {
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host);

    let a = server
        .create_session("a", AgentCapabilities::full())
        .unwrap();
    let b = server
        .create_session("b", AgentCapabilities::full())
        .unwrap();

    call(&server, &a, "vm.create", json!({"name": "a-one"}))
        .await
        .unwrap();
    call(&server, &a, "vm.create", json!({"name": "a-two"}))
        .await
        .unwrap();
    call(&server, &b, "vm.create", json!({"name": "b-one"}))
        .await
        .unwrap();

    let listed = call(&server, &a, "vm.list", json!({})).await.unwrap();
    assert_eq!(listed["total"], 2);

    let names: Vec<&str> = listed["vms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"a-one") && names.contains(&"a-two"));
    assert!(
        !names.contains(&"b-one"),
        "session A must not see session B's VM"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  Non-lifecycle tools
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn tools_with_no_host_equivalent_still_work() {
    // Installing a host must not break the tools it does not implement; they
    // read the session mirror the host dispatch keeps up to date.
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host);

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let created = call(&server, &session, "vm.create", json!({"name": "mixed"}))
        .await
        .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    let snapshot = call(
        &server,
        &session,
        "snapshot.create",
        json!({"vm_id": vm_id, "snapshot_name": "s1"}),
    )
    .await
    .expect("snapshot.create falls through to the session mirror");
    assert_eq!(snapshot["vm_id"], vm_id);
}

#[tokio::test]
async fn metrics_come_from_the_host_when_one_is_installed() {
    let host = Arc::new(RecordingHost::default());
    let server = McpServer::new();
    server.set_vm_host(host.clone());

    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let created = call(
        &server,
        &session,
        "vm.create",
        json!({"name": "observed", "cpu_cores": 8, "memory_gb": 16}),
    )
    .await
    .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    let metrics = call(&server, &session, "vm.metrics", json!({"vm_id": vm_id}))
        .await
        .expect("vm.metrics");

    assert_eq!(metrics["vm_id"], vm_id);
    assert_eq!(metrics["vcpu_count"], 8);
    assert_eq!(metrics["memory_total_bytes"], 16u64 * 1024 * 1024 * 1024);
    // `RecordingHost` measures nothing, so it inherits the trait's default.
    // The unmeasured fields must be null — a zero here would read as a real
    // observation of an idle VM.
    assert!(metrics["cpu_usage_percent"].is_null());
    assert!(metrics["memory_used_bytes"].is_null());
    assert!(metrics["uptime_seconds"].is_null());
}

#[tokio::test]
async fn metrics_without_a_host_measure_nothing_rather_than_reporting_zero() {
    let server = McpServer::new();
    let session = server
        .create_session("agent", AgentCapabilities::full())
        .unwrap();

    let created = call(
        &server,
        &session,
        "vm.create",
        json!({"name": "unhosted", "cpu_cores": 2, "memory_gb": 4}),
    )
    .await
    .unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();

    let metrics = call(&server, &session, "vm.metrics", json!({"vm_id": vm_id}))
        .await
        .expect("vm.metrics");

    assert_eq!(metrics["vm_id"], vm_id);
    // The shape is read back out of the session mirror, so these also guard
    // against the mirror's key names drifting away from what metrics expects.
    assert_eq!(metrics["vcpu_count"], 2);
    assert_eq!(metrics["memory_total_bytes"], 4u64 * 1024 * 1024 * 1024);
    assert_eq!(metrics["status"], "created");
    assert!(metrics["cpu_usage_percent"].is_null());
    assert!(metrics["memory_used_bytes"].is_null());
    assert!(metrics["uptime_seconds"].is_null());
}
