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
}

impl GuestMemory {
    /// Create a new guest memory space
    pub fn new(size: u64) -> Result<Self> {
        Ok(Self {
            regions: Arc::new(RwLock::new(Vec::new())),
            mappings: Arc::new(RwLock::new(Vec::new())),
            total_size: size,
        })
    }

    /// Allocate a memory region
    pub fn allocate_region(&self, size: u64, readonly: bool) -> Result<GuestAddress> {
        let mut regions = self.regions.write();

        // Find next available guest address
        let guest_addr = regions.last().map(|r| r.guest_addr + r.size).unwrap_or(0);

        // Create memory mapping
        let mapping = MemoryMapping::new(size)?;
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

/// Memory mapping backed by mmap for zero-copy
pub struct MemoryMapping {
    mmap: MmapMut,
}

impl MemoryMapping {
    pub fn new(size: u64) -> Result<Self> {
        let mmap = MmapMut::map_anon(size as usize)
            .map_err(|e| Error::Memory(format!("Failed to create memory mapping: {}", e)))?;

        Ok(Self { mmap })
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.mmap.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.mmap.as_mut_ptr()
    }

    pub fn size(&self) -> usize {
        self.mmap.len()
    }
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
}
