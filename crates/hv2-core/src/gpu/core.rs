//! GPU Core Types and Abstractions
//!
//! This module provides core GPU types including pixel formats,
//! display modes, and screen management.

use std::sync::atomic::{AtomicU64, Ordering};

/// Pixel format (color encoding)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit ARGB (8 bits per channel)
    Argb32,
    /// 32-bit XRGB (8 bits per channel, alpha ignored)
    Xrgb32,
    /// 32-bit RGBA (8 bits per channel)
    Rgba32,
    /// 32-bit BGRA (8 bits per channel)
    Bgra32,
    /// 24-bit RGB (8 bits per channel)
    Rgb24,
    /// 24-bit BGR (8 bits per channel)
    Bgr24,
    /// 16-bit RGB (5-6-5)
    Rgb565,
    /// 16-bit BGR (5-6-5)
    Bgr565,
    /// 15-bit RGB (5-5-5 with 1 bit padding)
    Rgb555,
    /// 8-bit indexed (palette)
    Indexed8,
    /// 8-bit grayscale
    Gray8,
    /// 1-bit monochrome
    Mono1,
}

impl PixelFormat {
    /// Bits per pixel
    pub fn bits_per_pixel(&self) -> u32 {
        match self {
            PixelFormat::Argb32 | PixelFormat::Xrgb32 => 32,
            PixelFormat::Rgba32 | PixelFormat::Bgra32 => 32,
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => 24,
            PixelFormat::Rgb565 | PixelFormat::Bgr565 => 16,
            PixelFormat::Rgb555 => 16,
            PixelFormat::Indexed8 | PixelFormat::Gray8 => 8,
            PixelFormat::Mono1 => 1,
        }
    }

    /// Bytes per pixel (rounded up)
    pub fn bytes_per_pixel(&self) -> u32 {
        self.bits_per_pixel().div_ceil(8)
    }

    /// Has alpha channel
    pub fn has_alpha(&self) -> bool {
        matches!(
            self,
            PixelFormat::Argb32 | PixelFormat::Rgba32 | PixelFormat::Bgra32
        )
    }

    /// Is indexed format
    pub fn is_indexed(&self) -> bool {
        matches!(self, PixelFormat::Indexed8)
    }
}

/// RGBA color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create new color
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create opaque color
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Black
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// White
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Red
    pub const RED: Color = Color::rgb(255, 0, 0);
    /// Green
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    /// Blue
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    /// Transparent
    pub const TRANSPARENT: Color = Color::new(0, 0, 0, 0);

    /// Convert to ARGB32
    pub fn to_argb32(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Convert to XRGB32
    pub fn to_xrgb32(&self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Convert to RGBA32
    pub fn to_rgba32(&self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | (self.a as u32)
    }

    /// Convert to BGRA32
    pub fn to_bgra32(&self) -> u32 {
        ((self.b as u32) << 24) | ((self.g as u32) << 16) | ((self.r as u32) << 8) | (self.a as u32)
    }

    /// Convert to RGB565
    pub fn to_rgb565(&self) -> u16 {
        let r = (self.r as u16 >> 3) & 0x1F;
        let g = (self.g as u16 >> 2) & 0x3F;
        let b = (self.b as u16 >> 3) & 0x1F;
        (r << 11) | (g << 5) | b
    }

    /// Create from ARGB32
    pub fn from_argb32(value: u32) -> Self {
        Self {
            a: ((value >> 24) & 0xFF) as u8,
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        }
    }

    /// Create from XRGB32 (alpha is always 255, X byte ignored)
    pub fn from_xrgb32(value: u32) -> Self {
        Self {
            a: 255,
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        }
    }

    /// Create from RGB565
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

    /// Blend with another color (alpha compositing)
    pub fn blend(&self, other: &Color) -> Color {
        if other.a == 255 {
            return *other;
        }
        if other.a == 0 {
            return *self;
        }

        let sa = other.a as u32;
        let da = 255 - sa;

        Color {
            r: ((other.r as u32 * sa + self.r as u32 * da) / 255) as u8,
            g: ((other.g as u32 * sa + self.g as u32 * da) / 255) as u8,
            b: ((other.b as u32 * sa + self.b as u32 * da) / 255) as u8,
            a: (other.a as u32 + self.a as u32 * da / 255) as u8,
        }
    }
}

/// Display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pixel format
    pub format: PixelFormat,
    /// Refresh rate in Hz
    pub refresh_rate: u32,
}

impl DisplayMode {
    /// Create new display mode
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            refresh_rate: 60,
        }
    }

    /// Create with refresh rate
    pub fn with_refresh(width: u32, height: u32, format: PixelFormat, refresh_rate: u32) -> Self {
        Self {
            width,
            height,
            format,
            refresh_rate,
        }
    }

    /// Stride (bytes per row)
    pub fn stride(&self) -> u32 {
        self.width * self.format.bytes_per_pixel()
    }

    /// Total frame buffer size in bytes
    pub fn framebuffer_size(&self) -> usize {
        (self.stride() * self.height) as usize
    }

    /// Common VGA mode (640x480)
    pub const VGA: DisplayMode = DisplayMode {
        width: 640,
        height: 480,
        format: PixelFormat::Xrgb32,
        refresh_rate: 60,
    };

    /// SVGA mode (800x600)
    pub const SVGA: DisplayMode = DisplayMode {
        width: 800,
        height: 600,
        format: PixelFormat::Xrgb32,
        refresh_rate: 60,
    };

    /// XGA mode (1024x768)
    pub const XGA: DisplayMode = DisplayMode {
        width: 1024,
        height: 768,
        format: PixelFormat::Xrgb32,
        refresh_rate: 60,
    };

    /// HD mode (1280x720)
    pub const HD: DisplayMode = DisplayMode {
        width: 1280,
        height: 720,
        format: PixelFormat::Xrgb32,
        refresh_rate: 60,
    };

    /// Full HD mode (1920x1080)
    pub const FULL_HD: DisplayMode = DisplayMode {
        width: 1920,
        height: 1080,
        format: PixelFormat::Xrgb32,
        refresh_rate: 60,
    };

    /// 4K UHD mode (3840x2160)
    pub const UHD_4K: DisplayMode = DisplayMode {
        width: 3840,
        height: 2160,
        format: PixelFormat::Xrgb32,
        refresh_rate: 60,
    };
}

/// Rectangle region
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Create new rectangle
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Create rectangle from points
    pub fn from_points(x1: u32, y1: u32, x2: u32, y2: u32) -> Self {
        Self {
            x: x1.min(x2),
            y: y1.min(y2),
            width: x1.abs_diff(x2),
            height: y1.abs_diff(y2),
        }
    }

    /// Right edge
    pub fn right(&self) -> u32 {
        self.x + self.width
    }

    /// Bottom edge
    pub fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// Area in pixels
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Check if point is inside
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Check if rectangles intersect
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Compute intersection
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        if !self.intersects(other) {
            return None;
        }

        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        Some(Rect::new(x, y, right - x, bottom - y))
    }

    /// Compute union (bounding box)
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());

        Rect::new(x, y, right - x, bottom - y)
    }

    /// Is empty (zero area)
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Cursor shape
#[derive(Debug, Clone)]
pub struct CursorShape {
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Hot spot X
    pub hot_x: u32,
    /// Hot spot Y
    pub hot_y: u32,
    /// Pixel data (ARGB32)
    pub data: Vec<u32>,
}

impl CursorShape {
    /// Create new cursor
    pub fn new(width: u32, height: u32, hot_x: u32, hot_y: u32) -> Self {
        let size = (width * height) as usize;
        Self {
            width,
            height,
            hot_x,
            hot_y,
            data: vec![0; size],
        }
    }

    /// Create default arrow cursor
    pub fn default_arrow() -> Self {
        const W: u32 = 16;
        const H: u32 = 16;
        let mut cursor = Self::new(W, H, 0, 0);

        // Simple arrow shape
        let black = Color::BLACK.to_argb32();
        let white = Color::WHITE.to_argb32();

        for y in 0..H {
            for x in 0..W.min(y + 1) {
                let idx = (y * W + x) as usize;
                if x == 0 || x == y || y == H - 1 {
                    cursor.data[idx] = black;
                } else if x < y {
                    cursor.data[idx] = white;
                }
            }
        }

        cursor
    }
}

/// Cursor state
#[derive(Debug, Clone, Default)]
pub struct CursorState {
    /// X position
    pub x: u32,
    /// Y position
    pub y: u32,
    /// Visible
    pub visible: bool,
    /// Current shape
    pub shape: Option<CursorShape>,
}

impl CursorState {
    /// Create new cursor state
    pub fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            visible: true,
            shape: None,
        }
    }

    /// Move cursor
    pub fn move_to(&mut self, x: u32, y: u32) {
        self.x = x;
        self.y = y;
    }

    /// Move cursor relative
    pub fn move_by(&mut self, dx: i32, dy: i32) {
        self.x = (self.x as i32 + dx).max(0) as u32;
        self.y = (self.y as i32 + dy).max(0) as u32;
    }

    /// Set visibility
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Set shape
    pub fn set_shape(&mut self, shape: CursorShape) {
        self.shape = Some(shape);
    }
}

/// Display surface (abstract framebuffer)
pub trait DisplaySurface: Send + Sync {
    /// Get display mode
    fn mode(&self) -> DisplayMode;

    /// Get width
    fn width(&self) -> u32 {
        self.mode().width
    }

    /// Get height
    fn height(&self) -> u32 {
        self.mode().height
    }

    /// Get pixel format
    fn format(&self) -> PixelFormat {
        self.mode().format
    }

    /// Set pixel at position
    fn set_pixel(&mut self, x: u32, y: u32, color: Color);

    /// Get pixel at position
    fn get_pixel(&self, x: u32, y: u32) -> Color;

    /// Fill rectangle with color
    fn fill_rect(&mut self, rect: &Rect, color: Color);

    /// Copy rectangle from source
    fn blit(&mut self, src: &dyn DisplaySurface, src_rect: &Rect, dst_x: u32, dst_y: u32);

    /// Invalidate region (mark for redraw)
    fn invalidate(&mut self, rect: &Rect);

    /// Get raw framebuffer data
    fn data(&self) -> &[u8];

    /// Get mutable framebuffer data
    fn data_mut(&mut self) -> &mut [u8];
}

/// Scanout configuration
#[derive(Debug, Clone)]
pub struct Scanout {
    /// Scanout ID
    pub id: u32,
    /// Display mode
    pub mode: DisplayMode,
    /// Enabled
    pub enabled: bool,
    /// X position (for multi-monitor)
    pub x: u32,
    /// Y position (for multi-monitor)
    pub y: u32,
}

impl Scanout {
    /// Create new scanout
    pub fn new(id: u32, mode: DisplayMode) -> Self {
        Self {
            id,
            mode,
            enabled: true,
            x: 0,
            y: 0,
        }
    }

    /// Rectangle covered by this scanout
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.mode.width, self.mode.height)
    }
}

/// GPU statistics
#[derive(Debug, Default)]
pub struct GpuStats {
    /// Frames rendered
    pub frames: AtomicU64,
    /// Pixels drawn
    pub pixels_drawn: AtomicU64,
    /// Blits performed
    pub blits: AtomicU64,
    /// Fill operations
    pub fills: AtomicU64,
    /// Cursor updates
    pub cursor_updates: AtomicU64,
}

impl GpuStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record frame
    pub fn record_frame(&self) {
        self.frames.fetch_add(1, Ordering::Relaxed);
    }

    /// Record pixels drawn
    pub fn record_pixels(&self, count: u64) {
        self.pixels_drawn.fetch_add(count, Ordering::Relaxed);
    }

    /// Record blit
    pub fn record_blit(&self) {
        self.blits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record fill
    pub fn record_fill(&self) {
        self.fills.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cursor update
    pub fn record_cursor_update(&self) {
        self.cursor_updates.fetch_add(1, Ordering::Relaxed);
    }

    /// Get frames count
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Get pixels drawn
    pub fn pixels_drawn(&self) -> u64 {
        self.pixels_drawn.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_bpp() {
        assert_eq!(PixelFormat::Argb32.bits_per_pixel(), 32);
        assert_eq!(PixelFormat::Rgb24.bits_per_pixel(), 24);
        assert_eq!(PixelFormat::Rgb565.bits_per_pixel(), 16);
        assert_eq!(PixelFormat::Indexed8.bits_per_pixel(), 8);
        assert_eq!(PixelFormat::Mono1.bits_per_pixel(), 1);
    }

    #[test]
    fn test_pixel_format_bytes() {
        assert_eq!(PixelFormat::Argb32.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Rgb24.bytes_per_pixel(), 3);
        assert_eq!(PixelFormat::Rgb565.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Indexed8.bytes_per_pixel(), 1);
        assert_eq!(PixelFormat::Mono1.bytes_per_pixel(), 1);
    }

    #[test]
    fn test_pixel_format_alpha() {
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
    fn test_color_to_argb32() {
        let color = Color::new(255, 128, 64, 200);
        let argb = color.to_argb32();
        assert_eq!(argb, 0xC8FF8040);
    }

    #[test]
    fn test_color_from_argb32() {
        let color = Color::from_argb32(0xC8FF8040);
        assert_eq!(color.a, 200);
        assert_eq!(color.r, 255);
        assert_eq!(color.g, 128);
        assert_eq!(color.b, 64);
    }

    #[test]
    fn test_color_rgb565() {
        let color = Color::rgb(255, 0, 0);
        let rgb565 = color.to_rgb565();
        assert_eq!(rgb565 >> 11, 0x1F); // Red at max

        let restored = Color::from_rgb565(rgb565);
        assert!(restored.r > 248); // Close to 255
        assert_eq!(restored.g, 0);
        assert_eq!(restored.b, 0);
    }

    #[test]
    fn test_color_blend() {
        let bg = Color::WHITE;
        let fg = Color::new(255, 0, 0, 128);
        let blended = bg.blend(&fg);
        // Should be pinkish (mix of white and red)
        assert!(blended.r > 127);
        assert!(blended.g > 0 && blended.g < 255);
    }

    #[test]
    fn test_display_mode() {
        let mode = DisplayMode::new(1920, 1080, PixelFormat::Argb32);
        assert_eq!(mode.width, 1920);
        assert_eq!(mode.height, 1080);
        assert_eq!(mode.stride(), 1920 * 4);
        assert_eq!(mode.framebuffer_size(), 1920 * 1080 * 4);
    }

    #[test]
    fn test_display_mode_constants() {
        assert_eq!(DisplayMode::VGA.width, 640);
        assert_eq!(DisplayMode::VGA.height, 480);
        assert_eq!(DisplayMode::FULL_HD.width, 1920);
        assert_eq!(DisplayMode::FULL_HD.height, 1080);
        assert_eq!(DisplayMode::UHD_4K.width, 3840);
    }

    #[test]
    fn test_rect_creation() {
        let rect = Rect::new(10, 20, 100, 50);
        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 20);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 50);
        assert_eq!(rect.right(), 110);
        assert_eq!(rect.bottom(), 70);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 10, 20, 20);
        assert!(rect.contains(15, 15));
        assert!(rect.contains(10, 10));
        assert!(!rect.contains(30, 30));
        assert!(!rect.contains(5, 15));
    }

    #[test]
    fn test_rect_intersects() {
        let r1 = Rect::new(0, 0, 20, 20);
        let r2 = Rect::new(10, 10, 20, 20);
        let r3 = Rect::new(30, 30, 20, 20);

        assert!(r1.intersects(&r2));
        assert!(!r1.intersects(&r3));
    }

    #[test]
    fn test_rect_intersection() {
        let r1 = Rect::new(0, 0, 20, 20);
        let r2 = Rect::new(10, 10, 20, 20);

        let inter = r1.intersection(&r2).unwrap();
        assert_eq!(inter.x, 10);
        assert_eq!(inter.y, 10);
        assert_eq!(inter.width, 10);
        assert_eq!(inter.height, 10);
    }

    #[test]
    fn test_rect_union() {
        let r1 = Rect::new(0, 0, 10, 10);
        let r2 = Rect::new(20, 20, 10, 10);

        let union = r1.union(&r2);
        assert_eq!(union.x, 0);
        assert_eq!(union.y, 0);
        assert_eq!(union.width, 30);
        assert_eq!(union.height, 30);
    }

    #[test]
    fn test_rect_area() {
        let rect = Rect::new(0, 0, 100, 50);
        assert_eq!(rect.area(), 5000);
    }

    #[test]
    fn test_cursor_shape() {
        let cursor = CursorShape::new(16, 16, 0, 0);
        assert_eq!(cursor.width, 16);
        assert_eq!(cursor.height, 16);
        assert_eq!(cursor.data.len(), 256);
    }

    #[test]
    fn test_cursor_default_arrow() {
        let cursor = CursorShape::default_arrow();
        assert_eq!(cursor.width, 16);
        assert_eq!(cursor.height, 16);
        assert_eq!(cursor.hot_x, 0);
        assert_eq!(cursor.hot_y, 0);
    }

    #[test]
    fn test_cursor_state() {
        let mut cursor = CursorState::new();
        assert!(cursor.visible);

        cursor.move_to(100, 200);
        assert_eq!(cursor.x, 100);
        assert_eq!(cursor.y, 200);

        cursor.move_by(-50, 10);
        assert_eq!(cursor.x, 50);
        assert_eq!(cursor.y, 210);
    }

    #[test]
    fn test_scanout() {
        let scanout = Scanout::new(0, DisplayMode::FULL_HD);
        assert_eq!(scanout.id, 0);
        assert!(scanout.enabled);

        let rect = scanout.rect();
        assert_eq!(rect.width, 1920);
        assert_eq!(rect.height, 1080);
    }

    #[test]
    fn test_gpu_stats() {
        let stats = GpuStats::new();
        stats.record_frame();
        stats.record_frame();
        stats.record_pixels(1000);
        stats.record_blit();

        assert_eq!(stats.frames(), 2);
        assert_eq!(stats.pixels_drawn(), 1000);
    }
}
