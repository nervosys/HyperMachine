//! Boot and initialization for Type-1 hypervisor
//!
//! This module handles the bare-metal boot process:
//! - Bootloader handoff
//! - Early console initialization
//! - Memory map processing
//! - CPU initialization

use crate::{Error, Result};

/// Boot information passed from bootloader
#[derive(Debug)]
pub struct BootInfo {
    /// Physical memory map
    pub memory_map: MemoryMap,
    /// Framebuffer information (if available)
    pub framebuffer: Option<FramebufferInfo>,
    /// RSDP address for ACPI
    pub rsdp_addr: Option<u64>,
    /// Kernel physical address
    pub kernel_addr: u64,
    /// Kernel size in bytes
    pub kernel_size: u64,
}

/// Memory region types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Usable memory
    Usable,
    /// Reserved memory
    Reserved,
    /// ACPI reclaimable
    AcpiReclaimable,
    /// ACPI NVS
    AcpiNvs,
    /// Bad memory
    BadMemory,
    /// Bootloader reclaimable
    BootloaderReclaimable,
    /// Kernel and modules
    KernelAndModules,
    /// Framebuffer
    Framebuffer,
}

/// A memory region in the memory map
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    /// Start physical address
    pub start: u64,
    /// Size in bytes
    pub size: u64,
    /// Region type
    pub kind: MemoryRegionType,
}

/// Physical memory map
#[derive(Debug)]
pub struct MemoryMap {
    /// Memory regions
    regions: [Option<MemoryRegion>; 64],
    /// Number of valid regions
    count: usize,
}

impl MemoryMap {
    /// Create a new empty memory map
    pub const fn new() -> Self {
        Self {
            regions: [None; 64],
            count: 0,
        }
    }

    /// Add a memory region
    pub fn add_region(&mut self, region: MemoryRegion) -> Result<()> {
        if self.count >= 64 {
            return Err(Error::OutOfMemory);
        }
        self.regions[self.count] = Some(region);
        self.count += 1;
        Ok(())
    }

    /// Get iterator over memory regions
    pub fn iter(&self) -> impl Iterator<Item = &MemoryRegion> {
        self.regions[..self.count].iter().filter_map(|r| r.as_ref())
    }

    /// Get total usable memory
    pub fn total_usable_memory(&self) -> u64 {
        self.iter()
            .filter(|r| r.kind == MemoryRegionType::Usable)
            .map(|r| r.size)
            .sum()
    }

    /// Find a region containing the given address
    pub fn find_region(&self, addr: u64) -> Option<&MemoryRegion> {
        self.iter().find(|r| addr >= r.start && addr < r.start + r.size)
    }
}

impl Default for MemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Framebuffer information
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// Physical address of framebuffer
    pub address: u64,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pitch (bytes per row)
    pub pitch: u32,
    /// Bits per pixel
    pub bpp: u8,
    /// Pixel format
    pub format: PixelFormat,
}

/// Pixel format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGB with 8 bits per channel
    Rgb,
    /// BGR with 8 bits per channel
    Bgr,
    /// Unknown format
    Unknown,
}

/// Early initialization before memory allocator is available
pub fn early_init() -> Result<()> {
    // Disable interrupts during early init
    crate::arch::cli();
    
    Ok(())
}

/// Late initialization after memory allocator is available
pub fn late_init(_boot_info: &BootInfo) -> Result<()> {
    // Initialize ACPI if RSDP is available
    // Initialize interrupt handling
    // Set up paging
    
    Ok(())
}

/// Kernel entry point (called from bootloader)
/// 
/// # Safety
/// This function must only be called once at boot time.
#[cfg(feature = "bootloader_api")]
pub unsafe fn kernel_main(boot_info: &'static bootloader_api::BootInfo) -> ! {
    // Convert bootloader_api boot info to our format
    let _info = convert_boot_info(boot_info);
    
    // Early initialization
    early_init().expect("Early init failed");
    
    // Initialize the hypervisor
    crate::initialize().expect("Hypervisor init failed");
    
    // Enter hypervisor main loop
    loop {
        crate::arch::hlt();
    }
}

/// Convert bootloader_api BootInfo to our format
#[cfg(feature = "bootloader_api")]
fn convert_boot_info(boot_info: &bootloader_api::BootInfo) -> BootInfo {
    let mut memory_map = MemoryMap::new();
    
    // Convert memory regions
    for region in boot_info.memory_regions.iter() {
        let kind = match region.kind {
            bootloader_api::info::MemoryRegionKind::Usable => MemoryRegionType::Usable,
            bootloader_api::info::MemoryRegionKind::Bootloader => MemoryRegionType::BootloaderReclaimable,
            _ => MemoryRegionType::Reserved,
        };
        
        let _ = memory_map.add_region(MemoryRegion {
            start: region.start,
            size: region.end - region.start,
            kind,
        });
    }
    
    // Convert framebuffer info
    let framebuffer = boot_info.framebuffer.as_ref().map(|fb| {
        FramebufferInfo {
            address: fb.buffer().as_ptr() as u64,
            width: fb.info().width as u32,
            height: fb.info().height as u32,
            pitch: fb.info().stride as u32 * fb.info().bytes_per_pixel as u32,
            bpp: (fb.info().bytes_per_pixel * 8) as u8,
            format: match fb.info().pixel_format {
                bootloader_api::info::PixelFormat::Rgb => PixelFormat::Rgb,
                bootloader_api::info::PixelFormat::Bgr => PixelFormat::Bgr,
                _ => PixelFormat::Unknown,
            },
        }
    });
    
    BootInfo {
        memory_map,
        framebuffer,
        rsdp_addr: boot_info.rsdp_addr.map(|a| a.into()),
        kernel_addr: 0, // TODO: Get from boot info
        kernel_size: 0, // TODO: Get from boot info
    }
}
