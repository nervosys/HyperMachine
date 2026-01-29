//! Example: Basic VM creation and management

use hv2_agent::AgentVM;
use anyhow::Result;

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
    println!("  Memory: {} GB", metrics.memory_size / (1024 * 1024 * 1024));

    // Pause the VM
    println!("\nPausing VM...");
    vm.pause().await?;
    println!("VM paused! State: {:?}", vm.state());

    // Resume the VM
    println!("\nResuming VM...");
    vm.resume().await?;
    println!("VM resumed! State: {:?}", vm.state());

    // Stop the VM
    println!("\nStopping VM...");
    vm.stop().await?;
    println!("VM stopped! State: {:?}", vm.state());

    Ok(())
}
