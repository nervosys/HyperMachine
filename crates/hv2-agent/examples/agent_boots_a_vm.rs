//! Example: an agent's MCP tool calls creating and booting a *real* VM.
//!
//! The other agent examples exercise the tool surface against the MCP server's
//! own bookkeeping — useful, and it runs anywhere, but no guest is created.
//! This one installs a [`LocalVmHost`], after which the same `vm.create` /
//! `vm.start` calls allocate guest memory, provision the hypervisor backend,
//! load the boot images, and run the guest.
//!
//! Point it at a kernel to boot one:
//!
//! ```bash
//! cargo run -p hv2-agent --example agent_boots_a_vm -- /boot/vmlinuz
//! ```
//!
//! With no argument it runs the same flow without a boot source, which shows
//! the difference the tool results report: `boot_protocol` is absent, and the
//! VM starts without executing anything.

use std::sync::Arc;

use hv2_agent::vm_host::LocalVmHost;
use hv2_agent::{AgentCapabilities, McpServer};
use hv2_core::BootSource;
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kernel = std::env::args().nth(1);

    // Install the host. This one line is the difference between the tool calls
    // below being a simulation and being a hypervisor.
    let server = McpServer::new();
    server.set_vm_host(Arc::new(LocalVmHost::new()));

    let session = server.create_session("boot-agent", AgentCapabilities::full())?;
    println!("session {} opened\n", session.id);

    // ── vm.create ────────────────────────────────────────────────────
    let mut params = json!({
        "name": "agent-booted-vm",
        "cpu_cores": 2,
        "memory_gb": 2,
    });

    match &kernel {
        Some(path) => {
            println!("booting kernel: {path}");
            let source = BootSource::linux(path).with_cmdline("console=ttyS0");
            params["boot"] = serde_json::to_value(&source)?;
        }
        None => println!(
            "no kernel given — creating a VM with no guest code \
             (pass a bzImage path to boot one)"
        ),
    }

    let created = call(&server, &session, "vm.create", params).await?;
    let vm_id = created["vm_id"].as_str().expect("vm_id").to_string();

    match created.get("boot_protocol").and_then(Value::as_str) {
        Some(protocol) => println!("  VM will boot via the {protocol} protocol"),
        None => println!("  VM has no boot source — starting it executes nothing"),
    }

    // ── vm.start ─────────────────────────────────────────────────────
    // With a boot source this provisions the backend partition, writes the
    // kernel into guest physical memory, positions vCPU 0 at the entry point,
    // and runs the guest.
    match call(&server, &session, "vm.start", json!({ "vm_id": vm_id })).await {
        Ok(started) => println!("  status: {}", started["status"]),
        Err(e) => {
            // Expected on a host with no hypervisor backend available.
            println!("  start failed: {e}");
            println!("  (a hardware backend — KVM, WHPX, or HVF — is required to boot a guest)");
        }
    }

    // ── vm.list, then tear down ──────────────────────────────────────
    let listed = call(&server, &session, "vm.list", json!({})).await?;
    println!("  session owns {} VM(s)", listed["total"]);

    call(&server, &session, "vm.stop", json!({ "vm_id": vm_id })).await?;
    call(&server, &session, "vm.delete", json!({ "vm_id": vm_id })).await?;

    println!("\naudit log:");
    for entry in server.get_audit_log(20) {
        println!("  {:<12} success={}", entry.tool, entry.success);
    }

    Ok(())
}

/// Invoke a tool and surface its result, or its error as an `Err`.
async fn call(
    server: &McpServer,
    session: &hv2_agent::AgentSession,
    tool: &str,
    params: Value,
) -> Result<Value, String> {
    let response = session.call_tool(server, tool, params).await;
    if response.success {
        let result = response.result.unwrap_or(Value::Null);
        println!("→ {tool}");
        Ok(result)
    } else {
        Err(response.error.unwrap_or_default())
    }
}
