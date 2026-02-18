//! Device Interrupt Example
//!
//! This example demonstrates how devices (Timer and Serial) generate interrupts
//! through the PIC and how they are delivered to the guest.

use hv2_core::{Pic8259, SerialDevice, TimerDevice};
use std::sync::Arc;
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("=== HyperMachine Device Interrupt Demo ===\n");

    // Create PIC
    let pic = Arc::new(Pic8259::new());
    info!("Created Intel 8259 PIC\n");

    // Create timer device and connect to PIC
    let mut timer = TimerDevice::new("PIT".to_string(), 0x40);
    timer.set_pic(pic.clone());
    info!("Created timer device at I/O port 0x40");
    info!("  Connected to PIC IRQ 0\n");

    // Create serial device and connect to PIC
    let mut serial = SerialDevice::new("COM1".to_string(), 0x3F8);
    serial.set_pic(pic.clone());
    info!("Created serial device at I/O port 0x3F8");
    info!("  Connected to PIC IRQ 4\n");

    // Demonstrate timer interrupts
    info!("--- Timer Interrupts ---\n");
    timer.set_interrupt_enabled(true);
    info!("  Timer interrupts enabled");

    for i in 0..3 {
        timer.tick()?;
        info!("  Timer tick {}", i);

        if let Some(vector) = pic.get_pending_interrupt() {
            info!("  → Interrupt pending: vector {:#x} (IRQ 0)", vector);
            pic.acknowledge_interrupt(vector)?;
            info!("  → Interrupt acknowledged and EOI'd\n");
        }
    }

    info!("Total timer ticks: {}\n", timer.total_ticks());

    // Demonstrate serial interrupts
    info!("--- Serial Interrupts ---\n");
    info!("  Simulating incoming data: \"Hello\"");
    serial.input(b"Hello")?;

    if let Some(vector) = pic.get_pending_interrupt() {
        info!("  → Interrupt pending: vector {:#x} (IRQ 4)", vector);
        pic.acknowledge_interrupt(vector)?;
        info!("  → Interrupt acknowledged\n");
    }

    // Demonstrate interrupt priorities
    info!("--- Interrupt Priorities ---\n");
    timer.tick()?;
    serial.input(b"X")?;
    info!("  Generated IRQ 0 (timer) and IRQ 4 (serial)");

    // First interrupt should be IRQ 0 (higher priority)
    if let Some(vector) = pic.get_pending_interrupt() {
        info!(
            "  → First interrupt: vector {:#x} (IRQ 0 - higher priority)",
            vector
        );
        pic.acknowledge_interrupt(vector)?;
    }

    // Second interrupt should be IRQ 4
    if let Some(vector) = pic.get_pending_interrupt() {
        info!("  → Second interrupt: vector {:#x} (IRQ 4)\n", vector);
        pic.acknowledge_interrupt(vector)?;
    }

    info!("=== Demo Complete ===");

    Ok(())
}
