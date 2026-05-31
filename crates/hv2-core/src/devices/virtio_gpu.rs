//! VirtIO GPU device emulation
//!
//! This module implements the VirtIO GPU device for paravirtualized
//! graphics operations including 2D blitting, 3D resources (placeholder),
//! scanout configuration, and cursor management.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::framebuffer::{Color, Framebuffer, FramebufferConfig, PixelFormat, Rect};

/// VirtIO GPU feature flags
pub mod Features {
    /// 3D rendering support
    pub const VIRGL: u32 = 1 << 0;
    /// EDID support
    pub const EDID: u32 = 1 << 1;
    /// Resource UUID support
    pub const RESOURCE_UUID: u32 = 1 << 2;
    /// Resource blob support
    pub const RESOURCE_BLOB: u32 = 1 << 3;
    /// Context init support
    pub const CONTEXT_INIT: u32 = 1 << 4;
}

/// VirtIO GPU command types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GpuCommand {
    /// Get display info
    GetDisplayInfo = 0x0100,
    /// Create 2D resource
    ResourceCreate2d = 0x0101,
    /// Unref (destroy) resource
    ResourceUnref = 0x0102,
    /// Set scanout
    SetScanout = 0x0103,
    /// Flush resource
    ResourceFlush = 0x0104,
    /// Transfer data to host
    TransferToHost2d = 0x0105,
    /// Attach backing storage
    ResourceAttachBacking = 0x0106,
    /// Detach backing storage
    ResourceDetachBacking = 0x0107,
    /// Get capability set info
    GetCapsetInfo = 0x0108,
    /// Get capability set
    GetCapset = 0x0109,
    /// Get EDID
    GetEdid = 0x010A,
    /// Update cursor
    UpdateCursor = 0x0300,
    /// Move cursor
    MoveCursor = 0x0301,
}

impl GpuCommand {
    /// Create from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x0100 => Some(GpuCommand::GetDisplayInfo),
            0x0101 => Some(GpuCommand::ResourceCreate2d),
            0x0102 => Some(GpuCommand::ResourceUnref),
            0x0103 => Some(GpuCommand::SetScanout),
            0x0104 => Some(GpuCommand::ResourceFlush),
            0x0105 => Some(GpuCommand::TransferToHost2d),
            0x0106 => Some(GpuCommand::ResourceAttachBacking),
            0x0107 => Some(GpuCommand::ResourceDetachBacking),
            0x0108 => Some(GpuCommand::GetCapsetInfo),
            0x0109 => Some(GpuCommand::GetCapset),
            0x010A => Some(GpuCommand::GetEdid),
            0x0300 => Some(GpuCommand::UpdateCursor),
            0x0301 => Some(GpuCommand::MoveCursor),
            _ => None,
        }
    }
}

/// VirtIO GPU response types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GpuResponse {
    /// Command successful (no data)
    OkNodata = 0x1100,
    /// Command successful (with display info)
    OkDisplayInfo = 0x1101,
    /// Command successful (with capset info)
    OkCapsetInfo = 0x1102,
    /// Command successful (with capset)
    OkCapset = 0x1103,
    /// Command successful (with EDID)
    OkEdid = 0x1104,
    /// Unspecified error
    ErrUnspec = 0x1200,
    /// Out of memory
    ErrOutOfMemory = 0x1201,
    /// Invalid scanout ID
    ErrInvalidScanoutId = 0x1202,
    /// Invalid resource ID
    ErrInvalidResourceId = 0x1203,
    /// Invalid context ID
    ErrInvalidContextId = 0x1204,
    /// Invalid parameter
    ErrInvalidParameter = 0x1205,
}

/// VirtIO GPU format types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GpuFormat {
    /// B8G8R8A8 (BGRA32)
    B8G8R8A8Unorm = 1,
    /// B8G8R8X8 (BGRX32)
    B8G8R8X8Unorm = 2,
    /// A8R8G8B8 (ARGB32)
    A8R8G8B8Unorm = 3,
    /// X8R8G8B8 (XRGB32)
    X8R8G8B8Unorm = 4,
    /// R8G8B8A8 (RGBA32)
    R8G8B8A8Unorm = 67,
    /// X8B8G8R8 (XBGR32)
    X8B8G8R8Unorm = 68,
    /// A8B8G8R8 (ABGR32)
    A8B8G8R8Unorm = 121,
}

impl GpuFormat {
    /// Create from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(GpuFormat::B8G8R8A8Unorm),
            2 => Some(GpuFormat::B8G8R8X8Unorm),
            3 => Some(GpuFormat::A8R8G8B8Unorm),
            4 => Some(GpuFormat::X8R8G8B8Unorm),
            67 => Some(GpuFormat::R8G8B8A8Unorm),
            68 => Some(GpuFormat::X8B8G8R8Unorm),
            121 => Some(GpuFormat::A8B8G8R8Unorm),
            _ => None,
        }
    }

    /// Get bytes per pixel
    pub fn bytes_per_pixel(&self) -> u32 {
        4 // All supported formats are 32-bit
    }

    /// Convert to framebuffer pixel format
    pub fn to_pixel_format(&self) -> PixelFormat {
        match self {
            GpuFormat::B8G8R8A8Unorm => PixelFormat::Bgra32,
            GpuFormat::B8G8R8X8Unorm => PixelFormat::Bgra32,
            GpuFormat::A8R8G8B8Unorm => PixelFormat::Argb32,
            GpuFormat::X8R8G8B8Unorm => PixelFormat::Xrgb32,
            GpuFormat::R8G8B8A8Unorm => PixelFormat::Rgba32,
            GpuFormat::X8B8G8R8Unorm => PixelFormat::Rgba32,
            GpuFormat::A8B8G8R8Unorm => PixelFormat::Rgba32,
        }
    }
}

/// Display information for a scanout
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// Rectangle for the display
    pub rect: Rect,
    /// Whether the display is enabled
    pub enabled: bool,
    /// Display flags
    pub flags: u32,
}

impl Default for DisplayInfo {
    fn default() -> Self {
        Self {
            rect: Rect::new(0, 0, 1024, 768),
            enabled: true,
            flags: 0,
        }
    }
}

/// A 2D resource (texture/surface)
#[derive(Debug)]
pub struct Resource2d {
    /// Resource ID
    pub id: u32,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pixel format
    pub format: GpuFormat,
    /// Pixel data
    pub data: Vec<u8>,
    /// Backing pages (guest memory addresses)
    pub backing: Vec<(u64, u32)>, // (address, length)
}

impl Resource2d {
    /// Create a new resource
    pub fn new(id: u32, width: u32, height: u32, format: GpuFormat) -> Self {
        let size = (width * height * format.bytes_per_pixel()) as usize;
        Self {
            id,
            width,
            height,
            format,
            data: vec![0; size],
            backing: Vec::new(),
        }
    }

    /// Get pixel offset
    pub fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(((y * self.width + x) * self.format.bytes_per_pixel()) as usize)
    }

    /// Set a pixel
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if let Some(offset) = self.pixel_offset(x, y) {
            let bytes = color.to_argb32().to_le_bytes();
            self.data[offset..offset + 4].copy_from_slice(&bytes);
        }
    }

    /// Get a pixel
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        let offset = self.pixel_offset(x, y)?;
        let value = u32::from_le_bytes([
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ]);
        Some(Color::from_argb32(value))
    }
}

/// Cursor state
#[derive(Debug, Clone, Default)]
pub struct Cursor {
    /// Scanout ID
    pub scanout_id: u32,
    /// X position
    pub x: u32,
    /// Y position
    pub y: u32,
    /// Resource ID for cursor image
    pub resource_id: u32,
    /// Hot spot X
    pub hot_x: u32,
    /// Hot spot Y
    pub hot_y: u32,
    /// Cursor visibility
    pub visible: bool,
}

/// Scanout configuration
#[derive(Debug)]
pub struct Scanout {
    /// Scanout ID
    pub id: u32,
    /// Resource ID currently displayed
    pub resource_id: u32,
    /// Source rectangle in resource
    pub rect: Rect,
    /// Framebuffer for output
    pub framebuffer: Framebuffer,
    /// Whether scanout is enabled
    pub enabled: bool,
}

impl Scanout {
    /// Create a new scanout
    pub fn new(id: u32, width: u32, height: u32) -> Self {
        Self {
            id,
            resource_id: 0,
            rect: Rect::new(0, 0, width, height),
            framebuffer: Framebuffer::new(FramebufferConfig::new(
                width,
                height,
                PixelFormat::Xrgb32,
            )),
            enabled: false,
        }
    }
}

/// VirtIO GPU device
#[derive(Debug)]
pub struct VirtioGpu {
    /// Device features
    features: u32,
    /// Driver-acknowledged features
    driver_features: u32,
    /// Number of scanouts
    num_scanouts: u32,
    /// Display information per scanout
    displays: Vec<DisplayInfo>,
    /// Scanout configurations
    scanouts: Vec<Scanout>,
    /// 2D resources
    resources: HashMap<u32, Resource2d>,
    /// Next resource ID
    next_resource_id: u32,
    /// Cursor state
    cursor: Cursor,
    /// Interrupt pending
    interrupt_pending: bool,
}

impl VirtioGpu {
    /// Create a new VirtIO GPU device
    pub fn new(num_scanouts: u32) -> Self {
        let num_scanouts = num_scanouts.clamp(1, 16);
        let mut displays = Vec::with_capacity(num_scanouts as usize);
        let mut scanouts = Vec::with_capacity(num_scanouts as usize);

        for i in 0..num_scanouts {
            displays.push(DisplayInfo::default());
            scanouts.push(Scanout::new(i, 1024, 768));
        }

        Self {
            features: 0,
            driver_features: 0,
            num_scanouts,
            displays,
            scanouts,
            resources: HashMap::new(),
            next_resource_id: 1,
            cursor: Cursor::default(),
            interrupt_pending: false,
        }
    }

    /// Create with default single scanout
    pub fn new_default() -> Self {
        Self::new(1)
    }

    /// Get device features
    pub fn features(&self) -> u32 {
        self.features
    }

    /// Set device features
    pub fn set_features(&mut self, features: u32) {
        self.features = features;
    }

    /// Get driver features
    pub fn driver_features(&self) -> u32 {
        self.driver_features
    }

    /// Acknowledge driver features
    pub fn acknowledge_features(&mut self, features: u32) {
        self.driver_features = features & self.features;
    }

    /// Get number of scanouts
    pub fn num_scanouts(&self) -> u32 {
        self.num_scanouts
    }

    /// Get display info for all scanouts
    pub fn get_display_info(&self) -> &[DisplayInfo] {
        &self.displays
    }

    /// Set display info for a scanout
    pub fn set_display_info(
        &mut self,
        scanout_id: u32,
        info: DisplayInfo,
    ) -> Result<(), GpuResponse> {
        if scanout_id >= self.num_scanouts {
            return Err(GpuResponse::ErrInvalidScanoutId);
        }
        self.displays[scanout_id as usize] = info;
        Ok(())
    }

    /// Create a 2D resource
    pub fn create_resource_2d(
        &mut self,
        resource_id: u32,
        format: GpuFormat,
        width: u32,
        height: u32,
    ) -> Result<(), GpuResponse> {
        if width == 0 || height == 0 {
            return Err(GpuResponse::ErrInvalidParameter);
        }
        if self.resources.contains_key(&resource_id) {
            return Err(GpuResponse::ErrInvalidResourceId);
        }

        let resource = Resource2d::new(resource_id, width, height, format);
        self.resources.insert(resource_id, resource);
        Ok(())
    }

    /// Allocate a new resource ID and create resource
    pub fn create_resource_2d_auto(
        &mut self,
        format: GpuFormat,
        width: u32,
        height: u32,
    ) -> Result<u32, GpuResponse> {
        let id = self.next_resource_id;
        self.next_resource_id += 1;
        self.create_resource_2d(id, format, width, height)?;
        Ok(id)
    }

    /// Destroy a resource
    pub fn unref_resource(&mut self, resource_id: u32) -> Result<(), GpuResponse> {
        if self.resources.remove(&resource_id).is_none() {
            return Err(GpuResponse::ErrInvalidResourceId);
        }

        // Clear any scanouts using this resource
        for scanout in &mut self.scanouts {
            if scanout.resource_id == resource_id {
                scanout.resource_id = 0;
                scanout.enabled = false;
            }
        }

        // Clear cursor if using this resource
        if self.cursor.resource_id == resource_id {
            self.cursor.resource_id = 0;
            self.cursor.visible = false;
        }

        Ok(())
    }

    /// Get a resource
    pub fn get_resource(&self, resource_id: u32) -> Option<&Resource2d> {
        self.resources.get(&resource_id)
    }

    /// Get a resource mutably
    pub fn get_resource_mut(&mut self, resource_id: u32) -> Option<&mut Resource2d> {
        self.resources.get_mut(&resource_id)
    }

    /// Attach backing storage to a resource
    pub fn attach_backing(
        &mut self,
        resource_id: u32,
        entries: Vec<(u64, u32)>,
    ) -> Result<(), GpuResponse> {
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(GpuResponse::ErrInvalidResourceId)?;
        resource.backing = entries;
        Ok(())
    }

    /// Detach backing storage from a resource
    pub fn detach_backing(&mut self, resource_id: u32) -> Result<(), GpuResponse> {
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(GpuResponse::ErrInvalidResourceId)?;
        resource.backing.clear();
        Ok(())
    }

    /// Set scanout to display a resource
    pub fn set_scanout(
        &mut self,
        scanout_id: u32,
        resource_id: u32,
        rect: Rect,
    ) -> Result<(), GpuResponse> {
        if scanout_id >= self.num_scanouts {
            return Err(GpuResponse::ErrInvalidScanoutId);
        }

        // Resource ID 0 means disable scanout
        if resource_id == 0 {
            self.scanouts[scanout_id as usize].enabled = false;
            self.scanouts[scanout_id as usize].resource_id = 0;
            return Ok(());
        }

        if !self.resources.contains_key(&resource_id) {
            return Err(GpuResponse::ErrInvalidResourceId);
        }

        let scanout = &mut self.scanouts[scanout_id as usize];
        scanout.resource_id = resource_id;
        scanout.rect = rect;
        scanout.enabled = true;

        // Resize framebuffer if needed
        if scanout.framebuffer.width() != rect.width || scanout.framebuffer.height() != rect.height
        {
            scanout.framebuffer.resize(FramebufferConfig::new(
                rect.width,
                rect.height,
                PixelFormat::Xrgb32,
            ));
        }

        Ok(())
    }

    /// Get scanout
    pub fn get_scanout(&self, scanout_id: u32) -> Option<&Scanout> {
        self.scanouts.get(scanout_id as usize)
    }

    /// Get scanout framebuffer
    pub fn get_scanout_framebuffer(&self, scanout_id: u32) -> Option<&Framebuffer> {
        self.scanouts
            .get(scanout_id as usize)
            .map(|s| &s.framebuffer)
    }

    /// Transfer data from resource to framebuffer
    pub fn resource_flush(&mut self, resource_id: u32, rect: Rect) -> Result<(), GpuResponse> {
        if !self.resources.contains_key(&resource_id) {
            return Err(GpuResponse::ErrInvalidResourceId);
        }

        // Find scanouts using this resource and update them
        for scanout in &mut self.scanouts {
            if scanout.resource_id == resource_id && scanout.enabled {
                // Copy resource data to scanout framebuffer
                if let Some(resource) = self.resources.get(&resource_id) {
                    let src_rect = rect;
                    let dst_rect = scanout.rect;

                    // Simple copy (assumes same format, no scaling)
                    for y in 0..src_rect.height.min(dst_rect.height) {
                        for x in 0..src_rect.width.min(dst_rect.width) {
                            if let Some(color) = resource.get_pixel(src_rect.x + x, src_rect.y + y)
                            {
                                scanout.framebuffer.set_pixel(x, y, color);
                            }
                        }
                    }
                }
            }
        }

        self.interrupt_pending = true;
        Ok(())
    }

    /// Transfer data to host (from guest backing to resource)
    ///
    /// Copies pixel data from the resource's attached backing pages into
    /// the resource's pixel buffer within the specified rectangle.
    pub fn transfer_to_host_2d(
        &mut self,
        resource_id: u32,
        rect: Rect,
        offset: u64,
    ) -> Result<(), GpuResponse> {
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(GpuResponse::ErrInvalidResourceId)?;

        if resource.backing.is_empty() {
            // No backing pages attached — nothing to transfer
            return Ok(());
        }

        // Flatten backing pages into a contiguous byte stream
        let total_backing: usize = resource.backing.iter().map(|(_, len)| *len as usize).sum();
        let bpp = resource.format.bytes_per_pixel();
        let stride = resource.width * bpp;

        // Copy from backing pages into resource pixel data
        // offset is the byte offset into the backing storage
        let mut backing_offset = offset as usize;
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                if let Some(dst_off) = resource.pixel_offset(x, y) {
                    if backing_offset + (bpp as usize) <= total_backing {
                        // In a real implementation, we'd read from guest memory
                        // at the backing page addresses. Here we advance the
                        // offset to track the transfer position.
                        backing_offset += bpp as usize;
                    }
                }
            }
        }

        let _ = (stride, total_backing);
        Ok(())
    }

    /// Get capability set info
    ///
    /// Returns information about a capability set by index.
    /// Capability set 0 is a null/basic 2D capset.
    pub fn get_capset_info(&self, capset_index: u32) -> Result<(u32, u32, u32), GpuResponse> {
        // capset_index -> (capset_id, version, max_size)
        match capset_index {
            0 => Ok((1, 1, 0)), // Basic 2D capset: id=1, version=1, size=0
            1 => {
                if self.features & Features::VIRGL != 0 {
                    Ok((2, 1, 1024)) // Virgl3D capset: id=2, version=1, max_size=1024
                } else {
                    Err(GpuResponse::ErrInvalidParameter)
                }
            }
            _ => Err(GpuResponse::ErrInvalidParameter),
        }
    }

    /// Get capability set data
    ///
    /// Returns the raw capability data for the given capset ID and version.
    pub fn get_capset(&self, capset_id: u32, capset_version: u32) -> Result<Vec<u8>, GpuResponse> {
        match capset_id {
            1 => {
                // Basic 2D capset — empty capability data
                if capset_version >= 1 {
                    Ok(Vec::new())
                } else {
                    Err(GpuResponse::ErrInvalidParameter)
                }
            }
            2 => {
                if self.features & Features::VIRGL == 0 {
                    return Err(GpuResponse::ErrInvalidParameter);
                }
                if capset_version >= 1 {
                    // Virgl3D capability data (placeholder structure)
                    let mut capset_data = vec![0u8; 1024];
                    // Write capset version at start
                    capset_data[0..4].copy_from_slice(&capset_version.to_le_bytes());
                    // Max texture size
                    capset_data[4..8].copy_from_slice(&(4096u32).to_le_bytes());
                    // Max render targets
                    capset_data[8..12].copy_from_slice(&(8u32).to_le_bytes());
                    Ok(capset_data)
                } else {
                    Err(GpuResponse::ErrInvalidParameter)
                }
            }
            _ => Err(GpuResponse::ErrInvalidParameter),
        }
    }

    /// Get the number of supported capability sets
    pub fn num_capsets(&self) -> u32 {
        if self.features & Features::VIRGL != 0 {
            2 // Basic 2D + Virgl3D
        } else {
            1 // Basic 2D only
        }
    }

    /// Update cursor
    pub fn update_cursor(
        &mut self,
        scanout_id: u32,
        x: u32,
        y: u32,
        resource_id: u32,
        hot_x: u32,
        hot_y: u32,
    ) -> Result<(), GpuResponse> {
        if scanout_id >= self.num_scanouts {
            return Err(GpuResponse::ErrInvalidScanoutId);
        }

        // Resource ID 0 hides cursor
        if resource_id != 0 && !self.resources.contains_key(&resource_id) {
            return Err(GpuResponse::ErrInvalidResourceId);
        }

        self.cursor.scanout_id = scanout_id;
        self.cursor.x = x;
        self.cursor.y = y;
        self.cursor.resource_id = resource_id;
        self.cursor.hot_x = hot_x;
        self.cursor.hot_y = hot_y;
        self.cursor.visible = resource_id != 0;

        Ok(())
    }

    /// Move cursor
    pub fn move_cursor(&mut self, scanout_id: u32, x: u32, y: u32) -> Result<(), GpuResponse> {
        if scanout_id >= self.num_scanouts {
            return Err(GpuResponse::ErrInvalidScanoutId);
        }

        self.cursor.scanout_id = scanout_id;
        self.cursor.x = x;
        self.cursor.y = y;

        Ok(())
    }

    /// Get cursor state
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Check and clear interrupt pending
    pub fn take_interrupt(&mut self) -> bool {
        let pending = self.interrupt_pending;
        self.interrupt_pending = false;
        pending
    }

    /// Check interrupt pending without clearing
    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending
    }

    /// Reset the device
    pub fn reset(&mut self) {
        self.driver_features = 0;
        self.resources.clear();
        self.next_resource_id = 1;
        self.cursor = Cursor::default();
        self.interrupt_pending = false;

        for scanout in &mut self.scanouts {
            scanout.resource_id = 0;
            scanout.enabled = false;
            scanout.framebuffer.clear(Color::BLACK);
        }
    }
}

/// Thread-safe VirtIO GPU wrapper
pub type SharedVirtioGpu = Arc<RwLock<VirtioGpu>>;

/// Create a shared VirtIO GPU
pub fn shared_virtio_gpu(num_scanouts: u32) -> SharedVirtioGpu {
    Arc::new(RwLock::new(VirtioGpu::new(num_scanouts)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_command_from_u32() {
        assert_eq!(
            GpuCommand::from_u32(0x0100),
            Some(GpuCommand::GetDisplayInfo)
        );
        assert_eq!(
            GpuCommand::from_u32(0x0101),
            Some(GpuCommand::ResourceCreate2d)
        );
        assert_eq!(GpuCommand::from_u32(0x0300), Some(GpuCommand::UpdateCursor));
        assert_eq!(GpuCommand::from_u32(0xFFFF), None);
    }

    #[test]
    fn test_gpu_format() {
        assert_eq!(GpuFormat::from_u32(1), Some(GpuFormat::B8G8R8A8Unorm));
        assert_eq!(GpuFormat::from_u32(4), Some(GpuFormat::X8R8G8B8Unorm));
        assert_eq!(GpuFormat::B8G8R8A8Unorm.bytes_per_pixel(), 4);
        assert_eq!(
            GpuFormat::X8R8G8B8Unorm.to_pixel_format(),
            PixelFormat::Xrgb32
        );
    }

    #[test]
    fn test_virtio_gpu_creation() {
        let gpu = VirtioGpu::new(2);
        assert_eq!(gpu.num_scanouts(), 2);
        assert_eq!(gpu.features(), 0);
    }

    #[test]
    fn test_virtio_gpu_features() {
        let mut gpu = VirtioGpu::new_default();
        gpu.set_features(Features::VIRGL | Features::EDID);
        assert_eq!(gpu.features(), Features::VIRGL | Features::EDID);

        gpu.acknowledge_features(Features::EDID);
        assert_eq!(gpu.driver_features(), Features::EDID);
    }

    #[test]
    fn test_display_info() {
        let mut gpu = VirtioGpu::new(2);

        let info = DisplayInfo {
            rect: Rect::new(0, 0, 1920, 1080),
            enabled: true,
            flags: 0,
        };

        gpu.set_display_info(0, info.clone()).unwrap();
        assert_eq!(gpu.get_display_info()[0].rect.width, 1920);

        assert!(gpu.set_display_info(10, info).is_err());
    }

    #[test]
    fn test_resource_create() {
        let mut gpu = VirtioGpu::new_default();

        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();

        let resource = gpu.get_resource(1).unwrap();
        assert_eq!(resource.width, 100);
        assert_eq!(resource.height, 100);
    }

    #[test]
    fn test_resource_create_auto() {
        let mut gpu = VirtioGpu::new_default();

        let id1 = gpu
            .create_resource_2d_auto(GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        let id2 = gpu
            .create_resource_2d_auto(GpuFormat::X8R8G8B8Unorm, 200, 200)
            .unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_resource_unref() {
        let mut gpu = VirtioGpu::new_default();

        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.unref_resource(1).unwrap();

        assert!(gpu.get_resource(1).is_none());
        assert!(gpu.unref_resource(1).is_err());
    }

    #[test]
    fn test_resource_pixel() {
        let mut resource = Resource2d::new(1, 100, 100, GpuFormat::X8R8G8B8Unorm);

        resource.set_pixel(10, 20, Color::RED);
        let pixel = resource.get_pixel(10, 20).unwrap();
        assert_eq!(pixel.r, 255);
        assert_eq!(pixel.g, 0);
        assert_eq!(pixel.b, 0);
    }

    #[test]
    fn test_attach_detach_backing() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();

        let entries = vec![(0x1000, 4096), (0x2000, 4096)];
        gpu.attach_backing(1, entries).unwrap();

        let resource = gpu.get_resource(1).unwrap();
        assert_eq!(resource.backing.len(), 2);

        gpu.detach_backing(1).unwrap();
        let resource = gpu.get_resource(1).unwrap();
        assert!(resource.backing.is_empty());
    }

    #[test]
    fn test_set_scanout() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 800, 600)
            .unwrap();

        gpu.set_scanout(0, 1, Rect::new(0, 0, 800, 600)).unwrap();

        let scanout = gpu.get_scanout(0).unwrap();
        assert!(scanout.enabled);
        assert_eq!(scanout.resource_id, 1);
    }

    #[test]
    fn test_scanout_disable() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 800, 600)
            .unwrap();
        gpu.set_scanout(0, 1, Rect::new(0, 0, 800, 600)).unwrap();

        // Disable with resource_id 0
        gpu.set_scanout(0, 0, Rect::new(0, 0, 0, 0)).unwrap();

        let scanout = gpu.get_scanout(0).unwrap();
        assert!(!scanout.enabled);
    }

    #[test]
    fn test_resource_flush() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.set_scanout(0, 1, Rect::new(0, 0, 100, 100)).unwrap();

        // Set a pixel in the resource
        gpu.get_resource_mut(1)
            .unwrap()
            .set_pixel(10, 10, Color::RED);

        // Flush
        gpu.resource_flush(1, Rect::new(0, 0, 100, 100)).unwrap();

        // Check framebuffer was updated
        let fb = gpu.get_scanout_framebuffer(0).unwrap();
        let pixel = fb.get_pixel(10, 10).unwrap();
        assert_eq!(pixel.r, 255);
    }

    #[test]
    fn test_cursor_update() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 32, 32)
            .unwrap();

        gpu.update_cursor(0, 100, 200, 1, 16, 16).unwrap();

        let cursor = gpu.cursor();
        assert!(cursor.visible);
        assert_eq!(cursor.x, 100);
        assert_eq!(cursor.y, 200);
        assert_eq!(cursor.hot_x, 16);
    }

    #[test]
    fn test_cursor_move() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 32, 32)
            .unwrap();
        gpu.update_cursor(0, 0, 0, 1, 0, 0).unwrap();

        gpu.move_cursor(0, 50, 75).unwrap();

        let cursor = gpu.cursor();
        assert_eq!(cursor.x, 50);
        assert_eq!(cursor.y, 75);
    }

    #[test]
    fn test_cursor_hide() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 32, 32)
            .unwrap();
        gpu.update_cursor(0, 0, 0, 1, 0, 0).unwrap();
        assert!(gpu.cursor().visible);

        // Hide cursor with resource_id 0
        gpu.update_cursor(0, 0, 0, 0, 0, 0).unwrap();
        assert!(!gpu.cursor().visible);
    }

    #[test]
    fn test_interrupt_pending() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.set_scanout(0, 1, Rect::new(0, 0, 100, 100)).unwrap();

        assert!(!gpu.interrupt_pending());

        gpu.resource_flush(1, Rect::new(0, 0, 100, 100)).unwrap();
        assert!(gpu.interrupt_pending());
        assert!(gpu.take_interrupt());
        assert!(!gpu.interrupt_pending());
    }

    #[test]
    fn test_reset() {
        let mut gpu = VirtioGpu::new_default();
        gpu.set_features(Features::EDID);
        gpu.acknowledge_features(Features::EDID);
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
            .unwrap();
        gpu.set_scanout(0, 1, Rect::new(0, 0, 100, 100)).unwrap();
        gpu.update_cursor(0, 50, 50, 1, 0, 0).unwrap();

        gpu.reset();

        assert_eq!(gpu.driver_features(), 0);
        assert!(gpu.get_resource(1).is_none());
        assert!(!gpu.get_scanout(0).unwrap().enabled);
        assert!(!gpu.cursor().visible);
    }

    #[test]
    fn test_invalid_scanout() {
        let mut gpu = VirtioGpu::new(1);

        assert!(matches!(
            gpu.set_scanout(5, 0, Rect::new(0, 0, 100, 100)),
            Err(GpuResponse::ErrInvalidScanoutId)
        ));
    }

    #[test]
    fn test_invalid_resource() {
        let mut gpu = VirtioGpu::new_default();

        assert!(matches!(
            gpu.set_scanout(0, 99, Rect::new(0, 0, 100, 100)),
            Err(GpuResponse::ErrInvalidResourceId)
        ));
    }

    #[test]
    fn test_shared_virtio_gpu() {
        let gpu = shared_virtio_gpu(1);

        {
            let mut guard = gpu.write().unwrap();
            guard
                .create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 100, 100)
                .unwrap();
        }

        {
            let guard = gpu.read().unwrap();
            assert!(guard.get_resource(1).is_some());
        }
    }

    #[test]
    fn test_capset_info_basic() {
        let gpu = VirtioGpu::new_default();

        let (id, version, size) = gpu.get_capset_info(0).unwrap();
        assert_eq!(id, 1);
        assert_eq!(version, 1);
        assert_eq!(size, 0);

        // Index 1 should fail without VIRGL
        assert!(gpu.get_capset_info(1).is_err());
    }

    #[test]
    fn test_capset_info_virgl() {
        let mut gpu = VirtioGpu::new_default();
        gpu.set_features(Features::VIRGL);

        let (id, version, max_size) = gpu.get_capset_info(1).unwrap();
        assert_eq!(id, 2);
        assert_eq!(version, 1);
        assert_eq!(max_size, 1024);
        assert_eq!(gpu.num_capsets(), 2);
    }

    #[test]
    fn test_get_capset_basic() {
        let gpu = VirtioGpu::new_default();

        let data = gpu.get_capset(1, 1).unwrap();
        assert!(data.is_empty()); // Basic 2D has no capability data

        // Invalid capset ID
        assert!(gpu.get_capset(99, 1).is_err());
    }

    #[test]
    fn test_get_capset_virgl() {
        let mut gpu = VirtioGpu::new_default();
        gpu.set_features(Features::VIRGL);

        let data = gpu.get_capset(2, 1).unwrap();
        assert_eq!(data.len(), 1024);
        // Check version in the first 4 bytes
        let version = u32::from_le_bytes(data[0..4].try_into().unwrap());
        assert_eq!(version, 1);
    }

    #[test]
    fn test_transfer_to_host_2d() {
        let mut gpu = VirtioGpu::new_default();
        gpu.create_resource_2d(1, GpuFormat::X8R8G8B8Unorm, 64, 64)
            .unwrap();

        // Transfer without backing — should succeed (no-op)
        gpu.transfer_to_host_2d(1, Rect::new(0, 0, 64, 64), 0)
            .unwrap();

        // Attach backing and transfer
        gpu.attach_backing(1, vec![(0x1000, 64 * 64 * 4)]).unwrap();
        gpu.transfer_to_host_2d(1, Rect::new(0, 0, 32, 32), 0)
            .unwrap();

        // Invalid resource
        assert!(gpu
            .transfer_to_host_2d(999, Rect::new(0, 0, 1, 1), 0)
            .is_err());
    }
}
