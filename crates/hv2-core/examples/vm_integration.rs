//! End-to-end VM integration example - demonstrates VM execution with registered devices

use hv2_core::{Device, DeviceManager, SerialDevice, TimerDevice, VMConfig};
use parking_lot::RwLock;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("🚀 HV2 End-to-End Integration Example");
    println!("{}", "=".repeat(50));

    // Step 1: Show VM configuration concept
    println!("\n⚙️  VM Configuration Concept:");
    let mut config = VMConfig::default();
    config.vcpu_count = 1;
    config.memory_size = 64 * 1024 * 1024; // 64 MB
    println!("  • VCPUs: {}", config.vcpu_count);
    println!("  • Memory: {} MB", config.memory_size / (1024 * 1024));

    // Step 2: Create device manager
    println!("\n🔌 Setting up devices...");
    let device_manager = Arc::new(DeviceManager::new());

    // Create and register serial device (COM1)
    let serial = Arc::new(RwLock::new(SerialDevice::new("COM1".to_string(), 0x3F8)));
    device_manager.register_device("serial".to_string(), serial.clone())?;
    device_manager.register_io_port_range("serial".to_string(), 0x3F8, 0x3FF)?;
    serial.write().init().await?;
    println!("  ✓ Serial device (COM1) registered at 0x3F8-0x3FF");

    // Create and register timer device (PIT)
    let timer = Arc::new(RwLock::new(TimerDevice::new("PIT".to_string(), 0x40)));
    device_manager.register_device("timer".to_string(), timer.clone())?;
    device_manager.register_io_port_range("timer".to_string(), 0x40, 0x43)?;
    timer.write().init().await?;
    println!("  ✓ Timer device (PIT) registered at 0x40-0x43");

    // Step 3: Create VM (which has its own DeviceManager)
    // In a real scenario, the VM would use its internal DeviceManager
    // and devices would be registered before VM.start()
    println!("\n🖥️  VM Integration Concept:");
    println!("  • VM creates internal DeviceManager on initialization");
    println!("  • Devices are registered before VM.start()");
    println!("  • Exit handlers use VM's DeviceManager for I/O routing");

    // Step 4: Demonstrate device access through exit handlers
    println!("\n📤 Simulating device I/O through VM exits...");

    // Simulate I/O port write to serial device
    if let Some(handle) = device_manager.find_io_device(0x3F8) {
        println!("\n  Serial Device (COM1):");
        println!("    • Writing 'Hello, VM!' to serial port...");

        // Write each character
        for &byte in b"Hello, VM!\n" {
            handle.write_register(0, byte as u32).await?;
        }

        // Read output from serial device
        let output = serial.read().output_string();
        println!("    ✓ Serial output: {:?}", output.trim());
    }

    // Simulate I/O port write to timer device
    if let Some(handle) = device_manager.find_io_device(0x40) {
        println!("\n  Timer Device (PIT):");
        println!("    • Configuring timer for 1ms interrupts...");

        // Write control word (Channel 0, LSB/MSB, Mode 2, Binary)
        handle.write_register(3, 0x34).await?;
        println!("    ✓ Control word written: 0x34");

        // Set reload value to 1193 (1ms at 1.193182 MHz)
        let reload_value: u16 = 1193;
        handle
            .write_register(0, (reload_value & 0xFF) as u32)
            .await?; // LSB
        handle.write_register(0, (reload_value >> 8) as u32).await?; // MSB
        println!("    ✓ Reload value set: {} (1ms period)", reload_value);
    }

    // Step 5: Show exit handler integration points
    println!("\n🔄 VM Exit Handler Integration:");
    println!("  When VM executes I/O instructions:");
    println!("  1. VM exits with IoExit(port, direction, size)");
    println!("  2. Exit handler calls device_manager.find_io_device(port)");
    println!("  3. Device handle routes to registered device");
    println!("  4. Device processes I/O operation");
    println!("  5. VM resumes execution");

    println!("\n  When VM accesses MMIO:");
    println!("  1. VM exits with MmioExit(address, read/write)");
    println!("  2. Exit handler calls device_manager.find_mmio_device(address)");
    println!("  3. Device handle routes to registered device");
    println!("  4. Device processes MMIO operation");
    println!("  5. VM resumes execution");

    // Step 6: Test MMIO device registration
    println!("\n💾 Testing MMIO Device Registration...");
    let mmio_serial = Arc::new(RwLock::new(SerialDevice::new(
        "MMIO_SERIAL".to_string(),
        0x1000_0000,
    )));
    device_manager.register_device("mmio_serial".to_string(), mmio_serial.clone())?;
    device_manager.register_mmio_region("mmio_serial".to_string(), 0x1000_0000, 0x1000)?;
    mmio_serial.write().init().await?;
    println!("  ✓ MMIO serial device registered at 0x1000_0000-0x1000_1000");

    if let Some(handle) = device_manager.find_mmio_device(0x1000_0000) {
        println!("  ✓ Device found via MMIO lookup");
        handle.write_register(0, 0x41).await?; // 'A'
        println!("  ✓ Wrote byte via MMIO handle");
    }

    // Step 7: Show device statistics
    println!("\n📊 Device Statistics:");
    println!("  Serial (COM1):");
    println!("    - Base address: 0x{:X}", serial.read().base_address());
    println!(
        "    - Output buffer: {} bytes",
        serial.read().output_string().len()
    );

    println!("  Timer (PIT):");
    println!("    - Base address: 0x{:X}", timer.read().base_address());
    println!("    - Total ticks: {}", timer.read().total_ticks());
    println!(
        "    - Interrupts: {}",
        if timer.read().interrupt_enabled() {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    // Step 8: Cleanup
    println!("\n🛑 Shutting down...");
    device_manager.shutdown_all().await?;
    println!("  ✓ All devices shut down");

    println!("\n✅ End-to-end integration example completed!");
    println!("\n📝 Key Takeaways:");
    println!("  • Device registration separates device creation from VM integration");
    println!("  • Exit handlers use device_manager to route I/O to registered devices");
    println!("  • Device handles provide safe, scoped access to device operations");
    println!("  • Overlap detection prevents conflicting device registrations");
    println!("  • Both I/O ports and MMIO regions are supported");

    Ok(())
}
