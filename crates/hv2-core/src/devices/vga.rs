//! VGA Text Mode Display
//!
//! This module implements a simple VGA text mode (80x25) display buffer.
//! It provides:
//! - 80x25 character text buffer
//! - 16-color text attributes
//! - Cursor position tracking
//! - CRTC register emulation
//!
//! Memory Map:
//! - 0xB8000-0xBFFFF: Text buffer (32KB, 4KB used for 80x25 mode)
//!
//! I/O Ports:
//! - 0x3D4: CRTC Index Register
//! - 0x3D5: CRTC Data Register

use crate::{Device, DeviceType, Error, Result};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

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
}

impl VgaState {
    fn new() -> Self {
        Self {
            buffer: [VgaCell::blank(); VGA_WIDTH * VGA_HEIGHT],
            cursor_pos: 0,
            crtc_index: 0,
            crtc_regs: [0; 256],
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
            if offset % 2 == 0 {
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
            if offset % 2 == 0 {
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
        self.state.lock().unwrap_or_else(|e| e.into_inner()).crtc_index
    }

    /// Write to CRTC index register (0x3D4)
    pub fn write_crtc_index(&self, value: u8) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).crtc_index = value;
    }

    /// Read from CRTC data register (0x3D5)
    pub fn read_crtc_data(&self) -> u8 {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.crtc_regs[state.crtc_index as usize]
    }

    /// Write to CRTC data register (0x3D5)
    pub fn write_crtc_data(&self, value: u8) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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

    /// Read from text buffer (MMIO)
    pub fn read_buffer(&self, offset: u64) -> Result<u8> {
        if offset >= VGA_SIZE as u64 {
            return Err(Error::Device(format!(
                "VGA buffer read out of range: {:#x}",
                offset
            )));
        }
        Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).read_buffer(offset as usize))
    }

    /// Write to text buffer (MMIO)
    pub fn write_buffer(&self, offset: u64, value: u8) -> Result<()> {
        if offset >= VGA_SIZE as u64 {
            return Err(Error::Device(format!(
                "VGA buffer write out of range: {:#x}",
                offset
            )));
        }
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write_buffer(offset as usize, value);
        Ok(())
    }

    /// Clear screen
    pub fn clear(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Get cursor position
    pub fn get_cursor(&self) -> (usize, usize) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).get_cursor()
    }

    /// Set cursor position
    pub fn set_cursor(&self, row: usize, col: usize) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).set_cursor(row, col);
    }

    /// Get text content (for debugging)
    pub fn get_text(&self) -> String {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).get_text()
    }

    /// Write string at current cursor position
    pub fn write_string(&self, s: &str, attr: VgaAttribute) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
        self.state.lock().unwrap_or_else(|e| e.into_inner()).put_char(ch, attr);
    }

    /// Write string with auto-scroll support
    pub fn put_string(&self, s: &str, attr: VgaAttribute) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for ch in s.bytes() {
            state.put_char(ch, attr);
        }
    }

    /// Scroll display up by one line
    pub fn scroll_up(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).scroll_up();
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
            .unwrap_or_else(|e| e.into_inner())
            .fill_region(start_row, start_col, end_row, end_col, ch, attr);
    }

    /// Get character at position
    pub fn get_char(&self, row: usize, col: usize) -> Option<(u8, VgaAttribute)> {
        if row >= VGA_HEIGHT || col >= VGA_WIDTH {
            return None;
        }
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
            0x3D4 => self.write_crtc_index(data[0]),
            0x3D5 => self.write_crtc_data(data[0]),
            _ => return Err(Error::Device(format!("Invalid VGA port: {:#x}", offset))),
        }

        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
}
