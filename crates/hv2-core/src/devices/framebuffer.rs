//! Framebuffer device emulation
//!
//! This module provides a simple linear framebuffer device for basic
//! graphics output. It supports configurable resolution, color depth,
//! and pixel formats.

use std::sync::{Arc, RwLock};

/// Pixel format for the framebuffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit ARGB (8 bits per channel, alpha in high byte)
    Argb32,
    /// 32-bit XRGB (8 bits per channel, high byte ignored)
    Xrgb32,
    /// 32-bit RGBA (8 bits per channel, alpha in low byte)
    Rgba32,
    /// 32-bit BGRA (8 bits per channel, blue first)
    Bgra32,
    /// 24-bit RGB (8 bits per channel, no alpha)
    Rgb24,
    /// 24-bit BGR (8 bits per channel, blue first)
    Bgr24,
    /// 16-bit RGB565 (5-6-5 bits)
    Rgb565,
    /// 8-bit indexed color (palette-based)
    Indexed8,
}

impl PixelFormat {
    /// Returns the number of bytes per pixel
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            PixelFormat::Argb32
            | PixelFormat::Xrgb32
            | PixelFormat::Rgba32
            | PixelFormat::Bgra32 => 4,
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => 3,
            PixelFormat::Rgb565 => 2,
            PixelFormat::Indexed8 => 1,
        }
    }

    /// Returns the number of bits per pixel
    pub fn bits_per_pixel(&self) -> u32 {
        self.bytes_per_pixel() * 8
    }

    /// Returns true if the format has an alpha channel
    pub fn has_alpha(&self) -> bool {
        matches!(self, PixelFormat::Argb32 | PixelFormat::Rgba32)
    }
}

/// RGBA color representation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a new color with full opacity
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create a new color with alpha
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Black color
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// White color
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Red color
    pub const RED: Color = Color::rgb(255, 0, 0);
    /// Green color
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    /// Blue color
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    /// Transparent color
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// Convert to u32 in ARGB format
    pub fn to_argb32(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Convert to u32 in RGBA format
    pub fn to_rgba32(&self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | (self.a as u32)
    }

    /// Convert to u32 in BGRA format
    pub fn to_bgra32(&self) -> u32 {
        ((self.b as u32) << 24) | ((self.g as u32) << 16) | ((self.r as u32) << 8) | (self.a as u32)
    }

    /// Convert to u16 in RGB565 format
    pub fn to_rgb565(&self) -> u16 {
        let r = (self.r as u16 >> 3) & 0x1F;
        let g = (self.g as u16 >> 2) & 0x3F;
        let b = (self.b as u16 >> 3) & 0x1F;
        (r << 11) | (g << 5) | b
    }

    /// Create from u32 in ARGB format
    pub fn from_argb32(value: u32) -> Self {
        Self {
            a: ((value >> 24) & 0xFF) as u8,
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        }
    }

    /// Create from u16 in RGB565 format
    pub fn from_rgb565(value: u16) -> Self {
        let r = ((value >> 11) & 0x1F) as u8;
        let g = ((value >> 5) & 0x3F) as u8;
        let b = (value & 0x1F) as u8;
        Self {
            r: (r << 3) | (r >> 2),
            g: (g << 2) | (g >> 4),
            b: (b << 3) | (b >> 2),
            a: 255,
        }
    }
}

/// Framebuffer configuration
#[derive(Debug, Clone)]
pub struct FramebufferConfig {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pixel format
    pub format: PixelFormat,
    /// Stride (bytes per row), may include padding
    pub stride: u32,
}

impl FramebufferConfig {
    /// Create a new configuration with the minimum stride
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let stride = width * format.bytes_per_pixel();
        Self {
            width,
            height,
            format,
            stride,
        }
    }

    /// Create a new configuration with a custom stride
    pub fn with_stride(width: u32, height: u32, format: PixelFormat, stride: u32) -> Self {
        Self {
            width,
            height,
            format,
            stride,
        }
    }

    /// Returns the total size of the framebuffer in bytes
    pub fn size(&self) -> usize {
        (self.stride * self.height) as usize
    }

    /// Returns the offset of a pixel in the buffer
    pub fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((y * self.stride + x * self.format.bytes_per_pixel()) as usize)
    }
}

impl Default for FramebufferConfig {
    fn default() -> Self {
        Self::new(800, 600, PixelFormat::Xrgb32)
    }
}

/// Rectangle for dirty region tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Create a new rectangle
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Check if two rectangles intersect
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// Compute the union of two rectangles
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let x2 = (self.x + self.width).max(other.x + other.width);
        let y2 = (self.y + self.height).max(other.y + other.height);
        Rect::new(x, y, x2 - x, y2 - y)
    }

    /// Check if a point is inside the rectangle
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Linear framebuffer device
pub struct Framebuffer {
    /// Configuration
    config: FramebufferConfig,
    /// Pixel data
    buffer: Vec<u8>,
    /// Dirty region (area that has been modified)
    dirty: Option<Rect>,
    /// 256-entry color palette for indexed modes
    palette: [Color; 256],
}

impl std::fmt::Debug for Framebuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Framebuffer")
            .field("config", &self.config)
            .field("buffer_size", &self.buffer.len())
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl Framebuffer {
    /// Create a new framebuffer with the given configuration
    pub fn new(config: FramebufferConfig) -> Self {
        let size = config.size();
        Self {
            config,
            buffer: vec![0; size],
            dirty: None,
            palette: [Color::BLACK; 256],
        }
    }

    /// Create a framebuffer with default 800x600 XRGB32 configuration
    pub fn new_default() -> Self {
        Self::new(FramebufferConfig::default())
    }

    /// Create a framebuffer with specific dimensions
    pub fn with_dimensions(width: u32, height: u32) -> Self {
        Self::new(FramebufferConfig::new(width, height, PixelFormat::Xrgb32))
    }

    /// Returns the framebuffer configuration
    pub fn config(&self) -> &FramebufferConfig {
        &self.config
    }

    /// Returns the width in pixels
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Returns the height in pixels
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// Returns the pixel format
    pub fn format(&self) -> PixelFormat {
        self.config.format
    }

    /// Returns the stride (bytes per row)
    pub fn stride(&self) -> u32 {
        self.config.stride
    }

    /// Returns the raw buffer data
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Returns the raw buffer data mutably
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    /// Returns the dirty region and clears it
    pub fn take_dirty(&mut self) -> Option<Rect> {
        self.dirty.take()
    }

    /// Returns the current dirty region without clearing
    pub fn dirty(&self) -> Option<Rect> {
        self.dirty
    }

    /// Mark the entire framebuffer as dirty
    pub fn mark_all_dirty(&mut self) {
        self.dirty = Some(Rect::new(0, 0, self.config.width, self.config.height));
    }

    /// Mark a region as dirty
    fn mark_dirty(&mut self, rect: Rect) {
        self.dirty = Some(match self.dirty {
            Some(existing) => existing.union(&rect),
            None => rect,
        });
    }

    /// Set a palette entry (for indexed color modes)
    pub fn set_palette(&mut self, index: u8, color: Color) {
        self.palette[index as usize] = color;
    }

    /// Get a palette entry
    pub fn get_palette(&self, index: u8) -> Color {
        self.palette[index as usize]
    }

    /// Set a pixel at the given coordinates
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if let Some(offset) = self.config.pixel_offset(x, y) {
            match self.config.format {
                PixelFormat::Argb32 => {
                    let value = color.to_argb32().to_le_bytes();
                    self.buffer[offset..offset + 4].copy_from_slice(&value);
                }
                PixelFormat::Xrgb32 => {
                    let value = (color.to_argb32() | 0xFF000000).to_le_bytes();
                    self.buffer[offset..offset + 4].copy_from_slice(&value);
                }
                PixelFormat::Rgba32 => {
                    let value = color.to_rgba32().to_le_bytes();
                    self.buffer[offset..offset + 4].copy_from_slice(&value);
                }
                PixelFormat::Bgra32 => {
                    let value = color.to_bgra32().to_le_bytes();
                    self.buffer[offset..offset + 4].copy_from_slice(&value);
                }
                PixelFormat::Rgb24 => {
                    self.buffer[offset] = color.r;
                    self.buffer[offset + 1] = color.g;
                    self.buffer[offset + 2] = color.b;
                }
                PixelFormat::Bgr24 => {
                    self.buffer[offset] = color.b;
                    self.buffer[offset + 1] = color.g;
                    self.buffer[offset + 2] = color.r;
                }
                PixelFormat::Rgb565 => {
                    let value = color.to_rgb565().to_le_bytes();
                    self.buffer[offset..offset + 2].copy_from_slice(&value);
                }
                PixelFormat::Indexed8 => {
                    // For indexed mode, find the closest palette entry
                    // (simplified: just use the red channel as index)
                    self.buffer[offset] = color.r;
                }
            }
            self.mark_dirty(Rect::new(x, y, 1, 1));
        }
    }

    /// Get a pixel at the given coordinates
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        let offset = self.config.pixel_offset(x, y)?;
        Some(match self.config.format {
            PixelFormat::Argb32 | PixelFormat::Xrgb32 => {
                let value = u32::from_le_bytes([
                    self.buffer[offset],
                    self.buffer[offset + 1],
                    self.buffer[offset + 2],
                    self.buffer[offset + 3],
                ]);
                Color::from_argb32(value)
            }
            PixelFormat::Rgba32 => Color {
                r: self.buffer[offset],
                g: self.buffer[offset + 1],
                b: self.buffer[offset + 2],
                a: self.buffer[offset + 3],
            },
            PixelFormat::Bgra32 => Color {
                b: self.buffer[offset],
                g: self.buffer[offset + 1],
                r: self.buffer[offset + 2],
                a: self.buffer[offset + 3],
            },
            PixelFormat::Rgb24 => Color::rgb(
                self.buffer[offset],
                self.buffer[offset + 1],
                self.buffer[offset + 2],
            ),
            PixelFormat::Bgr24 => Color::rgb(
                self.buffer[offset + 2],
                self.buffer[offset + 1],
                self.buffer[offset],
            ),
            PixelFormat::Rgb565 => {
                let value = u16::from_le_bytes([self.buffer[offset], self.buffer[offset + 1]]);
                Color::from_rgb565(value)
            }
            PixelFormat::Indexed8 => self.palette[self.buffer[offset] as usize],
        })
    }

    /// Clear the framebuffer with a color
    pub fn clear(&mut self, color: Color) {
        // Optimize for common case of black/zero
        if color == Color::BLACK && !self.config.format.has_alpha() {
            self.buffer.fill(0);
        } else {
            for y in 0..self.config.height {
                for x in 0..self.config.width {
                    self.set_pixel(x, y, color);
                }
            }
        }
        self.dirty = None; // Clear is not dirty, it's initialization
        self.mark_all_dirty();
    }

    /// Fill a rectangle with a color
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let x_end = (rect.x + rect.width).min(self.config.width);
        let y_end = (rect.y + rect.height).min(self.config.height);

        for y in rect.y..y_end {
            for x in rect.x..x_end {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// Draw a horizontal line
    pub fn draw_hline(&mut self, x: u32, y: u32, width: u32, color: Color) {
        let x_end = (x + width).min(self.config.width);
        for px in x..x_end {
            self.set_pixel(px, y, color);
        }
    }

    /// Draw a vertical line
    pub fn draw_vline(&mut self, x: u32, y: u32, height: u32, color: Color) {
        let y_end = (y + height).min(self.config.height);
        for py in y..y_end {
            self.set_pixel(x, py, color);
        }
    }

    /// Draw a rectangle outline
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        self.draw_hline(rect.x, rect.y, rect.width, color);
        self.draw_hline(
            rect.x,
            rect.y + rect.height.saturating_sub(1),
            rect.width,
            color,
        );
        self.draw_vline(rect.x, rect.y, rect.height, color);
        self.draw_vline(
            rect.x + rect.width.saturating_sub(1),
            rect.y,
            rect.height,
            color,
        );
    }

    /// Copy a region of the framebuffer to another location
    pub fn blit(&mut self, src: Rect, dst_x: u32, dst_y: u32) {
        // Handle overlapping regions by copying to temp buffer
        let bpp = self.config.format.bytes_per_pixel() as usize;
        let mut temp = vec![0u8; (src.width * src.height) as usize * bpp];

        // Copy source to temp
        for y in 0..src.height {
            for x in 0..src.width {
                if let Some(src_offset) = self.config.pixel_offset(src.x + x, src.y + y) {
                    let temp_offset = ((y * src.width + x) as usize) * bpp;
                    temp[temp_offset..temp_offset + bpp]
                        .copy_from_slice(&self.buffer[src_offset..src_offset + bpp]);
                }
            }
        }

        // Copy temp to destination
        for y in 0..src.height {
            for x in 0..src.width {
                if let Some(dst_offset) = self.config.pixel_offset(dst_x + x, dst_y + y) {
                    let temp_offset = ((y * src.width + x) as usize) * bpp;
                    self.buffer[dst_offset..dst_offset + bpp]
                        .copy_from_slice(&temp[temp_offset..temp_offset + bpp]);
                }
            }
        }

        self.mark_dirty(Rect::new(dst_x, dst_y, src.width, src.height));
    }

    /// Write raw pixel data to the framebuffer at the given offset
    pub fn write_raw(&mut self, offset: usize, data: &[u8]) {
        let end = (offset + data.len()).min(self.buffer.len());
        let len = end.saturating_sub(offset);
        if len > 0 {
            self.buffer[offset..end].copy_from_slice(&data[..len]);
            self.mark_all_dirty(); // Conservative: mark all dirty
        }
    }

    /// Read raw pixel data from the framebuffer
    pub fn read_raw(&self, offset: usize, len: usize) -> &[u8] {
        let end = (offset + len).min(self.buffer.len());
        &self.buffer[offset..end]
    }

    /// Resize the framebuffer (clears contents)
    pub fn resize(&mut self, config: FramebufferConfig) {
        self.config = config;
        self.buffer = vec![0; self.config.size()];
        self.dirty = None;
    }
}

/// Thread-safe framebuffer wrapper
pub type SharedFramebuffer = Arc<RwLock<Framebuffer>>;

/// Create a shared framebuffer
pub fn shared_framebuffer(config: FramebufferConfig) -> SharedFramebuffer {
    Arc::new(RwLock::new(Framebuffer::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_bpp() {
        assert_eq!(PixelFormat::Argb32.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Rgb24.bytes_per_pixel(), 3);
        assert_eq!(PixelFormat::Rgb565.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Indexed8.bytes_per_pixel(), 1);
    }

    #[test]
    fn test_pixel_format_has_alpha() {
        assert!(PixelFormat::Argb32.has_alpha());
        assert!(PixelFormat::Rgba32.has_alpha());
        assert!(!PixelFormat::Xrgb32.has_alpha());
        assert!(!PixelFormat::Rgb565.has_alpha());
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(Color::BLACK, Color::rgb(0, 0, 0));
        assert_eq!(Color::WHITE, Color::rgb(255, 255, 255));
        assert_eq!(Color::RED.r, 255);
        assert_eq!(Color::GREEN.g, 255);
        assert_eq!(Color::BLUE.b, 255);
    }

    #[test]
    fn test_color_conversions() {
        let color = Color::rgba(0x12, 0x34, 0x56, 0x78);
        assert_eq!(color.to_argb32(), 0x78123456);
        assert_eq!(color.to_rgba32(), 0x12345678);

        let from_argb = Color::from_argb32(0xFF112233);
        assert_eq!(from_argb.a, 0xFF);
        assert_eq!(from_argb.r, 0x11);
        assert_eq!(from_argb.g, 0x22);
        assert_eq!(from_argb.b, 0x33);
    }

    #[test]
    fn test_color_rgb565() {
        let red = Color::RED;
        let rgb565 = red.to_rgb565();
        assert_eq!(rgb565, 0xF800); // Red in 565 format

        let back = Color::from_rgb565(rgb565);
        assert!(back.r >= 248); // Some precision loss expected
        assert!(back.g < 8);
        assert!(back.b < 8);
    }

    #[test]
    fn test_framebuffer_config() {
        let config = FramebufferConfig::new(800, 600, PixelFormat::Xrgb32);
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
        assert_eq!(config.stride, 3200); // 800 * 4
        assert_eq!(config.size(), 1920000); // 800 * 600 * 4
    }

    #[test]
    fn test_framebuffer_config_custom_stride() {
        let config = FramebufferConfig::with_stride(800, 600, PixelFormat::Xrgb32, 4096);
        assert_eq!(config.stride, 4096);
        assert_eq!(config.size(), 4096 * 600);
    }

    #[test]
    fn test_pixel_offset() {
        let config = FramebufferConfig::new(100, 100, PixelFormat::Xrgb32);
        assert_eq!(config.pixel_offset(0, 0), Some(0));
        assert_eq!(config.pixel_offset(1, 0), Some(4));
        assert_eq!(config.pixel_offset(0, 1), Some(400));
        assert_eq!(config.pixel_offset(100, 0), None);
        assert_eq!(config.pixel_offset(0, 100), None);
    }

    #[test]
    fn test_rect_intersects() {
        let r1 = Rect::new(0, 0, 10, 10);
        let r2 = Rect::new(5, 5, 10, 10);
        let r3 = Rect::new(20, 20, 10, 10);

        assert!(r1.intersects(&r2));
        assert!(!r1.intersects(&r3));
    }

    #[test]
    fn test_rect_union() {
        let r1 = Rect::new(0, 0, 10, 10);
        let r2 = Rect::new(5, 5, 10, 10);
        let union = r1.union(&r2);

        assert_eq!(union.x, 0);
        assert_eq!(union.y, 0);
        assert_eq!(union.width, 15);
        assert_eq!(union.height, 15);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 10, 20, 20);
        assert!(rect.contains(15, 15));
        assert!(rect.contains(10, 10));
        assert!(!rect.contains(9, 10));
        assert!(!rect.contains(30, 30));
    }

    #[test]
    fn test_framebuffer_creation() {
        let fb = Framebuffer::new_default();
        assert_eq!(fb.width(), 800);
        assert_eq!(fb.height(), 600);
        assert_eq!(fb.format(), PixelFormat::Xrgb32);
    }

    #[test]
    fn test_framebuffer_set_get_pixel() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.set_pixel(10, 20, Color::RED);
        let pixel = fb.get_pixel(10, 20).unwrap();
        assert_eq!(pixel.r, 255);
        assert_eq!(pixel.g, 0);
        assert_eq!(pixel.b, 0);
    }

    #[test]
    fn test_framebuffer_clear() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.clear(Color::BLUE);
        let pixel = fb.get_pixel(50, 50).unwrap();
        assert_eq!(pixel.b, 255);
        assert_eq!(pixel.r, 0);
    }

    #[test]
    fn test_framebuffer_fill_rect() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.fill_rect(Rect::new(10, 10, 20, 20), Color::GREEN);

        // Inside the rect
        let inside = fb.get_pixel(15, 15).unwrap();
        assert_eq!(inside.g, 255);

        // Outside the rect
        let outside = fb.get_pixel(5, 5).unwrap();
        assert_eq!(outside.g, 0);
    }

    #[test]
    fn test_framebuffer_dirty_tracking() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        assert!(fb.dirty().is_none());

        fb.set_pixel(10, 10, Color::RED);
        assert!(fb.dirty().is_some());

        let dirty = fb.take_dirty().unwrap();
        assert!(dirty.contains(10, 10));

        assert!(fb.dirty().is_none());
    }

    #[test]
    fn test_framebuffer_palette() {
        let config = FramebufferConfig::new(100, 100, PixelFormat::Indexed8);
        let mut fb = Framebuffer::new(config);

        fb.set_palette(0, Color::BLACK);
        fb.set_palette(1, Color::WHITE);
        fb.set_palette(2, Color::RED);

        assert_eq!(fb.get_palette(0), Color::BLACK);
        assert_eq!(fb.get_palette(2), Color::RED);
    }

    #[test]
    fn test_framebuffer_draw_lines() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.draw_hline(10, 50, 30, Color::RED);
        assert_eq!(fb.get_pixel(20, 50).unwrap().r, 255);
        assert_eq!(fb.get_pixel(20, 51).unwrap().r, 0);

        fb.draw_vline(50, 10, 30, Color::GREEN);
        assert_eq!(fb.get_pixel(50, 20).unwrap().g, 255);
        assert_eq!(fb.get_pixel(51, 20).unwrap().g, 0);
    }

    #[test]
    fn test_framebuffer_draw_rect() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        fb.draw_rect(Rect::new(10, 10, 20, 20), Color::WHITE);

        // Top edge
        assert_eq!(fb.get_pixel(15, 10).unwrap().r, 255);
        // Bottom edge
        assert_eq!(fb.get_pixel(15, 29).unwrap().r, 255);
        // Inside (should be black)
        assert_eq!(fb.get_pixel(15, 20).unwrap().r, 0);
    }

    #[test]
    fn test_framebuffer_blit() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        // Draw a red square
        fb.fill_rect(Rect::new(0, 0, 10, 10), Color::RED);

        // Copy it to another location
        fb.blit(Rect::new(0, 0, 10, 10), 50, 50);

        // Original location
        assert_eq!(fb.get_pixel(5, 5).unwrap().r, 255);
        // New location
        assert_eq!(fb.get_pixel(55, 55).unwrap().r, 255);
    }

    #[test]
    fn test_framebuffer_raw_access() {
        let mut fb = Framebuffer::with_dimensions(100, 100);

        // Write raw data
        fb.write_raw(0, &[0xFF, 0x00, 0x00, 0xFF]); // BGRA blue pixel

        let raw = fb.read_raw(0, 4);
        assert_eq!(raw, &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn test_framebuffer_resize() {
        let mut fb = Framebuffer::with_dimensions(100, 100);
        fb.set_pixel(50, 50, Color::RED);

        fb.resize(FramebufferConfig::new(200, 200, PixelFormat::Xrgb32));

        assert_eq!(fb.width(), 200);
        assert_eq!(fb.height(), 200);
        // Content should be cleared
        assert_eq!(fb.get_pixel(50, 50).unwrap().r, 0);
    }

    #[test]
    fn test_shared_framebuffer() {
        let fb = shared_framebuffer(FramebufferConfig::default());

        {
            let mut guard = fb.write().unwrap();
            guard.set_pixel(10, 10, Color::RED);
        }

        {
            let guard = fb.read().unwrap();
            assert_eq!(guard.get_pixel(10, 10).unwrap().r, 255);
        }
    }

    #[test]
    fn test_rgb24_format() {
        let config = FramebufferConfig::new(10, 10, PixelFormat::Rgb24);
        let mut fb = Framebuffer::new(config);

        fb.set_pixel(0, 0, Color::rgb(0x11, 0x22, 0x33));
        let pixel = fb.get_pixel(0, 0).unwrap();
        assert_eq!(pixel.r, 0x11);
        assert_eq!(pixel.g, 0x22);
        assert_eq!(pixel.b, 0x33);
    }

    #[test]
    fn test_rgb565_format() {
        let config = FramebufferConfig::new(10, 10, PixelFormat::Rgb565);
        let mut fb = Framebuffer::new(config);

        fb.set_pixel(0, 0, Color::RED);
        let pixel = fb.get_pixel(0, 0).unwrap();
        // RGB565 has precision loss
        assert!(pixel.r >= 248);
        assert!(pixel.g < 8);
        assert!(pixel.b < 8);
    }
}
