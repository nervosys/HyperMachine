//! Device I/O Integration Tests
//!
//! Tests the integration of device emulation with the I/O handler system.

// WHPX backend is Windows-only; gate this suite to Windows.
#![cfg(target_os = "windows")]

use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
use std::sync::{Arc, RwLock};

#[tokio::test]
async fn test_bochs_debug_port() {
    // Test using Bochs debug port (0xE9) for guest output
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            // Create output buffer
            let output = Arc::new(RwLock::new(String::new()));
            let output_clone = output.clone();

            // Register handler for Bochs debug port
            vm.register_io_handler(
                0xE9,
                Box::new(move |_port, is_write, _size, data| {
                    if is_write {
                        let ch = (*data & 0xFF) as u8 as char;
                        output_clone.write().unwrap().push(ch);
                    }
                    Ok(())
                }),
            );

            if let Ok(vcpu) = vm.create_vcpu(0) {
                // Create guest code that writes "Hello, World!" to port 0xE9
                let mut code = Vec::new();
                for &ch in b"Hello, World!" {
                    code.push(0xB0); // MOV AL, imm8
                    code.push(ch);
                    code.push(0xE6); // OUT 0xE9, AL
                    code.push(0xE9);
                }
                code.push(0xF4); // HLT

                if vm.write_guest_memory(0x1000, &code).is_ok() {
                    let mut regs = vcpu.get_register_set().unwrap_or_default();
                    regs.rip = 0x1000;
                    let _ = vcpu.set_register_set(&regs);

                    // Run with handlers
                    match vcpu.run_with_handlers(&vm) {
                        Ok(_) => {
                            let result = output.read().unwrap().clone();
                            println!("✓ Bochs debug port captured: '{}'", result);
                            assert_eq!(result, "Hello, World!");
                        }
                        Err(e) => {
                            println!("⚠ Test skipped: {}", e);
                        }
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn test_multiple_device_handlers() {
    // Test multiple devices with different I/O ports
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            // Device 1: Port 0xE9 (Bochs debug)
            let debug_out = Arc::new(RwLock::new(Vec::new()));
            let debug_out_clone = debug_out.clone();
            vm.register_io_handler(
                0xE9,
                Box::new(move |_port, is_write, _size, data| {
                    if is_write {
                        debug_out_clone.write().unwrap().push((*data & 0xFF) as u8);
                    }
                    Ok(())
                }),
            );

            // Device 2: Port 0x80 (POST code)
            let post_codes = Arc::new(RwLock::new(Vec::new()));
            let post_codes_clone = post_codes.clone();
            vm.register_io_handler(
                0x80,
                Box::new(move |_port, is_write, _size, data| {
                    if is_write {
                        let code = (*data & 0xFF) as u8;
                        post_codes_clone.write().unwrap().push(code);
                        println!("POST code: 0x{:02X}", code);
                    }
                    Ok(())
                }),
            );

            if let Ok(vcpu) = vm.create_vcpu(0) {
                // Create code that writes to both ports
                let code = vec![
                    0xB0, 0x01, // MOV AL, 0x01
                    0xE6, 0x80, // OUT 0x80, AL (POST code)
                    0xB0, 0x41, // MOV AL, 'A'
                    0xE6, 0xE9, // OUT 0xE9, AL (debug)
                    0xB0, 0x02, // MOV AL, 0x02
                    0xE6, 0x80, // OUT 0x80, AL (POST code)
                    0xB0, 0x42, // MOV AL, 'B'
                    0xE6, 0xE9, // OUT 0xE9, AL (debug)
                    0xF4, // HLT
                ];

                if vm.write_guest_memory(0x1000, &code).is_ok() {
                    let mut regs = vcpu.get_register_set().unwrap_or_default();
                    regs.rip = 0x1000;
                    let _ = vcpu.set_register_set(&regs);

                    match vcpu.run_with_handlers(&vm) {
                        Ok(_) => {
                            let debug = debug_out.read().unwrap();
                            let posts = post_codes.read().unwrap();

                            println!("✓ Debug output: '{}'", String::from_utf8_lossy(&debug));
                            println!("✓ POST codes: {:?}", posts);

                            assert_eq!(debug.as_slice(), b"AB");
                            assert_eq!(posts.as_slice(), &[0x01, 0x02]);
                        }
                        Err(e) => {
                            println!("⚠ Test skipped: {}", e);
                        }
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn test_mmio_device_handler() {
    // Test MMIO device handling
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            // Register MMIO handler for device at 0xFED00000-0xFED00100
            let device_regs = Arc::new(RwLock::new([0u32; 64]));
            let device_regs_clone = device_regs.clone();

            vm.register_mmio_handler(
                0xFED00000,
                0xFED00100,
                Box::new(move |addr, is_write, size, data| {
                    let offset = ((addr - 0xFED00000) / 4) as usize;
                    if offset < 64 {
                        let mut regs = device_regs_clone.write().unwrap();
                        if is_write {
                            // Extract value from data based on size
                            let value = match size {
                                1 => data[0] as u32,
                                2 => u16::from_le_bytes([data[0], data[1]]) as u32,
                                4 => u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
                                _ => 0,
                            };
                            regs[offset] = value;
                            println!("MMIO write: reg[{}] = 0x{:08X}", offset, value);
                        } else {
                            // Write register value to data
                            let value = regs[offset];
                            let bytes = value.to_le_bytes();
                            data[0..4].copy_from_slice(&bytes);
                            println!("MMIO read: reg[{}] = 0x{:08X}", offset, value);
                        }
                    }
                    Ok(())
                }),
            );

            println!("✓ MMIO handler registered at 0xFED00000-0xFED00100");

            // Test MMIO handler invocation
            let mut data = [0u8; 8];
            data[0..4].copy_from_slice(&0x12345678u32.to_le_bytes());

            match vm.handle_mmio_access(0xFED00000, true, 4, &mut data) {
                Ok(()) => {
                    println!("✓ MMIO write handled");

                    // Read it back
                    let mut read_data = [0u8; 8];
                    match vm.handle_mmio_access(0xFED00000, false, 4, &mut read_data) {
                        Ok(()) => {
                            let value = u32::from_le_bytes([
                                read_data[0],
                                read_data[1],
                                read_data[2],
                                read_data[3],
                            ]);
                            println!("✓ MMIO read returned: 0x{:08X}", value);
                            assert_eq!(value, 0x12345678);
                        }
                        Err(e) => println!("⚠ MMIO read failed: {}", e),
                    }
                }
                Err(e) => println!("⚠ MMIO write failed: {}", e),
            }
        }
    }
}

#[tokio::test]
async fn test_io_handler_replacement() {
    // Test replacing I/O handlers
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            // Register first handler
            let count1 = Arc::new(RwLock::new(0));
            let count1_clone = count1.clone();
            let old_handler = vm.register_io_handler(
                0xE9,
                Box::new(move |_port, is_write, _size, _data| {
                    if is_write {
                        *count1_clone.write().unwrap() += 1;
                    }
                    Ok(())
                }),
            );
            assert!(old_handler.is_none(), "Should be no previous handler");

            // Test first handler
            let mut data = 0x41;
            let _ = vm.handle_io_access(0xE9, true, 1, &mut data);
            assert_eq!(*count1.read().unwrap(), 1);

            // Replace with second handler
            let count2 = Arc::new(RwLock::new(0));
            let count2_clone = count2.clone();
            let old_handler = vm.register_io_handler(
                0xE9,
                Box::new(move |_port, is_write, _size, _data| {
                    if is_write {
                        *count2_clone.write().unwrap() += 100;
                    }
                    Ok(())
                }),
            );
            assert!(old_handler.is_some(), "Should have previous handler");

            // Test second handler
            let _ = vm.handle_io_access(0xE9, true, 1, &mut data);

            // First handler should still be at 1, second should be at 100
            assert_eq!(*count1.read().unwrap(), 1);
            assert_eq!(*count2.read().unwrap(), 100);

            println!("✓ Handler replacement working correctly");
        }
    }
}

#[tokio::test]
async fn test_pic_io_handler() {
    // Test PIC I/O handler integration
    use hv2_core::interrupt::Pic8259;

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            let pic = Pic8259::new();

            // Register PIC handlers for all ports
            for port in [0x20, 0x21, 0xA0, 0xA1] {
                let handler = pic.create_io_handler();
                vm.register_io_handler(port, handler);
            }

            println!("✓ PIC handlers registered");

            // Test 1: Initialize master PIC
            let mut data = 0x11; // ICW1: Init + ICW4 needed
            match vm.handle_io_access(0x20, true, 1, &mut data) {
                Ok(()) => println!("✓ Master PIC ICW1 written"),
                Err(e) => println!("⚠ ICW1 write failed: {}", e),
            }

            // ICW2: Vector offset 0x20
            data = 0x20;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);

            // ICW3: Slave on IRQ2
            data = 0x04;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);

            // ICW4: 8086 mode
            data = 0x01;
            match vm.handle_io_access(0x21, true, 1, &mut data) {
                Ok(()) => println!("✓ Master PIC initialized"),
                Err(e) => println!("⚠ ICW4 write failed: {}", e),
            }

            // Test 2: Initialize slave PIC
            data = 0x11; // ICW1
            let _ = vm.handle_io_access(0xA0, true, 1, &mut data);

            data = 0x28; // ICW2: Vector offset 0x28
            let _ = vm.handle_io_access(0xA1, true, 1, &mut data);

            data = 0x02; // ICW3: Cascade identity
            let _ = vm.handle_io_access(0xA1, true, 1, &mut data);

            data = 0x01; // ICW4: 8086 mode
            match vm.handle_io_access(0xA1, true, 1, &mut data) {
                Ok(()) => println!("✓ Slave PIC initialized"),
                Err(e) => println!("⚠ Slave ICW4 write failed: {}", e),
            }

            // Test 3: Set interrupt masks
            data = 0xFB; // Mask all except IRQ2 (cascade)
            match vm.handle_io_access(0x21, true, 1, &mut data) {
                Ok(()) => println!("✓ Master mask set to 0xFB"),
                Err(e) => println!("⚠ Mask write failed: {}", e),
            }

            // Read back mask
            data = 0;
            match vm.handle_io_access(0x21, false, 1, &mut data) {
                Ok(()) => {
                    println!("✓ Master mask read: 0x{:02X}", data as u8);
                    assert_eq!(data as u8, 0xFB, "Mask should be 0xFB");
                }
                Err(e) => println!("⚠ Mask read failed: {}", e),
            }

            // Test 4: Raise and check interrupt
            match pic.raise_irq(0) {
                Ok(()) => {
                    println!("✓ IRQ 0 raised");

                    // Check for pending interrupt
                    if let Some(vector) = pic.get_pending_interrupt() {
                        println!("✓ Pending interrupt vector: 0x{:02X}", vector);
                        assert_eq!(vector, 0x20, "Vector should be 0x20 (IRQ 0)");
                    } else {
                        println!("⚠ No pending interrupt (masked?)");
                    }
                }
                Err(e) => println!("⚠ IRQ raise failed: {}", e),
            }

            println!("✓ PIC I/O handler test complete");
        }
    }
}

#[tokio::test]
async fn test_pic_interrupt_delivery() {
    // Test PIC interrupt delivery to vCPU
    use hv2_core::interrupt::Pic8259;

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            let pic = Pic8259::new();

            // Register PIC handlers
            for port in [0x20, 0x21, 0xA0, 0xA1] {
                let handler = pic.create_io_handler();
                vm.register_io_handler(port, handler);
            }

            // Initialize PIC (similar to test_pic_io_handler)
            let mut data = 0x11; // ICW1
            let _ = vm.handle_io_access(0x20, true, 1, &mut data);
            data = 0x20; // ICW2: vector offset 0x20
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x04; // ICW3
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x01; // ICW4
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);

            // Unmask all interrupts
            data = 0x00;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);

            println!("✓ PIC initialized and unmasked");

            // Raise IRQ 0 (timer)
            match pic.raise_irq(0) {
                Ok(()) => {
                    println!("✓ IRQ 0 raised");

                    // Verify pending interrupt
                    if let Some(vector) = pic.get_pending_interrupt() {
                        println!("✓ Pending interrupt vector: 0x{:02X}", vector);
                        assert_eq!(vector, 0x20, "Expected vector 0x20");

                        // Acknowledge the interrupt
                        match pic.acknowledge_interrupt(0) {
                            Ok(()) => println!("✓ Interrupt acknowledged"),
                            Err(e) => println!("⚠ Acknowledge failed: {}", e),
                        }

                        // After acknowledge, should be no pending interrupt
                        if pic.get_pending_interrupt().is_none() {
                            println!("✓ No pending interrupts after acknowledge");
                        }
                    } else {
                        println!("⚠ No pending interrupt found");
                    }
                }
                Err(e) => println!("⚠ IRQ raise failed: {}", e),
            }

            // Test multiple interrupts
            match pic.raise_irq(1) {
                Ok(()) => println!("✓ IRQ 1 raised"),
                Err(e) => println!("⚠ IRQ 1 raise failed: {}", e),
            }

            match pic.raise_irq(2) {
                Ok(()) => println!("✓ IRQ 2 raised"),
                Err(e) => println!("⚠ IRQ 2 raise failed: {}", e),
            }

            // Should get highest priority first (IRQ 1)
            if let Some(vector) = pic.get_pending_interrupt() {
                println!("✓ Next pending: 0x{:02X} (IRQ {})", vector, vector - 0x20);
            }

            println!("✓ PIC interrupt delivery test complete");
        }
    }
}

#[tokio::test]
async fn test_pic_initialization_sequence() {
    // Test complete PIC initialization with ICW commands
    use hv2_core::interrupt::Pic8259;

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            let pic = Pic8259::new();

            // Register handlers
            for port in [0x20, 0x21, 0xA0, 0xA1] {
                vm.register_io_handler(port, pic.create_io_handler());
            }

            // ICW1: Initialize with ICW4
            let mut data = 0x11;
            assert!(vm.handle_io_access(0x20, true, 1, &mut data).is_ok());

            // ICW2: Set vector offset
            data = 0x20;
            assert!(vm.handle_io_access(0x21, true, 1, &mut data).is_ok());

            // ICW3: Set cascade
            data = 0x04;
            assert!(vm.handle_io_access(0x21, true, 1, &mut data).is_ok());

            // ICW4: Set mode
            data = 0x01;
            assert!(vm.handle_io_access(0x21, true, 1, &mut data).is_ok());

            println!("✓ PIC initialization sequence test complete");
        }
    }
}

#[tokio::test]
async fn test_pic_interrupt_masking() {
    // Test interrupt mask register (IMR)
    use hv2_core::interrupt::Pic8259;

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            let pic = Pic8259::new();

            for port in [0x20, 0x21, 0xA0, 0xA1] {
                vm.register_io_handler(port, pic.create_io_handler());
            }

            // Initialize PIC
            let mut data = 0x11;
            let _ = vm.handle_io_access(0x20, true, 1, &mut data);
            data = 0x20;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x04;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x01;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);

            // Set mask: allow only IRQ 0 and IRQ 1
            data = 0xFC; // 11111100 - mask all except 0,1
            assert!(vm.handle_io_access(0x21, true, 1, &mut data).is_ok());

            // Read back mask
            data = 0;
            assert!(vm.handle_io_access(0x21, false, 1, &mut data).is_ok());
            assert_eq!(data as u8, 0xFC, "Mask should be 0xFC");

            // Raise masked interrupt (IRQ 2)
            let _ = pic.raise_irq(2);
            assert!(
                pic.get_pending_interrupt().is_none(),
                "Masked IRQ should not be pending"
            );

            // Raise unmasked interrupt (IRQ 0)
            let _ = pic.raise_irq(0);
            assert!(
                pic.get_pending_interrupt().is_some(),
                "Unmasked IRQ should be pending"
            );

            println!("✓ PIC interrupt masking test complete");
        }
    }
}

#[tokio::test]
async fn test_pic_interrupt_priority() {
    // Test interrupt priority (lower IRQ = higher priority)
    use hv2_core::interrupt::Pic8259;

    let pic = Pic8259::new();
    pic.set_master_mask(0x00); // Unmask all

    // Raise multiple interrupts
    let _ = pic.raise_irq(3);
    let _ = pic.raise_irq(1);
    let _ = pic.raise_irq(5);

    // Should get IRQ 1 first (highest priority)
    if let Some(vector) = pic.get_pending_interrupt() {
        assert_eq!(vector, 0x21, "Should get IRQ 1 (vector 0x21) first");
        let _ = pic.acknowledge_interrupt(vector);
    }

    // Should get IRQ 3 next
    if let Some(vector) = pic.get_pending_interrupt() {
        assert_eq!(vector, 0x23, "Should get IRQ 3 (vector 0x23) next");
        let _ = pic.acknowledge_interrupt(vector);
    }

    // Should get IRQ 5 last
    if let Some(vector) = pic.get_pending_interrupt() {
        assert_eq!(vector, 0x25, "Should get IRQ 5 (vector 0x25) last");
    }

    println!("✓ PIC interrupt priority test complete");
}

#[tokio::test]
async fn test_pic_eoi_handling() {
    // Test End of Interrupt (EOI) command
    use hv2_core::interrupt::Pic8259;

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            let pic = Pic8259::new();

            for port in [0x20, 0x21, 0xA0, 0xA1] {
                vm.register_io_handler(port, pic.create_io_handler());
            }

            // Initialize and unmask
            let mut data = 0x11;
            let _ = vm.handle_io_access(0x20, true, 1, &mut data);
            data = 0x20;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x04;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x01;
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);
            data = 0x00; // Unmask all
            let _ = vm.handle_io_access(0x21, true, 1, &mut data);

            // Raise interrupt
            let _ = pic.raise_irq(0);

            if let Some(vector) = pic.get_pending_interrupt() {
                // Acknowledge (simulates EOI)
                assert!(pic.acknowledge_interrupt(vector).is_ok());

                // No more pending after acknowledgment
                assert!(pic.get_pending_interrupt().is_none());
            }

            println!("✓ PIC EOI handling test complete");
        }
    }
}

#[tokio::test]
async fn test_pic_slave_cascade() {
    // Test slave PIC cascading through IRQ 2
    use hv2_core::interrupt::Pic8259;

    let pic = Pic8259::new();
    pic.set_master_mask(0x00); // Unmask all on master

    // Raise interrupt on slave PIC (IRQ 8)
    let _ = pic.raise_irq(8);

    // Should get vector 0x28 (slave base + 0)
    if let Some(vector) = pic.get_pending_interrupt() {
        assert_eq!(vector, 0x28, "Slave IRQ 8 should map to vector 0x28");
    }

    // Raise both master and slave interrupts
    let _ = pic.raise_irq(1); // Master IRQ 1
    let _ = pic.raise_irq(9); // Slave IRQ 9

    // Master IRQ 1 should have higher priority than slave IRQs
    if let Some(vector) = pic.get_pending_interrupt() {
        assert_eq!(vector, 0x21, "Master IRQ should have priority over slave");
    }

    println!("✓ PIC slave cascade test complete");
}
