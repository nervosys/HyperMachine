//! PS/2 Keyboard Controller
//!
//! This module implements the Intel 8042 keyboard controller, which manages:
//! - PS/2 keyboard input
//! - Scancode translation (Set 1, 2, 3)
//! - Keyboard buffer (FIFO)
//! - Status and command handling
//!
//! I/O Ports:
//! - 0x60: Data port (read/write)
//! - 0x64: Status register (read) / Command register (write)

use crate::interrupt::Pic8259;
use crate::{Device, DeviceType, Result};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

/// Status register flags
const STATUS_OBF: u8 = 0x01; // Output Buffer Full
const STATUS_IBF: u8 = 0x02; // Input Buffer Full
const STATUS_SYS: u8 = 0x04; // System Flag
const STATUS_CMD: u8 = 0x08; // Command/Data
const STATUS_UNLOCKED: u8 = 0x10; // Keyboard Unlocked
const STATUS_AUX_OBF: u8 = 0x20; // Auxiliary Output Buffer Full
const STATUS_TIMEOUT: u8 = 0x40; // Timeout Error
const STATUS_PARITY: u8 = 0x80; // Parity Error

/// Data port, relative to the base port the device is registered at.
///
/// Relative, not absolute: `DeviceManager` subtracts the base port before
/// calling, so a device decoding 0x60 directly never matches a real access.
pub const KEYBOARD_DATA_OFFSET: u64 = 0;

/// Status (read) and command (write) port, relative to the base port.
pub const KEYBOARD_STATUS_OFFSET: u64 = 4;

/// Controller commands (written to port 0x64)
const CMD_READ_CCB: u8 = 0x20; // Read Controller Command Byte
const CMD_WRITE_CCB: u8 = 0x60; // Write Controller Command Byte
const CMD_DISABLE_AUX: u8 = 0xA7; // Disable auxiliary device
const CMD_ENABLE_AUX: u8 = 0xA8; // Enable auxiliary device
const CMD_TEST_AUX: u8 = 0xA9; // Test auxiliary interface
const CMD_SELF_TEST: u8 = 0xAA; // Controller self-test
const CMD_TEST_KBD: u8 = 0xAB; // Test keyboard interface
const CMD_DISABLE_KBD: u8 = 0xAD; // Disable keyboard
const CMD_ENABLE_KBD: u8 = 0xAE; // Enable keyboard
const CMD_READ_INPUT: u8 = 0xC0; // Read input port
const CMD_READ_OUTPUT: u8 = 0xD0; // Read output port
const CMD_WRITE_OUTPUT: u8 = 0xD1; // Write output port
const CMD_PULSE_OUTPUT: u8 = 0xF0; // Pulse output port

/// Keyboard commands (written to port 0x60)
const KBD_SET_LED: u8 = 0xED; // Set LEDs
const KBD_ECHO: u8 = 0xEE; // Echo
const KBD_SET_SCANCODE: u8 = 0xF0; // Set scancode set
const KBD_GET_ID: u8 = 0xF2; // Get keyboard ID
const KBD_SET_RATE: u8 = 0xF3; // Set typematic rate
const KBD_ENABLE: u8 = 0xF4; // Enable scanning
const KBD_DISABLE: u8 = 0xF5; // Disable scanning
const KBD_RESET: u8 = 0xFF; // Reset

/// Keyboard responses
const KBD_ACK: u8 = 0xFA; // Acknowledge
const KBD_RESEND: u8 = 0xFE; // Resend last byte
const KBD_ERROR: u8 = 0xFC; // Error
const KBD_SELF_TEST_PASS: u8 = 0xAA; // Self-test passed
const KBD_ID1: u8 = 0xAB; // Keyboard ID byte 1
const KBD_ID2: u8 = 0x83; // Keyboard ID byte 2

/// Controller Command Byte flags
const CCB_INT_KBD: u8 = 0x01; // Keyboard interrupt enable
const CCB_INT_AUX: u8 = 0x02; // Auxiliary interrupt enable
const CCB_SYS_FLAG: u8 = 0x04; // System flag
const CCB_DISABLE_KBD: u8 = 0x10; // Keyboard disable
const CCB_DISABLE_AUX: u8 = 0x20; // Auxiliary disable
const CCB_TRANSLATE: u8 = 0x40; // Translate scancodes to Set 1

/// Internal keyboard state
#[derive(Debug)]
struct KeyboardState {
    /// Output buffer (keyboard to CPU)
    output_buffer: VecDeque<u8>,
    /// Input buffer (CPU to keyboard)
    input_buffer: Option<u8>,
    /// Status register
    status: u8,
    /// Controller Command Byte
    ccb: u8,
    /// Current command being processed
    current_command: Option<u8>,
    /// Keyboard enabled
    kbd_enabled: bool,
    /// Scancode set (1, 2, or 3)
    scancode_set: u8,
    /// LED state
    led_state: u8,
    /// PIC for raising IRQ 1
    pic: Option<Arc<Pic8259>>,
}

impl KeyboardState {
    fn new() -> Self {
        Self {
            output_buffer: VecDeque::new(),
            input_buffer: None,
            status: STATUS_SYS | STATUS_UNLOCKED, // System initialized, not locked
            ccb: CCB_INT_KBD | CCB_TRANSLATE,     // Interrupts enabled, translation on
            current_command: None,
            kbd_enabled: true,
            scancode_set: 1, // Default to Set 1 (most compatible)
            led_state: 0,
            pic: None,
        }
    }

    /// Check if output buffer has data
    fn output_buffer_full(&self) -> bool {
        !self.output_buffer.is_empty()
    }

    /// Update status register based on buffer state
    fn update_status(&mut self) {
        if self.output_buffer_full() {
            self.status |= STATUS_OBF;
        } else {
            self.status &= !STATUS_OBF;
        }

        if self.input_buffer.is_some() {
            self.status |= STATUS_IBF;
        } else {
            self.status &= !STATUS_IBF;
        }
    }

    /// Push data to output buffer
    fn push_output(&mut self, data: u8) {
        if self.output_buffer.len() < 16 {
            self.output_buffer.push_back(data);
            self.update_status();

            // Raise IRQ 1 if keyboard interrupts are enabled
            if (self.ccb & CCB_INT_KBD) != 0 {
                if let Some(ref pic) = self.pic {
                    let _ = pic.raise_irq(1);
                }
            }
        }
    }

    /// Pop data from output buffer
    fn pop_output(&mut self) -> Option<u8> {
        let data = self.output_buffer.pop_front();
        self.update_status();
        data
    }

    /// Handle controller command
    fn handle_controller_command(&mut self, cmd: u8) {
        match cmd {
            CMD_READ_CCB => {
                self.push_output(self.ccb);
            }
            CMD_WRITE_CCB => {
                self.current_command = Some(cmd);
            }
            CMD_DISABLE_KBD => {
                self.kbd_enabled = false;
                self.ccb |= CCB_DISABLE_KBD;
            }
            CMD_ENABLE_KBD => {
                self.kbd_enabled = true;
                self.ccb &= !CCB_DISABLE_KBD;
            }
            CMD_SELF_TEST => {
                // Controller self-test always passes
                self.push_output(0x55);
            }
            CMD_TEST_KBD => {
                // Keyboard interface test always passes
                self.push_output(0x00);
            }
            CMD_READ_OUTPUT => {
                // Output port (bit 0 = reset, bit 1 = A20 gate)
                self.push_output(0x03); // A20 enabled, no reset
            }
            CMD_WRITE_OUTPUT => {
                self.current_command = Some(cmd);
            }
            _ => {
                // Unknown command, ignore
            }
        }
    }

    /// Handle keyboard command
    fn handle_keyboard_command(&mut self, cmd: u8) {
        match cmd {
            KBD_SET_LED => {
                self.push_output(KBD_ACK);
                self.current_command = Some(cmd);
            }
            KBD_ECHO => {
                self.push_output(KBD_ECHO);
            }
            KBD_SET_SCANCODE => {
                self.push_output(KBD_ACK);
                self.current_command = Some(cmd);
            }
            KBD_GET_ID => {
                self.push_output(KBD_ACK);
                self.push_output(KBD_ID1);
                self.push_output(KBD_ID2);
            }
            KBD_SET_RATE => {
                self.push_output(KBD_ACK);
                self.current_command = Some(cmd);
            }
            KBD_ENABLE => {
                self.kbd_enabled = true;
                self.push_output(KBD_ACK);
            }
            KBD_DISABLE => {
                self.kbd_enabled = false;
                self.push_output(KBD_ACK);
            }
            KBD_RESET => {
                self.output_buffer.clear();
                self.push_output(KBD_ACK);
                self.push_output(KBD_SELF_TEST_PASS);
            }
            _ => {
                // Unknown command
                self.push_output(KBD_ACK);
            }
        }
    }

    /// Handle data written to port 0x60
    fn handle_data_write(&mut self, data: u8) {
        if let Some(cmd) = self.current_command {
            match cmd {
                CMD_WRITE_CCB => {
                    self.ccb = data;
                    self.kbd_enabled = (data & CCB_DISABLE_KBD) == 0;
                    self.current_command = None;
                }
                CMD_WRITE_OUTPUT => {
                    // Writing to output port (usually for A20 gate or reset)
                    self.current_command = None;
                }
                KBD_SET_LED => {
                    self.led_state = data;
                    self.push_output(KBD_ACK);
                    self.current_command = None;
                }
                KBD_SET_SCANCODE => {
                    if data == 0 {
                        // Get current scancode set
                        self.push_output(self.scancode_set);
                    } else if data <= 3 {
                        self.scancode_set = data;
                        self.push_output(KBD_ACK);
                    }
                    self.current_command = None;
                }
                KBD_SET_RATE => {
                    // Accept typematic rate
                    self.push_output(KBD_ACK);
                    self.current_command = None;
                }
                _ => {
                    self.current_command = None;
                }
            }
        } else {
            self.handle_keyboard_command(data);
        }
    }
}

/// Intel 8042 PS/2 Keyboard Controller
///
/// This device emulates the keyboard controller found in PC/AT systems.
/// It handles keyboard input, scancode translation, and status reporting.
#[derive(Debug)]
pub struct KeyboardDevice {
    state: Arc<Mutex<KeyboardState>>,
}

impl KeyboardDevice {
    /// Create a new keyboard controller
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(KeyboardState::new())),
        }
    }

    /// Set the PIC for interrupt generation
    pub fn set_pic(&self, pic: Arc<Pic8259>) {
        let mut state = self.state.lock();
        state.pic = Some(pic);
    }

    /// Read from data port (0x60)
    pub fn read_data(&self) -> u8 {
        let mut state = self.state.lock();
        state.pop_output().unwrap_or(0)
    }

    /// Write to data port (0x60)
    pub fn write_data(&self, data: u8) {
        let mut state = self.state.lock();
        state.handle_data_write(data);
    }

    /// Read from status port (0x64)
    pub fn read_status(&self) -> u8 {
        self.state.lock().status
    }

    /// Write to command port (0x64)
    pub fn write_command(&self, cmd: u8) {
        let mut state = self.state.lock();
        state.handle_controller_command(cmd);
    }

    /// Inject a key scancode (called when user presses/releases a key)
    pub fn inject_scancode(&self, scancode: u8) {
        let mut state = self.state.lock();
        if state.kbd_enabled {
            state.push_output(scancode);
        }
    }

    /// Check if keyboard has pending interrupt (IRQ 1)
    pub fn has_pending_interrupt(&self) -> bool {
        let state = self.state.lock();
        state.output_buffer_full() && (state.ccb & CCB_INT_KBD) != 0
    }
}

impl Default for KeyboardDevice {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for KeyboardDevice {
    fn name(&self) -> &str {
        "Intel 8042 Keyboard"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Input
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        // Offsets are relative to the base port, as everywhere else on the I/O
        // path: registered at 0x60, the data port arrives as 0 and the status
        // port as 4. Decoding the absolute 0x60 and 0x64 meant every real
        // access fell through to an error, and an error here stops the VM --
        // so a guest probing the keyboard controller killed itself.
        //
        // The ports in between (0x61 to 0x63) belong to other parts of the
        // chipset and read as an absent device rather than as a failure.
        // Erroring on them would be the same mistake in a different place.
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = match offset + index as u64 {
                KEYBOARD_DATA_OFFSET => self.read_data(),
                KEYBOARD_STATUS_OFFSET => self.read_status(),
                _ => 0xFF,
            };
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        for (index, byte) in data.iter().enumerate() {
            match offset + index as u64 {
                KEYBOARD_DATA_OFFSET => self.write_data(*byte),
                KEYBOARD_STATUS_OFFSET => self.write_command(*byte),
                _ => {}
            }
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut state = self.state.lock();
        *state = KeyboardState::new();
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_keyboard_creation() {
        let kbd = KeyboardDevice::new();
        assert_eq!(kbd.name(), "Intel 8042 Keyboard");
        assert_eq!(kbd.device_type(), DeviceType::Input);
    }

    #[tokio::test]
    async fn test_keyboard_self_test() {
        let kbd = KeyboardDevice::new();

        // Send self-test command
        kbd.write_command(CMD_SELF_TEST);

        // Should return 0x55 (test passed)
        assert_eq!(kbd.read_data(), 0x55);
    }

    #[tokio::test]
    async fn test_keyboard_enable_disable() {
        let kbd = KeyboardDevice::new();

        // Disable keyboard
        kbd.write_command(CMD_DISABLE_KBD);

        // Inject scancode (should not appear)
        kbd.inject_scancode(0x1E); // 'A' key

        // Enable keyboard
        kbd.write_command(CMD_ENABLE_KBD);

        // Inject scancode (should appear)
        kbd.inject_scancode(0x1E);

        // Read scancode
        assert_eq!(kbd.read_data(), 0x1E);
    }

    #[tokio::test]
    async fn test_keyboard_command_byte() {
        let kbd = KeyboardDevice::new();

        // Read CCB
        kbd.write_command(CMD_READ_CCB);
        let ccb = kbd.read_data();

        // Should have interrupts enabled and translation on
        assert_eq!(ccb & CCB_INT_KBD, CCB_INT_KBD);
        assert_eq!(ccb & CCB_TRANSLATE, CCB_TRANSLATE);

        // Write new CCB (disable translation)
        kbd.write_command(CMD_WRITE_CCB);
        kbd.write_data(CCB_INT_KBD); // Keep interrupts, disable translation

        // Read back
        kbd.write_command(CMD_READ_CCB);
        let ccb = kbd.read_data();
        assert_eq!(ccb & CCB_TRANSLATE, 0);
    }

    #[tokio::test]
    async fn test_keyboard_id() {
        let kbd = KeyboardDevice::new();

        // Get keyboard ID
        kbd.write_data(KBD_GET_ID);

        // Should receive ACK + ID bytes
        assert_eq!(kbd.read_data(), KBD_ACK);
        assert_eq!(kbd.read_data(), KBD_ID1);
        assert_eq!(kbd.read_data(), KBD_ID2);
    }

    #[tokio::test]
    async fn test_keyboard_reset() {
        let kbd = KeyboardDevice::new();

        // Send reset command
        kbd.write_data(KBD_RESET);

        // Should receive ACK + self-test passed
        assert_eq!(kbd.read_data(), KBD_ACK);
        assert_eq!(kbd.read_data(), KBD_SELF_TEST_PASS);
    }

    #[tokio::test]
    async fn test_keyboard_scancode_buffer() {
        let kbd = KeyboardDevice::new();

        // Inject multiple scancodes
        kbd.inject_scancode(0x1E); // 'A'
        kbd.inject_scancode(0x9E); // 'A' release
        kbd.inject_scancode(0x30); // 'B'

        // Read them back in order
        assert_eq!(kbd.read_data(), 0x1E);
        assert_eq!(kbd.read_data(), 0x9E);
        assert_eq!(kbd.read_data(), 0x30);
    }

    #[tokio::test]
    async fn test_keyboard_status_obf() {
        let kbd = KeyboardDevice::new();

        // Initially no data
        let status = kbd.read_status();
        assert_eq!(status & STATUS_OBF, 0);

        // Inject scancode
        kbd.inject_scancode(0x1E);

        // Status should show OBF
        let status = kbd.read_status();
        assert_eq!(status & STATUS_OBF, STATUS_OBF);

        // Read data
        kbd.read_data();

        // Status should clear OBF
        let status = kbd.read_status();
        assert_eq!(status & STATUS_OBF, 0);
    }

    #[tokio::test]
    async fn test_keyboard_device_trait() {
        let mut kbd = KeyboardDevice::new();

        kbd.init().await.unwrap();

        let mut buf = [0u8; 1];
        kbd.read(KEYBOARD_STATUS_OFFSET, &mut buf).await.unwrap();

        kbd.write(KEYBOARD_STATUS_OFFSET, &[CMD_SELF_TEST])
            .await
            .unwrap();
        kbd.read(KEYBOARD_DATA_OFFSET, &mut buf).await.unwrap();

        kbd.reset().await.unwrap();
        kbd.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_keyboard_interrupt_flag() {
        let kbd = KeyboardDevice::new();

        // Inject scancode (should have interrupt pending)
        kbd.inject_scancode(0x1E);
        assert!(kbd.has_pending_interrupt());

        // Read data (should clear interrupt)
        kbd.read_data();
        assert!(!kbd.has_pending_interrupt());
    }

    #[tokio::test]
    async fn test_keyboard_irq_generation() {
        let pic = Arc::new(Pic8259::new());
        let kbd = KeyboardDevice::new();
        kbd.set_pic(Arc::clone(&pic));

        // Inject scancode - should raise IRQ 1
        kbd.inject_scancode(0x1E); // 'A' key

        // Verify IRQ 1 was raised (output buffer full + interrupts enabled)
        assert!(kbd.has_pending_interrupt());

        // Verify scancode is in buffer
        assert_eq!(kbd.read_data(), 0x1E);
    }

    #[tokio::test]
    async fn test_keyboard_irq_disabled() {
        let pic = Arc::new(Pic8259::new());
        let kbd = KeyboardDevice::new();
        kbd.set_pic(Arc::clone(&pic));

        // Disable keyboard interrupts
        kbd.write_command(CMD_WRITE_CCB);
        kbd.write_data(0x00); // No flags set (interrupts disabled)

        // Inject scancode
        kbd.inject_scancode(0x1E);

        // Should NOT have pending interrupt (interrupts disabled)
        assert!(!kbd.has_pending_interrupt());

        // But scancode should still be in buffer
        assert_eq!(kbd.read_data(), 0x1E);
    }

    #[tokio::test]
    async fn test_keyboard_multiple_scancodes() {
        let pic = Arc::new(Pic8259::new());
        let kbd = KeyboardDevice::new();
        kbd.set_pic(Arc::clone(&pic));

        // Inject multiple scancodes
        kbd.inject_scancode(0x1E); // 'A' press
        kbd.inject_scancode(0x9E); // 'A' release
        kbd.inject_scancode(0x30); // 'B' press
        kbd.inject_scancode(0xB0); // 'B' release

        // All should be in buffer
        assert_eq!(kbd.read_data(), 0x1E);
        assert_eq!(kbd.read_data(), 0x9E);
        assert_eq!(kbd.read_data(), 0x30);
        assert_eq!(kbd.read_data(), 0xB0);
    }

    #[tokio::test]
    async fn test_keyboard_buffer_overflow() {
        let pic = Arc::new(Pic8259::new());
        let kbd = KeyboardDevice::new();
        kbd.set_pic(Arc::clone(&pic));

        // Inject 20 scancodes (buffer limit is 16)
        for i in 0..20 {
            kbd.inject_scancode(i as u8);
        }

        // Should only get first 16
        for i in 0..16 {
            assert_eq!(kbd.read_data(), i as u8);
        }

        // Buffer should be empty now
        assert_eq!(kbd.read_data(), 0);
    }
    #[tokio::test]
    async fn the_controller_decodes_offsets_relative_to_its_base_port() {
        // Registered at 0x60, the status port arrives as offset 4. Decoding
        // the absolute 0x64 meant a guest polling the controller got a device
        // error, and a device error on the I/O path stops the VM.
        let keyboard = KeyboardDevice::new();
        let mut status = [0u8; 1];
        keyboard
            .read(KEYBOARD_STATUS_OFFSET, &mut status)
            .await
            .expect("the status port has to answer");
        assert_ne!(
            status[0], 0xFF,
            "0xff is what an absent device reads as, and a kernel polling for a \
             bit to clear in it waits forever"
        );
    }

    #[tokio::test]
    async fn the_ports_between_read_as_absent_rather_than_failing() {
        // 0x61 to 0x63 belong to other parts of the chipset. Erroring on them
        // would stop the VM for an access that hardware simply ignores.
        let keyboard = KeyboardDevice::new();
        let mut byte = [0u8; 1];
        keyboard.read(1, &mut byte).await.expect("no error");
        assert_eq!(byte[0], 0xFF);
    }
}
