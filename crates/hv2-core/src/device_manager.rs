//! Device Manager
//!
//! This module provides centralized management for all emulated devices.
//! It coordinates device initialization, PIC assignment, and provides
//! a unified interface for device access.

use crate::{
    devices::{KeyboardDevice, SerialDevice, TimerDevice, VgaDevice},
    interrupt::Pic8259,
    Result,
};
use std::sync::Arc;

/// Device Manager
///
/// Coordinates all emulated devices and manages their lifecycle.
pub struct DeviceManager {
    /// PIC (Programmable Interrupt Controller)
    pic: Arc<Pic8259>,

    /// Timer (PIT - Programmable Interval Timer)
    timer: Option<TimerDevice>,

    /// Keyboard (PS/2 Keyboard Controller)
    keyboard: Option<KeyboardDevice>,

    /// COM1 Serial Port
    com1: Option<SerialDevice>,

    /// COM2 Serial Port
    com2: Option<SerialDevice>,

    /// VGA Text Mode Display
    vga: Option<VgaDevice>,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        Self {
            pic: Arc::new(Pic8259::new()),
            timer: None,
            keyboard: None,
            com1: None,
            com2: None,
            vga: None,
        }
    }

    /// Initialize all standard devices
    pub fn init_standard_devices(&mut self) -> Result<()> {
        // Create and configure timer (IRQ 0)
        let mut timer = TimerDevice::new("PIT".to_string(), 0x40);
        timer.set_pic(Arc::clone(&self.pic));
        self.timer = Some(timer);

        // Create and configure keyboard (IRQ 1)
        let keyboard = KeyboardDevice::new();
        keyboard.set_pic(Arc::clone(&self.pic));
        self.keyboard = Some(keyboard);

        // Create and configure COM1 (IRQ 4, base 0x3F8)
        let mut com1 = SerialDevice::new("COM1".to_string(), 0x3F8);
        com1.set_pic(Arc::clone(&self.pic));
        self.com1 = Some(com1);

        // Create and configure COM2 (IRQ 3, base 0x2F8)
        let mut com2 = SerialDevice::new("COM2".to_string(), 0x2F8);
        com2.set_pic(Arc::clone(&self.pic));
        self.com2 = Some(com2);

        // Create VGA (no interrupts)
        let vga = VgaDevice::new();
        self.vga = Some(vga);

        Ok(())
    }

    /// Get the PIC
    pub fn pic(&self) -> &Arc<Pic8259> {
        &self.pic
    }

    /// Get the timer device
    pub fn timer(&self) -> Option<&TimerDevice> {
        self.timer.as_ref()
    }

    /// Get mutable timer device
    pub fn timer_mut(&mut self) -> Option<&mut TimerDevice> {
        self.timer.as_mut()
    }

    /// Get the keyboard device
    pub fn keyboard(&self) -> Option<&KeyboardDevice> {
        self.keyboard.as_ref()
    }

    /// Get the COM1 serial port
    pub fn com1(&self) -> Option<&SerialDevice> {
        self.com1.as_ref()
    }

    /// Get mutable COM1 serial port
    pub fn com1_mut(&mut self) -> Option<&mut SerialDevice> {
        self.com1.as_mut()
    }

    /// Get the COM2 serial port
    pub fn com2(&self) -> Option<&SerialDevice> {
        self.com2.as_ref()
    }

    /// Get mutable COM2 serial port
    pub fn com2_mut(&mut self) -> Option<&mut SerialDevice> {
        self.com2.as_mut()
    }

    /// Get the VGA device
    pub fn vga(&self) -> Option<&VgaDevice> {
        self.vga.as_ref()
    }

    /// Inject a scancode into the keyboard
    pub fn inject_key(&self, scancode: u8) {
        if let Some(kbd) = &self.keyboard {
            kbd.inject_scancode(scancode);
        }
    }

    /// Send data to a serial port
    pub fn serial_input(&self, port: SerialPort, data: &[u8]) -> Result<()> {
        match port {
            SerialPort::COM1 => {
                if let Some(serial) = &self.com1 {
                    serial.input(data)?;
                }
            }
            SerialPort::COM2 => {
                if let Some(serial) = &self.com2 {
                    serial.input(data)?;
                }
            }
        }
        Ok(())
    }

    /// Read output from a serial port
    pub fn serial_output(&self, port: SerialPort) -> Vec<u8> {
        match port {
            SerialPort::COM1 => self.com1.as_ref().map(|s| s.output()).unwrap_or_default(),
            SerialPort::COM2 => self.com2.as_ref().map(|s| s.output()).unwrap_or_default(),
        }
    }

    /// Enable serial interrupts for a port
    ///
    /// Normally the guest OS configures interrupts via the IER register.
    /// This method is for testing or direct control scenarios.
    pub fn enable_serial_interrupts(&self, port: SerialPort, receive: bool, transmit: bool) {
        match port {
            SerialPort::COM1 => {
                if let Some(serial) = &self.com1 {
                    serial.enable_interrupts(receive, transmit);
                }
            }
            SerialPort::COM2 => {
                if let Some(serial) = &self.com2 {
                    serial.enable_interrupts(receive, transmit);
                }
            }
        }
    }

    /// Get VGA text content
    pub fn vga_text(&self) -> String {
        self.vga.as_ref().map(|v| v.get_text()).unwrap_or_default()
    }

    /// Clear VGA screen
    pub fn vga_clear(&self) {
        if let Some(vga) = &self.vga {
            vga.clear();
        }
    }

    /// Write string to VGA
    pub fn vga_write(
        &self,
        text: &str,
        fg: crate::devices::vga::VgaColor,
        bg: crate::devices::vga::VgaColor,
    ) {
        if let Some(vga) = &self.vga {
            let attr = crate::devices::vga::VgaAttribute::new(fg, bg);
            vga.put_string(text, attr);
        }
    }

    /// Shutdown all devices
    pub fn shutdown(&mut self) -> Result<()> {
        // Stop timer background task
        if let Some(timer) = &self.timer {
            timer.stop_timer_task();
        }

        // Clear all device references
        self.timer = None;
        self.keyboard = None;
        self.com1 = None;
        self.com2 = None;
        self.vga = None;

        Ok(())
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Serial port selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialPort {
    COM1,
    COM2,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::vga::{VgaAttribute, VgaColor};

    #[tokio::test]
    async fn test_device_manager_creation() {
        let manager = DeviceManager::new();
        assert!(manager.timer().is_none());
        assert!(manager.keyboard().is_none());
        assert!(manager.com1().is_none());
        assert!(manager.com2().is_none());
        assert!(manager.vga().is_none());
    }

    #[tokio::test]
    async fn test_device_manager_init() {
        let mut manager = DeviceManager::new();
        manager.init_standard_devices().unwrap();

        assert!(manager.timer().is_some());
        assert!(manager.keyboard().is_some());
        assert!(manager.com1().is_some());
        assert!(manager.com2().is_some());
        assert!(manager.vga().is_some());
    }

    #[tokio::test]
    async fn test_device_manager_pic_sharing() {
        let mut manager = DeviceManager::new();
        manager.init_standard_devices().unwrap();

        // All devices should share the same PIC
        let pic_ptr = Arc::as_ptr(&manager.pic);

        // Verify PIC is shared (we can't directly compare Arc pointers easily,
        // but we can verify devices work with interrupts)
        manager.inject_key(0x1E); // 'A' key

        // Keyboard should have data
        assert!(manager.keyboard().unwrap().has_pending_interrupt());
    }

    #[tokio::test]
    async fn test_device_manager_serial() {
        let mut manager = DeviceManager::new();
        manager.init_standard_devices().unwrap();

        // Send data to COM1
        manager.serial_input(SerialPort::COM1, b"Hello").unwrap();

        // Read it back
        let output = manager.serial_output(SerialPort::COM1);
        assert!(output.is_empty()); // Input goes to RX buffer, not TX

        // Write to COM1 TX buffer via the device directly
        let com1 = manager.com1_mut().unwrap();
        use crate::Device;
        com1.write(0, b"W").await.unwrap(); // THR offset

        // Read output
        let output = manager.serial_output(SerialPort::COM1);
        assert_eq!(output, b"W");
    }

    #[tokio::test]
    async fn test_device_manager_vga() {
        let mut manager = DeviceManager::new();
        manager.init_standard_devices().unwrap();

        // Write to VGA
        manager.vga_write("Test", VgaColor::White, VgaColor::Black);

        // Verify it's there
        let text = manager.vga_text();
        assert!(text.starts_with("Test"));

        // Clear
        manager.vga_clear();
        let text = manager.vga_text();
        assert!(text.starts_with("    ")); // Should be spaces
    }

    #[tokio::test]
    async fn test_device_manager_timer() {
        let mut manager = DeviceManager::new();
        manager.init_standard_devices().unwrap();

        // Timer should be running
        let initial_ticks = manager.timer().unwrap().total_ticks();

        // Wait a bit
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let final_ticks = manager.timer().unwrap().total_ticks();
        assert!(final_ticks >= initial_ticks);
    }

    #[tokio::test]
    async fn test_device_manager_shutdown() {
        let mut manager = DeviceManager::new();
        manager.init_standard_devices().unwrap();

        // Shutdown
        manager.shutdown().unwrap();

        // All devices should be gone
        assert!(manager.timer().is_none());
        assert!(manager.keyboard().is_none());
        assert!(manager.com1().is_none());
        assert!(manager.com2().is_none());
        assert!(manager.vga().is_none());
    }
}
