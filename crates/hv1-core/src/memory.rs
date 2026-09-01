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

    /// Initialize the allocator with a memory region.
    ///
    /// `start` and `end` are automatically page-aligned (start rounds up,
    /// end rounds down).  Returns `Err(InvalidParameter)` if the resulting
    /// region is empty or if `start >= end`.
    pub fn init(&mut self, start: u64, end: u64) -> Result<()> {
        if start >= end {
            return Err(Error::InvalidParameter);
        }
        // Align to page boundary (start up, end down)
        let aligned_start = (start + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
        let aligned_end = end & !(PAGE_SIZE as u64 - 1);
        if aligned_start >= aligned_end {
            return Err(Error::InvalidParameter);
        }
        self.next_frame = aligned_start;
        self.end_frame = aligned_end;
        Ok(())
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
        Self::new(
            phys_addr,
            flags | Self::READ | Self::WRITE | Self::EXECUTE | Self::LARGE_PAGE,
        )
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

    /// Map a guest physical region to host physical memory.
    ///
    /// Returns `Err(InvalidParameter)` if the new region overlaps with an
    /// already-mapped region, and `Err(OutOfMemory)` if the region table
    /// is full.
    pub fn map_region(&mut self, region: GuestMemoryRegion) -> Result<()> {
        if self.count >= 32 {
            return Err(Error::OutOfMemory);
        }
        // Check for overlapping guest physical address ranges
        let new_start = region.guest_phys_addr;
        let new_end = region.guest_phys_addr.saturating_add(region.size);
        for existing in self.regions[..self.count].iter().filter_map(|r| r.as_ref()) {
            let ex_start = existing.guest_phys_addr;
            let ex_end = existing.guest_phys_addr.saturating_add(existing.size);
            if new_start < ex_end && new_end > ex_start {
                return Err(Error::InvalidParameter);
            }
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

// ---------------------------------------------------------------------------
// EPT / NPT page-table construction
// ---------------------------------------------------------------------------

/// Result of building an EPT hierarchy: the physical address of the PML4
/// encoded as an EPTP value ready to be loaded into VMCS.
///
/// EPTP format (Intel SDM Vol 3, 24.6.11):
///   bits 2:0  — memory type for EPT paging structures (6 = WB)
///   bits 5:3  — EPT page-walk length minus 1 (3 = 4-level)
///   bit  6    — enable accessed/dirty flags
///   bits N:12 — physical address of PML4
#[cfg(feature = "intel")]
pub fn build_ept(allocator: &mut FrameAllocator, mapper: &GuestMemoryMapper) -> Result<u64> {
    // Allocate PML4
    let pml4_pa = allocator.allocate_frame()?;
    let pml4 = unsafe { &mut *(pml4_pa.as_u64() as *mut EptPageTable) };
    *pml4 = PageTable::new();

    for region in mapper.regions() {
        let mut gpa = region.guest_phys_addr & !(PAGE_SIZE as u64 - 1);
        let end = region.guest_phys_addr + region.size;
        let mut hpa = region.host_phys_addr & !(PAGE_SIZE as u64 - 1);

        while gpa < end {
            // Check whether we can use a 2 MB large page.
            let remaining = end - gpa;
            let use_2m = gpa & (LARGE_PAGE_SIZE as u64 - 1) == 0
                && hpa & (LARGE_PAGE_SIZE as u64 - 1) == 0
                && remaining >= LARGE_PAGE_SIZE as u64;

            let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
            let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
            let pd_idx = ((gpa >> 21) & 0x1FF) as usize;

            // --- PDPT ---
            if !pml4.entries[pml4_idx].is_present() {
                let pa = allocator.allocate_frame()?;
                let tbl = unsafe { &mut *(pa.as_u64() as *mut EptPageTable) };
                *tbl = PageTable::new();
                pml4.entries[pml4_idx] = EptEntry::table(pa.as_u64());
            }
            let pdpt = unsafe { &mut *(pml4.entries[pml4_idx].addr() as *mut EptPageTable) };

            // --- PD ---
            if !pdpt.entries[pdpt_idx].is_present() {
                let pa = allocator.allocate_frame()?;
                let tbl = unsafe { &mut *(pa.as_u64() as *mut EptPageTable) };
                *tbl = PageTable::new();
                pdpt.entries[pdpt_idx] = EptEntry::table(pa.as_u64());
            }
            let pd = unsafe { &mut *(pdpt.entries[pdpt_idx].addr() as *mut EptPageTable) };

            if use_2m {
                let mut flags = EptEntry::READ | EptEntry::WRITE | EptEntry::MT_WB;
                if region.executable {
                    flags |= EptEntry::EXECUTE;
                }
                pd.entries[pd_idx] = EptEntry::page_2m(hpa, flags);
                gpa += LARGE_PAGE_SIZE as u64;
                hpa += LARGE_PAGE_SIZE as u64;
            } else {
                // --- PT (4 KB) ---
                let pt_idx = ((gpa >> 12) & 0x1FF) as usize;

                if !pd.entries[pd_idx].is_present() {
                    let pa = allocator.allocate_frame()?;
                    let tbl = unsafe { &mut *(pa.as_u64() as *mut EptPageTable) };
                    *tbl = PageTable::new();
                    pd.entries[pd_idx] = EptEntry::table(pa.as_u64());
                }
                let pt = unsafe { &mut *(pd.entries[pd_idx].addr() as *mut EptPageTable) };

                let mut flags = EptEntry::READ | EptEntry::MT_WB;
                if region.writable {
                    flags |= EptEntry::WRITE;
                }
                if region.executable {
                    flags |= EptEntry::EXECUTE;
                }
                pt.entries[pt_idx] = EptEntry::page_4k(hpa, flags);
                gpa += PAGE_SIZE as u64;
                hpa += PAGE_SIZE as u64;
            }
        }
    }

    // Compose EPTP: WB memory type (6), page-walk length 3, AD enabled
    let eptp = pml4_pa.as_u64() | (6) | (3 << 3) | (1 << 6);
    Ok(eptp)
}

/// Build an AMD Nested Page Table hierarchy and return the physical address
/// of the top-level page table (NCR3 value for VMCB).
#[cfg(feature = "amd")]
pub fn build_npt(allocator: &mut FrameAllocator, mapper: &GuestMemoryMapper) -> Result<u64> {
    // Allocate PML4
    let pml4_pa = allocator.allocate_frame()?;
    let pml4 = unsafe { &mut *(pml4_pa.as_u64() as *mut NptPageTable) };
    *pml4 = PageTable::new();

    for region in mapper.regions() {
        let mut gpa = region.guest_phys_addr & !(PAGE_SIZE as u64 - 1);
        let end = region.guest_phys_addr + region.size;
        let mut hpa = region.host_phys_addr & !(PAGE_SIZE as u64 - 1);

        while gpa < end {
            let remaining = end - gpa;
            let use_2m = gpa & (LARGE_PAGE_SIZE as u64 - 1) == 0
                && hpa & (LARGE_PAGE_SIZE as u64 - 1) == 0
                && remaining >= LARGE_PAGE_SIZE as u64;

            let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
            let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
            let pd_idx = ((gpa >> 21) & 0x1FF) as usize;

            // --- PDPT ---
            if !pml4.entries[pml4_idx].is_present() {
                let pa = allocator.allocate_frame()?;
                let tbl = unsafe { &mut *(pa.as_u64() as *mut NptPageTable) };
                *tbl = PageTable::new();
                pml4.entries[pml4_idx] = NptEntry::table(pa.as_u64());
            }
            let pdpt = unsafe { &mut *(pml4.entries[pml4_idx].addr() as *mut NptPageTable) };

            // --- PD ---
            if !pdpt.entries[pdpt_idx].is_present() {
                let pa = allocator.allocate_frame()?;
                let tbl = unsafe { &mut *(pa.as_u64() as *mut NptPageTable) };
                *tbl = PageTable::new();
                pdpt.entries[pdpt_idx] = NptEntry::table(pa.as_u64());
            }
            let pd = unsafe { &mut *(pdpt.entries[pdpt_idx].addr() as *mut NptPageTable) };

            if use_2m {
                pd.entries[pd_idx] = NptEntry::page_2m(hpa, region.writable);
                gpa += LARGE_PAGE_SIZE as u64;
                hpa += LARGE_PAGE_SIZE as u64;
            } else {
                let pt_idx = ((gpa >> 12) & 0x1FF) as usize;

                if !pd.entries[pd_idx].is_present() {
                    let pa = allocator.allocate_frame()?;
                    let tbl = unsafe { &mut *(pa.as_u64() as *mut NptPageTable) };
                    *tbl = PageTable::new();
                    pd.entries[pd_idx] = NptEntry::table(pa.as_u64());
                }
                let pt = unsafe { &mut *(pd.entries[pd_idx].addr() as *mut NptPageTable) };

                pt.entries[pt_idx] = NptEntry::page_4k(hpa, region.writable);
                gpa += PAGE_SIZE as u64;
                hpa += PAGE_SIZE as u64;
            }
        }
    }

    Ok(pml4_pa.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Constants ---

    #[test]
    fn page_size_constants() {
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(LARGE_PAGE_SIZE, 2 * 1024 * 1024);
        assert_eq!(HUGE_PAGE_SIZE, 1024 * 1024 * 1024);
    }

    // --- PhysicalRegion ---

    #[test]
    fn physical_region_new_and_contains() {
        let region = PhysicalRegion::new(0x1000, 0x3000); // start=0x1000, size=0x3000, end=0x4000
        assert!(region.contains(PhysAddr::new(0x1000)));
        assert!(region.contains(PhysAddr::new(0x3FFF)));
        assert!(!region.contains(PhysAddr::new(0x0FFF)));
        assert!(!region.contains(PhysAddr::new(0x4000)));
    }

    #[test]
    fn physical_region_end() {
        let region = PhysicalRegion::new(0x1000, 0x3000);
        assert_eq!(region.end(), PhysAddr::new(0x4000));
    }

    // --- FrameAllocator ---

    #[test]
    fn frame_allocator_default_is_uninit() {
        let fa = FrameAllocator::new();
        assert_eq!(fa.allocated_count(), 0);
        assert_eq!(fa.free_count(), 0);
    }

    #[test]
    fn frame_allocator_init_and_counts() {
        let mut fa = FrameAllocator::new();
        fa.init(0x10_0000, 0x20_0000).unwrap(); // 1 MiB region = 256 frames
        assert_eq!(fa.allocated_count(), 0);
        assert_eq!(fa.free_count(), 256);
    }

    #[test]
    fn frame_allocator_allocate_one() {
        let mut fa = FrameAllocator::new();
        fa.init(0x10_0000, 0x20_0000).unwrap();
        let frame = fa.allocate_frame().unwrap();
        assert_eq!(frame.as_u64(), 0x10_0000);
        assert_eq!(fa.allocated_count(), 1);
        assert_eq!(fa.free_count(), 255);
    }

    #[test]
    fn frame_allocator_allocate_many() {
        let mut fa = FrameAllocator::new();
        fa.init(0x10_0000, 0x20_0000).unwrap();
        let f1 = fa.allocate_frame().unwrap();
        let f2 = fa.allocate_frame().unwrap();
        assert_eq!(f1.as_u64(), 0x10_0000);
        assert_eq!(f2.as_u64(), 0x10_1000);
    }

    #[test]
    fn frame_allocator_allocate_frames() {
        let mut fa = FrameAllocator::new();
        fa.init(0x10_0000, 0x20_0000).unwrap();
        let base = fa.allocate_frames(4).unwrap();
        assert_eq!(base.as_u64(), 0x10_0000);
        assert_eq!(fa.allocated_count(), 4);
    }

    #[test]
    fn frame_allocator_oom() {
        let mut fa = FrameAllocator::new();
        fa.init(0x10_0000, 0x10_1000).unwrap(); // exactly 1 frame
        fa.allocate_frame().unwrap();
        assert!(fa.allocate_frame().is_err());
    }

    // --- EptEntry ---

    #[test]
    fn ept_entry_empty() {
        let e = EptEntry::EMPTY;
        assert!(!e.is_present());
        assert_eq!(e.raw(), 0);
        assert_eq!(e.addr(), 0);
    }

    #[test]
    fn ept_entry_page_4k() {
        let e = EptEntry::page_4k(0x200_000, 0);
        assert!(e.is_present());
        assert_eq!(e.addr(), 0x200_000);
        assert_ne!(e.raw() & EptEntry::READ, 0);
        assert_ne!(e.raw() & EptEntry::WRITE, 0);
        assert_ne!(e.raw() & EptEntry::EXECUTE, 0);
    }

    #[test]
    fn ept_entry_page_2m() {
        let e = EptEntry::page_2m(0x20_0000, EptEntry::MT_WB);
        assert!(e.is_present());
        assert_ne!(e.raw() & EptEntry::LARGE_PAGE, 0);
    }

    #[test]
    fn ept_entry_table() {
        let e = EptEntry::table(0x300_000);
        assert!(e.is_present());
        assert_eq!(e.addr(), 0x300_000);
    }

    #[test]
    fn ept_entry_addr_masks_low_bits() {
        let e = EptEntry::new(0xDEAD_BEEF_1234_5FFF, 0);
        // Only bits 51:12 survive
        assert_eq!(
            e.addr(),
            0x0000_DEAD_BEEF_1234_5000_u64 & 0x000F_FFFF_FFFF_F000
        );
    }

    // --- NptEntry ---

    #[test]
    fn npt_entry_empty() {
        let e = NptEntry::EMPTY;
        assert!(!e.is_present());
        assert_eq!(e.raw(), 0);
    }

    #[test]
    fn npt_entry_page_4k_writable() {
        let e = NptEntry::page_4k(0x1000, true);
        assert!(e.is_present());
        assert_ne!(e.raw() & NptEntry::WRITABLE, 0);
    }

    #[test]
    fn npt_entry_page_4k_readonly() {
        let e = NptEntry::page_4k(0x1000, false);
        assert!(e.is_present());
        assert_eq!(e.raw() & NptEntry::WRITABLE, 0);
    }

    #[test]
    fn npt_entry_page_2m() {
        let e = NptEntry::page_2m(0x20_0000, true);
        assert!(e.is_present());
        assert_ne!(e.raw() & NptEntry::LARGE_PAGE, 0);
        assert_ne!(e.raw() & NptEntry::WRITABLE, 0);
    }

    #[test]
    fn npt_entry_table() {
        let e = NptEntry::table(0x400_000);
        assert!(e.is_present());
        assert_ne!(e.raw() & NptEntry::USER, 0);
        assert_eq!(e.addr(), 0x400_000);
    }

    // --- GuestMemoryMapper ---

    #[test]
    fn mapper_empty() {
        let mapper = GuestMemoryMapper::new();
        assert_eq!(mapper.regions().count(), 0);
        assert_eq!(mapper.translate(0x1000), None);
    }

    #[test]
    fn mapper_map_and_translate() {
        let mut mapper = GuestMemoryMapper::new();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x0,
                host_phys_addr: 0x100_0000,
                size: 0x10_0000,
                writable: true,
                executable: true,
            })
            .unwrap();

        assert_eq!(mapper.translate(0x0), Some(0x100_0000));
        assert_eq!(mapper.translate(0x500), Some(0x100_0500));
        assert_eq!(mapper.translate(0xF_FFFF), Some(0x10F_FFFF));
        assert_eq!(mapper.translate(0x10_0000), None); // past end
    }

    #[test]
    fn mapper_multiple_regions() {
        let mut mapper = GuestMemoryMapper::new();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x0,
                host_phys_addr: 0x100_0000,
                size: 0x1000,
                writable: true,
                executable: true,
            })
            .unwrap();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x10_0000,
                host_phys_addr: 0x200_0000,
                size: 0x1000,
                writable: false,
                executable: false,
            })
            .unwrap();

        assert_eq!(mapper.translate(0x0), Some(0x100_0000));
        assert_eq!(mapper.translate(0x10_0000), Some(0x200_0000));
        assert_eq!(mapper.translate(0x5000), None); // gap
        assert_eq!(mapper.regions().count(), 2);
    }

    #[test]
    fn mapper_overflow() {
        let mut mapper = GuestMemoryMapper::new();
        for i in 0..32 {
            mapper
                .map_region(GuestMemoryRegion {
                    guest_phys_addr: i * 0x1000,
                    host_phys_addr: i * 0x1000,
                    size: 0x1000,
                    writable: true,
                    executable: true,
                })
                .unwrap();
        }
        // 33rd should fail
        assert!(mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0xFF_0000,
                host_phys_addr: 0xFF_0000,
                size: 0x1000,
                writable: true,
                executable: true,
            })
            .is_err());
    }

    // --- Hardening: FrameAllocator input validation ---

    #[test]
    fn frame_allocator_init_start_ge_end() {
        let mut fa = FrameAllocator::new();
        assert!(fa.init(0x20_0000, 0x10_0000).is_err()); // start > end
        assert!(fa.init(0x10_0000, 0x10_0000).is_err()); // start == end
    }

    #[test]
    fn frame_allocator_init_too_small_for_page() {
        let mut fa = FrameAllocator::new();
        // Region smaller than one page after alignment
        assert!(fa.init(0x10_0001, 0x10_0FFF).is_err());
    }

    #[test]
    fn frame_allocator_init_aligns_boundaries() {
        let mut fa = FrameAllocator::new();
        // start=0x10_0001 rounds up to 0x10_1000, end=0x10_3FFF rounds down to 0x10_3000
        fa.init(0x10_0001, 0x10_3FFF).unwrap();
        assert_eq!(fa.free_count(), 2); // 0x10_1000 and 0x10_2000
    }

    // --- Hardening: GuestMemoryMapper overlap detection ---

    #[test]
    fn mapper_rejects_full_overlap() {
        let mut mapper = GuestMemoryMapper::new();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x1000,
                host_phys_addr: 0x100_0000,
                size: 0x3000,
                writable: true,
                executable: true,
            })
            .unwrap();
        // Exact same range overlaps
        assert!(mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x1000,
                host_phys_addr: 0x200_0000,
                size: 0x3000,
                writable: true,
                executable: true,
            })
            .is_err());
    }

    #[test]
    fn mapper_rejects_partial_overlap_start() {
        let mut mapper = GuestMemoryMapper::new();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x2000,
                host_phys_addr: 0x100_0000,
                size: 0x2000,
                writable: true,
                executable: true,
            })
            .unwrap();
        // New region overlaps at the start of existing
        assert!(mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x1000,
                host_phys_addr: 0x200_0000,
                size: 0x2000, // 0x1000..0x3000 overlaps 0x2000..0x4000
                writable: true,
                executable: true,
            })
            .is_err());
    }

    #[test]
    fn mapper_rejects_partial_overlap_end() {
        let mut mapper = GuestMemoryMapper::new();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x1000,
                host_phys_addr: 0x100_0000,
                size: 0x2000,
                writable: true,
                executable: true,
            })
            .unwrap();
        // New region overlaps at the end of existing
        assert!(mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x2000,
                host_phys_addr: 0x200_0000,
                size: 0x2000, // 0x2000..0x4000 overlaps 0x1000..0x3000
                writable: true,
                executable: true,
            })
            .is_err());
    }

    #[test]
    fn mapper_allows_adjacent_non_overlapping() {
        let mut mapper = GuestMemoryMapper::new();
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x0,
                host_phys_addr: 0x100_0000,
                size: 0x1000,
                writable: true,
                executable: true,
            })
            .unwrap();
        // Adjacent but not overlapping
        mapper
            .map_region(GuestMemoryRegion {
                guest_phys_addr: 0x1000,
                host_phys_addr: 0x200_0000,
                size: 0x1000,
                writable: true,
                executable: true,
            })
            .unwrap();
        assert_eq!(mapper.regions().count(), 2);
    }
}
