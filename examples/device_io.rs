//! Device I/O Example
//!
//! Demonstrates how to use the I/O handler system for device emulation.
//! This example shows:
//! - Registering I/O port handlers
//! - Registering MMIO handlers
//! - Running guest code with automatic I/O handling
//! - Multiple devices with different ports

use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
use hv2_core::hypervisor::HypervisorBackend;
use std::sync::{Arc, RwLock};

#[tokio::main]
async fn main() -> hv2_core::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Device I/O Example ===\n");

    // Create backend and VM
    let mut backend = WhpxBackend::new()?;
    backend.init().await?;

    let vm = backend.create_vm(1, 16 * 1024 * 1024).await?;
    println!("✓ Created VM with 16MB memory\n");

    // === Device 1: Bochs Debug Port (0xE9) ===
    println!("Registering Bochs debug port (0xE9)...");
    let debug_output = Arc::new(RwLock::new(String::new()));
    let debug_output_clone = debug_output.clone();

    vm.register_io_handler(
        0xE9,
        Box::new(move |_port, is_write, _size, data| {
            if is_write {
                let ch = (*data & 0xFF) as u8 as char;
                debug_output_clone.write().unwrap().push(ch);
                print!("{}", ch); // Echo to console
            }
            Ok(())
        }),
    );

    // === Device 2: POST Code Port (0x80) ===
    println!("Registering POST code port (0x80)...");
    let post_codes = Arc::new(RwLock::new(Vec::new()));
    let post_codes_clone = post_codes.clone();

    vm.register_io_handler(
        0x80,
        Box::new(move |_port, is_write, _size, data| {
            if is_write {
                let code = (*data & 0xFF) as u8;
                post_codes_clone.write().unwrap().push(code);
                println!("  [POST] Code: 0x{:02X}", code);
            }
            Ok(())
        }),
    );

    // === Device 3: Simple LED Register (0x90-0x93) ===
    println!("Registering LED device (0x90-0x93)...");
    let led_state = Arc::new(RwLock::new([false; 4]));
    
    for i in 0..4 {
        let led_state_clone = led_state.clone();
        let led_num = i;
        
        vm.register_io_handler(
            0x90 + i as u16,
            Box::new(move |_port, is_write, _size, data| {
                if is_write {
                    let on = (*data & 0x01) != 0;
                    led_state_clone.write().unwrap()[led_num] = on;
                    println!("  [LED] LED {} {}", led_num, if on { "ON" } else { "OFF" });
                } else {
                    *data = if led_state_clone.read().unwrap()[led_num] { 1 } else { 0 };
                }
                Ok(())
            }),
        );
    }

    println!("\n✓ All devices registered\n");

    // === Create vCPU ===
    let vcpu = vm.create_vcpu(0)?;

    // === Create Guest Code ===
    println!("Creating guest code...\n");
    
    let mut code = Vec::new();

    // POST code 0x01
    code.extend_from_slice(&[
        0xB0, 0x01, // MOV AL, 0x01
        0xE6, 0x80, // OUT 0x80, AL
    ]);

    // Write "Hello" to debug port
    for &ch in b"Hello from guest!\n" {
        code.extend_from_slice(&[
            0xB0, ch, // MOV AL, ch
            0xE6, 0xE9, // OUT 0xE9, AL
        ]);
    }

    // POST code 0x02
    code.extend_from_slice(&[
        0xB0, 0x02, // MOV AL, 0x02
        0xE6, 0x80, // OUT 0x80, AL
    ]);

    // Turn on LEDs 0 and 2
    code.extend_from_slice(&[
        0xB0, 0x01, // MOV AL, 1
        0xE6, 0x90, // OUT 0x90, AL (LED 0)
        0xB0, 0x01, // MOV AL, 1
        0xE6, 0x92, // OUT 0x92, AL (LED 2)
    ]);

    // POST code 0x03
    code.extend_from_slice(&[
        0xB0, 0x03, // MOV AL, 0x03
        0xE6, 0x80, // OUT 0x80, AL
    ]);

    // Write more text
    for &ch in b"Devices working!\n" {
        code.extend_from_slice(&[
            0xB0, ch, // MOV AL, ch
            0xE6, 0xE9, // OUT 0xE9, AL
        ]);
    }

    // Turn off LED 0, turn on LED 1
    code.extend_from_slice(&[
        0xB0, 0x00, // MOV AL, 0
        0xE6, 0x90, // OUT 0x90, AL (LED 0 off)
        0xB0, 0x01, // MOV AL, 1
        0xE6, 0x91, // OUT 0x91, AL (LED 1 on)
    ]);

    // POST code 0xFF (done)
    code.extend_from_slice(&[
        0xB0, 0xFF, // MOV AL, 0xFF
        0xE6, 0x80, // OUT 0x80, AL
    ]);

    // Final message
    for &ch in b"Execution complete!\n" {
        code.extend_from_slice(&[
            0xB0, ch, // MOV AL, ch
            0xE6, 0xE9, // OUT 0xE9, AL
        ]);
    }

    // HLT
    code.push(0xF4);

    // Load code into guest memory at 0x1000
    vm.write_guest_memory(0x1000, &code)?;
    println!("✓ Loaded {} bytes of guest code at 0x1000\n", code.len());

    // Set initial vCPU state
    let mut regs = vcpu.get_register_set()?;
    regs.rip = 0x1000;
    regs.rflags = 0x2; // Interrupt flag
    vcpu.set_register_set(&regs)?;

    // === Execute Guest Code ===
    println!("=== Executing Guest Code ===\n");
    println!("Guest output:");
    println!("---");

    match vcpu.run_with_handlers(&vm) {
        Ok(exit) => {
            println!("---");
            println!("\n✓ Guest execution completed: {:?}", exit);
        }
        Err(e) => {
            println!("---");
            println!("\n✗ Guest execution failed: {}", e);
            return Err(e);
        }
    }

    // === Display Results ===
    println!("\n=== Execution Results ===\n");

    let debug = debug_output.read().unwrap();
    println!("Debug output length: {} characters", debug.len());

    let posts = post_codes.read().unwrap();
    println!("POST codes received: {:?}", posts);

    let leds = led_state.read().unwrap();
    println!("\nFinal LED states:");
    for (i, &on) in leds.iter().enumerate() {
        println!("  LED {}: {}", i, if on { "ON 💡" } else { "OFF ⚫" });
    }

    println!("\n=== Example Complete ===");
    Ok(())
}
