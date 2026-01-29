//! VM Exit Handling Example
//!
//! This example demonstrates the new VM exit handling mechanism.
//! It shows how VmExit types work and how to handle different exit reasons.

use hv2_core::{
    hypervisor::{create_backend, HypervisorBackend},
    IoDirection, VCpu, VmExit,
};
use tokio;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("=== HV2 VM Exit Handling Demo ===\n");

    // Create hypervisor backend
    let backend = create_backend()?;
    info!("Backend: {:?}", backend.platform());

    // Demonstrate different VM exit types
    demonstrate_exit_types().await?;

    // Demonstrate exit handling loop (simulated)
    demonstrate_exit_loop(&*backend).await?;

    Ok(())
}

async fn demonstrate_exit_types() -> Result<(), Box<dyn std::error::Error>> {
    info!("\n--- VM Exit Types ---\n");

    // 1. MMIO Read
    let mmio_read = VmExit::mmio_read(0xFEE00000, 4);
    info!("MMIO Read: {}", mmio_read);
    assert!(mmio_read.is_mmio());

    // 2. MMIO Write
    let data = [0x12, 0x34, 0x56, 0x78];
    let mmio_write = VmExit::mmio_write(0xFEE00010, &data);
    info!("MMIO Write: {}", mmio_write);

    // 3. I/O Port IN
    let io_in = VmExit::io_in(0x3F8, 1); // Serial port COM1
    info!("I/O IN: {}", io_in);
    assert!(io_in.is_io());

    // 4. I/O Port OUT
    let io_out = VmExit::io_out(0x3F8, 1, 0x41); // Write 'A' to serial
    info!("I/O OUT: {}", io_out);

    // 5. HLT
    let hlt = VmExit::Hlt;
    info!("HLT: {}", hlt);
    assert!(hlt.is_hlt());

    // 6. Shutdown
    let shutdown = VmExit::Shutdown;
    info!("Shutdown: {}", shutdown);
    assert!(shutdown.is_shutdown());

    // 7. Interrupt Window
    let int_window = VmExit::InterruptWindow;
    info!("Interrupt Window: {}", int_window);

    // 8. Exception
    let exception = VmExit::Exception {
        vector: 13, // General Protection Fault
        error_code: Some(0),
    };
    info!("Exception: {}", exception);

    Ok(())
}

async fn demonstrate_exit_loop(
    backend: &dyn HypervisorBackend,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n--- VM Execution Loop (Simulated) ---\n");

    // Create a dummy vCPU
    let vcpu = VCpu::new(0);

    info!("Starting VM execution loop...");

    // Simulate a few iterations of the VM execution loop
    for iteration in 0..5 {
        info!("\nIteration {}", iteration + 1);

        // Run vCPU until it exits
        let exit = backend.run_vcpu(&vcpu).await?;
        info!("  Exit reason: {}", exit);

        // Handle the exit
        match exit {
            VmExit::Mmio {
                phys_addr,
                data,
                len,
                is_write,
            } => {
                if is_write {
                    info!(
                        "  Handling MMIO write to {:#x}: {:02x?} ({} bytes)",
                        phys_addr,
                        &data[..len as usize],
                        len
                    );
                    // In real implementation: forward to device via MMIO manager
                } else {
                    info!("  Handling MMIO read from {:#x} ({} bytes)", phys_addr, len);
                    // In real implementation: read from device and return data
                }
            }

            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                match direction {
                    IoDirection::In => {
                        info!("  Handling I/O IN from port {:#x} ({} bytes)", port, size);
                        // In real implementation: read from device
                    }
                    IoDirection::Out => {
                        info!(
                            "  Handling I/O OUT to port {:#x}: {:#x} ({} bytes)",
                            port, data, size
                        );
                        // In real implementation: write to device
                    }
                }
            }

            VmExit::Hlt => {
                info!("  Guest halted, checking for pending interrupts...");
                // In real implementation:
                // if let Some(vector) = interrupt_controller.get_pending() {
                //     backend.inject_interrupt(&vcpu, vector).await?;
                // } else {
                //     tokio::time::sleep(Duration::from_millis(1)).await;
                // }

                // For demo, just simulate injecting an interrupt
                info!("  Injecting simulated timer interrupt (vector 32)");
                backend.inject_interrupt(&vcpu, 32).await?;
            }

            VmExit::Shutdown => {
                info!("  Guest requested shutdown, stopping VM");
                break;
            }

            VmExit::InterruptWindow => {
                info!("  Interrupt window opened, injecting pending interrupts");
                // In real implementation: inject queued interrupts
            }

            VmExit::Exception { vector, error_code } => {
                info!(
                    "  Exception occurred: vector={}, error_code={:?}",
                    vector, error_code
                );
                // In real implementation: either handle or inject into guest
            }

            VmExit::Debug { ref info } => {
                info!("  Debug event: {}", info);
            }

            VmExit::Unknown { reason } => {
                info!("  Unknown exit reason: {}", reason);
            }
        }

        // In a real implementation, we would continue the loop
        // For this demo, we'll break after showing the pattern
        if iteration >= 2 {
            info!("\n  (Demo complete - would continue running in production)");
            break;
        }
    }

    Ok(())
}
