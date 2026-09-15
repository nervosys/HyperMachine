//! VM Exit Handling Example
//!
//! This example demonstrates the new VM exit handling mechanism.
//! It shows how VmExit types work and how to handle different exit reasons.

use hv2_core::{
    hypervisor::{create_backend, HypervisorBackend},
    IoDirection, VmExit,
};
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("=== HyperMachine VM Exit Handling Demo ===\n");

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

    // The exits a guest produces, and what each one is answered with.
    //
    // These are named here rather than run out of a vCPU, and that is a
    // correction rather than a shortcut. This loop used to call
    // `backend.run_vcpu(&vcpu)` on a `VCpu::new(0)` that no `create_vm` had
    // ever registered, so it failed at the first iteration with "vCPU 0 not
    // found" and none of the handling below had ever executed. Giving it a
    // real VM would fix the error and produce a different lie: a VM with no
    // guest image in it runs whatever zeroed memory decodes to, and the exit
    // that comes back says nothing about the case it is standing in for.
    //
    // What this example is actually for is the mapping from exit to response.
    // So the exits are stated, and the mapping is real. The examples that run
    // a guest and take its exits for real are `hv1_under_hv2`, `vsock_echo`
    // and `net_echo`.
    let exits = vec![
        VmExit::Hlt,
        VmExit::Io {
            port: 0x3f8,
            direction: IoDirection::Out,
            size: 1,
            data: u32::from(b'h'),
        },
        VmExit::Mmio {
            phys_addr: 0xd000_0000,
            data: [0; 8],
            len: 4,
            is_write: false,
        },
        VmExit::InterruptWindow,
        VmExit::Shutdown,
    ];

    info!("Walking the exits a guest produces...");

    for (iteration, exit) in exits.into_iter().enumerate() {
        info!("\nIteration {}", iteration + 1);
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
                    _ => {
                        info!("  Unknown I/O direction");
                    }
                }
            }

            VmExit::Hlt => {
                info!("  Guest halted, so something must wake it");
                // A halted vCPU is woken by an interrupt, and an interrupt
                // reaches it through the controller. Not `inject_interrupt`,
                // which this used to call: that hands a *vector* to a vCPU,
                // and KVM permits it only when the controller is in userspace.
                // Every VM this backend builds has one in the kernel, so the
                // ioctl answers ENXIO. The line is what the host holds; the
                // vector is the controller's to produce.
                info!("  Pulsing IRQ 0, the timer line");
                match backend.set_irq_line(0, true).await {
                    Ok(()) => {
                        backend.set_irq_line(0, false).await?;
                        info!("  The controller decides which vector that becomes");
                    }
                    Err(e) => info!("  This backend drives no interrupt line: {e}"),
                }
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

            VmExit::Hypercall { nr, .. } => {
                info!("  Hypercall nr={:#x}", nr);
            }

            VmExit::SystemEvent { type_, flags } => {
                info!("  System event: type={} flags={:#x}", type_, flags);
            }

            VmExit::Nmi => {
                info!("  NMI received");
            }

            VmExit::Rdmsr { index } => {
                info!("  RDMSR index={:#x}", index);
            }

            VmExit::Wrmsr { index, data } => {
                info!("  WRMSR index={:#x} data={:#x}", index, data);
            }

            VmExit::IoapicEoi { vector } => {
                info!("  IOAPIC EOI vector={}", vector);
            }

            VmExit::Unknown { reason } => {
                info!("  Unknown exit reason: {}", reason);
            }

            _ => {
                info!("  Unhandled exit: {:?}", exit);
            }
        }
    }

    Ok(())
}
