//! VM Execution with Interrupts Example
//!
//! This example demonstrates a complete VM execution loop that:
//! 1. Creates a VM with devices connected to the PIC
//! 2. Runs the VM in a loop, handling exits
//! 3. Checks for pending interrupts after each exit
//! 4. Injects interrupts into the guest when pending

use hv2_core::{
    hypervisor::create_backend, Device, HypervisorBackend, Pic8259, SerialDevice, TimerDevice,
    VMConfig, VmExit, VM,
};
use std::sync::Arc;
use tracing::{info, Level};

async fn initialize_pic(pic: &mut Pic8259) -> Result<(), Box<dyn std::error::Error>> {
    // ICW1: Start initialization
    pic.write(0x20, &[0x11]).await?;
    pic.write(0xA0, &[0x11]).await?;

    // ICW2: Set base interrupt vectors
    pic.write(0x21, &[0x20]).await?; // Master: 0x20-0x27
    pic.write(0xA1, &[0x28]).await?; // Slave: 0x28-0x2F

    // ICW3: Configure cascade
    pic.write(0x21, &[0x04]).await?; // Master: IRQ2 has slave
    pic.write(0xA1, &[0x02]).await?; // Slave: cascade identity

    // ICW4: Set mode
    pic.write(0x21, &[0x01]).await?; // 8086 mode
    pic.write(0xA1, &[0x01]).await?;

    // OCW1: Unmask all interrupts
    pic.write(0x21, &[0x00]).await?;
    pic.write(0xA1, &[0x00]).await?;

    Ok(())
}

async fn vm_execution_loop(
    vm: Arc<VM>,
    backend: Box<dyn HypervisorBackend>,
    timer: Arc<TimerDevice>,
    serial: Arc<SerialDevice>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- VM Execution Loop with Interrupts ---\n");

    let vcpu = vm.vcpu(0).unwrap();
    let pic = vm.pic();

    // Enable timer interrupts
    timer.set_interrupt_enabled(true);
    info!("Timer interrupts enabled");

    // Simulate some incoming serial data
    serial.input(b"Hello from serial!\n")?;
    info!("Serial data received\n");

    // Main VM execution loop
    for tick in 0..10 {
        info!("=== Tick {} ===", tick);

        // Simulate timer tick (in real VM, this would happen automatically)
        if tick % 3 == 0 {
            timer.tick()?;
            info!("  Timer tick");
        }

        // Check for pending interrupts BEFORE running vCPU
        if let Some(vector) = pic.get_pending_interrupt() {
            info!("  → Pending interrupt: vector {:#x}", vector);

            // Inject interrupt into guest
            backend.inject_interrupt(&vcpu, vector).await?;
            info!("  → Interrupt injected into guest");

            // Acknowledge interrupt (CPU would do this after handling)
            pic.acknowledge_interrupt(vector)?;
            info!("  → Interrupt acknowledged");

            // In real hardware, guest would send EOI when done
            // For demo, we simulate the guest sending EOI
            info!("  → EOI sent (simulated by guest)");
        }

        // Run vCPU (simulated - would normally execute guest code)
        match backend.run_vcpu(&vcpu).await? {
            VmExit::Hlt => {
                info!("  Guest executed HLT");
                // Check for interrupts to wake from halt
                if pic.get_pending_interrupt().is_some() {
                    info!("  → Waking from HLT for interrupt");
                    continue;
                }
                // No interrupt, just continue
            }

            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                info!(
                    "  Guest I/O: port={:#x}, direction={:?}, size={}, data={:#x}",
                    port, direction, size, data
                );

                // Handle PIC I/O ports
                if port == 0x20 || port == 0x21 || port == 0xA0 || port == 0xA1 {
                    info!("    → PIC register access");
                }
            }

            VmExit::Mmio {
                phys_addr,
                is_write,
                len,
                ..
            } => {
                info!(
                    "  Guest MMIO: addr={:#x}, write={}, len={}",
                    phys_addr, is_write, len
                );
            }

            VmExit::Shutdown => {
                info!("  Guest requested shutdown");
                break;
            }

            VmExit::InterruptWindow => {
                info!("  Interrupt window opened");
            }

            exit => {
                info!("  Other exit: {:?}", exit);
            }
        }

        // Small delay to make output readable
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        info!("");
    }

    info!("VM execution loop complete");
    info!("Total timer ticks: {}", timer.total_ticks());

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("=== HyperMachine VM with Interrupts Demo ===\n");

    // Create VM
    let config = VMConfig {
        name: "interrupt-demo-vm".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024, // 64 MB
        ..Default::default()
    };

    let vm = Arc::new(VM::new(config)?);
    info!("Created VM: {}", vm.config().name);
    info!("  vCPUs: {}", vm.config().vcpu_count);
    info!("  Memory: {} MB\n", vm.config().memory_size / (1024 * 1024));

    // Get PIC from VM
    let pic = vm.pic();
    info!("PIC available from VM");

    // Initialize PIC
    let mut pic_mut = Pic8259::new();
    initialize_pic(&mut pic_mut).await?;
    info!("PIC initialized\n");

    // Create hypervisor backend
    let backend = create_backend()?;
    info!("Hypervisor backend created\n");

    // Create timer device and connect to PIC
    let mut timer = TimerDevice::new("PIT".to_string(), 0x40);
    timer.set_pic(pic.clone());
    let timer = Arc::new(timer);
    info!("Timer device created and connected to PIC (IRQ 0)");

    // Create serial device and connect to PIC
    let mut serial = SerialDevice::new("COM1".to_string(), 0x3F8);
    serial.set_pic(pic.clone());
    let serial = Arc::new(serial);
    info!("Serial device created and connected to PIC (IRQ 4)\n");

    // Start VM
    vm.start().await?;
    info!("VM started\n");

    // Run execution loop
    vm_execution_loop(vm.clone(), backend, timer, serial).await?;

    // Stop VM
    vm.stop().await?;
    info!("\nVM stopped");

    info!("\n=== Demo Complete ===");

    Ok(())
}
