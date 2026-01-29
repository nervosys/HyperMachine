//! Integrated example - AI agents controlling VM with serial console

use hv2_agent::AgentVM;
use hv2_core::{Device, MmioManager, SerialDevice};
use parking_lot::RwLock;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("🤖 HV2 Integrated Example: AI + Devices");
    println!("{}", "=".repeat(60));

    // Create MMIO manager and serial device
    let mmio = Arc::new(MmioManager::new());
    let serial = Arc::new(RwLock::new(SerialDevice::new("COM1".to_string(), 0x3F8)));

    serial.write().init().await?;
    mmio.map_device(0x3F8, 8, serial.clone())?;
    println!("✓ Serial console initialized at 0x3F8");

    // Create VM with AI agent capabilities
    let vm = AgentVM::builder()
        .name("integrated-demo")
        .cpu_cores(2)
        .memory_gb(2)
        .enable_gpu(false)
        .build()
        .await?;

    println!("✓ VM created: integrated-demo");

    // Start VM
    vm.vm().start().await?;
    println!("✓ VM started");

    // AI Script 1: Monitor VM and report status via serial
    println!("\n📜 Script 1: VM Status Reporter");
    let script1 = r#"
        print("AI Agent: Checking VM status...");
        
        let status = #{
            vm_name: vm_name,
            state: vm_state,
            vcpus: vcpu_count,
            memory_gb: memory_size
        };
        
        print("AI Agent: VM status retrieved");
        
        status
    "#;

    let result = vm.execute_agent_script(script1).await?;
    println!("  ✓ Script completed: {}", result);

    // Simulate guest writing to serial
    println!("\n📤 Simulating guest output to serial...");
    for &byte in b"[GUEST] Boot sequence initiated\n" {
        mmio.write(0x3F8, &[byte]).await?;
    }
    for &byte in b"[GUEST] Loading kernel...\n" {
        mmio.write(0x3F8, &[byte]).await?;
    }

    let output = serial.read().output_string();
    println!("  Serial output:\n{}", output);

    // AI Script 2: Analyze serial output
    println!("\n📜 Script 2: Serial Output Analyzer");
    let serial_data = output.clone();
    let script2 = format!(
        r#"
        print("AI Agent: Analyzing serial console output...");
        
        let output = "{}";
        let lines = output.len();
        
        let analysis = #{{
            output_length: lines,
            contains_boot: output.contains("Boot"),
            contains_kernel: output.contains("kernel"),
            status: if output.contains("Boot") {{ "booting" }} else {{ "unknown" }}
        }};
        
        print("AI Agent: Analyzed serial output");
        
        analysis
    "#,
        serial_data.replace("\"", "\\\"").replace("\n", "\\n")
    );

    let result = vm.execute_agent_script(&script2).await?;
    println!("  ✓ Analysis result: {}", result);

    // AI Script 3: Send commands to guest via serial
    println!("\n📜 Script 3: Guest Command Sender");
    let script3 = r#"
        print("AI Agent: Preparing commands for guest...");
        
        let commands = [
            "echo 'Hello from AI'",
            "uname -a",
            "df -h"
        ];
        
        print("AI Agent: Commands prepared");
        
        #{
            command_count: commands.len(),
            status: "commands_queued"
        }
    "#;

    let result = vm.execute_agent_script(script3).await?;
    println!("  ✓ Script completed: {}", result);

    // Simulate sending commands to guest
    println!("\n📥 Sending commands to guest via serial...");
    serial.read().input(b"echo 'Hello from AI'\n");
    serial.read().input(b"uname -a\n");

    // Simulate guest reading commands
    println!("  Guest reading commands:");
    let mut received = Vec::new();
    loop {
        let mut lsr = [0u8; 1];
        mmio.read(0x3F8 + 5, &mut lsr).await?;

        if lsr[0] & 0x01 == 0 {
            break;
        }

        let mut byte = [0u8; 1];
        mmio.read(0x3F8, &mut byte).await?;
        received.push(byte[0]);
    }

    let guest_received = String::from_utf8_lossy(&received);
    for line in guest_received.lines() {
        println!("    > {}", line);
    }

    // AI Script 4: Performance recommendation
    println!("\n📜 Script 4: Performance Optimizer");
    let script4 = r#"
        print("AI Agent: Analyzing VM performance...");
        
        let current_vcpus = vcpu_count;
        let current_memory = memory_size;
        
        let recommendation = if current_vcpus < 4 {
            #{
                action: "upgrade",
                recommended_vcpus: 4,
                reason: "Low vCPU count for modern workloads"
            }
        } else {
            #{
                action: "maintain",
                recommended_vcpus: current_vcpus,
                reason: "Configuration is optimal"
            }
        };
        
        print("AI Agent: Performance analysis complete");
        
        recommendation
    "#;

    let result = vm.execute_agent_script(script4).await?;
    println!("  ✓ Recommendation: {}", result);

    // Show MMIO statistics
    println!("\n📊 MMIO Statistics:");
    for region in mmio.regions() {
        println!(
            "  • {}: 0x{:X}-0x{:X} ({} bytes)",
            region.device_name,
            region.base,
            region.base + region.size,
            region.size
        );
    }

    // Stop VM
    vm.vm().stop().await?;
    println!("\n✓ VM stopped");

    println!("\n✅ Integrated example completed successfully!");
    println!("   Demonstrated:");
    println!("   - AI agent VM control");
    println!("   - Serial device emulation");
    println!("   - MMIO management");
    println!("   - Agent-device interaction");

    Ok(())
}
