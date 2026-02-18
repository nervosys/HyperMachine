//! Example: AI agent script execution

use anyhow::Result;
use hv2_agent::AgentVM;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Creating VM with AI agent support...");

    // Create VM with custom script timeout
    let vm = AgentVM::builder()
        .name("ai-agent-vm")
        .cpu_cores(4)
        .memory_gb(8)
        .enable_gpu(true)
        .with_tracing()
        .script_timeout(Duration::from_secs(60))
        .build()
        .await?;

    // Start the VM
    vm.start().await?;
    println!("VM started!\n");

    // Example 1: Simple state query
    println!("=== Example 1: Query VM State ===");
    let script1 = r#"
        print("VM Name: " + vm_name);
        print("vCPU Count: " + vcpu_count);
        print("State: " + vm_state);
        
        vm_state
    "#;

    let result1 = vm.execute_agent_script(script1).await?;
    println!("Result: {}\n", result1);

    // Example 2: Conditional logic
    println!("=== Example 2: Conditional Logic ===");
    let script2 = r#"
        let vcpus = vcpu_count;
        
        if vcpus < 8 {
            print("Low vCPU count detected: " + vcpus);
            print("Consider scaling up");
            "needs_scaling"
        } else {
            print("vCPU count adequate: " + vcpus);
            "ok"
        }
    "#;

    let result2 = vm.execute_agent_script(script2).await?;
    println!("Result: {}\n", result2);

    // Example 3: Math and calculations
    println!("=== Example 3: Memory Calculations ===");
    let script3 = r#"
        let mem_bytes = memory_size;
        let mem_gb = mem_bytes / (1024 * 1024 * 1024);
        
        print("Total Memory: " + mem_gb + " GB");
        
        let recommended_swap = mem_gb * 2;
        print("Recommended Swap: " + recommended_swap + " GB");
        
        mem_gb
    "#;

    let result3 = vm.execute_agent_script(script3).await?;
    println!("Memory (GB): {}\n", result3);

    // Example 4: Error handling
    println!("=== Example 4: Script with Error Handling ===");
    let script4 = r#"
        try {
            let state = vm_state;
            print("Current state: " + state);
            
            if state == "Running" {
                print("VM is healthy");
                true
            } else {
                print("VM not running");
                false
            }
        } catch (error) {
            print("Error: " + error);
            false
        }
    "#;

    let result4 = vm.execute_agent_script(script4).await?;
    println!("Health check result: {}\n", result4);

    // Stop the VM
    vm.stop().await?;
    println!("VM stopped!");

    Ok(())
}
