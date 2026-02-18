//! Serial console example - demonstrates device emulation and MMIO

use hv2_core::{Device, MmioManager, SerialDevice};
use parking_lot::RwLock;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("🖥️  HyperMachine Serial Console Example");
    println!("{}", "=".repeat(50));

    // Create MMIO manager
    let mmio = MmioManager::new();
    println!("\n✓ MMIO manager created");

    // Create serial device (COM1 at standard address 0x3F8)
    let mut serial_dev = SerialDevice::new("COM1".to_string(), 0x3F8);

    // Initialize device
    serial_dev.init().await?;
    let serial = Arc::new(RwLock::new(serial_dev));

    // Map device to MMIO
    mmio.map_device(0x3F8, 8, serial.clone())?;
    println!("✓ Serial device mapped to 0x3F8-0x3FF");

    // Show mapped regions
    println!("\n📍 Mapped MMIO Regions:");
    for region in mmio.regions() {
        println!(
            "  • {}: 0x{:X}-0x{:X} ({} bytes)",
            region.device_name,
            region.base,
            region.base + region.size,
            region.size
        );
    }

    // Simulate guest writes to serial port
    println!("\n📤 Guest writing to serial port...");
    let message = "Hello from guest VM!\n";
    for &byte in message.as_bytes() {
        mmio.write(0x3F8, &[byte]).await?; // THR register
    }

    // Read back from device
    let output = serial.read().output_string();
    println!("✓ Received from guest: {:?}", output.trim());

    // Simulate host input to guest
    println!("\n📥 Host sending input to guest...");
    serial.read().input(b"Hello from host!\n")?;

    // Guest reads from serial port
    println!("✓ Guest reading from serial port...");
    let mut received = Vec::new();
    loop {
        // Check LSR (Line Status Register) for data ready
        let mut lsr = [0u8; 1];
        mmio.read(0x3F8 + 5, &mut lsr).await?;

        if lsr[0] & 0x01 == 0 {
            break; // No more data
        }

        // Read byte from RBR (Receiver Buffer Register)
        let mut byte = [0u8; 1];
        mmio.read(0x3F8, &mut byte).await?;
        received.push(byte[0]);
    }

    let guest_received = String::from_utf8_lossy(&received);
    println!("✓ Guest received: {:?}", guest_received.trim());

    // Test multiple serial ports
    println!("\n🔌 Adding second serial port (COM2)...");
    let mut serial2_dev = SerialDevice::new("COM2".to_string(), 0x2F8);
    serial2_dev.init().await?;
    let serial2 = Arc::new(RwLock::new(serial2_dev));
    mmio.map_device(0x2F8, 8, serial2.clone())?;

    println!("✓ COM2 mapped to 0x2F8-0x2FF");

    // Show all regions
    println!("\n📍 All MMIO Regions:");
    for region in mmio.regions() {
        println!(
            "  • {}: 0x{:X}-0x{:X}",
            region.device_name,
            region.base,
            region.base + region.size
        );
    }

    // Write to both serial ports
    println!("\n✍️  Writing to both serial ports...");
    for &byte in b"COM1 message\n" {
        mmio.write(0x3F8, &[byte]).await?;
    }
    for &byte in b"COM2 message\n" {
        mmio.write(0x2F8, &[byte]).await?;
    }

    println!("✓ COM1 output: {:?}", serial.read().output_string().trim());
    println!("✓ COM2 output: {:?}", serial2.read().output_string().trim());

    // Test unmapped region access
    println!("\n🔍 Testing unmapped region access...");
    let mut buf = [0u8; 1];
    mmio.read(0x1000, &mut buf).await?; // Unmapped address
    println!("✓ Read from unmapped address returned: 0x{:02X}", buf[0]);

    println!("\n✅ Serial console example completed!");

    Ok(())
}
