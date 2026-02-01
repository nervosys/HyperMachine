//! Memory management for Type-1 hypervisor
//!
//! This module handles:
//! - Physical memory allocation
//! - Page table management
//! - EPT (Intel) / NPT (AMD) for guest physical memory
//! - Memory virtualization

use crate::{Error, Result};
use core::alloc::{GlobalAlloc, Layout};
use x86_64::structures::paging::{PageSize, Size4KiB};
use x86_64::PhysAddr;

/// Page size (4KB)
pub const PAGE_SIZE: usize = 4096;

/// Large page size (2MB)
pub const LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Huge page size (1GB)
pub const HUGE_PAGE_SIZE: usize = 1024 * 1024 * 1024;

/// Physical memory region
#[derive(Debug, Clone, Copy)]
pub struct PhysicalRegion {
    /// Start physical address
    pub start: PhysAddr,
    /// Size in bytes
    pub size: u64,
}

impl PhysicalRegion {
    /// Create a new physical region
    pub const fn new(start: u64, size: u64) -> Self {
        Self {
            start: PhysAddr::new(start),
            size,
        }
    }

    /// Check if this region contains the given address
    pub fn contains(&self, addr: PhysAddr) -> bool {
        addr >= self.start && addr < self.start + self.size
    }

    /// Get the end address (exclusive)
    pub fn end(&self) -> PhysAddr {
        self.start + self.size
    }
}

/// Frame allocator for physical memory
pub struct FrameAllocator {
    /// Next free frame
    next_frame: u64,
    /// End of allocatable memory
    end_frame: u64,
    /// Allocated frame count
    allocated_count: u64,
}

impl FrameAllocator {
    /// Create a new frame allocator
    pub const fn new() -> Self {
        Self {
            next_frame: 0,
            end_frame: 0,
            allocated_count: 0,
        }
    }

    /// Initialize the allocator with a memory region
    pub fn init(&mut self, start: u64, end: u64) {
        // Align to page boundary
        self.next_frame = (start + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
        self.end_frame = end & !(PAGE_SIZE as u64 - 1);
    }

    /// Allocate a single frame
    pub fn allocate_frame(&mut self) -> Result<PhysAddr> {
        if self.next_frame >= self.end_frame {
            return Err(Error::OutOfMemory);
        }

        let frame = self.next_frame;
        self.next_frame += PAGE_SIZE as u64;
        self.allocated_count += 1;

        Ok(PhysAddr::new(frame))
    }

    /// Allocate multiple contiguous frames
    pub fn allocate_frames(&mut self, count: usize) -> Result<PhysAddr> {
        let size = count as u64 * PAGE_SIZE as u64;
        
        if self.next_frame + size > self.end_frame {
            return Err(Error::OutOfMemory);
        }

        let frame = self.next_frame;
        self.next_frame += size;
        self.allocated_count += count as u64;

        Ok(PhysAddr::new(frame))
    }

    /// Get the number of allocated frames
    pub fn allocated_count(&self) -> u64 {
        self.allocated_count
    }

    /// Get the number of free frames
    pub fn free_count(&self) -> u64 {
        (self.end_frame - self.next_frame) / PAGE_SIZE as u64
    }
}

impl Default for FrameAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// EPT (Extended Page Table) entry for Intel
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EptEntry(u64);

impl EptEntry {
    /// Entry is not present
    pub const EMPTY: Self = Self(0);

    /// Read permission
    pub const READ: u64 = 1 << 0;
    /// Write permission
    pub const WRITE: u64 = 1 << 1;
    /// Execute permission
    pub const EXECUTE: u64 = 1 << 2;
    /// Memory type (bits 3-5)
    pub const MEMORY_TYPE_MASK: u64 = 0x7 << 3;
    /// Ignore PAT
    pub const IGNORE_PAT: u64 = 1 << 6;
    /// Large page (2MB or 1GB)
    pub const LARGE_PAGE: u64 = 1 << 7;
    /// Accessed flag
    pub const ACCESSED: u64 = 1 << 8;
    /// Dirty flag
    pub const DIRTY: u64 = 1 << 9;
    /// User-mode execute
    pub const USER_EXECUTE: u64 = 1 << 10;

    /// Memory type: Uncacheable
    pub const MT_UC: u64 = 0 << 3;
    /// Memory type: Write Combining
    pub const MT_WC: u64 = 1 << 3;
    /// Memory type: Write Through
    pub const MT_WT: u64 = 4 << 3;
    /// Memory type: Write Protected
    pub const MT_WP: u64 = 5 << 3;
    /// Memory type: Write Back
    pub const MT_WB: u64 = 6 << 3;

    /// Create a new EPT entry
    pub const fn new(phys_addr: u64, flags: u64) -> Self {
        Self((phys_addr & 0x000F_FFFF_FFFF_F000) | flags)
    }

    /// Create an entry for a 4KB page
    pub const fn page_4k(phys_addr: u64, flags: u64) -> Self {
        Self::new(phys_addr, flags | Self::READ | Self::WRITE | Self::EXECUTE)
    }

    /// Create an entry for a 2MB page
    pub const fn page_2m(phys_addr: u64, flags: u64) -> Self {
        Self::new(phys_addr, flags | Self::READ | Self::WRITE | Self::EXECUTE | Self::LARGE_PAGE)
    }

    /// Create an entry pointing to a page table
    pub const fn table(phys_addr: u64) -> Self {
        Self::new(phys_addr, Self::READ | Self::WRITE | Self::EXECUTE)
    }

    /// Check if entry is present
    pub const fn is_present(&self) -> bool {
        self.0 & (Self::READ | Self::WRITE | Self::EXECUTE) != 0
    }

    /// Get the physical address
    pub const fn addr(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    /// Get the raw value
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

/// NPT (Nested Page Table) entry for AMD
/// NPT uses the same format as regular x86-64 page tables
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct NptEntry(u64);

impl NptEntry {
    /// Entry is not present
    pub const EMPTY: Self = Self(0);

    /// Present
    pub const PRESENT: u64 = 1 << 0;
    /// Writable
    pub const WRITABLE: u64 = 1 << 1;
    /// User accessible
    pub const USER: u64 = 1 << 2;
    /// Write through
    pub const WRITE_THROUGH: u64 = 1 << 3;
    /// Cache disabled
    pub const CACHE_DISABLED: u64 = 1 << 4;
    /// Accessed
    pub const ACCESSED: u64 = 1 << 5;
    /// Dirty
    pub const DIRTY: u64 = 1 << 6;
    /// Large page (2MB/1GB)
    pub const LARGE_PAGE: u64 = 1 << 7;
    /// Global
    pub const GLOBAL: u64 = 1 << 8;
    /// No execute
    pub const NO_EXECUTE: u64 = 1 << 63;

    /// Create a new NPT entry
    pub const fn new(phys_addr: u64, flags: u64) -> Self {
        Self((phys_addr & 0x000F_FFFF_FFFF_F000) | flags)
    }

    /// Create an entry for a 4KB page
    pub const fn page_4k(phys_addr: u64, writable: bool) -> Self {
        let flags = Self::PRESENT | if writable { Self::WRITABLE } else { 0 };
        Self::new(phys_addr, flags)
    }

    /// Create an entry for a 2MB page
    pub const fn page_2m(phys_addr: u64, writable: bool) -> Self {
        let flags = Self::PRESENT | Self::LARGE_PAGE | if writable { Self::WRITABLE } else { 0 };
        Self::new(phys_addr, flags)
    }

    /// Create an entry pointing to a page table
    pub const fn table(phys_addr: u64) -> Self {
        Self::new(phys_addr, Self::PRESENT | Self::WRITABLE | Self::USER)
    }

    /// Check if entry is present
    pub const fn is_present(&self) -> bool {
        self.0 & Self::PRESENT != 0
    }

    /// Get the physical address
    pub const fn addr(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    /// Get the raw value
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

/// 4-level page table (EPT or NPT)
#[repr(C, align(4096))]
pub struct PageTable<E: Copy> {
    entries: [E; 512],
}

impl<E: Copy + Default> PageTable<E> {
    /// Create an empty page table
    pub fn new() -> Self {
        Self {
            entries: [E::default(); 512],
        }
    }
}

impl<E: Copy + Default> Default for PageTable<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for EptEntry {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Default for NptEntry {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// EPT page table
pub type EptPageTable = PageTable<EptEntry>;

/// NPT page table
pub type NptPageTable = PageTable<NptEntry>;

/// Guest physical memory region
#[derive(Debug, Clone, Copy)]
pub struct GuestMemoryRegion {
    /// Guest physical address
    pub guest_phys_addr: u64,
    /// Host physical address
    pub host_phys_addr: u64,
    /// Size in bytes
    pub size: u64,
    /// Is writable
    pub writable: bool,
    /// Is executable
    pub executable: bool,
}

/// Memory mapper for guest physical memory
pub struct GuestMemoryMapper {
    /// Mapped regions
    regions: [Option<GuestMemoryRegion>; 32],
    /// Number of mapped regions
    count: usize,
}

impl GuestMemoryMapper {
    /// Create a new memory mapper
    pub const fn new() -> Self {
        Self {
            regions: [None; 32],
            count: 0,
        }
    }

    /// Map a guest physical region to host physical memory
    pub fn map_region(&mut self, region: GuestMemoryRegion) -> Result<()> {
        if self.count >= 32 {
            return Err(Error::OutOfMemory);
        }
        self.regions[self.count] = Some(region);
        self.count += 1;
        Ok(())
    }

    /// Translate guest physical address to host physical address
    pub fn translate(&self, guest_phys: u64) -> Option<u64> {
        for region in self.regions[..self.count].iter().filter_map(|r| r.as_ref()) {
            if guest_phys >= region.guest_phys_addr 
                && guest_phys < region.guest_phys_addr + region.size 
            {
                let offset = guest_phys - region.guest_phys_addr;
                return Some(region.host_phys_addr + offset);
            }
        }
        None
    }

    /// Get iterator over mapped regions
    pub fn regions(&self) -> impl Iterator<Item = &GuestMemoryRegion> {
        self.regions[..self.count].iter().filter_map(|r| r.as_ref())
    }
}

impl Default for GuestMemoryMapper {
    fn default() -> Self {
        Self::new()
    }
}
