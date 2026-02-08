# Memory Management

HyperMachine provides advanced memory virtualization with hardware-assisted translation, memory ballooning, and zero-copy operations.

## Memory Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Guest Virtual Address                  │
│                      (Guest Page Tables)                 │
├─────────────────────────────────────────────────────────┤
│                  Guest Physical Address                  │
│                      (EPT/NPT Translation)               │
├─────────────────────────────────────────────────────────┤
│                   Host Physical Address                  │
│                    (Actual RAM/MMIO)                     │
└─────────────────────────────────────────────────────────┘
```

## Guest Memory Layout

```rust
pub struct GuestMemory {
    regions: Vec<MemoryRegion>,
    total_size: usize,
}

pub struct MemoryRegion {
    guest_address: u64,      // Guest physical address
    host_address: *mut u8,   // Host virtual address (mmap'd)
    size: usize,
    flags: MemoryFlags,
}

impl GuestMemory {
    pub fn new(size_mb: usize) -> Result<Self> {
        let size = size_mb * 1024 * 1024;
        
        // Allocate host memory (huge pages if available)
        let host_mem = unsafe {
            mmap(
                std::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB,
                -1,
                0,
            )
        };
        
        let region = MemoryRegion {
            guest_address: 0,
            host_address: host_mem as *mut u8,
            size,
            flags: MemoryFlags::READ | MemoryFlags::WRITE,
        };
        
        Ok(Self {
            regions: vec![region],
            total_size: size,
        })
    }
    
    pub fn read(&self, guest_addr: u64, buf: &mut [u8]) -> Result<()> {
        let host_ptr = self.translate(guest_addr)?;
        unsafe {
            std::ptr::copy_nonoverlapping(host_ptr, buf.as_mut_ptr(), buf.len());
        }
        Ok(())
    }
    
    pub fn write(&mut self, guest_addr: u64, data: &[u8]) -> Result<()> {
        let host_ptr = self.translate_mut(guest_addr)?;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), host_ptr, data.len());
        }
        Ok(())
    }
}
```

## Extended Page Tables (EPT)

Intel EPT provides hardware-assisted guest-to-host address translation:

```rust
pub struct EptManager {
    pml4: Box<EptPml4>,
    allocator: PageAllocator,
}

#[repr(C, align(4096))]
pub struct EptPml4 {
    entries: [EptEntry; 512],
}

#[repr(transparent)]
pub struct EptEntry(u64);

impl EptEntry {
    const READ: u64 = 1 << 0;
    const WRITE: u64 = 1 << 1;
    const EXECUTE: u64 = 1 << 2;
    const MEMORY_TYPE_MASK: u64 = 0x7 << 3;
    const HUGE_PAGE: u64 = 1 << 7;
    const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    
    pub fn new(addr: u64, flags: u64) -> Self {
        Self((addr & Self::ADDR_MASK) | flags)
    }
    
    pub fn set_permissions(&mut self, read: bool, write: bool, exec: bool) {
        self.0 &= !0x7;
        if read { self.0 |= Self::READ; }
        if write { self.0 |= Self::WRITE; }
        if exec { self.0 |= Self::EXECUTE; }
    }
}

impl EptManager {
    pub fn map_page(
        &mut self,
        guest_phys: u64,
        host_phys: u64,
        flags: EptFlags,
    ) -> Result<()> {
        let pml4_idx = (guest_phys >> 39) & 0x1FF;
        let pdpt_idx = (guest_phys >> 30) & 0x1FF;
        let pd_idx = (guest_phys >> 21) & 0x1FF;
        let pt_idx = (guest_phys >> 12) & 0x1FF;
        
        // Walk page tables, creating entries as needed
        let pdpt = self.get_or_create_pdpt(pml4_idx)?;
        let pd = self.get_or_create_pd(pdpt, pdpt_idx)?;
        let pt = self.get_or_create_pt(pd, pd_idx)?;
        
        // Map the page
        pt.entries[pt_idx as usize] = EptEntry::new(
            host_phys,
            flags.bits(),
        );
        
        Ok(())
    }
}
```

## Memory Ballooning

Dynamic memory adjustment using virtio-balloon:

```rust
pub struct MemoryBalloon {
    inflated_pages: Vec<u64>,
    target_pages: usize,
    current_pages: usize,
}

impl MemoryBalloon {
    pub fn inflate(&mut self, num_pages: usize) -> Result<()> {
        // Guest returns pages to host
        for _ in 0..num_pages {
            let page = self.guest_allocate_page()?;
            self.inflated_pages.push(page);
            
            // Advise host that page is no longer needed
            unsafe {
                madvise(
                    page as *mut c_void,
                    PAGE_SIZE,
                    MADV_DONTNEED,
                );
            }
        }
        self.current_pages -= num_pages;
        Ok(())
    }
    
    pub fn deflate(&mut self, num_pages: usize) -> Result<()> {
        // Host returns pages to guest
        for _ in 0..num_pages {
            if let Some(page) = self.inflated_pages.pop() {
                self.guest_free_page(page)?;
            }
        }
        self.current_pages += num_pages;
        Ok(())
    }
}
```

## Zero-Copy Operations

Efficient data transfer between host and guest:

```rust
pub struct ZeroCopyBuffer {
    host_addr: *mut u8,
    guest_addr: u64,
    size: usize,
    shared: bool,
}

impl ZeroCopyBuffer {
    /// Create a shared memory region visible to both host and guest
    pub fn new_shared(size: usize) -> Result<Self> {
        // Create shared memory
        let fd = memfd_create("hm-shared", MFD_CLOEXEC)?;
        ftruncate(fd, size as i64)?;
        
        let host_addr = mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            fd,
            0,
        )? as *mut u8;
        
        Ok(Self {
            host_addr,
            guest_addr: 0, // Set when mapped to guest
            size,
            shared: true,
        })
    }
    
    /// Transfer data from host to guest without copying
    pub fn write_to_guest(&self, data: &[u8], offset: usize) -> Result<()> {
        if offset + data.len() > self.size {
            return Err(Error::OutOfBounds);
        }
        
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                self.host_addr.add(offset),
                data.len(),
            );
        }
        
        // Memory fence to ensure visibility
        std::sync::atomic::fence(Ordering::SeqCst);
        
        Ok(())
    }
}
```

## Memory Types

| Type           | Use Case                 | Caching         |
| -------------- | ------------------------ | --------------- |
| **Normal RAM** | Guest memory             | Write-back      |
| **MMIO**       | Device registers         | Uncacheable     |
| **ROM**        | BIOS/UEFI                | Write-protected |
| **Shared**     | Host-guest communication | Write-back      |

## Huge Pages

Using huge pages improves TLB efficiency:

```rust
pub fn allocate_huge_pages(size: usize) -> Result<*mut u8> {
    // Try 1GB pages first
    let addr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB | MAP_HUGE_1GB,
            -1,
            0,
        )
    };
    
    if addr != MAP_FAILED {
        return Ok(addr as *mut u8);
    }
    
    // Fall back to 2MB pages
    let addr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB | MAP_HUGE_2MB,
            -1,
            0,
        )
    };
    
    if addr != MAP_FAILED {
        return Ok(addr as *mut u8);
    }
    
    // Fall back to regular pages
    let addr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    
    if addr == MAP_FAILED {
        return Err(Error::AllocationFailed);
    }
    
    Ok(addr as *mut u8)
}
```

## Performance Metrics

| Operation           | Latency          |
| ------------------- | ---------------- |
| EPT walk (cached)   | ~10 ns           |
| EPT walk (uncached) | ~100 ns          |
| Page fault handling | ~1-10 μs         |
| Balloon inflate     | ~1 ms/1000 pages |

## Next Steps

- [GPU Virtualization](./gpu.md) - GPU memory management
- [Architecture Overview](./overview.md) - System architecture
- [Type-2 Hypervisor](./type-2.md) - Platform-specific memory
