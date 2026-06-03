//! GPU passthrough support (VFIO/IOMMU-based)
//!
//! This module provides GPU passthrough functionality that allows direct
//! hardware access to a GPU from within a virtual machine. It uses VFIO
//! on Linux and similar mechanisms on other platforms.

use crate::{GpuError, Result};
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicI32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
#[cfg(target_os = "linux")]
use tokio::sync::Mutex;
use tokio::sync::RwLock;

/// PCI device address
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PciAddress {
    /// PCI domain (usually 0)
    pub domain: u16,
    /// Bus number
    pub bus: u8,
    /// Device number
    pub device: u8,
    /// Function number
    pub function: u8,
}

impl PciAddress {
    /// Create a new PCI address
    #[must_use]
    pub fn new(domain: u16, bus: u8, device: u8, function: u8) -> Self {
        Self {
            domain,
            bus,
            device,
            function,
        }
    }

    /// Parse from BDF string (e.g., "0000:01:00.0")
    pub fn from_bdf(bdf: &str) -> Option<Self> {
        let parts: Vec<&str> = bdf.split(&[':', '.'][..]).collect();
        if parts.len() == 4 {
            Some(Self {
                domain: u16::from_str_radix(parts[0], 16).ok()?,
                bus: u8::from_str_radix(parts[1], 16).ok()?,
                device: u8::from_str_radix(parts[2], 16).ok()?,
                function: u8::from_str_radix(parts[3], 16).ok()?,
            })
        } else if parts.len() == 3 {
            Some(Self {
                domain: 0,
                bus: u8::from_str_radix(parts[0], 16).ok()?,
                device: u8::from_str_radix(parts[1], 16).ok()?,
                function: u8::from_str_radix(parts[2], 16).ok()?,
            })
        } else {
            None
        }
    }

    /// Get the BDF string
    pub fn to_bdf(&self) -> String {
        format!(
            "{:04x}:{:02x}:{:02x}.{:x}",
            self.domain, self.bus, self.device, self.function
        )
    }

    /// Get the sysfs path for this device
    #[cfg(target_os = "linux")]
    pub fn sysfs_path(&self) -> PathBuf {
        PathBuf::from(format!("/sys/bus/pci/devices/{}", self.to_bdf()))
    }
}

impl std::fmt::Display for PciAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_bdf())
    }
}

/// GPU passthrough configuration
#[derive(Debug, Clone)]
pub struct PassthroughConfig {
    /// PCI address of the GPU
    pub pci_address: PciAddress,
    /// Vendor ID (e.g., 0x10de for NVIDIA)
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Enable IOMMU
    pub iommu_enabled: bool,
    /// Use VFIO
    pub vfio: bool,
    /// Enable GPU reset on detach
    pub reset_on_detach: bool,
    /// Allow host driver rebind
    pub allow_rebind: bool,
    /// BAR (Base Address Register) sizes to expose
    pub bar_sizes: Vec<u64>,
    /// Enable MSI-X
    pub msix_enabled: bool,
    /// ROM file path (optional)
    pub rom_path: Option<PathBuf>,
}

impl PassthroughConfig {
    /// Create a new passthrough config
    #[must_use]
    pub fn new(pci_address: PciAddress, vendor_id: u16, device_id: u16) -> Self {
        Self {
            pci_address,
            vendor_id,
            device_id,
            iommu_enabled: true,
            vfio: true,
            reset_on_detach: true,
            allow_rebind: true,
            bar_sizes: Vec::new(),
            msix_enabled: true,
            rom_path: None,
        }
    }
}

/// PCI BAR (Base Address Register) information
#[derive(Debug, Clone)]
pub struct PciBar {
    /// BAR index (0-5)
    pub index: u8,
    /// Base address
    pub address: u64,
    /// Size in bytes
    pub size: u64,
    /// Is memory (vs I/O)
    pub is_memory: bool,
    /// Is 64-bit
    pub is_64bit: bool,
    /// Is prefetchable
    pub prefetchable: bool,
}

/// GPU device information
#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    /// PCI address
    pub pci_address: PciAddress,
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Subsystem vendor ID
    pub subsystem_vendor_id: u16,
    /// Subsystem device ID
    pub subsystem_device_id: u16,
    /// Device class
    pub class_code: u32,
    /// Device name
    pub name: String,
    /// Current driver
    pub driver: Option<String>,
    /// IOMMU group
    pub iommu_group: Option<u32>,
    /// BAR information
    pub bars: Vec<PciBar>,
}

/// Passthrough state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassthroughState {
    /// Device is detached (available for passthrough)
    Detached,
    /// Device is attached to host driver
    HostAttached,
    /// Device is bound to VFIO
    VfioBound,
    /// Device is passed through to a VM
    PassedThrough,
    /// Error state
    Error,
}

/// Statistics for passthrough device
#[derive(Debug, Default)]
pub struct PassthroughStats {
    /// Interrupts delivered
    pub interrupts: AtomicU64,
    /// MMIO reads
    pub mmio_reads: AtomicU64,
    /// MMIO writes
    pub mmio_writes: AtomicU64,
    /// DMA transfers
    pub dma_transfers: AtomicU64,
    /// Bytes transferred via DMA
    pub dma_bytes: AtomicU64,
}

/// VFIO container for managing IOMMU domains
#[cfg(target_os = "linux")]
pub struct VfioContainer {
    /// Container file descriptor
    fd: i32,
    /// IOMMU groups
    groups: HashMap<u32, i32>,
}

#[cfg(target_os = "linux")]
impl VfioContainer {
    /// Create a new VFIO container
    pub fn new() -> Result<Self> {
        use std::ffi::CString;

        // SAFETY: CString::new on a literal without embedded NULs never fails
        let path = CString::new("/dev/vfio/vfio").expect("static path has no NUL bytes");
        // SAFETY: `path` is a valid NUL-terminated C string. Standard POSIX open;
        // return value is checked immediately.
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return Err(GpuError::NotAvailable(
                "Failed to open VFIO container".into(),
            ));
        }

        // Check VFIO API version
        const VFIO_GET_API_VERSION: u64 = 0x3B64;
        const VFIO_API_VERSION: i32 = 0;
        // SAFETY: `fd` is a valid open VFIO container file descriptor.
        let version = unsafe { libc::ioctl(fd, VFIO_GET_API_VERSION) };
        if version != VFIO_API_VERSION {
            // SAFETY: fd is a valid open file descriptor from the successful open above.
            unsafe { libc::close(fd) };
            return Err(GpuError::NotAvailable(format!(
                "VFIO API version mismatch: {} != {}",
                version, VFIO_API_VERSION
            )));
        }

        Ok(Self {
            fd,
            groups: HashMap::new(),
        })
    }

    /// Add an IOMMU group to the container
    pub fn add_group(&mut self, group_id: u32) -> Result<()> {
        use std::ffi::CString;

        // SAFETY: group_id is a u32, format! output contains no NUL bytes
        let group_path = CString::new(format!("/dev/vfio/{}", group_id))
            .expect("u32 format output has no NUL bytes");
        // SAFETY: group_path is a valid NUL-terminated C string. Standard POSIX open.
        let group_fd = unsafe { libc::open(group_path.as_ptr(), libc::O_RDWR) };
        if group_fd < 0 {
            return Err(GpuError::NotAvailable(format!(
                "Failed to open VFIO group {}",
                group_id
            )));
        }

        // Set container for group
        const VFIO_GROUP_SET_CONTAINER: u64 = 0x3B66;
        // SAFETY: group_fd is a valid open VFIO group fd; self.fd is a valid container fd.
        let ret = unsafe { libc::ioctl(group_fd, VFIO_GROUP_SET_CONTAINER, &self.fd) };
        if ret < 0 {
            // SAFETY: group_fd is a valid fd that we just opened above.
            unsafe { libc::close(group_fd) };
            return Err(GpuError::InitFailed(format!(
                "Failed to set container for group {}",
                group_id
            )));
        }

        self.groups.insert(group_id, group_fd);
        Ok(())
    }

    /// Get a device from a group
    pub fn get_device(&self, group_id: u32, device_name: &str) -> Result<i32> {
        use std::ffi::CString;

        let group_fd = self.groups.get(&group_id).ok_or_else(|| {
            GpuError::NotAvailable(format!("IOMMU group {} not in container", group_id))
        })?;

        let name = CString::new(device_name).map_err(|_| {
            GpuError::NotAvailable(format!(
                "Device name '{}' contains interior NUL byte",
                device_name
            ))
        })?;
        const VFIO_GROUP_GET_DEVICE_FD: u64 = 0x3B6A;
        // SAFETY: group_fd is a valid open VFIO group fd; name is a NUL-terminated C string.
        let device_fd = unsafe { libc::ioctl(*group_fd, VFIO_GROUP_GET_DEVICE_FD, name.as_ptr()) };
        if device_fd < 0 {
            return Err(GpuError::NotAvailable(format!(
                "Failed to get device {} from group {}",
                device_name, group_id
            )));
        }

        Ok(device_fd)
    }
}

#[cfg(target_os = "linux")]
impl Drop for VfioContainer {
    fn drop(&mut self) {
        for (_, group_fd) in self.groups.drain() {
            // SAFETY: group_fd is a valid fd from a successful libc::open call.
            unsafe { libc::close(group_fd) };
        }
        // SAFETY: self.fd is a valid fd from a successful libc::open call in new().
        unsafe { libc::close(self.fd) };
    }
}

/// A sub-range of a BAR that VFIO permits `mmap`-ing, described by geometry
/// alone so the routing and capability-parsing logic stays platform-agnostic
/// and unit-testable (the actual mapping is Linux/`unsafe` and lives below).
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MmapRange {
    /// Byte offset of this range from the start of the BAR.
    bar_offset: u64,
    /// Length of the range in bytes.
    len: u64,
}

/// VFIO capability id for a sparse-`mmap` description (`linux/vfio.h`).
#[cfg(any(target_os = "linux", test))]
const VFIO_REGION_INFO_CAP_SPARSE_MMAP: u16 = 1;

/// If an aligned `[offset, offset + size)` access lies entirely within
/// `range`, return its offset *within the range* (for direct pointer access);
/// otherwise `None`, meaning the caller must fall back to `pread`/`pwrite`.
///
/// Misaligned accesses always fall back: typed volatile access to mapped MMIO
/// requires natural alignment, and the page-aligned mapping base means an
/// access aligned within the BAR is aligned within the mapping.
#[cfg(any(target_os = "linux", test))]
fn range_contains_access(range: MmapRange, offset: u64, size: u8) -> Option<u64> {
    let size = size as u64;
    if size == 0 || !offset.is_multiple_of(size) {
        return None;
    }
    let end = offset.checked_add(size)?;
    let range_end = range.bar_offset.checked_add(range.len)?;
    if offset >= range.bar_offset && end <= range_end {
        Some(offset - range.bar_offset)
    } else {
        None
    }
}

/// Parse the VFIO sparse-`mmap` capability out of a `vfio_region_info`
/// capability buffer. `buf` is the whole region-info structure as returned by
/// the kernel; the capability chain begins at byte `cap_offset` from its start.
/// Returns the mmap-able sub-ranges, or an empty vec if the chain holds no
/// sparse-mmap capability (in which case the whole region is mappable).
///
/// ABI (`linux/vfio.h`):
/// ```text
/// struct vfio_info_cap_header { __u16 id; __u16 version; __u32 next; }
/// struct vfio_region_info_cap_sparse_mmap {
///     header; __u32 nr_areas; __u32 reserved;
///     struct { __u64 offset; __u64 size; } areas[nr_areas];
/// }
/// ```
/// `next` is a byte offset from the start of `buf`; `0` ends the chain.
#[cfg(any(target_os = "linux", test))]
fn parse_sparse_mmap(buf: &[u8], cap_offset: u32) -> Vec<MmapRange> {
    let read_u16 = |b: &[u8], p: usize| u16::from_le_bytes([b[p], b[p + 1]]);
    let read_u32 = |b: &[u8], p: usize| u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
    let read_u64 = |b: &[u8], p: usize| {
        u64::from_le_bytes([
            b[p],
            b[p + 1],
            b[p + 2],
            b[p + 3],
            b[p + 4],
            b[p + 5],
            b[p + 6],
            b[p + 7],
        ])
    };

    let mut pos = cap_offset as usize;
    // Bound the walk so a malformed/cyclic chain can never spin.
    for _ in 0..64 {
        if pos == 0 || pos + 8 > buf.len() {
            break;
        }
        let id = read_u16(buf, pos);
        let next = read_u32(buf, pos + 4) as usize;

        if id == VFIO_REGION_INFO_CAP_SPARSE_MMAP {
            // header(8) + nr_areas(4) + reserved(4), then the areas[].
            let body = pos + 8;
            if body + 8 > buf.len() {
                break;
            }
            let nr_areas = read_u32(buf, body) as usize;
            let areas = body + 8;
            let mut ranges = Vec::with_capacity(nr_areas);
            for a in 0..nr_areas {
                let base = areas + a * 16;
                if base + 16 > buf.len() {
                    break;
                }
                let off = read_u64(buf, base);
                let len = read_u64(buf, base + 8);
                if len > 0 {
                    ranges.push(MmapRange {
                        bar_offset: off,
                        len,
                    });
                }
            }
            return ranges;
        }

        // The chain must move strictly forward.
        if next <= pos {
            break;
        }
        pos = next;
    }
    Vec::new()
}

/// A BAR sub-range mapped into our address space via `mmap` on the VFIO fd.
#[cfg(target_os = "linux")]
struct MappedRange {
    /// Geometry of this range within the BAR.
    range: MmapRange,
    /// `mmap` base pointer for the range.
    ptr: *mut u8,
    /// Length passed to `mmap` (and to `munmap` on teardown).
    map_len: usize,
}

// SAFETY: `ptr` refers to a shared device mapping (`MAP_SHARED`) that stays
// valid for the lifetime of the attachment. The device hardware — not Rust —
// owns the memory's interior mutability; all access goes through volatile
// reads/writes, so concurrent use across threads is sound.
#[cfg(target_os = "linux")]
unsafe impl Send for MappedRange {}
#[cfg(target_os = "linux")]
unsafe impl Sync for MappedRange {}

/// VFIO BAR region mapping info (cached from VFIO_DEVICE_GET_REGION_INFO).
#[cfg(target_os = "linux")]
struct VfioBarRegion {
    /// Offset within the VFIO device fd for this BAR region.
    offset: u64,
    /// Size of this BAR region in bytes.
    size: u64,
    /// `mmap`'d sub-ranges. Empty means no fast path — every access to this
    /// BAR falls back to `pread`/`pwrite`.
    mapped: Vec<MappedRange>,
}

/// Read a naturally-aligned 1/2/4/8-byte value from mapped MMIO.
///
/// # Safety
/// `ptr` must point `size` valid bytes into a live device mapping and be
/// aligned to `size`. PCI MMIO is little-endian; on the supported (x86-64)
/// hosts a native typed load matches that ordering.
#[cfg(target_os = "linux")]
unsafe fn read_volatile_mmio(ptr: *const u8, size: u8) -> u64 {
    match size {
        1 => u64::from(ptr.read_volatile()),
        2 => u64::from(ptr.cast::<u16>().read_volatile()),
        4 => u64::from(ptr.cast::<u32>().read_volatile()),
        8 => ptr.cast::<u64>().read_volatile(),
        _ => 0,
    }
}

/// Write a naturally-aligned 1/2/4/8-byte value to mapped MMIO.
///
/// # Safety
/// As [`read_volatile_mmio`]; `ptr` must be writable device-mapped memory.
#[cfg(target_os = "linux")]
unsafe fn write_volatile_mmio(ptr: *mut u8, size: u8, value: u64) {
    match size {
        1 => ptr.write_volatile(value as u8),
        2 => ptr.cast::<u16>().write_volatile(value as u16),
        4 => ptr.cast::<u32>().write_volatile(value as u32),
        8 => ptr.cast::<u64>().write_volatile(value),
        _ => {}
    }
}

/// GPU passthrough manager
pub struct GpuPassthrough {
    /// Configuration
    config: PassthroughConfig,
    /// Device information
    device_info: RwLock<Option<GpuDeviceInfo>>,
    /// Current state
    state: RwLock<PassthroughState>,
    /// VFIO container (Linux only)
    #[cfg(target_os = "linux")]
    vfio_container: Mutex<Option<VfioContainer>>,
    /// VFIO device fd (Linux only)
    #[cfg(target_os = "linux")]
    vfio_device_fd: Mutex<Option<i32>>,
    /// Lock-free mirror of the VFIO device fd for the MMIO hot path (`pread`/
    /// `pwrite` fallback). `-1` when detached. Set under the same critical
    /// sections that own `vfio_device_fd`, read with `Acquire` on every access.
    #[cfg(target_os = "linux")]
    device_fd: AtomicI32,
    /// Cached BAR region mappings from VFIO (Linux only)
    #[cfg(target_os = "linux")]
    bar_regions: RwLock<[Option<VfioBarRegion>; 6]>,
    /// Is attached
    attached: AtomicBool,
    /// Statistics
    stats: Arc<PassthroughStats>,
    /// Interrupt handler
    #[allow(clippy::type_complexity)]
    interrupt_handler: RwLock<Option<Arc<dyn Fn(u32) + Send + Sync>>>,
}

impl GpuPassthrough {
    /// Create a new GPU passthrough manager
    #[must_use]
    pub fn new(config: PassthroughConfig) -> Self {
        Self {
            config,
            device_info: RwLock::new(None),
            state: RwLock::new(PassthroughState::Detached),
            #[cfg(target_os = "linux")]
            vfio_container: Mutex::new(None),
            #[cfg(target_os = "linux")]
            vfio_device_fd: Mutex::new(None),
            #[cfg(target_os = "linux")]
            device_fd: AtomicI32::new(-1),
            #[cfg(target_os = "linux")]
            bar_regions: RwLock::new(std::array::from_fn(|_| None)),
            attached: AtomicBool::new(false),
            stats: Arc::new(PassthroughStats::default()),
            interrupt_handler: RwLock::new(None),
        }
    }

    /// Get the PCI address
    pub fn pci_address(&self) -> &PciAddress {
        &self.config.pci_address
    }

    /// Get current state
    pub async fn state(&self) -> PassthroughState {
        *self.state.read().await
    }

    /// Check if attached
    pub fn is_attached(&self) -> bool {
        self.attached.load(Ordering::SeqCst)
    }

    /// Get device info
    pub async fn device_info(&self) -> Option<GpuDeviceInfo> {
        self.device_info.read().await.clone()
    }

    /// Get statistics
    pub fn stats(&self) -> &PassthroughStats {
        &self.stats
    }

    /// Set interrupt handler
    pub async fn set_interrupt_handler<F>(&self, handler: F)
    where
        F: Fn(u32) + Send + Sync + 'static,
    {
        *self.interrupt_handler.write().await = Some(Arc::new(handler));
    }

    /// Discover GPU device information
    pub async fn discover(&self) -> Result<GpuDeviceInfo> {
        tracing::info!("Discovering GPU at {}", self.config.pci_address);

        #[cfg(target_os = "linux")]
        {
            let sysfs_path = self.config.pci_address.sysfs_path();

            // Read vendor ID
            let vendor_path = sysfs_path.join("vendor");
            let vendor_str = std::fs::read_to_string(&vendor_path)
                .map_err(|e| GpuError::NotAvailable(format!("Cannot read vendor: {}", e)))?;
            let vendor_id = u16::from_str_radix(vendor_str.trim().trim_start_matches("0x"), 16)
                .map_err(|_| GpuError::NotAvailable("Invalid vendor ID".into()))?;

            // Read device ID
            let device_path = sysfs_path.join("device");
            let device_str = std::fs::read_to_string(&device_path)
                .map_err(|e| GpuError::NotAvailable(format!("Cannot read device: {}", e)))?;
            let device_id = u16::from_str_radix(device_str.trim().trim_start_matches("0x"), 16)
                .map_err(|_| GpuError::NotAvailable("Invalid device ID".into()))?;

            // Read class code
            let class_path = sysfs_path.join("class");
            let class_str =
                std::fs::read_to_string(&class_path).unwrap_or_else(|_| "0x030000".into());
            let class_code = u32::from_str_radix(class_str.trim().trim_start_matches("0x"), 16)
                .unwrap_or(0x030000);

            // Read driver
            let driver_link = sysfs_path.join("driver");
            let driver = std::fs::read_link(&driver_link)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));

            // Read IOMMU group
            let iommu_link = sysfs_path.join("iommu_group");
            let iommu_group = std::fs::read_link(&iommu_link).ok().and_then(|p| {
                // Keep the borrow of `p` inside the closure: file_name() returns
                // a reference into `p`, which is dropped at the closure boundary.
                p.file_name()
                    .and_then(|n| n.to_string_lossy().parse::<u32>().ok())
            });

            // Read BARs
            let mut bars = Vec::new();
            for i in 0..6 {
                let resource_path = sysfs_path.join(format!("resource{}", i));
                if let Ok(content) = std::fs::read_to_string(&resource_path) {
                    // Parse resource file: start end flags
                    if let Some(line) = content.lines().next() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 3 {
                            if let (Ok(start), Ok(end), Ok(flags)) = (
                                u64::from_str_radix(parts[0].trim_start_matches("0x"), 16),
                                u64::from_str_radix(parts[1].trim_start_matches("0x"), 16),
                                u64::from_str_radix(parts[2].trim_start_matches("0x"), 16),
                            ) {
                                if end > start {
                                    bars.push(PciBar {
                                        index: i,
                                        address: start,
                                        size: end - start + 1,
                                        is_memory: (flags & 0x200) == 0,
                                        is_64bit: (flags & 0x4) != 0,
                                        prefetchable: (flags & 0x8) != 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            let info = GpuDeviceInfo {
                pci_address: self.config.pci_address.clone(),
                vendor_id,
                device_id,
                subsystem_vendor_id: 0,
                subsystem_device_id: 0,
                class_code,
                name: format!("{:04x}:{:04x}", vendor_id, device_id),
                driver,
                iommu_group,
                bars,
            };

            *self.device_info.write().await = Some(info.clone());
            Ok(info)
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err(GpuError::Unsupported(
                "GPU passthrough discovery only supported on Linux".into(),
            ))
        }
    }

    /// Attach the GPU for passthrough
    pub async fn attach(&self) -> Result<()> {
        tracing::info!("Attaching GPU passthrough: {}", self.config.pci_address);

        // Discover device first
        #[allow(unused_variables)]
        let device_info = self.discover().await?;

        #[cfg(target_os = "linux")]
        {
            let iommu_group = device_info
                .iommu_group
                .ok_or_else(|| GpuError::NotAvailable("Device has no IOMMU group".into()))?;

            // Unbind from current driver if needed
            if device_info.driver.is_some() {
                self.unbind_driver().await?;
            }

            // Bind to vfio-pci driver
            self.bind_vfio().await?;

            // Set up VFIO container
            let mut container = VfioContainer::new()?;
            container.add_group(iommu_group)?;

            let device_fd = container.get_device(iommu_group, &self.config.pci_address.to_bdf())?;

            *self.vfio_container.lock().await = Some(container);
            *self.vfio_device_fd.lock().await = Some(device_fd);
            // Publish the fd for the lock-free MMIO fallback path.
            self.device_fd.store(device_fd, Ordering::Release);

            // Query VFIO region info for each BAR (indices 0-5) and map the
            // mmap-able sub-ranges for the direct-access fast path.
            self.discover_bar_regions(device_fd).await;

            *self.state.write().await = PassthroughState::PassedThrough;
        }

        #[cfg(not(target_os = "linux"))]
        {
            // On non-Linux platforms, we simulate attachment
            *self.state.write().await = PassthroughState::PassedThrough;
            tracing::warn!("GPU passthrough is simulated on this platform");
        }

        self.attached.store(true, Ordering::SeqCst);
        tracing::info!("GPU passthrough attached successfully");
        Ok(())
    }

    /// Unbind the device from its current driver
    #[cfg(target_os = "linux")]
    async fn unbind_driver(&self) -> Result<()> {
        let unbind_path = self.config.pci_address.sysfs_path().join("driver/unbind");
        let bdf = self.config.pci_address.to_bdf();

        std::fs::write(&unbind_path, &bdf)
            .map_err(|e| GpuError::InitFailed(format!("Failed to unbind driver: {}", e)))?;

        tracing::debug!("Unbound device {} from driver", bdf);
        Ok(())
    }

    /// Bind the device to vfio-pci
    #[cfg(target_os = "linux")]
    async fn bind_vfio(&self) -> Result<()> {
        // Write vendor:device to vfio-pci new_id
        let new_id_path = PathBuf::from("/sys/bus/pci/drivers/vfio-pci/new_id");
        let id_str = format!(
            "{:04x} {:04x}",
            self.config.vendor_id, self.config.device_id
        );

        if let Err(e) = std::fs::write(&new_id_path, &id_str) {
            tracing::debug!("new_id write failed (may already exist): {}", e);
        }

        // Bind to vfio-pci
        let bind_path = PathBuf::from("/sys/bus/pci/drivers/vfio-pci/bind");
        let bdf = self.config.pci_address.to_bdf();

        std::fs::write(&bind_path, &bdf)
            .map_err(|e| GpuError::InitFailed(format!("Failed to bind to vfio-pci: {}", e)))?;

        tracing::debug!("Bound device {} to vfio-pci", bdf);
        *self.state.write().await = PassthroughState::VfioBound;
        Ok(())
    }

    /// Discover BAR region offsets/sizes via VFIO_DEVICE_GET_REGION_INFO
    #[cfg(target_os = "linux")]
    async fn discover_bar_regions(&self, device_fd: i32) {
        // VFIO region info ioctl and struct layout (from linux/vfio.h)
        //   struct vfio_region_info {
        //       __u32 argsz;   // offset 0
        //       __u32 flags;   // offset 4
        //       __u32 index;   // offset 8
        //       __u32 cap_offset; // offset 12
        //       __u64 size;    // offset 16
        //       __u64 offset;  // offset 24
        //   }
        // Total size = 32 bytes
        const VFIO_DEVICE_GET_REGION_INFO: u64 = 0x3B68;
        const VFIO_REGION_INFO_FLAG_MMAP: u32 = 1 << 1;
        const VFIO_REGION_INFO_FLAG_CAPS: u32 = 1 << 3;
        const REGION_INFO_SIZE: usize = 32;

        let mut regions: [Option<VfioBarRegion>; 6] = std::array::from_fn(|_| None);

        for bar_index in 0u32..6 {
            #[repr(C)]
            struct VfioRegionInfo {
                argsz: u32,
                flags: u32,
                index: u32,
                cap_offset: u32,
                size: u64,
                offset: u64,
            }

            let mut info = VfioRegionInfo {
                argsz: REGION_INFO_SIZE as u32,
                flags: 0,
                index: bar_index,
                cap_offset: 0,
                size: 0,
                offset: 0,
            };

            // First pass: learn flags/size/offset and (via argsz) how large a
            // buffer the kernel needs to also hand back the capability chain.
            // SAFETY: device_fd is a valid VFIO device fd; info is correctly sized.
            let ret = unsafe { libc::ioctl(device_fd, VFIO_DEVICE_GET_REGION_INFO, &mut info) };
            if ret != 0 || info.size == 0 {
                continue;
            }

            let mappable = info.flags & VFIO_REGION_INFO_FLAG_MMAP != 0;
            let has_caps = info.flags & VFIO_REGION_INFO_FLAG_CAPS != 0;

            // Work out which sub-ranges of the BAR may be mapped.
            let ranges: Vec<MmapRange> = if !mappable {
                Vec::new()
            } else if has_caps && info.argsz as usize > REGION_INFO_SIZE {
                // Second pass: re-issue with a buffer big enough to receive the
                // capability chain, then parse out the sparse-mmap areas.
                let total = info.argsz as usize;
                let mut buf = vec![0u8; total];
                buf[0..4].copy_from_slice(&info.argsz.to_le_bytes());
                buf[8..12].copy_from_slice(&bar_index.to_le_bytes());
                // SAFETY: buf is `total` (== requested argsz) bytes with the
                // argsz/index header set; the kernel fills the remainder.
                let ret2 = unsafe {
                    libc::ioctl(device_fd, VFIO_DEVICE_GET_REGION_INFO, buf.as_mut_ptr())
                };
                let parsed = if ret2 == 0 {
                    let cap_offset = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
                    parse_sparse_mmap(&buf, cap_offset)
                } else {
                    Vec::new()
                };
                if parsed.is_empty() {
                    // Mappable, but no sparse description: the whole region maps.
                    vec![MmapRange {
                        bar_offset: 0,
                        len: info.size,
                    }]
                } else {
                    parsed
                }
            } else {
                // Mappable with no capability chain: the whole region maps.
                vec![MmapRange {
                    bar_offset: 0,
                    len: info.size,
                }]
            };

            // Map each range into our address space at its VFIO fd offset.
            let mut mapped = Vec::new();
            for range in ranges {
                let fd_off = info.offset.wrapping_add(range.bar_offset) as libc::off_t;
                // SAFETY: device_fd is the VFIO device fd; (offset, len) come
                // from the kernel's region / sparse descriptors and are page
                // aligned, as mmap requires.
                let ptr = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        range.len as usize,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_SHARED,
                        device_fd,
                        fd_off,
                    )
                };
                if ptr == libc::MAP_FAILED {
                    tracing::debug!(
                        "BAR{} mmap failed (off=0x{:x} len=0x{:x}): {}",
                        bar_index,
                        range.bar_offset,
                        range.len,
                        std::io::Error::last_os_error()
                    );
                    continue;
                }
                mapped.push(MappedRange {
                    range,
                    ptr: ptr.cast(),
                    map_len: range.len as usize,
                });
            }

            tracing::debug!(
                "BAR{}: offset=0x{:x} size=0x{:x} flags=0x{:x} mmap_ranges={}",
                bar_index,
                info.offset,
                info.size,
                info.flags,
                mapped.len(),
            );
            regions[bar_index as usize] = Some(VfioBarRegion {
                offset: info.offset,
                size: info.size,
                mapped,
            });
        }

        *self.bar_regions.write().await = regions;
    }

    /// Read from a MMIO region
    pub async fn mmio_read(&self, bar: u8, offset: u64, size: u8) -> Result<u64> {
        if !self.is_attached() {
            return Err(GpuError::NotAvailable("Device not attached".into()));
        }

        self.stats.mmio_reads.fetch_add(1, Ordering::Relaxed);

        #[cfg(target_os = "linux")]
        {
            let bar_idx = bar as usize;
            if bar_idx >= 6 {
                return Err(GpuError::InvalidOperation(format!(
                    "Invalid BAR index: {}",
                    bar
                )));
            }

            let regions = self.bar_regions.read().await;
            let region = regions[bar_idx]
                .as_ref()
                .ok_or_else(|| GpuError::NotAvailable(format!("BAR{} region not mapped", bar)))?;

            // Validate the read is within the BAR region bounds
            let read_size = size as u64;
            if offset
                .checked_add(read_size)
                .is_none_or(|end| end > region.size)
            {
                return Err(GpuError::InvalidOperation(format!(
                    "MMIO read out of bounds: BAR{} offset=0x{:x} size={} region_size=0x{:x}",
                    bar, offset, size, region.size
                )));
            }

            // Only allow 1/2/4/8-byte reads
            if !matches!(size, 1 | 2 | 4 | 8) {
                return Err(GpuError::InvalidOperation(format!(
                    "Invalid MMIO read size: {} (must be 1, 2, 4, or 8)",
                    size
                )));
            }

            // Fast path: a direct volatile load from a mapped BAR sub-range —
            // no syscall, no lock. This is the common case once a device is up.
            for mr in &region.mapped {
                if let Some(intra) = range_contains_access(mr.range, offset, size) {
                    // SAFETY: the router guarantees intra + size <= range.len so
                    // the pointer stays within the mapping; offset is
                    // size-aligned and the mapping base is page-aligned, so the
                    // access is naturally aligned.
                    let value = unsafe { read_volatile_mmio(mr.ptr.add(intra as usize), size) };
                    return Ok(value);
                }
            }

            // Fallback: positioned read on the lock-free fd mirror (for
            // sub-ranges the kernel did not expose as mappable).
            let device_fd = self.device_fd.load(Ordering::Acquire);
            if device_fd < 0 {
                return Err(GpuError::NotAvailable(
                    "VFIO device fd not available".into(),
                ));
            }

            let mut buf = [0u8; 8];
            let file_offset = region.offset + offset;

            // SAFETY: device_fd is a valid VFIO device fd; buf holds 8 bytes and
            // read_size <= 8. pread64 is positioned I/O and thread-safe across
            // concurrent callers on the same fd.
            let bytes_read = unsafe {
                libc::pread64(
                    device_fd,
                    buf.as_mut_ptr().cast(),
                    read_size as usize,
                    file_offset as i64,
                )
            };

            if bytes_read < 0 {
                return Err(GpuError::IoError(format!(
                    "MMIO read failed: BAR{} offset=0x{:x}: {}",
                    bar,
                    offset,
                    std::io::Error::last_os_error()
                )));
            }

            // Convert bytes to value (little-endian, as PCI MMIO is LE)
            let value = match size {
                1 => buf[0] as u64,
                2 => u16::from_le_bytes([buf[0], buf[1]]) as u64,
                4 => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64,
                8 => u64::from_le_bytes(buf),
                _ => unreachable!(),
            };

            Ok(value)
        }

        #[cfg(not(target_os = "linux"))]
        {
            tracing::trace!(
                "MMIO read (simulated): BAR{} offset=0x{:x} size={}",
                bar,
                offset,
                size
            );
            // Return 0xFFFFFFFF for 4-byte reads (common PCI "not present" response)
            // and size-appropriate equivalents for other sizes
            Ok(match size {
                1 => 0xFF,
                2 => 0xFFFF,
                4 => 0xFFFF_FFFF,
                8 => 0xFFFF_FFFF_FFFF_FFFF,
                _ => 0,
            })
        }
    }

    /// Write to a MMIO region
    pub async fn mmio_write(&self, bar: u8, offset: u64, size: u8, value: u64) -> Result<()> {
        if !self.is_attached() {
            return Err(GpuError::NotAvailable("Device not attached".into()));
        }

        self.stats.mmio_writes.fetch_add(1, Ordering::Relaxed);

        #[cfg(target_os = "linux")]
        {
            let bar_idx = bar as usize;
            if bar_idx >= 6 {
                return Err(GpuError::InvalidOperation(format!(
                    "Invalid BAR index: {}",
                    bar
                )));
            }

            let regions = self.bar_regions.read().await;
            let region = regions[bar_idx]
                .as_ref()
                .ok_or_else(|| GpuError::NotAvailable(format!("BAR{} region not mapped", bar)))?;

            // Validate the write is within the BAR region bounds
            let write_size = size as u64;
            if offset
                .checked_add(write_size)
                .is_none_or(|end| end > region.size)
            {
                return Err(GpuError::InvalidOperation(format!(
                    "MMIO write out of bounds: BAR{} offset=0x{:x} size={} region_size=0x{:x}",
                    bar, offset, size, region.size
                )));
            }

            // Only allow 1/2/4/8-byte writes
            if !matches!(size, 1 | 2 | 4 | 8) {
                return Err(GpuError::InvalidOperation(format!(
                    "Invalid MMIO write size: {} (must be 1, 2, 4, or 8)",
                    size
                )));
            }

            // Fast path: a direct volatile store into a mapped BAR sub-range.
            for mr in &region.mapped {
                if let Some(intra) = range_contains_access(mr.range, offset, size) {
                    // SAFETY: see the read path — bounded, aligned, live mapping.
                    unsafe {
                        write_volatile_mmio(mr.ptr.add(intra as usize), size, value);
                    }
                    return Ok(());
                }
            }

            // Fallback: positioned write on the lock-free fd mirror.
            let device_fd = self.device_fd.load(Ordering::Acquire);
            if device_fd < 0 {
                return Err(GpuError::NotAvailable(
                    "VFIO device fd not available".into(),
                ));
            }

            // Convert value to bytes (little-endian)
            let buf: [u8; 8] = value.to_le_bytes();
            let file_offset = region.offset + offset;

            // SAFETY: device_fd is a valid VFIO device fd; buf holds 8 bytes and
            // write_size <= 8. pwrite64 is positioned, thread-safe I/O.
            let bytes_written = unsafe {
                libc::pwrite64(
                    device_fd,
                    buf.as_ptr().cast(),
                    write_size as usize,
                    file_offset as i64,
                )
            };

            if bytes_written < 0 {
                return Err(GpuError::IoError(format!(
                    "MMIO write failed: BAR{} offset=0x{:x}: {}",
                    bar,
                    offset,
                    std::io::Error::last_os_error()
                )));
            }

            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        {
            tracing::trace!(
                "MMIO write (simulated): BAR{} offset=0x{:x} size={} value=0x{:x}",
                bar,
                offset,
                size,
                value
            );
            Ok(())
        }
    }

    /// Detach the GPU from passthrough
    pub async fn detach(&self) -> Result<()> {
        if !self.is_attached() {
            return Ok(());
        }

        tracing::info!("Detaching GPU passthrough: {}", self.config.pci_address);

        #[cfg(target_os = "linux")]
        {
            // Drain any in-flight MMIO first: taking the regions write lock
            // waits for all readers (which hold the read guard across each
            // access) to finish, so nothing can touch a mapping or the fd while
            // we tear them down. Unmap every BAR range, then clear the table.
            {
                let mut regions = self.bar_regions.write().await;
                for region in regions.iter_mut().flatten() {
                    for mr in region.mapped.drain(..) {
                        // SAFETY: ptr/map_len come from a successful mmap and are
                        // unmapped exactly once here.
                        unsafe {
                            libc::munmap(mr.ptr.cast(), mr.map_len);
                        }
                    }
                }
                *regions = std::array::from_fn(|_| None);
            }

            // Retire the lock-free fd mirror before closing, so a late fallback
            // access observes -1 rather than a closed/recycled fd.
            self.device_fd.store(-1, Ordering::Release);

            // Close VFIO device
            if let Some(fd) = self.vfio_device_fd.lock().await.take() {
                // SAFETY: fd is a valid VFIO device fd from a successful ioctl call.
                unsafe { libc::close(fd) };
            }

            // Drop VFIO container
            *self.vfio_container.lock().await = None;

            // Unbind from vfio-pci
            let unbind_path = PathBuf::from("/sys/bus/pci/drivers/vfio-pci/unbind");
            let bdf = self.config.pci_address.to_bdf();
            let _ = std::fs::write(&unbind_path, &bdf);

            // Reset device if configured
            if self.config.reset_on_detach {
                let reset_path = self.config.pci_address.sysfs_path().join("reset");
                let _ = std::fs::write(&reset_path, "1");
            }

            // Rebind to original driver if allowed
            if self.config.allow_rebind {
                let rescan_path = PathBuf::from("/sys/bus/pci/rescan");
                let _ = std::fs::write(&rescan_path, "1");
            }
        }

        self.attached.store(false, Ordering::SeqCst);
        *self.state.write().await = PassthroughState::Detached;

        tracing::info!("GPU passthrough detached");
        Ok(())
    }
}

impl Drop for GpuPassthrough {
    fn drop(&mut self) {
        // Note: async drop not possible, cleanup handled by RAII
        #[cfg(target_os = "linux")]
        {
            // Unmap any BAR mappings still live (best-effort; at drop there
            // should be no concurrent accessor, so try_write succeeds).
            if let Ok(mut regions) = self.bar_regions.try_write() {
                for region in regions.iter_mut().flatten() {
                    for mr in region.mapped.drain(..) {
                        // SAFETY: ptr/map_len come from a successful mmap, unmapped once.
                        unsafe {
                            libc::munmap(mr.ptr.cast(), mr.map_len);
                        }
                    }
                }
            }
            self.device_fd.store(-1, Ordering::Release);

            // Synchronously close VFIO device fd if still open
            if let Ok(mut fd_guard) = self.vfio_device_fd.try_lock() {
                if let Some(fd) = fd_guard.take() {
                    // SAFETY: fd is a valid VFIO device fd, guarded by try_lock.
                    unsafe { libc::close(fd) };
                }
            }
        }
    }
}

/// Enumerate GPUs available for passthrough
#[cfg(target_os = "linux")]
pub fn enumerate_gpus() -> Result<Vec<GpuDeviceInfo>> {
    let mut gpus = Vec::new();

    let pci_path = PathBuf::from("/sys/bus/pci/devices");
    if let Ok(entries) = std::fs::read_dir(&pci_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let device_path = entry.path();

            // Read class to check if it's a display controller (03xxxx)
            let class_path = device_path.join("class");
            if let Ok(class_str) = std::fs::read_to_string(&class_path) {
                let class =
                    u32::from_str_radix(class_str.trim().trim_start_matches("0x"), 16).unwrap_or(0);

                // Class 0x03xxxx is display controller
                if (class >> 16) == 0x03 {
                    if let Some(name) = entry.file_name().to_str() {
                        if let Some(pci_addr) = PciAddress::from_bdf(name) {
                            let config = PassthroughConfig::new(pci_addr, 0, 0);
                            let pt = GpuPassthrough::new(config);

                            // Use a runtime to call async discover
                            if let Ok(info) = tokio::runtime::Handle::try_current()
                                .map_err(|_| GpuError::NotAvailable("No runtime".into()))
                                .and_then(|rt| rt.block_on(pt.discover()))
                            {
                                gpus.push(info);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(gpus)
}

#[cfg(not(target_os = "linux"))]
pub fn enumerate_gpus() -> Result<Vec<GpuDeviceInfo>> {
    Err(GpuError::Unsupported(
        "GPU enumeration only supported on Linux".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_contains_access_routes_aligned_in_bounds_only() {
        let r = MmapRange {
            bar_offset: 0x1000,
            len: 0x1000,
        };
        // Aligned and inside → offset within the range.
        assert_eq!(range_contains_access(r, 0x1000, 4), Some(0));
        assert_eq!(range_contains_access(r, 0x1040, 4), Some(0x40));
        // Final 8 bytes of the range.
        assert_eq!(range_contains_access(r, 0x1ff8, 8), Some(0xff8));
        // Misaligned access falls back (volatile typed load needs alignment).
        assert_eq!(range_contains_access(r, 0x1001, 4), None);
        // Spanning past the end falls back.
        assert_eq!(range_contains_access(r, 0x1ffc, 8), None);
        // Before the range falls back.
        assert_eq!(range_contains_access(r, 0x0ff8, 8), None);
    }

    #[test]
    fn parse_sparse_mmap_extracts_areas() {
        // region_info header is 32 bytes; place the capability chain after it.
        let cap = 32usize;
        let mut buf = vec![0u8; cap + 16 + 32];
        // vfio_info_cap_header { id = SPARSE_MMAP, version = 1, next = 0 }
        buf[cap..cap + 2].copy_from_slice(&VFIO_REGION_INFO_CAP_SPARSE_MMAP.to_le_bytes());
        buf[cap + 2..cap + 4].copy_from_slice(&1u16.to_le_bytes());
        buf[cap + 4..cap + 8].copy_from_slice(&0u32.to_le_bytes());
        // nr_areas = 2, reserved = 0
        buf[cap + 8..cap + 12].copy_from_slice(&2u32.to_le_bytes());
        // areas[0] = { offset: 0x1000, size: 0x2000 }
        let a0 = cap + 16;
        buf[a0..a0 + 8].copy_from_slice(&0x1000u64.to_le_bytes());
        buf[a0 + 8..a0 + 16].copy_from_slice(&0x2000u64.to_le_bytes());
        // areas[1] = { offset: 0x4000, size: 0x1000 }
        let a1 = a0 + 16;
        buf[a1..a1 + 8].copy_from_slice(&0x4000u64.to_le_bytes());
        buf[a1 + 8..a1 + 16].copy_from_slice(&0x1000u64.to_le_bytes());

        let ranges = parse_sparse_mmap(&buf, cap as u32);
        assert_eq!(
            ranges,
            vec![
                MmapRange {
                    bar_offset: 0x1000,
                    len: 0x2000
                },
                MmapRange {
                    bar_offset: 0x4000,
                    len: 0x1000
                },
            ]
        );
    }

    #[test]
    fn parse_sparse_mmap_absent_or_other_cap_is_empty() {
        // A single non-sparse capability that terminates the chain.
        let cap = 32usize;
        let mut buf = vec![0u8; cap + 8];
        buf[cap..cap + 2].copy_from_slice(&7u16.to_le_bytes()); // some other id
        buf[cap + 4..cap + 8].copy_from_slice(&0u32.to_le_bytes()); // next = 0
        assert!(parse_sparse_mmap(&buf, cap as u32).is_empty());
        // A zero cap_offset means "no capabilities".
        assert!(parse_sparse_mmap(&buf, 0).is_empty());
    }

    // Exercise the MMIO fast-path machinery (routing + volatile access) against
    // a real anonymous mmap — everything the kernel would set up except the
    // VFIO fd. Runs on Linux (incl. WSL2); needs no GPU or IOMMU.
    #[cfg(target_os = "linux")]
    #[test]
    fn volatile_mmio_roundtrips_over_real_mmap() {
        let len = 4096usize;
        // SAFETY: standard anonymous mmap; result is checked against MAP_FAILED.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
        let base = ptr.cast::<u8>();

        // A mapped BAR sub-range whose BAR offset is 0x1000.
        let range = MmapRange {
            bar_offset: 0x1000,
            len: len as u64,
        };
        let intra = range_contains_access(range, 0x1040, 4).expect("aligned in-range access");
        assert_eq!(intra, 0x40);

        // SAFETY: every offset below is width-aligned and within the mapping.
        unsafe {
            write_volatile_mmio(base.add(intra as usize), 4, 0xDEAD_BEEF);
            assert_eq!(read_volatile_mmio(base.add(intra as usize), 4), 0xDEAD_BEEF);

            write_volatile_mmio(base.add(0x80), 8, 0x1122_3344_5566_7788);
            assert_eq!(read_volatile_mmio(base.add(0x80), 8), 0x1122_3344_5566_7788);

            write_volatile_mmio(base.add(0x100), 2, 0xABCD);
            assert_eq!(read_volatile_mmio(base.add(0x100), 2), 0xABCD);

            write_volatile_mmio(base.add(0x101), 1, 0x5A);
            assert_eq!(read_volatile_mmio(base.add(0x101), 1), 0x5A);

            libc::munmap(ptr, len);
        }
    }

    #[test]
    fn test_pci_address_from_bdf() {
        let addr = PciAddress::from_bdf("0000:01:00.0").unwrap();
        assert_eq!(addr.domain, 0);
        assert_eq!(addr.bus, 1);
        assert_eq!(addr.device, 0);
        assert_eq!(addr.function, 0);

        let addr2 = PciAddress::from_bdf("01:00.0").unwrap();
        assert_eq!(addr2.domain, 0);
        assert_eq!(addr2.bus, 1);
    }

    #[test]
    fn test_pci_address_to_bdf() {
        let addr = PciAddress::new(0, 1, 0, 0);
        assert_eq!(addr.to_bdf(), "0000:01:00.0");
    }

    #[test]
    fn test_passthrough_config() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        assert_eq!(config.vendor_id, 0x10de);
        assert_eq!(config.device_id, 0x2204);
        assert!(config.iommu_enabled);
        assert!(config.vfio);
    }

    #[tokio::test]
    async fn test_gpu_passthrough_creation() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);

        assert!(!pt.is_attached());
        assert_eq!(pt.state().await, PassthroughState::Detached);
    }

    #[tokio::test]
    async fn test_mmio_read_not_attached() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);

        // Should fail because device is not attached
        let result = pt.mmio_read(0, 0, 4).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mmio_write_not_attached() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);

        // Should fail because device is not attached
        let result = pt.mmio_write(0, 0, 4, 0xDEADBEEF).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_passthrough_stats_default() {
        let stats = PassthroughStats::default();
        assert_eq!(stats.interrupts.load(Ordering::Relaxed), 0);
        assert_eq!(stats.mmio_reads.load(Ordering::Relaxed), 0);
        assert_eq!(stats.mmio_writes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.dma_transfers.load(Ordering::Relaxed), 0);
        assert_eq!(stats.dma_bytes.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_pci_address_display() {
        let addr = PciAddress::new(0, 0x3e, 0x1f, 3);
        assert_eq!(format!("{}", addr), "0000:3e:1f.3");
    }

    #[test]
    fn test_pci_address_invalid_bdf() {
        assert!(PciAddress::from_bdf("invalid").is_none());
        assert!(PciAddress::from_bdf("").is_none());
        assert!(PciAddress::from_bdf("0000:zz:00.0").is_none());
    }

    #[test]
    fn test_passthrough_state_variants() {
        assert_ne!(PassthroughState::Detached, PassthroughState::HostAttached);
        assert_ne!(PassthroughState::VfioBound, PassthroughState::PassedThrough);
        assert_ne!(PassthroughState::Error, PassthroughState::Detached);
    }

    // --- New tests below ---

    #[test]
    fn test_pci_address_new() {
        let addr = PciAddress::new(1, 2, 3, 4);
        assert_eq!(addr.domain, 1);
        assert_eq!(addr.bus, 2);
        assert_eq!(addr.device, 3);
        assert_eq!(addr.function, 4);
    }

    #[test]
    fn test_pci_address_roundtrip() {
        let addr = PciAddress::new(0, 0x3e, 0x1f, 3);
        let bdf = addr.to_bdf();
        let parsed = PciAddress::from_bdf(&bdf).unwrap();
        assert_eq!(parsed, addr);
    }

    #[test]
    fn test_pci_address_short_bdf() {
        let addr = PciAddress::from_bdf("02:03.1").unwrap();
        assert_eq!(addr.domain, 0);
        assert_eq!(addr.bus, 2);
        assert_eq!(addr.device, 3);
        assert_eq!(addr.function, 1);
    }

    #[test]
    fn test_pci_address_equality() {
        let a = PciAddress::new(0, 1, 0, 0);
        let b = PciAddress::new(0, 1, 0, 0);
        let c = PciAddress::new(0, 2, 0, 0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_pci_address_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(PciAddress::new(0, 1, 0, 0));
        set.insert(PciAddress::new(0, 1, 0, 0)); // duplicate
        set.insert(PciAddress::new(0, 2, 0, 0));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_passthrough_config_defaults() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr.clone(), 0x1002, 0x7340);
        assert_eq!(config.vendor_id, 0x1002);
        assert_eq!(config.device_id, 0x7340);
        assert!(config.iommu_enabled);
        assert!(config.vfio);
        assert!(config.reset_on_detach);
        assert!(config.allow_rebind);
        assert!(config.bar_sizes.is_empty());
        assert!(config.msix_enabled);
        assert!(config.rom_path.is_none());
        assert_eq!(config.pci_address, addr);
    }

    #[test]
    fn test_pci_bar_fields() {
        let bar = PciBar {
            index: 0,
            address: 0xFE00_0000,
            size: 0x100_0000,
            is_memory: true,
            is_64bit: true,
            prefetchable: true,
        };
        assert_eq!(bar.index, 0);
        assert_eq!(bar.size, 0x100_0000);
        assert!(bar.is_memory);
        assert!(bar.is_64bit);
        assert!(bar.prefetchable);
    }

    #[test]
    fn test_gpu_device_info_fields() {
        let info = GpuDeviceInfo {
            pci_address: PciAddress::new(0, 1, 0, 0),
            vendor_id: 0x10de,
            device_id: 0x2204,
            subsystem_vendor_id: 0x10de,
            subsystem_device_id: 0x1467,
            class_code: 0x030000,
            name: "NVIDIA RTX 3090".to_string(),
            driver: Some("nvidia".to_string()),
            iommu_group: Some(14),
            bars: vec![PciBar {
                index: 0,
                address: 0xFB00_0000,
                size: 0x100_0000,
                is_memory: true,
                is_64bit: true,
                prefetchable: true,
            }],
        };
        assert_eq!(info.vendor_id, 0x10de);
        assert_eq!(info.name, "NVIDIA RTX 3090");
        assert_eq!(info.driver.as_deref(), Some("nvidia"));
        assert_eq!(info.iommu_group, Some(14));
        assert_eq!(info.bars.len(), 1);
    }

    #[test]
    fn test_passthrough_stats_increment() {
        let stats = PassthroughStats::default();
        stats.interrupts.fetch_add(10, Ordering::Relaxed);
        stats.mmio_reads.fetch_add(50, Ordering::Relaxed);
        stats.mmio_writes.fetch_add(25, Ordering::Relaxed);
        stats.dma_transfers.fetch_add(3, Ordering::Relaxed);
        stats.dma_bytes.fetch_add(4096, Ordering::Relaxed);
        assert_eq!(stats.interrupts.load(Ordering::Relaxed), 10);
        assert_eq!(stats.mmio_reads.load(Ordering::Relaxed), 50);
        assert_eq!(stats.mmio_writes.load(Ordering::Relaxed), 25);
        assert_eq!(stats.dma_transfers.load(Ordering::Relaxed), 3);
        assert_eq!(stats.dma_bytes.load(Ordering::Relaxed), 4096);
    }

    #[tokio::test]
    async fn test_gpu_passthrough_state_default() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);
        assert_eq!(pt.state().await, PassthroughState::Detached);
    }

    #[tokio::test]
    async fn test_gpu_passthrough_pci_address() {
        let addr = PciAddress::new(0, 3, 0, 0);
        let config = PassthroughConfig::new(addr.clone(), 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);
        assert_eq!(*pt.pci_address(), addr);
    }

    #[tokio::test]
    async fn test_gpu_passthrough_device_info_before_discover() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);
        assert!(pt.device_info().await.is_none());
    }

    #[tokio::test]
    async fn test_detach_already_detached() {
        let addr = PciAddress::new(0, 1, 0, 0);
        let config = PassthroughConfig::new(addr, 0x10de, 0x2204);
        let pt = GpuPassthrough::new(config);
        // Detaching an already-detached device should be a no-op
        pt.detach().await.unwrap();
        assert_eq!(pt.state().await, PassthroughState::Detached);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_enumerate_gpus_non_linux() {
        let result = enumerate_gpus();
        assert!(result.is_err());
    }

    #[test]
    fn test_pci_address_from_bdf_too_many_parts() {
        assert!(PciAddress::from_bdf("0000:01:00.0.1").is_none());
    }

    #[test]
    fn test_passthrough_state_all_variants_distinct() {
        let variants = [
            PassthroughState::Detached,
            PassthroughState::HostAttached,
            PassthroughState::VfioBound,
            PassthroughState::PassedThrough,
            PassthroughState::Error,
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }
}
