//! Advanced VM example with events and hypervisor backend

use hv2_agent::AgentVM;
use hv2_core::{hypervisor, HypervisorPlatform};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 HyperMachine Advanced VM Example\n");

    // Detect hypervisor platform
    let platform = HypervisorPlatform::detect();
    println!("📊 Detected hypervisor platform: {:?}", platform);

    // Create hypervisor backend
    let backend = hypervisor::create_backend()?;
    println!("✓ Hypervisor backend initialized");
    println!("  Platform: {:?}", backend.platform());

    let caps = backend.capabilities();
    println!("  Max vCPUs: {}", caps.max_vcpus);
    println!(
        "  Max Memory: {} GB",
        caps.max_memory / (1024 * 1024 * 1024)
    );
    println!("  Nested Virtualization: {}", caps.supports_nested_virt);
    println!("  GPU Passthrough: {}\n", caps.supports_gpu_passthrough);

    // Create a VM with AgentVM builder
    println!("📦 Creating VM...");
    let vm = AgentVM::builder()
        .name("advanced-demo")
        .cpu_cores(4)
        .memory_gb(4)
        .enable_gpu(true)
        .enable_networking(true)
        .build()
        .await?;

    println!("✓ VM created: advanced-demo");
    println!("  vCPUs: 4");
    println!("  Memory: 4 GB");
    println!("  GPU: Enabled");
    println!("  Network: Enabled\n");

    // Subscribe to VM events
    println!("📡 Subscribing to VM events...");
    let mut event_receiver = vm.vm().subscribe_events();

    // Spawn a task to listen for events
    let event_task = tokio::spawn(async move {
        while let Ok(event) = event_receiver.recv().await {
            println!(
                "🔔 Event: {}",
                serde_json::to_string_pretty(&event).unwrap()
            );
        }
    });

    // Start the VM
    println!("\n▶️  Starting VM...");
    vm.start().await?;
    sleep(Duration::from_millis(500)).await;

    // Get metrics
    let metrics = vm.get_metrics().await?;
    println!("\n📊 VM Metrics:");
    println!("{}", serde_json::to_string_pretty(&metrics)?);

    // Execute some AI agent scripts
    println!("\n🤖 Executing AI agent scripts...\n");

    // Script 1: Query VM state
    let script1 = r#"
        let state = vm_state;
        let name = vm_name;
        let vcpus = vcpu_count;
        
        print("VM Information:");
        print("  Name: " + name);
        print("  State: " + state);
        print("  vCPUs: " + vcpus);
        
        #{
            message: "VM is operational",
            vm_name: name,
            state: state,
            healthy: true
        }
    "#;

    println!("Script 1: Query VM State");
    match vm.execute_agent_script(script1).await {
        Ok(result) => println!("✓ Result: {}\n", result),
        Err(e) => println!("✗ Error: {}\n", e),
    }

    sleep(Duration::from_millis(300)).await;

    // Script 2: Conditional logic
    let script2 = r#"
        let vcpus = vcpu_count;
        let recommendation = if vcpus >= 4 {
            "High performance configuration"
        } else if vcpus >= 2 {
            "Standard configuration"
        } else {
            "Minimal configuration"
        };
        
        #{
            vcpu_count: vcpus,
            profile: recommendation,
            optimal: vcpus >= 4
        }
    "#;

    println!("Script 2: Performance Analysis");
    match vm.execute_agent_script(script2).await {
        Ok(result) => println!("✓ Result: {}\n", result),
        Err(e) => println!("✗ Error: {}\n", e),
    }

    sleep(Duration::from_millis(300)).await;

    // Script 3: Resource calculation
    let script3 = r#"
        // Calculate resource usage percentage
        fn calc_usage(used, total) {
            (used / total) * 100.0
        }
        
        let memory_gb = 4;
        let memory_used_gb = 1.5;
        let usage = calc_usage(memory_used_gb, memory_gb);
        
        let status = if usage > 80.0 { "high" } else { "normal" };
        
        #{
            memory_total_gb: memory_gb,
            memory_used_gb: memory_used_gb,
            usage_percent: usage,
            status: status
        }
    "#;

    println!("Script 3: Resource Monitoring");
    match vm.execute_agent_script(script3).await {
        Ok(result) => println!("✓ Result: {}\n", result),
        Err(e) => println!("✗ Error: {}\n", e),
    }

    // Pause the VM
    println!("\n⏸️  Pausing VM...");
    vm.pause().await?;
    sleep(Duration::from_millis(500)).await;

    // Resume the VM
    println!("▶️  Resuming VM...");
    vm.resume().await?;
    sleep(Duration::from_millis(500)).await;

    // Stop the VM
    println!("\n⏹️  Stopping VM...");
    vm.stop().await?;
    sleep(Duration::from_millis(500)).await;

    // Wait a bit for events to process
    sleep(Duration::from_millis(200)).await;

    // Cancel the event listener
    event_task.abort();

    println!("\n✅ Advanced VM example completed successfully!");

    Ok(())
}
