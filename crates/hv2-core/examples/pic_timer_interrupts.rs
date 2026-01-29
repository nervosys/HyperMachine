//! PIC Timer Interrupt Example
//!
//! This example demonstrates the complete interrupt flow:
//! 1. Timer device raises IRQ 0
//! 2. PIC queues the interrupt
//! 3. vCPU execution loop checks for pending interrupts
//! 4. Interrupt is injected into the vCPU
//! 5. Guest code handles the interrupt via IDT
//!
//! Run with: cargo run --example pic_timer_interrupts

use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
use hv2_core::interrupt::Pic8259;
use hv2_core::Result;

fn main() -> Result<()> {
    println!("=== PIC Timer Interrupt Example ===\n");

    // Step 1: Create hypervisor backend and VM
    println!("1. Creating hypervisor backend...");
    let _backend = WhpxBackend::new()?;
    println!("   ✓ Backend created");

    println!("2. Creating VM (1 vCPU, 16MB RAM)...");
    let vm = match WhpxVm::new(1, 16 * 1024 * 1024) {
        Ok(vm) => vm,
        Err(_) => {
            println!("   ⚠ VM creation failed (hypervisor may be unavailable)");
            println!("   → Continuing with PIC-only demonstration\n");
            return demonstrate_pic_only();
        }
    };
    println!("   ✓ VM created");

    // Step 2: Create and initialize PIC
    println!("\n3. Creating 8259 PIC...");
    let pic = Pic8259::new();
    println!("   ✓ PIC created");

    // Step 3: Register PIC I/O handlers
    println!("\n4. Registering PIC I/O handlers...");
    for port in [0x20, 0x21, 0xA0, 0xA1] {
        let handler = pic.create_io_handler();
        vm.register_io_handler(port, handler);
        println!("   ✓ Handler registered for port 0x{:02X}", port);
    }

    // Step 4: Initialize PIC via I/O ports (ICW sequence)
    println!("\n5. Initializing PIC (ICW sequence)...");

    // Master PIC initialization
    let mut data = 0x11; // ICW1: Init + ICW4 needed
    vm.handle_io_access(0x20, true, 1, &mut data)?;
    println!("   ✓ ICW1: Initialization command");

    data = 0x20; // ICW2: Vector offset 0x20
    vm.handle_io_access(0x21, true, 1, &mut data)?;
    println!("   ✓ ICW2: Vector offset 0x20 (IRQ 0 -> INT 0x20)");

    data = 0x04; // ICW3: Slave on IRQ2
    vm.handle_io_access(0x21, true, 1, &mut data)?;
    println!("   ✓ ICW3: Slave PIC on IRQ 2");

    data = 0x01; // ICW4: 8086 mode
    vm.handle_io_access(0x21, true, 1, &mut data)?;
    println!("   ✓ ICW4: 8086/8088 mode");

    // Slave PIC initialization
    data = 0x11; // ICW1
    vm.handle_io_access(0xA0, true, 1, &mut data)?;

    data = 0x28; // ICW2: Vector offset 0x28
    vm.handle_io_access(0xA1, true, 1, &mut data)?;

    data = 0x02; // ICW3: Cascade identity
    vm.handle_io_access(0xA1, true, 1, &mut data)?;

    data = 0x01; // ICW4: 8086 mode
    vm.handle_io_access(0xA1, true, 1, &mut data)?;
    println!("   ✓ Slave PIC initialized");

    // Step 5: Configure interrupt masks
    println!("\n6. Configuring interrupt masks...");
    data = 0xFE; // Mask all except IRQ 0 (timer)
    vm.handle_io_access(0x21, true, 1, &mut data)?;
    println!("   ✓ Master mask: 0xFE (IRQ 0 enabled)");

    data = 0xFF; // Mask all slave interrupts
    vm.handle_io_access(0xA1, true, 1, &mut data)?;
    println!("   ✓ Slave mask: 0xFF (all masked)");

    // Step 6: Simulate timer raising IRQ 0
    println!("\n7. Simulating timer tick (raising IRQ 0)...");
    pic.raise_irq(0)?;
    println!("   ✓ IRQ 0 raised by timer");

    // Step 7: Check for pending interrupt
    println!("\n8. Checking PIC for pending interrupts...");
    if let Some(vector) = pic.get_pending_interrupt() {
        println!("   ✓ Pending interrupt found!");
        println!("     - Vector: 0x{:02X}", vector);
        println!("     - IRQ: {}", vector - 0x20);
        println!("     - Type: Timer interrupt");

        // Step 8: Acknowledge interrupt (normally done after vCPU handles it)
        println!("\n9. Acknowledging interrupt...");
        pic.acknowledge_interrupt(0)?;
        println!("   ✓ IRQ 0 acknowledged");

        // Verify no more pending interrupts
        if pic.get_pending_interrupt().is_none() {
            println!("   ✓ No more pending interrupts");
        }
    } else {
        println!("   ✗ No pending interrupt (masked?)");
    }

    // Step 9: Demonstrate multiple interrupts
    println!("\n10. Demonstrating interrupt priority...");
    pic.raise_irq(0)?; // Timer
    pic.raise_irq(1)?; // Keyboard
    pic.raise_irq(2)?; // Cascade (should be masked)
    println!("    ✓ Raised IRQs 0, 1, 2");

    // Check which interrupt has highest priority
    if let Some(vector) = pic.get_pending_interrupt() {
        let irq = vector - 0x20;
        println!(
            "    ✓ Highest priority: IRQ {} (vector 0x{:02X})",
            irq, vector
        );

        // Note about priority
        if irq == 0 {
            println!("      (IRQ 0 has highest priority)");
        }
    }

    // Step 10: Read interrupt masks back
    println!("\n11. Verifying PIC state...");
    data = 0;
    vm.handle_io_access(0x21, false, 1, &mut data)?;
    println!("    ✓ Master IMR: 0x{:02X}", data as u8);

    data = 0;
    vm.handle_io_access(0xA1, false, 1, &mut data)?;
    println!("    ✓ Slave IMR: 0x{:02X}", data as u8);

    println!("\n=== Complete Interrupt Flow ===");
    println!("Device (Timer) → raise_irq(0)");
    println!("       ↓");
    println!("PIC → Queue in IRR, check IMR");
    println!("       ↓");
    println!("PIC → get_pending_interrupt() → 0x20");
    println!("       ↓");
    println!("vCPU → inject_interrupt(0x20)");
    println!("       ↓");
    println!("Guest → IDT[0x20] handler executes");
    println!("       ↓");
    println!("Guest → OUT 0x20, 0x20 (EOI)");
    println!("       ↓");
    println!("PIC → acknowledge_interrupt(0)");
    println!("\n✓ Example complete!");

    Ok(())
}

/// Demonstrate PIC functionality without a full VM
fn demonstrate_pic_only() -> Result<()> {
    println!("=== PIC-Only Demonstration ===\n");

    // Create PIC
    println!("1. Creating 8259 PIC...");
    let pic = Pic8259::new();
    println!("   ✓ PIC created (default state)");

    // Simulate timer raising IRQ 0
    println!("\n2. Simulating timer tick (raising IRQ 0)...");
    pic.raise_irq(0)?;
    println!("   ✓ IRQ 0 raised by timer device");

    // Check for pending interrupt
    println!("\n3. Checking for pending interrupts...");
    if let Some(vector) = pic.get_pending_interrupt() {
        println!("   ✓ Pending interrupt found!");
        println!("     - Vector: 0x{:02X}", vector);
        println!("     - IRQ: {}", vector - 0x20);
        println!("     - Type: Timer interrupt (IRQ 0)");
    } else {
        println!("   ✗ No pending interrupt (masked by default)");
        println!("   → IRQ 0 is masked in default PIC state");
    }

    // Unmask IRQ 0
    println!("\n4. Unmasking IRQ 0...");
    pic.set_master_mask(0xFE); // Unmask bit 0 (IRQ 0)
    println!("   ✓ Master mask set to 0xFE (IRQ 0 enabled)");

    // Raise IRQ 0 again
    println!("\n5. Raising IRQ 0 again (now unmasked)...");
    pic.raise_irq(0)?;
    println!("   ✓ IRQ 0 raised");

    // Check for pending interrupt
    if let Some(vector) = pic.get_pending_interrupt() {
        println!("\n6. Pending interrupt detected!");
        println!("   ✓ Vector: 0x{:02X} (IRQ {})", vector, vector - 0x20);

        // Acknowledge the interrupt
        println!("\n7. Acknowledging interrupt...");
        pic.acknowledge_interrupt(vector)?; // Pass vector, not IRQ
        println!("   ✓ Interrupt acknowledged");

        // Verify no more pending
        if pic.get_pending_interrupt().is_none() {
            println!("   ✓ No more pending interrupts");
        }
    }

    // Demonstrate interrupt priority
    println!("\n8. Demonstrating interrupt priority...");
    pic.raise_irq(3)?; // COM2
    pic.raise_irq(1)?; // Keyboard (higher priority)
    pic.raise_irq(4)?; // COM1
    println!("   ✓ Raised IRQs: 1 (keyboard), 3 (COM2), 4 (COM1)");

    if let Some(vector) = pic.get_pending_interrupt() {
        let irq = vector - 0x20;
        println!(
            "   ✓ Highest priority interrupt: IRQ {} (vector 0x{:02X})",
            irq, vector
        );
        if irq == 1 {
            println!("     (IRQ 1 has highest priority among raised IRQs)");
        }
    }

    // Demonstrate cascading (slave PIC)
    println!("\n9. Demonstrating slave PIC (IRQs 8-15)...");
    pic.raise_irq(8)?; // RTC
    println!("   ✓ Raised IRQ 8 (RTC on slave PIC)");

    if let Some(vector) = pic.get_pending_interrupt() {
        let irq = if vector >= 0x28 {
            vector - 0x28 + 8
        } else {
            vector - 0x20
        };
        println!("   ✓ Pending: IRQ {} (vector 0x{:02X})", irq, vector);
    }

    println!("\n=== Complete Interrupt Flow ===");
    println!("┌─────────────────┐");
    println!("│ Timer Device    │ 18.2 Hz tick");
    println!("└────────┬────────┘");
    println!("         │");
    println!("         │ raise_irq(0)");
    println!("         ▼");
    println!("┌─────────────────┐");
    println!("│ 8259 PIC        │");
    println!("│  - Set IRR bit  │ Interrupt Request Register");
    println!("│  - Check IMR    │ Interrupt Mask Register");
    println!("│  - Set ISR bit  │ In-Service Register");
    println!("└────────┬────────┘");
    println!("         │");
    println!("         │ get_pending_interrupt() → 0x20");
    println!("         ▼");
    println!("┌─────────────────┐");
    println!("│ vCPU            │");
    println!("│  inject_int(32) │ Vector 0x20 = IRQ 0");
    println!("└────────┬────────┘");
    println!("         │");
    println!("         │ IDT lookup");
    println!("         ▼");
    println!("┌─────────────────┐");
    println!("│ Guest OS        │");
    println!("│  Timer Handler  │ Execute ISR");
    println!("│  OUT 0x20, 0x20 │ Send EOI");
    println!("└────────┬────────┘");
    println!("         │");
    println!("         │ acknowledge_interrupt(0)");
    println!("         ▼");
    println!("┌─────────────────┐");
    println!("│ 8259 PIC        │");
    println!("│  - Clear ISR    │ Ready for next interrupt");
    println!("└─────────────────┘");

    println!("\n✓ PIC demonstration complete!");
    println!("\nKey Concepts:");
    println!("• IRQ 0-7: Master PIC (vectors 0x20-0x27)");
    println!("• IRQ 8-15: Slave PIC (vectors 0x28-0x2F)");
    println!("• Lower IRQ numbers have higher priority");
    println!("• IMR (mask) controls which IRQs can trigger interrupts");
    println!("• ISR tracks which interrupt is currently being serviced");

    Ok(())
}
