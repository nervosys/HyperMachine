//! End-to-end agent runtime: spawn a fleet of copy-on-write agents from one
//! warm baseline image, run a tool over MCP, and reclaim them.
//!
//! ```bash
//! cargo run -p hv2-agent --example agent_runtime
//! ```

use hv2_agent::{AgentCapabilities, AgentRuntime};
use serde_json::json;

#[tokio::main]
async fn main() {
    const MIB: usize = 1024 * 1024;

    // A 64 MiB "warm" baseline — a booted agent image / loaded runtime that
    // every agent starts from. Captured once.
    let baseline = vec![0u8; 64 * MIB];
    let rt = AgentRuntime::new(&baseline);
    println!("warm baseline: {} MiB", rt.baseline_bytes() / MIB);

    // Spawn 100 agents. Each spawn is an O(1) copy-on-write clone of the
    // baseline — no per-agent multi-megabyte copy.
    let mut agents = Vec::new();
    for i in 0..100 {
        agents.push(
            rt.spawn_agent(&format!("agent-{i}"), AgentCapabilities::full())
                .expect("spawn"),
        );
    }
    println!("\nspawned {} agents (O(1) each)", rt.live_agents());
    println!(
        "fleet memory: {} MiB shared  vs  {} MiB if each copied the baseline",
        rt.fleet_memory_bytes() / MIB,
        100 * rt.baseline_bytes() / MIB,
    );

    // Agents are isolated: a write in one becomes private to it.
    agents[0]
        .sandbox
        .write(0, b"agent-0 was here")
        .expect("write");
    let private: Vec<u8> = agents[0].sandbox.read(0, 16).unwrap();
    println!(
        "\nagent-0 private write: {:?}",
        String::from_utf8_lossy(&private)
    );
    println!(
        "agent-1 still sees baseline: {:?}",
        agents[1].sandbox.read(0, 16).unwrap()
    );

    // An agent calls a tool over the MCP surface.
    let resp = agents[0]
        .session
        .call_tool(rt.server(), "vm.list", json!({}))
        .await;
    println!("\nagent-0 vm.list -> success={}", resp.success);

    // Reclaim: release half explicitly, drop the rest and reap them. Each
    // teardown frees that agent's private pages back to the fleet.
    for a in agents.drain(..50) {
        rt.release_agent(a.session_id());
    }
    drop(agents);
    let reaped = rt.reap_idle(std::time::Duration::from_secs(0));
    println!(
        "\nreleased 50, reaped {reaped} -> {} agents live, fleet {} MiB",
        rt.live_agents(),
        rt.fleet_memory_bytes() / MIB,
    );

    println!("\n✅ agent runtime: O(1) spawn, shared baseline, isolation, auto-reclaim.");
}
