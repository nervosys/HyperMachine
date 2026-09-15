//! Example: Basic VM creation and management
//!
//! Create a VM, start it, read its metrics, stop it. The lifecycle this shows
//! is the one that exists.
//!
//! It used to call `pause` and `resume` between those, and it had never once
//! got past the `pause`: `VM::pause` requires each vCPU to be in
//! `VCpuState::Running`, and nothing in this repository has ever put a vCPU in
//! that state. Every VM refused, not just this one. The call is kept below,
//! with its refusal printed rather than propagated, because a lifecycle
//! example that silently omitted the two operations people ask for first would
//! be the more misleading of the two options.

use anyhow::Result;
use hv2_agent::AgentVM;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Creating VM...");

    // Create a new VM
    let vm = AgentVM::builder()
        .name("example-vm")
        .cpu_cores(2)
        .memory_gb(4)
        .enable_networking(false)
        .enable_gpu(false)
        .with_tracing()
        .build()
        .await?;

    println!("VM created successfully!");
    println!("State: {:?}", vm.state());

    // Start the VM
    println!("\nStarting VM...");
    vm.start().await?;
    println!("VM started! State: {:?}", vm.state());

    // Get metrics
    let metrics = vm.get_metrics().await?;
    println!("\nVM Metrics:");
    println!("  State: {:?}", metrics.state);
    println!("  vCPUs: {}", metrics.vcpu_count);
    println!(
        "  Memory: {} GB",
        metrics.memory_size / (1024 * 1024 * 1024)
    );

    // Pause and resume, which this hypervisor does not implement. Shown
    // rather than hidden: the refusal is the useful part, and it is the same
    // refusal any VM gives -- see the note at the top of this file.
    println!("\nPausing VM...");
    match vm.pause().await {
        Ok(()) => println!("VM paused! State: {:?}", vm.state()),
        Err(e) => println!("  refused, and correctly: {e}"),
    }

    println!("\nResuming VM...");
    match vm.resume().await {
        Ok(()) => println!("VM resumed! State: {:?}", vm.state()),
        Err(e) => println!("  refused, and correctly: {e}"),
    }

    // Stop the VM
    println!("\nStopping VM...");
    vm.stop().await?;
    println!("VM stopped! State: {:?}", vm.state());

    Ok(())
}
