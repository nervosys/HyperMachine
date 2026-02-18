//! Guest Address Space Management
//!
//! This module provides comprehensive management of the guest physical address space,
//! including RAM regions, MMIO regions, and memory-mapped devices. It handles address
//! translation between guest physical addresses (GPA) and host virtual addresses (HVA).
//!
//! # Architecture
//!
//! ```text
//! Guest Physical Address Space:
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │ 0x0000_0000 - 0x0009_FFFF: Low Memory (640KB conventional)         │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ 0x000A_0000 - 0x000B_FFFF: VGA Memory (128KB)                      │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ 0x000C_0000 - 0x000F_FFFF: ROM/BIOS Area (256KB)                   │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ 0x0010_0000 - 0xXXXX_XXXX: Extended Memory (RAM)                   │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ 0xFEE0_0000 - 0xFEE0_0FFF: Local APIC (4KB)                        │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ 0xFEC0_0000 - 0xFEC0_0FFF: I/O APIC (4KB)                          │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```

use crate::{Error, Result};
use parking_lot::RwLock;
use std::collections::BTreeMap;

/// Guest Physical Address
pub type GuestPhysAddr = u64;

/// Host Virtual Address  
pub type HostVirtAddr = u64;

/// Page size (4KB)
pub const PAGE_SIZE: u64 = 4096;

/// Large page size (2MB)
pub const LARGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;

/// Huge page size (1GB)
pub const HUGE_PAGE_SIZE: u64 = 1024 * 1024 * 1024;

// Standard PC memory layout constants
/// Low memory end (640KB)
pub const LOW_MEMORY_END: u64 = 0x000A_0000;
/// VGA memory start
pub const VGA_MEMORY_START: u64 = 0x000A_0000;
/// VGA memory end  
pub const VGA_MEMORY_END: u64 = 0x000C_0000;
/// ROM/BIOS start
pub const ROM_START: u64 = 0x000C_0000;
/// ROM/BIOS end (1MB boundary)
pub const ROM_END: u64 = 0x0010_0000;
/// Extended memory start (above 1MB)
pub const EXTENDED_MEMORY_START: u64 = 0x0010_0000;
/// Local APIC base address
pub const LOCAL_APIC_BASE: u64 = 0xFEE0_0000;
/// I/O APIC base address
pub const IO_APIC_BASE: u64 = 0xFEC0_0000;
/// APIC region size
pub const APIC_SIZE: u64 = PAGE_SIZE;

/// Memory protection flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryFlags {
    bits: u32,
}

impl MemoryFlags {
    /// Readable
    pub const READ: u32 = 1 << 0;
    /// Writable
    pub const WRITE: u32 = 1 << 1;
    /// Executable
    pub const EXECUTE: u32 = 1 << 2;
    /// User-accessible (ring 3)
    pub const USER: u32 = 1 << 3;
    /// Memory-mapped I/O (not backed by RAM)
    pub const MMIO: u32 = 1 << 4;
    /// ROM (read-only memory, not writable)
    pub const ROM: u32 = 1 << 5;
    /// Dirty (has been written to)
    pub const DIRTY: u32 = 1 << 6;
    /// Accessed (has been read)
    pub const ACCESSED: u32 = 1 << 7;
    /// Present (mapped)
    pub const PRESENT: u32 = 1 << 8;

    /// Create empty flags
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Create with bits
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Standard RAM flags (read/write)
    pub const fn ram() -> Self {
        Self::from_bits(Self::READ | Self::WRITE | Self::PRESENT)
    }

    /// Read-only memory (ROM)
    pub const fn rom() -> Self {
        Self::from_bits(Self::READ | Self::ROM | Self::PRESENT)
    }

    /// Executable memory
    pub const fn code() -> Self {
        Self::from_bits(Self::READ | Self::EXECUTE | Self::PRESENT)
    }

    /// MMIO region
    pub const fn mmio() -> Self {
        Self::from_bits(Self::READ | Self::WRITE | Self::MMIO | Self::PRESENT)
    }

    /// Check if readable
    pub fn is_readable(&self) -> bool {
        self.bits & Self::READ != 0
    }

    /// Check if writable
    pub fn is_writable(&self) -> bool {
        self.bits & Self::WRITE != 0 && self.bits & Self::ROM == 0
    }

    /// Check if executable
    pub fn is_executable(&self) -> bool {
        self.bits & Self::EXECUTE != 0
    }

    /// Check if MMIO
    pub fn is_mmio(&self) -> bool {
        self.bits & Self::MMIO != 0
    }

    /// Check if present
    pub fn is_present(&self) -> bool {
        self.bits & Self::PRESENT != 0
    }

    /// Check if dirty
    pub fn is_dirty(&self) -> bool {
        self.bits & Self::DIRTY != 0
    }

    /// Set dirty flag
    pub fn set_dirty(&mut self) {
        self.bits |= Self::DIRTY;
    }

    /// Clear dirty flag
    pub fn clear_dirty(&mut self) {
        self.bits &= !Self::DIRTY;
    }

    /// Set accessed flag
    pub fn set_accessed(&mut self) {
        self.bits |= Self::ACCESSED;
    }

    /// Get raw bits
    pub fn bits(&self) -> u32 {
        self.bits
    }
}

impl std::ops::BitOr for MemoryFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self::from_bits(self.bits | rhs.bits)
    }
}

impl std::ops::BitAnd for MemoryFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self::from_bits(self.bits & rhs.bits)
    }
}

/// Type of memory region
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionType {
    /// Regular RAM
    Ram,
    /// Video memory (VGA)
    Video,
    /// ROM/BIOS
    Rom,
    /// Memory-mapped I/O device
    Mmio,
    /// Reserved (not usable)
    Reserved,
}

impl std::fmt::Display for RegionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegionType::Ram => write!(f, "RAM"),
            RegionType::Video => write!(f, "VIDEO"),
            RegionType::Rom => write!(f, "ROM"),
            RegionType::Mmio => write!(f, "MMIO"),
            RegionType::Reserved => write!(f, "RESERVED"),
        }
    }
}

/// A region in the guest physical address space
#[derive(Debug, Clone)]
pub struct AddressRegion {
    /// Starting guest physical address
    pub guest_base: GuestPhysAddr,
    /// Size of the region in bytes
    pub size: u64,
    /// Host virtual address (None for MMIO)
    pub host_base: Option<HostVirtAddr>,
    /// Memory protection flags
    pub flags: MemoryFlags,
    /// Type of region
    pub region_type: RegionType,
    /// Human-readable name
    pub name: String,
}

impl AddressRegion {
    /// Create a new RAM region
    pub fn ram(guest_base: GuestPhysAddr, size: u64, host_base: HostVirtAddr) -> Self {
        Self {
            guest_base,
            size,
            host_base: Some(host_base),
            flags: MemoryFlags::ram(),
            region_type: RegionType::Ram,
            name: format!("RAM@{:#x}", guest_base),
        }
    }

    /// Create a new MMIO region
    pub fn mmio(guest_base: GuestPhysAddr, size: u64, name: &str) -> Self {
        Self {
            guest_base,
            size,
            host_base: None,
            flags: MemoryFlags::mmio(),
            region_type: RegionType::Mmio,
            name: name.to_string(),
        }
    }

    /// Create a new ROM region
    pub fn rom(guest_base: GuestPhysAddr, size: u64, host_base: HostVirtAddr) -> Self {
        Self {
            guest_base,
            size,
            host_base: Some(host_base),
            flags: MemoryFlags::rom(),
            region_type: RegionType::Rom,
            name: format!("ROM@{:#x}", guest_base),
        }
    }

    /// Create a video memory region
    pub fn video(guest_base: GuestPhysAddr, size: u64, host_base: HostVirtAddr) -> Self {
        Self {
            guest_base,
            size,
            host_base: Some(host_base),
            flags: MemoryFlags::ram(),
            region_type: RegionType::Video,
            name: "VGA".to_string(),
        }
    }

    /// Check if an address is contained in this region
    pub fn contains(&self, addr: GuestPhysAddr) -> bool {
        addr >= self.guest_base && addr < self.guest_base + self.size
    }

    /// Check if this region overlaps with another
    pub fn overlaps(&self, other: &Self) -> bool {
        let self_end = self.guest_base + self.size;
        let other_end = other.guest_base + other.size;
        self.guest_base < other_end && other.guest_base < self_end
    }

    /// Translate a guest address to host address
    pub fn translate(&self, guest_addr: GuestPhysAddr) -> Option<HostVirtAddr> {
        if !self.contains(guest_addr) {
            return None;
        }
        self.host_base
            .map(|hva| hva + (guest_addr - self.guest_base))
    }

    /// Get the end address (exclusive)
    pub fn end(&self) -> GuestPhysAddr {
        self.guest_base + self.size
    }
}

/// Page tracking information for dirty page tracking
#[derive(Debug, Clone, Default)]
pub struct PageInfo {
    /// Page is dirty (has been written)
    pub dirty: bool,
    /// Page has been accessed (read or written)
    pub accessed: bool,
    /// Page is pinned (cannot be swapped)
    pub pinned: bool,
}

/// Guest Address Space
///
/// Manages the entire guest physical address space, including RAM, MMIO,
/// and special regions. Provides address translation and memory protection.
pub struct GuestAddressSpace {
    /// Memory regions sorted by guest base address
    regions: RwLock<BTreeMap<GuestPhysAddr, AddressRegion>>,
    /// Total allocated RAM size
    total_ram: RwLock<u64>,
    /// Page-level tracking (for dirty page logging)
    page_tracking: RwLock<BTreeMap<u64, PageInfo>>,
    /// Backing memory allocations
    allocations: RwLock<Vec<MemoryAllocation>>,
}

/// Backing memory allocation
struct MemoryAllocation {
    /// Memory-mapped region
    #[allow(dead_code)]
    mmap: memmap2::MmapMut,
    /// Host address
    host_addr: HostVirtAddr,
    /// Size
    size: u64,
}

impl GuestAddressSpace {
    /// Create a new empty address space
    pub fn new() -> Self {
        Self {
            regions: RwLock::new(BTreeMap::new()),
            total_ram: RwLock::new(0),
            page_tracking: RwLock::new(BTreeMap::new()),
            allocations: RwLock::new(Vec::new()),
        }
    }

    /// Add a memory region
    pub fn add_region(&self, region: AddressRegion) -> Result<()> {
        let mut regions = self.regions.write();

        // Check for overlaps
        for (_, existing) in regions.iter() {
            if region.overlaps(existing) {
                return Err(Error::Memory(format!(
                    "Region '{}' [{:#x}-{:#x}) overlaps with '{}' [{:#x}-{:#x})",
                    region.name,
                    region.guest_base,
                    region.end(),
                    existing.name,
                    existing.guest_base,
                    existing.end()
                )));
            }
        }

        // Update total RAM if this is a RAM region
        if region.region_type == RegionType::Ram {
            *self.total_ram.write() += region.size;
        }

        tracing::debug!(
            "Adding region '{}' type={} [{:#x}-{:#x})",
            region.name,
            region.region_type,
            region.guest_base,
            region.end()
        );

        regions.insert(region.guest_base, region);
        Ok(())
    }

    /// Remove a memory region by base address
    pub fn remove_region(&self, guest_base: GuestPhysAddr) -> Result<AddressRegion> {
        let mut regions = self.regions.write();

        let region = regions
            .remove(&guest_base)
            .ok_or_else(|| Error::Memory(format!("No region at {:#x}", guest_base)))?;

        if region.region_type == RegionType::Ram {
            *self.total_ram.write() -= region.size;
        }

        Ok(region)
    }

    /// Allocate RAM and add as a region
    pub fn allocate_ram(&self, guest_base: GuestPhysAddr, size: u64) -> Result<()> {
        // Allocate host memory
        let mmap = memmap2::MmapMut::map_anon(size as usize)
            .map_err(|e| Error::Memory(format!("Failed to allocate {} bytes: {}", size, e)))?;

        let host_addr = mmap.as_ptr() as HostVirtAddr;

        // Store the allocation
        self.allocations.write().push(MemoryAllocation {
            mmap,
            host_addr,
            size,
        });

        // Create and add the region
        let region = AddressRegion::ram(guest_base, size, host_addr);
        self.add_region(region)
    }

    /// Find the region containing a guest address
    pub fn find_region(&self, guest_addr: GuestPhysAddr) -> Option<AddressRegion> {
        let regions = self.regions.read();

        // Find the region that contains this address
        for (_, region) in regions.iter() {
            if region.contains(guest_addr) {
                return Some(region.clone());
            }
        }

        None
    }

    /// Translate guest physical address to host virtual address
    pub fn translate(&self, guest_addr: GuestPhysAddr) -> Result<HostVirtAddr> {
        self.find_region(guest_addr)
            .and_then(|region| region.translate(guest_addr))
            .ok_or_else(|| Error::Memory(format!("Unmapped guest address: {:#x}", guest_addr)))
    }

    /// Check if a guest address is mapped
    pub fn is_mapped(&self, guest_addr: GuestPhysAddr) -> bool {
        self.find_region(guest_addr).is_some()
    }

    /// Check if a guest address range is all mapped
    pub fn is_range_mapped(&self, guest_addr: GuestPhysAddr, size: u64) -> bool {
        let mut addr = guest_addr;
        while addr < guest_addr + size {
            if let Some(region) = self.find_region(addr) {
                // Jump to the end of this region
                addr = region.end();
            } else {
                return false;
            }
        }
        true
    }

    /// Check if an address is MMIO
    pub fn is_mmio(&self, guest_addr: GuestPhysAddr) -> bool {
        self.find_region(guest_addr)
            .map(|r| r.flags.is_mmio())
            .unwrap_or(false)
    }

    /// Read from guest memory
    pub fn read(&self, guest_addr: GuestPhysAddr, buf: &mut [u8]) -> Result<()> {
        let region = self.find_region(guest_addr).ok_or_else(|| {
            Error::Memory(format!("Read from unmapped address: {:#x}", guest_addr))
        })?;

        if !region.flags.is_readable() {
            return Err(Error::Memory(format!(
                "Read from non-readable region at {:#x}",
                guest_addr
            )));
        }

        if region.flags.is_mmio() {
            return Err(Error::Memory(format!(
                "MMIO read at {:#x} requires device handler",
                guest_addr
            )));
        }

        let host_addr = region
            .translate(guest_addr)
            .ok_or_else(|| Error::Memory(format!("Cannot translate {:#x}", guest_addr)))?;

        // Check bounds
        let end_addr = guest_addr + buf.len() as u64;
        if end_addr > region.end() {
            return Err(Error::Memory(format!(
                "Read crosses region boundary at {:#x}",
                guest_addr
            )));
        }

        // SAFETY: `host_addr` was validated by `region.translate()` to point within
        // a mapped memory region. Bounds check above ensures `buf.len()` bytes fit
        // within the region. The destination buffer is a valid mutable slice.
        unsafe {
            std::ptr::copy_nonoverlapping(host_addr as *const u8, buf.as_mut_ptr(), buf.len());
        }

        // Track page access
        self.mark_accessed(guest_addr);

        Ok(())
    }

    /// Write to guest memory
    pub fn write(&self, guest_addr: GuestPhysAddr, buf: &[u8]) -> Result<()> {
        let region = self.find_region(guest_addr).ok_or_else(|| {
            Error::Memory(format!("Write to unmapped address: {:#x}", guest_addr))
        })?;

        if !region.flags.is_writable() {
            return Err(Error::Memory(format!(
                "Write to non-writable region at {:#x}",
                guest_addr
            )));
        }

        if region.flags.is_mmio() {
            return Err(Error::Memory(format!(
                "MMIO write at {:#x} requires device handler",
                guest_addr
            )));
        }

        let host_addr = region
            .translate(guest_addr)
            .ok_or_else(|| Error::Memory(format!("Cannot translate {:#x}", guest_addr)))?;

        // Check bounds
        let end_addr = guest_addr + buf.len() as u64;
        if end_addr > region.end() {
            return Err(Error::Memory(format!(
                "Write crosses region boundary at {:#x}",
                guest_addr
            )));
        }

        // SAFETY: `host_addr` was validated by `region.translate()` to point within
        // a mapped, writable memory region. Bounds check above ensures `buf.len()`
        // bytes fit within the region. The source buffer is a valid slice.
        unsafe {
            std::ptr::copy_nonoverlapping(buf.as_ptr(), host_addr as *mut u8, buf.len());
        }

        // Track page access and dirty
        self.mark_dirty(guest_addr);

        Ok(())
    }

    /// Mark a page as accessed
    fn mark_accessed(&self, guest_addr: GuestPhysAddr) {
        let page = guest_addr & !(PAGE_SIZE - 1);
        self.page_tracking.write().entry(page).or_default().accessed = true;
    }

    /// Mark a page as dirty
    fn mark_dirty(&self, guest_addr: GuestPhysAddr) {
        let page = guest_addr & !(PAGE_SIZE - 1);
        let mut tracking = self.page_tracking.write();
        let info = tracking.entry(page).or_default();
        info.accessed = true;
        info.dirty = true;
    }

    /// Get dirty pages since last clear
    pub fn get_dirty_pages(&self) -> Vec<GuestPhysAddr> {
        self.page_tracking
            .read()
            .iter()
            .filter(|(_, info)| info.dirty)
            .map(|(&addr, _)| addr)
            .collect()
    }

    /// Clear dirty page tracking
    pub fn clear_dirty_pages(&self) {
        for info in self.page_tracking.write().values_mut() {
            info.dirty = false;
        }
    }

    /// Get all regions
    pub fn regions(&self) -> Vec<AddressRegion> {
        self.regions.read().values().cloned().collect()
    }

    /// Get total RAM size
    pub fn total_ram(&self) -> u64 {
        *self.total_ram.read()
    }

    /// Get a slice of guest memory (unsafe, returns raw pointer)
    ///
    /// # Safety
    /// Caller must ensure the address range is valid and not MMIO
    pub unsafe fn get_slice(&self, guest_addr: GuestPhysAddr, len: usize) -> Result<&[u8]> {
        let host_addr = self.translate(guest_addr)?;
        Ok(std::slice::from_raw_parts(host_addr as *const u8, len))
    }

    /// Get a mutable slice of guest memory.
    ///
    /// # Safety
    ///
    /// - `guest_addr .. guest_addr + len` must lie within a RAM region (not MMIO).
    /// - The caller must ensure no other reference (mutable **or** shared) to the
    ///   same byte range is live at the same time — the usual Rust aliasing rules
    ///   apply even though the signature takes `&self`.
    ///
    /// `&self` is intentional: guest RAM is owned via a raw pointer, so requiring
    /// `&mut self` would needlessly serialise all vCPU memory accesses.
    #[allow(clippy::mut_from_ref)] // SAFETY: see doc above
    pub unsafe fn get_slice_mut(&self, guest_addr: GuestPhysAddr, len: usize) -> Result<&mut [u8]> {
        let host_addr = self.translate(guest_addr)?;
        Ok(std::slice::from_raw_parts_mut(host_addr as *mut u8, len))
    }
}

impl Default for GuestAddressSpace {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for standard PC memory layout
pub struct AddressSpaceBuilder {
    /// Low memory size (up to 640KB)
    low_memory: u64,
    /// Extended memory size (above 1MB)
    extended_memory: u64,
    /// Include VGA memory region
    vga: bool,
    /// Include ROM region
    rom: bool,
    /// MMIO regions to add
    mmio_regions: Vec<(GuestPhysAddr, u64, String)>,
}

impl AddressSpaceBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        Self {
            low_memory: LOW_MEMORY_END,
            extended_memory: 0,
            vga: true,
            rom: true,
            mmio_regions: Vec::new(),
        }
    }

    /// Set the total RAM size (will be split into low + extended)
    pub fn total_ram(mut self, size: u64) -> Self {
        if size <= LOW_MEMORY_END {
            self.low_memory = size;
            self.extended_memory = 0;
        } else {
            self.low_memory = LOW_MEMORY_END;
            self.extended_memory = size - LOW_MEMORY_END;
        }
        self
    }

    /// Include VGA memory region
    pub fn with_vga(mut self, enabled: bool) -> Self {
        self.vga = enabled;
        self
    }

    /// Include ROM region
    pub fn with_rom(mut self, enabled: bool) -> Self {
        self.rom = enabled;
        self
    }

    /// Add an MMIO region
    pub fn add_mmio(mut self, base: GuestPhysAddr, size: u64, name: &str) -> Self {
        self.mmio_regions.push((base, size, name.to_string()));
        self
    }

    /// Add Local APIC MMIO region
    pub fn with_local_apic(self) -> Self {
        self.add_mmio(LOCAL_APIC_BASE, APIC_SIZE, "Local APIC")
    }

    /// Add I/O APIC MMIO region
    pub fn with_io_apic(self) -> Self {
        self.add_mmio(IO_APIC_BASE, APIC_SIZE, "I/O APIC")
    }

    /// Build the address space
    pub fn build(self) -> Result<GuestAddressSpace> {
        let space = GuestAddressSpace::new();

        // Allocate low memory (0 - 640KB)
        if self.low_memory > 0 {
            space.allocate_ram(0, self.low_memory)?;
        }

        // Add VGA region (0xA0000 - 0xBFFFF)
        if self.vga {
            let vga_size = VGA_MEMORY_END - VGA_MEMORY_START;
            let mmap = memmap2::MmapMut::map_anon(vga_size as usize)
                .map_err(|e| Error::Memory(format!("Failed to allocate VGA memory: {}", e)))?;
            let host_addr = mmap.as_ptr() as HostVirtAddr;
            space.allocations.write().push(MemoryAllocation {
                mmap,
                host_addr,
                size: vga_size,
            });
            space.add_region(AddressRegion::video(VGA_MEMORY_START, vga_size, host_addr))?;
        }

        // Add ROM region (0xC0000 - 0xFFFFF) - this would be loaded with BIOS
        if self.rom {
            let rom_size = ROM_END - ROM_START;
            let mmap = memmap2::MmapMut::map_anon(rom_size as usize)
                .map_err(|e| Error::Memory(format!("Failed to allocate ROM: {}", e)))?;
            let host_addr = mmap.as_ptr() as HostVirtAddr;
            space.allocations.write().push(MemoryAllocation {
                mmap,
                host_addr,
                size: rom_size,
            });
            space.add_region(AddressRegion::rom(ROM_START, rom_size, host_addr))?;
        }

        // Allocate extended memory (above 1MB)
        if self.extended_memory > 0 {
            space.allocate_ram(EXTENDED_MEMORY_START, self.extended_memory)?;
        }

        // Add MMIO regions
        for (base, size, name) in self.mmio_regions {
            space.add_region(AddressRegion::mmio(base, size, &name))?;
        }

        Ok(space)
    }
}

impl Default for AddressSpaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_flags() {
        let flags = MemoryFlags::ram();
        assert!(flags.is_readable());
        assert!(flags.is_writable());
        assert!(!flags.is_executable());
        assert!(!flags.is_mmio());
        assert!(flags.is_present());

        let rom_flags = MemoryFlags::rom();
        assert!(rom_flags.is_readable());
        assert!(!rom_flags.is_writable());

        let mmio_flags = MemoryFlags::mmio();
        assert!(mmio_flags.is_mmio());
    }

    #[test]
    fn test_memory_flags_dirty() {
        let mut flags = MemoryFlags::ram();
        assert!(!flags.is_dirty());

        flags.set_dirty();
        assert!(flags.is_dirty());

        flags.clear_dirty();
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_region_overlap() {
        let r1 = AddressRegion::ram(0x1000, 0x1000, 0);
        let r2 = AddressRegion::ram(0x1800, 0x1000, 0);
        let r3 = AddressRegion::ram(0x2000, 0x1000, 0);

        assert!(r1.overlaps(&r2)); // Overlapping
        assert!(r2.overlaps(&r1));
        assert!(!r1.overlaps(&r3)); // Adjacent, not overlapping
        assert!(!r3.overlaps(&r1));
    }

    #[test]
    fn test_region_contains() {
        let region = AddressRegion::ram(0x1000, 0x1000, 0);

        assert!(region.contains(0x1000)); // Start
        assert!(region.contains(0x1500)); // Middle
        assert!(region.contains(0x1FFF)); // End - 1
        assert!(!region.contains(0x2000)); // End (exclusive)
        assert!(!region.contains(0x0FFF)); // Before
        assert!(!region.contains(0x3000)); // After
    }

    #[test]
    fn test_address_space_basic() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0, 0x10000).unwrap();

        assert!(space.is_mapped(0));
        assert!(space.is_mapped(0x8000));
        assert!(!space.is_mapped(0x20000));

        let regions = space.regions();
        assert_eq!(regions.len(), 1);
        assert_eq!(space.total_ram(), 0x10000);
    }

    #[test]
    fn test_address_space_overlap_detection() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0x1000, 0x1000).unwrap();

        // Try to add overlapping region
        let result = space.add_region(AddressRegion::ram(0x1800, 0x1000, 0));
        assert!(result.is_err());
    }

    #[test]
    fn test_address_space_read_write() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0, 0x10000).unwrap();

        // Write data
        let data = [0x12, 0x34, 0x56, 0x78];
        space.write(0x1000, &data).unwrap();

        // Read back
        let mut buf = [0u8; 4];
        space.read(0x1000, &mut buf).unwrap();

        assert_eq!(buf, data);
    }

    #[test]
    fn test_address_space_dirty_tracking() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0, 0x10000).unwrap();

        // Initially no dirty pages
        assert!(space.get_dirty_pages().is_empty());

        // Write to a page
        space.write(0x1000, &[0x42]).unwrap();

        // Now have dirty page
        let dirty = space.get_dirty_pages();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0], 0x1000);

        // Clear dirty tracking
        space.clear_dirty_pages();
        assert!(space.get_dirty_pages().is_empty());
    }

    #[test]
    fn test_address_space_rom_not_writable() {
        let space = GuestAddressSpace::new();

        // Create ROM region
        let mmap = memmap2::MmapMut::map_anon(0x1000).unwrap();
        let host_addr = mmap.as_ptr() as u64;
        space.allocations.write().push(MemoryAllocation {
            mmap,
            host_addr,
            size: 0x1000,
        });
        space
            .add_region(AddressRegion::rom(0x1000, 0x1000, host_addr))
            .unwrap();

        // Can read
        let mut buf = [0u8; 4];
        assert!(space.read(0x1000, &mut buf).is_ok());

        // Cannot write
        assert!(space.write(0x1000, &[0x42]).is_err());
    }

    #[test]
    fn test_address_space_mmio() {
        let space = GuestAddressSpace::new();
        space
            .add_region(AddressRegion::mmio(0xFEE0_0000, 0x1000, "APIC"))
            .unwrap();

        assert!(space.is_mapped(0xFEE0_0000));
        assert!(space.is_mmio(0xFEE0_0000));

        // MMIO reads/writes should fail (need device handler)
        let mut buf = [0u8; 4];
        assert!(space.read(0xFEE0_0000, &mut buf).is_err());
    }

    #[test]
    fn test_address_space_builder() {
        let space = AddressSpaceBuilder::new()
            .total_ram(2 * 1024 * 1024) // 2MB
            .with_vga(true)
            .with_rom(true)
            .build()
            .unwrap();

        // Check low memory
        assert!(space.is_mapped(0));
        assert!(space.is_mapped(0x9_FFFF));

        // Check VGA
        assert!(space.is_mapped(VGA_MEMORY_START));

        // Check ROM
        assert!(space.is_mapped(ROM_START));

        // Check extended memory
        assert!(space.is_mapped(EXTENDED_MEMORY_START));
    }

    #[test]
    fn test_address_space_builder_with_apic() {
        let space = AddressSpaceBuilder::new()
            .total_ram(1024 * 1024)
            .with_local_apic()
            .with_io_apic()
            .build()
            .unwrap();

        assert!(space.is_mmio(LOCAL_APIC_BASE));
        assert!(space.is_mmio(IO_APIC_BASE));
    }

    #[test]
    fn test_translation() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0x1000, 0x2000).unwrap();

        // Get the host address for the region
        let region = space.find_region(0x1000).unwrap();
        let expected_host = region.host_base.unwrap();

        // Translate addresses
        let host = space.translate(0x1000).unwrap();
        assert_eq!(host, expected_host);

        let host2 = space.translate(0x1500).unwrap();
        assert_eq!(host2, expected_host + 0x500);

        // Unmapped should fail
        assert!(space.translate(0).is_err());
    }

    #[test]
    fn test_is_range_mapped() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0, 0x10000).unwrap();

        assert!(space.is_range_mapped(0, 0x10000));
        assert!(space.is_range_mapped(0x1000, 0x2000));
        assert!(!space.is_range_mapped(0, 0x20000)); // Extends beyond
        assert!(!space.is_range_mapped(0x20000, 0x1000)); // Completely outside
    }

    #[test]
    fn test_remove_region() {
        let space = GuestAddressSpace::new();
        space.allocate_ram(0x1000, 0x1000).unwrap();
        space.allocate_ram(0x3000, 0x1000).unwrap();

        assert_eq!(space.total_ram(), 0x2000);
        assert_eq!(space.regions().len(), 2);

        // Remove one region
        let removed = space.remove_region(0x1000).unwrap();
        assert_eq!(removed.guest_base, 0x1000);

        assert_eq!(space.total_ram(), 0x1000);
        assert_eq!(space.regions().len(), 1);
        assert!(!space.is_mapped(0x1000));
        assert!(space.is_mapped(0x3000));
    }
}
