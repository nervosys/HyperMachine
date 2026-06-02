//! Guest memory management with zero-copy design

use crate::{Error, Result};
use memmap2::MmapMut;
use parking_lot::RwLock;
use std::sync::Arc;

/// Guest physical address
pub type GuestAddress = u64;

/// Host virtual address
pub type HostAddress = u64;

/// Memory region in guest physical address space
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub guest_addr: GuestAddress,
    pub size: u64,
    pub host_addr: HostAddress,
    pub readonly: bool,
}

/// Guest memory manager with zero-copy access
pub struct GuestMemory {
    regions: Arc<RwLock<Vec<MemoryRegion>>>,
    mappings: Arc<RwLock<Vec<MemoryMapping>>>,
    total_size: u64,
    /// Host NUMA node this guest's memory is bound to, if any. When set, region
    /// allocations are placed on that node (best-effort; see [`MemoryMapping`]).
    numa_node: Option<u32>,
}

impl GuestMemory {
    /// Create a new guest memory space (host-default NUMA placement).
    pub fn new(size: u64) -> Result<Self> {
        Self::new_on_node(size, None)
    }

    /// Create a new guest memory space whose regions are bound to host NUMA
    /// `numa_node` when `Some` and the platform supports it. Pair this with a
    /// matching `vcpu_affinity` so a VM's cores and memory share a NUMA node.
    pub fn new_on_node(size: u64, numa_node: Option<u32>) -> Result<Self> {
        Ok(Self {
            regions: Arc::new(RwLock::new(Vec::new())),
            mappings: Arc::new(RwLock::new(Vec::new())),
            total_size: size,
            numa_node,
        })
    }

    /// The host NUMA node this guest memory is bound to, if any.
    pub fn numa_node(&self) -> Option<u32> {
        self.numa_node
    }

    /// Allocate a memory region
    pub fn allocate_region(&self, size: u64, readonly: bool) -> Result<GuestAddress> {
        let mut regions = self.regions.write();

        // Find next available guest address
        let guest_addr = regions.last().map(|r| r.guest_addr + r.size).unwrap_or(0);

        // Create memory mapping, bound to this guest's NUMA node when set.
        let mapping = MemoryMapping::new_on_node(size, self.numa_node)?;
        let host_addr = mapping.as_ptr() as u64;

        let region = MemoryRegion {
            guest_addr,
            size,
            host_addr,
            readonly,
        };

        regions.push(region.clone());
        self.mappings.write().push(mapping);

        Ok(guest_addr)
    }

    /// Translate guest address to host address
    pub fn translate(&self, guest_addr: GuestAddress) -> Result<HostAddress> {
        let regions = self.regions.read();

        for region in regions.iter() {
            if guest_addr >= region.guest_addr && guest_addr < region.guest_addr + region.size {
                let offset = guest_addr - region.guest_addr;
                return Ok(region.host_addr + offset);
            }
        }

        Err(Error::Memory(format!(
            "Invalid guest address: 0x{:x}",
            guest_addr
        )))
    }

    /// Translate a guest address range, validating that the full range
    /// fits within a single memory region.
    fn translate_range(&self, guest_addr: GuestAddress, len: u64) -> Result<HostAddress> {
        if len == 0 {
            return self.translate(guest_addr);
        }
        let regions = self.regions.read();
        for region in regions.iter() {
            if guest_addr >= region.guest_addr && guest_addr < region.guest_addr + region.size {
                let offset = guest_addr - region.guest_addr;
                let end_offset = offset.checked_add(len).ok_or_else(|| {
                    Error::Memory(format!(
                        "Address range overflow: 0x{:x} + 0x{:x}",
                        guest_addr, len
                    ))
                })?;
                if end_offset > region.size {
                    return Err(Error::Memory(format!(
                        "Access 0x{:x}..0x{:x} exceeds region 0x{:x}..0x{:x} (overrun by {} bytes)",
                        guest_addr,
                        guest_addr + len,
                        region.guest_addr,
                        region.guest_addr + region.size,
                        end_offset - region.size
                    )));
                }
                return Ok(region.host_addr + offset);
            }
        }
        Err(Error::Memory(format!(
            "Invalid guest address: 0x{:x}",
            guest_addr
        )))
    }

    /// Write bytes to guest memory
    pub fn write_bytes(&self, guest_addr: GuestAddress, data: &[u8]) -> Result<()> {
        let host_addr = self.translate_range(guest_addr, data.len() as u64)?;

        // SAFETY: `host_addr` was validated by `translate_range()` to lie within a
        // mapped memory region, and the full `data.len()` byte range has been
        // bounds-checked against the containing region.
        unsafe {
            let ptr = host_addr as *mut u8;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        }

        Ok(())
    }

    /// Read bytes from guest memory
    pub fn read_bytes(&self, guest_addr: GuestAddress, len: usize) -> Result<Vec<u8>> {
        let host_addr = self.translate_range(guest_addr, len as u64)?;
        let mut data = vec![0u8; len];

        // SAFETY: `host_addr` was validated by `translate_range()` to lie within a
        // mapped memory region, and the full `len` byte range has been bounds-checked
        // against the containing region.
        unsafe {
            let ptr = host_addr as *const u8;
            std::ptr::copy_nonoverlapping(ptr, data.as_mut_ptr(), len);
        }

        Ok(data)
    }

    /// Read bytes from guest memory into an existing buffer (zero allocation)
    ///
    /// This is more efficient than `read_bytes` when you can reuse a buffer.
    pub fn read_bytes_into(&self, guest_addr: GuestAddress, buf: &mut [u8]) -> Result<()> {
        let host_addr = self.translate_range(guest_addr, buf.len() as u64)?;

        // SAFETY: `host_addr` was validated by `translate_range()` to lie within a
        // mapped memory region, and the full `buf.len()` byte range has been bounds-
        // checked against the containing region.
        unsafe {
            let ptr = host_addr as *const u8;
            std::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), buf.len());
        }

        Ok(())
    }

    /// Get total memory size
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Get all memory regions
    pub fn regions(&self) -> Vec<MemoryRegion> {
        self.regions.read().clone()
    }
}

/// Memory mapping backing a guest region.
///
/// By default this is an anonymous `mmap` (zero-copy, host-default NUMA
/// placement). When a region is bound to a host NUMA node via
/// [`MemoryMapping::new_on_node`], it is allocated on that node where the
/// platform supports it (Windows `VirtualAllocExNuma`, Linux `mmap` + `mbind`),
/// falling back to an anonymous mapping otherwise.
pub struct MemoryMapping {
    backing: Backing,
}

enum Backing {
    Anon(MmapMut),
    Numa { ptr: *mut u8, len: usize },
}

// SAFETY: a `Numa` backing owns its allocation exclusively for the lifetime of
// the mapping (freed exactly once on `Drop`); the raw pointer is stable and all
// guest access is externally synchronized by `GuestMemory`'s `RwLock`, the same
// guarantees `MmapMut` relies on.
unsafe impl Send for MemoryMapping {}
unsafe impl Sync for MemoryMapping {}

impl MemoryMapping {
    /// Anonymous mapping with host-default NUMA placement.
    pub fn new(size: u64) -> Result<Self> {
        let mmap = MmapMut::map_anon(size as usize)
            .map_err(|e| Error::Memory(format!("Failed to create memory mapping: {}", e)))?;
        Ok(Self {
            backing: Backing::Anon(mmap),
        })
    }

    /// Mapping bound to host NUMA `node` when `Some` and the platform supports
    /// it; otherwise an anonymous mapping (best-effort, never fails over).
    pub fn new_on_node(size: u64, node: Option<u32>) -> Result<Self> {
        if let Some(node) = node {
            if let Some(ptr) = numa_backing::alloc(size as usize, node) {
                return Ok(Self {
                    backing: Backing::Numa {
                        ptr,
                        len: size as usize,
                    },
                });
            }
        }
        Self::new(size)
    }

    pub fn as_ptr(&self) -> *const u8 {
        match &self.backing {
            Backing::Anon(m) => m.as_ptr(),
            Backing::Numa { ptr, .. } => *ptr as *const u8,
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        match &mut self.backing {
            Backing::Anon(m) => m.as_mut_ptr(),
            Backing::Numa { ptr, .. } => *ptr,
        }
    }

    pub fn size(&self) -> usize {
        match &self.backing {
            Backing::Anon(m) => m.len(),
            Backing::Numa { len, .. } => *len,
        }
    }
}

impl Drop for MemoryMapping {
    fn drop(&mut self) {
        if let Backing::Numa { ptr, len } = &self.backing {
            // SAFETY: `ptr`/`len` came from `numa_backing::alloc` and are
            // released exactly once here.
            unsafe { numa_backing::free(*ptr, *len) };
        }
    }
}

/// Platform NUMA-node-bound allocation for guest regions. `alloc` returns
/// `None` if NUMA placement is unsupported or fails, so callers fall back to an
/// ordinary anonymous mapping.
mod numa_backing {
    #[cfg(windows)]
    pub fn alloc(size: usize, node: u32) -> Option<*mut u8> {
        use windows_sys::Win32::System::Memory::{
            VirtualAllocExNuma, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: standard VirtualAllocExNuma call; returns null on failure.
        let p = unsafe {
            VirtualAllocExNuma(
                GetCurrentProcess(),
                std::ptr::null(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
                node,
            )
        };
        if p.is_null() {
            None
        } else {
            Some(p as *mut u8)
        }
    }

    #[cfg(windows)]
    pub unsafe fn free(ptr: *mut u8, _size: usize) {
        use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
        let _ = VirtualFree(ptr as *mut core::ffi::c_void, 0, MEM_RELEASE);
    }

    #[cfg(target_os = "linux")]
    pub fn alloc(size: usize, node: u32) -> Option<*mut u8> {
        // SAFETY: standard anonymous mmap; MAP_FAILED is checked.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return None;
        }
        // Bind the region to `node` via mbind(2). Best-effort: on failure the
        // memory is still valid, just not node-bound.
        const MPOL_BIND: i32 = 2;
        const MPOL_MF_MOVE: u32 = 1 << 1;
        const BITS: usize = 1024;
        let mut nodemask = [0u64; BITS / 64];
        let idx = (node as usize) / 64;
        if idx < nodemask.len() {
            nodemask[idx] = 1u64 << ((node as usize) % 64);
        }
        // SAFETY: mbind over the freshly-mapped [ptr, ptr+size) region.
        unsafe {
            libc::syscall(
                libc::SYS_mbind,
                ptr,
                size,
                MPOL_BIND,
                nodemask.as_ptr(),
                BITS as core::ffi::c_ulong,
                MPOL_MF_MOVE,
            );
        }
        Some(ptr as *mut u8)
    }

    #[cfg(target_os = "linux")]
    pub unsafe fn free(ptr: *mut u8, size: usize) {
        libc::munmap(ptr as *mut core::ffi::c_void, size);
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub fn alloc(_size: usize, _node: u32) -> Option<*mut u8> {
        None
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub unsafe fn free(_ptr: *mut u8, _size: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guest_memory_allocation() {
        let memory = GuestMemory::new(1024 * 1024).unwrap();
        let addr = memory.allocate_region(4096, false).unwrap();
        assert_eq!(addr, 0);
    }

    #[test]
    fn test_memory_read_write() {
        let memory = GuestMemory::new(1024 * 1024).unwrap();
        let addr = memory.allocate_region(4096, false).unwrap();

        let data = vec![1, 2, 3, 4, 5];
        memory.write_bytes(addr, &data).unwrap();

        let read_data = memory.read_bytes(addr, 5).unwrap();
        assert_eq!(data, read_data);
    }

    #[test]
    fn test_write_bytes_bounds_check() {
        let memory = GuestMemory::new(1024 * 1024).unwrap();
        let addr = memory.allocate_region(16, false).unwrap();
        let data = vec![0xAA; 16];
        memory.write_bytes(addr, &data).unwrap();
        let bad = vec![0xBB; 17];
        assert!(memory.write_bytes(addr, &bad).is_err());
        memory.write_bytes(addr + 15, &[0xCC]).unwrap();
        assert!(memory.write_bytes(addr + 16, &[0xDD]).is_err());
        assert!(memory.write_bytes(addr + 15, &[0xEE, 0xFF]).is_err());
    }

    #[test]
    fn test_read_bytes_bounds_check() {
        let memory = GuestMemory::new(1024 * 1024).unwrap();
        let addr = memory.allocate_region(32, false).unwrap();
        let _ = memory.read_bytes(addr, 32).unwrap();
        assert!(memory.read_bytes(addr, 33).is_err());
        let mut buf = [0u8; 32];
        memory.read_bytes_into(addr, &mut buf).unwrap();
        let mut bad_buf = [0u8; 33];
        assert!(memory.read_bytes_into(addr, &mut bad_buf).is_err());
    }

    #[test]
    fn test_translate_range_zero_length() {
        let memory = GuestMemory::new(1024 * 1024).unwrap();
        let addr = memory.allocate_region(4096, false).unwrap();
        memory.write_bytes(addr, &[]).unwrap();
        let data = memory.read_bytes(addr, 0).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn new_defaults_to_no_numa_node() {
        let memory = GuestMemory::new(4096).unwrap();
        assert_eq!(memory.numa_node(), None);
    }

    #[test]
    fn numa_bound_region_reads_and_writes() {
        // Node 0 exists on every system. On Windows this exercises the real
        // VirtualAllocExNuma path; elsewhere it falls back to an anonymous map.
        // Either way the region must behave as normal guest memory.
        let memory = GuestMemory::new_on_node(1024 * 1024, Some(0)).unwrap();
        assert_eq!(memory.numa_node(), Some(0));
        let addr = memory.allocate_region(4096, false).unwrap();
        let data = vec![0xCD; 256];
        memory.write_bytes(addr, &data).unwrap();
        assert_eq!(memory.read_bytes(addr, 256).unwrap(), data);
    }
}
