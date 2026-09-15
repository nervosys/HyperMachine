//! VM Execution with Interrupts Example
//!
//! This example demonstrates a complete VM execution loop that:
//! 1. Creates a VM with devices connected to the PIC
//! 2. Runs the VM in a loop, handling exits
//! 3. Checks for pending interrupts after each exit
//! 4. Injects interrupts into the guest when pending

use hv2_core::{
    hypervisor::create_backend, HypervisorBackend, Pic8259, SerialDevice, TimerDevice, VMConfig, VM,
};
use std::sync::Arc;
use tracing::{info, Level};

/// Where `initialize_pic` below remaps the master PIC's lines.
///
/// Named because two places depend on it and they must not drift: the ICW2
/// written into the controller, and the subtraction that recovers a line
/// number from a vector.
const PIC_VECTOR_BASE: u8 = 0x20;

/// Program the controller: remap the lines and unmask them.
///
/// Through `write_port`, which takes `&self`, rather than the `Device::write`
/// this used before. That mattered for a reason beyond taste: `Device::write`
/// needs `&mut self`, an `Arc<Pic8259>` cannot give one, and so the only
/// controller this function could be pointed at was a fresh one it owned --
/// never the VM's. The interior mutability was already there; the `&mut`
/// receiver was what hid it.
async fn initialize_pic(pic: &Pic8259) -> Result<(), Box<dyn std::error::Error>> {
    // ICW1: Start initialization
    pic.write_port(0x20, 0x11).await?;
    pic.write_port(0xA0, 0x11).await?;

    // ICW2: Set base interrupt vectors
    pic.write_port(0x21, PIC_VECTOR_BASE).await?; // Master: 0x20-0x27
    pic.write_port(0xA1, 0x28).await?; // Slave: 0x28-0x2F

    // ICW3: Configure cascade
    pic.write_port(0x21, 0x04).await?; // Master: IRQ2 has slave
    pic.write_port(0xA1, 0x02).await?; // Slave: cascade identity

    // ICW4: Set mode
    pic.write_port(0x21, 0x01).await?; // 8086 mode
    pic.write_port(0xA1, 0x01).await?;

    // OCW1: Unmask all interrupts
    pic.write_port(0x21, 0x00).await?;
    pic.write_port(0xA1, 0x00).await?;

    Ok(())
}

async fn vm_execution_loop(
    vm: Arc<VM>,
    backend: Box<dyn HypervisorBackend>,
    timer: Arc<TimerDevice>,
    serial: Arc<SerialDevice>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- VM Execution Loop with Interrupts ---\n");

    let pic = vm.pic();

    // A VM on the backend, because an interrupt line belongs to one. Without
    // this the calls below reach a backend that has created nothing, and the
    // answer is "vCPU 0 not found" rather than anything about interrupts.
    //
    // This is a second, bare VM beside the `VM` above, which is what this
    // example has always done -- it holds a `VM` for its devices and a
    // backend for its hypervisor, and the two were never joined.
    let backend_vm = match backend.create_vm(1, 16 * 1024 * 1024).await {
        Ok(vm) => Some(vm),
        Err(e) => {
            info!("No hypervisor VM: {e}");
            info!("Device interrupts will be shown on the model only.\n");
            None
        }
    };

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

            // Back to the line the vector came from. This program can do that
            // only because it programmed the controller itself, a few lines
            // up: the offset is its own choice. A host that had not made that
            // choice could not perform this subtraction, which is the whole
            // reason a vector cannot be handed to a hypervisor whose
            // controller lives in the kernel.
            let irq = u32::from(vector).saturating_sub(u32::from(PIC_VECTOR_BASE));

            if backend_vm.is_some() {
                // Asserted and released. The devices behind these lines are
                // edge-like -- a UART that transmits instantly is always ready
                // to send -- so holding the line would re-interrupt forever.
                backend.set_irq_line(irq, true).await?;
                backend.set_irq_line(irq, false).await?;
                info!("  → IRQ {irq} pulsed on the in-kernel controller");
            } else {
                info!("  → IRQ {irq} would be pulsed, if a VM existed to own it");
            }

            // Acknowledge interrupt (CPU would do this after handling)
            pic.acknowledge_interrupt(vector)?;
            info!("  → Interrupt acknowledged");

            // In real hardware, guest would send EOI when done
            // For demo, we simulate the guest sending EOI
            info!("  → EOI sent (simulated by guest)");
        }

        // No `run_vcpu` here. There is no guest code in this VM, so running it
        // would execute whatever zeroed memory decodes to, and the exit it
        // produced would say nothing about interrupts. The examples that run a
        // real guest are `hv1_under_hv2`, `vsock_echo` and `net_echo`.

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

    // Initialize the VM's PIC -- the one the devices below are wired to.
    //
    // This used to build a fresh `Pic8259`, program *that*, and drop it. So
    // the controller the timer and serial port actually raise lines on was
    // left masked and un-remapped, and `get_pending_interrupt` on it could
    // never return anything: the interrupt branch of the loop below had
    // never once been entered, in an example whose subject is interrupts.
    initialize_pic(&pic).await?;
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
