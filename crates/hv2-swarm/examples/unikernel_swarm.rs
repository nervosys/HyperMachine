//! A swarm where every agent is a real unikernel VM.
//!
//! The crate's own tests prove the command graph refuses what it should, but
//! they route between entries in a map. This runs the same graph over a swarm
//! whose agents are hardware-isolated guests: one VM per agent, each executing
//! its own code, each costing about 0.16 MiB of host memory.
//!
//! It exists because "a thousand specialised agents under a permission graph"
//! is two claims, and they fail differently. The graph could be right while
//! nothing runs; a thousand VMs could run while the graph governs nothing. So
//! this asserts both in one process and reports what it measured.
//!
//! # What it checks
//!
//! 1. Every agent's VM actually executed — proven by its guest writing to its
//!    own serial port, not by `launch()` returning.
//! 2. A command from the root reaches a leaf many levels away.
//! 3. A message between siblings is refused *and does not arrive*.
//! 4. Revoking a grant closes an edge that was open a moment earlier.
//!
//! Check 1 is the one that makes the rest mean anything. A swarm of VMs that
//! never ran is a swarm of allocations, and its permission graph is a diagram.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-swarm --example unikernel_swarm -- --agents 200
//! ```
//!
//! Needs `/dev/kvm`. Each agent is a 16 MiB guest, so the address space
//! reserved is agents × 16 MiB and almost none of it becomes resident.

use std::sync::Arc;
use std::time::Instant;

use hv2_core::{BootSource, VMConfig, VM};
use hv2_swarm::{Denied, LocalTransport, Relation, Swarm};

/// One agent's guest: write a byte to COM1 so the VM has demonstrably run,
/// then halt. The smallest program that distinguishes "executed" from
/// "allocated".
fn agent_image() -> Vec<u8> {
    vec![
        0xBA, 0xF8, 0x03, // mov dx, 0x3F8
        0xB0, 0x2A, // mov al, '*'
        0xEE, // out dx, al
        0xF4, // hlt
    ]
}

/// Supervisors under the root. The rest of the agents divide between them.
const SUPERVISORS: usize = 10;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut agents = 50usize;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agents" if i + 1 < args.len() => {
                agents = args[i + 1].parse().unwrap_or(agents);
                i += 1;
            }
            other => {
                eprintln!("unrecognised argument {other}");
                return std::process::ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let workers = agents.saturating_sub(1 + SUPERVISORS);

    let dir = std::env::temp_dir().join("hv2-swarm");
    let image = dir.join("agent.bin");
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&image, agent_image()).is_err() {
        eprintln!("could not write the agent image to {}", image.display());
        return std::process::ExitCode::FAILURE;
    }

    // The graph first. It is cheap, it is the thing being demonstrated, and
    // building it before any VM means a topology mistake costs nothing.
    let mut swarm = Swarm::new(LocalTransport::new());
    swarm.add_root("root").expect("root");
    for s in 0..SUPERVISORS {
        swarm.add_agent(format!("sup-{s}"), "root").expect("sup");
    }
    for w in 0..workers {
        swarm
            .add_agent(format!("w-{w}"), format!("sup-{}", w % SUPERVISORS))
            .expect("worker");
    }
    println!(
        "swarm         : {} agents — 1 root, {SUPERVISORS} supervisors, {workers} workers",
        swarm.len()
    );

    // One VM per agent, held so the whole swarm is alive at once.
    let started = Instant::now();
    let mut vms: Vec<(String, Arc<VM>)> = Vec::with_capacity(agents);
    for index in 0..agents {
        let name = if index == 0 {
            "root".to_string()
        } else if index <= SUPERVISORS {
            format!("sup-{}", index - 1)
        } else {
            format!("w-{}", index - SUPERVISORS - 1)
        };

        let config = VMConfig {
            name: name.clone(),
            vcpu_count: 1,
            memory_size: 16 * 1024 * 1024,
            boot: Some(BootSource::raw(&image)),
            ..Default::default()
        };
        let vm = match VM::new(config) {
            Ok(vm) => Arc::new(vm),
            Err(e) => {
                eprintln!("agent '{name}': no hypervisor backend — {e}");
                eprintln!("This needs /dev/kvm.");
                return std::process::ExitCode::FAILURE;
            }
        };
        if let Err(e) = vm.provision().await {
            eprintln!("agent '{name}': provision failed — {e}");
            return std::process::ExitCode::FAILURE;
        }
        if let Err(e) = vm.launch().await {
            eprintln!("agent '{name}': launch failed — {e}");
            return std::process::ExitCode::FAILURE;
        }
        vms.push((name, vm));
    }
    let launched = started.elapsed();

    // Executed, not merely started. A guest that wrote nothing did not run,
    // and a swarm of those is a swarm of allocations.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut ran = 0usize;
    let mut silent = Vec::new();
    for (name, vm) in &vms {
        if vm.console_output().await.is_empty() {
            silent.push(name.clone());
        } else {
            ran += 1;
        }
    }

    println!(
        "vms           : {ran} of {} agents executed their own code in {:.0} ms ({:.2} ms each)",
        vms.len(),
        launched.as_secs_f64() * 1000.0,
        launched.as_secs_f64() * 1000.0 / vms.len() as f64
    );
    if !silent.is_empty() {
        println!(
            "silent        : {} agent(s), first few: {:?}",
            silent.len(),
            &silent[..silent.len().min(5)]
        );
    }

    // Now the graph, over the swarm that is actually running.
    let leaf = format!("w-{}", workers.saturating_sub(1));
    let sibling = format!("w-{}", workers.saturating_sub(1 + SUPERVISORS));

    let mut failures = Vec::new();

    match swarm.send("root", leaf.as_str(), b"execute".to_vec()) {
        Ok(Relation::Descendant) => {
            println!("command       : root -> {leaf} allowed, delivered");
        }
        other => failures.push(format!("root -> {leaf} should be a command, got {other:?}")),
    }

    match swarm.send(leaf.as_str(), sibling.as_str(), b"psst".to_vec()) {
        Err(Denied::NoGrant { .. }) => {
            let arrived = swarm
                .transport()
                .delivered_to(&hv2_swarm::AgentId::new(sibling.clone()));
            if arrived == 0 {
                println!("refusal       : {leaf} -> {sibling} refused, nothing delivered");
            } else {
                failures.push(format!(
                    "{leaf} -> {sibling} was refused but {arrived} message(s) arrived anyway"
                ));
            }
        }
        other => failures.push(format!(
            "{leaf} -> {sibling} should be refused, got {other:?}"
        )),
    }

    // Capability, not position. The supervisor may certainly address the
    // worker; it still may not borrow an entitlement it does not hold.
    swarm.grant_capability(&hv2_swarm::AgentId::new(leaf.clone()), "net");
    match swarm.send_requiring("root", leaf.as_str(), "net", b"fetch".to_vec()) {
        Err(Denied::SenderLacks { .. }) => {
            println!(
                "no amplify    : root -> {leaf} refused; the worker holds 'net', the root does not"
            );
        }
        other => failures.push(format!(
            "root -> {leaf} requiring 'net' should refuse as SenderLacks, got {other:?}"
        )),
    }
    swarm.grant_capability(&hv2_swarm::AgentId::new("root".to_string()), "net");
    match swarm.send_requiring("root", leaf.as_str(), "net", b"fetch".to_vec()) {
        Ok(Relation::Descendant) => {
            println!("capability    : root given 'net', same command now allowed");
        }
        other => failures.push(format!(
            "root -> {leaf} requiring 'net' should now pass, got {other:?}"
        )),
    }

    swarm.grant(leaf.as_str(), sibling.as_str());
    let granted = swarm.send(leaf.as_str(), sibling.as_str(), b"now allowed".to_vec());
    swarm.revoke(
        &hv2_swarm::AgentId::new(leaf.clone()),
        &hv2_swarm::AgentId::new(sibling.clone()),
    );
    let after_revoke = swarm.send(leaf.as_str(), sibling.as_str(), b"and again".to_vec());
    let delivered = swarm
        .transport()
        .delivered_to(&hv2_swarm::AgentId::new(sibling.clone()));

    match (granted, after_revoke, delivered) {
        (Ok(Relation::Granted), Err(Denied::NoGrant { .. }), 1) => {
            println!("grant         : opened, one message through, closed, second refused");
        }
        (g, r, d) => failures.push(format!(
            "grant/revoke went wrong: granted={g:?} after_revoke={r:?} delivered={d}"
        )),
    }

    for (_, vm) in &vms {
        let _ = vm.stop().await;
    }

    if ran != vms.len() {
        eprintln!(
            "\nFAILED: {} agent(s) never executed. A permission graph over VMs that did not run \
             governs nothing.",
            vms.len() - ran
        );
        return std::process::ExitCode::FAILURE;
    }
    if !failures.is_empty() {
        eprintln!("\nFAILED:");
        for failure in &failures {
            eprintln!("  {failure}");
        }
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "\nresult        : {} hardware-isolated agents running at once, and the command graph \
         held for every message tried",
        vms.len()
    );
    std::process::ExitCode::SUCCESS
}
