//! GPU passthrough support (VFIO/IOMMU-based)
//!
//! This module provides GPU passthrough functionality that allows direct
//! hardware access to a GPU from within a virtual machine. It uses VFIO
//! on Linux and similar mechanisms on other platforms.

use crate::{GpuError, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
        use std::os::unix::io::RawFd;

        let path = CString::new("/dev/vfio/vfio").unwrap();
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return Err(GpuError::NotAvailable(
                "Failed to open VFIO container".into(),
            ));
        }

        // Check VFIO API version
        const VFIO_GET_API_VERSION: u64 = 0x3B64;
        const VFIO_API_VERSION: i32 = 0;
        let version = unsafe { libc::ioctl(fd, VFIO_GET_API_VERSION) };
        if version != VFIO_API_VERSION {
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

        let group_path = CString::new(format!("/dev/vfio/{}", group_id)).unwrap();
        let group_fd = unsafe { libc::open(group_path.as_ptr(), libc::O_RDWR) };
        if group_fd < 0 {
            return Err(GpuError::NotAvailable(format!(
                "Failed to open VFIO group {}",
                group_id
            )));
        }

        // Set container for group
        const VFIO_GROUP_SET_CONTAINER: u64 = 0x3B66;
        let ret = unsafe { libc::ioctl(group_fd, VFIO_GROUP_SET_CONTAINER, &self.fd) };
        if ret < 0 {
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

        let name = CString::new(device_name).unwrap();
        const VFIO_GROUP_GET_DEVICE_FD: u64 = 0x3B6A;
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
            unsafe { libc::close(group_fd) };
        }
        unsafe { libc::close(self.fd) };
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
    /// Is attached
    attached: AtomicBool,
    /// Statistics
    stats: Arc<PassthroughStats>,
    /// Interrupt handler
    interrupt_handler: RwLock<Option<Arc<dyn Fn(u32) + Send + Sync>>>,
}

impl GpuPassthrough {
    /// Create a new GPU passthrough manager
    pub fn new(config: PassthroughConfig) -> Self {
        Self {
            config,
            device_info: RwLock::new(None),
            state: RwLock::new(PassthroughState::Detached),
            #[cfg(target_os = "linux")]
            vfio_container: Mutex::new(None),
            #[cfg(target_os = "linux")]
            vfio_device_fd: Mutex::new(None),
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
            let iommu_group = std::fs::read_link(&iommu_link)
                .ok()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_string_lossy().parse::<u32>().ok());

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

    /// Read from a MMIO region
    pub async fn mmio_read(&self, bar: u8, offset: u64, size: u8) -> Result<u64> {
        if !self.is_attached() {
            return Err(GpuError::NotAvailable("Device not attached".into()));
        }

        self.stats.mmio_reads.fetch_add(1, Ordering::Relaxed);

        #[cfg(target_os = "linux")]
        {
            // In a real implementation, we would:
            // 1. Get the mapped region for the BAR
            // 2. Read the value at the offset
            // For now, return 0
            tracing::trace!("MMIO read: BAR{} offset=0x{:x} size={}", bar, offset, size);
            Ok(0)
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok(0)
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
            // In a real implementation, we would:
            // 1. Get the mapped region for the BAR
            // 2. Write the value at the offset
            tracing::trace!(
                "MMIO write: BAR{} offset=0x{:x} size={} value=0x{:x}",
                bar,
                offset,
                size,
                value
            );
            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        {
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
            // Close VFIO device
            if let Some(fd) = self.vfio_device_fd.lock().await.take() {
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
            // Synchronously close VFIO device fd if still open
            if let Ok(mut fd_guard) = self.vfio_device_fd.try_lock() {
                if let Some(fd) = fd_guard.take() {
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
}
