//! Framebuffer Implementation
//!
//! This module provides a software framebuffer with pixel operations,
//! blitting, scrolling, and dirty region tracking.

use super::core::{Color, CursorState, DisplayMode, DisplaySurface, GpuStats, PixelFormat, Rect};
use std::sync::atomic::Ordering;

/// Software framebuffer
#[derive(Debug)]
pub struct Framebuffer {
    /// Display mode
    mode: DisplayMode,
    /// Pixel data
    data: Vec<u8>,
    /// Dirty regions
    dirty: Vec<Rect>,
    /// Cursor state
    cursor: CursorState,
    /// Statistics
    stats: GpuStats,
}

impl Framebuffer {
    /// Create new framebuffer
    pub fn new(mode: DisplayMode) -> Self {
        let size = mode.framebuffer_size();
        Self {
            mode,
            data: vec![0; size],
            dirty: Vec::new(),
            cursor: CursorState::new(),
            stats: GpuStats::new(),
        }
    }

    /// Create with specific resolution
    pub fn with_resolution(width: u32, height: u32) -> Self {
        Self::new(DisplayMode::new(width, height, PixelFormat::Xrgb32))
    }

    /// Get stride (bytes per row)
    pub fn stride(&self) -> u32 {
        self.mode.stride()
    }

    /// Get cursor state
    pub fn cursor(&self) -> &CursorState {
        &self.cursor
    }

    /// Get mutable cursor state
    pub fn cursor_mut(&mut self) -> &mut CursorState {
        &mut self.cursor
    }

    /// Get statistics
    pub fn stats(&self) -> &GpuStats {
        &self.stats
    }

    /// Calculate offset for pixel at (x, y)
    fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.mode.width || y >= self.mode.height {
            return None;
        }
        let offset = (y * self.stride() + x * self.mode.format.bytes_per_pixel()) as usize;
        Some(offset)
    }

    /// Clear framebuffer with color
    pub fn clear(&mut self, color: Color) {
        self.fill_rect(&Rect::new(0, 0, self.mode.width, self.mode.height), color);
    }

    /// Draw horizontal line
    pub fn draw_hline(&mut self, x: u32, y: u32, length: u32, color: Color) {
        if y >= self.mode.height {
            return;
        }
        let end_x = (x + length).min(self.mode.width);
        for px in x..end_x {
            self.set_pixel(px, y, color);
        }
    }

    /// Draw vertical line
    pub fn draw_vline(&mut self, x: u32, y: u32, length: u32, color: Color) {
        if x >= self.mode.width {
            return;
        }
        let end_y = (y + length).min(self.mode.height);
        for py in y..end_y {
            self.set_pixel(x, py, color);
        }
    }

    /// Draw rectangle outline
    pub fn draw_rect(&mut self, rect: &Rect, color: Color) {
        // Top and bottom edges
        self.draw_hline(rect.x, rect.y, rect.width, color);
        if rect.height > 1 {
            self.draw_hline(rect.x, rect.y + rect.height - 1, rect.width, color);
        }
        // Left and right edges
        if rect.height > 2 {
            self.draw_vline(rect.x, rect.y + 1, rect.height - 2, color);
            if rect.width > 1 {
                self.draw_vline(rect.x + rect.width - 1, rect.y + 1, rect.height - 2, color);
            }
        }
    }

    /// Copy region within framebuffer
    pub fn copy_rect(&mut self, src_rect: &Rect, dst_x: u32, dst_y: u32) {
        // Handle overlapping regions correctly
        let src_x = src_rect.x;
        let src_y = src_rect.y;
        let width = src_rect.width.min(self.mode.width.saturating_sub(dst_x));
        let height = src_rect.height.min(self.mode.height.saturating_sub(dst_y));

        if width == 0 || height == 0 {
            return;
        }

        // Determine copy direction based on overlap
        let copy_down = dst_y > src_y;
        let copy_right = dst_x > src_x;

        let bpp = self.mode.format.bytes_per_pixel() as usize;
        let stride = self.stride() as usize;
        let row_bytes = width as usize * bpp;

        // Fast path: a full-width vertical move (the console-scroll case) spans
        // a contiguous source and destination, so it is a single memmove
        // instead of one bounds-checked copy per scanline. `copy_within` has
        // memmove semantics and handles up/down overlap correctly.
        if src_x == 0 && dst_x == 0 && row_bytes == stride && src_y + height <= self.mode.height {
            let len = height as usize * stride;
            let src_start = src_y as usize * stride;
            let dst_start = dst_y as usize * stride;
            if src_start + len <= self.data.len() && dst_start + len <= self.data.len() {
                self.data.copy_within(src_start..src_start + len, dst_start);
                self.invalidate(&Rect::new(dst_x, dst_y, width, height));
                self.stats.blits.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        // Create temporary buffer for overlapping copies
        let mut row_buffer = vec![0u8; row_bytes];

        let y_range: Box<dyn Iterator<Item = u32>> = if copy_down {
            Box::new((0..height).rev())
        } else {
            Box::new(0..height)
        };

        for dy in y_range {
            let sy = src_y + dy;
            let ty = dst_y + dy;

            if sy >= self.mode.height || ty >= self.mode.height {
                continue;
            }

            let src_start = sy as usize * stride + src_x as usize * bpp;
            let dst_start = ty as usize * stride + dst_x as usize * bpp;

            // Copy to temp buffer then to destination
            if copy_right && src_start < dst_start && dst_start < src_start + row_bytes {
                // Overlapping, use temp buffer
                row_buffer.copy_from_slice(&self.data[src_start..src_start + row_bytes]);
                self.data[dst_start..dst_start + row_bytes].copy_from_slice(&row_buffer);
            } else {
                // Non-overlapping, copy directly
                self.data
                    .copy_within(src_start..src_start + row_bytes, dst_start);
            }
        }

        self.invalidate(&Rect::new(dst_x, dst_y, width, height));
        self.stats.blits.fetch_add(1, Ordering::Relaxed);
    }

    /// Scroll framebuffer up by n pixels
    pub fn scroll_up(&mut self, pixels: u32, fill_color: Color) {
        if pixels >= self.mode.height {
            self.clear(fill_color);
            return;
        }

        // Copy content up
        let src_rect = Rect::new(0, pixels, self.mode.width, self.mode.height - pixels);
        self.copy_rect(&src_rect, 0, 0);

        // Fill bottom with color
        let fill_rect = Rect::new(0, self.mode.height - pixels, self.mode.width, pixels);
        self.fill_rect(&fill_rect, fill_color);
    }

    /// Scroll framebuffer down by n pixels
    pub fn scroll_down(&mut self, pixels: u32, fill_color: Color) {
        if pixels >= self.mode.height {
            self.clear(fill_color);
            return;
        }

        // Copy content down
        let src_rect = Rect::new(0, 0, self.mode.width, self.mode.height - pixels);
        self.copy_rect(&src_rect, 0, pixels);

        // Fill top with color
        let fill_rect = Rect::new(0, 0, self.mode.width, pixels);
        self.fill_rect(&fill_rect, fill_color);
    }

    /// Get dirty regions
    pub fn dirty_regions(&self) -> &[Rect] {
        &self.dirty
    }

    /// Clear dirty regions
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Resize framebuffer
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == self.mode.width && height == self.mode.height {
            return;
        }

        let new_mode = DisplayMode::new(width, height, self.mode.format);
        let new_size = new_mode.framebuffer_size();

        // Copy existing content
        let old_data = std::mem::take(&mut self.data);
        let mut new_data = vec![0u8; new_size];

        let copy_width = width.min(self.mode.width);
        let copy_height = height.min(self.mode.height);
        let bpp = self.mode.format.bytes_per_pixel() as usize;
        let old_stride = self.stride() as usize;
        let new_stride = new_mode.stride() as usize;

        for y in 0..copy_height {
            let src_start = y as usize * old_stride;
            let dst_start = y as usize * new_stride;
            let row_bytes = copy_width as usize * bpp;

            if src_start + row_bytes <= old_data.len() && dst_start + row_bytes <= new_data.len() {
                new_data[dst_start..dst_start + row_bytes]
                    .copy_from_slice(&old_data[src_start..src_start + row_bytes]);
            }
        }

        self.mode = new_mode;
        self.data = new_data;
        self.dirty.clear();
        self.invalidate(&Rect::new(0, 0, width, height));
    }

    /// Convert to different pixel format
    pub fn convert_format(&self, format: PixelFormat) -> Vec<u8> {
        if format == self.mode.format {
            return self.data.clone();
        }

        let width = self.mode.width;
        let height = self.mode.height;
        let dst_bpp = format.bytes_per_pixel() as usize;
        let dst_stride = width as usize * dst_bpp;
        let mut result = vec![0u8; dst_stride * height as usize];

        for y in 0..height {
            for x in 0..width {
                let color = self.get_pixel(x, y);
                let dst_offset = y as usize * dst_stride + x as usize * dst_bpp;

                match format {
                    PixelFormat::Argb32 => {
                        let value = color.to_argb32();
                        result[dst_offset..dst_offset + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    PixelFormat::Xrgb32 => {
                        let value = color.to_xrgb32();
                        result[dst_offset..dst_offset + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    PixelFormat::Rgb565 => {
                        let value = color.to_rgb565();
                        result[dst_offset..dst_offset + 2].copy_from_slice(&value.to_le_bytes());
                    }
                    PixelFormat::Rgb24 => {
                        result[dst_offset] = color.r;
                        result[dst_offset + 1] = color.g;
                        result[dst_offset + 2] = color.b;
                    }
                    PixelFormat::Bgr24 => {
                        result[dst_offset] = color.b;
                        result[dst_offset + 1] = color.g;
                        result[dst_offset + 2] = color.r;
                    }
                    PixelFormat::Gray8 => {
                        let gray = ((color.r as u32 + color.g as u32 + color.b as u32) / 3) as u8;
                        result[dst_offset] = gray;
                    }
                    _ => {}
                }
            }
        }

        result
    }
}

impl DisplaySurface for Framebuffer {
    fn mode(&self) -> DisplayMode {
        self.mode
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if let Some(offset) = self.pixel_offset(x, y) {
            match self.mode.format {
                PixelFormat::Argb32 => {
                    let value = color.to_argb32();
                    self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                PixelFormat::Xrgb32 => {
                    let value = color.to_xrgb32();
                    self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                PixelFormat::Rgba32 => {
                    let value = color.to_rgba32();
                    self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                PixelFormat::Bgra32 => {
                    let value = color.to_bgra32();
                    self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                PixelFormat::Rgb565 => {
                    let value = color.to_rgb565();
                    self.data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
                }
                PixelFormat::Rgb24 => {
                    self.data[offset] = color.r;
                    self.data[offset + 1] = color.g;
                    self.data[offset + 2] = color.b;
                }
                PixelFormat::Bgr24 => {
                    self.data[offset] = color.b;
                    self.data[offset + 1] = color.g;
                    self.data[offset + 2] = color.r;
                }
                PixelFormat::Indexed8 | PixelFormat::Gray8 => {
                    let gray = ((color.r as u32 + color.g as u32 + color.b as u32) / 3) as u8;
                    self.data[offset] = gray;
                }
                _ => {}
            }
            self.stats.pixels_drawn.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn get_pixel(&self, x: u32, y: u32) -> Color {
        if let Some(offset) = self.pixel_offset(x, y) {
            match self.mode.format {
                PixelFormat::Argb32 => {
                    let value = u32::from_le_bytes(
                        self.data[offset..offset + 4]
                            .try_into()
                            .expect("slice is exactly 4 bytes"),
                    );
                    Color::from_argb32(value)
                }
                PixelFormat::Xrgb32 => {
                    let value = u32::from_le_bytes(
                        self.data[offset..offset + 4]
                            .try_into()
                            .expect("slice is exactly 4 bytes"),
                    );
                    Color::from_xrgb32(value)
                }
                PixelFormat::Rgb565 => {
                    let value = u16::from_le_bytes(
                        self.data[offset..offset + 2]
                            .try_into()
                            .expect("slice is exactly 2 bytes"),
                    );
                    Color::from_rgb565(value)
                }
                PixelFormat::Rgb24 => Color::rgb(
                    self.data[offset],
                    self.data[offset + 1],
                    self.data[offset + 2],
                ),
                PixelFormat::Bgr24 => Color::rgb(
                    self.data[offset + 2],
                    self.data[offset + 1],
                    self.data[offset],
                ),
                PixelFormat::Gray8 => {
                    let gray = self.data[offset];
                    Color::rgb(gray, gray, gray)
                }
                _ => Color::BLACK,
            }
        } else {
            Color::BLACK
        }
    }

    fn fill_rect(&mut self, rect: &Rect, color: Color) {
        let clip_x = rect.x.min(self.mode.width);
        let clip_y = rect.y.min(self.mode.height);
        let clip_right = rect.right().min(self.mode.width);
        let clip_bottom = rect.bottom().min(self.mode.height);

        if clip_x >= clip_right || clip_y >= clip_bottom {
            return;
        }

        let bpp = self.mode.format.bytes_per_pixel() as usize;
        let stride = self.stride() as usize;

        // Prepare pixel bytes
        let pixel_bytes: Vec<u8> = match self.mode.format {
            PixelFormat::Argb32 => color.to_argb32().to_le_bytes().to_vec(),
            PixelFormat::Xrgb32 => color.to_xrgb32().to_le_bytes().to_vec(),
            PixelFormat::Rgb565 => color.to_rgb565().to_le_bytes().to_vec(),
            PixelFormat::Rgb24 => vec![color.r, color.g, color.b],
            PixelFormat::Bgr24 => vec![color.b, color.g, color.r],
            _ => vec![0; bpp],
        };

        for y in clip_y..clip_bottom {
            let row_start = y as usize * stride + clip_x as usize * bpp;
            for x in 0..(clip_right - clip_x) as usize {
                let offset = row_start + x * bpp;
                self.data[offset..offset + bpp].copy_from_slice(&pixel_bytes);
            }
        }

        let pixels = (clip_right - clip_x) as u64 * (clip_bottom - clip_y) as u64;
        self.stats.pixels_drawn.fetch_add(pixels, Ordering::Relaxed);
        self.stats.fills.fetch_add(1, Ordering::Relaxed);
        self.invalidate(&Rect::new(
            clip_x,
            clip_y,
            clip_right - clip_x,
            clip_bottom - clip_y,
        ));
    }

    fn blit(&mut self, src: &dyn DisplaySurface, src_rect: &Rect, dst_x: u32, dst_y: u32) {
        let src_mode = src.mode();

        // Clip to source bounds
        let src_x = src_rect.x.min(src_mode.width);
        let src_y = src_rect.y.min(src_mode.height);
        let src_right = src_rect.right().min(src_mode.width);
        let src_bottom = src_rect.bottom().min(src_mode.height);

        if src_x >= src_right || src_y >= src_bottom {
            return;
        }

        // Clip to destination bounds
        let width = (src_right - src_x).min(self.mode.width.saturating_sub(dst_x));
        let height = (src_bottom - src_y).min(self.mode.height.saturating_sub(dst_y));

        if width == 0 || height == 0 {
            return;
        }

        // Copy pixels (with format conversion if needed)
        for dy in 0..height {
            for dx in 0..width {
                let color = src.get_pixel(src_x + dx, src_y + dy);
                self.set_pixel(dst_x + dx, dst_y + dy, color);
            }
        }

        self.stats.blits.fetch_add(1, Ordering::Relaxed);
        self.invalidate(&Rect::new(dst_x, dst_y, width, height));
    }

    fn invalidate(&mut self, rect: &Rect) {
        // Merge with existing dirty regions if overlapping
        for existing in &mut self.dirty {
            if existing.intersects(rect) {
                *existing = existing.union(rect);
                return;
            }
        }
        self.dirty.push(*rect);
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

/// Double-buffered framebuffer
#[derive(Debug)]
pub struct DoubleBuffer {
    /// Front buffer (displayed)
    front: Framebuffer,
    /// Back buffer (drawn to)
    back: Framebuffer,
    /// Vsync enabled
    vsync: bool,
}

impl DoubleBuffer {
    /// Create new double buffer
    pub fn new(mode: DisplayMode) -> Self {
        Self {
            front: Framebuffer::new(mode),
            back: Framebuffer::new(mode),
            vsync: true,
        }
    }

    /// Get back buffer for drawing
    pub fn back_buffer(&mut self) -> &mut Framebuffer {
        &mut self.back
    }

    /// Get front buffer for display
    pub fn front_buffer(&self) -> &Framebuffer {
        &self.front
    }

    /// Swap buffers
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
        self.back.clear_dirty();
        self.front.stats.record_frame();
    }

    /// Set vsync
    pub fn set_vsync(&mut self, vsync: bool) {
        self.vsync = vsync;
    }

    /// Is vsync enabled
    pub fn vsync(&self) -> bool {
        self.vsync
    }

    /// Get display mode
    pub fn mode(&self) -> DisplayMode {
        self.front.mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framebuffer_creation() {
        let fb = Framebuffer::new(DisplayMode::VGA);
        assert_eq!(fb.width(), 640);
        assert_eq!(fb.height(), 480);
        assert_eq!(fb.stride(), 640 * 4);
    }

    #[test]
    fn test_framebuffer_with_resolution() {
        let fb = Framebuffer::with_resolution(800, 600);
        assert_eq!(fb.width(), 800);
        assert_eq!(fb.height(), 600);
    }

    #[test]
    fn test_set_get_pixel() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        let color = Color::rgb(255, 128, 64);

        fb.set_pixel(50, 50, color);
        let read = fb.get_pixel(50, 50);

        assert_eq!(read.r, 255);
        assert_eq!(read.g, 128);
        assert_eq!(read.b, 64);
    }

    #[test]
    fn test_pixel_out_of_bounds() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.set_pixel(200, 200, Color::RED); // Should not crash
        let color = fb.get_pixel(200, 200);
        assert_eq!(color, Color::BLACK); // Out of bounds returns black
    }

    #[test]
    fn test_clear() {
        let mut fb = Framebuffer::with_resolution(10, 10);
        fb.clear(Color::RED);

        for y in 0..10 {
            for x in 0..10 {
                let pixel = fb.get_pixel(x, y);
                assert_eq!(pixel.r, 255);
            }
        }
    }

    #[test]
    fn test_fill_rect() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.fill_rect(&Rect::new(10, 10, 20, 20), Color::BLUE);

        assert_eq!(fb.get_pixel(15, 15).b, 255);
        assert_eq!(fb.get_pixel(5, 5).b, 0);
    }

    #[test]
    fn test_draw_hline() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.draw_hline(10, 50, 30, Color::GREEN);

        assert_eq!(fb.get_pixel(20, 50).g, 255);
        assert_eq!(fb.get_pixel(5, 50).g, 0);
    }

    #[test]
    fn test_draw_vline() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.draw_vline(50, 10, 30, Color::RED);

        assert_eq!(fb.get_pixel(50, 20).r, 255);
        assert_eq!(fb.get_pixel(50, 5).r, 0);
    }

    #[test]
    fn test_draw_rect() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.draw_rect(&Rect::new(20, 20, 40, 40), Color::WHITE);

        // Check corners
        assert_eq!(fb.get_pixel(20, 20), Color::WHITE);
        assert_eq!(fb.get_pixel(59, 20), Color::WHITE);
        assert_eq!(fb.get_pixel(20, 59), Color::WHITE);
        assert_eq!(fb.get_pixel(59, 59), Color::WHITE);

        // Inside should be uninitialized (transparent/zeroed)
        let inside = fb.get_pixel(40, 40);
        assert_eq!(inside.r, 0);
        assert_eq!(inside.g, 0);
        assert_eq!(inside.b, 0);
    }

    #[test]
    fn test_copy_rect() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.fill_rect(&Rect::new(0, 0, 10, 10), Color::RED);

        fb.copy_rect(&Rect::new(0, 0, 10, 10), 50, 50);

        assert_eq!(fb.get_pixel(55, 55).r, 255);
    }

    #[test]
    fn test_scroll_up() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.fill_rect(&Rect::new(0, 90, 100, 10), Color::RED);

        fb.scroll_up(10, Color::BLACK);

        // Red should now be at y=80
        assert_eq!(fb.get_pixel(50, 80).r, 255);
        // Bottom should be black
        assert_eq!(fb.get_pixel(50, 95).r, 0);
    }

    #[test]
    fn test_scroll_down() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.fill_rect(&Rect::new(0, 0, 100, 10), Color::BLUE);

        fb.scroll_down(10, Color::BLACK);

        // Blue should now be at y=10
        assert_eq!(fb.get_pixel(50, 15).b, 255);
        // Top should be black
        assert_eq!(fb.get_pixel(50, 5).b, 0);
    }

    #[test]
    fn test_dirty_tracking() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        assert!(fb.dirty_regions().is_empty());

        fb.fill_rect(&Rect::new(10, 10, 20, 20), Color::RED);
        assert!(!fb.dirty_regions().is_empty());

        fb.clear_dirty();
        assert!(fb.dirty_regions().is_empty());
    }

    #[test]
    fn test_resize() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.fill_rect(&Rect::new(0, 0, 50, 50), Color::GREEN);

        fb.resize(200, 200);

        assert_eq!(fb.width(), 200);
        assert_eq!(fb.height(), 200);
        // Original content preserved
        assert_eq!(fb.get_pixel(25, 25).g, 255);
    }

    #[test]
    fn test_convert_format() {
        let mut fb = Framebuffer::with_resolution(10, 10);
        fb.fill_rect(&Rect::new(0, 0, 10, 10), Color::rgb(255, 128, 64));

        let rgb565 = fb.convert_format(PixelFormat::Rgb565);
        assert_eq!(rgb565.len(), 10 * 10 * 2);
    }

    #[test]
    fn test_cursor_state() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.cursor_mut().move_to(50, 50);
        assert_eq!(fb.cursor().x, 50);
        assert_eq!(fb.cursor().y, 50);
    }

    #[test]
    fn test_stats() {
        let mut fb = Framebuffer::with_resolution(100, 100);
        fb.set_pixel(0, 0, Color::RED);
        fb.fill_rect(&Rect::new(0, 0, 10, 10), Color::BLUE);

        let stats = fb.stats();
        assert!(stats.pixels_drawn() > 0);
    }

    #[test]
    fn test_double_buffer_creation() {
        let db = DoubleBuffer::new(DisplayMode::VGA);
        assert_eq!(db.mode().width, 640);
        assert_eq!(db.mode().height, 480);
    }

    #[test]
    fn test_double_buffer_swap() {
        let mut db = DoubleBuffer::new(DisplayMode::new(100, 100, PixelFormat::Xrgb32));

        // Draw to back buffer
        db.back_buffer()
            .fill_rect(&Rect::new(0, 0, 50, 50), Color::RED);

        // Front should still be uninitialized (zeroed)
        let front_pixel = db.front_buffer().get_pixel(25, 25);
        assert_eq!(front_pixel.r, 0);
        assert_eq!(front_pixel.g, 0);
        assert_eq!(front_pixel.b, 0);

        // Swap
        db.swap();

        // Now front has the red
        assert_eq!(db.front_buffer().get_pixel(25, 25).r, 255);
    }

    #[test]
    fn test_double_buffer_vsync() {
        let mut db = DoubleBuffer::new(DisplayMode::VGA);
        assert!(db.vsync());

        db.set_vsync(false);
        assert!(!db.vsync());
    }

    #[test]
    fn test_blit_between_surfaces() {
        let mut src = Framebuffer::with_resolution(50, 50);
        src.fill_rect(&Rect::new(0, 0, 50, 50), Color::GREEN);

        let mut dst = Framebuffer::with_resolution(100, 100);
        dst.blit(&src, &Rect::new(0, 0, 50, 50), 25, 25);

        assert_eq!(dst.get_pixel(50, 50).g, 255);
        // Uninitialized area should be zeroed
        let uninit = dst.get_pixel(10, 10);
        assert_eq!(uninit.r, 0);
        assert_eq!(uninit.g, 0);
        assert_eq!(uninit.b, 0);
    }
}
