//! End-to-End Device Interrupt Integration Tests
//!
//! This module contains integration tests that verify the complete interrupt flow:
//! Device Event → PIC → Interrupt Window → vCPU Injection

#[cfg(test)]
mod tests {
    use crate::device_manager::{DeviceManager, SerialPort};
    use crate::devices::vga::VgaColor;
    use std::time::Duration;
    use tokio::time::sleep;

    /// Helper function to create a device manager with unmasked interrupts
    /// and serial interrupts enabled for testing
    fn setup_device_manager() -> DeviceManager {
        let mut dm = DeviceManager::new();
        dm.init_standard_devices().unwrap();

        // Unmask all interrupts so they can be delivered
        let pic = dm.pic();
        pic.set_master_mask(0x00); // Unmask all master PIC interrupts
        pic.set_slave_mask(0x00); // Unmask all slave PIC interrupts

        // Enable serial interrupts (normally guest OS does this via IER)
        dm.enable_serial_interrupts(SerialPort::COM1, true, true);
        dm.enable_serial_interrupts(SerialPort::COM2, true, true);

        dm
    }

    #[tokio::test]
    async fn test_timer_interrupt_flow() {
        // Test that timer generates IRQ 0 and it appears in PIC
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Timer should be running and generating interrupts
        // Wait for at least one tick (54.925ms per tick)
        sleep(Duration::from_millis(100)).await;

        // Check if IRQ 0 is pending in the PIC
        let pending = pic.get_pending_interrupt();

        // We should have at least one interrupt (IRQ 0, vector 0x20)
        // Note: Timer generates IRQ 0 which maps to vector 0x20 (master PIC base)
        assert!(
            pending.is_some() && pending.unwrap() == 0x20,
            "Timer should have generated IRQ 0 (vector 0x20), got {:?}",
            pending
        );

        // Acknowledge and send EOI for the interrupt
        pic.acknowledge_interrupt(0x20).unwrap();
        pic.send_eoi(0x20).unwrap();

        // Cleanup
        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_keyboard_interrupt_flow() {
        // Test that keyboard scancodes generate IRQ 1
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Inject a scancode (this should raise IRQ 1)
        dm.inject_key(0x1E); // 'A' key scancode

        // Check if IRQ 1 is pending (vector 0x21)
        let pending_after = pic.get_pending_interrupt();

        assert!(
            pending_after.is_some() && pending_after.unwrap() == 0x21,
            "Keyboard should have generated IRQ 1 (vector 0x21), got {:?}",
            pending_after
        );

        // Acknowledge and send EOI for the interrupt
        pic.acknowledge_interrupt(0x21).unwrap();
        pic.send_eoi(0x21).unwrap();

        // Verify interrupt was cleared
        let pending_final = pic.get_pending_interrupt();
        assert_ne!(
            pending_final,
            Some(0x21),
            "IRQ 1 should be cleared after acknowledge"
        );

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_serial_com1_interrupt_flow() {
        // Test that COM1 serial port generates IRQ 4
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Send data to COM1 (should raise IRQ 4)
        dm.serial_input(SerialPort::COM1, b"Hello").unwrap();

        // Check if IRQ 4 is pending (vector 0x24)
        let pending = pic.get_pending_interrupt();

        assert!(
            pending.is_some() && pending.unwrap() == 0x24,
            "COM1 should have generated IRQ 4 (vector 0x24), got {:?}",
            pending
        );

        // Acknowledge and send EOI
        pic.acknowledge_interrupt(0x24).unwrap();
        pic.send_eoi(0x24).unwrap();

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_serial_com2_interrupt_flow() {
        // Test that COM2 serial port generates IRQ 3
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Send data to COM2 (should raise IRQ 3)
        dm.serial_input(SerialPort::COM2, b"World").unwrap();

        // Check if IRQ 3 is pending (vector 0x23)
        let pending = pic.get_pending_interrupt();

        assert!(
            pending.is_some() && pending.unwrap() == 0x23,
            "COM2 should have generated IRQ 3 (vector 0x23), got {:?}",
            pending
        );

        // Acknowledge and send EOI
        pic.acknowledge_interrupt(0x23).unwrap();
        pic.send_eoi(0x23).unwrap();

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_multiple_interrupt_sources() {
        // Test multiple devices generating interrupts simultaneously
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Generate interrupts from multiple sources
        dm.inject_key(0x1E); // Keyboard IRQ 1
        dm.serial_input(SerialPort::COM1, b"Test").unwrap(); // COM1 IRQ 4
        dm.serial_input(SerialPort::COM2, b"Data").unwrap(); // COM2 IRQ 3

        // The PIC should return the highest priority pending interrupt
        // Priority order (highest to lowest): IRQ 0, 1, 2, 3, 4, 5, 6, 7
        // So we should get keyboard (IRQ 1) first
        let first = pic.get_pending_interrupt();
        assert_eq!(
            first,
            Some(0x21),
            "First interrupt should be keyboard IRQ 1 (highest priority)"
        );
        pic.acknowledge_interrupt(0x21).unwrap();
        pic.send_eoi(0x21).unwrap();

        // Next should be COM2 (IRQ 3)
        let second = pic.get_pending_interrupt();
        assert_eq!(second, Some(0x23), "Second interrupt should be COM2 IRQ 3");
        pic.acknowledge_interrupt(0x23).unwrap();
        pic.send_eoi(0x23).unwrap();

        // Finally COM1 (IRQ 4)
        let third = pic.get_pending_interrupt();
        assert_eq!(third, Some(0x24), "Third interrupt should be COM1 IRQ 4");
        pic.acknowledge_interrupt(0x24).unwrap();
        pic.send_eoi(0x24).unwrap();

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_interrupt_masking() {
        // Test that masked interrupts are not delivered
        let mut dm = DeviceManager::new();
        dm.init_standard_devices().unwrap();

        let pic = dm.pic();

        // Mask IRQ 1 (keyboard) - set bit 1 in master mask
        // 0xFF = all masked, 0xFD = all except IRQ 1 masked, 0x02 = only IRQ 1 masked
        pic.set_master_mask(0x02); // Mask only IRQ 1

        // Try to generate keyboard interrupt
        dm.inject_key(0x1E);

        // Should not see IRQ 1 pending (it's masked)
        let pending = pic.get_pending_interrupt();
        assert_ne!(pending, Some(0x21), "Masked IRQ 1 should not be pending");

        // Unmask IRQ 1 (set mask to 0x00 = all unmasked)
        pic.set_master_mask(0x00);

        // Now inject another key
        dm.inject_key(0x30); // 'B' key

        // Should see IRQ 1 now
        let pending_after = pic.get_pending_interrupt();
        assert_eq!(
            pending_after,
            Some(0x21),
            "Unmasked IRQ 1 should be pending"
        );

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_interrupt_acknowledge_clears_isr() {
        // Test that acknowledging an interrupt and sending EOI properly clears it
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Generate keyboard interrupt
        dm.inject_key(0x1E);

        // Get pending interrupt
        let pending1 = pic.get_pending_interrupt();
        assert_eq!(pending1, Some(0x21));

        // Acknowledge it and send EOI
        pic.acknowledge_interrupt(0x21).unwrap();
        pic.send_eoi(0x21).unwrap();

        // Generate another keyboard interrupt
        dm.inject_key(0x30);

        // Should see the new interrupt (not the old one)
        let pending2 = pic.get_pending_interrupt();
        assert_eq!(
            pending2,
            Some(0x21),
            "New interrupt should be deliverable after acknowledge + EOI"
        );

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_timer_periodic_interrupts() {
        // Test that timer generates multiple periodic interrupts
        let mut dm = setup_device_manager();

        let pic = dm.pic();
        let timer = dm.timer().unwrap();

        // Get initial tick count
        let initial_ticks = timer.total_ticks();

        // Wait for multiple timer ticks (200ms = ~3-4 ticks at 18.2 Hz)
        sleep(Duration::from_millis(200)).await;

        // Verify tick count increased
        let final_ticks = timer.total_ticks();
        assert!(
            final_ticks > initial_ticks,
            "Timer should generate periodic interrupts (ticks: {} -> {})",
            initial_ticks,
            final_ticks
        );

        // Verify we can get at least one pending interrupt
        let pending = pic.get_pending_interrupt();
        assert!(pending.is_some(), "Timer should have pending interrupts");

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_device_manager_pic_consistency() {
        // Verify all devices share the same PIC instance
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Generate interrupts from different devices
        dm.inject_key(0x1E); // Keyboard
        dm.serial_input(SerialPort::COM1, b"X").unwrap(); // Serial

        // Both interrupts should be visible through the same PIC
        let mut interrupt_count = 0;

        // Count pending interrupts
        while let Some(vector) = pic.get_pending_interrupt() {
            interrupt_count += 1;
            pic.acknowledge_interrupt(vector).unwrap();
            pic.send_eoi(vector).unwrap();

            // Safety check to avoid infinite loop
            if interrupt_count > 10 {
                break;
            }
        }

        assert!(
            interrupt_count >= 2,
            "Should have at least 2 interrupts from different devices, got {}",
            interrupt_count
        );

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_vga_does_not_generate_interrupts() {
        // VGA text mode does not generate interrupts
        // This test verifies VGA operations don't affect PIC state

        // Don't use setup_device_manager() to avoid timer interrupts
        let mut dm = DeviceManager::new();
        dm.init_standard_devices().unwrap();

        let pic = dm.pic();

        // Mask all interrupts first to start with clean state
        pic.set_master_mask(0xFF);
        pic.set_slave_mask(0xFF);

        // Clear any pending interrupts
        while pic.get_pending_interrupt().is_some() {}

        // Unmask only the interrupts VGA would potentially use (it shouldn't use any)
        // Keep all masked to have clean baseline

        // Perform VGA operations
        dm.vga_write("Hello World", VgaColor::White, VgaColor::Black);
        dm.vga_clear();

        // Unmask to check if VGA raised anything
        pic.set_master_mask(0x00);
        pic.set_slave_mask(0x00);

        // Should have no pending interrupts from VGA
        // (Timer might have raised something but that's expected, we check for IRQs
        // that would be attributed to VGA which doesn't use any)
        let pending = pic.get_pending_interrupt();
        if let Some(vector) = pending {
            // VGA doesn't use any IRQs, so if we got anything it should be timer (0x20)
            // Any other interrupt would indicate VGA is incorrectly generating interrupts
            assert!(
                vector == 0x20, // Timer IRQ is okay
                "VGA operations should not generate unexpected interrupts, got vector {:?}",
                vector
            );
        }

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_interrupt_priority_order() {
        // Test that PIC respects interrupt priority (IRQ 0 > IRQ 1 > ... > IRQ 7)
        let mut dm = setup_device_manager();

        let pic = dm.pic();

        // Generate low priority interrupt first
        dm.serial_input(SerialPort::COM1, b"Low").unwrap(); // IRQ 4

        // Then generate high priority interrupt
        dm.inject_key(0x1E); // IRQ 1 (higher priority than IRQ 4)

        // Should get IRQ 1 first (higher priority)
        let first = pic.get_pending_interrupt();
        assert_eq!(
            first,
            Some(0x21),
            "Higher priority IRQ 1 should be delivered first"
        );
        pic.acknowledge_interrupt(0x21).unwrap();
        pic.send_eoi(0x21).unwrap();

        // Then get IRQ 4
        let second = pic.get_pending_interrupt();
        assert_eq!(second, Some(0x24), "IRQ 4 should be delivered second");

        dm.shutdown().unwrap();
    }

    #[tokio::test]
    async fn test_serial_receive_generates_interrupt() {
        // Test that receiving data on serial port generates interrupt
        let mut dm = setup_device_manager();

        // Send data to COM1's receive buffer
        dm.serial_input(SerialPort::COM1, b"RX Test").unwrap();

        // Check for IRQ 4 from receive data available
        let pic = dm.pic();
        let pending = pic.get_pending_interrupt();
        assert!(
            pending.is_some() && pending.unwrap() == 0x24,
            "Serial receive should generate IRQ 4, got {:?}",
            pending
        );

        dm.shutdown().unwrap();
    }
}
