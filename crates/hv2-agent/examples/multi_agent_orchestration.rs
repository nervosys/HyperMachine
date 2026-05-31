//! Example: multiple AI agents coordinating over shared VMs.
//!
//! HyperMachine is built for *fleets* of cooperating agents, not just one.
//! This shows the orchestration primitives that make that safe:
//!
//! - **Role-scoped agents** — an `Operator` can manage VMs; a `Monitor` cannot.
//! - **Exclusive VM claims** — two agents can never act on the same VM at once.
//! - **Inter-agent messaging** — agents hand off work explicitly.
//!
//! Run with:
//! ```bash
//! cargo run -p hv2-agent --example multi_agent_orchestration
//! ```

use hv2_agent::{AgentOrchestrator, AgentRole, OrchMessageType};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🤖 HyperMachine — multi-agent orchestration\n{}",
        "=".repeat(60)
    );

    let orch = AgentOrchestrator::new();

    // A small fleet: two operators that manage VMs and one read-only monitor.
    orch.register_agent("ops-1", "Operator", AgentRole::Operator)?;
    orch.register_agent("ops-2", "Operator-2", AgentRole::Operator)?;
    orch.register_agent("mon-1", "Monitor", AgentRole::Monitor)?;
    println!("\n[register] 3 agents: ops-1 (Operator), ops-2 (Operator), mon-1 (Monitor)");

    // ops-1 takes an exclusive claim on a VM before operating on it.
    println!("\n[claim]");
    let claim = orch.claim_vm("ops-1", "web-vm", Some("rolling restart"), None)?;
    println!(
        "  ops-1 claimed {} (reason: {:?})",
        claim.vm_name, claim.reason
    );

    // A second operator cannot claim the same VM — the claim is exclusive.
    match orch.claim_vm("ops-2", "web-vm", None, None) {
        Err(e) => println!("  ops-2 claim correctly rejected: {e}"),
        Ok(_) => panic!("expected a claim conflict"),
    }

    // A monitor has no claim permission at all — role enforcement.
    match orch.claim_vm("mon-1", "web-vm", None, None) {
        Err(e) => println!("  mon-1 (Monitor) correctly denied: {e}"),
        Ok(_) => panic!("a monitor must not be able to claim VMs"),
    }

    // ops-1 messages ops-2 to coordinate a hand-off.
    println!("\n[message]");
    orch.send_message(
        "ops-1",
        "ops-2",
        OrchMessageType::Info,
        json!({ "event": "handoff", "vm": "web-vm", "when": "after restart" }),
    )?;
    let inbox = orch.receive_messages("ops-2", 10);
    println!("  ops-2 inbox: {} message(s)", inbox.len());
    for m in &inbox {
        println!("    from {}: {}", m.sender, m.payload);
    }

    // ops-1 finishes and releases the claim; now ops-2 can take over cleanly.
    println!("\n[handoff]");
    orch.release_vm("ops-1", "web-vm")?;
    let claim2 = orch.claim_vm("ops-2", "web-vm", Some("take over after handoff"), None)?;
    println!("  ops-1 released; ops-2 now holds {}", claim2.vm_name);

    println!("\n✅ orchestration complete — role-scoped, conflict-free, coordinated.");
    Ok(())
}
