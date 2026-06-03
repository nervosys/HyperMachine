//! VirtIO-GPU Device Implementation
//!
//! This module provides a VirtIO-GPU device with 2D operations,
//! scanout configuration, and resource management.

use super::core::{Color, DisplayMode, DisplaySurface, PixelFormat, Rect, Scanout};
use super::framebuffer::Framebuffer;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// VirtIO GPU control type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VirtioGpuCtrlType {
    // 2D commands
    CmdGetDisplayInfo = 0x0100,
    CmdResourceCreate2d = 0x0101,
    CmdResourceUnref = 0x0102,
    CmdSetScanout = 0x0103,
    CmdResourceFlush = 0x0104,
    CmdTransferToHost2d = 0x0105,
    CmdResourceAttachBacking = 0x0106,
    CmdResourceDetachBacking = 0x0107,
    CmdGetCapsetInfo = 0x0108,
    CmdGetCapset = 0x0109,
    CmdGetEdid = 0x010a,

    // Cursor commands
    CmdUpdateCursor = 0x0300,
    CmdMoveCursor = 0x0301,

    // Responses
    RespOkNodata = 0x1100,
    RespOkDisplayInfo = 0x1101,
    RespOkCapsetInfo = 0x1102,
    RespOkCapset = 0x1103,
    RespOkEdid = 0x1104,

    // Errors
    RespErrUnspec = 0x1200,
    RespErrOutOfMemory = 0x1201,
    RespErrInvalidScanoutId = 0x1202,
    RespErrInvalidResourceId = 0x1203,
    RespErrInvalidContextId = 0x1204,
    RespErrInvalidParameter = 0x1205,
}

impl VirtioGpuCtrlType {
    /// Create from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x0100 => Some(Self::CmdGetDisplayInfo),
            0x0101 => Some(Self::CmdResourceCreate2d),
            0x0102 => Some(Self::CmdResourceUnref),
            0x0103 => Some(Self::CmdSetScanout),
            0x0104 => Some(Self::CmdResourceFlush),
            0x0105 => Some(Self::CmdTransferToHost2d),
            0x0106 => Some(Self::CmdResourceAttachBacking),
            0x0107 => Some(Self::CmdResourceDetachBacking),
            0x0108 => Some(Self::CmdGetCapsetInfo),
            0x0109 => Some(Self::CmdGetCapset),
            0x010a => Some(Self::CmdGetEdid),
            0x0300 => Some(Self::CmdUpdateCursor),
            0x0301 => Some(Self::CmdMoveCursor),
            0x1100 => Some(Self::RespOkNodata),
            0x1101 => Some(Self::RespOkDisplayInfo),
            _ => None,
        }
    }
}

/// GPU command with payload data for dispatch
#[derive(Debug, Clone)]
pub enum GpuCommand {
    /// Get display info for all scanouts
    GetDisplayInfo,
    /// Create a 2D resource
    ResourceCreate2d {
        resource_id: u32,
        format: VirtioGpuFormat,
        width: u32,
        height: u32,
    },
    /// Destroy a resource
    ResourceUnref { resource_id: u32 },
    /// Bind a resource to a scanout
    SetScanout {
        scanout_id: u32,
        resource_id: u32,
        rect: Rect,
    },
    /// Flush a resource to the display
    ResourceFlush { resource_id: u32, rect: Rect },
    /// Transfer pixel data from guest to host resource
    TransferToHost2d {
        resource_id: u32,
        rect: Rect,
        data: Vec<u8>,
        offset: usize,
    },
    /// Attach backing pages to a resource
    ResourceAttachBacking { resource_id: u32 },
    /// Detach backing pages from a resource
    ResourceDetachBacking { resource_id: u32 },
    /// Update cursor image and position
    UpdateCursor {
        scanout_id: u32,
        x: u32,
        y: u32,
        resource_id: u32,
        hot_x: u32,
        hot_y: u32,
    },
    /// Move cursor position (image unchanged)
    MoveCursor { scanout_id: u32, x: u32, y: u32 },
    /// Query capability set info (capset index → id + max version + max size)
    GetCapsetInfo { capset_index: u32 },
    /// Retrieve a capability set blob
    GetCapset { capset_id: u32, capset_version: u32 },
    /// Get EDID data for a scanout
    GetEdid { scanout_id: u32 },
}

/// Capability set information returned by `CmdGetCapsetInfo`.
#[derive(Debug, Clone)]
pub struct CapsetInfo {
    /// Capability set ID (e.g., VIRTIO_GPU_CAPSET_VIRGL = 1)
    pub capset_id: u32,
    /// Maximum version supported
    pub max_version: u32,
    /// Maximum size of the capability blob in bytes
    pub max_size: u32,
}

/// VirtIO GPU formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VirtioGpuFormat {
    B8G8R8A8Unorm = 1,
    B8G8R8X8Unorm = 2,
    A8R8G8B8Unorm = 3,
    X8R8G8B8Unorm = 4,
    R8G8B8A8Unorm = 67,
    X8B8G8R8Unorm = 68,
    A8B8G8R8Unorm = 121,
    R8G8B8X8Unorm = 134,
}

impl VirtioGpuFormat {
    /// Create from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::B8G8R8A8Unorm),
            2 => Some(Self::B8G8R8X8Unorm),
            3 => Some(Self::A8R8G8B8Unorm),
            4 => Some(Self::X8R8G8B8Unorm),
            67 => Some(Self::R8G8B8A8Unorm),
            68 => Some(Self::X8B8G8R8Unorm),
            121 => Some(Self::A8B8G8R8Unorm),
            134 => Some(Self::R8G8B8X8Unorm),
            _ => None,
        }
    }

    /// Convert to internal pixel format
    pub fn to_pixel_format(&self) -> PixelFormat {
        match self {
            VirtioGpuFormat::B8G8R8A8Unorm => PixelFormat::Bgra32,
            VirtioGpuFormat::B8G8R8X8Unorm => PixelFormat::Bgra32,
            VirtioGpuFormat::A8R8G8B8Unorm => PixelFormat::Argb32,
            VirtioGpuFormat::X8R8G8B8Unorm => PixelFormat::Xrgb32,
            VirtioGpuFormat::R8G8B8A8Unorm => PixelFormat::Rgba32,
            VirtioGpuFormat::X8B8G8R8Unorm => PixelFormat::Xrgb32,
            VirtioGpuFormat::A8B8G8R8Unorm => PixelFormat::Argb32,
            VirtioGpuFormat::R8G8B8X8Unorm => PixelFormat::Rgba32,
        }
    }

    /// Bytes per pixel
    pub fn bytes_per_pixel(&self) -> u32 {
        4 // All VirtIO GPU formats are 32-bit
    }
}

/// GPU resource (2D image)
#[derive(Debug)]
pub struct GpuResource {
    /// Resource ID
    pub id: u32,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Format
    pub format: VirtioGpuFormat,
    /// Pixel data
    pub data: Vec<u8>,
    /// Backing pages attached
    pub backing_attached: bool,
}

impl GpuResource {
    /// Create new resource
    pub fn new(id: u32, width: u32, height: u32, format: VirtioGpuFormat) -> Self {
        let size = (width * height * format.bytes_per_pixel()) as usize;
        Self {
            id,
            width,
            height,
            format,
            data: vec![0; size],
            backing_attached: false,
        }
    }

    /// Get stride
    pub fn stride(&self) -> u32 {
        self.width * self.format.bytes_per_pixel()
    }

    /// Get pixel at position
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color::BLACK;
        }

        let offset = (y * self.stride() + x * 4) as usize;
        if offset + 4 > self.data.len() {
            return Color::BLACK;
        }

        let bytes: [u8; 4] = self.data[offset..offset + 4]
            .try_into()
            .expect("slice is exactly 4 bytes");
        match self.format {
            VirtioGpuFormat::B8G8R8A8Unorm | VirtioGpuFormat::B8G8R8X8Unorm => {
                Color::new(bytes[2], bytes[1], bytes[0], bytes[3])
            }
            VirtioGpuFormat::A8R8G8B8Unorm | VirtioGpuFormat::X8R8G8B8Unorm => {
                Color::new(bytes[1], bytes[2], bytes[3], bytes[0])
            }
            VirtioGpuFormat::R8G8B8A8Unorm | VirtioGpuFormat::R8G8B8X8Unorm => {
                Color::new(bytes[0], bytes[1], bytes[2], bytes[3])
            }
            _ => Color::from_argb32(u32::from_le_bytes(bytes)),
        }
    }

    /// Set pixel at position
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = (y * self.stride() + x * 4) as usize;
        if offset + 4 > self.data.len() {
            return;
        }

        let bytes = match self.format {
            VirtioGpuFormat::B8G8R8A8Unorm | VirtioGpuFormat::B8G8R8X8Unorm => {
                [color.b, color.g, color.r, color.a]
            }
            VirtioGpuFormat::A8R8G8B8Unorm | VirtioGpuFormat::X8R8G8B8Unorm => {
                [color.a, color.r, color.g, color.b]
            }
            VirtioGpuFormat::R8G8B8A8Unorm | VirtioGpuFormat::R8G8B8X8Unorm => {
                [color.r, color.g, color.b, color.a]
            }
            _ => color.to_argb32().to_le_bytes(),
        };
        self.data[offset..offset + 4].copy_from_slice(&bytes);
    }

    /// Transfer data from guest to host
    pub fn transfer_to_host(&mut self, rect: &Rect, data: &[u8], offset: usize) {
        let bpp = self.format.bytes_per_pixel() as usize;
        let src_stride = rect.width as usize * bpp;
        let dst_stride = self.stride() as usize;

        if src_stride == 0 || rect.height == 0 {
            return;
        }

        // Fast path: a full-width transfer (x == 0, matching strides) lays the
        // source rows out contiguously over a contiguous destination span, so
        // the whole rectangle collapses to a single memcpy instead of one
        // bounds-checked copy per scanline. This is the common whole-surface
        // flush a guest issues after rendering a frame.
        if rect.x == 0 && src_stride == dst_stride {
            let total = src_stride * rect.height as usize;
            let dst_start = rect.y as usize * dst_stride;
            if offset + total <= data.len() && dst_start + total <= self.data.len() {
                self.data[dst_start..dst_start + total]
                    .copy_from_slice(&data[offset..offset + total]);
                return;
            }
        }

        for y in 0..rect.height {
            let src_row_start = offset + y as usize * src_stride;
            let dst_row_start = (rect.y + y) as usize * dst_stride + rect.x as usize * bpp;

            if src_row_start + src_stride <= data.len()
                && dst_row_start + src_stride <= self.data.len()
            {
                self.data[dst_row_start..dst_row_start + src_stride]
                    .copy_from_slice(&data[src_row_start..src_row_start + src_stride]);
            }
        }
    }
}

/// Scanout state
#[derive(Debug)]
pub struct ScanoutState {
    /// Configuration
    pub config: Scanout,
    /// Resource ID bound to this scanout
    pub resource_id: u32,
    /// Source rectangle in resource
    pub src_rect: Rect,
}

impl ScanoutState {
    /// Create new scanout state
    pub fn new(id: u32, mode: DisplayMode) -> Self {
        Self {
            config: Scanout::new(id, mode),
            resource_id: 0,
            src_rect: Rect::default(),
        }
    }
}

/// Cursor state for a VirtIO GPU device
#[derive(Debug, Clone, Default)]
pub struct CursorState {
    /// Scanout the cursor belongs to
    pub scanout_id: u32,
    /// Cursor X position
    pub x: u32,
    /// Cursor Y position
    pub y: u32,
    /// Resource ID providing the cursor image (0 = hidden)
    pub resource_id: u32,
    /// Hot-spot X offset within the cursor image
    pub hot_x: u32,
    /// Hot-spot Y offset within the cursor image
    pub hot_y: u32,
    /// Whether the cursor is visible
    pub visible: bool,
}

/// VirtIO GPU device
#[derive(Debug)]
pub struct VirtioGpu {
    /// Device name
    name: String,
    /// Resources
    resources: HashMap<u32, GpuResource>,
    /// Scanouts
    scanouts: Vec<ScanoutState>,
    /// Primary framebuffer
    framebuffer: Framebuffer,
    /// Next resource ID
    next_resource_id: AtomicU32,
    /// Enabled
    enabled: bool,
    /// Statistics
    stats: VirtioGpuStats,
    /// Supported capability sets (index → info)
    capsets: Vec<CapsetInfo>,
    /// Hardware cursor state
    cursor: CursorState,
}

/// VirtIO GPU statistics
#[derive(Debug, Default)]
pub struct VirtioGpuStats {
    /// Commands processed
    pub commands: AtomicU64,
    /// Resources created
    pub resources_created: AtomicU64,
    /// Resources destroyed
    pub resources_destroyed: AtomicU64,
    /// Transfers
    pub transfers: AtomicU64,
    /// Flushes
    pub flushes: AtomicU64,
    /// Scanout updates
    pub scanout_updates: AtomicU64,
}

impl VirtioGpu {
    /// Maximum scanouts
    pub const MAX_SCANOUTS: usize = 16;

    /// Create new VirtIO GPU
    pub fn new(name: &str, width: u32, height: u32) -> Self {
        let mode = DisplayMode::new(width, height, PixelFormat::Xrgb32);
        let mut scanouts = Vec::new();
        scanouts.push(ScanoutState::new(0, mode));

        Self {
            name: name.to_string(),
            resources: HashMap::new(),
            scanouts,
            framebuffer: Framebuffer::new(mode),
            next_resource_id: AtomicU32::new(1),
            enabled: true,
            stats: VirtioGpuStats::default(),
            capsets: vec![
                // VIRTIO_GPU_CAPSET_VIRGL (basic 3D)
                CapsetInfo {
                    capset_id: 1,
                    max_version: 1,
                    max_size: 0,
                },
                // VIRTIO_GPU_CAPSET_VIRGL2 (extended 3D)
                CapsetInfo {
                    capset_id: 2,
                    max_version: 1,
                    max_size: 0,
                },
            ],
            cursor: CursorState::default(),
        }
    }

    /// Get name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get framebuffer
    pub fn framebuffer(&self) -> &Framebuffer {
        &self.framebuffer
    }

    /// Get mutable framebuffer
    pub fn framebuffer_mut(&mut self) -> &mut Framebuffer {
        &mut self.framebuffer
    }

    /// Get statistics
    pub fn stats(&self) -> &VirtioGpuStats {
        &self.stats
    }

    /// Get number of supported capability sets
    pub fn num_capsets(&self) -> u32 {
        self.capsets.len() as u32
    }

    /// Get capability set info by index
    pub fn get_capset_info(&self, index: u32) -> Option<&CapsetInfo> {
        self.capsets.get(index as usize)
    }

    /// Get capability set blob (returns empty blob for now)
    pub fn get_capset(&self, capset_id: u32, _version: u32) -> Option<Vec<u8>> {
        if self.capsets.iter().any(|c| c.capset_id == capset_id) {
            // Capability set data would be populated by the 3D renderer backend
            Some(Vec::new())
        } else {
            None
        }
    }

    /// Get current cursor state
    pub fn cursor(&self) -> &CursorState {
        &self.cursor
    }

    /// Update cursor image and position
    pub fn update_cursor(
        &mut self,
        scanout_id: u32,
        x: u32,
        y: u32,
        resource_id: u32,
        hot_x: u32,
        hot_y: u32,
    ) -> Result<(), VirtioGpuError> {
        if scanout_id as usize >= self.scanouts.len() {
            return Err(VirtioGpuError::InvalidScanoutId);
        }
        if resource_id != 0 && !self.resources.contains_key(&resource_id) {
            return Err(VirtioGpuError::InvalidResourceId);
        }
        self.cursor = CursorState {
            scanout_id,
            x,
            y,
            resource_id,
            hot_x,
            hot_y,
            visible: resource_id != 0,
        };
        Ok(())
    }

    /// Move cursor position without changing image
    pub fn move_cursor(&mut self, scanout_id: u32, x: u32, y: u32) -> Result<(), VirtioGpuError> {
        if scanout_id as usize >= self.scanouts.len() {
            return Err(VirtioGpuError::InvalidScanoutId);
        }
        self.cursor.scanout_id = scanout_id;
        self.cursor.x = x;
        self.cursor.y = y;
        Ok(())
    }

    /// Get EDID data for a scanout (128-byte base EDID block)
    pub fn get_edid(&self, scanout_id: u32) -> Option<Vec<u8>> {
        let scanout = self.scanouts.get(scanout_id as usize)?;
        let w = scanout.config.mode.width;
        let h = scanout.config.mode.height;

        // Build a minimal 128-byte EDID 1.4 block
        let mut edid = vec![0u8; 128];
        // Header
        edid[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        // Manufacturer ID "HVM" (H=8,V=22,M=13) packed into 2 bytes
        edid[8] = 0x22; // (H-1)<<2 | (V-1)>>3
        edid[9] = 0xCD; // ((V-1)&7)<<5 | (M-1)
                        // EDID version 1.4
        edid[18] = 1;
        edid[19] = 4;
        // Preferred timing: encode active pixels
        // Detailed timing descriptor at offset 54
        let pixel_clock: u16 = ((w as u32 * h as u32 * 60) / 10000) as u16;
        edid[54] = pixel_clock as u8;
        edid[55] = (pixel_clock >> 8) as u8;
        edid[56] = w as u8;
        edid[58] = ((w >> 8) as u8 & 0x0F) << 4;
        edid[59] = h as u8;
        edid[61] = ((h >> 8) as u8 & 0x0F) << 4;
        // Checksum: make byte 127 such that all bytes sum to 0 mod 256
        let sum: u8 = edid[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        edid[127] = 0u8.wrapping_sub(sum);

        Some(edid)
    }

    /// Add scanout
    pub fn add_scanout(&mut self, mode: DisplayMode) -> u32 {
        let id = self.scanouts.len() as u32;
        if (id as usize) < Self::MAX_SCANOUTS {
            self.scanouts.push(ScanoutState::new(id, mode));
        }
        id
    }

    /// Get scanout info
    pub fn get_scanout(&self, id: u32) -> Option<&ScanoutState> {
        self.scanouts.get(id as usize)
    }

    /// Create 2D resource
    pub fn create_resource_2d(
        &mut self,
        resource_id: u32,
        format: VirtioGpuFormat,
        width: u32,
        height: u32,
    ) -> Result<(), VirtioGpuError> {
        if self.resources.contains_key(&resource_id) {
            return Err(VirtioGpuError::ResourceExists);
        }

        if width == 0 || height == 0 {
            return Err(VirtioGpuError::InvalidParameter);
        }

        let resource = GpuResource::new(resource_id, width, height, format);
        self.resources.insert(resource_id, resource);
        self.stats.resources_created.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Destroy resource
    pub fn unref_resource(&mut self, resource_id: u32) -> Result<(), VirtioGpuError> {
        if self.resources.remove(&resource_id).is_some() {
            self.stats
                .resources_destroyed
                .fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            Err(VirtioGpuError::InvalidResourceId)
        }
    }

    /// Attach backing memory
    pub fn attach_backing(&mut self, resource_id: u32) -> Result<(), VirtioGpuError> {
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(VirtioGpuError::InvalidResourceId)?;

        resource.backing_attached = true;
        Ok(())
    }

    /// Detach backing memory
    pub fn detach_backing(&mut self, resource_id: u32) -> Result<(), VirtioGpuError> {
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(VirtioGpuError::InvalidResourceId)?;

        resource.backing_attached = false;
        Ok(())
    }

    /// Transfer data to host (guest → resource)
    pub fn transfer_to_host_2d(
        &mut self,
        resource_id: u32,
        rect: &Rect,
        data: &[u8],
        offset: usize,
    ) -> Result<(), VirtioGpuError> {
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(VirtioGpuError::InvalidResourceId)?;

        if !resource.backing_attached {
            return Err(VirtioGpuError::BackingNotAttached);
        }

        resource.transfer_to_host(rect, data, offset);
        self.stats.transfers.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Set scanout (bind resource to scanout)
    pub fn set_scanout(
        &mut self,
        scanout_id: u32,
        resource_id: u32,
        rect: &Rect,
    ) -> Result<(), VirtioGpuError> {
        let scanout = self
            .scanouts
            .get_mut(scanout_id as usize)
            .ok_or(VirtioGpuError::InvalidScanoutId)?;

        // Resource ID 0 means disable scanout
        if resource_id == 0 {
            scanout.resource_id = 0;
            scanout.config.enabled = false;
            return Ok(());
        }

        if !self.resources.contains_key(&resource_id) {
            return Err(VirtioGpuError::InvalidResourceId);
        }

        scanout.resource_id = resource_id;
        scanout.src_rect = *rect;
        scanout.config.enabled = true;
        self.stats.scanout_updates.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Flush resource to display
    pub fn resource_flush(&mut self, resource_id: u32, rect: &Rect) -> Result<(), VirtioGpuError> {
        // Find which scanout uses this resource
        let scanout_info: Option<(Rect, bool)> = self.scanouts.iter().find_map(|s| {
            if s.resource_id == resource_id && s.config.enabled {
                Some((s.src_rect, true))
            } else {
                None
            }
        });

        if scanout_info.is_none() {
            return Ok(()); // Resource not bound to any scanout
        }

        let resource = self
            .resources
            .get(&resource_id)
            .ok_or(VirtioGpuError::InvalidResourceId)?;

        // Copy resource to framebuffer
        let dst_rect = rect.intersection(&Rect::new(0, 0, resource.width, resource.height));
        if let Some(clip) = dst_rect {
            for y in clip.y..clip.bottom() {
                for x in clip.x..clip.right() {
                    let color = resource.get_pixel(x, y);
                    self.framebuffer.set_pixel(x, y, color);
                }
            }
        }

        self.framebuffer.stats().record_frame();
        self.stats.flushes.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Process control command
    pub fn process_command(
        &mut self,
        cmd_type: VirtioGpuCtrlType,
    ) -> Result<VirtioGpuCtrlType, VirtioGpuError> {
        self.stats.commands.fetch_add(1, Ordering::Relaxed);

        match cmd_type {
            VirtioGpuCtrlType::CmdGetDisplayInfo => Ok(VirtioGpuCtrlType::RespOkDisplayInfo),
            VirtioGpuCtrlType::CmdResourceCreate2d => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdResourceUnref => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdSetScanout => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdResourceFlush => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdTransferToHost2d => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdResourceAttachBacking => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdResourceDetachBacking => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdGetCapsetInfo => Ok(VirtioGpuCtrlType::RespOkCapsetInfo),
            VirtioGpuCtrlType::CmdGetCapset => Ok(VirtioGpuCtrlType::RespOkCapset),
            VirtioGpuCtrlType::CmdGetEdid => Ok(VirtioGpuCtrlType::RespOkEdid),
            VirtioGpuCtrlType::CmdUpdateCursor => Ok(VirtioGpuCtrlType::RespOkNodata),
            VirtioGpuCtrlType::CmdMoveCursor => Ok(VirtioGpuCtrlType::RespOkNodata),
            _ => Err(VirtioGpuError::InvalidCommand),
        }
    }

    /// Dispatch a command with full payload, calling the appropriate backend method.
    ///
    /// Unlike [`Self::process_command`] (which only validates the command type), this
    /// method performs the actual GPU operation.
    pub fn dispatch_command(
        &mut self,
        cmd: GpuCommand,
    ) -> Result<VirtioGpuCtrlType, VirtioGpuError> {
        self.stats.commands.fetch_add(1, Ordering::Relaxed);

        match cmd {
            GpuCommand::GetDisplayInfo => Ok(VirtioGpuCtrlType::RespOkDisplayInfo),
            GpuCommand::ResourceCreate2d {
                resource_id,
                format,
                width,
                height,
            } => {
                self.create_resource_2d(resource_id, format, width, height)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::ResourceUnref { resource_id } => {
                self.unref_resource(resource_id)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::SetScanout {
                scanout_id,
                resource_id,
                rect,
            } => {
                self.set_scanout(scanout_id, resource_id, &rect)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::ResourceFlush { resource_id, rect } => {
                self.resource_flush(resource_id, &rect)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::TransferToHost2d {
                resource_id,
                rect,
                data,
                offset,
            } => {
                self.transfer_to_host_2d(resource_id, &rect, &data, offset)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::ResourceAttachBacking { resource_id } => {
                self.attach_backing(resource_id)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::ResourceDetachBacking { resource_id } => {
                self.detach_backing(resource_id)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::UpdateCursor {
                scanout_id,
                x,
                y,
                resource_id,
                hot_x,
                hot_y,
            } => {
                self.update_cursor(scanout_id, x, y, resource_id, hot_x, hot_y)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::MoveCursor { scanout_id, x, y } => {
                self.move_cursor(scanout_id, x, y)?;
                Ok(VirtioGpuCtrlType::RespOkNodata)
            }
            GpuCommand::GetCapsetInfo { capset_index } => {
                if self.get_capset_info(capset_index).is_some() {
                    Ok(VirtioGpuCtrlType::RespOkCapsetInfo)
                } else {
                    Err(VirtioGpuError::InvalidParameter)
                }
            }
            GpuCommand::GetCapset {
                capset_id,
                capset_version,
            } => {
                if self.get_capset(capset_id, capset_version).is_some() {
                    Ok(VirtioGpuCtrlType::RespOkCapset)
                } else {
                    Err(VirtioGpuError::InvalidParameter)
                }
            }
            GpuCommand::GetEdid { scanout_id } => {
                if self.get_edid(scanout_id).is_some() {
                    Ok(VirtioGpuCtrlType::RespOkEdid)
                } else {
                    Err(VirtioGpuError::InvalidScanoutId)
                }
            }
        }
    }

    /// Get display info for all scanouts
    pub fn get_display_info(&self) -> Vec<DisplayInfo> {
        self.scanouts
            .iter()
            .map(|s| DisplayInfo {
                width: s.config.mode.width,
                height: s.config.mode.height,
                enabled: s.config.enabled,
                flags: if s.config.enabled { 1 } else { 0 },
            })
            .collect()
    }

    /// Enable/disable device
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get resource
    pub fn get_resource(&self, id: u32) -> Option<&GpuResource> {
        self.resources.get(&id)
    }

    /// Get mutable resource
    pub fn get_resource_mut(&mut self, id: u32) -> Option<&mut GpuResource> {
        self.resources.get_mut(&id)
    }

    /// Get number of resources
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// Get number of scanouts
    pub fn scanout_count(&self) -> usize {
        self.scanouts.len()
    }
}

/// Display info response
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Enabled
    pub enabled: bool,
    /// Flags
    pub flags: u32,
}

/// VirtIO GPU error
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VirtioGpuError {
    /// Invalid command
    #[error("invalid command")]
    InvalidCommand,
    /// Invalid resource ID
    #[error("invalid resource ID")]
    InvalidResourceId,
    /// Invalid scanout ID
    #[error("invalid scanout ID")]
    InvalidScanoutId,
    /// Resource already exists
    #[error("resource already exists")]
    ResourceExists,
    /// Invalid parameter
    #[error("invalid parameter")]
    InvalidParameter,
    /// Out of memory
    #[error("out of memory")]
    OutOfMemory,
    /// Backing not attached
    #[error("backing not attached")]
    BackingNotAttached,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtio_gpu_ctrl_type() {
        assert_eq!(
            VirtioGpuCtrlType::from_u32(0x0100),
            Some(VirtioGpuCtrlType::CmdGetDisplayInfo)
        );
        assert_eq!(
            VirtioGpuCtrlType::from_u32(0x0101),
            Some(VirtioGpuCtrlType::CmdResourceCreate2d)
        );
        assert!(VirtioGpuCtrlType::from_u32(0xFFFF).is_none());
    }

    #[test]
    fn test_virtio_gpu_format() {
        let format = VirtioGpuFormat::X8R8G8B8Unorm;
        assert_eq!(format.bytes_per_pixel(), 4);
        assert_eq!(format.to_pixel_format(), PixelFormat::Xrgb32);
    }

    #[test]
    fn test_gpu_resource_creation() {
        let resource = GpuResource::new(1, 100, 100, VirtioGpuFormat::X8R8G8B8Unorm);
        assert_eq!(resource.id, 1);
        assert_eq!(resource.width, 100);
        assert_eq!(resource.height, 100);
        assert_eq!(resource.stride(), 400);
    }

    #[test]
    fn test_gpu_resource_pixel_ops() {
        let mut resource = GpuResource::new(1, 10, 10, VirtioGpuFormat::X8R8G8B8Unorm);
        let color = Color::rgb(255, 128, 64);

        resource.set_pixel(5, 5, color);
        let read = resource.get_pixel(5, 5);

        assert_eq!(read.r, 255);
        assert_eq!(read.g, 128);
        assert_eq!(read.b, 64);
    }

    #[test]
    fn test_virtio_gpu_creation() {
        let gpu = VirtioGpu::new("gpu0", 1024, 768);
        assert_eq!(gpu.name(), "gpu0");
        assert_eq!(gpu.scanout_count(), 1);
        assert_eq!(gpu.resource_count(), 0);
    }

    #[test]
    fn test_create_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();

        assert_eq!(gpu.resource_count(), 1);
        assert!(gpu.get_resource(1).is_some());
    }

    #[test]
    fn test_create_resource_duplicate() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();

        let result = gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 50, 50);
        assert_eq!(result, Err(VirtioGpuError::ResourceExists));
    }

    #[test]
    fn test_unref_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        assert_eq!(gpu.resource_count(), 1);

        gpu.unref_resource(1).unwrap();
        assert_eq!(gpu.resource_count(), 0);
    }

    #[test]
    fn test_attach_detach_backing() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();

        assert!(!gpu.get_resource(1).unwrap().backing_attached);

        gpu.attach_backing(1).unwrap();
        assert!(gpu.get_resource(1).unwrap().backing_attached);

        gpu.detach_backing(1).unwrap();
        assert!(!gpu.get_resource(1).unwrap().backing_attached);
    }

    #[test]
    fn test_set_scanout() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 640, 480)
            .unwrap();

        gpu.set_scanout(0, 1, &Rect::new(0, 0, 640, 480)).unwrap();

        let scanout = gpu.get_scanout(0).unwrap();
        assert_eq!(scanout.resource_id, 1);
        assert!(scanout.config.enabled);
    }

    #[test]
    fn test_set_scanout_disable() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 640, 480)
            .unwrap();
        gpu.set_scanout(0, 1, &Rect::new(0, 0, 640, 480)).unwrap();
        assert!(gpu.get_scanout(0).unwrap().config.enabled);

        // Resource 0 disables scanout
        gpu.set_scanout(0, 0, &Rect::default()).unwrap();
        assert!(!gpu.get_scanout(0).unwrap().config.enabled);
    }

    #[test]
    fn test_resource_flush() {
        let mut gpu = VirtioGpu::new("gpu0", 100, 100);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.attach_backing(1).unwrap();
        gpu.set_scanout(0, 1, &Rect::new(0, 0, 100, 100)).unwrap();

        // Set a pixel in the resource
        gpu.get_resource_mut(1)
            .unwrap()
            .set_pixel(50, 50, Color::RED);

        // Flush to framebuffer
        gpu.resource_flush(1, &Rect::new(0, 0, 100, 100)).unwrap();

        // Check framebuffer
        assert_eq!(gpu.framebuffer().get_pixel(50, 50).r, 255);
    }

    #[test]
    fn test_transfer_to_host() {
        let mut gpu = VirtioGpu::new("gpu0", 100, 100);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 10, 10)
            .unwrap();
        gpu.attach_backing(1).unwrap();

        // Prepare test data (10x10 red pixels)
        let mut data = vec![0u8; 10 * 10 * 4];
        for i in 0..100 {
            // X8R8G8B8: [padding, R, G, B]
            data[i * 4] = 0;
            data[i * 4 + 1] = 255; // R
            data[i * 4 + 2] = 0; // G
            data[i * 4 + 3] = 0; // B
        }

        gpu.transfer_to_host_2d(1, &Rect::new(0, 0, 10, 10), &data, 0)
            .unwrap();

        let resource = gpu.get_resource(1).unwrap();
        let pixel = resource.get_pixel(5, 5);
        assert_eq!(pixel.r, 255);
    }

    #[test]
    fn test_transfer_to_host_fast_and_general_paths() {
        // 4x4 R8G8B8A8 resource: stride = 16 bytes.
        let mut gpu = VirtioGpu::new("gpu0", 100, 100);
        gpu.create_resource_2d(1, VirtioGpuFormat::R8G8B8A8Unorm, 4, 4)
            .unwrap();
        gpu.attach_backing(1).unwrap();

        // General path: a 2x2 sub-rect at (1,1) where src_stride (8) differs
        // from dst_stride (16), so the per-scanline branch runs.
        let block = [
            10u8, 20, 30, 40, 50, 60, 70, 80, // row 0: two pixels
            11, 21, 31, 41, 51, 61, 71, 81, // row 1: two pixels
        ];
        gpu.transfer_to_host_2d(1, &Rect::new(1, 1, 2, 2), &block, 0)
            .unwrap();
        let r = gpu.get_resource(1).unwrap();
        assert_eq!(r.get_pixel(1, 1).r, 10);
        assert_eq!(r.get_pixel(2, 1).r, 50);
        assert_eq!(r.get_pixel(1, 2).r, 11);
        assert_eq!(r.get_pixel(0, 0).r, 0, "untouched pixel stays clear");

        // Fast path at y>0: a full-width 4x2 block at (0,2) is contiguous in
        // both source and destination and collapses to one memcpy.
        let rows = vec![123u8; 4 * 4 * 2];
        gpu.transfer_to_host_2d(1, &Rect::new(0, 2, 4, 2), &rows, 0)
            .unwrap();
        let r = gpu.get_resource(1).unwrap();
        assert_eq!(r.get_pixel(0, 2).r, 123);
        assert_eq!(r.get_pixel(3, 3).r, 123);
        // The fast path must not bleed into the rows above it.
        assert_eq!(r.get_pixel(0, 0).r, 0);
    }

    #[test]
    fn test_process_command() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        let response = gpu
            .process_command(VirtioGpuCtrlType::CmdGetDisplayInfo)
            .unwrap();
        assert_eq!(response, VirtioGpuCtrlType::RespOkDisplayInfo);

        let response = gpu
            .process_command(VirtioGpuCtrlType::CmdResourceCreate2d)
            .unwrap();
        assert_eq!(response, VirtioGpuCtrlType::RespOkNodata);
    }

    #[test]
    fn test_get_display_info() {
        let gpu = VirtioGpu::new("gpu0", 1920, 1080);
        let info = gpu.get_display_info();

        assert_eq!(info.len(), 1);
        assert_eq!(info[0].width, 1920);
        assert_eq!(info[0].height, 1080);
    }

    #[test]
    fn test_add_scanout() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        assert_eq!(gpu.scanout_count(), 1);

        let id = gpu.add_scanout(DisplayMode::FULL_HD);
        assert_eq!(id, 1);
        assert_eq!(gpu.scanout_count(), 2);
    }

    #[test]
    fn test_enable_disable() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        assert!(gpu.is_enabled());

        gpu.set_enabled(false);
        assert!(!gpu.is_enabled());
    }

    #[test]
    fn test_stats() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.create_resource_2d(2, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.unref_resource(2).unwrap();

        let stats = gpu.stats();
        assert_eq!(stats.resources_created.load(Ordering::Relaxed), 2);
        assert_eq!(stats.resources_destroyed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_invalid_resource_operations() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        assert_eq!(
            gpu.unref_resource(999),
            Err(VirtioGpuError::InvalidResourceId)
        );
        assert_eq!(
            gpu.attach_backing(999),
            Err(VirtioGpuError::InvalidResourceId)
        );
        assert_eq!(
            gpu.set_scanout(0, 999, &Rect::default()),
            Err(VirtioGpuError::InvalidResourceId)
        );
    }

    #[test]
    fn test_invalid_scanout() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);

        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();

        assert_eq!(
            gpu.set_scanout(99, 1, &Rect::default()),
            Err(VirtioGpuError::InvalidScanoutId)
        );
    }

    #[test]
    fn test_dispatch_create_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.dispatch_command(GpuCommand::ResourceCreate2d {
            resource_id: 1,
            format: VirtioGpuFormat::R8G8B8A8Unorm,
            width: 320,
            height: 240,
        });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkNodata);
        assert_eq!(gpu.resource_count(), 1);
        let res = gpu.get_resource(1).unwrap();
        assert_eq!(res.width, 320);
        assert_eq!(res.height, 240);
    }

    #[test]
    fn test_dispatch_unref_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        gpu.dispatch_command(GpuCommand::ResourceCreate2d {
            resource_id: 5,
            format: VirtioGpuFormat::B8G8R8A8Unorm,
            width: 64,
            height: 64,
        })
        .unwrap();
        assert_eq!(gpu.resource_count(), 1);

        let result = gpu.dispatch_command(GpuCommand::ResourceUnref { resource_id: 5 });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkNodata);
        assert_eq!(gpu.resource_count(), 0);
    }

    #[test]
    fn test_dispatch_attach_detach_backing() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        gpu.dispatch_command(GpuCommand::ResourceCreate2d {
            resource_id: 1,
            format: VirtioGpuFormat::X8R8G8B8Unorm,
            width: 32,
            height: 32,
        })
        .unwrap();

        let result = gpu.dispatch_command(GpuCommand::ResourceAttachBacking { resource_id: 1 });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkNodata);
        assert!(gpu.get_resource(1).unwrap().backing_attached);

        let result = gpu.dispatch_command(GpuCommand::ResourceDetachBacking { resource_id: 1 });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkNodata);
        assert!(!gpu.get_resource(1).unwrap().backing_attached);
    }

    #[test]
    fn test_dispatch_get_display_info() {
        let mut gpu = VirtioGpu::new("gpu0", 1920, 1080);
        let result = gpu.dispatch_command(GpuCommand::GetDisplayInfo);
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkDisplayInfo);
    }

    #[test]
    fn test_dispatch_invalid_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.dispatch_command(GpuCommand::ResourceUnref { resource_id: 999 });
        assert_eq!(result, Err(VirtioGpuError::InvalidResourceId));
    }

    #[test]
    fn test_dispatch_get_capset_info() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        assert_eq!(gpu.num_capsets(), 2);
        let result = gpu.dispatch_command(GpuCommand::GetCapsetInfo { capset_index: 0 });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkCapsetInfo);
        let result = gpu.dispatch_command(GpuCommand::GetCapsetInfo { capset_index: 99 });
        assert_eq!(result, Err(VirtioGpuError::InvalidParameter));
    }

    #[test]
    fn test_dispatch_get_capset() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.dispatch_command(GpuCommand::GetCapset {
            capset_id: 1,
            capset_version: 1,
        });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkCapset);
        let result = gpu.dispatch_command(GpuCommand::GetCapset {
            capset_id: 999,
            capset_version: 1,
        });
        assert_eq!(result, Err(VirtioGpuError::InvalidParameter));
    }

    #[test]
    fn test_dispatch_get_edid() {
        let mut gpu = VirtioGpu::new("gpu0", 1920, 1080);
        let result = gpu.dispatch_command(GpuCommand::GetEdid { scanout_id: 0 });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkEdid);
        // EDID checksum should be valid
        let edid = gpu.get_edid(0).unwrap();
        assert_eq!(edid.len(), 128);
        let checksum: u8 = edid.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        assert_eq!(checksum, 0);
    }

    #[test]
    fn test_dispatch_get_edid_invalid_scanout() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.dispatch_command(GpuCommand::GetEdid { scanout_id: 99 });
        assert_eq!(result, Err(VirtioGpuError::InvalidScanoutId));
    }

    #[test]
    fn test_process_command_capset() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.process_command(VirtioGpuCtrlType::CmdGetCapsetInfo);
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkCapsetInfo);
        let result = gpu.process_command(VirtioGpuCtrlType::CmdGetCapset);
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkCapset);
        let result = gpu.process_command(VirtioGpuCtrlType::CmdGetEdid);
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkEdid);
    }

    #[test]
    fn test_dispatch_update_cursor() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 32, 32)
            .unwrap();

        let result = gpu.dispatch_command(GpuCommand::UpdateCursor {
            scanout_id: 0,
            x: 100,
            y: 200,
            resource_id: 1,
            hot_x: 16,
            hot_y: 16,
        });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkNodata);

        let cursor = gpu.cursor();
        assert!(cursor.visible);
        assert_eq!(cursor.x, 100);
        assert_eq!(cursor.y, 200);
        assert_eq!(cursor.resource_id, 1);
        assert_eq!(cursor.hot_x, 16);
    }

    #[test]
    fn test_dispatch_move_cursor() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 32, 32)
            .unwrap();
        gpu.update_cursor(0, 0, 0, 1, 0, 0).unwrap();

        let result = gpu.dispatch_command(GpuCommand::MoveCursor {
            scanout_id: 0,
            x: 50,
            y: 75,
        });
        assert_eq!(result.unwrap(), VirtioGpuCtrlType::RespOkNodata);

        let cursor = gpu.cursor();
        assert_eq!(cursor.x, 50);
        assert_eq!(cursor.y, 75);
    }

    #[test]
    fn test_cursor_hide_with_zero_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        gpu.create_resource_2d(1, VirtioGpuFormat::X8R8G8B8Unorm, 32, 32)
            .unwrap();
        gpu.update_cursor(0, 10, 20, 1, 0, 0).unwrap();
        assert!(gpu.cursor().visible);

        // Resource 0 hides cursor
        gpu.update_cursor(0, 0, 0, 0, 0, 0).unwrap();
        assert!(!gpu.cursor().visible);
    }

    #[test]
    fn test_cursor_invalid_scanout() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.update_cursor(99, 0, 0, 0, 0, 0);
        assert_eq!(result, Err(VirtioGpuError::InvalidScanoutId));

        let result = gpu.move_cursor(99, 0, 0);
        assert_eq!(result, Err(VirtioGpuError::InvalidScanoutId));
    }

    #[test]
    fn test_cursor_invalid_resource() {
        let mut gpu = VirtioGpu::new("gpu0", 640, 480);
        let result = gpu.update_cursor(0, 0, 0, 999, 0, 0);
        assert_eq!(result, Err(VirtioGpuError::InvalidResourceId));
    }
}
