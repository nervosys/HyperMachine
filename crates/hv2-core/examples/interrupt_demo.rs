//! Interrupt Controller Example
//!
//! This example demonstrates the Intel 8259 PIC (Programmable Interrupt Controller)
//! and how hardware interrupts work in HV2.

use hv2_core::{hypervisor::create_backend, Device, Pic8259, VCpu};
use tokio;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("=== HV2 Interrupt Controller Demo ===\n");

    // Create PIC
    let mut pic = Pic8259::new();
    info!("Created Intel 8259 PIC");
    info!("  Master: IRQ 0-7  → INT 0x20-0x27");
    info!("  Slave:  IRQ 8-15 → INT 0x28-0x2F");
    info!("  Cascade: Slave on Master IRQ 2\n");

    // Initialize the PIC (like BIOS would do)
    initialize_pic(&mut pic).await?;

    // Demonstrate interrupt handling
    demonstrate_timer_interrupt(&mut pic).await?;
    demonstrate_keyboard_interrupt(&mut pic).await?;
    demonstrate_serial_interrupt(&mut pic).await?;
    demonstrate_interrupt_masking(&mut pic).await?;
    demonstrate_slave_interrupts(&mut pic).await?;

    // Demonstrate with actual VM execution loop
    demonstrate_vm_interrupt_loop(&mut pic).await?;

    Ok(())
}

async fn initialize_pic(pic: &mut Pic8259) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- Initializing PIC ---\n");

    // ICW1: Begin initialization
    let icw1: u8 = 0x11; // ICW4 needed + init
    pic.write(0x20, &[icw1]).await?;
    pic.write(0xA0, &[icw1]).await?;
    info!("  ICW1: Initialization started");

    // ICW2: Set base interrupt vectors
    pic.write(0x21, &[0x20]).await?; // Master: 0x20-0x27
    pic.write(0xA1, &[0x28]).await?; // Slave: 0x28-0x2F
    info!("  ICW2: Base vectors set (Master=0x20, Slave=0x28)");

    // ICW3: Cascade configuration
    pic.write(0x21, &[0x04]).await?; // Master: IRQ2 has slave
    pic.write(0xA1, &[0x02]).await?; // Slave: cascade identity
    info!("  ICW3: Cascade configured (slave on IRQ 2)");

    // ICW4: Mode configuration
    let icw4: u8 = 0x01; // 8086 mode
    pic.write(0x21, &[icw4]).await?;
    pic.write(0xA1, &[icw4]).await?;
    info!("  ICW4: 8086 mode enabled");

    // OCW1: Unmask all interrupts
    pic.write(0x21, &[0x00]).await?; // Master: all unmasked
    pic.write(0xA1, &[0x00]).await?; // Slave: all unmasked
    info!("  OCW1: All interrupts unmasked\n");

    Ok(())
}

async fn demonstrate_timer_interrupt(pic: &mut Pic8259) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- Timer Interrupt (IRQ 0) ---\n");

    // Simulate timer tick
    pic.raise_irq(0)?;
    info!("  Timer raised IRQ 0");

    // Check pending interrupt
    if let Some(vector) = pic.get_pending_interrupt() {
        info!("  Pending interrupt: vector {:#x} (INT 0x20)", vector);

        // Acknowledge interrupt (CPU would do this)
        pic.acknowledge_interrupt(vector)?;
        info!("  Interrupt acknowledged");

        // Send EOI (End of Interrupt)
        pic.write(0x20, &[0x20]).await?; // Non-specific EOI
        info!("  EOI sent to PIC\n");
    }

    Ok(())
}

async fn demonstrate_keyboard_interrupt(
    pic: &mut Pic8259,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- Keyboard Interrupt (IRQ 1) ---\n");

    pic.raise_irq(1)?;
    info!("  Keyboard raised IRQ 1");

    if let Some(vector) = pic.get_pending_interrupt() {
        info!("  Pending interrupt: vector {:#x} (INT 0x21)", vector);
        pic.acknowledge_interrupt(vector)?;
        pic.write(0x20, &[0x20]).await?;
        info!("  Interrupt handled\n");
    }

    Ok(())
}

async fn demonstrate_serial_interrupt(pic: &mut Pic8259) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- Serial Port Interrupt (IRQ 4) ---\n");

    pic.raise_irq(4)?;
    info!("  Serial port raised IRQ 4");

    if let Some(vector) = pic.get_pending_interrupt() {
        info!("  Pending interrupt: vector {:#x} (INT 0x24)", vector);
        pic.acknowledge_interrupt(vector)?;
        pic.write(0x20, &[0x20]).await?;
        info!("  Interrupt handled\n");
    }

    Ok(())
}

async fn demonstrate_interrupt_masking(
    pic: &mut Pic8259,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- Interrupt Masking ---\n");

    // Mask IRQ 1 (keyboard)
    let imr: u8 = 0x02; // Bit 1 = 1 (mask IRQ 1)
    pic.write(0x21, &[imr]).await?;
    info!("  Masked IRQ 1 (keyboard)");

    // Raise IRQ 0 and IRQ 1
    pic.raise_irq(0)?;
    pic.raise_irq(1)?;
    info!("  Raised IRQ 0 and IRQ 1");

    // Only IRQ 0 should be pending
    if let Some(vector) = pic.get_pending_interrupt() {
        info!(
            "  Pending interrupt: vector {:#x} (IRQ 0 only, IRQ 1 masked)",
            vector
        );
        pic.acknowledge_interrupt(vector)?;
        pic.write(0x20, &[0x20]).await?;
    }

    // Unmask IRQ 1
    pic.write(0x21, &[0x00]).await?;
    info!("  Unmasked all interrupts\n");

    Ok(())
}

async fn demonstrate_slave_interrupts(pic: &mut Pic8259) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- Slave PIC Interrupts (IRQ 8-15) ---\n");

    // Raise IRQ 8 (RTC)
    pic.raise_irq(8)?;
    info!("  RTC raised IRQ 8");

    if let Some(vector) = pic.get_pending_interrupt() {
        info!(
            "  Pending interrupt: vector {:#x} (INT 0x28, slave IRQ 0)",
            vector
        );
        info!("  Note: Slave IRQ cascades through master IRQ 2");
        pic.acknowledge_interrupt(vector)?;

        // EOI to both master and slave
        pic.write(0xA0, &[0x20]).await?; // Slave EOI
        pic.write(0x20, &[0x20]).await?; // Master EOI
        info!("  EOI sent to both PICs\n");
    }

    Ok(())
}

async fn demonstrate_vm_interrupt_loop(
    pic: &mut Pic8259,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("--- VM Interrupt Loop (Simulated) ---\n");

    let backend = create_backend()?;
    let vcpu = VCpu::new(0);

    info!("Simulating VM execution with interrupts...\n");

    for tick in 0..5 {
        info!("Tick {}", tick);

        // Simulate timer interrupt every tick
        pic.raise_irq(0)?;
        info!("  Timer raised IRQ 0");

        // Check for pending interrupts
        if let Some(vector) = pic.get_pending_interrupt() {
            info!("  Injecting interrupt vector {:#x}", vector);

            // Inject interrupt into vCPU
            backend.inject_interrupt(&vcpu, vector).await?;

            // Acknowledge interrupt
            pic.acknowledge_interrupt(vector)?;

            // In a real VM, the guest would handle the interrupt
            // and send EOI when done
            pic.write(0x20, &[0x20]).await?;
            info!("  Interrupt delivered and EOI'd");
        }

        info!("");
    }

    info!("VM execution loop complete\n");

    Ok(())
}
