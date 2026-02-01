//! Device registration example - demonstrates the new DeviceManager API

use hv2_core::{Device, DeviceManager, SerialDevice, TimerDevice};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("🔌 HV2 Device Registration Example");
    println!("{}", "=".repeat(50));

    // Create device manager
    let manager = DeviceManager::new();
    println!("\n✓ Device manager created");

    // Create serial device (COM1)
    println!("\n📟 Creating Serial Device (COM1)...");
    let serial: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(SerialDevice::new("COM1".to_string(), 0x3F8)));

    // Register the serial device
    manager
        .register_device("serial".to_string(), serial.clone())
        .await?;
    println!("  ✓ Device registered: 'serial'");

    // Register I/O ports for serial device (0x3F8-0x3FF)
    manager
        .register_io_port_range("serial".to_string(), 0x3F8, 0x3FF)
        .await?;
    println!("  ✓ I/O ports registered: 0x3F8-0x3FF");

    // Initialize the device
    serial.write().await.init().await?;
    println!("  ✓ Device initialized");

    // Create timer device (PIT)
    println!("\n⏱️  Creating Timer Device (PIT)...");
    let timer: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TimerDevice::new("PIT".to_string(), 0x40)));

    // Register the timer device
    manager
        .register_device("timer".to_string(), timer.clone())
        .await?;
    println!("  ✓ Device registered: 'timer'");

    // Register I/O ports for timer device (0x40-0x43)
    manager
        .register_io_port_range("timer".to_string(), 0x40, 0x43)
        .await?;
    println!("  ✓ I/O ports registered: 0x40-0x43");

    // Initialize the device
    timer.write().await.init().await?;
    println!("  ✓ Device initialized");

    // Demonstrate device lookup by I/O port
    println!("\n🔍 Testing Device Lookup...");

    if let Some(handle) = manager.find_io_device(0x3F8).await {
        println!("  ✓ Found device at port 0x3F8: {}", handle.device_name());

        // Write to serial port
        handle.write_register(0, 0x48).await?; // 'H'
        println!("  ✓ Wrote byte to serial port");
    }

    if let Some(handle) = manager.find_io_device(0x40).await {
        println!("  ✓ Found device at port 0x40: {}", handle.device_name());

        // Write to timer control register
        handle.write_register(3, 0x34).await?; // Control word
        println!("  ✓ Wrote control word to timer");
    }

    // Test lookup at unregistered port
    if manager.find_io_device(0x1000).await.is_none() {
        println!("  ✓ No device found at unregistered port 0x1000");
    }

    // Demonstrate overlap detection
    println!("\n⚠️  Testing Overlap Detection...");

    // Try to register overlapping I/O port range
    let serial2: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(SerialDevice::new("COM2".to_string(), 0x2F8)));
    manager
        .register_device("serial2".to_string(), serial2.clone())
        .await?;

    match manager
        .register_io_port_range("serial2".to_string(), 0x3F0, 0x3FF)
        .await
    {
        Ok(_) => println!("  ❌ Overlap detection failed!"),
        Err(e) => println!("  ✓ Overlap correctly detected: {}", e),
    }

    // Register non-overlapping range for COM2
    manager
        .register_io_port_range("serial2".to_string(), 0x2F8, 0x2FF)
        .await?;
    println!("  ✓ COM2 registered at non-overlapping range: 0x2F8-0x2FF");

    // Show all registered devices
    println!("\n📋 Registered Devices:");
    if let Some(device) = manager.get_device("serial").await {
        println!(
            "  • serial: type={:?}, name='{}'",
            device.read().await.device_type(),
            device.read().await.name()
        );
    }
    if let Some(device) = manager.get_device("timer").await {
        println!(
            "  • timer: type={:?}, name='{}'",
            device.read().await.device_type(),
            device.read().await.name()
        );
    }
    if let Some(device) = manager.get_device("serial2").await {
        println!(
            "  • serial2: type={:?}, name='{}'",
            device.read().await.device_type(),
            device.read().await.name()
        );
    }

    // Test MMIO region registration
    println!("\n💾 Testing MMIO Registration...");

    // Create a device for MMIO testing
    let mmio_device: Arc<RwLock<dyn Device>> = Arc::new(RwLock::new(SerialDevice::new(
        "MMIO_SERIAL".to_string(),
        0x1000_0000,
    )));
    manager
        .register_device("mmio_serial".to_string(), mmio_device.clone())
        .await?;
    println!("  ✓ MMIO device registered");

    // Register MMIO region (1 MB at 0x1000_0000)
    manager
        .register_mmio_region("mmio_serial".to_string(), 0x1000_0000, 0x10_0000)
        .await?;
    println!("  ✓ MMIO region registered: 0x1000_0000-0x1010_0000 (1 MB)");

    // Test MMIO lookup
    if let Some(handle) = manager.find_mmio_device(0x1000_5000).await {
        println!(
            "  ✓ Found device at address 0x1000_5000: {}",
            handle.device_name()
        );
    }

    // Test MMIO overlap detection
    let mmio_device2: Arc<RwLock<dyn Device>> = Arc::new(RwLock::new(SerialDevice::new(
        "MMIO_SERIAL2".to_string(),
        0x2000_0000,
    )));
    manager
        .register_device("mmio_serial2".to_string(), mmio_device2.clone())
        .await?;

    match manager
        .register_mmio_region("mmio_serial2".to_string(), 0x1000_8000, 0x10_0000)
        .await
    {
        Ok(_) => println!("  ❌ MMIO overlap detection failed!"),
        Err(e) => println!("  ✓ MMIO overlap correctly detected: {}", e),
    }

    // Register non-overlapping MMIO region
    manager
        .register_mmio_region("mmio_serial2".to_string(), 0x2000_0000, 0x10_0000)
        .await?;
    println!("  ✓ MMIO2 registered at non-overlapping region: 0x2000_0000-0x2010_0000");

    // Shutdown all devices
    println!("\n🛑 Shutting down devices...");
    manager.shutdown_all().await?;
    println!("  ✓ All devices shut down");

    println!("\n✅ Device registration example completed!");

    Ok(())
}
