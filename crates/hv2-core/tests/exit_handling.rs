//! VM Exit Handling Integration Tests
//!
//! This test suite validates the VM execution loop and exit handling mechanism.

use hv2_core::{IoDirection, Result, VMConfig, VmExit, VM};
use std::sync::Arc;

/// Helper function to create a test VM with standard configuration
async fn create_test_vm(name: &str) -> Result<Arc<VM>> {
    let config = VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024, // 64MB
        enable_gpu: false,
        enable_networking: false,
        enable_tracing: true,
    };

    let vm = Arc::new(VM::new(config)?);

    // Unmask all PIC interrupts for testing
    let pic = vm.pic();
    pic.set_master_mask(0x00);
    pic.set_slave_mask(0x00);

    Ok(vm)
}

#[tokio::test]
async fn test_vm_creation_with_backend() -> Result<()> {
    let vm = create_test_vm("test-creation").await?;

    // Verify backend exists
    let backend = vm.backend();
    assert_eq!(backend.platform().to_string(), "Tcg");

    Ok(())
}

#[tokio::test]
async fn test_vm_start_and_stop() -> Result<()> {
    let vm = create_test_vm("test-lifecycle").await?;

    // Start VM
    vm.start().await?;
    assert_eq!(vm.state().to_string(), "Running");

    // Stop VM
    vm.stop().await?;
    assert_eq!(vm.state().to_string(), "Stopped");

    Ok(())
}

#[tokio::test]
async fn test_vm_exit_display() {
    // Test VmExit Display implementations
    let exit = VmExit::Hlt;
    assert_eq!(format!("{}", exit), "HLT");

    let exit = VmExit::Shutdown;
    assert_eq!(format!("{}", exit), "SHUTDOWN");

    let exit = VmExit::mmio_read(0x1000, 4);
    assert_eq!(format!("{}", exit), "MMIO READ at 0x1000 (4 bytes)");

    let exit = VmExit::mmio_write(0x2000, &[0x12, 0x34]);
    assert!(format!("{}", exit).contains("MMIO WRITE at 0x2000"));

    let exit = VmExit::io_in(0x3F8, 1);
    assert!(format!("{}", exit).contains("IO IN port 0x3f8"));

    let exit = VmExit::io_out(0x3F8, 1, 0x41);
    assert!(format!("{}", exit).contains("IO OUT port 0x3f8"));
}

#[tokio::test]
async fn test_vm_exit_helpers() {
    // Test VmExit helper methods
    let exit = VmExit::Hlt;
    assert!(exit.is_hlt());
    assert!(!exit.is_mmio());
    assert!(!exit.is_io());
    assert!(!exit.is_shutdown());

    let exit = VmExit::mmio_read(0x1000, 4);
    assert!(exit.is_mmio());
    assert!(!exit.is_hlt());

    let exit = VmExit::io_in(0x3F8, 1);
    assert!(exit.is_io());
    assert!(!exit.is_mmio());

    let exit = VmExit::Shutdown;
    assert!(exit.is_shutdown());
}

#[tokio::test]
async fn test_pic_port_handling() -> Result<()> {
    let vm = create_test_vm("test-pic-ports").await?;
    let pic = vm.pic();

    // Test PIC port detection
    assert!(pic.handles_port(0x20)); // Master command
    assert!(pic.handles_port(0x21)); // Master data
    assert!(pic.handles_port(0xA0)); // Slave command
    assert!(pic.handles_port(0xA1)); // Slave data
    assert!(!pic.handles_port(0x3F8)); // Not a PIC port

    // Test PIC port read/write
    pic.write_port(0x21, 0xFE).await?; // Mask all except IRQ 0
    let mask = pic.read_port(0x21).await?;
    assert_eq!(mask, 0xFE);

    Ok(())
}

#[tokio::test]
async fn test_interrupt_with_pic() -> Result<()> {
    let vm = create_test_vm("test-pic-interrupt").await?;
    let pic = vm.pic();

    // Raise IRQ 0
    pic.raise_irq(0)?;

    // Should see pending interrupt
    let vector = pic.get_pending_interrupt();
    assert_eq!(vector, Some(0x20)); // IRQ 0 maps to vector 0x20

    // Acknowledge
    pic.acknowledge_interrupt(0x20)?;

    // Should no longer be pending (moved to ISR)
    let vector = pic.get_pending_interrupt();
    assert!(vector.is_none());

    Ok(())
}

#[tokio::test]
async fn test_event_bus_integration() -> Result<()> {
    let vm = create_test_vm("test-events").await?;

    // Subscribe to events
    let mut event_rx = vm.subscribe_events();

    // Start VM (generates state change event)
    vm.start().await?;

    // Should receive state change event
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), event_rx.recv()).await;

    assert!(event.is_ok());
    let event = event.unwrap();
    assert!(event.is_ok());

    Ok(())
}

// Note: Full VM execution loop testing requires a working CPU emulator
// or real hypervisor backend. These tests validate the infrastructure
// is in place for exit handling.
