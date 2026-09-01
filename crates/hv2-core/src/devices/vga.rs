//! VGA Text Mode Display
//!
//! This module implements a simple VGA text mode (80x25) display buffer.
//! It provides:
//! - 80x25 character text buffer
//! - 16-color text attributes
//! - Cursor position tracking
//! - CRTC register emulation
//!
//!
//! # Not wired to the device manager
//!
//! Nothing registers this device with [`crate::DeviceManager`]. Its `Device`
//! implementation decodes **absolute** ports (0x3C0..=0x3D5) while the manager
//! passes `port - base_port`, and it returns `Err` for any access that is not
//! one byte -- an error there stops the VM. Registered as-is it would decode
//! nothing and kill the guest on the first wide access.
//!
//! Unlike the IDE controller this is a single contiguous range, so the fix is
//! mechanical: decode relative to a `VGA_BASE` of 0x3C0, walk consecutive
//! ports for a wide access, and read an unimplemented port as an absent device
//! rather than as a failure. It has not been done because nothing needs it
//! yet, and untested changes to a device no guest touches are not worth the
//! risk they carry.
//! Memory Map:
//! - 0xB8000-0xBFFFF: Text buffer (32KB, 4KB used for 80x25 mode)
//!
//! I/O Ports:
//! - 0x3D4: CRTC Index Register
//! - 0x3D5: CRTC Data Register

use crate::{Device, DeviceType, Error, Result};
use async_trait::async_trait;
use std::sync::Arc;

use parking_lot::Mutex;

/// VGA text mode dimensions
pub const VGA_WIDTH: usize = 80;
pub const VGA_HEIGHT: usize = 25;
pub const VGA_SIZE: usize = VGA_WIDTH * VGA_HEIGHT * 2; // 2 bytes per character

/// VGA text buffer base address
pub const VGA_TEXT_BASE: u64 = 0xB8000;

/// CRTC register indices
const CRTC_CURSOR_START: u8 = 0x0A;
const CRTC_CURSOR_END: u8 = 0x0B;
const CRTC_CURSOR_LOC_HIGH: u8 = 0x0E;
const CRTC_CURSOR_LOC_LOW: u8 = 0x0F;

/// VGA color codes
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VgaColor {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    LightMagenta = 13,
    Yellow = 14,
    White = 15,
}

impl VgaColor {
    /// Safely convert a `u8` to a `VgaColor`, defaulting to `Black` for out-of-range values.
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Black,
            1 => Self::Blue,
            2 => Self::Green,
            3 => Self::Cyan,
            4 => Self::Red,
            5 => Self::Magenta,
            6 => Self::Brown,
            7 => Self::LightGray,
            8 => Self::DarkGray,
            9 => Self::LightBlue,
            10 => Self::LightGreen,
            11 => Self::LightCyan,
            12 => Self::LightRed,
            13 => Self::LightMagenta,
            14 => Self::Yellow,
            15 => Self::White,
            _ => Self::Black,
        }
    }
}

/// VGA character attribute
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VgaAttribute {
    pub foreground: VgaColor,
    pub background: VgaColor,
    pub blink: bool,
}

impl VgaAttribute {
    pub fn new(foreground: VgaColor, background: VgaColor) -> Self {
        Self {
            foreground,
            background,
            blink: false,
        }
    }

    pub fn to_byte(self) -> u8 {
        let fg = self.foreground as u8;
        let bg = self.background as u8;
        let blink = if self.blink { 0x80 } else { 0x00 };
        (bg << 4) | fg | blink
    }

    pub fn from_byte(byte: u8) -> Self {
        let fg = byte & 0x0F;
        let bg = (byte >> 4) & 0x07;
        let blink = (byte & 0x80) != 0;

        Self {
            foreground: VgaColor::from_u8(fg),
            background: VgaColor::from_u8(bg),
            blink,
        }
    }
}

/// VGA character cell
#[derive(Debug, Clone, Copy)]
struct VgaCell {
    character: u8,
    attribute: VgaAttribute,
}

impl VgaCell {
    fn blank() -> Self {
        Self {
            character: b' ',
            attribute: VgaAttribute::new(VgaColor::LightGray, VgaColor::Black),
        }
    }
}

/// Internal VGA state
#[derive(Debug)]
struct VgaState {
    /// Text buffer (80x25 characters + attributes)
    buffer: [VgaCell; VGA_WIDTH * VGA_HEIGHT],
    /// Cursor position (linear index)
    cursor_pos: u16,
    /// CRTC registers
    crtc_index: u8,
    crtc_regs: [u8; 256],
    /// Sequencer registers (0x3C4/0x3C5)
    seq_index: u8,
    seq_regs: [u8; 5],
    /// Graphics controller registers (0x3CE/0x3CF)
    gc_index: u8,
    gc_regs: [u8; 9],
    /// Attribute controller registers (0x3C0/0x3C1)
    ac_index: u8,
    ac_regs: [u8; 21],
    /// Attribute controller flip-flop (false=index, true=data)
    ac_flip_flop: bool,
    /// Miscellaneous output register (0x3C2 write / 0x3CC read)
    misc_output: u8,
    /// DAC palette state
    dac_read_index: u8,
    dac_write_index: u8,
    dac_component: u8,
    palette: [[u8; 3]; 256],
    /// DAC mask register (0x3C6)
    dac_mask: u8,
}

impl VgaState {
    fn new() -> Self {
        Self {
            buffer: [VgaCell::blank(); VGA_WIDTH * VGA_HEIGHT],
            cursor_pos: 0,
            crtc_index: 0,
            crtc_regs: [0; 256],
            seq_index: 0,
            seq_regs: [0x03, 0x00, 0x03, 0x00, 0x02], // Typical text-mode defaults
            gc_index: 0,
            gc_regs: [0; 9],
            ac_index: 0,
            ac_regs: [0; 21],
            ac_flip_flop: false,
            misc_output: 0x67, // Color mode, RAM enabled, clock select
            dac_read_index: 0,
            dac_write_index: 0,
            dac_component: 0,
            palette: [[0; 3]; 256],
            dac_mask: 0xFF,
        }
    }

    /// Get cursor position (row, col)
    fn get_cursor(&self) -> (usize, usize) {
        let pos = self.cursor_pos as usize;
        (pos / VGA_WIDTH, pos % VGA_WIDTH)
    }

    /// Set cursor position
    fn set_cursor(&mut self, row: usize, col: usize) {
        let pos = (row * VGA_WIDTH + col).min(VGA_WIDTH * VGA_HEIGHT - 1);
        self.cursor_pos = pos as u16;
        self.crtc_regs[CRTC_CURSOR_LOC_HIGH as usize] = (pos >> 8) as u8;
        self.crtc_regs[CRTC_CURSOR_LOC_LOW as usize] = (pos & 0xFF) as u8;
    }

    /// Read from text buffer
    fn read_buffer(&self, offset: usize) -> u8 {
        let index = offset / 2;
        if index < self.buffer.len() {
            if offset.is_multiple_of(2) {
                self.buffer[index].character
            } else {
                self.buffer[index].attribute.to_byte()
            }
        } else {
            0
        }
    }

    /// Write to text buffer
    fn write_buffer(&mut self, offset: usize, value: u8) {
        let index = offset / 2;
        if index < self.buffer.len() {
            if offset.is_multiple_of(2) {
                self.buffer[index].character = value;
            } else {
                self.buffer[index].attribute = VgaAttribute::from_byte(value);
            }
        }
    }

    /// Clear screen
    fn clear(&mut self) {
        self.buffer = [VgaCell::blank(); VGA_WIDTH * VGA_HEIGHT];
        self.cursor_pos = 0;
    }

    /// Scroll up by one line
    fn scroll_up(&mut self) {
        // Move all lines up by one
        for row in 1..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                let src_idx = row * VGA_WIDTH + col;
                let dst_idx = (row - 1) * VGA_WIDTH + col;
                self.buffer[dst_idx] = self.buffer[src_idx];
            }
        }
        // Clear the last line
        for col in 0..VGA_WIDTH {
            let idx = (VGA_HEIGHT - 1) * VGA_WIDTH + col;
            self.buffer[idx] = VgaCell::blank();
        }
    }

    /// Write character at position with auto-scroll
    fn put_char(&mut self, ch: u8, attr: VgaAttribute) {
        let (mut row, mut col) = self.get_cursor();

        match ch {
            b'\n' => {
                // Newline: move to start of next line
                row += 1;
                col = 0;
            }
            b'\r' => {
                // Carriage return: move to start of current line
                col = 0;
            }
            b'\t' => {
                // Tab: move to next 8-column boundary
                col = (col + 8) & !7;
                if col >= VGA_WIDTH {
                    col = 0;
                    row += 1;
                }
            }
            b'\x08' => {
                // Backspace: move back one position
                if col > 0 {
                    col -= 1;
                } else if row > 0 {
                    row -= 1;
                    col = VGA_WIDTH - 1;
                }
            }
            _ => {
                // Normal character: write and advance
                if row < VGA_HEIGHT {
                    let idx = row * VGA_WIDTH + col;
                    self.buffer[idx] = VgaCell {
                        character: ch,
                        attribute: attr,
                    };
                }
                col += 1;
                if col >= VGA_WIDTH {
                    col = 0;
                    row += 1;
                }
            }
        }

        // Handle scrolling if we've gone past the bottom
        while row >= VGA_HEIGHT {
            self.scroll_up();
            row -= 1;
        }

        self.set_cursor(row, col);
    }

    /// Fill region with character
    fn fill_region(
        &mut self,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
        ch: u8,
        attr: VgaAttribute,
    ) {
        for row in start_row..=end_row.min(VGA_HEIGHT - 1) {
            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row {
                end_col
            } else {
                VGA_WIDTH - 1
            };

            for col in col_start..=col_end.min(VGA_WIDTH - 1) {
                let idx = row * VGA_WIDTH + col;
                self.buffer[idx] = VgaCell {
                    character: ch,
                    attribute: attr,
                };
            }
        }
    }

    /// Get text content as string (for debugging/testing)
    fn get_text(&self) -> String {
        let mut result = String::new();
        for row in 0..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                let cell = self.buffer[row * VGA_WIDTH + col];
                result.push(cell.character as char);
            }
            result.push('\n');
        }
        result
    }
}

/// VGA Text Mode Display Device
///
/// This device emulates a simple VGA text mode display.
/// It provides a 80x25 character buffer at 0xB8000.
#[derive(Debug)]
pub struct VgaDevice {
    state: Arc<Mutex<VgaState>>,
}

impl VgaDevice {
    /// Create a new VGA device
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(VgaState::new())),
        }
    }

    /// Read from CRTC index register (0x3D4)
    pub fn read_crtc_index(&self) -> u8 {
        self.state.lock().crtc_index
    }

    /// Write to CRTC index register (0x3D4)
    pub fn write_crtc_index(&self, value: u8) {
        self.state.lock().crtc_index = value;
    }

    /// Read from CRTC data register (0x3D5)
    pub fn read_crtc_data(&self) -> u8 {
        let state = self.state.lock();
        state.crtc_regs[state.crtc_index as usize]
    }

    /// Write to CRTC data register (0x3D5)
    pub fn write_crtc_data(&self, value: u8) {
        let mut state = self.state.lock();
        let index = state.crtc_index;
        state.crtc_regs[index as usize] = value;

        // Handle special registers
        match index {
            CRTC_CURSOR_LOC_HIGH => {
                let low = state.crtc_regs[CRTC_CURSOR_LOC_LOW as usize];
                state.cursor_pos = ((value as u16) << 8) | (low as u16);
            }
            CRTC_CURSOR_LOC_LOW => {
                let high = state.crtc_regs[CRTC_CURSOR_LOC_HIGH as usize];
                state.cursor_pos = ((high as u16) << 8) | (value as u16);
            }
            _ => {}
        }
    }

    // --- Sequencer (0x3C4/0x3C5) ---

    /// Read sequencer index register
    pub fn read_seq_index(&self) -> u8 {
        self.state.lock().seq_index
    }

    /// Write sequencer index register
    pub fn write_seq_index(&self, value: u8) {
        self.state.lock().seq_index = value & 0x07;
    }

    /// Read sequencer data register
    pub fn read_seq_data(&self) -> u8 {
        let state = self.state.lock();
        let idx = state.seq_index as usize;
        if idx < state.seq_regs.len() {
            state.seq_regs[idx]
        } else {
            0
        }
    }

    /// Write sequencer data register
    pub fn write_seq_data(&self, value: u8) {
        let mut state = self.state.lock();
        let idx = state.seq_index as usize;
        if idx < state.seq_regs.len() {
            state.seq_regs[idx] = value;
        }
    }

    // --- Graphics Controller (0x3CE/0x3CF) ---

    /// Read graphics controller index register
    pub fn read_gc_index(&self) -> u8 {
        self.state.lock().gc_index
    }

    /// Write graphics controller index register
    pub fn write_gc_index(&self, value: u8) {
        self.state.lock().gc_index = value & 0x0F;
    }

    /// Read graphics controller data register
    pub fn read_gc_data(&self) -> u8 {
        let state = self.state.lock();
        let idx = state.gc_index as usize;
        if idx < state.gc_regs.len() {
            state.gc_regs[idx]
        } else {
            0
        }
    }

    /// Write graphics controller data register
    pub fn write_gc_data(&self, value: u8) {
        let mut state = self.state.lock();
        let idx = state.gc_index as usize;
        if idx < state.gc_regs.len() {
            state.gc_regs[idx] = value;
        }
    }

    // --- Attribute Controller (0x3C0/0x3C1) ---

    /// Read attribute controller (0x3C1)
    pub fn read_ac(&self) -> u8 {
        let state = self.state.lock();
        let idx = state.ac_index as usize;
        if idx < state.ac_regs.len() {
            state.ac_regs[idx]
        } else {
            0
        }
    }

    /// Write attribute controller (0x3C0) — alternates index/data via flip-flop
    pub fn write_ac(&self, value: u8) {
        let mut state = self.state.lock();
        if !state.ac_flip_flop {
            // Index write
            state.ac_index = value & 0x1F;
        } else {
            // Data write
            let idx = state.ac_index as usize;
            if idx < state.ac_regs.len() {
                state.ac_regs[idx] = value;
            }
        }
        state.ac_flip_flop = !state.ac_flip_flop;
    }

    // --- Miscellaneous Output (0x3C2 write / 0x3CC read) ---

    /// Read miscellaneous output register
    pub fn read_misc_output(&self) -> u8 {
        self.state.lock().misc_output
    }

    /// Write miscellaneous output register
    pub fn write_misc_output(&self, value: u8) {
        self.state.lock().misc_output = value;
    }

    // --- DAC / Palette (0x3C6–0x3C9) ---

    /// Read DAC mask register (0x3C6)
    pub fn read_dac_mask(&self) -> u8 {
        self.state.lock().dac_mask
    }

    /// Write DAC mask register (0x3C6)
    pub fn write_dac_mask(&self, value: u8) {
        self.state.lock().dac_mask = value;
    }

    /// Write DAC read index (0x3C7)
    pub fn write_dac_read_index(&self, value: u8) {
        let mut state = self.state.lock();
        state.dac_read_index = value;
        state.dac_component = 0;
    }

    /// Write DAC write index (0x3C8)
    pub fn write_dac_write_index(&self, value: u8) {
        let mut state = self.state.lock();
        state.dac_write_index = value;
        state.dac_component = 0;
    }

    /// Read DAC data (0x3C9) — returns R/G/B components sequentially
    pub fn read_dac_data(&self) -> u8 {
        let mut state = self.state.lock();
        let idx = state.dac_read_index as usize;
        let comp = state.dac_component as usize;
        let value = state.palette[idx][comp];
        state.dac_component += 1;
        if state.dac_component >= 3 {
            state.dac_component = 0;
            state.dac_read_index = state.dac_read_index.wrapping_add(1);
        }
        value
    }

    /// Write DAC data (0x3C9) — sets R/G/B components sequentially
    pub fn write_dac_data(&self, value: u8) {
        let mut state = self.state.lock();
        let idx = state.dac_write_index as usize;
        let comp = state.dac_component as usize;
        state.palette[idx][comp] = value;
        state.dac_component += 1;
        if state.dac_component >= 3 {
            state.dac_component = 0;
            state.dac_write_index = state.dac_write_index.wrapping_add(1);
        }
    }

    /// Read from text buffer (MMIO)
    pub fn read_buffer(&self, offset: u64) -> Result<u8> {
        if offset >= VGA_SIZE as u64 {
            return Err(Error::Device(format!(
                "VGA buffer read out of range: {:#x}",
                offset
            )));
        }
        Ok(self.state.lock().read_buffer(offset as usize))
    }

    /// Write to text buffer (MMIO)
    pub fn write_buffer(&self, offset: u64, value: u8) -> Result<()> {
        if offset >= VGA_SIZE as u64 {
            return Err(Error::Device(format!(
                "VGA buffer write out of range: {:#x}",
                offset
            )));
        }
        self.state.lock().write_buffer(offset as usize, value);
        Ok(())
    }

    /// Clear screen
    pub fn clear(&self) {
        self.state.lock().clear();
    }

    /// Get cursor position
    pub fn get_cursor(&self) -> (usize, usize) {
        self.state.lock().get_cursor()
    }

    /// Set cursor position
    pub fn set_cursor(&self, row: usize, col: usize) {
        self.state.lock().set_cursor(row, col);
    }

    /// Get text content (for debugging)
    pub fn get_text(&self) -> String {
        self.state.lock().get_text()
    }

    /// Write string at current cursor position
    pub fn write_string(&self, s: &str, attr: VgaAttribute) {
        let mut state = self.state.lock();
        let mut pos = state.cursor_pos as usize;

        for ch in s.chars() {
            if pos >= VGA_WIDTH * VGA_HEIGHT {
                break;
            }

            if ch == '\n' {
                pos = ((pos / VGA_WIDTH) + 1) * VGA_WIDTH;
            } else {
                state.buffer[pos] = VgaCell {
                    character: ch as u8,
                    attribute: attr,
                };
                pos += 1;
            }
        }

        state.cursor_pos = pos.min(VGA_WIDTH * VGA_HEIGHT - 1) as u16;
    }

    /// Write character at cursor with auto-scroll
    pub fn put_char(&self, ch: u8, attr: VgaAttribute) {
        self.state.lock().put_char(ch, attr);
    }

    /// Write string with auto-scroll support
    pub fn put_string(&self, s: &str, attr: VgaAttribute) {
        let mut state = self.state.lock();
        for ch in s.bytes() {
            state.put_char(ch, attr);
        }
    }

    /// Scroll display up by one line
    pub fn scroll_up(&self) {
        self.state.lock().scroll_up();
    }

    /// Fill region with character
    pub fn fill_region(
        &self,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
        ch: u8,
        attr: VgaAttribute,
    ) {
        self.state
            .lock()
            .fill_region(start_row, start_col, end_row, end_col, ch, attr);
    }

    /// Get character at position
    pub fn get_char(&self, row: usize, col: usize) -> Option<(u8, VgaAttribute)> {
        if row >= VGA_HEIGHT || col >= VGA_WIDTH {
            return None;
        }
        let state = self.state.lock();
        let idx = row * VGA_WIDTH + col;
        let cell = state.buffer[idx];
        Some((cell.character, cell.attribute))
    }
}

impl Default for VgaDevice {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for VgaDevice {
    fn name(&self) -> &str {
        "VGA Text Mode"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Display
    }

    async fn init(&mut self) -> Result<()> {
        self.clear();
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(Error::Device("VGA only supports single-byte reads".into()));
        }

        let value = match offset {
            0x3C0 => self.state.lock().ac_index,
            0x3C1 => self.read_ac(),
            0x3C4 => self.read_seq_index(),
            0x3C5 => self.read_seq_data(),
            0x3C6 => self.read_dac_mask(),
            0x3C7 => 0, // DAC state: write mode indicator
            0x3C8 => self.state.lock().dac_write_index,
            0x3C9 => self.read_dac_data(),
            0x3CC => self.read_misc_output(),
            0x3CE => self.read_gc_index(),
            0x3CF => self.read_gc_data(),
            0x3D4 => self.read_crtc_index(),
            0x3D5 => self.read_crtc_data(),
            _ => return Err(Error::Device(format!("Invalid VGA port: {:#x}", offset))),
        };

        data[0] = value;
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(Error::Device("VGA only supports single-byte writes".into()));
        }

        match offset {
            0x3C0 => self.write_ac(data[0]),
            0x3C2 => self.write_misc_output(data[0]),
            0x3C4 => self.write_seq_index(data[0]),
            0x3C5 => self.write_seq_data(data[0]),
            0x3C6 => self.write_dac_mask(data[0]),
            0x3C7 => self.write_dac_read_index(data[0]),
            0x3C8 => self.write_dac_write_index(data[0]),
            0x3C9 => self.write_dac_data(data[0]),
            0x3CE => self.write_gc_index(data[0]),
            0x3CF => self.write_gc_data(data[0]),
            0x3D4 => self.write_crtc_index(data[0]),
            0x3D5 => self.write_crtc_data(data[0]),
            _ => return Err(Error::Device(format!("Invalid VGA port: {:#x}", offset))),
        }

        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut state = self.state.lock();
        *state = VgaState::new();
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
    async fn test_vga_creation() {
        let vga = VgaDevice::new();
        assert_eq!(vga.name(), "VGA Text Mode");
        assert_eq!(vga.device_type(), DeviceType::Display);
    }

    #[tokio::test]
    async fn test_vga_buffer_read_write() {
        let vga = VgaDevice::new();

        // Write character and attribute
        vga.write_buffer(0, b'A').unwrap();
        vga.write_buffer(1, 0x0F).unwrap(); // White on black

        // Read back
        assert_eq!(vga.read_buffer(0).unwrap(), b'A');
        assert_eq!(vga.read_buffer(1).unwrap(), 0x0F);
    }

    #[tokio::test]
    async fn test_vga_cursor() {
        let vga = VgaDevice::new();

        // Set cursor position
        vga.set_cursor(5, 10);
        let (row, col) = vga.get_cursor();
        assert_eq!(row, 5);
        assert_eq!(col, 10);

        // Check CRTC registers
        vga.write_crtc_index(CRTC_CURSOR_LOC_HIGH);
        let high = vga.read_crtc_data();
        vga.write_crtc_index(CRTC_CURSOR_LOC_LOW);
        let low = vga.read_crtc_data();

        let cursor_pos = ((high as u16) << 8) | (low as u16);
        assert_eq!(cursor_pos, 5 * 80 + 10);
    }

    #[tokio::test]
    async fn test_vga_clear() {
        let vga = VgaDevice::new();

        // Write some data
        vga.write_buffer(0, b'X').unwrap();
        vga.write_buffer(100, b'Y').unwrap();

        // Clear
        vga.clear();

        // Should be blank
        assert_eq!(vga.read_buffer(0).unwrap(), b' ');
        assert_eq!(vga.read_buffer(100).unwrap(), b' ');
    }

    #[tokio::test]
    async fn test_vga_attribute() {
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Blue);
        let byte = attr.to_byte();
        assert_eq!(byte, 0x1F); // Blue bg (1), White fg (F)

        let parsed = VgaAttribute::from_byte(byte);
        assert_eq!(parsed.foreground, VgaColor::White);
        assert_eq!(parsed.background, VgaColor::Blue);
    }

    #[tokio::test]
    async fn test_vga_write_string() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        vga.write_string("Hello", attr);

        // Check first 5 characters
        assert_eq!(vga.read_buffer(0).unwrap(), b'H');
        assert_eq!(vga.read_buffer(2).unwrap(), b'e');
        assert_eq!(vga.read_buffer(4).unwrap(), b'l');
        assert_eq!(vga.read_buffer(6).unwrap(), b'l');
        assert_eq!(vga.read_buffer(8).unwrap(), b'o');
    }

    #[tokio::test]
    async fn test_vga_bounds_checking() {
        let vga = VgaDevice::new();

        // Out of range read
        assert!(vga.read_buffer(VGA_SIZE as u64).is_err());

        // Out of range write
        assert!(vga.write_buffer(VGA_SIZE as u64, 0).is_err());
    }

    #[tokio::test]
    async fn test_vga_device_trait() {
        let mut vga = VgaDevice::new();

        vga.init().await.unwrap();

        let mut buf = [0u8; 1];
        vga.read(0x3D4, &mut buf).await.unwrap();

        vga.write(0x3D4, &[CRTC_CURSOR_LOC_HIGH]).await.unwrap();
        vga.write(0x3D5, &[5]).await.unwrap();

        vga.reset().await.unwrap();
        vga.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_vga_newline() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        vga.write_string("Line1\nLine2", attr);

        // First line at offset 0
        assert_eq!(vga.read_buffer(0).unwrap(), b'L');

        // Second line at offset 80*2 (row 1, col 0)
        assert_eq!(vga.read_buffer(160).unwrap(), b'L');
    }

    #[test]
    fn test_vga_dimensions() {
        assert_eq!(VGA_WIDTH, 80);
        assert_eq!(VGA_HEIGHT, 25);
        assert_eq!(VGA_SIZE, 4000); // 80*25*2
    }

    #[tokio::test]
    async fn test_vga_scroll_up() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        // Fill first 3 lines with different characters
        vga.set_cursor(0, 0);
        vga.write_string("Line 0", attr);
        vga.set_cursor(1, 0);
        vga.write_string("Line 1", attr);
        vga.set_cursor(2, 0);
        vga.write_string("Line 2", attr);

        // Scroll up
        vga.scroll_up();

        // Line 0 should now contain "Line 1"
        assert_eq!(vga.read_buffer(0).unwrap(), b'L');
        assert_eq!(vga.read_buffer(2).unwrap(), b'i');
        assert_eq!(vga.read_buffer(4).unwrap(), b'n');
        assert_eq!(vga.read_buffer(6).unwrap(), b'e');
        assert_eq!(vga.read_buffer(8).unwrap(), b' ');
        assert_eq!(vga.read_buffer(10).unwrap(), b'1');

        // Line 1 should now contain "Line 2"
        let line1_offset = 80 * 2;
        assert_eq!(vga.read_buffer(line1_offset).unwrap(), b'L');
        assert_eq!(vga.read_buffer(line1_offset + 10).unwrap(), b'2');

        // Last line should be blank
        let last_line_offset = ((VGA_HEIGHT - 1) * 80 * 2) as u64;
        assert_eq!(vga.read_buffer(last_line_offset).unwrap(), b' ');
    }

    #[tokio::test]
    async fn test_vga_put_char_newline() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        vga.put_char(b'A', attr);
        vga.put_char(b'\n', attr);
        vga.put_char(b'B', attr);

        // A at position 0
        let (ch, _) = vga.get_char(0, 0).unwrap();
        assert_eq!(ch, b'A');

        // B at position (1, 0) after newline
        let (ch, _) = vga.get_char(1, 0).unwrap();
        assert_eq!(ch, b'B');
    }

    #[tokio::test]
    async fn test_vga_put_char_scroll_simple() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::Yellow, VgaColor::Blue);

        // Manually test scrolling
        vga.set_cursor(0, 0);
        vga.put_char(b'A', attr);
        vga.set_cursor(1, 0);
        vga.put_char(b'B', attr);

        // Scroll up
        vga.scroll_up();

        // First line should now be 'B'
        let (ch, _) = vga.get_char(0, 0).unwrap();
        assert_eq!(ch, b'B');

        // Second line should be blank
        let (ch, _) = vga.get_char(1, 0).unwrap();
        assert_eq!(ch, b' ');
    }

    #[tokio::test]
    async fn test_vga_put_char_backspace() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        vga.put_char(b'A', attr);
        vga.put_char(b'B', attr);
        vga.put_char(b'\x08', attr); // Backspace

        let (row, col) = vga.get_cursor();
        assert_eq!(row, 0);
        assert_eq!(col, 1); // Back to position after 'A'
    }

    #[tokio::test]
    async fn test_vga_put_char_tab() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        vga.set_cursor(0, 5);
        vga.put_char(b'\t', attr);

        let (row, col) = vga.get_cursor();
        assert_eq!(row, 0);
        assert_eq!(col, 8); // Tab to next 8-column boundary
    }

    #[tokio::test]
    async fn test_vga_put_char_carriage_return() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        vga.put_char(b'A', attr);
        vga.put_char(b'B', attr);
        vga.put_char(b'\r', attr); // CR

        let (row, col) = vga.get_cursor();
        assert_eq!(row, 0);
        assert_eq!(col, 0); // Back to start of line
    }

    #[tokio::test]
    async fn test_vga_put_string() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::Green, VgaColor::Black);

        vga.put_string("Hello\nWorld!", attr);

        // "Hello" on line 0
        let (ch, _) = vga.get_char(0, 0).unwrap();
        assert_eq!(ch, b'H');

        // "World!" on line 1
        let (ch, _) = vga.get_char(1, 0).unwrap();
        assert_eq!(ch, b'W');

        let (row, col) = vga.get_cursor();
        assert_eq!(row, 1);
        assert_eq!(col, 6); // After "World!"
    }

    #[tokio::test]
    async fn test_vga_fill_region() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::Red, VgaColor::Yellow);

        // Fill a 3x3 region with 'X'
        vga.fill_region(5, 10, 7, 12, b'X', attr);

        // Check corners
        let (ch, cell_attr) = vga.get_char(5, 10).unwrap();
        assert_eq!(ch, b'X');
        assert_eq!(cell_attr.foreground, VgaColor::Red);

        let (ch, _) = vga.get_char(7, 12).unwrap();
        assert_eq!(ch, b'X');

        // Check outside region is blank
        let (ch, _) = vga.get_char(5, 9).unwrap();
        assert_eq!(ch, b' ');

        let (ch, _) = vga.get_char(8, 10).unwrap();
        assert_eq!(ch, b' ');
    }

    #[tokio::test]
    async fn test_vga_get_char() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::Cyan, VgaColor::Magenta);

        vga.put_char(b'Q', attr);

        let result = vga.get_char(0, 0);
        assert!(result.is_some());
        let (ch, cell_attr) = result.unwrap();
        assert_eq!(ch, b'Q');
        assert_eq!(cell_attr.foreground, VgaColor::Cyan);
        assert_eq!(cell_attr.background, VgaColor::Magenta);

        // Out of bounds
        assert!(vga.get_char(25, 0).is_none());
        assert!(vga.get_char(0, 80).is_none());
    }

    #[tokio::test]
    async fn test_vga_line_wrap() {
        let vga = VgaDevice::new();
        let attr = VgaAttribute::new(VgaColor::White, VgaColor::Black);

        // Position at end of line
        vga.set_cursor(0, 79);
        vga.put_char(b'A', attr);
        vga.put_char(b'B', attr); // Should wrap to next line

        let (row, col) = vga.get_cursor();
        assert_eq!(row, 1);
        assert_eq!(col, 1);

        let (ch, _) = vga.get_char(0, 79).unwrap();
        assert_eq!(ch, b'A');

        let (ch, _) = vga.get_char(1, 0).unwrap();
        assert_eq!(ch, b'B');
    }

    #[tokio::test]
    async fn test_vga_sequencer_registers() {
        let vga = VgaDevice::new();

        // Write sequencer index and data via Device trait
        vga.write_seq_index(0x02); // Map Mask register
        assert_eq!(vga.read_seq_index(), 0x02);

        vga.write_seq_data(0x0F); // Enable all planes
        assert_eq!(vga.read_seq_data(), 0x0F);

        // Also test via Device trait I/O
        let mut buf = [0u8; 1];
        vga.read(0x3C4, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0x02);
    }

    #[tokio::test]
    async fn test_vga_graphics_controller() {
        let vga = VgaDevice::new();

        vga.write_gc_index(0x05); // Mode register
        assert_eq!(vga.read_gc_index(), 0x05);

        vga.write_gc_data(0x10);
        assert_eq!(vga.read_gc_data(), 0x10);
    }

    #[tokio::test]
    async fn test_vga_attribute_controller() {
        let vga = VgaDevice::new();

        // Reset flip-flop by reading Input Status (not emulated, but we can
        // exercise the AC directly)
        // First write sets index
        vga.write_ac(0x10); // Mode Control register index
                            // Second write sets data
        vga.write_ac(0x01);

        assert_eq!(vga.read_ac(), 0x01);
    }

    #[tokio::test]
    async fn test_vga_misc_output() {
        let vga = VgaDevice::new();

        // Default should be 0x67
        assert_eq!(vga.read_misc_output(), 0x67);

        vga.write_misc_output(0x23);
        assert_eq!(vga.read_misc_output(), 0x23);

        // Via Device trait
        let mut buf = [0u8; 1];
        vga.read(0x3CC, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0x23);
    }

    #[tokio::test]
    async fn test_vga_dac_palette() {
        let mut vga = VgaDevice::new();

        // Set write index to color 5
        vga.write_dac_write_index(5);
        // Write R, G, B
        vga.write_dac_data(0x3F);
        vga.write_dac_data(0x00);
        vga.write_dac_data(0x15);

        // Read back: set read index to 5
        vga.write_dac_read_index(5);
        assert_eq!(vga.read_dac_data(), 0x3F); // R
        assert_eq!(vga.read_dac_data(), 0x00); // G
        assert_eq!(vga.read_dac_data(), 0x15); // B

        // Verify via Device trait
        vga.write(0x3C8, &[10]).await.unwrap();
        vga.write(0x3C9, &[0x01]).await.unwrap();
        vga.write(0x3C9, &[0x02]).await.unwrap();
        vga.write(0x3C9, &[0x03]).await.unwrap();

        vga.write(0x3C7, &[10]).await.unwrap();
        let mut buf = [0u8; 1];
        vga.read(0x3C9, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0x01);
    }
}
