//! Timer device example - demonstrates PIT emulation

use hv2_core::{Device, MmioManager, TimerDevice};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("⏱️  HyperMachine Timer Device Example");
    println!("{}", "=".repeat(50));

    // Create MMIO manager
    let mmio = MmioManager::new();
    println!("\n✓ MMIO manager created");

    // Create PIT (Programmable Interval Timer) at standard address 0x40
    let mut timer_dev = TimerDevice::new("PIT".to_string(), 0x40);

    // Initialize timer
    timer_dev.init().await?;
    let timer = Arc::new(RwLock::new(timer_dev));

    // Map timer to MMIO
    mmio.map_device(0x40, 4, timer.clone())?;
    println!("✓ PIT timer mapped to 0x40-0x43");

    // Configure channel 0 for rate generator mode
    println!("\n⚙️  Configuring Timer Channel 0");
    println!("  Mode: Rate Generator (Mode 2)");
    println!("  Frequency: ~1000 Hz (1 ms period)");

    // Control word: channel 0, LSB/MSB, mode 2, binary
    // Bits: SC1 SC0 RW1 RW0 M2 M1 M0 BCD
    //       0   0   1   1  0  1  0  0  = 0x34
    mmio.write(0x43, &[0b00110100]).await?;
    println!("  ✓ Control word written: 0x34");

    // Set reload value to 1193 (1 ms at 1.193182 MHz base frequency)
    let reload_value: u16 = 1193;
    mmio.write(0x40, &[(reload_value & 0xFF) as u8]).await?; // LSB
    mmio.write(0x40, &[(reload_value >> 8) as u8]).await?; // MSB
    println!("  ✓ Reload value set: {} (1 ms period)", reload_value);

    // Enable interrupts
    timer.read().set_interrupt_enabled(true);
    println!("  ✓ Interrupts enabled");

    // Read back the timer configuration
    println!("\n📊 Timer Statistics:");
    println!("  Base Address: 0x{:X}", timer.read().base_address());
    println!("  Total Ticks: {}", timer.read().total_ticks());
    println!(
        "  Interrupts: {}",
        if timer.read().interrupt_enabled() {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    // Simulate timer operation
    println!("\n⏳ Simulating timer operation...");
    for i in 1..=5 {
        sleep(Duration::from_millis(100)).await;

        // Read count from channel 0
        let mut lsb = [0u8; 1];
        let mut msb = [0u8; 1];
        mmio.read(0x40, &mut lsb).await?;
        mmio.read(0x40, &mut msb).await?;
        let count = (lsb[0] as u16) | ((msb[0] as u16) << 8);

        println!("  [{}] Current count: {} (0x{:04X})", i, count, count);
    }

    // Show final statistics
    println!("\n📈 Final Statistics:");
    println!("  Total Ticks: {}", timer.read().total_ticks());

    // Show all MMIO regions
    println!("\n📍 MMIO Regions:");
    for region in mmio.regions() {
        println!(
            "  • {}: 0x{:X}-0x{:X} ({} bytes)",
            region.device_name,
            region.base,
            region.base + region.size,
            region.size
        );
    }

    println!("\n✅ Timer device example completed!");

    Ok(())
}
