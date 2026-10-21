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

use crate::input::ps2_mouse::{MouseButtons, Ps2Mouse};
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

/// Offset of the system-control port (0x61) from the controller's base port.
const SYSTEM_CONTROL_OFFSET: u64 = 1;

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
const CMD_WRITE_AUX: u8 = 0xD4; // Send the next data byte to the auxiliary device

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
/// The keyboard controller's line on a PC.
const KBD_IRQ: u8 = 1;

/// The auxiliary (mouse) port's line.
const AUX_IRQ: u8 = 12;

/// Bytes the controller's output buffer holds, from either device.
const OUTPUT_LIMIT: usize = 16;

#[derive(Debug)]
struct KeyboardState {
    /// Output buffer (devices to CPU), in arrival order. The flag marks a byte
    /// from the auxiliary device, which is what status bit 5 reports for the
    /// byte at the head.
    output_buffer: VecDeque<(u8, bool)>,
    /// The mouse on the auxiliary port.
    mouse: Ps2Mouse,
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
    /// Where an interrupt actually reaches a guest.
    ///
    /// The `Pic8259` above is a userspace model, and a guest whose interrupt
    /// controller lives inside the hypervisor never reads it. Without this,
    /// [`KeyboardDevice::inject_scancode`] put a byte in the output buffer that
    /// the guest was never told about: a public API for "give the guest a
    /// keystroke" whose interrupt had nowhere to go.
    interrupt_sink: Option<Arc<dyn crate::device::InterruptSink>>,
}

impl KeyboardState {
    fn new() -> Self {
        // A PS/2 mouse powers up with reporting off; the guest turns it on
        // with 0xF4. `Ps2Mouse::new` starts with it on.
        let mut mouse = Ps2Mouse::new();
        mouse.reset();
        Self {
            output_buffer: VecDeque::new(),
            mouse,
            input_buffer: None,
            status: STATUS_SYS | STATUS_UNLOCKED, // System initialized, not locked
            ccb: CCB_INT_KBD | CCB_TRANSLATE,     // Interrupts enabled, translation on
            current_command: None,
            kbd_enabled: true,
            scancode_set: 1, // Default to Set 1 (most compatible)
            led_state: 0,
            pic: None,
            interrupt_sink: None,
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

        // Bit 5 says the byte at the head came from the auxiliary device.
        if matches!(self.output_buffer.front(), Some((_, true))) {
            self.status |= STATUS_AUX_OBF;
        } else {
            self.status &= !STATUS_AUX_OBF;
        }

        if self.input_buffer.is_some() {
            self.status |= STATUS_IBF;
        } else {
            self.status &= !STATUS_IBF;
        }
    }

    /// Push data to output buffer
    fn push_output(&mut self, data: u8) {
        self.push_entry(data, false);
    }

    /// Push a byte from the keyboard (`aux` false) or the mouse (`aux` true).
    fn push_entry(&mut self, data: u8, aux: bool) {
        if self.output_buffer.len() < OUTPUT_LIMIT {
            self.output_buffer.push_back((data, aux));
            self.update_status();

            // Raise the device's IRQ if its interrupt is enabled.
            //
            // Both places, and the second is the one that reaches a guest: the
            // PIC below is a userspace model that a guest with an in-kernel
            // interrupt controller never reads, so raising only there is
            // indistinguishable from raising nothing.
            self.raise_irq(aux);
        }
    }

    /// Move what the mouse has to say into the output buffer, as far as there
    /// is room; the rest stays with the mouse until the guest drains some.
    fn pump_aux(&mut self) {
        while self.output_buffer.len() < OUTPUT_LIMIT && self.mouse.has_data() {
            if let Some(byte) = self.mouse.read() {
                self.push_entry(byte, true);
            }
        }
    }

    /// IRQ and command-byte enable bit for a byte from `aux`'s device.
    fn irq_for(aux: bool) -> (u8, u8) {
        if aux {
            (AUX_IRQ, CCB_INT_AUX)
        } else {
            (KBD_IRQ, CCB_INT_KBD)
        }
    }

    /// Raise the IRQ of the device a byte came from, if its interrupt is
    /// enabled in the command byte.
    fn raise_irq(&self, aux: bool) {
        let (irq, enable) = Self::irq_for(aux);
        if (self.ccb & enable) != 0 {
            if let Some(ref pic) = self.pic {
                let _ = pic.raise_irq(irq);
            }
            if let Some(sink) = &self.interrupt_sink {
                sink.raise(irq);
            }
        }
    }

    /// Pop data from output buffer
    fn pop_output(&mut self) -> Option<u8> {
        let data = self.output_buffer.pop_front().map(|(byte, _)| byte);
        self.update_status();
        // Mouse bytes that did not fit move in now that there is room.
        self.pump_aux();
        // The controller loads the next byte into the output buffer as soon as
        // the guest has read one, and that raises IRQ 1 again. Without this a
        // guest that reads one byte per interrupt, as a handler does, saw the
        // first byte of a make/break sequence and never the rest: they were
        // queued, with no interrupt left to announce them.
        //
        // Only on the userspace PIC. The interrupt sink queues its pulses for a
        // delivery task, so a pulse raised here can reach an in-kernel
        // controller after the guest has drained the buffer, and every such
        // stale pulse is an interrupt that reads an empty buffer (seen on KVM:
        // a seventh IRQ 1 for six bytes). The userspace PIC latches the edge
        // at once, so there the count is exact.
        if let (Some(_), Some(&(_, aux))) = (data, self.output_buffer.front()) {
            let (irq, enable) = Self::irq_for(aux);
            if (self.ccb & enable) != 0 {
                if let Some(ref pic) = self.pic {
                    let _ = pic.raise_irq(irq);
                }
            }
        }
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
            CMD_ENABLE_AUX => {
                self.ccb &= !CCB_DISABLE_AUX;
            }
            CMD_DISABLE_AUX => {
                self.ccb |= CCB_DISABLE_AUX;
            }
            CMD_TEST_AUX => {
                // Auxiliary interface test always passes
                self.push_output(0x00);
            }
            CMD_WRITE_AUX => {
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
                // A keyboard reset does not touch what the mouse has said.
                self.output_buffer.retain(|&(_, aux)| aux);
                self.update_status();
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
                CMD_WRITE_AUX => {
                    // The byte goes to the mouse, not the keyboard. A guest
                    // that disabled the auxiliary port gets no answer.
                    self.current_command = None;
                    if (self.ccb & CCB_DISABLE_AUX) == 0 {
                        self.mouse.write(data);
                        self.pump_aux();
                    }
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
    /// Port 0x61, inside this controller's range but a different chip. Set by
    /// the VM where the hypervisor does not provide the timer behind it.
    system_control: Mutex<Option<Arc<crate::devices::timer::SystemControlPort>>>,
}

impl KeyboardDevice {
    /// Create a new keyboard controller
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(KeyboardState::new())),
            system_control: Mutex::new(None),
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
        match state.output_buffer.front() {
            Some(&(_, aux)) => (state.ccb & KeyboardState::irq_for(aux).1) != 0,
            None => false,
        }
    }
}

impl Default for KeyboardDevice {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for KeyboardDevice {
    fn pending_interrupt(&self) -> Option<u8> {
        // The inherent `has_pending_interrupt` existed and this trait method
        // did not, so the dispatch layer could never see this device's
        // interrupt -- the same defect the RTC carried, and for the same
        // reason: a public check nothing on the delivery path called.
        if !self.has_pending_interrupt() {
            return None;
        }
        let aux = matches!(self.state.lock().output_buffer.front(), Some((_, true)));
        Some(if aux { AUX_IRQ } else { KBD_IRQ })
    }

    /// Host pointer motion as PS/2 packets on the auxiliary port.
    ///
    /// A move larger than one packet can carry (-255 to 255 on an axis) is
    /// split into several, all with `buttons`. All or nothing, like
    /// [`Self::console_input`]: the controller's 16-byte output buffer has to
    /// hold every packet, or none is queued.
    fn pointer_input(&mut self, dx: i32, dy: i32, buttons: u8) -> Result<()> {
        if buttons > 0x07 {
            return Err(crate::Error::Device(format!(
                "pointer buttons {buttons:#x}: only bits 0 to 2 (left, right, middle) exist"
            )));
        }
        let mut state = self.state.lock();
        if (state.ccb & CCB_DISABLE_AUX) != 0 {
            return Err(crate::Error::Device(
                "the mouse port is disabled by the guest".to_string(),
            ));
        }
        if !state.mouse.is_reporting_enabled() {
            return Err(crate::Error::Device(
                "the guest has not enabled mouse reporting (0xF4)".to_string(),
            ));
        }
        let changed = buttons != state.mouse.buttons().to_byte();
        if dx == 0 && dy == 0 && !changed {
            return Ok(());
        }
        const STEP: i32 = 255;
        let packets = ((dx.unsigned_abs().div_ceil(STEP as u32))
            .max(dy.unsigned_abs().div_ceil(STEP as u32))
            .max(1)) as usize;
        let size = state.mouse.protocol().packet_size();
        let room = OUTPUT_LIMIT
            .saturating_sub(state.output_buffer.len())
            .saturating_sub(state.mouse.output_len());
        if packets * size > room {
            return Err(crate::Error::Device(format!(
                "the mouse buffer has room for {room} more byte(s), {} were needed",
                packets * size
            )));
        }
        let (mut rx, mut ry) = (dx, dy);
        let held = MouseButtons::from_byte(buttons);
        for _ in 0..packets {
            let (sx, sy) = (rx.clamp(-STEP, STEP), ry.clamp(-STEP, STEP));
            state.mouse.move_with_buttons(sx as i16, sy as i16, held);
            rx -= sx;
            ry -= sy;
        }
        state.pump_aux();
        Ok(())
    }

    fn set_interrupt_sink(&mut self, sink: Arc<dyn crate::device::InterruptSink>) {
        self.state.lock().interrupt_sink = Some(sink);
    }

    fn connect_pic(&self, pic: Arc<Pic8259>) {
        self.set_pic(pic);
    }

    fn connect_system_control(&self, port: Arc<crate::devices::timer::SystemControlPort>) {
        *self.system_control.lock() = Some(port);
    }

    /// Host input is PS/2 scancodes (set 1, make and break), queued for the
    /// guest to read at port 0x60. A press is `0x1E`, its release `0x9E`; an
    /// extended key such as the left Super key is `E0 5B` and `E0 DB`.
    ///
    /// All or nothing: the controller's output buffer holds 16 bytes and the
    /// guest drains it one interrupt at a time, so bytes that do not fit are
    /// refused rather than dropped, and a caller can send the rest later.
    fn console_input(&mut self, data: &[u8]) -> Result<()> {
        let mut state = self.state.lock();
        if !state.kbd_enabled {
            return Err(crate::Error::Device(
                "the keyboard is disabled by the guest".to_string(),
            ));
        }
        let room = OUTPUT_LIMIT
            .saturating_sub(state.output_buffer.len())
            .saturating_sub(state.mouse.output_len());
        if data.len() > room {
            return Err(crate::Error::Device(format!(
                "the keyboard buffer has room for {room} more byte(s), {} were sent",
                data.len()
            )));
        }
        for &byte in data {
            state.push_output(byte);
        }
        Ok(())
    }

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
                SYSTEM_CONTROL_OFFSET => self
                    .system_control
                    .lock()
                    .as_ref()
                    .map_or(0xFF, |port| port.read_byte()),
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
                SYSTEM_CONTROL_OFFSET => {
                    if let Some(port) = self.system_control.lock().as_ref() {
                        port.write_byte(*byte);
                    }
                }
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

    /// Counts the pulses a device raises.
    #[derive(Debug, Default)]
    struct CountingSink(std::sync::atomic::AtomicUsize);

    impl crate::device::InterruptSink for CountingSink {
        fn raise(&self, irq: u8) {
            assert_eq!(irq, KBD_IRQ);
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// A guest reads one byte per interrupt. Four bytes queued at once must
    /// therefore leave IRQ 1 pending again after each read until the buffer is
    /// empty, or the last three wait for a keypress that may never come. On the
    /// userspace PIC, where the edge is latched at once and the count is exact.
    #[tokio::test]
    async fn each_byte_read_leaves_irq1_pending_until_the_buffer_is_empty() {
        let pic = Arc::new(Pic8259::new());
        // The sequence a kernel sends: master base 0x20, only IRQ 1 open.
        for (port, value) in [
            (0x20, 0x11),
            (0x21, 0x20),
            (0x21, 0x04),
            (0x21, 0x01),
            (0x21, 0xFD),
        ] {
            pic.write_port(port, value).await.unwrap();
        }
        let mut kbd = KeyboardDevice::new();
        kbd.set_pic(Arc::clone(&pic));

        // Left Super down, Return down, Return up, Super up.
        kbd.console_input(&[0xE0, 0x5B, 0x1C, 0x9C]).unwrap();

        let mut seen = Vec::new();
        let mut interrupts = 0;
        while let Some(vector) = pic.get_pending_interrupt() {
            assert_eq!(vector, 0x21, "IRQ 1 with base 0x20");
            pic.acknowledge_interrupt(vector).unwrap();
            seen.push(kbd.read_data());
            pic.send_eoi(vector).unwrap();
            interrupts += 1;
            assert!(interrupts <= 8, "no more interrupts than bytes");
        }
        assert_eq!(seen, [0xE0, 0x5B, 0x1C, 0x9C], "in order, none lost");
        assert_eq!(interrupts, 4, "one interrupt per byte, none left over");
    }

    /// The sink queues its pulses for a task that delivers them later, so a
    /// re-raise through it can arrive after the buffer is empty and read as an
    /// interrupt with nothing behind it. Only the pushes pulse it.
    #[tokio::test]
    async fn the_interrupt_sink_gets_one_pulse_per_queued_byte() {
        let mut kbd = KeyboardDevice::new();
        let sink = Arc::new(CountingSink::default());
        kbd.set_interrupt_sink(sink.clone());
        kbd.console_input(&[0xE0, 0x5B, 0x1C, 0x9C]).unwrap();
        for _ in 0..4 {
            kbd.read_data();
        }
        assert_eq!(sink.0.load(std::sync::atomic::Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn host_input_is_all_or_nothing() {
        let mut kbd = KeyboardDevice::new();
        kbd.console_input(&[0x1E; 10]).unwrap();
        let refused = kbd.console_input(&[0x30; 7]);
        assert!(refused.is_err(), "7 more do not fit in the 16-byte buffer");
        // Nothing of the refused call was queued: 6 more still fit.
        kbd.console_input(&[0x30; 6]).unwrap();
        assert!(kbd.console_input(&[0x30]).is_err(), "now full");
    }

    /// Write `byte` to the mouse the way a guest does: 0xD4 on 0x64, then the
    /// byte on 0x60.
    fn to_mouse(kbd: &KeyboardDevice, byte: u8) {
        kbd.write_command(CMD_WRITE_AUX);
        kbd.write_data(byte);
    }

    /// Read one byte as a polling guest does, returning it with status bit 5.
    fn poll_byte(kbd: &KeyboardDevice) -> (u8, bool) {
        let status = kbd.read_status();
        assert_ne!(status & STATUS_OBF, 0, "a byte should be waiting");
        (kbd.read_data(), status & STATUS_AUX_OBF != 0)
    }

    #[tokio::test]
    async fn the_mouse_answers_commands_sent_through_0xd4() {
        let kbd = KeyboardDevice::new();
        kbd.write_command(CMD_ENABLE_AUX);
        to_mouse(&kbd, 0xF6); // defaults
        assert_eq!(poll_byte(&kbd), (0xFA, true), "an ACK, flagged as aux");
        to_mouse(&kbd, 0xF4); // enable reporting
        assert_eq!(poll_byte(&kbd), (0xFA, true));
        // A byte for the keyboard is not a byte for the mouse.
        kbd.write_data(KBD_ECHO);
        assert_eq!(poll_byte(&kbd), (KBD_ECHO, false), "no aux flag");
    }

    #[tokio::test]
    async fn a_pointer_move_arrives_as_a_three_byte_packet() {
        let mut kbd = KeyboardDevice::new();
        assert!(
            kbd.pointer_input(5, 5, 0).is_err(),
            "reporting is off until the guest turns it on"
        );
        to_mouse(&kbd, 0xF4);
        poll_byte(&kbd);
        // Right 10, down 3 (PS/2 y is up), left button held.
        kbd.pointer_input(10, -3, 1).unwrap();
        let packet: Vec<_> = (0..3).map(|_| poll_byte(&kbd)).collect();
        assert!(packet.iter().all(|&(_, aux)| aux));
        assert_eq!(packet[0].0, 0x08 | 0x01 | 0x20, "bit 3, left, y negative");
        assert_eq!(packet[1].0, 10);
        assert_eq!(packet[2].0, (-3i8) as u8);
    }

    #[tokio::test]
    async fn a_large_move_is_split_and_a_full_buffer_refuses_all_of_it() {
        let mut kbd = KeyboardDevice::new();
        to_mouse(&kbd, 0xF4);
        poll_byte(&kbd);
        // 600 right is three packets: 255 + 255 + 90.
        kbd.pointer_input(600, 0, 0).unwrap();
        let bytes: Vec<u8> = (0..9).map(|_| poll_byte(&kbd).0).collect();
        assert_eq!([bytes[1], bytes[4], bytes[7]], [255, 255, 90]);
        // 16 bytes hold five packets, not six; the refusal queues nothing.
        assert!(kbd.pointer_input(6 * 255, 0, 0).is_err());
        assert_eq!(kbd.read_status() & STATUS_OBF, 0, "nothing was queued");
        kbd.pointer_input(5 * 255, 0, 0).unwrap();
    }

    #[tokio::test]
    async fn the_mouse_interrupt_is_irq_12_and_only_when_enabled() {
        let mut kbd = KeyboardDevice::new();
        #[derive(Debug, Default)]
        struct Recorder(std::sync::Mutex<Vec<u8>>);
        impl crate::device::InterruptSink for Recorder {
            fn raise(&self, irq: u8) {
                self.0.lock().unwrap().push(irq);
            }
        }
        let sink = Arc::new(Recorder::default());
        kbd.set_interrupt_sink(sink.clone());
        to_mouse(&kbd, 0xF4);
        poll_byte(&kbd);
        kbd.pointer_input(1, 1, 0).unwrap();
        assert!(
            sink.0.lock().unwrap().is_empty(),
            "the aux interrupt is off in the default command byte"
        );
        for _ in 0..3 {
            poll_byte(&kbd);
        }
        kbd.write_command(CMD_WRITE_CCB);
        kbd.write_data(CCB_INT_KBD | CCB_INT_AUX | CCB_TRANSLATE);
        kbd.pointer_input(1, 1, 0).unwrap();
        assert_eq!(
            *sink.0.lock().unwrap(),
            [AUX_IRQ; 3],
            "IRQ 12, one pulse per packet byte"
        );
        assert_eq!(kbd.pending_interrupt(), Some(AUX_IRQ));
    }

    #[tokio::test]
    async fn a_disabled_mouse_port_takes_no_pointer_input() {
        let mut kbd = KeyboardDevice::new();
        to_mouse(&kbd, 0xF4);
        poll_byte(&kbd);
        kbd.write_command(CMD_DISABLE_AUX);
        assert!(kbd.pointer_input(1, 1, 0).is_err());
        assert!(kbd.pointer_input(0, 0, 8).is_err(), "no fourth button");
    }

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

    /// A keystroke injected from the host interrupts the guest.
    ///
    /// `inject_scancode` is a public API for "give the guest a keystroke", and
    /// it used to raise only on the userspace `Pic8259` — which a guest whose
    /// interrupt controller lives inside the hypervisor never reads. The byte
    /// went into the output buffer and the guest was never told, so a guest
    /// waiting on the keyboard waited forever while the host believed it had
    /// typed. Raising on nothing and raising on something a guest cannot see
    /// are the same failure, and neither shows up in a test that only checks
    /// the buffer.
    #[test]
    fn an_injected_keystroke_reaches_a_guest_that_does_not_read_the_userspace_pic() {
        #[derive(Debug, Default)]
        struct Recorder(std::sync::Mutex<Vec<u8>>);
        impl crate::device::InterruptSink for Recorder {
            fn raise(&self, irq: u8) {
                self.0.lock().expect("recorder").push(irq);
            }
        }

        let raised = Arc::new(Recorder::default());
        let mut kbd = KeyboardDevice::new();
        kbd.set_interrupt_sink(raised.clone());

        kbd.inject_scancode(0x1e);

        assert_eq!(
            *raised.0.lock().expect("recorder"),
            vec![1],
            "injecting a scancode should raise IRQ 1 somewhere the guest reads"
        );
        assert!(
            kbd.has_pending_interrupt(),
            "and the device should still report the interrupt as pending"
        );
        assert_eq!(
            Device::pending_interrupt(&kbd),
            Some(1),
            "the dispatch layer sees it through the trait, not the inherent method"
        );
    }
}
