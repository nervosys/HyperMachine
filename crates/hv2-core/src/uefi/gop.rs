//! Graphics Output Protocol (GOP)
//!
//! This module provides the UEFI Graphics Output Protocol implementation
//! for framebuffer-based graphics output.

use super::types::{Guid, Handle, Status, guids};
use std::sync::atomic::{AtomicU64, Ordering};

/// GOP pixel format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum GopPixelFormat {
    /// Red-Green-Blue-Reserved 8-bit per color
    #[default]
    RedGreenBlueReserved8BitPerColor = 0,
    /// Blue-Green-Red-Reserved 8-bit per color
    BlueGreenRedReserved8BitPerColor = 1,
    /// Pixel format defined by pixel bitmask
    BitMask = 2,
    /// Only valid for Blt operations
    BltOnly = 3,
    /// Format max sentinel
    FormatMax = 4,
}

impl GopPixelFormat {
    /// Get bytes per pixel
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            GopPixelFormat::RedGreenBlueReserved8BitPerColor => 4,
            GopPixelFormat::BlueGreenRedReserved8BitPerColor => 4,
            GopPixelFormat::BitMask => 4,
            GopPixelFormat::BltOnly => 0,
            GopPixelFormat::FormatMax => 0,
        }
    }

    /// Check if format supports framebuffer access
    pub fn has_framebuffer(&self) -> bool {
        !matches!(self, GopPixelFormat::BltOnly | GopPixelFormat::FormatMax)
    }
}

/// GOP pixel bitmask
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct GopPixelBitmask {
    /// Red mask
    pub red_mask: u32,
    /// Green mask
    pub green_mask: u32,
    /// Blue mask
    pub blue_mask: u32,
    /// Reserved mask
    pub reserved_mask: u32,
}

impl GopPixelBitmask {
    /// Create new bitmask
    pub fn new(red: u32, green: u32, blue: u32, reserved: u32) -> Self {
        Self {
            red_mask: red,
            green_mask: green,
            blue_mask: blue,
            reserved_mask: reserved,
        }
    }

    /// Standard RGBX bitmask
    pub fn rgbx() -> Self {
        Self::new(0x000000FF, 0x0000FF00, 0x00FF0000, 0xFF000000)
    }

    /// Standard BGRX bitmask
    pub fn bgrx() -> Self {
        Self::new(0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)
    }
}

/// GOP mode information
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GopModeInfo {
    /// Version number
    pub version: u32,
    /// Horizontal resolution in pixels
    pub horizontal_resolution: u32,
    /// Vertical resolution in pixels
    pub vertical_resolution: u32,
    /// Pixel format
    pub pixel_format: GopPixelFormat,
    /// Pixel bitmask (only valid for BitMask format)
    pub pixel_information: GopPixelBitmask,
    /// Pixels per scan line
    pub pixels_per_scan_line: u32,
}

impl Default for GopModeInfo {
    fn default() -> Self {
        Self {
            version: 0,
            horizontal_resolution: 800,
            vertical_resolution: 600,
            pixel_format: GopPixelFormat::BlueGreenRedReserved8BitPerColor,
            pixel_information: GopPixelBitmask::bgrx(),
            pixels_per_scan_line: 800,
        }
    }
}

impl GopModeInfo {
    /// Create new mode info
    pub fn new(width: u32, height: u32, format: GopPixelFormat) -> Self {
        Self {
            version: 0,
            horizontal_resolution: width,
            vertical_resolution: height,
            pixel_format: format,
            pixel_information: match format {
                GopPixelFormat::RedGreenBlueReserved8BitPerColor => GopPixelBitmask::rgbx(),
                GopPixelFormat::BlueGreenRedReserved8BitPerColor => GopPixelBitmask::bgrx(),
                _ => GopPixelBitmask::default(),
            },
            pixels_per_scan_line: width,
        }
    }

    /// Create with custom stride
    pub fn with_stride(mut self, stride: u32) -> Self {
        self.pixels_per_scan_line = stride;
        self
    }

    /// Get framebuffer size in bytes
    pub fn framebuffer_size(&self) -> u64 {
        let stride_bytes = self.pixels_per_scan_line as u64 * self.pixel_format.bytes_per_pixel() as u64;
        stride_bytes * self.vertical_resolution as u64
    }

    /// Get total pixels
    pub fn total_pixels(&self) -> u64 {
        self.horizontal_resolution as u64 * self.vertical_resolution as u64
    }
}

/// GOP mode
#[derive(Debug, Clone)]
#[repr(C)]
pub struct GopMode {
    /// Maximum mode number
    pub max_mode: u32,
    /// Current mode number
    pub mode: u32,
    /// Pointer to mode information
    pub info: u64,
    /// Size of mode information
    pub size_of_info: u64,
    /// Framebuffer base address
    pub frame_buffer_base: u64,
    /// Framebuffer size
    pub frame_buffer_size: u64,
}

impl Default for GopMode {
    fn default() -> Self {
        Self {
            max_mode: 1,
            mode: 0,
            info: 0,
            size_of_info: std::mem::size_of::<GopModeInfo>() as u64,
            frame_buffer_base: 0,
            frame_buffer_size: 0,
        }
    }
}

/// BLT pixel
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct GopBltPixel {
    /// Blue component
    pub blue: u8,
    /// Green component
    pub green: u8,
    /// Red component
    pub red: u8,
    /// Reserved (usually alpha)
    pub reserved: u8,
}

impl GopBltPixel {
    /// Create new pixel
    pub fn new(red: u8, green: u8, blue: u8) -> Self {
        Self {
            blue,
            green,
            red,
            reserved: 0,
        }
    }

    /// Create with alpha
    pub fn with_alpha(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            blue,
            green,
            red,
            reserved: alpha,
        }
    }

    /// Black pixel
    pub const BLACK: GopBltPixel = GopBltPixel {
        blue: 0,
        green: 0,
        red: 0,
        reserved: 0,
    };

    /// White pixel
    pub const WHITE: GopBltPixel = GopBltPixel {
        blue: 255,
        green: 255,
        red: 255,
        reserved: 0,
    };

    /// Red pixel
    pub const RED: GopBltPixel = GopBltPixel {
        blue: 0,
        green: 0,
        red: 255,
        reserved: 0,
    };

    /// Green pixel
    pub const GREEN: GopBltPixel = GopBltPixel {
        blue: 0,
        green: 255,
        red: 0,
        reserved: 0,
    };

    /// Blue pixel
    pub const BLUE: GopBltPixel = GopBltPixel {
        blue: 255,
        green: 0,
        red: 0,
        reserved: 0,
    };

    /// Convert to u32 (BGRX format)
    pub fn to_bgrx(&self) -> u32 {
        (self.reserved as u32) << 24
            | (self.red as u32) << 16
            | (self.green as u32) << 8
            | self.blue as u32
    }

    /// Convert from u32 (BGRX format)
    pub fn from_bgrx(value: u32) -> Self {
        Self {
            blue: value as u8,
            green: (value >> 8) as u8,
            red: (value >> 16) as u8,
            reserved: (value >> 24) as u8,
        }
    }

    /// Convert to u32 (RGBX format)
    pub fn to_rgbx(&self) -> u32 {
        (self.reserved as u32) << 24
            | (self.blue as u32) << 16
            | (self.green as u32) << 8
            | self.red as u32
    }

    /// Convert from u32 (RGBX format)
    pub fn from_rgbx(value: u32) -> Self {
        Self {
            red: value as u8,
            green: (value >> 8) as u8,
            blue: (value >> 16) as u8,
            reserved: (value >> 24) as u8,
        }
    }
}

/// BLT operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GopBltOperation {
    /// Write data from BltBuffer to video
    VideoFill = 0,
    /// Read data from video to BltBuffer
    VideoToBltBuffer = 1,
    /// Write data from BltBuffer to video
    BltBufferToVideo = 2,
    /// Copy video to video
    VideoToVideo = 3,
    /// Max sentinel
    Max = 4,
}

/// GOP statistics
#[derive(Debug, Default)]
pub struct GopStats {
    /// Mode queries
    mode_queries: AtomicU64,
    /// Mode sets
    mode_sets: AtomicU64,
    /// BLT operations
    blt_operations: AtomicU64,
    /// Pixels transferred
    pixels_transferred: AtomicU64,
}

impl GopStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record mode query
    pub fn record_mode_query(&self) {
        self.mode_queries.fetch_add(1, Ordering::Relaxed);
    }

    /// Record mode set
    pub fn record_mode_set(&self) {
        self.mode_sets.fetch_add(1, Ordering::Relaxed);
    }

    /// Record BLT operation
    pub fn record_blt(&self, pixels: u64) {
        self.blt_operations.fetch_add(1, Ordering::Relaxed);
        self.pixels_transferred.fetch_add(pixels, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> GopStatsSnapshot {
        GopStatsSnapshot {
            mode_queries: self.mode_queries.load(Ordering::Relaxed),
            mode_sets: self.mode_sets.load(Ordering::Relaxed),
            blt_operations: self.blt_operations.load(Ordering::Relaxed),
            pixels_transferred: self.pixels_transferred.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct GopStatsSnapshot {
    /// Mode queries
    pub mode_queries: u64,
    /// Mode sets
    pub mode_sets: u64,
    /// BLT operations
    pub blt_operations: u64,
    /// Pixels transferred
    pub pixels_transferred: u64,
}

/// Graphics Output Protocol
pub struct GraphicsOutputProtocol {
    /// Handle
    handle: Handle,
    /// Available modes
    modes: Vec<GopModeInfo>,
    /// Current mode index
    current_mode: u32,
    /// Framebuffer base address
    framebuffer_base: u64,
    /// Framebuffer data (for emulation)
    framebuffer: Vec<u8>,
    /// Statistics
    stats: GopStats,
}

impl Default for GraphicsOutputProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphicsOutputProtocol {
    /// Create new GOP
    pub fn new() -> Self {
        let default_mode = GopModeInfo::default();
        let fb_size = default_mode.framebuffer_size() as usize;

        Self {
            handle: Handle::NULL,
            modes: vec![default_mode],
            current_mode: 0,
            framebuffer_base: 0xFD000000, // Standard linear framebuffer address
            framebuffer: vec![0; fb_size],
            stats: GopStats::new(),
        }
    }

    /// Create with specific modes
    pub fn with_modes(modes: Vec<GopModeInfo>) -> Self {
        let fb_size = modes.first().map(|m| m.framebuffer_size()).unwrap_or(0) as usize;

        Self {
            handle: Handle::NULL,
            modes,
            current_mode: 0,
            framebuffer_base: 0xFD000000,
            framebuffer: vec![0; fb_size],
            stats: GopStats::new(),
        }
    }

    /// Get GUID
    pub fn guid() -> Guid {
        guids::EFI_GRAPHICS_OUTPUT_PROTOCOL
    }

    /// Get handle
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// Set handle
    pub fn set_handle(&mut self, handle: Handle) {
        self.handle = handle;
    }

    /// Get statistics
    pub fn stats(&self) -> &GopStats {
        &self.stats
    }

    /// Get framebuffer base
    pub fn framebuffer_base(&self) -> u64 {
        self.framebuffer_base
    }

    /// Set framebuffer base
    pub fn set_framebuffer_base(&mut self, base: u64) {
        self.framebuffer_base = base;
    }

    /// Get framebuffer data
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Get mutable framebuffer data
    pub fn framebuffer_mut(&mut self) -> &mut [u8] {
        &mut self.framebuffer
    }

    /// Query mode
    pub fn query_mode(&self, mode_number: u32) -> Result<GopModeInfo, Status> {
        self.stats.record_mode_query();

        if mode_number as usize >= self.modes.len() {
            return Err(Status::INVALID_PARAMETER);
        }

        Ok(self.modes[mode_number as usize])
    }

    /// Set mode
    pub fn set_mode(&mut self, mode_number: u32) -> Status {
        self.stats.record_mode_set();

        if mode_number as usize >= self.modes.len() {
            return Status::INVALID_PARAMETER;
        }

        self.current_mode = mode_number;
        let mode = &self.modes[mode_number as usize];
        let new_size = mode.framebuffer_size() as usize;

        // Resize framebuffer
        self.framebuffer.resize(new_size, 0);
        self.framebuffer.fill(0);

        Status::SUCCESS
    }

    /// Get current mode
    pub fn mode(&self) -> GopMode {
        let mode_info = &self.modes[self.current_mode as usize];
        GopMode {
            max_mode: self.modes.len() as u32,
            mode: self.current_mode,
            info: 0, // Address would be set by firmware
            size_of_info: std::mem::size_of::<GopModeInfo>() as u64,
            frame_buffer_base: self.framebuffer_base,
            frame_buffer_size: mode_info.framebuffer_size(),
        }
    }

    /// Get current mode info
    pub fn current_mode_info(&self) -> &GopModeInfo {
        &self.modes[self.current_mode as usize]
    }

    /// Number of modes
    pub fn max_mode(&self) -> u32 {
        self.modes.len() as u32
    }

    /// Add mode
    pub fn add_mode(&mut self, mode: GopModeInfo) {
        self.modes.push(mode);
    }

    /// BLT (Block Transfer) operation
    pub fn blt(
        &mut self,
        blt_buffer: Option<&mut [GopBltPixel]>,
        blt_operation: GopBltOperation,
        source_x: u32,
        source_y: u32,
        dest_x: u32,
        dest_y: u32,
        width: u32,
        height: u32,
        delta: u32,
    ) -> Status {
        let mode = &self.modes[self.current_mode as usize];
        let bpp = mode.pixel_format.bytes_per_pixel();
        let stride = mode.pixels_per_scan_line * bpp;

        let pixels = width as u64 * height as u64;
        self.stats.record_blt(pixels);

        match blt_operation {
            GopBltOperation::VideoFill => {
                // Fill video with single pixel from buffer
                let pixel = blt_buffer
                    .and_then(|b| b.first())
                    .copied()
                    .unwrap_or(GopBltPixel::BLACK);

                let pixel_value = pixel.to_bgrx();

                for y in 0..height {
                    let row_offset = ((dest_y + y) * stride + dest_x * bpp) as usize;
                    for x in 0..width {
                        let offset = row_offset + (x * bpp) as usize;
                        if offset + 4 <= self.framebuffer.len() {
                            self.framebuffer[offset..offset + 4]
                                .copy_from_slice(&pixel_value.to_le_bytes());
                        }
                    }
                }
            }

            GopBltOperation::VideoToBltBuffer => {
                // Read from video to buffer
                if let Some(buffer) = blt_buffer {
                    let buffer_stride = if delta == 0 { width } else { delta / 4 };

                    for y in 0..height {
                        let src_row = ((source_y + y) * stride + source_x * bpp) as usize;
                        let dst_row = (y * buffer_stride) as usize;

                        for x in 0..width {
                            let src_offset = src_row + (x * bpp) as usize;
                            let dst_offset = dst_row + x as usize;

                            if src_offset + 4 <= self.framebuffer.len() && dst_offset < buffer.len()
                            {
                                let value = u32::from_le_bytes([
                                    self.framebuffer[src_offset],
                                    self.framebuffer[src_offset + 1],
                                    self.framebuffer[src_offset + 2],
                                    self.framebuffer[src_offset + 3],
                                ]);
                                buffer[dst_offset] = GopBltPixel::from_bgrx(value);
                            }
                        }
                    }
                }
            }

            GopBltOperation::BltBufferToVideo => {
                // Write from buffer to video
                if let Some(buffer) = blt_buffer {
                    let buffer_stride = if delta == 0 { width } else { delta / 4 };

                    for y in 0..height {
                        let src_row = ((source_y + y) * buffer_stride + source_x) as usize;
                        let dst_row = ((dest_y + y) * stride + dest_x * bpp) as usize;

                        for x in 0..width {
                            let src_offset = src_row + x as usize;
                            let dst_offset = dst_row + (x * bpp) as usize;

                            if src_offset < buffer.len() && dst_offset + 4 <= self.framebuffer.len()
                            {
                                let pixel_value = buffer[src_offset].to_bgrx();
                                self.framebuffer[dst_offset..dst_offset + 4]
                                    .copy_from_slice(&pixel_value.to_le_bytes());
                            }
                        }
                    }
                }
            }

            GopBltOperation::VideoToVideo => {
                // Copy within video memory
                // Handle overlapping regions
                if source_y < dest_y || (source_y == dest_y && source_x < dest_x) {
                    // Copy backwards
                    for y in (0..height).rev() {
                        let src_row = ((source_y + y) * stride + source_x * bpp) as usize;
                        let dst_row = ((dest_y + y) * stride + dest_x * bpp) as usize;
                        let row_bytes = (width * bpp) as usize;

                        if src_row + row_bytes <= self.framebuffer.len()
                            && dst_row + row_bytes <= self.framebuffer.len()
                        {
                            // Use temporary buffer for overlap safety
                            let temp: Vec<u8> =
                                self.framebuffer[src_row..src_row + row_bytes].to_vec();
                            self.framebuffer[dst_row..dst_row + row_bytes].copy_from_slice(&temp);
                        }
                    }
                } else {
                    // Copy forwards
                    for y in 0..height {
                        let src_row = ((source_y + y) * stride + source_x * bpp) as usize;
                        let dst_row = ((dest_y + y) * stride + dest_x * bpp) as usize;
                        let row_bytes = (width * bpp) as usize;

                        if src_row + row_bytes <= self.framebuffer.len()
                            && dst_row + row_bytes <= self.framebuffer.len()
                        {
                            let temp: Vec<u8> =
                                self.framebuffer[src_row..src_row + row_bytes].to_vec();
                            self.framebuffer[dst_row..dst_row + row_bytes].copy_from_slice(&temp);
                        }
                    }
                }
            }

            GopBltOperation::Max => {
                return Status::INVALID_PARAMETER;
            }
        }

        Status::SUCCESS
    }

    /// Get pixel at coordinates
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<GopBltPixel> {
        let mode = &self.modes[self.current_mode as usize];
        let bpp = mode.pixel_format.bytes_per_pixel();
        let stride = mode.pixels_per_scan_line * bpp;

        if x >= mode.horizontal_resolution || y >= mode.vertical_resolution {
            return None;
        }

        let offset = (y * stride + x * bpp) as usize;
        if offset + 4 <= self.framebuffer.len() {
            let value = u32::from_le_bytes([
                self.framebuffer[offset],
                self.framebuffer[offset + 1],
                self.framebuffer[offset + 2],
                self.framebuffer[offset + 3],
            ]);
            Some(GopBltPixel::from_bgrx(value))
        } else {
            None
        }
    }

    /// Set pixel at coordinates
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: GopBltPixel) -> Status {
        let mode = &self.modes[self.current_mode as usize];
        let bpp = mode.pixel_format.bytes_per_pixel();
        let stride = mode.pixels_per_scan_line * bpp;

        if x >= mode.horizontal_resolution || y >= mode.vertical_resolution {
            return Status::INVALID_PARAMETER;
        }

        let offset = (y * stride + x * bpp) as usize;
        if offset + 4 <= self.framebuffer.len() {
            let value = pixel.to_bgrx();
            self.framebuffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            Status::SUCCESS
        } else {
            Status::INVALID_PARAMETER
        }
    }

    /// Clear screen
    pub fn clear(&mut self, pixel: GopBltPixel) {
        let mode = &self.modes[self.current_mode as usize];
        self.blt(
            Some(&mut [pixel]),
            GopBltOperation::VideoFill,
            0,
            0,
            0,
            0,
            mode.horizontal_resolution,
            mode.vertical_resolution,
            0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gop_pixel_format() {
        assert_eq!(GopPixelFormat::BlueGreenRedReserved8BitPerColor.bytes_per_pixel(), 4);
        assert!(GopPixelFormat::BlueGreenRedReserved8BitPerColor.has_framebuffer());
        assert!(!GopPixelFormat::BltOnly.has_framebuffer());
    }

    #[test]
    fn test_gop_bitmask() {
        let bgrx = GopPixelBitmask::bgrx();
        assert_eq!(bgrx.red_mask, 0x00FF0000);
        assert_eq!(bgrx.blue_mask, 0x000000FF);

        let rgbx = GopPixelBitmask::rgbx();
        assert_eq!(rgbx.red_mask, 0x000000FF);
        assert_eq!(rgbx.blue_mask, 0x00FF0000);
    }

    #[test]
    fn test_gop_mode_info() {
        let mode = GopModeInfo::new(1024, 768, GopPixelFormat::BlueGreenRedReserved8BitPerColor);
        assert_eq!(mode.horizontal_resolution, 1024);
        assert_eq!(mode.vertical_resolution, 768);
        assert_eq!(mode.framebuffer_size(), 1024 * 768 * 4);
        assert_eq!(mode.total_pixels(), 1024 * 768);
    }

    #[test]
    fn test_gop_mode_info_stride() {
        let mode = GopModeInfo::new(800, 600, GopPixelFormat::BlueGreenRedReserved8BitPerColor)
            .with_stride(1024);
        assert_eq!(mode.pixels_per_scan_line, 1024);
        assert_eq!(mode.framebuffer_size(), 1024 * 600 * 4);
    }

    #[test]
    fn test_gop_blt_pixel() {
        let pixel = GopBltPixel::new(255, 128, 64);
        assert_eq!(pixel.red, 255);
        assert_eq!(pixel.green, 128);
        assert_eq!(pixel.blue, 64);

        let bgrx = pixel.to_bgrx();
        let restored = GopBltPixel::from_bgrx(bgrx);
        assert_eq!(restored.red, pixel.red);
        assert_eq!(restored.green, pixel.green);
        assert_eq!(restored.blue, pixel.blue);
    }

    #[test]
    fn test_gop_blt_pixel_constants() {
        assert_eq!(GopBltPixel::BLACK.red, 0);
        assert_eq!(GopBltPixel::WHITE.red, 255);
        assert_eq!(GopBltPixel::RED.red, 255);
        assert_eq!(GopBltPixel::RED.green, 0);
    }

    #[test]
    fn test_gop_creation() {
        let gop = GraphicsOutputProtocol::new();
        assert_eq!(gop.max_mode(), 1);

        let mode = gop.mode();
        assert_eq!(mode.mode, 0);
        assert!(mode.frame_buffer_size > 0);
    }

    #[test]
    fn test_gop_query_mode() {
        let gop = GraphicsOutputProtocol::new();

        let result = gop.query_mode(0);
        assert!(result.is_ok());

        let result = gop.query_mode(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_gop_set_mode() {
        let modes = vec![
            GopModeInfo::new(800, 600, GopPixelFormat::BlueGreenRedReserved8BitPerColor),
            GopModeInfo::new(1024, 768, GopPixelFormat::BlueGreenRedReserved8BitPerColor),
        ];
        let mut gop = GraphicsOutputProtocol::with_modes(modes);

        let status = gop.set_mode(1);
        assert!(status.is_success());
        assert_eq!(gop.current_mode_info().horizontal_resolution, 1024);

        let status = gop.set_mode(100);
        assert!(status.is_error());
    }

    #[test]
    fn test_gop_set_pixel() {
        let mut gop = GraphicsOutputProtocol::new();
        let pixel = GopBltPixel::RED;

        let status = gop.set_pixel(100, 100, pixel);
        assert!(status.is_success());

        let read = gop.get_pixel(100, 100).unwrap();
        assert_eq!(read.red, pixel.red);
        assert_eq!(read.green, pixel.green);
        assert_eq!(read.blue, pixel.blue);
    }

    #[test]
    fn test_gop_set_pixel_out_of_bounds() {
        let mut gop = GraphicsOutputProtocol::new();

        let status = gop.set_pixel(10000, 10000, GopBltPixel::RED);
        assert!(status.is_error());

        let result = gop.get_pixel(10000, 10000);
        assert!(result.is_none());
    }

    #[test]
    fn test_gop_blt_video_fill() {
        let mut gop = GraphicsOutputProtocol::new();
        let pixel = GopBltPixel::GREEN;

        let status = gop.blt(
            Some(&mut [pixel]),
            GopBltOperation::VideoFill,
            0,
            0,
            10,
            10,
            5,
            5,
            0,
        );
        assert!(status.is_success());

        // Check filled pixels
        let read = gop.get_pixel(12, 12).unwrap();
        assert_eq!(read.green, pixel.green);
    }

    #[test]
    fn test_gop_blt_buffer_to_video() {
        let mut gop = GraphicsOutputProtocol::new();
        let mut buffer = vec![GopBltPixel::BLUE; 4];

        let status = gop.blt(
            Some(&mut buffer),
            GopBltOperation::BltBufferToVideo,
            0,
            0,
            0,
            0,
            2,
            2,
            0,
        );
        assert!(status.is_success());

        let read = gop.get_pixel(0, 0).unwrap();
        assert_eq!(read.blue, GopBltPixel::BLUE.blue);
    }

    #[test]
    fn test_gop_blt_video_to_buffer() {
        let mut gop = GraphicsOutputProtocol::new();
        gop.set_pixel(5, 5, GopBltPixel::RED);

        let mut buffer = vec![GopBltPixel::BLACK; 1];
        let status = gop.blt(
            Some(&mut buffer),
            GopBltOperation::VideoToBltBuffer,
            5,
            5,
            0,
            0,
            1,
            1,
            0,
        );
        assert!(status.is_success());
        assert_eq!(buffer[0].red, 255);
    }

    #[test]
    fn test_gop_clear() {
        let mut gop = GraphicsOutputProtocol::new();
        gop.set_pixel(0, 0, GopBltPixel::RED);

        gop.clear(GopBltPixel::WHITE);

        let pixel = gop.get_pixel(0, 0).unwrap();
        assert_eq!(pixel.red, 255);
        assert_eq!(pixel.green, 255);
        assert_eq!(pixel.blue, 255);
    }

    #[test]
    fn test_gop_stats() {
        let mut gop = GraphicsOutputProtocol::new();

        gop.query_mode(0).ok();
        gop.set_mode(0);
        gop.blt(None, GopBltOperation::VideoFill, 0, 0, 0, 0, 10, 10, 0);

        let stats = gop.stats().snapshot();
        assert!(stats.mode_queries > 0);
        assert!(stats.mode_sets > 0);
        assert!(stats.blt_operations > 0);
        assert_eq!(stats.pixels_transferred, 100);
    }

    #[test]
    fn test_gop_framebuffer_access() {
        let mut gop = GraphicsOutputProtocol::new();

        // Direct framebuffer access
        let fb = gop.framebuffer_mut();
        fb[0] = 255; // Blue
        fb[1] = 0;   // Green
        fb[2] = 0;   // Red
        fb[3] = 0;   // Reserved

        let pixel = gop.get_pixel(0, 0).unwrap();
        assert_eq!(pixel.blue, 255);
        assert_eq!(pixel.red, 0);
    }

    #[test]
    fn test_gop_video_to_video() {
        let mut gop = GraphicsOutputProtocol::new();

        // Set source pixels
        gop.set_pixel(0, 0, GopBltPixel::RED);
        gop.set_pixel(1, 0, GopBltPixel::GREEN);

        // Copy to different location
        let status = gop.blt(
            None,
            GopBltOperation::VideoToVideo,
            0,
            0,
            10,
            10,
            2,
            1,
            0,
        );
        assert!(status.is_success());

        // Verify copy
        let p1 = gop.get_pixel(10, 10).unwrap();
        assert_eq!(p1.red, 255);
        let p2 = gop.get_pixel(11, 10).unwrap();
        assert_eq!(p2.green, 255);
    }

    #[test]
    fn test_gop_add_mode() {
        let mut gop = GraphicsOutputProtocol::new();
        let initial_count = gop.max_mode();

        gop.add_mode(GopModeInfo::new(1920, 1080, GopPixelFormat::BlueGreenRedReserved8BitPerColor));

        assert_eq!(gop.max_mode(), initial_count + 1);
    }
}
