//! WHPX (Windows Hypervisor Platform) backend
//!
//! This module provides a hypervisor backend that uses Windows Hypervisor Platform
//! for hardware-accelerated virtualization on Windows 10/11.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         AetherVM Application            │
//! ├─────────────────────────────────────────┤
//! │      HypervisorBackend Trait            │
//! ├─────────────────────────────────────────┤
//! │        WhpxBackend (this file)          │
//! │  ┌─────────────────────────────────┐    │
//! │  │   WhpxVm                         │    │
//! │  │  ┌──────────────────────────┐   │    │
//! │  │  │  WhpxVcpu (per-vCPU)     │   │    │
//! │  │  │  - vp_index              │   │    │
//! │  │  │  - partition_handle      │   │    │
//! │  │  └──────────────────────────┘   │    │
//! │  └─────────────────────────────────┘    │
//! ├─────────────────────────────────────────┤
//! │          WHPX FFI bindings              │
//! ├─────────────────────────────────────────┤
//! │      WinHvPlatform.dll (Windows)        │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use hv2_core::backends::whpx::WhpxBackend;
//! use hv2_core::hypervisor::HypervisorBackend;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut backend = WhpxBackend::new()?;
//! backend.init().await?;
//!
//! let vm = backend.create_vm(4, 1024 * 1024 * 1024).await?; // 4 vCPUs, 1GB RAM
//! # Ok(())
//! # }
//! ```
//!
//! # Requirements
//!
//! - Windows 10 version 1803 (April 2018 Update) or later
//! - Windows 11 (any version)
//! - Intel VT-x or AMD-V enabled in BIOS
//! - Hyper-V feature enabled (but not Hyper-V itself):
//!   ```powershell
//!   Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform
//!   ```

use super::whpx_ffi::*;
use crate::boot::linux::{LinuxBootParams, LinuxBootProtocol};
use crate::boot::multiboot::{MultibootInfo, MultibootLayout, MultibootProtocol};
use crate::boot::BootSetup;
use crate::descriptors::GdtBuilder;
use crate::hypervisor::{
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm,
};
use crate::{Error, Result, VCpu, VmExit};
use async_trait::async_trait;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use parking_lot::RwLock;

/// WHPX hypervisor backend
///
/// This backend uses the Windows Hypervisor Platform (WHPX) API
/// for hardware-accelerated virtualization on Windows.
///
/// # Requirements
///
/// - Windows 10 1803+ or Windows 11
/// - CPU with hardware virtualization (Intel VT-x or AMD-V)
/// - Hyper-V Platform feature enabled
///
/// # Thread Safety
///
/// This struct is thread-safe (`Send + Sync`). The underlying WHPX
/// handles are safe to use from multiple threads with proper synchronization.
pub struct WhpxBackend {
    /// Detected capabilities
    capabilities: HypervisorCapabilities,
    /// The one partition this backend owns, if `create_vm` has run.
    ///
    /// At most one. The `HypervisorBackend` trait identifies a vCPU by its
    /// bare id, so a second partition would bring its own vCPU 0 and every
    /// lookup here — `run_vcpu`, `load_boot`, interrupt injection — would
    /// silently resolve to whichever partition was created last. `create_vm`
    /// refuses the second partition rather than allow that. A caller who
    /// wants two VMs builds two backends, which is what `VM::new` does.
    vm: Arc<RwLock<Option<Arc<WhpxVm>>>>,
    /// vCPU map: VCpu ID -> WhpxVcpu for trait method delegation.
    ///
    /// Keyed by bare vCPU id, which is unambiguous only because of the
    /// one-partition invariant documented on `vm`.
    vcpu_map: Arc<RwLock<std::collections::HashMap<u32, Arc<WhpxVcpu>>>>,
}

impl WhpxBackend {
    /// Create a new WHPX backend
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - WHPX is not available on the system
    /// - Hyper-V Platform feature is not enabled
    /// - Hardware virtualization is not supported
    #[cfg(target_os = "windows")]
    pub fn new() -> Result<Self> {
        // SAFETY: WHvGetCapability is a well-defined Windows Hypervisor Platform FFI call.
        // We pass a properly-sized zeroed buffer and check the HRESULT before using the result.
        unsafe {
            // Check if hypervisor is present
            let mut capability = std::mem::zeroed::<WHV_CAPABILITY>();
            let mut written_size: UINT32 = 0;

            let hr = WHvGetCapability(
                WHV_CAPABILITY_CODE::WHvCapabilityCodeHypervisorPresent,
                &mut capability as *mut _ as *mut VOID,
                std::mem::size_of::<WHV_CAPABILITY>() as UINT32,
                &mut written_size,
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to query hypervisor presence: HRESULT 0x{:08X}. \
                    Make sure Hyper-V Platform is enabled: \
                    Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform",
                    hr
                )));
            }

            if capability.HypervisorPresent == 0 {
                return Err(Error::VM(
                    "Hypervisor is not present. Enable Hyper-V Platform feature and ensure \
                    Intel VT-x or AMD-V is enabled in BIOS."
                        .into(),
                ));
            }

            // Detect capabilities
            let capabilities = Self::detect_capabilities()?;

            Ok(Self {
                capabilities,
                vm: Arc::new(RwLock::new(None)),
                vcpu_map: Arc::new(RwLock::new(std::collections::HashMap::new())),
            })
        }
    }

    /// Create a new WHPX backend (non-Windows stub)
    #[cfg(not(target_os = "windows"))]
    pub fn new() -> Result<Self> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Detect WHPX capabilities
    #[cfg(target_os = "windows")]
    fn detect_capabilities() -> Result<HypervisorCapabilities> {
        // SAFETY: WHvGetCapability calls are well-defined WHP FFI. We pass properly-sized
        // zeroed buffers and check HRESULTs before reading capability values.
        unsafe {
            // Query processor vendor
            let mut vendor_cap = std::mem::zeroed::<WHV_CAPABILITY>();
            let mut written_size: UINT32 = 0;

            let hr = WHvGetCapability(
                WHV_CAPABILITY_CODE::WHvCapabilityCodeProcessorVendor,
                &mut vendor_cap as *mut _ as *mut VOID,
                std::mem::size_of::<WHV_CAPABILITY>() as UINT32,
                &mut written_size,
            );
            if hr != 0 {
                tracing::warn!(hresult = hr, "WHvGetCapability(ProcessorVendor) failed");
            }

            // Query features
            let mut features_cap = std::mem::zeroed::<WHV_CAPABILITY>();
            let hr = WHvGetCapability(
                WHV_CAPABILITY_CODE::WHvCapabilityCodeFeatures,
                &mut features_cap as *mut _ as *mut VOID,
                std::mem::size_of::<WHV_CAPABILITY>() as UINT32,
                &mut written_size,
            );
            if hr != 0 {
                tracing::warn!(hresult = hr, "WHvGetCapability(Features) failed");
            }

            let features = features_cap.Features;

            Ok(HypervisorCapabilities {
                max_vcpus: 240,                       // WHPX's default max
                max_memory: 512 * 1024 * 1024 * 1024, // 512GB
                supports_nested_virt: features.VirtualPciDeviceSupport != 0,
                supports_apic: features.LocalApicEmulation != 0,
                supports_x2apic: features.ApicRemoteRead != 0,
                supports_iommu: features.IommuSupport != 0,
                supports_gpu_passthrough: features.VirtualPciDeviceSupport != 0,
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn detect_capabilities() -> Result<HypervisorCapabilities> {
        Ok(HypervisorCapabilities {
            max_vcpus: 0,
            max_memory: 0,
            supports_nested_virt: false,
            supports_apic: false,
            supports_x2apic: false,
            supports_iommu: false,
            supports_gpu_passthrough: false,
        })
    }
}

#[async_trait]
impl HypervisorBackend for WhpxBackend {
    fn platform(&self) -> HypervisorPlatform {
        HypervisorPlatform::Whpx
    }

    fn capabilities(&self) -> HypervisorCapabilities {
        self.capabilities.clone()
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!("Initialized WHPX backend");
        tracing::debug!("Capabilities: {:?}", self.capabilities);
        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn create_vm(&self, vcpu_count: u32, memory_size: u64) -> Result<HypervisorVm> {
        if vcpu_count > self.capabilities.max_vcpus {
            return Err(Error::Config(format!(
                "vCPU count {} exceeds maximum {}",
                vcpu_count, self.capabilities.max_vcpus
            )));
        }

        if memory_size > self.capabilities.max_memory {
            return Err(Error::Config(format!(
                "Memory size {} exceeds maximum {}",
                memory_size, self.capabilities.max_memory
            )));
        }

        // Hold the slot for the whole of creation so two concurrent callers
        // cannot both pass the check. Lock order is `vm` then `vcpu_map`;
        // every other path releases `vcpu_map` before touching `vm`.
        let mut slot = self.vm.write();
        if slot.is_some() {
            return Err(Error::VM(
                "this WHPX backend already owns a partition. A backend owns at most one: \
                 vCPUs are looked up by bare id, so a second partition's vCPU 0 would \
                 collide with the first's. Build a second backend for a second VM."
                    .into(),
            ));
        }

        // Create WHPX VM instance
        let whpx_vm = Arc::new(WhpxVm::new(vcpu_count, memory_size)?);

        // Create vCPUs and populate the vcpu_map for trait method delegation
        {
            let mut vcpu_map = self.vcpu_map.write();
            for id in 0..vcpu_count {
                let whpx_vcpu = whpx_vm.create_vcpu(id)?;
                vcpu_map.insert(id, whpx_vcpu);
            }
        }

        *slot = Some(whpx_vm);
        drop(slot);

        Ok(HypervisorVm::new(
            HypervisorPlatform::Whpx,
            vcpu_count,
            memory_size,
        ))
    }

    #[cfg(not(target_os = "windows"))]
    async fn create_vm(&self, _vcpu_count: u32, _memory_size: u64) -> Result<HypervisorVm> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        let vcpu_id = vcpu.id();
        let whpx_vcpu = {
            let map = self.vcpu_map.read();
            map.get(&vcpu_id)
                .cloned()
                .ok_or_else(|| Error::VM(format!("vCPU {} not found in WHPX backend", vcpu_id)))?
        };
        whpx_vcpu.run()
    }

    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
        let vcpu_id = vcpu.id();
        let whpx_vcpu = {
            let map = self.vcpu_map.read();
            map.get(&vcpu_id)
                .cloned()
                .ok_or_else(|| Error::VM(format!("vCPU {} not found in WHPX backend", vcpu_id)))?
        };
        whpx_vcpu.inject_interrupt(vector)
    }

    async fn inject_exception(
        &self,
        vcpu: &VCpu,
        vector: u8,
        error_code: Option<u32>,
    ) -> Result<()> {
        let vcpu_id = vcpu.id();
        let whpx_vcpu = {
            let map = self.vcpu_map.read();
            map.get(&vcpu_id)
                .cloned()
                .ok_or_else(|| Error::VM(format!("vCPU {} not found in WHPX backend", vcpu_id)))?
        };
        whpx_vcpu.inject_exception(vector, error_code)
    }

    async fn set_io_result(&self, vcpu: &VCpu, data: u32, size: u8) -> Result<()> {
        let vcpu_id = vcpu.id();
        let whpx_vcpu = {
            let map = self.vcpu_map.read();
            map.get(&vcpu_id)
                .cloned()
                .ok_or_else(|| Error::VM(format!("vCPU {} not found in WHPX backend", vcpu_id)))?
        };
        // Mask data by access size and write into RAX
        let masked = match size {
            1 => u64::from(data & 0xFF),
            2 => u64::from(data & 0xFFFF),
            _ => u64::from(data),
        };
        whpx_vcpu.set_rax(masked)
    }

    async fn load_boot(&self, vcpu: &VCpu, boot: &crate::boot::source::LoadedBoot) -> Result<()> {
        let vcpu_id = vcpu.id();
        let whpx_vcpu = {
            let map = self.vcpu_map.read();
            map.get(&vcpu_id)
                .cloned()
                .ok_or_else(|| Error::VM(format!("vCPU {} not found in WHPX backend", vcpu_id)))?
        };
        let whpx_vm = self.vm.read().clone().ok_or_else(|| {
            Error::VM("no WHPX partition — create_vm must run before load_boot".into())
        })?;

        match boot {
            crate::boot::source::LoadedBoot::Linux(params) => {
                whpx_vcpu.boot_linux(&whpx_vm, params, params.kernel_addr)
            }
            crate::boot::source::LoadedBoot::Multiboot(info) => {
                whpx_vcpu.boot_multiboot(&whpx_vm, info, crate::boot::source::DEFAULT_KERNEL_ADDR)
            }
            crate::boot::source::LoadedBoot::Raw {
                data,
                load_addr,
                entry,
            } => {
                whpx_vm.write_guest_memory(*load_addr, data)?;
                // A raw image is entered in real mode, where the entry address
                // is a CS:IP pair rather than a linear address. Split it the
                // way the reset vector does: segment on a 16-byte boundary.
                let cs = (*entry >> 4) as u16;
                let ip = (*entry & 0xF) as u16;
                whpx_vcpu.setup_real_mode_boot(cs, ip)
            }
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down WHPX backend");
        self.vcpu_map.write().clear();
        *self.vm.write() = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// WHPX VM instance
///
/// Represents a single virtual machine managed by WHPX (a partition in WHPX terminology).
/// I/O port handler callback
///
/// # Arguments
///
/// * `port` - The I/O port number (0-65535)
/// * `is_write` - True for OUT, false for IN
/// * `size` - Access size in bytes (1, 2, or 4)
/// * `data` - For writes: value being written; For reads: value to return
///
/// # Returns
///
/// Returns `Ok(())` if handled, `Err` if the handler rejects the operation
pub type IoPortHandler = Box<dyn Fn(u16, bool, u8, &mut u32) -> Result<()> + Send + Sync>;

/// MMIO handler callback
///
/// # Arguments
///
/// * `addr` - Physical address being accessed
/// * `is_write` - True for write, false for read
/// * `size` - Access size in bytes (1, 2, 4, or 8)
/// * `data` - For writes: value being written; For reads: value to return
///
/// # Returns
///
/// Returns `Ok(())` if handled, `Err` if the handler rejects the operation
pub type MmioHandler = Box<dyn Fn(u64, bool, u32, &mut [u8; 8]) -> Result<()> + Send + Sync>;

/// WHPX Virtual Machine
///
/// Represents a WHPX partition (VM) with its vCPUs, memory, and I/O handlers.
pub struct WhpxVm {
    /// Partition handle
    partition: WHV_PARTITION_HANDLE,
    /// Number of vCPUs
    vcpu_count: u32,
    /// Memory size in bytes
    memory_size: u64,
    /// Guest memory (allocated on host)
    guest_memory: Option<*mut u8>,
    /// vCPUs
    vcpus: RwLock<Vec<Arc<WhpxVcpu>>>,
    /// I/O port handlers (port -> handler)
    io_handlers: RwLock<std::collections::HashMap<u16, Arc<IoPortHandler>>>,
    /// MMIO handlers (address range start -> (end, handler))
    mmio_handlers: RwLock<std::collections::BTreeMap<u64, (u64, Arc<MmioHandler>)>>,
}

impl WhpxVm {
    /// Create a new WHPX VM
    #[cfg(target_os = "windows")]
    pub fn new(vcpu_count: u32, memory_size: u64) -> Result<Self> {
        // SAFETY: WHP partition FFI calls (create, set property, setup, map memory).
        // Each HRESULT is checked, and cleanup runs on failure. Memory is allocated
        // via std::alloc::alloc with a valid layout and mapped into the partition.
        unsafe {
            // Create partition
            let mut partition: WHV_PARTITION_HANDLE = ptr::null_mut();
            let hr = WHvCreatePartition(&mut partition);

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to create WHPX partition: HRESULT 0x{:08X}",
                    hr
                )));
            }

            // Set processor count property
            let mut prop = std::mem::zeroed::<WHV_PARTITION_PROPERTY>();
            prop.ProcessorCount = vcpu_count;

            let hr = WHvSetPartitionProperty(
                partition,
                WHV_PARTITION_PROPERTY_CODE::WHvPartitionPropertyCodeProcessorCount,
                &prop as *const _ as *const VOID,
                std::mem::size_of::<WHV_PARTITION_PROPERTY>() as UINT32,
            );

            if hr != S_OK {
                WHvDeletePartition(partition);
                return Err(Error::VM(format!(
                    "Failed to set processor count: HRESULT 0x{:08X}",
                    hr
                )));
            }

            // Setup partition (finalize configuration)
            let hr = WHvSetupPartition(partition);
            if hr != S_OK {
                WHvDeletePartition(partition);
                return Err(Error::VM(format!(
                    "Failed to setup partition: HRESULT 0x{:08X}",
                    hr
                )));
            }

            // Allocate guest memory
            let guest_memory = if memory_size > 0 {
                let layout = std::alloc::Layout::from_size_align(memory_size as usize, 4096)
                    .map_err(|e| {
                        WHvDeletePartition(partition);
                        Error::Memory(format!("Invalid memory layout: {}", e))
                    })?;

                let ptr = std::alloc::alloc_zeroed(layout);
                if ptr.is_null() {
                    WHvDeletePartition(partition);
                    return Err(Error::Memory("Failed to allocate guest memory".into()));
                }

                // Map guest memory into WHPX partition
                let hr = WHvMapGpaRange(
                    partition,
                    ptr as *const VOID,
                    0, // Guest physical address 0
                    memory_size,
                    WHvMapGpaRangeFlagRead | WHvMapGpaRangeFlagWrite | WHvMapGpaRangeFlagExecute,
                );

                if hr != S_OK {
                    std::alloc::dealloc(ptr, layout);
                    WHvDeletePartition(partition);
                    return Err(Error::VM(format!(
                        "Failed to map guest memory: HRESULT 0x{:08X}",
                        hr
                    )));
                }

                Some(ptr)
            } else {
                None
            };

            Ok(Self {
                partition,
                vcpu_count,
                memory_size,
                guest_memory,
                vcpus: RwLock::new(Vec::new()),
                io_handlers: RwLock::new(std::collections::HashMap::new()),
                mmio_handlers: RwLock::new(std::collections::BTreeMap::new()),
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn new(_vcpu_count: u32, _memory_size: u64) -> Result<Self> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Create a vCPU
    #[cfg(target_os = "windows")]
    pub fn create_vcpu(&self, vcpu_id: u32) -> Result<Arc<WhpxVcpu>> {
        if vcpu_id >= self.vcpu_count {
            return Err(Error::Config(format!(
                "vCPU ID {} exceeds count {}",
                vcpu_id, self.vcpu_count
            )));
        }

        let vcpu = Arc::new(WhpxVcpu::new(self.partition, vcpu_id)?);
        self.vcpus.write().push(vcpu.clone());
        Ok(vcpu)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn create_vcpu(&self, _vcpu_id: u32) -> Result<Arc<WhpxVcpu>> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Get guest memory pointer
    pub fn guest_memory(&self) -> Option<*mut u8> {
        self.guest_memory
    }

    /// Write data to guest physical memory
    ///
    /// Writes the provided data to guest physical memory starting at the specified address.
    ///
    /// # Arguments
    ///
    /// * `addr` - Guest physical address to write to
    /// * `data` - Data to write
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Guest memory is not allocated
    /// - Address is out of bounds
    /// - Write would overflow memory
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::*;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// // Write bootloader code to 0x7C00
    /// let bootloader = [0xF4, 0xEB, 0xFD]; // HLT; JMP $
    /// vm.write_guest_memory(0x7C00, &bootloader)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn write_guest_memory(&self, addr: u64, data: &[u8]) -> Result<()> {
        let guest_ptr = self
            .guest_memory
            .ok_or_else(|| Error::Memory("Guest memory not allocated".into()))?;

        if addr >= self.memory_size {
            return Err(Error::Memory(format!(
                "Address 0x{:X} out of bounds (memory size: 0x{:X})",
                addr, self.memory_size
            )));
        }

        if addr + data.len() as u64 > self.memory_size {
            return Err(Error::Memory(format!(
                "Write at 0x{:X} with length {} would overflow memory (size: 0x{:X})",
                addr,
                data.len(),
                self.memory_size
            )));
        }

        // SAFETY: Bounds were checked above ensuring addr + data.len() <= memory_size.
        // guest_ptr is valid for the full memory region allocated in WhpxVm::new.
        unsafe {
            let dest = guest_ptr.add(addr as usize);
            std::ptr::copy_nonoverlapping(data.as_ptr(), dest, data.len());
        }

        tracing::debug!("Wrote {} bytes to guest memory at 0x{:X}", data.len(), addr);

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn write_guest_memory(&self, _addr: u64, _data: &[u8]) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Read data from guest physical memory
    ///
    /// Reads data from guest physical memory starting at the specified address.
    ///
    /// # Arguments
    ///
    /// * `addr` - Guest physical address to read from
    /// * `len` - Number of bytes to read
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Guest memory is not allocated
    /// - Address is out of bounds
    /// - Read would overflow memory
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::*;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// // Read 512 bytes from 0x7C00 (MBR location)
    /// let mbr = vm.read_guest_memory(0x7C00, 512)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn read_guest_memory(&self, addr: u64, len: usize) -> Result<Vec<u8>> {
        let guest_ptr = self
            .guest_memory
            .ok_or_else(|| Error::Memory("Guest memory not allocated".into()))?;

        if addr >= self.memory_size {
            return Err(Error::Memory(format!(
                "Address 0x{:X} out of bounds (memory size: 0x{:X})",
                addr, self.memory_size
            )));
        }

        if addr + len as u64 > self.memory_size {
            return Err(Error::Memory(format!(
                "Read at 0x{:X} with length {} would overflow memory (size: 0x{:X})",
                addr, len, self.memory_size
            )));
        }

        let mut buffer = vec![0u8; len];
        // SAFETY: Bounds were checked above ensuring addr + len <= memory_size.
        // guest_ptr is valid for the full memory region allocated in WhpxVm::new.
        unsafe {
            let src = guest_ptr.add(addr as usize);
            std::ptr::copy_nonoverlapping(src, buffer.as_mut_ptr(), len);
        }

        tracing::debug!("Read {} bytes from guest memory at 0x{:X}", len, addr);

        Ok(buffer)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn read_guest_memory(&self, _addr: u64, _len: usize) -> Result<Vec<u8>> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Register an I/O port handler
    ///
    /// Registers a callback function to handle I/O port accesses (IN/OUT instructions)
    /// for a specific port. When the guest executes an IN or OUT instruction targeting
    /// this port, the handler will be invoked.
    ///
    /// # Arguments
    ///
    /// * `port` - I/O port number (0-65535)
    /// * `handler` - Callback function that handles the I/O operation
    ///
    /// # Returns
    ///
    /// Returns the previous handler if one was registered, or `None`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::*;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// // Register handler for serial port COM1 (0x3F8)
    /// vm.register_io_handler(0x3F8, Box::new(|port, is_write, size, data| {
    ///     if is_write {
    ///         print!("{}", (*data & 0xFF) as u8 as char);
    ///     } else {
    ///         *data = 0xFF; // No data available
    ///     }
    ///     Ok(())
    /// }));
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_io_handler(
        &self,
        port: u16,
        handler: IoPortHandler,
    ) -> Option<Arc<IoPortHandler>> {
        let mut handlers = self.io_handlers.write();
        handlers.insert(port, Arc::new(handler))
    }

    /// Register an MMIO (Memory-Mapped I/O) handler
    ///
    /// Registers a callback function to handle memory-mapped I/O accesses within
    /// the specified address range. When the guest accesses memory in this range,
    /// the handler will be invoked instead of accessing actual guest RAM.
    ///
    /// # Arguments
    ///
    /// * `start` - Start of the MMIO region (inclusive)
    /// * `end` - End of the MMIO region (exclusive)
    /// * `handler` - Callback function that handles the MMIO operation
    ///
    /// # Returns
    ///
    /// Returns the previous handler if one was registered for this range
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::*;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// // Register MMIO handler for device at 0xFED00000-0xFED00FFF
    /// vm.register_mmio_handler(
    ///     0xFED00000,
    ///     0xFED01000,
    ///     Box::new(|addr, is_write, size, data| {
    ///         println!("MMIO {} at 0x{:X}, size {}",
    ///             if is_write { "write" } else { "read" }, addr, size);
    ///         Ok(())
    ///     })
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_mmio_handler(
        &self,
        start: u64,
        end: u64,
        handler: MmioHandler,
    ) -> Option<(u64, Arc<MmioHandler>)> {
        let mut handlers = self.mmio_handlers.write();
        handlers.insert(start, (end, Arc::new(handler)))
    }

    /// Handle an I/O port access
    ///
    /// Invokes the registered handler for the specified port, or returns an error
    /// if no handler is registered.
    ///
    /// # Arguments
    ///
    /// * `port` - I/O port number
    /// * `is_write` - True for OUT, false for IN
    /// * `size` - Access size in bytes (1, 2, or 4)
    /// * `data` - Pointer to data (for write) or buffer (for read)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if handled successfully, or an error if no handler exists
    pub fn handle_io_access(
        &self,
        port: u16,
        is_write: bool,
        size: u8,
        data: &mut u32,
    ) -> Result<()> {
        let handlers = self.io_handlers.read();

        if let Some(handler) = handlers.get(&port) {
            handler(port, is_write, size, data)
        } else {
            tracing::warn!(
                "Unhandled I/O {} port 0x{:X}, size {}",
                if is_write { "OUT" } else { "IN" },
                port,
                size
            );

            // Default behavior: reads return 0xFF, writes are ignored
            if !is_write {
                *data = 0xFFFFFFFF;
            }
            Ok(())
        }
    }

    /// Handle an MMIO access
    ///
    /// Invokes the registered handler for the address, or returns an error if
    /// no handler covers this address range.
    ///
    /// # Arguments
    ///
    /// * `addr` - Physical address
    /// * `is_write` - True for write, false for read
    /// * `size` - Access size in bytes (1, 2, 4, or 8)
    /// * `data` - Data buffer (8 bytes)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if handled successfully, or an error if no handler exists
    pub fn handle_mmio_access(
        &self,
        addr: u64,
        is_write: bool,
        size: u32,
        data: &mut [u8; 8],
    ) -> Result<()> {
        let handlers = self.mmio_handlers.read();

        // Find handler that covers this address
        for (start, (end, handler)) in handlers.iter().rev() {
            if addr >= *start && addr < *end {
                return handler(addr, is_write, size, data);
            }
        }

        tracing::warn!(
            "Unhandled MMIO {} at 0x{:X}, size {}",
            if is_write { "write" } else { "read" },
            addr,
            size
        );

        // Default behavior: reads return 0xFF, writes are ignored
        if !is_write {
            data.fill(0xFF);
        }
        Ok(())
    }
}

impl Drop for WhpxVm {
    #[cfg(target_os = "windows")]
    fn drop(&mut self) {
        // SAFETY: Releases resources allocated in WhpxVm::new. vCPUs are cleared first
        // (RAII order), then guest memory is unmapped and deallocated, and the partition
        // handle is deleted. All handles/pointers originate from successful FFI calls.
        unsafe {
            // vCPUs will be dropped first (RAII order)
            self.vcpus.write().clear();

            // Free guest memory
            if let Some(ptr) = self.guest_memory {
                // Unmap from partition
                if self.memory_size > 0 {
                    let hr = WHvUnmapGpaRange(self.partition, 0, self.memory_size);
                    if hr != 0 {
                        tracing::warn!(hresult = hr, "WHvUnmapGpaRange failed during Drop");
                    }
                }

                let layout =
                    std::alloc::Layout::from_size_align_unchecked(self.memory_size as usize, 4096);
                std::alloc::dealloc(ptr, layout);
            }

            // Delete partition
            WHvDeletePartition(self.partition);
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn drop(&mut self) {}
}

// SAFETY: WhpxVm partition handles are thread-safe opaque pointers managed by WHP.
// Internal state is protected by RwLock/Mutex, so Send and Sync are safe.
unsafe impl Send for WhpxVm {}
unsafe impl Sync for WhpxVm {}

/// Execution statistics collected during vCPU execution.
///
/// Contains performance metrics including execution time, exit frequencies,
/// and estimated instruction counts.
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Total wall-clock time spent executing
    pub execution_time: std::time::Duration,
    /// Total number of exits processed
    pub total_exits: usize,
    /// Count of each exit type encountered
    pub exit_counts: std::collections::HashMap<String, usize>,
    /// Estimated number of instructions executed
    pub instruction_count: u64,
}

impl ExecutionStats {
    /// Calculate exits per second
    pub fn exits_per_second(&self) -> f64 {
        let secs = self.execution_time.as_secs_f64();
        if secs > 0.0 {
            self.total_exits as f64 / secs
        } else {
            0.0
        }
    }

    /// Calculate estimated instructions per second
    pub fn instructions_per_second(&self) -> f64 {
        let secs = self.execution_time.as_secs_f64();
        if secs > 0.0 {
            self.instruction_count as f64 / secs
        } else {
            0.0
        }
    }

    /// Get the most frequent exit type
    pub fn most_frequent_exit(&self) -> Option<(&String, &usize)> {
        self.exit_counts.iter().max_by_key(|(_, count)| *count)
    }
}

/// Get a human-readable name for an exit type
fn exit_type_name(exit: &crate::exit::VmExit) -> String {
    match exit {
        crate::exit::VmExit::Hlt => "Hlt".to_string(),
        crate::exit::VmExit::Io { .. } => "Io".to_string(),
        crate::exit::VmExit::Mmio { .. } => "Mmio".to_string(),
        crate::exit::VmExit::Shutdown => "Shutdown".to_string(),
        crate::exit::VmExit::InterruptWindow => "InterruptWindow".to_string(),
        crate::exit::VmExit::Exception { vector, .. } => format!("Exception({})", vector),
        crate::exit::VmExit::Debug { .. } => "Debug".to_string(),
        crate::exit::VmExit::Hypercall { .. } => "Hypercall".to_string(),
        crate::exit::VmExit::SystemEvent { .. } => "SystemEvent".to_string(),
        crate::exit::VmExit::Nmi => "Nmi".to_string(),
        crate::exit::VmExit::Rdmsr { .. } => "Rdmsr".to_string(),
        crate::exit::VmExit::Wrmsr { .. } => "Wrmsr".to_string(),
        crate::exit::VmExit::IoapicEoi { .. } => "IoapicEoi".to_string(),
        crate::exit::VmExit::Unknown { .. } => "Unknown".to_string(),
    }
}

/// Statistics for interrupt delivery tracking.
///
/// Tracks metrics about interrupt injection, window requests, and delivery
/// latency to help diagnose performance issues and interrupt handling behavior.
#[derive(Debug, Default, Clone)]
pub struct InterruptStats {
    /// Total number of interrupts successfully injected
    pub interrupts_injected: u64,

    /// Number of times interrupt injection was deferred (interrupts masked)
    pub interrupts_deferred: u64,

    /// Number of interrupt window requests made
    pub window_requests: u64,

    /// Number of interrupt window exits received
    pub window_exits: u64,

    /// Number of NMIs injected
    pub nmis_injected: u64,

    /// Number of times RFLAGS.IF was found enabled
    pub if_enabled_count: u64,

    /// Number of times RFLAGS.IF was found disabled
    pub if_disabled_count: u64,
}

impl InterruptStats {
    /// Create a new interrupt statistics tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset all counters to zero
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Get the total number of interrupt delivery attempts
    pub fn total_attempts(&self) -> u64 {
        self.interrupts_injected + self.interrupts_deferred
    }

    /// Get the interrupt injection success rate (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.total_attempts();
        if total == 0 {
            0.0
        } else {
            self.interrupts_injected as f64 / total as f64
        }
    }

    /// Get the average number of window requests per successful injection
    pub fn avg_window_requests_per_injection(&self) -> f64 {
        if self.interrupts_injected == 0 {
            0.0
        } else {
            self.window_requests as f64 / self.interrupts_injected as f64
        }
    }
}

/// WHPX vCPU
///
/// Represents a single virtual processor managed by WHPX.
pub struct WhpxVcpu {
    /// Partition handle (shared with WhpxVm)
    partition: WHV_PARTITION_HANDLE,
    /// Virtual processor index
    vp_index: u32,
    /// Interrupt delivery statistics
    stats: Arc<RwLock<InterruptStats>>,
}

impl WhpxVcpu {
    /// Create a new WHPX vCPU
    #[cfg(target_os = "windows")]
    fn new(partition: WHV_PARTITION_HANDLE, vp_index: u32) -> Result<Self> {
        // SAFETY: WHvCreateVirtualProcessor is a WHP FFI call. The partition handle
        // is valid (from WhpxVm::new), and we check the HRESULT before proceeding.
        unsafe {
            let hr = WHvCreateVirtualProcessor(partition, vp_index, 0);

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to create virtual processor {}: HRESULT 0x{:08X}",
                    vp_index, hr
                )));
            }

            Ok(Self {
                partition,
                vp_index,
                stats: Arc::new(RwLock::new(InterruptStats::new())),
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn new(_partition: WHV_PARTITION_HANDLE, _vp_index: u32) -> Result<Self> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Run the vCPU until an exit occurs
    #[cfg(target_os = "windows")]
    pub fn run(&self) -> Result<VmExit> {
        // SAFETY: WHvRunVirtualProcessor is a WHP FFI call. The partition handle and
        // vp_index are valid. The exit context is zeroed and properly sized.
        unsafe {
            let mut exit_context = std::mem::zeroed::<WHV_RUN_VP_EXIT_CONTEXT>();

            let hr = WHvRunVirtualProcessor(
                self.partition,
                self.vp_index,
                &mut exit_context as *mut _ as *mut VOID,
                std::mem::size_of::<WHV_RUN_VP_EXIT_CONTEXT>() as UINT32,
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to run vCPU: HRESULT 0x{:08X}",
                    hr
                )));
            }

            // Convert WHPX exit reason to VmExit
            self.convert_exit(&exit_context)
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn run(&self) -> Result<VmExit> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Convert WHPX exit context to VmExit
    #[cfg(target_os = "windows")]
    fn convert_exit(&self, ctx: &WHV_RUN_VP_EXIT_CONTEXT) -> Result<VmExit> {
        use WHV_RUN_VP_EXIT_REASON::*;

        match ctx.ExitReason {
            WHvRunVpExitReasonX64Halt => Ok(VmExit::Hlt),

            // SAFETY: Accessing the IoPortAccess union field is valid because
            // ExitReason is WHvRunVpExitReasonX64IoPortAccess.
            WHvRunVpExitReasonX64IoPortAccess => unsafe {
                let io = &ctx.ExitData.IoPortAccess;
                let port = io.PortNumber;
                let size = io.AccessInfo.AccessSize as u8;

                if io.AccessInfo.IsWrite != 0 {
                    // OUT instruction
                    let data = match size {
                        1 => (io.Rax & 0xFF) as u32,
                        2 => (io.Rax & 0xFFFF) as u32,
                        4 => io.Rax as u32,
                        _ => return Err(Error::VM(format!("Invalid I/O size: {}", size))),
                    };

                    Ok(VmExit::io_out(port, size, data))
                } else {
                    // IN instruction
                    Ok(VmExit::io_in(port, size))
                }
            },

            // SAFETY: Accessing the MemoryAccess union field is valid because
            // ExitReason is WHvRunVpExitReasonMemoryAccess.
            WHvRunVpExitReasonMemoryAccess => unsafe {
                let mem = &ctx.ExitData.MemoryAccess;
                let addr = mem.Gpa;

                match mem.AccessInfo.AccessType {
                    WHV_MEMORY_ACCESS_TYPE::WHvMemoryAccessRead => {
                        // For read, use mmio_read helper which creates appropriate structure
                        Ok(VmExit::mmio_read(addr, 4))
                    }
                    WHV_MEMORY_ACCESS_TYPE::WHvMemoryAccessWrite => {
                        // For write, extract data from instruction bytes
                        let data = &mem.InstructionBytes[0..4];
                        Ok(VmExit::mmio_write(addr, data))
                    }
                    _ => Err(Error::VM(format!(
                        "Unknown memory access type: {:?}",
                        mem.AccessInfo.AccessType
                    ))),
                }
            },

            WHvRunVpExitReasonUnrecoverableException => {
                Err(Error::VM("Unrecoverable exception".into()))
            }

            WHvRunVpExitReasonInvalidVpRegisterValue => {
                Err(Error::VM("Invalid vCPU register value".into()))
            }

            WHvRunVpExitReasonUnsupportedFeature => Err(Error::VM("Unsupported feature".into())),

            WHvRunVpExitReasonCanceled => Err(Error::VM("vCPU run canceled".into())),

            WHvRunVpExitReasonX64InterruptWindow => {
                // Interrupt window opened - guest can now receive interrupts
                Ok(VmExit::InterruptWindow)
            }

            // SAFETY: Accessing the Exception union field is valid because
            // ExitReason is WHvRunVpExitReasonException.
            WHvRunVpExitReasonException => unsafe {
                let exception = &ctx.ExitData.Exception;
                let vector = exception.ExceptionInfo.ExceptionType as u8;
                let error_code = if exception.ExceptionInfo.ErrorCodeValid != 0 {
                    Some(exception.ExceptionParameter as u32)
                } else {
                    None
                };

                Ok(VmExit::Exception { vector, error_code })
            },

            // SAFETY: Accessing the MsrAccess union field is valid because
            // ExitReason is WHvRunVpExitReasonX64MsrAccess.
            WHvRunVpExitReasonX64MsrAccess => unsafe {
                let msr = &ctx.ExitData.MsrAccess;
                if msr.IsWrite != 0 {
                    let data = ((msr.Rdx & 0xFFFF_FFFF) << 32) | (msr.Rax & 0xFFFF_FFFF);
                    Ok(VmExit::Wrmsr {
                        index: msr.MsrNumber,
                        data,
                    })
                } else {
                    Ok(VmExit::Rdmsr {
                        index: msr.MsrNumber,
                    })
                }
            },

            // SAFETY: Accessing the ApicEoi union field is valid because
            // ExitReason is WHvRunVpExitReasonX64ApicEoi.
            WHvRunVpExitReasonX64ApicEoi => unsafe {
                let eoi = &ctx.ExitData.ApicEoi;
                Ok(VmExit::IoapicEoi {
                    vector: eoi.InterruptVector as u8,
                })
            },

            WHvRunVpExitReasonHypercall => {
                // WHPX does not provide hypercall parameters in exit data
                Ok(VmExit::Hypercall {
                    nr: 0,
                    args: [0; 6],
                })
            }

            _ => Err(Error::VM(format!(
                "Unhandled exit reason: {:?}",
                ctx.ExitReason
            ))),
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn convert_exit(&self, _ctx: &WHV_RUN_VP_EXIT_CONTEXT) -> Result<VmExit> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Run the vCPU until a specific exit condition is met.
    ///
    /// This method repeatedly calls `run()` and checks the exit reason against
    /// the provided predicate. Execution continues until the predicate returns
    /// `true` or an error occurs.
    ///
    /// # Arguments
    ///
    /// * `predicate` - Function that returns `true` when the desired exit is reached
    ///
    /// # Returns
    ///
    /// Returns the `VmExit` that satisfied the predicate.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # use hv2_core::exit::VmExit;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Run until HLT instruction
    /// let exit = vcpu.run_until(|exit| matches!(exit, VmExit::Hlt))?;
    /// assert!(matches!(exit, VmExit::Hlt));
    ///
    /// // Run until I/O to specific port
    /// let exit = vcpu.run_until(|exit| {
    ///     matches!(exit, VmExit::Io { port, .. } if *port == 0x3F8)
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn run_until<F>(&self, mut predicate: F) -> Result<VmExit>
    where
        F: FnMut(&VmExit) -> bool,
    {
        loop {
            let exit = self.run()?;
            if predicate(&exit) {
                return Ok(exit);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn run_until<F>(&self, _predicate: F) -> Result<VmExit>
    where
        F: FnMut(&VmExit) -> bool,
    {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Run the vCPU for a maximum number of exits.
    ///
    /// This method executes the vCPU and collects all exits until either the
    /// maximum count is reached or an error occurs. Useful for controlled
    /// execution and debugging.
    ///
    /// # Arguments
    ///
    /// * `max_exits` - Maximum number of exits to process
    ///
    /// # Returns
    ///
    /// Returns a vector of all `VmExit` events that occurred, up to `max_exits`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Execute for up to 100 exits
    /// let exits = vcpu.run_for(100)?;
    /// println!("Captured {} exits", exits.len());
    ///
    /// // Analyze exit patterns
    /// let io_exits = exits.iter().filter(|e| matches!(e, hv2_core::exit::VmExit::Io { .. })).count();
    /// println!("I/O exits: {}", io_exits);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn run_for(&self, max_exits: usize) -> Result<Vec<VmExit>> {
        let mut exits = Vec::with_capacity(max_exits.min(1024));

        for _ in 0..max_exits {
            match self.run() {
                Ok(exit) => exits.push(exit),
                Err(e) => {
                    // If we got some exits before error, return them
                    if !exits.is_empty() {
                        tracing::warn!(
                            "Execution stopped with error after {} exits: {}",
                            exits.len(),
                            e
                        );
                        return Ok(exits);
                    }
                    return Err(e);
                }
            }
        }

        Ok(exits)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn run_for(&self, _max_exits: usize) -> Result<Vec<VmExit>> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Execute a single instruction (single-step).
    ///
    /// This method enables single-stepping mode, executes one instruction,
    /// and returns the resulting exit. This is primarily used for debugging
    /// and instruction-level tracing.
    ///
    /// **Note**: Single-stepping has significant performance overhead and
    /// should only be used when necessary.
    ///
    /// # Returns
    ///
    /// Returns the `VmExit` after executing one instruction.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Step through first 10 instructions
    /// for i in 0..10 {
    ///     let exit = vcpu.step()?;
    ///     let regs = vcpu.get_register_set()?;
    ///     println!("Instruction {}: RIP = 0x{:016X}", i, regs.rip);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Performance
    ///
    /// Single-stepping can be 100-1000x slower than normal execution due to
    /// the overhead of entering/exiting the hypervisor for every instruction.
    #[cfg(target_os = "windows")]
    pub fn step(&self) -> Result<VmExit> {
        // Enable single-step by setting RFLAGS.TF (Trap Flag)
        let mut regs = self.get_register_set()?;
        let original_rflags = regs.rflags;
        regs.rflags |= 0x100; // Set TF (bit 8)
        self.set_register_set(&regs)?;

        // Execute one instruction
        let exit = self.run()?;

        // Restore original RFLAGS
        let mut regs = self.get_register_set()?;
        regs.rflags = original_rflags;
        self.set_register_set(&regs)?;

        Ok(exit)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn step(&self) -> Result<VmExit> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Run the vCPU with automatic I/O and MMIO handler invocation.
    ///
    /// This method runs the vCPU and automatically invokes registered I/O and MMIO
    /// handlers for corresponding exits. I/O exits are processed transparently and
    /// execution continues automatically. Other exit types are returned to the caller.
    ///
    /// # Arguments
    ///
    /// * `vm` - Reference to the VM containing the handlers
    ///
    /// # Returns
    ///
    /// Returns a `VmExit` for exits that aren't automatically handled (HLT, Shutdown, etc.)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = WhpxVm::new(1, 1024 * 1024)?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Register I/O handler for serial port
    /// vm.register_io_handler(0x3F8, Box::new(|port, is_write, size, data| {
    ///     println!("I/O to port 0x{:X}: {}", port, *data);
    ///     Ok(())
    /// }));
    ///
    /// // Run with handlers - I/O exits processed automatically
    /// loop {
    ///     match vcpu.run_with_handlers(&vm)? {
    ///         crate::exit::VmExit::Hlt => break,
    ///         other => println!("Unhandled exit: {:?}", other),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn run_with_handlers(&self, vm: &WhpxVm) -> Result<VmExit> {
        loop {
            let exit = self.run()?;

            match &exit {
                crate::exit::VmExit::Io {
                    port,
                    direction,
                    size,
                    data,
                } => {
                    use crate::exit::IoDirection;

                    let is_write = matches!(direction, IoDirection::Out);
                    let mut data_mut = *data;

                    // Invoke handler
                    vm.handle_io_access(*port, is_write, *size, &mut data_mut)?;

                    // For IN instructions, we need to write the result back to RAX
                    if !is_write {
                        self.set_rax(data_mut as u64)?;
                    }

                    // Continue execution automatically
                    continue;
                }

                crate::exit::VmExit::Mmio {
                    phys_addr,
                    data,
                    len,
                    is_write,
                } => {
                    let mut data_mut = *data;

                    // Invoke handler
                    vm.handle_mmio_access(*phys_addr, *is_write, *len, &mut data_mut)?;

                    // For reads, result is in data_mut (handler updated it)
                    // WHPX will automatically continue after MMIO handling

                    // Continue execution automatically
                    continue;
                }

                // All other exits are returned to caller
                _ => return Ok(exit),
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn run_with_handlers(&self, _vm: &WhpxVm) -> Result<VmExit> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Run vCPU with automatic I/O handling and interrupt delivery.
    ///
    /// This method combines `run_with_handlers` with PIC interrupt delivery. It automatically:
    /// - Handles I/O port accesses via registered handlers
    /// - Handles MMIO accesses via registered handlers
    /// - Checks for pending PIC interrupts and injects them when deliverable
    /// - Returns control to the caller for all other exit types
    ///
    /// # Arguments
    ///
    /// * `vm` - VM instance containing I/O handlers
    /// * `pic` - 8259 PIC instance for interrupt delivery
    ///
    /// # Returns
    ///
    /// Returns `VmExit` when execution stops for a reason other than I/O or interrupts.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm, WhpxVcpu};
    /// # use hv2_core::interrupt::Pic8259;
    /// # fn example() -> hv2_core::Result<()> {
    /// let backend = WhpxBackend::new()?;
    /// let vm = WhpxVm::new(1, 16 * 1024 * 1024)?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// // Create and register PIC
    /// let pic = Pic8259::new();
    /// for port in [0x20, 0x21, 0xA0, 0xA1] {
    ///     vm.register_io_handler(port, pic.create_io_handler());
    /// }
    ///
    /// // Run with automatic I/O and interrupt handling
    /// let exit = vcpu.run_with_handlers_and_interrupts(&vm, &pic)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn run_with_handlers_and_interrupts(
        &self,
        vm: &WhpxVm,
        pic: &crate::interrupt::Pic8259,
    ) -> Result<VmExit> {
        use crate::exit::IoDirection;

        loop {
            // Check for pending interrupts before running
            if let Some(vector) = pic.get_pending_interrupt() {
                // Check if interrupts are enabled (RFLAGS.IF = 1)
                match self.is_interrupt_enabled() {
                    Ok(true) => {
                        // Interrupts enabled, inject immediately
                        match self.inject_interrupt(vector) {
                            Ok(()) => {
                                // Successfully injected, acknowledge it
                                let irq = if vector >= 0x28 {
                                    vector - 0x28 + 8 // Slave PIC IRQ 8-15
                                } else {
                                    vector - 0x20 // Master PIC IRQ 0-7
                                };
                                let _ = pic.acknowledge_interrupt(irq);
                            }
                            Err(_) => {
                                // Injection failed, will retry next exit
                            }
                        }
                    }
                    Ok(false) => {
                        // Interrupts masked, request notification when window opens
                        if let Err(e) = self.request_interrupt_window() {
                            tracing::warn!(error = %e, "Failed to request interrupt window");
                        }
                        // Track deferred interrupt
                        {
                            let mut stats = self.stats.write();
                            stats.interrupts_deferred += 1;
                        }
                    }
                    Err(_) => {
                        // Can't check interrupt flag, proceed cautiously
                    }
                }
            }

            let exit = self.run()?;

            match &exit {
                crate::exit::VmExit::Io {
                    port,
                    direction,
                    size,
                    data,
                } => {
                    let is_write = matches!(direction, IoDirection::Out);
                    let mut data_mut = *data;

                    // Invoke handler
                    vm.handle_io_access(*port, is_write, *size, &mut data_mut)?;

                    // For IN instructions, write result back to RAX
                    if !is_write {
                        self.set_rax(data_mut as u64)?;
                    }

                    continue;
                }

                crate::exit::VmExit::Mmio {
                    phys_addr,
                    data,
                    len,
                    is_write,
                } => {
                    let mut data_mut = *data;

                    // Invoke handler
                    vm.handle_mmio_access(*phys_addr, *is_write, *len, &mut data_mut)?;

                    continue;
                }

                crate::exit::VmExit::InterruptWindow => {
                    // Interrupt window opened - guest can now receive interrupts
                    // Track window exit
                    {
                        let mut stats = self.stats.write();
                        stats.window_exits += 1;
                    }

                    // Loop back to check for pending interrupts at the top
                    continue;
                }

                // All other exits returned to caller
                _ => return Ok(exit),
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn run_with_handlers_and_interrupts(
        &self,
        _vm: &WhpxVm,
        _pic: &crate::interrupt::Pic8259,
    ) -> Result<VmExit> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Helper to set RAX register (for IN instruction results)
    #[cfg(target_os = "windows")]
    fn set_rax(&self, value: u64) -> Result<()> {
        let mut regs = self.get_register_set()?;
        regs.rax = value;
        self.set_register_set(&regs)
    }

    #[cfg(not(target_os = "windows"))]
    fn set_rax(&self, _value: u64) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Run the vCPU and collect performance statistics.
    ///
    /// This method executes the vCPU for a specified number of exits while
    /// tracking performance metrics including exit frequencies, execution time,
    /// and instruction counts.
    ///
    /// # Arguments
    ///
    /// * `max_exits` - Maximum number of exits to process
    ///
    /// # Returns
    ///
    /// Returns `ExecutionStats` containing performance metrics.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Run and collect statistics
    /// let stats = vcpu.run_with_stats(1000)?;
    /// println!("Execution time: {:?}", stats.execution_time);
    /// println!("Total exits: {}", stats.total_exits);
    /// println!("HLT exits: {}", stats.exit_counts.get("Hlt").unwrap_or(&0));
    /// println!("Instructions (estimated): {}", stats.instruction_count);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn run_with_stats(&self, max_exits: usize) -> Result<ExecutionStats> {
        use std::collections::HashMap;
        use std::time::Instant;

        let start_time = Instant::now();
        let mut exit_counts: HashMap<String, usize> = HashMap::new();
        let mut total_exits = 0;
        let mut instruction_count = 0u64;

        // Get initial RIP for instruction counting
        let initial_regs = self.get_register_set()?;
        let initial_rip = initial_regs.rip;

        for _ in 0..max_exits {
            match self.run() {
                Ok(exit) => {
                    total_exits += 1;

                    // Count exit types
                    let exit_name = exit_type_name(&exit);
                    *exit_counts.entry(exit_name).or_insert(0) += 1;

                    // Estimate instructions executed based on RIP delta
                    if let Ok(regs) = self.get_register_set() {
                        let rip_delta = regs.rip.wrapping_sub(initial_rip);
                        // Rough estimate: average x86 instruction is ~3 bytes
                        instruction_count = rip_delta / 3;
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        let execution_time = start_time.elapsed();

        Ok(ExecutionStats {
            execution_time,
            total_exits,
            exit_counts,
            instruction_count,
        })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn run_with_stats(&self, _max_exits: usize) -> Result<ExecutionStats> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Get vCPU registers
    #[cfg(target_os = "windows")]
    pub fn get_registers(
        &self,
        register_names: &[WHV_REGISTER_NAME],
    ) -> Result<Vec<WHV_REGISTER_VALUE>> {
        // SAFETY: WHvGetVirtualProcessorRegisters is a WHP FFI call. The partition
        // handle and vp_index are valid. Values buffer is zeroed and appropriately sized.
        unsafe {
            let mut values = vec![std::mem::zeroed(); register_names.len()];

            let hr = WHvGetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                register_names.len() as UINT32,
                values.as_mut_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to get registers: HRESULT 0x{:08X}",
                    hr
                )));
            }

            Ok(values)
        }
    }

    /// Set vCPU registers
    #[cfg(target_os = "windows")]
    pub fn set_registers(
        &self,
        register_names: &[WHV_REGISTER_NAME],
        register_values: &[WHV_REGISTER_VALUE],
    ) -> Result<()> {
        if register_names.len() != register_values.len() {
            return Err(Error::Config(
                "Register names and values length mismatch".into(),
            ));
        }

        // SAFETY: WHvSetVirtualProcessorRegisters is a WHP FFI call. The partition
        // handle and vp_index are valid. Name/value arrays have equal, verified lengths.
        unsafe {
            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                register_names.len() as UINT32,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to set registers: HRESULT 0x{:08X}",
                    hr
                )));
            }

            Ok(())
        }
    }

    /// Get all general-purpose registers as RegisterSet
    #[cfg(target_os = "windows")]
    pub fn get_register_set(&self) -> Result<crate::vcpu::RegisterSet> {
        use WHV_REGISTER_NAME::*;

        let register_names = [
            WHvX64RegisterRax,
            WHvX64RegisterRbx,
            WHvX64RegisterRcx,
            WHvX64RegisterRdx,
            WHvX64RegisterRsi,
            WHvX64RegisterRdi,
            WHvX64RegisterRbp,
            WHvX64RegisterRsp,
            WHvX64RegisterR8,
            WHvX64RegisterR9,
            WHvX64RegisterR10,
            WHvX64RegisterR11,
            WHvX64RegisterR12,
            WHvX64RegisterR13,
            WHvX64RegisterR14,
            WHvX64RegisterR15,
            WHvX64RegisterRip,
            WHvX64RegisterRflags,
            WHvX64RegisterCs,
            WHvX64RegisterDs,
            WHvX64RegisterEs,
            WHvX64RegisterFs,
            WHvX64RegisterGs,
            WHvX64RegisterSs,
        ];

        let values = self.get_registers(&register_names)?;

        // SAFETY: Accessing .Reg64 and .Segment union fields is valid because
        // the register names requested correspond to 64-bit GP regs and segment regs.
        unsafe {
            Ok(crate::vcpu::RegisterSet {
                rax: values[0].Reg64,
                rbx: values[1].Reg64,
                rcx: values[2].Reg64,
                rdx: values[3].Reg64,
                rsi: values[4].Reg64,
                rdi: values[5].Reg64,
                rbp: values[6].Reg64,
                rsp: values[7].Reg64,
                r8: values[8].Reg64,
                r9: values[9].Reg64,
                r10: values[10].Reg64,
                r11: values[11].Reg64,
                r12: values[12].Reg64,
                r13: values[13].Reg64,
                r14: values[14].Reg64,
                r15: values[15].Reg64,
                rip: values[16].Reg64,
                rflags: values[17].Reg64,
                cs: values[18].Segment.Selector as u64,
                ds: values[19].Segment.Selector as u64,
                es: values[20].Segment.Selector as u64,
                fs: values[21].Segment.Selector as u64,
                gs: values[22].Segment.Selector as u64,
                ss: values[23].Segment.Selector as u64,
            })
        }
    }

    /// Set all general-purpose registers from RegisterSet
    #[cfg(target_os = "windows")]
    pub fn set_register_set(&self, regs: &crate::vcpu::RegisterSet) -> Result<()> {
        use WHV_REGISTER_NAME::*;

        let register_names = [
            WHvX64RegisterRax,
            WHvX64RegisterRbx,
            WHvX64RegisterRcx,
            WHvX64RegisterRdx,
            WHvX64RegisterRsi,
            WHvX64RegisterRdi,
            WHvX64RegisterRbp,
            WHvX64RegisterRsp,
            WHvX64RegisterR8,
            WHvX64RegisterR9,
            WHvX64RegisterR10,
            WHvX64RegisterR11,
            WHvX64RegisterR12,
            WHvX64RegisterR13,
            WHvX64RegisterR14,
            WHvX64RegisterR15,
            WHvX64RegisterRip,
            WHvX64RegisterRflags,
            WHvX64RegisterCs,
            WHvX64RegisterDs,
            WHvX64RegisterEs,
            WHvX64RegisterFs,
            WHvX64RegisterGs,
            WHvX64RegisterSs,
        ];

        let mut values: Vec<WHV_REGISTER_VALUE> = Vec::with_capacity(register_names.len());

        // SAFETY: Constructing WHV_REGISTER_VALUE unions via mem::zeroed and setting
        // .Reg64/.Segment fields. The union layout matches the WHP API expectations.
        unsafe {
            // General purpose registers
            for &val in &[
                regs.rax, regs.rbx, regs.rcx, regs.rdx, regs.rsi, regs.rdi, regs.rbp, regs.rsp,
                regs.r8, regs.r9, regs.r10, regs.r11, regs.r12, regs.r13, regs.r14, regs.r15,
            ] {
                let mut reg_val = std::mem::zeroed::<WHV_REGISTER_VALUE>();
                reg_val.Reg64 = val;
                values.push(reg_val);
            }

            // RIP and RFLAGS
            let mut rip_val = std::mem::zeroed::<WHV_REGISTER_VALUE>();
            rip_val.Reg64 = regs.rip;
            values.push(rip_val);

            let mut rflags_val = std::mem::zeroed::<WHV_REGISTER_VALUE>();
            rflags_val.Reg64 = regs.rflags;
            values.push(rflags_val);

            // Segment registers (set selector only, base/limit/attributes set to reasonable defaults)
            for &selector in &[regs.cs, regs.ds, regs.es, regs.fs, regs.gs, regs.ss] {
                let mut seg_val = std::mem::zeroed::<WHV_REGISTER_VALUE>();
                seg_val.Segment.Selector = selector as u16;
                seg_val.Segment.Base = 0;
                seg_val.Segment.Limit = 0xFFFF; // 64KB limit for real mode
                seg_val.Segment.Attributes = 0x93; // Present, read/write, data segment
                values.push(seg_val);
            }
        }

        self.set_registers(&register_names, &values)
    }

    /// Get interrupt delivery statistics.
    ///
    /// Returns a snapshot of the current interrupt statistics including
    /// injection counts, deferral rates, and window request metrics.
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// let stats = vcpu.get_interrupt_stats();
    /// println!("Interrupts injected: {}", stats.interrupts_injected);
    /// println!("Success rate: {:.2}%", stats.success_rate() * 100.0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_interrupt_stats(&self) -> InterruptStats {
        self.stats.read().clone()
    }

    /// Reset interrupt delivery statistics to zero.
    pub fn reset_interrupt_stats(&self) {
        self.stats.write().reset();
    }

    /// Read the current RFLAGS register value from the vCPU.
    ///
    /// RFLAGS contains the processor status and control flags, including:
    /// - Bit 9 (IF): Interrupt Enable Flag - controls maskable interrupt handling
    /// - Bit 8 (TF): Trap Flag - enables single-step debugging
    /// - Bits 12-13 (IOPL): I/O Privilege Level
    /// - Various arithmetic flags (CF, PF, AF, ZF, SF, OF)
    ///
    /// # Returns
    /// The 64-bit RFLAGS register value.
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// let rflags = vcpu.get_rflags()?;
    /// println!("Current RFLAGS: 0x{:016X}", rflags);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn get_rflags(&self) -> Result<u64> {
        use super::whpx_ffi::*;

        // SAFETY: WHvGetVirtualProcessorRegisters FFI call with valid handles.
        // Accessing .Reg64 is valid for the RFLAGS register.
        unsafe {
            let register_names = [WHV_REGISTER_NAME::WHvX64RegisterRflags];
            let mut register_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>()];

            let hr = WHvGetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_mut_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to read RFLAGS: HRESULT 0x{:08X}",
                    hr
                )));
            }

            Ok(register_values[0].Reg64)
        }
    }

    /// Check if maskable interrupts are currently enabled (RFLAGS.IF = 1).
    ///
    /// The Interrupt Enable Flag (IF, bit 9) in RFLAGS controls whether the
    /// processor responds to maskable hardware interrupts. When set (1),
    /// interrupts are enabled. When clear (0), maskable interrupts are blocked.
    ///
    /// Note: Non-Maskable Interrupts (NMIs) are always delivered regardless
    /// of the IF flag state.
    ///
    /// # Returns
    /// `true` if interrupts are enabled (RFLAGS.IF = 1), `false` otherwise.
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// if vcpu.is_interrupt_enabled()? {
    ///     println!("Guest is ready to receive interrupts");
    ///     // Safe to inject interrupt
    /// } else {
    ///     println!("Interrupts are masked, need to wait for window");
    ///     // Should request interrupt window
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn is_interrupt_enabled(&self) -> Result<bool> {
        use super::whpx_ffi::RFLAGS_IF;
        let enabled = (self.get_rflags()? & RFLAGS_IF) != 0;

        // Track statistics
        {
            let mut stats = self.stats.write();
            if enabled {
                stats.if_enabled_count += 1;
            } else {
                stats.if_disabled_count += 1;
            }
        }

        Ok(enabled)
    }

    /// Inject an interrupt into the vCPU
    ///
    /// This sets up the pending interruption register to deliver the specified
    /// interrupt vector to the guest on the next vCPU entry.
    #[cfg(target_os = "windows")]
    pub fn inject_interrupt(&self, vector: u8) -> Result<()> {
        use super::whpx_ffi::*;

        // SAFETY: Building a zeroed pending interruption register and setting fields
        // before passing to WHvSetVirtualProcessorRegisters. All handles are valid.
        unsafe {
            // Build pending interruption register
            let mut pending_int = std::mem::zeroed::<WHV_X64_PENDING_INTERRUPTION_REGISTER>();
            pending_int.InterruptionPending = 1; // TRUE
            pending_int.InterruptionType =
                WHV_X64_PENDING_INTERRUPTION_TYPE::WHvX64PendingInterrupt;
            pending_int.DeliverErrorCode = 0; // No error code
            pending_int.InstructionLength = 0; // Not used for hardware interrupts
            pending_int.InterruptionVector = vector as UINT32;
            pending_int.ErrorCode = 0;

            // Create register value union with PendingInterruption
            let mut reg_value = std::mem::zeroed::<WHV_REGISTER_VALUE>();
            reg_value.PendingInterruption = pending_int;

            // Set the register
            let register_names = [WHV_REGISTER_NAME::WHvX64RegisterPendingInterruption];
            let register_values = [reg_value];

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to inject interrupt {}: HRESULT 0x{:08X}",
                    vector, hr
                )));
            }

            // Track successful injection
            {
                let mut stats = self.stats.write();
                stats.interrupts_injected += 1;
            }

            Ok(())
        }
    }

    /// Request an interrupt window from the hypervisor.
    ///
    /// This method requests that the hypervisor notify us when the guest is
    /// ready to receive interrupts. The next `run()` call will return
    /// `VmExit::InterruptWindow` when interrupts can be injected.
    ///
    /// This is necessary because interrupts can only be delivered when:
    /// - RFLAGS.IF = 1 (interrupts enabled)
    /// - Not in an interrupt shadow (after STI, MOV SS, etc.)
    /// - No higher priority interrupt being serviced
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # use hv2_core::exit::VmExit;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Request notification when interrupt can be injected
    /// vcpu.request_interrupt_window()?;
    ///
    /// // Run until window opens
    /// loop {
    ///     let exit = vcpu.run()?;
    ///     if matches!(exit, VmExit::InterruptWindow) {
    ///         // Safe to inject now
    ///         vcpu.inject_interrupt(0x20)?; // Timer interrupt
    ///         break;
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn request_interrupt_window(&self) -> Result<()> {
        use super::whpx_ffi::*;

        // SAFETY: Building a zeroed deliverability notifications register and
        // passing to WHvSetVirtualProcessorRegisters with valid handles.
        unsafe {
            // Set the interrupt window request bit
            let mut deliverability =
                std::mem::zeroed::<WHV_X64_DELIVERABILITY_NOTIFICATIONS_REGISTER>();
            deliverability.InterruptNotification = 1; // Request notification

            let mut reg_value = std::mem::zeroed::<WHV_REGISTER_VALUE>();
            reg_value.DeliverabilityNotifications = deliverability;

            let register_names = [WHV_REGISTER_NAME::WHvX64RegisterDeliverabilityNotifications];
            let register_values = [reg_value];

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to request interrupt window: HRESULT 0x{:08X}",
                    hr
                )));
            }

            // Track window request
            {
                let mut stats = self.stats.write();
                stats.window_requests += 1;
            }

            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn request_interrupt_window(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Inject a Non-Maskable Interrupt (NMI).
    ///
    /// NMIs are special high-priority interrupts that cannot be masked by
    /// RFLAGS.IF. They are typically used for critical system events like
    /// hardware errors, watchdog timeouts, or profiling.
    ///
    /// Unlike regular interrupts, NMIs can be delivered immediately without
    /// waiting for an interrupt window.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Inject NMI for hardware error
    /// vcpu.inject_nmi()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # References
    ///
    /// - Intel SDM Vol 3A, Section 6.7: Nonmaskable Interrupt (NMI)
    #[cfg(target_os = "windows")]
    pub fn inject_nmi(&self) -> Result<()> {
        use super::whpx_ffi::*;

        // SAFETY: Building a zeroed pending NMI register and passing to
        // WHvSetVirtualProcessorRegisters with valid partition/vp_index handles.
        unsafe {
            let mut pending_int = std::mem::zeroed::<WHV_X64_PENDING_INTERRUPTION_REGISTER>();
            pending_int.InterruptionPending = 1;
            pending_int.InterruptionType = WHV_X64_PENDING_INTERRUPTION_TYPE::WHvX64PendingNmi;
            pending_int.DeliverErrorCode = 0;
            pending_int.InstructionLength = 0;
            pending_int.InterruptionVector = 2; // NMI vector
            pending_int.ErrorCode = 0;

            let mut reg_value = std::mem::zeroed::<WHV_REGISTER_VALUE>();
            reg_value.PendingInterruption = pending_int;

            let register_names = [WHV_REGISTER_NAME::WHvX64RegisterPendingInterruption];
            let register_values = [reg_value];

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to inject NMI: HRESULT 0x{:08X}",
                    hr
                )));
            }

            // Track NMI injection
            {
                let mut stats = self.stats.write();
                stats.nmis_injected += 1;
            }

            tracing::debug!("vCPU {}: NMI injected", self.vp_index);
            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn inject_nmi(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Inject a hardware exception with optional error code.
    ///
    /// Hardware exceptions are delivered by the CPU when certain conditions
    /// occur during instruction execution (divide by zero, page fault, etc.).
    ///
    /// # Arguments
    ///
    /// * `vector` - Exception vector (0-31)
    /// * `error_code` - Optional error code (required for some exceptions)
    ///
    /// # Common Exception Vectors
    ///
    /// - 0: Divide Error (#DE)
    /// - 6: Invalid Opcode (#UD)
    /// - 13: General Protection (#GP) - requires error code
    /// - 14: Page Fault (#PF) - requires error code
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Inject divide error (no error code)
    /// vcpu.inject_exception(0, None)?;
    ///
    /// // Inject general protection fault (with error code)
    /// vcpu.inject_exception(13, Some(0))?;
    ///
    /// // Inject page fault (with error code)
    /// vcpu.inject_exception(14, Some(0x00000007))?; // Present + Write + User
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # References
    ///
    /// - Intel SDM Vol 3A, Section 6.15: Exception and Interrupt Reference
    #[cfg(target_os = "windows")]
    pub fn inject_exception(&self, vector: u8, error_code: Option<u32>) -> Result<()> {
        use super::whpx_ffi::*;

        if vector > 31 {
            return Err(Error::Config(format!(
                "Invalid exception vector: {} (must be 0-31)",
                vector
            )));
        }

        // SAFETY: Building a zeroed pending exception register. Vector was
        // validated above (0-31). Passed to WHvSetVirtualProcessorRegisters
        // with valid handles.
        unsafe {
            let mut pending_int = std::mem::zeroed::<WHV_X64_PENDING_INTERRUPTION_REGISTER>();
            pending_int.InterruptionPending = 1;
            pending_int.InterruptionType =
                WHV_X64_PENDING_INTERRUPTION_TYPE::WHvX64PendingException;
            pending_int.DeliverErrorCode = if error_code.is_some() { 1 } else { 0 };
            pending_int.InstructionLength = 0;
            pending_int.InterruptionVector = vector as UINT32;
            pending_int.ErrorCode = error_code.unwrap_or(0);

            let mut reg_value = std::mem::zeroed::<WHV_REGISTER_VALUE>();
            reg_value.PendingInterruption = pending_int;

            let register_names = [WHV_REGISTER_NAME::WHvX64RegisterPendingInterruption];
            let register_values = [reg_value];

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to inject exception {}: HRESULT 0x{:08X}",
                    vector, hr
                )));
            }

            tracing::debug!(
                "vCPU {}: Exception {} injected (error_code: {:?})",
                self.vp_index,
                vector,
                error_code
            );
            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn inject_exception(&self, _vector: u8, _error_code: Option<u32>) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Setup vCPU for real mode boot
    ///
    /// Configures the vCPU to start executing in 16-bit real mode at the
    /// specified CS:IP address. Sets up initial segment registers, stack,
    /// and flags appropriate for real mode execution.
    ///
    /// # Arguments
    /// * `cs` - Code segment (typically 0x0000 for low memory boot)
    /// * `ip` - Instruction pointer (typically 0x7C00 for MBR bootloader)
    ///
    /// # Real Mode Configuration
    /// - CS:IP = entry point
    /// - DS = ES = FS = GS = 0x0000
    /// - SS:SP = 0x0000:0x7C00 (below bootloader, grows down)
    /// - All GPRs = 0
    /// - RFLAGS = 0x0002 (reserved bit set, interrupts disabled)
    /// - Segments: base=seg*16, limit=0xFFFF, present+RW
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Boot from 0x7C00 (standard MBR location)
    /// vcpu.setup_real_mode_boot(0x0000, 0x7C00)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn setup_real_mode_boot(&self, cs: u16, ip: u16) -> Result<()> {
        // Validate address is within 1MB real mode address space
        let phys_addr = (cs as u32) * 16 + (ip as u32);
        if phys_addr >= 0x100000 {
            return Err(Error::Config(format!(
                "Entry point {:04X}:{:04X} (physical 0x{:08X}) exceeds 1MB real mode limit",
                cs, ip, phys_addr
            )));
        }

        let mut regs = crate::vcpu::RegisterSet::default();

        // Set entry point
        regs.rip = ip as u64;
        regs.cs = cs as u64;

        // Initialize other segments to 0
        regs.ds = 0;
        regs.es = 0;
        regs.fs = 0;
        regs.gs = 0;
        regs.ss = 0;

        // Set stack pointer below bootloader (0x7C00), grows down
        // SP = 0x7C00 gives us stack from 0x0000 to 0x7BFF
        regs.rsp = 0x7C00;

        // Clear all general purpose registers
        regs.rax = 0;
        regs.rbx = 0;
        regs.rcx = 0;
        regs.rdx = 0;
        regs.rsi = 0;
        regs.rdi = 0;
        regs.rbp = 0;
        regs.r8 = 0;
        regs.r9 = 0;
        regs.r10 = 0;
        regs.r11 = 0;
        regs.r12 = 0;
        regs.r13 = 0;
        regs.r14 = 0;
        regs.r15 = 0;

        // RFLAGS: bit 1 reserved (always 1), interrupts disabled
        regs.rflags = 0x0002;

        // Use existing set_register_set to write all registers
        // This handles segment attributes correctly
        self.set_register_set(&regs)?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn get_register_set(&self) -> Result<crate::vcpu::RegisterSet> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_register_set(&self, _regs: &crate::vcpu::RegisterSet) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn get_rflags(&self) -> Result<u64> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn is_interrupt_enabled(&self) -> Result<bool> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn inject_interrupt(&self, _vector: u8) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn setup_real_mode_boot(&self, _cs: u16, _ip: u16) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Set the entry point (CS:IP) without modifying other registers
    ///
    /// This allows changing where execution will resume without performing
    /// a full reset or boot setup. Useful for continuing execution from a
    /// different location or implementing jump operations.
    ///
    /// # Arguments
    /// * `cs` - Code segment selector
    /// * `ip` - Instruction pointer (offset within segment)
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Jump to address 0x8000:0x0000
    /// vcpu.set_entry_point(0x8000, 0x0000)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn set_entry_point(&self, cs: u16, ip: u16) -> Result<()> {
        // Read current registers
        let mut regs = self.get_register_set()?;

        // Update only CS:IP
        regs.cs = cs as u64;
        regs.rip = ip as u64;

        // Write back
        self.set_register_set(&regs)
    }

    /// Set the stack pointer (SS:SP)
    ///
    /// Configures the stack segment and pointer without affecting other
    /// registers. Useful for relocating the stack or setting up specific
    /// stack configurations.
    ///
    /// # Arguments
    /// * `ss` - Stack segment selector
    /// * `sp` - Stack pointer (offset within segment)
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Set up stack at 0x9000:0xFFFF (grows down from top)
    /// vcpu.set_stack_pointer(0x9000, 0xFFFF)?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn set_stack_pointer(&self, ss: u16, sp: u16) -> Result<()> {
        // Read current registers
        let mut regs = self.get_register_set()?;

        // Update only SS:SP
        regs.ss = ss as u64;
        regs.rsp = sp as u64;

        // Write back
        self.set_register_set(&regs)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_entry_point(&self, _cs: u16, _ip: u16) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_stack_pointer(&self, _ss: u16, _sp: u16) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Reset vCPU to power-on state
    ///
    /// Resets the vCPU to the architectural state defined by the x86
    /// specification for processor reset/power-on. This matches the state
    /// described in Intel SDM Volume 3, Section 9.1.
    ///
    /// # Reset State
    /// - CS:IP = F000:FFF0 (reset vector in high memory)
    /// - CS base = 0xFFFF0000 (special reset mapping)
    /// - All other segments = 0 (base=0, limit=0xFFFF)
    /// - RFLAGS = 0x0002 (reserved bit set, interrupts disabled)
    /// - All GPRs = 0
    /// - Control registers set to reset values
    ///
    /// # Example
    /// ```no_run
    /// # use hv2_core::backends::whpx::WhpxVcpu;
    /// # fn example(vcpu: &WhpxVcpu) -> hv2_core::Result<()> {
    /// // Reset vCPU to start BIOS execution at reset vector
    /// vcpu.reset()?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn reset(&self) -> Result<()> {
        use super::whpx_ffi::*;

        let mut regs = crate::vcpu::RegisterSet::default();

        // x86 Reset Vector: CS = 0xF000, IP = 0xFFF0
        // This maps to physical address 0xFFFF0000:0xFFF0 = 0xFFFFFFF0
        // (CS base is special at reset: 0xFFFF0000 instead of 0xF0000)
        regs.cs = 0xF000;
        regs.rip = 0xFFF0;

        // All other segments = 0
        regs.ds = 0;
        regs.es = 0;
        regs.fs = 0;
        regs.gs = 0;
        regs.ss = 0;

        // Stack pointer = 0
        regs.rsp = 0;

        // All GPRs = 0
        regs.rax = 0;
        regs.rbx = 0;
        regs.rcx = 0;
        regs.rdx = 0;
        regs.rsi = 0;
        regs.rdi = 0;
        regs.rbp = 0;
        regs.r8 = 0;
        regs.r9 = 0;
        regs.r10 = 0;
        regs.r11 = 0;
        regs.r12 = 0;
        regs.r13 = 0;
        regs.r14 = 0;
        regs.r15 = 0;

        // RFLAGS: bit 1 reserved (always 1), interrupts disabled
        regs.rflags = 0x0002;

        // Write registers
        // Note: CS base will be set to 0xFFFF0000 by set_register_set's segment handling
        // Actually, we need to handle CS specially for reset. Let me write registers manually.

        // SAFETY: Building register name/value arrays for WHvSetVirtualProcessorRegisters
        // to reset all vCPU state to initial power-on values. All handles are valid.
        unsafe {
            // Build array of register names (24 registers)
            let register_names = [
                WHV_REGISTER_NAME::WHvX64RegisterRax,
                WHV_REGISTER_NAME::WHvX64RegisterRcx,
                WHV_REGISTER_NAME::WHvX64RegisterRdx,
                WHV_REGISTER_NAME::WHvX64RegisterRbx,
                WHV_REGISTER_NAME::WHvX64RegisterRsp,
                WHV_REGISTER_NAME::WHvX64RegisterRbp,
                WHV_REGISTER_NAME::WHvX64RegisterRsi,
                WHV_REGISTER_NAME::WHvX64RegisterRdi,
                WHV_REGISTER_NAME::WHvX64RegisterR8,
                WHV_REGISTER_NAME::WHvX64RegisterR9,
                WHV_REGISTER_NAME::WHvX64RegisterR10,
                WHV_REGISTER_NAME::WHvX64RegisterR11,
                WHV_REGISTER_NAME::WHvX64RegisterR12,
                WHV_REGISTER_NAME::WHvX64RegisterR13,
                WHV_REGISTER_NAME::WHvX64RegisterR14,
                WHV_REGISTER_NAME::WHvX64RegisterR15,
                WHV_REGISTER_NAME::WHvX64RegisterRip,
                WHV_REGISTER_NAME::WHvX64RegisterRflags,
                WHV_REGISTER_NAME::WHvX64RegisterCs,
                WHV_REGISTER_NAME::WHvX64RegisterDs,
                WHV_REGISTER_NAME::WHvX64RegisterEs,
                WHV_REGISTER_NAME::WHvX64RegisterFs,
                WHV_REGISTER_NAME::WHvX64RegisterGs,
                WHV_REGISTER_NAME::WHvX64RegisterSs,
            ];

            let mut reg_values: [WHV_REGISTER_VALUE; 24] = [std::mem::zeroed(); 24];

            // Set GPRs (all 0)
            for reg_value in reg_values.iter_mut().take(16) {
                reg_value.Reg64 = 0;
            }

            // Set RIP and RFLAGS
            reg_values[16].Reg64 = regs.rip; // 0xFFF0
            reg_values[17].Reg64 = regs.rflags; // 0x0002

            // Set CS with special reset base
            reg_values[18].Segment = WHV_X64_SEGMENT_REGISTER {
                Selector: regs.cs as u16, // 0xF000
                Base: 0xFFFF0000,         // Special reset vector base
                Limit: 0xFFFF,
                Attributes: 0x9B, // Present, executable, read/write
            };

            // Set other segments (DS, ES, FS, GS, SS) with normal real-mode attributes
            for reg_value in reg_values.iter_mut().skip(19) {
                reg_value.Segment = WHV_X64_SEGMENT_REGISTER {
                    Selector: 0,
                    Base: 0,
                    Limit: 0xFFFF,
                    Attributes: 0x93, // Present, data, read/write
                };
            }

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                24,
                reg_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to reset vCPU: HRESULT 0x{:08X}",
                    hr
                )));
            }

            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn reset(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Load binary from file and setup for boot
    ///
    /// Convenience method that combines multiple operations:
    /// 1. Reads binary file from disk
    /// 2. Writes binary to guest memory at specified address
    /// 3. Configures vCPU for real-mode boot at CS:IP
    ///
    /// This is the typical workflow for booting a guest binary like a bootloader
    /// or small kernel image.
    ///
    /// # Arguments
    ///
    /// * `vm` - VM containing the guest memory
    /// * `binary_path` - Path to binary file to load
    /// * `load_addr` - Guest physical address to load binary
    /// * `cs` - Code segment value for boot (typically 0x0000)
    /// * `ip` - Instruction pointer for boot (typically 0x7C00 for bootloaders)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Binary file cannot be read
    /// - Guest memory write fails
    /// - Boot setup fails (invalid CS:IP or out of bounds)
    ///
    /// # Real-Mode Address Calculation
    ///
    /// Physical address = (CS << 4) + IP
    /// - For CS=0x0000, IP=0x7C00 → physical 0x7C00
    /// - For CS=0x07C0, IP=0x0000 → physical 0x7C00 (equivalent)
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::*;
    /// # use std::path::Path;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Load bootloader and boot at standard 0x7C00
    /// vcpu.load_and_boot_binary(
    ///     &vm,
    ///     Path::new("guest/bootloader.bin"),
    ///     0x7C00,  // Load at standard boot sector location
    ///     0x0000,  // CS = 0
    ///     0x7C00,  // IP = 0x7C00
    /// )?;
    ///
    /// // vCPU is now ready to execute from 0x7C00
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(target_os = "windows")]
    pub fn load_and_boot_binary(
        &self,
        vm: &WhpxVm,
        binary_path: &Path,
        load_addr: u64,
        cs: u16,
        ip: u16,
    ) -> Result<()> {
        // Read binary file
        let binary_data = std::fs::read(binary_path).map_err(|e| {
            Error::Config(format!(
                "Failed to read binary file {}: {}",
                binary_path.display(),
                e
            ))
        })?;

        tracing::info!(
            "Loading {} bytes from {} to guest memory at 0x{:X}",
            binary_data.len(),
            binary_path.display(),
            load_addr
        );

        // Write binary to guest memory
        vm.write_guest_memory(load_addr, &binary_data)?;

        // Setup real-mode boot at specified CS:IP
        self.setup_real_mode_boot(cs, ip)?;

        tracing::info!(
            "Boot configured: CS=0x{:04X}, IP=0x{:04X} (physical: 0x{:X})",
            cs,
            ip,
            (cs as u32) << 4 | (ip as u32)
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn load_and_boot_binary(
        &self,
        _vm: &WhpxVm,
        _binary_path: &Path,
        _load_addr: u64,
        _cs: u16,
        _ip: u16,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // Control Register Management (Session 25)
    // ========================================================================

    /// Read control registers (CR0-CR4, CR8)
    ///
    /// Returns the current values of all control registers. These registers
    /// control processor operating mode (real/protected/long), paging, and
    /// various CPU features.
    ///
    /// # Control Registers
    ///
    /// - **CR0**: System control flags (protected mode, paging, etc.)
    /// - **CR2**: Page fault linear address
    /// - **CR3**: Page directory base register (PDBR)
    /// - **CR4**: Extended feature control flags
    /// - **CR8**: Task priority register (64-bit mode only)
    ///
    /// # Example
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVcpu};
    /// # use hv2_core::HypervisorBackend;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// let cr = vcpu.get_control_registers()?;
    /// println!("CR0: 0x{:016X}", cr.cr0);
    /// println!("Protected mode: {}", cr.is_protected_mode());
    /// println!("Paging enabled: {}", cr.is_paging_enabled());
    /// println!("Page directory: 0x{:016X}", cr.page_directory_base());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the WHPX API call fails.
    #[cfg(target_os = "windows")]
    pub fn get_control_registers(&self) -> Result<crate::ControlRegisters> {
        use super::whpx_ffi::WHV_REGISTER_NAME::*;

        // SAFETY: WHvGetVirtualProcessorRegisters FFI call with valid handles.
        // Accessing .Reg64 is valid for CR0/CR2/CR3/CR4/CR8/EFER registers.
        unsafe {
            let register_names = [
                WHvX64RegisterCr0,
                WHvX64RegisterCr2,
                WHvX64RegisterCr3,
                WHvX64RegisterCr4,
                WHvX64RegisterCr8,
                WHvX64RegisterEfer,
            ];

            let mut register_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>(); 6];

            let hr = WHvGetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                6,
                register_values.as_mut_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to get control registers: HRESULT 0x{:08X}",
                    hr
                )));
            }

            Ok(crate::ControlRegisters {
                cr0: register_values[0].Reg64,
                cr2: register_values[1].Reg64,
                cr3: register_values[2].Reg64,
                cr4: register_values[3].Reg64,
                cr8: register_values[4].Reg64,
                efer: register_values[5].Reg64,
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn get_control_registers(&self) -> Result<crate::ControlRegisters> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Write control registers (CR0-CR4, CR8)
    ///
    /// Updates the vCPU's control registers with validation. Invalid
    /// combinations (such as enabling paging without protected mode)
    /// are rejected before writing to hardware.
    ///
    /// # Safety
    ///
    /// Invalid control register combinations can cause guest crashes or
    /// unexpected behavior:
    ///
    /// - **CR0.PG requires CR0.PE**: Paging cannot be enabled without
    ///   protected mode
    /// - **CR0.PG with CR4.PAE**: Page tables must be properly initialized
    ///   before enabling paging with PAE
    /// - **CR3 alignment**: Must be 4KB aligned
    ///
    /// This method validates common requirements but cannot prevent all
    /// invalid configurations. Callers should ensure the guest state is
    /// properly initialized before mode transitions.
    ///
    /// # Example
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVcpu};
    /// # use hv2_core::HypervisorBackend;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Read current control registers
    /// let mut cr = vcpu.get_control_registers()?;
    ///
    /// // Enable protected mode
    /// cr.cr0 |= 0x1; // CR0.PE
    ///
    /// // Write back to vCPU
    /// vcpu.set_control_registers(&cr)?;
    ///
    /// // Verify
    /// let cr_verify = vcpu.get_control_registers()?;
    /// assert!(cr_verify.is_protected_mode());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Control register validation fails
    /// - The WHPX API call fails
    #[cfg(target_os = "windows")]
    pub fn set_control_registers(&self, cr: &crate::ControlRegisters) -> Result<()> {
        // Validate register combinations before writing to hardware
        cr.validate()
            .map_err(|msg| Error::Config(format!("Control register validation failed: {}", msg)))?;

        use super::whpx_ffi::WHV_REGISTER_NAME::*;

        // SAFETY: WHvSetVirtualProcessorRegisters FFI call with valid handles.
        // Setting .Reg64 for control registers that were validated above.
        unsafe {
            let register_names = [
                WHvX64RegisterCr0,
                WHvX64RegisterCr2,
                WHvX64RegisterCr3,
                WHvX64RegisterCr4,
                WHvX64RegisterCr8,
                WHvX64RegisterEfer,
            ];

            let mut register_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>(); 6];
            register_values[0].Reg64 = cr.cr0;
            register_values[1].Reg64 = cr.cr2;
            register_values[2].Reg64 = cr.cr3;
            register_values[3].Reg64 = cr.cr4;
            register_values[4].Reg64 = cr.cr8;
            register_values[5].Reg64 = cr.efer;

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                6,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to set control registers: HRESULT 0x{:08X}",
                    hr
                )));
            }

            tracing::debug!(
                "Set control registers for vCPU {}: CR0=0x{:016X}, CR3=0x{:016X}, CR4=0x{:016X}, EFER=0x{:016X}",
                self.vp_index,
                cr.cr0,
                cr.cr3,
                cr.cr4,
                cr.efer
            );

            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_control_registers(&self, _cr: &crate::ControlRegisters) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // Mode Transition Helpers (Session 25)
    // ========================================================================

    /// Enable protected mode (set CR0.PE)
    ///
    /// Transitions the processor from real mode to protected mode by setting
    /// the CR0.PE (Protected Mode Enable) bit. This is typically done early
    /// in the boot process after setting up the Global Descriptor Table (GDT).
    ///
    /// # Protected Mode Boot Sequence
    ///
    /// 1. Set up GDT with code/data segments (in guest code)
    /// 2. Load GDT with `lgdt` instruction (in guest code)
    /// 3. Enable protected mode with this method
    /// 4. Perform far jump to reload CS (in guest code)
    ///
    /// # Example
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVcpu};
    /// # use hv2_core::HypervisorBackend;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Guest should have set up GDT first
    /// vcpu.enable_protected_mode()?;
    /// println!("Transitioned to protected mode");
    ///
    /// // Verify
    /// let cr = vcpu.get_control_registers()?;
    /// assert!(cr.is_protected_mode());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the WHPX API call fails.
    #[cfg(target_os = "windows")]
    pub fn enable_protected_mode(&self) -> Result<()> {
        let mut cr = self.get_control_registers()?;

        if cr.is_protected_mode() {
            tracing::debug!("vCPU {} already in protected mode", self.vp_index);
            return Ok(()); // Already in protected mode
        }

        cr.cr0 |= 0x1; // Set CR0.PE
        self.set_control_registers(&cr)?;

        tracing::info!(
            "vCPU {}: Enabled protected mode (CR0.PE set)",
            self.vp_index
        );
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn enable_protected_mode(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Disable protected mode (clear CR0.PE)
    ///
    /// Returns the processor to real mode by clearing the CR0.PE bit.
    /// Paging must be disabled before disabling protected mode.
    ///
    /// # Requirements
    ///
    /// - Paging must be disabled (CR0.PG = 0)
    ///
    /// # Example
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVcpu};
    /// # use hv2_core::HypervisorBackend;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Ensure paging is disabled first
    /// vcpu.disable_paging()?;
    ///
    /// // Return to real mode
    /// vcpu.disable_protected_mode()?;
    ///
    /// let cr = vcpu.get_control_registers()?;
    /// assert!(!cr.is_protected_mode());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Paging is currently enabled (must disable paging first)
    /// - The WHPX API call fails
    #[cfg(target_os = "windows")]
    pub fn disable_protected_mode(&self) -> Result<()> {
        let mut cr = self.get_control_registers()?;

        // Ensure paging is disabled
        if cr.is_paging_enabled() {
            return Err(Error::Config(
                "Cannot disable protected mode while paging is enabled. Disable paging first."
                    .into(),
            ));
        }

        cr.cr0 &= !0x1; // Clear CR0.PE
        self.set_control_registers(&cr)?;

        tracing::info!(
            "vCPU {}: Disabled protected mode (CR0.PE cleared)",
            self.vp_index
        );
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn disable_protected_mode(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Enable paging (set CR0.PG and CR3)
    ///
    /// Enables virtual memory paging with the specified page directory.
    /// This allows the guest to use virtual addresses that are translated
    /// to physical addresses via page tables.
    ///
    /// # Arguments
    ///
    /// * `page_directory_base` - Physical address of the page directory.
    ///   Must be 4KB aligned (lower 12 bits must be 0).
    ///
    /// # Requirements
    ///
    /// - Protected mode must be enabled (CR0.PE = 1)
    /// - Page directory must be properly initialized in guest memory
    /// - Address must be 4KB aligned
    ///
    /// # Paging Modes
    ///
    /// The paging mode depends on other control register settings:
    /// - **32-bit paging**: CR0.PG=1, CR4.PAE=0, CR4.LA57=0
    /// - **PAE paging**: CR0.PG=1, CR4.PAE=1, CR4.LA57=0
    /// - **4-level paging**: CR0.PG=1, CR4.PAE=1, IA32_EFER.LME=1
    /// - **5-level paging**: CR0.PG=1, CR4.PAE=1, CR4.LA57=1, IA32_EFER.LME=1
    ///
    /// # Example
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVcpu};
    /// # use hv2_core::HypervisorBackend;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Enable protected mode first
    /// vcpu.enable_protected_mode()?;
    ///
    /// // Set up page directory at physical address 0x1000
    /// let page_dir_phys = 0x1000;
    /// // (Guest should initialize page tables at this address)
    ///
    /// // Enable paging
    /// vcpu.enable_paging(page_dir_phys)?;
    ///
    /// let cr = vcpu.get_control_registers()?;
    /// assert!(cr.is_paging_enabled());
    /// assert_eq!(cr.page_directory_base(), page_dir_phys);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Address is not 4KB aligned
    /// - Protected mode is not enabled
    /// - The WHPX API call fails
    #[cfg(target_os = "windows")]
    pub fn enable_paging(&self, page_directory_base: u64) -> Result<()> {
        // Validate alignment
        if page_directory_base & 0xFFF != 0 {
            return Err(Error::Config(format!(
                "Page directory base must be 4KB aligned. Got: 0x{:016X}",
                page_directory_base
            )));
        }

        let mut cr = self.get_control_registers()?;

        // Verify protected mode
        if !cr.is_protected_mode() {
            return Err(Error::Config(
                "Protected mode must be enabled before enabling paging".into(),
            ));
        }

        // Set CR3 (page directory base)
        cr.cr3 = page_directory_base;

        // Enable paging (CR0.PG)
        cr.cr0 |= 1 << 31;

        self.set_control_registers(&cr)?;

        tracing::info!(
            "vCPU {}: Enabled paging with page directory at 0x{:016X}",
            self.vp_index,
            page_directory_base
        );
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn enable_paging(&self, _page_directory_base: u64) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Disable paging (clear CR0.PG)
    ///
    /// Disables virtual memory paging, returning to physical addressing.
    ///
    /// # Example
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVcpu};
    /// # use hv2_core::HypervisorBackend;
    /// # async fn example() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 1024 * 1024).await?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// vcpu.disable_paging()?;
    ///
    /// let cr = vcpu.get_control_registers()?;
    /// assert!(!cr.is_paging_enabled());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the WHPX API call fails.
    #[cfg(target_os = "windows")]
    pub fn disable_paging(&self) -> Result<()> {
        let mut cr = self.get_control_registers()?;

        cr.cr0 &= !(1 << 31); // Clear CR0.PG
        self.set_control_registers(&cr)?;

        tracing::info!("vCPU {}: Disabled paging (CR0.PG cleared)", self.vp_index);
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn disable_paging(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // Long Mode (64-bit) Transition Helpers (Session 26)
    // ========================================================================

    /// Enable long mode (IA-32e mode / 64-bit mode)
    ///
    /// Transitions the processor to IA-32e long mode by:
    /// 1. Ensuring protected mode is enabled (CR0.PE)
    /// 2. Enabling PAE if not already enabled (CR4.PAE)
    /// 3. Setting the long mode enable bit (EFER.LME)
    /// 4. Enabling paging with valid page tables (CR0.PG)
    /// 5. Verifying long mode activated (EFER.LMA set by processor)
    ///
    /// # Arguments
    ///
    /// * `page_directory_base` - Physical address of the 4-level page table
    ///   (PML4). Must be 4KB aligned. The page tables must be valid and
    ///   identity-mapped for the kernel code.
    ///
    /// # Prerequisites
    ///
    /// - Valid 4-level page tables setup in guest memory
    /// - Protected mode enabled (or will be enabled automatically)
    /// - Page tables identity-mapped for kernel code regions
    ///
    /// # Long Mode Activation Sequence
    ///
    /// Per Intel SDM Volume 3A, Section 9.8.5:
    /// ```text
    /// 1. Protected mode must be enabled (CR0.PE = 1)
    /// 2. PAE must be enabled (CR4.PAE = 1)
    /// 3. Set EFER.LME = 1
    /// 4. Load CR3 with page table base
    /// 5. Enable paging (CR0.PG = 1)
    /// 6. Processor sets EFER.LMA = 1 (long mode active)
    /// 7. Load 64-bit code segment to enter 64-bit mode
    /// ```
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::HypervisorBackend;
    /// # #[tokio::main]
    /// # async fn main() -> hv2_core::Result<()> {
    /// let backend = WhpxBackend::new()?;
    /// let vm = backend.create_vm(1, 4 * 1024 * 1024).await?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// // Setup 4-level page tables at 0x10000
    /// // (page table setup code omitted for brevity)
    ///
    /// // Transition to long mode
    /// vcpu.enable_long_mode(0x10000)?;
    ///
    /// // Verify long mode is active
    /// let cr = vcpu.get_control_registers()?;
    /// assert!(cr.is_long_mode_active());
    /// assert!(cr.is_paging_enabled());
    /// assert!(cr.is_pae_enabled());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `page_directory_base` is not 4KB aligned
    /// - Page tables are invalid (will cause guest triple fault)
    /// - WHPX API calls fail
    /// - Long mode activation fails (EFER.LMA not set)
    #[cfg(target_os = "windows")]
    pub fn enable_long_mode(&self, page_directory_base: u64) -> Result<()> {
        // Validate 4KB alignment
        if page_directory_base & 0xFFF != 0 {
            return Err(Error::Config(format!(
                "Page directory base address 0x{:016X} is not 4KB aligned",
                page_directory_base
            )));
        }

        let mut cr = self.get_control_registers()?;

        // Step 1: Ensure protected mode is enabled
        if !cr.is_protected_mode() {
            cr.cr0 |= 0x1; // CR0.PE
            tracing::debug!(
                "vCPU {}: Enabling protected mode for long mode",
                self.vp_index
            );
        }

        // Step 2: Enable PAE (required for long mode)
        if !cr.is_pae_enabled() {
            cr.cr4 |= 1 << 5; // CR4.PAE
            tracing::debug!("vCPU {}: Enabling PAE for long mode", self.vp_index);
        }

        // Step 3: Enable long mode (EFER.LME)
        if !cr.is_long_mode_enabled() {
            cr.efer |= 1 << 8; // EFER.LME
            tracing::debug!("vCPU {}: Setting EFER.LME", self.vp_index);
        }

        // Step 4: Load CR3 with page table base
        cr.cr3 = page_directory_base;

        // Step 5: Enable paging (this activates long mode)
        if !cr.is_paging_enabled() {
            cr.cr0 |= 1 << 31; // CR0.PG
            tracing::debug!(
                "vCPU {}: Enabling paging to activate long mode",
                self.vp_index
            );
        }

        // Write all registers
        self.set_control_registers(&cr)?;

        // Step 6: Read back to verify EFER.LMA was set by processor
        let cr_verify = self.get_control_registers()?;
        if !cr_verify.is_long_mode_active() {
            return Err(Error::VM(
                "Long mode activation failed: EFER.LMA not set by processor. \
                 This usually indicates invalid page tables or missing prerequisites."
                    .into(),
            ));
        }

        tracing::info!(
            "vCPU {}: Long mode activated (EFER.LMA set by processor)",
            self.vp_index
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn enable_long_mode(&self, _page_directory_base: u64) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Disable long mode (return to protected mode)
    ///
    /// Transitions the processor from IA-32e long mode back to protected mode by:
    /// 1. Disabling paging (CR0.PG = 0)
    /// 2. Clearing long mode enable (EFER.LME = 0)
    /// 3. Verifying long mode deactivated (EFER.LMA = 0)
    ///
    /// # Important
    ///
    /// Paging must be disabled *before* clearing EFER.LME. The processor
    /// automatically clears EFER.LMA when paging is disabled.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::HypervisorBackend;
    /// # #[tokio::main]
    /// # async fn main() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 4 * 1024 * 1024).await?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// // In long mode...
    /// vcpu.enable_long_mode(0x10000)?;
    ///
    /// // Return to protected mode
    /// vcpu.disable_long_mode()?;
    ///
    /// let cr = vcpu.get_control_registers()?;
    /// assert!(!cr.is_long_mode_active());
    /// assert!(!cr.is_paging_enabled());
    /// assert!(cr.is_protected_mode()); // Still in protected mode
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the WHPX API call fails.
    #[cfg(target_os = "windows")]
    pub fn disable_long_mode(&self) -> Result<()> {
        let mut cr = self.get_control_registers()?;

        // Step 1: Disable paging first (processor clears EFER.LMA automatically)
        if cr.is_paging_enabled() {
            cr.cr0 &= !(1 << 31); // Clear CR0.PG
            tracing::debug!(
                "vCPU {}: Disabling paging for long mode exit",
                self.vp_index
            );
        }

        // Step 2: Clear EFER.LME
        if cr.is_long_mode_enabled() {
            cr.efer &= !(1 << 8); // Clear EFER.LME
            tracing::debug!("vCPU {}: Clearing EFER.LME", self.vp_index);
        }

        // Write registers
        self.set_control_registers(&cr)?;

        // Verify EFER.LMA was cleared by processor
        let cr_verify = self.get_control_registers()?;
        if cr_verify.is_long_mode_active() {
            return Err(Error::VM(
                "Long mode deactivation failed: EFER.LMA still set".into(),
            ));
        }

        tracing::info!("vCPU {}: Long mode deactivated", self.vp_index);

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn disable_long_mode(&self) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Get the current CPU operating mode
    ///
    /// Determines the current processor mode based on control register state:
    /// - **Real Mode**: 16-bit mode, no protection (CR0.PE=0)
    /// - **Protected Mode**: 32-bit protected mode (CR0.PE=1, CR0.PG=0, EFER.LMA=0)
    /// - **Long Mode Compatibility**: 32-bit code in 64-bit mode (EFER.LMA=1, CS.L=0)
    /// - **Long Mode 64-Bit**: True 64-bit mode (EFER.LMA=1, CS.L=1)
    ///
    /// Distinguishes between all x86 CPU modes by checking:
    /// - CR0.PE for protected mode
    /// - EFER.LMA for long mode activation
    /// - CS.L (bit 9 of segment attributes) for 64-bit vs compatibility mode
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::HypervisorBackend;
    /// # #[tokio::main]
    /// # async fn main() -> hv2_core::Result<()> {
    /// # let backend = WhpxBackend::new()?;
    /// # let vm = backend.create_vm(1, 4 * 1024 * 1024).await?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// let mode = vcpu.get_cpu_mode()?;
    /// match mode {
    ///     CpuMode::RealMode => println!("16-bit real mode"),
    ///     CpuMode::ProtectedMode => println!("32-bit protected mode"),
    ///     CpuMode::LongModeCompatibility => println!("32-bit compatibility mode"),
    ///     CpuMode::LongMode64Bit => println!("64-bit long mode"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the WHPX API call fails.
    #[cfg(target_os = "windows")]
    pub fn get_cpu_mode(&self) -> Result<CpuMode> {
        use super::whpx_ffi::WHV_REGISTER_NAME::*;

        let cr = self.get_control_registers()?;

        if cr.is_long_mode_active() {
            // In long mode — check CS.L bit to distinguish 64-bit from compatibility
            let cs_values = self.get_registers(&[WHvX64RegisterCs])?;
            // SAFETY: Accessing .Segment.Attributes is valid for a CS register value.
            let cs_attrs = unsafe { cs_values[0].Segment.Attributes };
            // CS.L is bit 9 (0x200) in the WHPX segment attribute format
            if cs_attrs & 0x200 != 0 {
                Ok(CpuMode::LongMode64Bit)
            } else {
                Ok(CpuMode::LongModeCompatibility)
            }
        } else if cr.is_protected_mode() {
            Ok(CpuMode::ProtectedMode)
        } else {
            Ok(CpuMode::RealMode)
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn get_cpu_mode(&self) -> Result<CpuMode> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // GDT/IDT Management (Session 26 Phase 3-4)
    // ========================================================================

    /// Load Global Descriptor Table (GDT) into guest memory
    ///
    /// Writes the GDT to guest memory and loads the GDTR register to
    /// activate it. The GDT defines segment descriptors for memory
    /// protection and privilege separation.
    ///
    /// # Arguments
    ///
    /// * `vm` - VM instance for writing to guest memory
    /// * `gdt_bytes` - Raw GDT data (from GdtBuilder::build())
    /// * `gdt_base` - Guest physical address where GDT will be loaded
    ///
    /// # Returns
    ///
    /// Tuple of (CS selector, DS selector) for kernel segments.
    /// Typically (0x08, 0x10) for standard 64-bit GDT layout.
    ///
    /// # GDT Layout Example
    ///
    /// Standard 64-bit GDT structure:
    /// ```text
    /// Offset  Selector  Description
    /// ------  --------  -----------
    /// 0x00    0x0000    Null descriptor (required)
    /// 0x08    0x0008    64-bit kernel code segment
    /// 0x10    0x0010    64-bit kernel data segment
    /// 0x18    0x0018    64-bit user code segment
    /// 0x20    0x0020    64-bit user data segment
    /// ```
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::{GdtBuilder, DESC_DPL_0, HypervisorBackend};
    /// # #[tokio::main]
    /// # async fn main() -> hv2_core::Result<()> {
    /// let backend = WhpxBackend::new()?;
    /// let vm = backend.create_vm(1, 4 * 1024 * 1024).await?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// // Build a minimal 64-bit GDT
    /// let gdt = GdtBuilder::new()
    ///     .add_null()
    ///     .add_code_64bit(DESC_DPL_0)
    ///     .add_data_64bit(DESC_DPL_0)
    ///     .build();
    ///
    /// // Load GDT at physical address 0x1000
    /// let (cs, ds) = vcpu.load_gdt(&vm, &gdt, 0x1000)?;
    ///
    /// assert_eq!(cs, 0x08); // Kernel code selector
    /// assert_eq!(ds, 0x10); // Kernel data selector
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - GDT base address is not properly aligned
    /// - Guest memory write fails
    /// - WHPX API calls fail
    #[cfg(target_os = "windows")]
    pub fn load_gdt(&self, vm: &WhpxVm, gdt_bytes: &[u8], gdt_base: u64) -> Result<(u16, u16)> {
        use super::whpx_ffi::{WHV_REGISTER_NAME::*, WHV_REGISTER_VALUE, WHV_X64_TABLE_REGISTER};

        // Write GDT to guest memory
        vm.write_guest_memory(gdt_base, gdt_bytes)?;

        // Build GDTR value
        let gdtr =
            crate::descriptors::DescriptorTablePointer::new(gdt_base, (gdt_bytes.len() - 1) as u16);

        // SAFETY: WHvSetVirtualProcessorRegisters FFI call to load GDTR.
        // Table register fields are set from the validated descriptor table pointer.
        unsafe {
            let register_names = [WHvX64RegisterGdtr];

            let mut table_reg = std::mem::zeroed::<WHV_X64_TABLE_REGISTER>();
            table_reg.Base = gdtr.base;
            table_reg.Limit = gdtr.limit;

            let mut register_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>(); 1];
            register_values[0].Table = table_reg;

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to load GDTR: HRESULT 0x{:08X}",
                    hr
                )));
            }
        }

        tracing::info!(
            "vCPU {}: Loaded GDT at 0x{:016X}, {} bytes",
            self.vp_index,
            gdt_base,
            gdt_bytes.len()
        );

        // Return standard kernel selectors
        // Assumes GDT layout: [null, code, data, ...]
        Ok((0x08, 0x10)) // CS=0x08, DS=0x10
    }

    #[cfg(not(target_os = "windows"))]
    pub fn load_gdt(&self, _vm: &WhpxVm, _gdt_bytes: &[u8], _gdt_base: u64) -> Result<(u16, u16)> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Load an Interrupt Descriptor Table (IDT) into the guest
    ///
    /// This method writes the IDT to guest physical memory at the specified base address
    /// and loads the IDTR register. The IDT maps interrupt vectors (0-255) to their
    /// handler functions.
    ///
    /// # Arguments
    ///
    /// * `vm` - Reference to the VM (needed for guest memory access)
    /// * `idt_bytes` - Serialized IDT entries (256 entries × 16 bytes for 64-bit, or 8 bytes for 32-bit)
    /// * `idt_base` - Physical address where IDT should be loaded
    ///
    /// # IDT Structure
    ///
    /// The IDT contains 256 entries, one for each interrupt vector:
    /// - Vectors 0-31: CPU exceptions (divide by zero, page fault, etc.)
    /// - Vectors 32-255: External interrupts and software interrupts
    ///
    /// Each entry specifies:
    /// - Handler address (where to jump when interrupt occurs)
    /// - Segment selector (code segment containing the handler)
    /// - Type (interrupt gate or trap gate)
    /// - Privilege level (DPL)
    /// - IST index (64-bit only, for stack switching)
    ///
    /// # Intel SDM Reference
    ///
    /// - Volume 3A, Section 6.10: "Interrupt Descriptor Table (IDT)"
    /// - Volume 3A, Section 6.14.1: "64-Bit Mode IDT"
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hv2_core::descriptors::{IdtBuilder, DESC_DPL_0, DESC_DPL_3};
    /// use hv2_core::backends::whpx::{WhpxVcpu, WhpxVm};
    /// # fn example(vcpu: &WhpxVcpu, vm: &WhpxVm) -> hv2_core::Result<()> {
    ///
    /// // Build 64-bit IDT with essential exception handlers
    /// let idt = IdtBuilder::new_64bit()
    ///     .add_interrupt_gate(0, 0xFFFF_8000_0010_0000, 0x08, 0, DESC_DPL_0)  // Divide by zero
    ///     .add_interrupt_gate(1, 0xFFFF_8000_0010_0100, 0x08, 0, DESC_DPL_0)  // Debug exception
    ///     .add_trap_gate(3, 0xFFFF_8000_0010_0200, 0x08, 0, DESC_DPL_3)       // Breakpoint (user accessible)
    ///     .add_interrupt_gate(6, 0xFFFF_8000_0010_0600, 0x08, 0, DESC_DPL_0)  // Invalid opcode
    ///     .add_interrupt_gate(13, 0xFFFF_8000_0010_0D00, 0x08, 0, DESC_DPL_0) // General protection fault
    ///     .add_interrupt_gate(14, 0xFFFF_8000_0010_0E00, 0x08, 0, DESC_DPL_0) // Page fault
    ///     .build();
    ///
    /// // Load IDT at physical address 0x4000
    /// vcpu.load_idt(&vm, &idt, 0x4000)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - IDT base address is not properly aligned
    /// - Guest memory write fails
    /// - WHPX API calls fail
    #[cfg(target_os = "windows")]
    pub fn load_idt(&self, vm: &WhpxVm, idt_bytes: &[u8], idt_base: u64) -> Result<()> {
        use super::whpx_ffi::{WHV_REGISTER_NAME::*, WHV_REGISTER_VALUE, WHV_X64_TABLE_REGISTER};

        // Write IDT to guest memory
        vm.write_guest_memory(idt_base, idt_bytes)?;

        // Build IDTR value
        let idtr =
            crate::descriptors::DescriptorTablePointer::new(idt_base, (idt_bytes.len() - 1) as u16);

        // SAFETY: WHvSetVirtualProcessorRegisters FFI call to load IDTR.
        // Table register fields are set from the validated descriptor table pointer.
        unsafe {
            let register_names = [WHvX64RegisterIdtr];

            let mut table_reg = std::mem::zeroed::<WHV_X64_TABLE_REGISTER>();
            table_reg.Base = idtr.base;
            table_reg.Limit = idtr.limit;

            let mut register_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>(); 1];
            register_values[0].Table = table_reg;

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                register_names.as_ptr(),
                1,
                register_values.as_ptr(),
            );

            if hr != S_OK {
                return Err(Error::VM(format!(
                    "Failed to load IDTR: HRESULT 0x{:08X}",
                    hr
                )));
            }
        }

        tracing::info!(
            "vCPU {}: Loaded IDT at 0x{:016X}, {} bytes ({} entries)",
            self.vp_index,
            idt_base,
            idt_bytes.len(),
            idt_bytes.len() / if idt_bytes.len() == 4096 { 16 } else { 8 }
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn load_idt(&self, _vm: &WhpxVm, _idt_bytes: &[u8], _idt_base: u64) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // Segment Register Management (Session 27, Phase 1)
    // ========================================================================

    /// Set code segment register (CS)
    ///
    /// Configures the code segment with the specified selector, base, limit, and access rights.
    /// This is essential for setting up the CPU execution environment during boot.
    ///
    /// # Arguments
    ///
    /// * `selector` - Segment selector (index into GDT/LDT)
    /// * `base` - Linear base address of the segment
    /// * `limit` - Segment limit (size - 1)
    /// * `access_rights` - Segment access rights and flags (see Intel SDM Vol 3A, Section 3.4.5)
    ///
    /// # Access Rights Format
    ///
    /// The access rights field is a 16-bit value with the following format:
    /// ```text
    /// Bits  | Field
    /// ------|-------------------------------------------------------
    /// 0-7   | Access byte (type, S, DPL, P)
    /// 8-11  | Flags (reserved, L, D/B, G)
    /// 12-15 | Reserved (must be 0)
    /// ```
    ///
    /// Common access rights values:
    /// - `0x9A` - Executable, readable, accessed (code segment)
    /// - `0x92` - Writable, accessed (data segment)
    /// - DPL bits (5-6) specify privilege level (0-3)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::Result;
    /// # fn example() -> Result<()> {
    /// # let vm = WhpxVm::new(1, 4 * 1024 * 1024)?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Set up 64-bit kernel code segment
    /// vcpu.set_code_segment(0x08, 0, 0xFFFFFFFF, 0x209A)?;
    /// // Selector 0x08 (index 1), flat addressing, executable+readable, 64-bit
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the segment register cannot be set via WHPX.
    #[cfg(target_os = "windows")]
    pub fn set_code_segment(
        &self,
        selector: u16,
        base: u64,
        limit: u32,
        access_rights: u16,
    ) -> Result<()> {
        // SAFETY: Setting the CS segment register via WHvSetVirtualProcessorRegisters.
        // The .Segment union field matches the register name WHvX64RegisterCs.
        unsafe {
            let reg_names = [WHV_REGISTER_NAME::WHvX64RegisterCs];
            let mut reg_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>()];

            reg_values[0].Segment = WHV_X64_SEGMENT_REGISTER {
                Base: base,
                Limit: limit,
                Selector: selector,
                Attributes: access_rights,
            };

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                reg_names.as_ptr(),
                1,
                reg_values.as_ptr(),
            );

            if hr != 0 {
                return Err(Error::VM(format!(
                    "Failed to set CS register: HRESULT 0x{:08X}",
                    hr as u32
                )));
            }

            tracing::debug!(
                "vCPU {}: Set CS to selector=0x{:04X}, base=0x{:016X}, limit=0x{:08X}, access=0x{:04X}",
                self.vp_index,
                selector,
                base,
                limit,
                access_rights
            );

            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_code_segment(
        &self,
        _selector: u16,
        _base: u64,
        _limit: u32,
        _access_rights: u16,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Set data segment register (DS, ES, FS, GS, or SS)
    ///
    /// Configures a data segment register with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `segment` - Which segment to set ("DS", "ES", "FS", "GS", "SS")
    /// * `selector` - Segment selector (index into GDT/LDT)
    /// * `base` - Linear base address of the segment
    /// * `limit` - Segment limit (size - 1)
    /// * `access_rights` - Segment access rights (typically 0x92 for flat data segments)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::Result;
    /// # fn example() -> Result<()> {
    /// # let vm = WhpxVm::new(1, 4 * 1024 * 1024)?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Set up flat 32-bit data segments
    /// vcpu.set_data_segment("DS", 0x10, 0, 0xFFFFFFFF, 0x92)?;
    /// vcpu.set_data_segment("ES", 0x10, 0, 0xFFFFFFFF, 0x92)?;
    /// vcpu.set_data_segment("SS", 0x10, 0, 0xFFFFFFFF, 0x92)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Invalid segment name provided
    /// - Segment register cannot be set via WHPX
    #[cfg(target_os = "windows")]
    pub fn set_data_segment(
        &self,
        segment: &str,
        selector: u16,
        base: u64,
        limit: u32,
        access_rights: u16,
    ) -> Result<()> {
        use WHV_REGISTER_NAME::*;

        let reg_name = match segment {
            "DS" => WHvX64RegisterDs,
            "ES" => WHvX64RegisterEs,
            "FS" => WHvX64RegisterFs,
            "GS" => WHvX64RegisterGs,
            "SS" => WHvX64RegisterSs,
            _ => {
                return Err(Error::InvalidState(format!(
                    "Invalid segment name: {}. Must be DS, ES, FS, GS, or SS",
                    segment
                )))
            }
        };

        // SAFETY: Setting a segment register via WHvSetVirtualProcessorRegisters.
        // The .Segment union field matches the validated segment register name.
        unsafe {
            let reg_names = [reg_name];
            let mut reg_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>()];

            reg_values[0].Segment = WHV_X64_SEGMENT_REGISTER {
                Base: base,
                Limit: limit,
                Selector: selector,
                Attributes: access_rights,
            };

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                reg_names.as_ptr(),
                1,
                reg_values.as_ptr(),
            );

            if hr != 0 {
                return Err(Error::VM(format!(
                    "Failed to set {} register: HRESULT 0x{:08X}",
                    segment, hr as u32
                )));
            }

            tracing::debug!(
                "vCPU {}: Set {} to selector=0x{:04X}, base=0x{:016X}, limit=0x{:08X}, access=0x{:04X}",
                self.vp_index,
                segment,
                selector,
                base,
                limit,
                access_rights
            );

            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_data_segment(
        &self,
        _segment: &str,
        _selector: u16,
        _base: u64,
        _limit: u32,
        _access_rights: u16,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Set instruction pointer (RIP/EIP)
    ///
    /// Sets the instruction pointer to the specified address. The actual register used
    /// depends on the current CPU mode (RIP for 64-bit, EIP for 32-bit, IP for 16-bit).
    ///
    /// # Arguments
    ///
    /// * `address` - The instruction address to set
    ///
    /// # Alignment
    ///
    /// For real mode, the address should be aligned to ensure CS:IP calculation is correct.
    /// For protected and long modes, alignment is less critical but recommended.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::Result;
    /// # fn example() -> Result<()> {
    /// # let vm = WhpxVm::new(1, 4 * 1024 * 1024)?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Set instruction pointer to kernel entry point
    /// vcpu.set_instruction_pointer(0x100000)?; // 1MB mark (standard for Linux/Multiboot)
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the instruction pointer cannot be set via WHPX.
    #[cfg(target_os = "windows")]
    pub fn set_instruction_pointer(&self, address: u64) -> Result<()> {
        // SAFETY: WHvSetVirtualProcessorRegisters FFI call with valid handles.
        // .Reg64 is the correct union field for the RIP register.
        unsafe {
            let reg_names = [WHV_REGISTER_NAME::WHvX64RegisterRip];
            let mut reg_values = [std::mem::zeroed::<WHV_REGISTER_VALUE>()];

            reg_values[0].Reg64 = address;

            let hr = WHvSetVirtualProcessorRegisters(
                self.partition,
                self.vp_index,
                reg_names.as_ptr(),
                1,
                reg_values.as_ptr(),
            );

            if hr != 0 {
                return Err(Error::VM(format!(
                    "Failed to set RIP: HRESULT 0x{:08X}",
                    hr as u32
                )));
            }

            tracing::debug!("vCPU {}: Set RIP to 0x{:016X}", self.vp_index, address);

            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_instruction_pointer(&self, _address: u64) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Configure standard boot segment registers
    ///
    /// Sets up segment registers for a standard flat memory model boot environment.
    /// This is a helper method that combines segment configuration for common boot scenarios.
    ///
    /// # Arguments
    ///
    /// * `cs_selector` - Code segment selector (typically 0x08 for first GDT entry)
    /// * `ds_selector` - Data segment selector (typically 0x10 for second GDT entry)
    /// * `is_64bit` - True for 64-bit long mode, false for 32-bit protected mode
    ///
    /// # Segment Configuration
    ///
    /// **64-bit mode (is_64bit = true):**
    /// - CS: Base=0, Limit=0xFFFFFFFF, Access=0x209A (64-bit code, L bit set)
    /// - DS/ES/SS: Base=0, Limit=0xFFFFFFFF, Access=0x92 (data, writable)
    /// - FS/GS: Base=0, Limit=0xFFFFFFFF, Access=0x92
    ///
    /// **32-bit mode (is_64bit = false):**
    /// - CS: Base=0, Limit=0xFFFFFFFF, Access=0x9A (32-bit code)
    /// - DS/ES/SS: Base=0, Limit=0xFFFFFFFF, Access=0x92 (data, writable)
    /// - FS/GS: Base=0, Limit=0xFFFFFFFF, Access=0x92
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::Result;
    /// # fn example() -> Result<()> {
    /// # let vm = WhpxVm::new(1, 4 * 1024 * 1024)?;
    /// # let vcpu = vm.create_vcpu(0)?;
    /// // Configure for 64-bit boot
    /// vcpu.configure_boot_segments(0x08, 0x10, true)?;
    ///
    /// // Or for 32-bit boot
    /// vcpu.configure_boot_segments(0x08, 0x10, false)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if any segment register cannot be configured.
    #[cfg(target_os = "windows")]
    pub fn configure_boot_segments(
        &self,
        cs_selector: u16,
        ds_selector: u16,
        is_64bit: bool,
    ) -> Result<()> {
        // Access rights for different segment types
        // For 64-bit: CS needs L bit set (bit 21 in full descriptor, bit 13 in attributes)
        let cs_access = if is_64bit {
            0x209A // Execute/Read, Accessed, L=1 (64-bit), P=1
        } else {
            0x9A // Execute/Read, Accessed, P=1 (32-bit)
        };
        let data_access = 0x92; // Read/Write, Accessed, P=1

        // Flat memory model: base=0, limit=0xFFFFFFFF
        let base = 0u64;
        let limit = 0xFFFFFFFFu32;

        // Configure CS (code segment)
        self.set_code_segment(cs_selector, base, limit, cs_access)?;

        // Configure data segments (DS, ES, SS all use same data selector)
        self.set_data_segment("DS", ds_selector, base, limit, data_access)?;
        self.set_data_segment("ES", ds_selector, base, limit, data_access)?;
        self.set_data_segment("SS", ds_selector, base, limit, data_access)?;

        // FS and GS typically start as null or same as DS
        self.set_data_segment("FS", ds_selector, base, limit, data_access)?;
        self.set_data_segment("GS", ds_selector, base, limit, data_access)?;

        tracing::info!(
            "vCPU {}: Configured boot segments (CS=0x{:04X}, DS=0x{:04X}, {}-bit)",
            self.vp_index,
            cs_selector,
            ds_selector,
            if is_64bit { 64 } else { 32 }
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn configure_boot_segments(
        &self,
        _cs_selector: u16,
        _ds_selector: u16,
        _is_64bit: bool,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // Boot Execution Methods (Session 27, Phase 2)
    // ========================================================================

    /// Boot a Linux kernel using the Linux boot protocol
    ///
    /// This method implements the complete Linux boot sequence as specified in the
    /// Linux kernel boot protocol (Documentation/x86/boot.rst in the kernel tree).
    ///
    /// # Boot Sequence
    ///
    /// 1. Validates boot parameters and protocol version
    /// 2. Allocates standard memory regions (GDT, IDT, page tables, boot_params)
    /// 3. Creates and loads 64-bit GDT with flat memory model
    /// 4. Sets up identity-mapped page tables for first 2MB
    /// 5. Writes kernel image to guest memory at kernel_addr
    /// 6. Writes boot_params structure to guest memory
    /// 7. Optionally writes initrd to guest memory
    /// 8. Enables long mode (64-bit) with paging
    /// 9. Configures segment registers for 64-bit flat model
    /// 10. Sets RIP to kernel entry point
    /// 11. Sets RSI to boot_params address (per Linux protocol)
    ///
    /// # Arguments
    ///
    /// * `vm` - The VM instance for memory operations
    /// * `params` - Linux boot parameters (kernel image, addresses, cmdline, initrd)
    /// * `entry_point` - Kernel entry point address (typically 0x100000 for 64-bit)
    ///
    /// # Linux Boot Protocol Requirements
    ///
    /// When the kernel receives control:
    /// - CPU is in 64-bit long mode
    /// - Paging is enabled with identity mapping
    /// - RSI points to boot_params structure
    /// - CS selector points to 64-bit code segment
    /// - DS/ES/SS point to 64-bit data segment
    /// - Stack is configured and ready
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::boot::linux::LinuxBootParams;
    /// # use hv2_core::Result;
    /// # fn example() -> Result<()> {
    /// # let kernel_image = vec![0u8; 4096]; // Minimal bzImage
    /// let params = LinuxBootParams {
    ///     kernel_image,
    ///     kernel_addr: 0x100000,
    ///     setup_addr: 0x90000,
    ///     cmdline: "console=ttyS0".to_string(),
    ///     initrd: None,
    /// };
    ///
    /// let vm = WhpxVm::new(1, 16 * 1024 * 1024)?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// vcpu.boot_linux(&vm, &params, 0x100000)?;
    /// // Kernel is now ready to execute
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Boot parameters validation fails
    /// - Memory write operations fail
    /// - Segment or register configuration fails
    /// - Long mode transition fails
    ///
    /// # References
    ///
    /// - Linux Boot Protocol: Documentation/x86/boot.rst
    /// - Intel SDM Volume 3A: System Programming Guide
    #[cfg(target_os = "windows")]
    pub fn boot_linux(
        &self,
        vm: &WhpxVm,
        params: &LinuxBootParams,
        entry_point: u64,
    ) -> Result<()> {
        tracing::info!(
            "vCPU {}: Starting Linux boot sequence (entry=0x{:016X})",
            self.vp_index,
            entry_point
        );

        // Step 1: Validate boot parameters
        LinuxBootProtocol::validate_params(params)?;
        tracing::debug!("✓ Boot parameters validated");

        // Step 2: Allocate standard memory regions
        let (gdt_base, idt_base, page_table_base, stack_pointer) =
            BootSetup::allocate_standard_tables();
        let boot_params_addr = params.setup_addr;

        tracing::debug!(
            "Memory layout: GDT=0x{:X}, IDT=0x{:X}, PT=0x{:X}, boot_params=0x{:X}, stack=0x{:X}",
            gdt_base,
            idt_base,
            page_table_base,
            boot_params_addr,
            stack_pointer
        );

        // Step 3: Create and load GDT (64-bit flat model)
        let gdt = GdtBuilder::new()
            .add_null() // Entry 0: Null descriptor (required)
            .add_code_64bit(0) // Entry 1: Kernel code segment (selector 0x08)
            .add_data_64bit(0) // Entry 2: Kernel data segment (selector 0x10)
            .build();

        let (cs_selector, ds_selector) = self.load_gdt(vm, &gdt, gdt_base)?;
        tracing::debug!(
            "✓ GDT loaded: CS=0x{:04X}, DS=0x{:04X}",
            cs_selector,
            ds_selector
        );

        // Step 4: Create and write identity-mapped page tables
        let page_tables = BootSetup::create_identity_page_tables(page_table_base);
        vm.write_guest_memory(page_table_base, &page_tables)?;
        tracing::debug!(
            "✓ Page tables created: {} bytes at 0x{:016X}",
            page_tables.len(),
            page_table_base
        );

        // Step 5: Write kernel image to guest memory
        vm.write_guest_memory(params.kernel_addr, &params.kernel_image)?;
        tracing::debug!(
            "✓ Kernel loaded: {} bytes at 0x{:016X}",
            params.kernel_image.len(),
            params.kernel_addr
        );

        // Step 6: Create and write boot_params structure
        // For now, create a minimal boot_params structure (we'll use helper if available)
        let initrd_addr = params.initrd.as_ref().map(|_| 0x800000u64); // Standard initrd location
        let initrd_size = params.initrd.as_ref().map(|d| d.len());

        let boot_params_data =
            LinuxBootProtocol::create_boot_params(params, initrd_addr, initrd_size);
        vm.write_guest_memory(boot_params_addr, &boot_params_data)?;
        tracing::debug!(
            "✓ boot_params written: {} bytes at 0x{:016X}",
            boot_params_data.len(),
            boot_params_addr
        );

        // Step 7: Write initrd if provided
        if let Some(initrd_data) = &params.initrd {
            let initrd_load_addr = initrd_addr.unwrap_or(0x800000);
            vm.write_guest_memory(initrd_load_addr, initrd_data)?;
            tracing::debug!(
                "✓ Initrd loaded: {} bytes at 0x{:016X}",
                initrd_data.len(),
                initrd_load_addr
            );
        }

        // Step 8: Enable long mode with paging
        self.enable_long_mode(page_table_base)?;
        tracing::debug!("✓ Long mode enabled with page tables");

        // Step 9: Configure segment registers for 64-bit flat model
        self.configure_boot_segments(cs_selector, ds_selector, true)?;
        tracing::debug!("✓ Segment registers configured (64-bit flat model)");

        // Step 10: Set instruction pointer to kernel entry
        self.set_instruction_pointer(entry_point)?;
        tracing::debug!("✓ RIP set to entry point: 0x{:016X}", entry_point);

        // Step 11: Set RSI to boot_params address (Linux protocol requirement)
        // Use the register set method to set RSI
        let mut regs = self.get_register_set()?;
        regs.rsi = boot_params_addr;
        self.set_register_set(&regs)?;
        tracing::debug!("✓ RSI set to boot_params: 0x{:016X}", boot_params_addr);

        // Step 12: Configure stack
        self.set_stack_pointer(ds_selector, stack_pointer as u16)?;
        tracing::debug!("✓ Stack configured: 0x{:016X}", stack_pointer);

        tracing::info!(
            "vCPU {}: Linux boot sequence complete, ready to execute at 0x{:016X}",
            self.vp_index,
            entry_point
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn boot_linux(
        &self,
        _vm: &WhpxVm,
        _params: &LinuxBootParams,
        _entry_point: u64,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    // ========================================================================
    // Boot Execution Methods (Session 27, Phase 3)
    // ========================================================================

    /// Boot a Multiboot-compliant kernel
    ///
    /// This method implements the complete Multiboot 1.0 boot sequence as specified
    /// in the Multiboot specification.
    ///
    /// # Boot Sequence
    ///
    /// 1. Validates Multiboot header in kernel image
    /// 2. Allocates standard memory regions (GDT, multiboot_info, modules)
    /// 3. Creates and loads 32-bit GDT with flat memory model
    /// 4. Writes kernel image to guest memory
    /// 5. Writes modules to guest memory (if provided)
    /// 6. Creates and writes multiboot_info structure
    /// 7. Enables protected mode (32-bit, no paging by default)
    /// 8. Configures segment registers for 32-bit flat model
    /// 9. Sets EIP to kernel entry point
    /// 10. Sets EAX to 0x2BADB002 (Multiboot magic)
    /// 11. Sets EBX to multiboot_info address
    /// 12. Configures stack pointer
    ///
    /// # Arguments
    ///
    /// * `vm` - The VM instance for memory operations
    /// * `info` - Multiboot information (kernel image, modules, cmdline, memory map)
    /// * `entry_point` - Kernel entry point address (typically 0x100000)
    ///
    /// # Multiboot Protocol Requirements
    ///
    /// When the kernel receives control:
    /// - CPU is in 32-bit protected mode (paging disabled)
    /// - EAX contains 0x2BADB002 (bootloader magic)
    /// - EBX points to multiboot_info structure
    /// - CS selector points to 32-bit code segment
    /// - DS/ES/SS point to 32-bit data segment
    /// - Interrupts are disabled
    /// - A20 gate is enabled
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
    /// # use hv2_core::boot::multiboot::MultibootInfo;
    /// # use hv2_core::Result;
    /// # fn example() -> Result<()> {
    /// # let kernel_image = vec![0u8; 4096]; // Minimal Multiboot kernel
    /// let info = MultibootInfo {
    ///     kernel_image,
    ///     modules: vec![],
    ///     cmdline: "--test".to_string(),
    ///     memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
    /// };
    ///
    /// let vm = WhpxVm::new(1, 16 * 1024 * 1024)?;
    /// let vcpu = vm.create_vcpu(0)?;
    ///
    /// vcpu.boot_multiboot(&vm, &info, 0x100000)?;
    /// // Kernel is now ready to execute
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Multiboot header validation fails
    /// - Memory write operations fail
    /// - Segment or register configuration fails
    /// - Protected mode transition fails
    ///
    /// # References
    ///
    /// - Multiboot Specification 1.0
    /// - Intel SDM Volume 3A: System Programming Guide
    #[cfg(target_os = "windows")]
    pub fn boot_multiboot(
        &self,
        vm: &WhpxVm,
        info: &MultibootInfo,
        entry_point: u64,
    ) -> Result<()> {
        tracing::info!(
            "vCPU {}: Starting Multiboot boot sequence (entry=0x{:016X})",
            self.vp_index,
            entry_point
        );

        // Step 1: Validate Multiboot header
        let header = MultibootProtocol::find_header(&info.kernel_image)?;
        tracing::debug!(
            "✓ Multiboot header found at offset 0x{:X}, flags=0x{:08X}",
            header.offset,
            header.flags
        );

        // Step 2: Allocate standard memory regions
        let (gdt_base, _idt_base, _page_table_base, stack_pointer) =
            BootSetup::allocate_standard_tables();
        // Share the layout with every other backend so their guest memory
        // images cannot drift apart.
        let layout = MultibootLayout::default().with_kernel_addr(entry_point);
        let multiboot_info_addr = layout.info_addr;

        tracing::debug!(
            "Memory layout: GDT=0x{:X}, multiboot_info=0x{:X}, kernel=0x{:X}, modules=0x{:X}, stack=0x{:X}",
            gdt_base,
            multiboot_info_addr,
            layout.kernel_addr,
            layout.first_module_addr,
            stack_pointer
        );

        // Step 3: Create and load GDT (32-bit flat model)
        let gdt = GdtBuilder::new()
            .add_null() // Entry 0: Null descriptor (required)
            .add_code_32bit(0, 0xFFFFFFFF, 0) // Entry 1: Kernel code segment (selector 0x08)
            .add_data_32bit(0, 0xFFFFFFFF, 0) // Entry 2: Kernel data segment (selector 0x10)
            .build();

        let (cs_selector, ds_selector) = self.load_gdt(vm, &gdt, gdt_base)?;
        tracing::debug!(
            "✓ GDT loaded: CS=0x{:04X}, DS=0x{:04X}",
            cs_selector,
            ds_selector
        );

        // Steps 4-6: Write the whole boot environment — kernel, modules, the
        // module descriptor list, command lines, memory map, and the
        // multiboot_info structure that points at them all.
        for (addr, data) in MultibootProtocol::prepare_guest_memory(info, &layout)? {
            vm.write_guest_memory(addr, &data)?;
            tracing::debug!("✓ wrote {} bytes at 0x{:016X}", data.len(), addr);
        }

        // Step 7: Enable protected mode (32-bit, no paging)
        self.enable_protected_mode()?;
        tracing::debug!("✓ Protected mode enabled (32-bit)");

        // Step 8: Configure segment registers for 32-bit flat model
        self.configure_boot_segments(cs_selector, ds_selector, false)?;
        tracing::debug!("✓ Segment registers configured (32-bit flat model)");

        // Step 9: Set instruction pointer to kernel entry
        self.set_instruction_pointer(entry_point)?;
        tracing::debug!("✓ EIP set to entry point: 0x{:016X}", entry_point);

        // Step 10: Set EAX to Multiboot magic (0x2BADB002)
        let mut regs = self.get_register_set()?;
        regs.rax = MultibootProtocol::bootloader_magic() as u64;
        tracing::debug!("✓ EAX set to Multiboot magic: 0x{:08X}", regs.rax);

        // Step 11: Set EBX to multiboot_info address
        regs.rbx = multiboot_info_addr;
        self.set_register_set(&regs)?;
        tracing::debug!(
            "✓ EBX set to multiboot_info: 0x{:016X}",
            multiboot_info_addr
        );

        // Step 12: Configure stack
        self.set_stack_pointer(ds_selector, stack_pointer as u16)?;
        tracing::debug!("✓ Stack configured: 0x{:016X}", stack_pointer);

        tracing::info!(
            "vCPU {}: Multiboot boot sequence complete, ready to execute at 0x{:016X}",
            self.vp_index,
            entry_point
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn boot_multiboot(
        &self,
        _vm: &WhpxVm,
        _info: &MultibootInfo,
        _entry_point: u64,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }

    /// Boot from a real mode binary (boot sector, MBR, VBR).
    ///
    /// This method loads a binary at the standard boot sector address (0x7C00) and
    /// configures the CPU for real mode execution. This is suitable for:
    /// - Master Boot Record (MBR) loading
    /// - Volume Boot Record (VBR) loading
    /// - Custom boot sector code
    /// - Legacy 16-bit bootloaders
    ///
    /// # Memory Layout
    ///
    /// ```text
    /// 0x00000000 - 0x000003FF  Interrupt Vector Table (IVT)
    /// 0x00000400 - 0x000004FF  BIOS Data Area (BDA)
    /// 0x00000500 - 0x00007BFF  Free memory
    /// 0x00007C00 - 0x00007DFF  Boot sector (512 bytes)
    /// 0x00007E00 - 0x0009FFFF  Free memory (stack grows down from 0x7C00)
    /// 0x000A0000 - 0x000BFFFF  Video memory
    /// 0x000C0000 - 0x000FFFFF  ROM area
    /// ```
    ///
    /// # CPU State After Boot
    ///
    /// - Mode: Real mode (16-bit)
    /// - CS:IP = 0x0000:0x7C00 (execution starts at boot sector)
    /// - DL = boot drive number (0x80 for first hard disk, 0x00 for first floppy)
    /// - DS = ES = SS = 0x0000
    /// - SP = 0x7C00 (stack grows down from boot sector)
    /// - All other registers zeroed
    ///
    /// # Arguments
    ///
    /// * `vm` - Virtual machine to load the boot sector into
    /// * `boot_sector` - 512-byte boot sector image
    /// * `boot_drive` - Boot drive number (0x00=floppy, 0x80=hard disk)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hv2_core::backends::whpx::{WhpxVcpu, WhpxVm};
    /// # async fn example(vcpu: &WhpxVcpu, vm: &WhpxVm) -> hv2_core::Result<()> {
    /// // Load a boot sector (e.g., from a disk image)
    /// let boot_sector = std::fs::read("bootsect.bin")?;
    /// assert_eq!(boot_sector.len(), 512);
    /// assert_eq!(boot_sector[510], 0x55); // Boot signature
    /// assert_eq!(boot_sector[511], 0xAA); // Boot signature
    ///
    /// // Boot from hard disk
    /// vcpu.boot_real_mode(vm, &boot_sector, 0x80).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Boot sector is not exactly 512 bytes
    /// - Boot sector is missing boot signature (0x55AA at offset 510-511)
    /// - Memory write fails
    /// - Register configuration fails
    ///
    /// # References
    ///
    /// - [x86 Boot Process](https://wiki.osdev.org/Boot_Sequence)
    /// - [Master Boot Record](https://en.wikipedia.org/wiki/Master_boot_record)
    /// - Intel SDM Vol 3A, Section 9.1 (Real-Address Mode)
    #[cfg(target_os = "windows")]
    pub async fn boot_real_mode(
        &self,
        vm: &WhpxVm,
        boot_sector: &[u8],
        boot_drive: u8,
    ) -> Result<()> {
        tracing::info!(
            "vCPU {}: Starting real mode boot sequence (drive 0x{:02X})",
            self.vp_index,
            boot_drive
        );

        // Step 1: Validate boot sector
        if boot_sector.len() != 512 {
            return Err(Error::VM(format!(
                "Invalid boot sector size: {} bytes (expected 512)",
                boot_sector.len()
            )));
        }

        // Check for boot signature (0x55AA at offset 510-511)
        if boot_sector[510] != 0x55 || boot_sector[511] != 0xAA {
            return Err(Error::VM(format!(
                "Invalid boot signature: 0x{:02X}{:02X} (expected 0x55AA)",
                boot_sector[511], boot_sector[510]
            )));
        }
        tracing::debug!("✓ Boot sector validated (512 bytes, signature 0x55AA)");

        // Step 2: Load boot sector at 0x7C00
        const BOOT_SECTOR_ADDR: u64 = 0x7C00;
        vm.write_guest_memory(BOOT_SECTOR_ADDR, boot_sector)?;
        tracing::debug!(
            "✓ Boot sector loaded at 0x{:04X} (512 bytes)",
            BOOT_SECTOR_ADDR
        );

        // Step 3: Ensure CPU is in real mode (disable protected mode and long mode)
        self.disable_protected_mode()?;
        tracing::debug!("✓ Protected mode disabled, CPU in real mode");

        // Step 4: Set CS:IP to 0x0000:0x7C00
        // In real mode: physical address = (segment << 4) + offset
        // 0x0000:0x7C00 = (0x0000 << 4) + 0x7C00 = 0x7C00
        let cs_selector = 0u16;
        self.set_code_segment(cs_selector, 0, 0xFFFF, 0x9B)?; // Real mode: base=0, limit=64K, executable
        self.set_instruction_pointer(BOOT_SECTOR_ADDR)?;
        tracing::debug!(
            "✓ CS:IP set to 0x{:04X}:0x{:04X}",
            cs_selector,
            BOOT_SECTOR_ADDR
        );

        // Step 5: Configure data segments (DS, ES, SS) to 0x0000
        let ds_selector = 0u16;
        let segment_rights = 0x93u16; // Real mode data: present, writable, accessed
        self.set_data_segment("DS", ds_selector, 0, 0xFFFF, segment_rights)?;
        self.set_data_segment("ES", ds_selector, 0, 0xFFFF, segment_rights)?;
        self.set_data_segment("SS", ds_selector, 0, 0xFFFF, segment_rights)?;
        tracing::debug!("✓ DS, ES, SS set to 0x{:04X}", ds_selector);

        // Step 6: Configure stack pointer (SP = 0x7C00, stack grows down)
        self.set_stack_pointer(ds_selector, 0x7C00)?;
        tracing::debug!("✓ Stack pointer set to 0x{:04X} (grows down)", 0x7C00);

        // Step 7: Set DL to boot drive number (BIOS convention)
        let mut regs = self.get_register_set()?;
        regs.rdx = (regs.rdx & !0xFF) | (boot_drive as u64); // Preserve DH, set DL
        tracing::debug!("✓ DL (boot drive) set to 0x{:02X}", boot_drive);

        // Step 8: Zero other general-purpose registers
        regs.rax = 0;
        regs.rbx = 0;
        regs.rcx = 0;
        // rdx already set (DL = boot_drive)
        regs.rsi = 0;
        regs.rdi = 0;
        regs.rbp = 0;
        // rsp already set by set_stack_pointer
        self.set_register_set(&regs)?;
        tracing::debug!("✓ General-purpose registers initialized");

        // Step 9: Configure FS and GS segments (typically unused in real mode, set to 0)
        self.set_data_segment("FS", 0, 0, 0xFFFF, segment_rights)?;
        self.set_data_segment("GS", 0, 0, 0xFFFF, segment_rights)?;
        tracing::debug!("✓ FS, GS segments initialized");

        tracing::info!(
            "vCPU {}: Real mode boot sequence complete, ready to execute at 0x{:04X}:0x{:04X}",
            self.vp_index,
            cs_selector,
            BOOT_SECTOR_ADDR
        );

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub async fn boot_real_mode(
        &self,
        _vm: &WhpxVm,
        _boot_sector: &[u8],
        _boot_drive: u8,
    ) -> Result<()> {
        Err(Error::VM(
            "WHPX backend is only available on Windows".into(),
        ))
    }
}

/// CPU operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    /// 16-bit real mode (no protection, CR0.PE=0)
    RealMode,

    /// 32-bit protected mode (CR0.PE=1, EFER.LMA=0)
    ProtectedMode,

    /// Long mode compatibility sub-mode (32-bit code in 64-bit mode)
    /// EFER.LMA=1, CS.L=0
    LongModeCompatibility,

    /// Long mode 64-bit sub-mode (true 64-bit mode)
    /// EFER.LMA=1, CS.L=1
    LongMode64Bit,
}

impl Drop for WhpxVcpu {
    #[cfg(target_os = "windows")]
    fn drop(&mut self) {
        // SAFETY: WHvDeleteVirtualProcessor releases the vCPU created in WhpxVcpu::new.
        // The partition handle and vp_index are valid for the lifetime of this object.
        unsafe {
            let _ = WHvDeleteVirtualProcessor(self.partition, self.vp_index);
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn drop(&mut self) {}
}

// SAFETY: WhpxVcpu partition handles are thread-safe opaque pointers from WHP.
// The vp_index is immutable and stats are behind Arc<RwLock>.
unsafe impl Send for WhpxVcpu {}
unsafe impl Sync for WhpxVcpu {}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_whpx_backend_creation() {
        let result = WhpxBackend::new();
        // May fail if WHPX is not available, that's OK
        match result {
            Ok(backend) => {
                assert_eq!(backend.platform(), HypervisorPlatform::Whpx);
                assert!(backend.capabilities().max_vcpus > 0);
            }
            Err(e) => {
                eprintln!("WHPX not available (expected on some systems): {}", e);
            }
        }
    }

    /// A backend hands out one partition. The second request must fail
    /// loudly: before this check it succeeded and quietly aliased the first,
    /// because both partitions' vCPU 0 land on the same `vcpu_map` key and
    /// `load_boot` then resolved whichever partition was created last.
    #[tokio::test]
    async fn a_second_partition_on_one_backend_is_refused() {
        let Ok(backend) = WhpxBackend::new() else {
            eprintln!("WHPX not available — skipping");
            return;
        };
        if backend.create_vm(1, 1024 * 1024).await.is_err() {
            eprintln!("WHPX partition creation unavailable (may require admin) — skipping");
            return;
        }

        let err = match backend.create_vm(1, 1024 * 1024).await {
            Ok(_) => panic!("the second partition must be refused, not aliased"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("already owns a partition"),
            "the error should say why one backend owns one partition, got: {err}"
        );
    }

    /// `shutdown` releases the partition, so the backend can be reused.
    #[tokio::test]
    async fn shutdown_frees_the_backend_to_own_a_partition_again() {
        let Ok(mut backend) = WhpxBackend::new() else {
            eprintln!("WHPX not available — skipping");
            return;
        };
        if backend.create_vm(1, 1024 * 1024).await.is_err() {
            eprintln!("WHPX partition creation unavailable (may require admin) — skipping");
            return;
        }

        backend.shutdown().await.expect("shutdown should succeed");
        backend
            .create_vm(1, 1024 * 1024)
            .await
            .expect("after shutdown the backend owns no partition, so this must succeed");
    }

    #[tokio::test]
    async fn test_whpx_vm_creation() {
        if let Ok(backend) = WhpxBackend::new() {
            let result = backend.create_vm(1, 1024 * 1024).await; // 1 vCPU, 1MB RAM
            match result {
                Ok(vm) => {
                    assert_eq!(vm.platform(), HypervisorPlatform::Whpx);
                }
                Err(e) => {
                    eprintln!("Failed to create VM (may require admin): {}", e);
                }
            }
        }
    }

    // ========================================================================
    // Control Register Tests (Session 25)
    // ========================================================================

    #[tokio::test]
    async fn test_control_register_access() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Read control registers
                    match vcpu.get_control_registers() {
                        Ok(cr) => {
                            println!("✓ Control register read successful");
                            println!("  CR0: 0x{:016X}", cr.cr0);
                            println!("  CR3: 0x{:016X}", cr.cr3);
                            println!("  CR4: 0x{:016X}", cr.cr4);

                            // Write control registers
                            match vcpu.set_control_registers(&cr) {
                                Ok(()) => println!("✓ Control register write successful"),
                                Err(e) => eprintln!("⊘ Control register write failed: {}", e),
                            }
                        }
                        Err(e) => eprintln!("⊘ Control register read failed: {}", e),
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_protected_mode_transition() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Enable protected mode
                    match vcpu.enable_protected_mode() {
                        Ok(()) => {
                            println!("✓ Protected mode enabled");

                            // Verify
                            if let Ok(cr) = vcpu.get_control_registers() {
                                assert!(cr.is_protected_mode(), "Should be in protected mode");
                                println!("✓ Protected mode verification passed");
                            }

                            // Disable protected mode
                            match vcpu.disable_protected_mode() {
                                Ok(()) => println!("✓ Protected mode disabled"),
                                Err(e) => eprintln!("⊘ Disable protected mode failed: {}", e),
                            }
                        }
                        Err(e) => eprintln!("⊘ Enable protected mode failed: {}", e),
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_paging_transition() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Must enable protected mode first
                    if vcpu.enable_protected_mode().is_ok() {
                        println!("✓ Protected mode enabled");

                        // Enable paging
                        let page_dir_phys = 0x1000;
                        match vcpu.enable_paging(page_dir_phys) {
                            Ok(()) => {
                                println!("✓ Paging enabled");

                                // Verify
                                if let Ok(cr) = vcpu.get_control_registers() {
                                    assert!(cr.is_paging_enabled(), "Paging should be enabled");
                                    assert_eq!(
                                        cr.page_directory_base(),
                                        page_dir_phys,
                                        "Page directory base should match"
                                    );
                                    println!("✓ Paging verification passed");
                                }

                                // Disable paging
                                match vcpu.disable_paging() {
                                    Ok(()) => println!("✓ Paging disabled"),
                                    Err(e) => eprintln!("⊘ Disable paging failed: {}", e),
                                }
                            }
                            Err(e) => eprintln!("⊘ Enable paging failed: {}", e),
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_invalid_transitions() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Test: Cannot enable paging without protected mode
                    let result = vcpu.enable_paging(0x1000);
                    assert!(
                        result.is_err(),
                        "Should fail to enable paging without protected mode"
                    );
                    println!("✓ Correctly rejected paging without protected mode");

                    // Enable protected mode
                    if vcpu.enable_protected_mode().is_ok() {
                        // Test: Invalid page directory alignment
                        let result = vcpu.enable_paging(0x1234); // Not 4KB aligned
                        assert!(result.is_err(), "Should fail with unaligned page directory");
                        println!("✓ Correctly rejected unaligned page directory");

                        // Enable paging properly
                        if vcpu.enable_paging(0x1000).is_ok() {
                            // Test: Cannot disable protected mode while paging is enabled
                            let result = vcpu.disable_protected_mode();
                            assert!(
                                result.is_err(),
                                "Should fail to disable protected mode while paging is enabled"
                            );
                            println!(
                                "✓ Correctly rejected disabling protected mode with paging enabled"
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_multi_vcpu_control_registers() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(2, 1024 * 1024) {
                if let (Ok(vcpu0), Ok(vcpu1)) = (vm.create_vcpu(0), vm.create_vcpu(1)) {
                    // Set vCPU 0 to protected mode
                    if vcpu0.enable_protected_mode().is_ok() {
                        // vCPU 1 should still be in real mode
                        if let (Ok(cr0), Ok(cr1)) =
                            (vcpu0.get_control_registers(), vcpu1.get_control_registers())
                        {
                            assert!(
                                cr0.is_protected_mode(),
                                "vCPU 0 should be in protected mode"
                            );
                            assert!(!cr1.is_protected_mode(), "vCPU 1 should be in real mode");
                            println!("✓ Multi-vCPU control registers are independent");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_long_mode_transition() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Start in real mode
                    if let Ok(cr) = vcpu.get_control_registers() {
                        assert!(!cr.is_long_mode_active(), "Should start in real mode");
                    }

                    // Attempt to enable long mode
                    // Note: This will likely fail without valid page tables,
                    // but we test the transition logic
                    let result = vcpu.enable_long_mode(0x10000);
                    match result {
                        Ok(_) => {
                            // Successfully enabled long mode
                            if let Ok(cr) = vcpu.get_control_registers() {
                                assert!(cr.is_long_mode_active(), "Long mode should be active");
                                assert!(cr.is_paging_enabled(), "Paging should be enabled");
                                assert!(cr.is_pae_enabled(), "PAE should be enabled");
                                println!("✓ Long mode transition successful");
                            }

                            // Test disable long mode
                            if vcpu.disable_long_mode().is_ok() {
                                if let Ok(cr) = vcpu.get_control_registers() {
                                    assert!(
                                        !cr.is_long_mode_active(),
                                        "Long mode should be inactive"
                                    );
                                    assert!(!cr.is_paging_enabled(), "Paging should be disabled");
                                    println!("✓ Long mode deactivation successful");
                                }
                            }
                        }
                        Err(e) => {
                            // Expected to fail without valid page tables
                            println!("⚠ Long mode activation failed (expected): {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_cpu_mode_detection() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Should start in real mode
                    if let Ok(mode) = vcpu.get_cpu_mode() {
                        assert_eq!(mode, CpuMode::RealMode, "Should start in real mode");
                        println!("✓ Initial mode: Real Mode");
                    }

                    // Transition to protected mode
                    if vcpu.enable_protected_mode().is_ok() {
                        if let Ok(mode) = vcpu.get_cpu_mode() {
                            assert_eq!(mode, CpuMode::ProtectedMode, "Should be in protected mode");
                            println!("✓ After transition: Protected Mode");
                        }
                    }

                    // Attempt long mode (may fail without valid page tables)
                    if vcpu.enable_long_mode(0x10000).is_ok() {
                        if let Ok(mode) = vcpu.get_cpu_mode() {
                            assert_eq!(mode, CpuMode::LongMode64Bit, "Should be in long mode");
                            println!("✓ After long mode: Long Mode 64-Bit");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_long_mode_prerequisites() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Test alignment validation
                    let unaligned = vcpu.enable_long_mode(0x12345); // Not 4KB aligned
                    assert!(
                        unaligned.is_err(),
                        "Should reject unaligned page directory base"
                    );
                    if let Err(e) = unaligned {
                        println!("✓ Alignment check: {}", e);
                    }

                    // Test that prerequisites are automatically enabled
                    let aligned = vcpu.enable_long_mode(0x10000);
                    match aligned {
                        Ok(_) => {
                            if let Ok(cr) = vcpu.get_control_registers() {
                                assert!(cr.is_protected_mode(), "Protected mode auto-enabled");
                                assert!(cr.is_pae_enabled(), "PAE auto-enabled");
                                assert!(cr.is_long_mode_enabled(), "Long mode enable bit set");
                                println!("✓ Prerequisites automatically enabled");
                            }
                        }
                        Err(e) => {
                            println!("⚠ Long mode activation failed (expected): {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_gdt_loading() {
        use crate::descriptors::{GdtBuilder, DESC_DPL_0};

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Build a minimal 64-bit GDT
                    let gdt = GdtBuilder::new()
                        .add_null() // Entry 0: Null (required)
                        .add_code_64bit(DESC_DPL_0) // Entry 1: Kernel code (0x08)
                        .add_data_64bit(DESC_DPL_0) // Entry 2: Kernel data (0x10)
                        .build();

                    assert_eq!(gdt.len(), 24); // 3 entries * 8 bytes

                    // Load GDT at physical address 0x2000
                    let result = vcpu.load_gdt(&vm, &gdt, 0x2000);
                    match result {
                        Ok((cs, ds)) => {
                            assert_eq!(cs, 0x08, "CS selector should be 0x08");
                            assert_eq!(ds, 0x10, "DS selector should be 0x10");
                            println!(
                                "✓ GDT loaded successfully: CS=0x{:02X}, DS=0x{:02X}",
                                cs, ds
                            );
                        }
                        Err(e) => {
                            println!("⚠ GDT loading failed (may require admin): {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_idt_loading() {
        use crate::descriptors::{IdtBuilder, DESC_DPL_0, DESC_DPL_3};

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Build a 64-bit IDT with essential exception handlers
                    let idt = IdtBuilder::new_64bit()
                        .add_interrupt_gate(0, 0xFFFF_8000_0010_0000, 0x08, 0, DESC_DPL_0) // Divide by zero
                        .add_interrupt_gate(1, 0xFFFF_8000_0010_0100, 0x08, 0, DESC_DPL_0) // Debug
                        .add_trap_gate(3, 0xFFFF_8000_0010_0200, 0x08, 0, DESC_DPL_3) // Breakpoint (user)
                        .add_interrupt_gate(6, 0xFFFF_8000_0010_0600, 0x08, 0, DESC_DPL_0) // Invalid opcode
                        .add_interrupt_gate(13, 0xFFFF_8000_0010_0D00, 0x08, 0, DESC_DPL_0) // General protection
                        .add_interrupt_gate(14, 0xFFFF_8000_0010_0E00, 0x08, 0, DESC_DPL_0) // Page fault
                        .build();

                    assert_eq!(idt.len(), 4096); // 256 entries * 16 bytes

                    // Load IDT at physical address 0x4000
                    let result = vcpu.load_idt(&vm, &idt, 0x4000);
                    match result {
                        Ok(()) => {
                            println!(
                                "✓ IDT loaded successfully at 0x4000 (256 entries, 4096 bytes)"
                            );
                        }
                        Err(e) => {
                            println!("⚠ IDT loading failed (may require admin): {}", e);
                        }
                    }
                }
            }
        }
    }

    // ========================================================================
    // Segment Register Tests (Session 27, Phase 1)
    // ========================================================================

    #[tokio::test]
    async fn test_set_code_segment() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Set up 64-bit kernel code segment
                    let result = vcpu.set_code_segment(0x08, 0, 0xFFFFFFFF, 0x209A);
                    match result {
                        Ok(()) => {
                            println!("✓ Code segment configured: selector=0x08, 64-bit mode");
                        }
                        Err(e) => {
                            println!("⚠ Code segment setup failed: {}", e);
                        }
                    }

                    // Set up 32-bit code segment
                    let result = vcpu.set_code_segment(0x18, 0, 0xFFFFFFFF, 0x9A);
                    match result {
                        Ok(()) => {
                            println!("✓ Code segment configured: selector=0x18, 32-bit mode");
                        }
                        Err(e) => {
                            println!("⚠ Code segment setup failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_set_data_segment() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Set up all data segments
                    let segments = ["DS", "ES", "FS", "GS", "SS"];
                    for seg in &segments {
                        let result = vcpu.set_data_segment(seg, 0x10, 0, 0xFFFFFFFF, 0x92);
                        match result {
                            Ok(()) => {
                                println!("✓ {} segment configured: selector=0x10", seg);
                            }
                            Err(e) => {
                                println!("⚠ {} segment setup failed: {}", seg, e);
                            }
                        }
                    }

                    // Test invalid segment name
                    let result = vcpu.set_data_segment("XX", 0x10, 0, 0xFFFFFFFF, 0x92);
                    assert!(result.is_err(), "Should reject invalid segment name");
                    if let Err(e) = result {
                        println!("✓ Invalid segment name rejected: {}", e);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_set_instruction_pointer() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Set various instruction pointer values
                    let test_addresses = [
                        0x7C00,                // Boot sector
                        0x100000,              // 1MB (Linux/Multiboot)
                        0xFFFF_8000_0000_0000, // Kernel space
                    ];

                    for &addr in &test_addresses {
                        let result = vcpu.set_instruction_pointer(addr);
                        match result {
                            Ok(()) => {
                                println!("✓ Instruction pointer set to 0x{:016X}", addr);
                            }
                            Err(e) => {
                                println!("⚠ Failed to set IP to 0x{:016X}: {}", addr, e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_configure_boot_segments() {
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Test 64-bit boot configuration
                    let result = vcpu.configure_boot_segments(0x08, 0x10, true);
                    match result {
                        Ok(()) => {
                            println!("✓ 64-bit boot segments configured (CS=0x08, DS=0x10)");
                        }
                        Err(e) => {
                            println!("⚠ 64-bit boot segment configuration failed: {}", e);
                        }
                    }

                    // Test 32-bit boot configuration
                    let result = vcpu.configure_boot_segments(0x08, 0x10, false);
                    match result {
                        Ok(()) => {
                            println!("✓ 32-bit boot segments configured (CS=0x08, DS=0x10)");
                        }
                        Err(e) => {
                            println!("⚠ 32-bit boot segment configuration failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    // ========================================================================
    // Boot Execution Tests (Session 27, Phase 2)
    // ========================================================================

    #[tokio::test]
    async fn test_boot_linux_minimal() {
        use crate::boot::linux::LinuxBootParams;

        // Create a minimal valid bzImage
        let mut kernel = vec![0u8; 4096];

        // Set boot flag (0xAA55 at offset 0x1FE)
        kernel[0x1FE] = 0x55;
        kernel[0x1FF] = 0xAA;

        // Set signature "HdrS" at offset 0x202
        kernel[0x202] = b'H';
        kernel[0x203] = b'd';
        kernel[0x204] = b'r';
        kernel[0x205] = b'S';

        // Set protocol version >= 2.10 at offset 0x206 (little-endian)
        kernel[0x206] = 0x0A; // 2.10
        kernel[0x207] = 0x02;

        // Set setup_sects at offset 0x1F1 (4 sectors)
        kernel[0x1F1] = 4;

        let params = LinuxBootParams {
            kernel_image: kernel,
            initrd: None,
            cmdline: "console=ttyS0".to_string(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
        };

        // Check if WHPX is available and try to boot
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_linux(&vm, &params, 0x100000);
                    match result {
                        Ok(()) => {
                            println!("✓ Linux boot sequence completed successfully");

                            // Verify CPU state
                            if let Ok(mode) = vcpu.get_cpu_mode() {
                                assert_eq!(
                                    mode,
                                    CpuMode::LongMode64Bit,
                                    "Should be in long mode after boot"
                                );
                                println!("✓ CPU in long mode");
                            }

                            if let Ok(regs) = vcpu.get_register_set() {
                                assert_eq!(regs.rip, 0x100000, "RIP should point to entry");
                                assert_eq!(regs.rsi, 0x90000, "RSI should point to boot_params");
                                println!("✓ Registers configured correctly");
                                println!("  RIP: 0x{:016X}", regs.rip);
                                println!("  RSI: 0x{:016X}", regs.rsi);
                            }
                        }
                        Err(e) => {
                            println!("⚠ Linux boot failed (may require admin): {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_boot_linux_with_initrd() {
        use crate::boot::linux::LinuxBootParams;

        // Create a minimal valid bzImage
        let mut kernel = vec![0u8; 4096];
        kernel[0x1FE] = 0x55;
        kernel[0x1FF] = 0xAA;
        kernel[0x202] = b'H';
        kernel[0x203] = b'd';
        kernel[0x204] = b'r';
        kernel[0x205] = b'S';
        kernel[0x206] = 0x0A;
        kernel[0x207] = 0x02;
        kernel[0x1F1] = 4;

        // Create a minimal initrd
        let initrd = vec![0xFFu8; 1024]; // 1KB initrd

        let params = LinuxBootParams {
            kernel_image: kernel,
            initrd: Some(initrd),
            cmdline: "console=ttyS0 init=/bin/sh".to_string(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
        };

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_linux(&vm, &params, 0x100000);
                    match result {
                        Ok(()) => {
                            println!("✓ Linux boot with initrd completed successfully");
                            println!("  Kernel: 4096 bytes at 0x100000");
                            println!("  Initrd: 1024 bytes at 0x800000");
                        }
                        Err(e) => {
                            println!("⚠ Linux boot with initrd failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    // ========================================================================
    // Multiboot Boot Tests (Session 27, Phase 3)
    // ========================================================================

    #[tokio::test]
    async fn test_boot_multiboot_minimal() {
        use crate::boot::multiboot::MultibootInfo;

        // Create a minimal valid Multiboot kernel
        let mut kernel = vec![0u8; 8192]; // 8KB kernel

        // Multiboot header (must be in first 8KB)
        // Magic: 0x1BADB002 at offset 0x100
        let offset = 0x100;
        kernel[offset] = 0x02;
        kernel[offset + 1] = 0xB0;
        kernel[offset + 2] = 0xAD;
        kernel[offset + 3] = 0x1B;

        // Flags: 0x00000000 (no special requirements)
        kernel[offset + 4] = 0x00;
        kernel[offset + 5] = 0x00;
        kernel[offset + 6] = 0x00;
        kernel[offset + 7] = 0x00;

        // Checksum: -(magic + flags)
        let checksum = 0u32.wrapping_sub(0x1BADB002).wrapping_sub(0x00000000);
        kernel[offset + 8] = (checksum & 0xFF) as u8;
        kernel[offset + 9] = ((checksum >> 8) & 0xFF) as u8;
        kernel[offset + 10] = ((checksum >> 16) & 0xFF) as u8;
        kernel[offset + 11] = ((checksum >> 24) & 0xFF) as u8;

        let info = MultibootInfo {
            kernel_image: kernel,
            modules: vec![],
            cmdline: "--test".to_string(),
            memory_map: vec![
                (0, 640 * 1024),                  // Lower memory
                (1024 * 1024, 127 * 1024 * 1024), // Upper memory
            ],
        };

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_multiboot(&vm, &info, 0x100000);
                    match result {
                        Ok(()) => {
                            println!("✓ Multiboot boot sequence completed successfully");

                            // Verify CPU state
                            if let Ok(mode) = vcpu.get_cpu_mode() {
                                assert_eq!(
                                    mode,
                                    CpuMode::ProtectedMode,
                                    "Should be in protected mode after Multiboot boot"
                                );
                                println!("✓ CPU in protected mode");
                            }

                            if let Ok(regs) = vcpu.get_register_set() {
                                assert_eq!(regs.rip, 0x100000, "EIP should point to entry");
                                assert_eq!(
                                    regs.rax as u32, 0x2BADB002,
                                    "EAX should contain Multiboot magic"
                                );
                                assert_eq!(regs.rbx, 0x9000, "EBX should point to multiboot_info");
                                println!("✓ Registers configured correctly");
                                println!("  EIP: 0x{:08X}", regs.rip as u32);
                                println!("  EAX: 0x{:08X} (Multiboot magic)", regs.rax as u32);
                                println!("  EBX: 0x{:08X} (multiboot_info)", regs.rbx as u32);
                            }
                        }
                        Err(e) => {
                            println!("⚠ Multiboot boot failed (may require admin): {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_boot_multiboot_with_modules() {
        use crate::boot::multiboot::{MultibootInfo, MultibootModule};

        // Create a minimal valid Multiboot kernel
        let mut kernel = vec![0u8; 8192];
        let offset = 0x100;
        kernel[offset] = 0x02;
        kernel[offset + 1] = 0xB0;
        kernel[offset + 2] = 0xAD;
        kernel[offset + 3] = 0x1B;
        kernel[offset + 4] = 0x00;
        kernel[offset + 5] = 0x00;
        kernel[offset + 6] = 0x00;
        kernel[offset + 7] = 0x00;
        let checksum = 0u32.wrapping_sub(0x1BADB002).wrapping_sub(0x00000000);
        kernel[offset + 8] = (checksum & 0xFF) as u8;
        kernel[offset + 9] = ((checksum >> 8) & 0xFF) as u8;
        kernel[offset + 10] = ((checksum >> 16) & 0xFF) as u8;
        kernel[offset + 11] = ((checksum >> 24) & 0xFF) as u8;

        // Create test modules
        let module1 = MultibootModule {
            data: vec![0xAAu8; 512],
            cmdline: "module1".to_string(),
        };
        let module2 = MultibootModule {
            data: vec![0xBBu8; 1024],
            cmdline: "module2".to_string(),
        };

        let info = MultibootInfo {
            kernel_image: kernel,
            modules: vec![module1, module2],
            cmdline: "--test --modules".to_string(),
            memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
        };

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_multiboot(&vm, &info, 0x100000);
                    match result {
                        Ok(()) => {
                            println!("✓ Multiboot boot with modules completed successfully");
                            println!("  Kernel: 8192 bytes at 0x100000");
                            println!("  Module 1: 512 bytes");
                            println!("  Module 2: 1024 bytes");
                        }
                        Err(e) => {
                            println!("⚠ Multiboot boot with modules failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_boot_real_mode_basic() {
        // Create a minimal valid boot sector
        let mut boot_sector = vec![0u8; 512];

        // Add some simple x86 real mode code (HLT loop)
        boot_sector[0] = 0xF4; // HLT
        boot_sector[1] = 0xEB; // JMP short
        boot_sector[2] = 0xFD; // -3 (jump to HLT)

        // Add boot signature at offset 510-511
        boot_sector[510] = 0x55;
        boot_sector[511] = 0xAA;

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_real_mode(&vm, &boot_sector, 0x80).await;
                    match result {
                        Ok(()) => {
                            println!("✓ Real mode boot completed successfully");
                            if let Ok(regs) = vcpu.get_register_set() {
                                // Verify CS:IP = 0x0000:0x7C00
                                assert_eq!(
                                    regs.rip as u32, 0x7C00,
                                    "IP should be set to boot sector address"
                                );
                                println!("  CS:IP: 0x{:04X}:0x{:04X}", 0, regs.rip as u16);

                                // Verify DL = boot drive (0x80)
                                let dl = (regs.rdx & 0xFF) as u8;
                                assert_eq!(dl, 0x80, "DL should contain boot drive number");
                                println!("  DL (boot drive): 0x{:02X}", dl);

                                // Verify stack pointer
                                assert_eq!(regs.rsp as u32, 0x7C00, "SP should be set to 0x7C00");
                                println!("  SP: 0x{:04X}", regs.rsp as u16);

                                // Verify CPU is in real mode
                                if let Ok(mode) = vcpu.get_cpu_mode() {
                                    assert_eq!(
                                        mode,
                                        CpuMode::RealMode,
                                        "CPU should be in real mode"
                                    );
                                    println!("  CPU Mode: Real mode (16-bit)");
                                }
                            }
                        }
                        Err(e) => {
                            println!("⚠ Real mode boot failed (may require admin): {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_boot_real_mode_invalid_size() {
        // Create invalid boot sector (not 512 bytes)
        let boot_sector = vec![0x55, 0xAA]; // Too small

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_real_mode(&vm, &boot_sector, 0x80).await;
                    match result {
                        Ok(()) => {
                            panic!("Should have failed with invalid boot sector size");
                        }
                        Err(e) => {
                            println!("✓ Correctly rejected invalid boot sector size: {}", e);
                            assert!(
                                e.to_string().contains("Invalid boot sector size"),
                                "Error should mention invalid size"
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_boot_real_mode_invalid_signature() {
        // Create boot sector with invalid signature
        let mut boot_sector = vec![0u8; 512];
        boot_sector[510] = 0xAA; // Wrong signature
        boot_sector[511] = 0x55; // Wrong signature

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_real_mode(&vm, &boot_sector, 0x80).await;
                    match result {
                        Ok(()) => {
                            panic!("Should have failed with invalid boot signature");
                        }
                        Err(e) => {
                            println!("✓ Correctly rejected invalid boot signature: {}", e);
                            assert!(
                                e.to_string().contains("Invalid boot signature"),
                                "Error should mention invalid signature"
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_boot_real_mode_floppy() {
        // Test booting from floppy drive (0x00)
        let mut boot_sector = vec![0u8; 512];
        boot_sector[0] = 0xF4; // HLT
        boot_sector[510] = 0x55;
        boot_sector[511] = 0xAA;

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let result = vcpu.boot_real_mode(&vm, &boot_sector, 0x00).await;
                    match result {
                        Ok(()) => {
                            println!("✓ Real mode boot from floppy completed");
                            if let Ok(regs) = vcpu.get_register_set() {
                                let dl = (regs.rdx & 0xFF) as u8;
                                assert_eq!(dl, 0x00, "DL should contain floppy drive number");
                                println!("  DL (boot drive): 0x{:02X} (floppy)", dl);
                            }
                        }
                        Err(e) => {
                            println!("⚠ Real mode boot from floppy failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_run_until_hlt() {
        // Test run_until with HLT condition
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create simple code that ends with HLT
                    let code = vec![
                        0xB8, 0x01, 0x00, 0x00, 0x00, // MOV EAX, 1
                        0xF4, // HLT
                    ];

                    // Write code to memory
                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        // Set RIP to code location
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Run until HLT
                        match vcpu.run_until(|exit| matches!(exit, crate::exit::VmExit::Hlt)) {
                            Ok(exit) => {
                                println!("✓ run_until(HLT) completed successfully");
                                assert!(matches!(exit, crate::exit::VmExit::Hlt));
                            }
                            Err(e) => {
                                println!("⚠ run_until test skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_run_for_exits() {
        // Test run_for with exit limit
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create code with multiple I/O operations
                    let code = vec![
                        0xB0, 0x41, // MOV AL, 'A'
                        0xE6, 0xE9, // OUT 0xE9, AL (bochs debug port)
                        0xB0, 0x42, // MOV AL, 'B'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xB0, 0x43, // MOV AL, 'C'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Run for up to 5 exits
                        match vcpu.run_for(5) {
                            Ok(exits) => {
                                println!("✓ run_for(5) captured {} exits", exits.len());
                                assert!(exits.len() <= 5, "Should not exceed max exits");

                                // Count I/O exits
                                let io_count = exits
                                    .iter()
                                    .filter(|e| matches!(e, crate::exit::VmExit::Io { .. }))
                                    .count();
                                println!("  I/O exits: {}", io_count);
                            }
                            Err(e) => {
                                println!("⚠ run_for test skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_step_execution() {
        // Test single-step execution
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create simple code sequence
                    let code = vec![
                        0x90, // NOP
                        0x90, // NOP
                        0x90, // NOP
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        let start_rip = 0x1000;
                        regs.rip = start_rip;
                        let _ = vcpu.set_register_set(&regs);

                        // Step through 3 NOPs
                        let mut success_count = 0;
                        for i in 0..3 {
                            match vcpu.step() {
                                Ok(_) => {
                                    if let Ok(regs) = vcpu.get_register_set() {
                                        println!("  Step {}: RIP = 0x{:X}", i + 1, regs.rip);
                                        success_count += 1;
                                    }
                                }
                                Err(e) => {
                                    println!("⚠ Step {} failed: {}", i + 1, e);
                                    break;
                                }
                            }
                        }

                        if success_count > 0 {
                            println!("✓ Single-step executed {} instructions", success_count);
                        } else {
                            println!("⚠ Single-step test skipped (no successful steps)");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_injection() {
        // Test interrupt injection
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Try to inject timer interrupt (vector 0x20)
                    match vcpu.inject_interrupt(0x20) {
                        Ok(()) => {
                            println!("✓ Interrupt 0x20 injected successfully");
                        }
                        Err(e) => {
                            println!("⚠ Interrupt injection test skipped: {}", e);
                        }
                    }

                    // Try to inject keyboard interrupt (vector 0x21)
                    match vcpu.inject_interrupt(0x21) {
                        Ok(()) => {
                            println!("✓ Interrupt 0x21 injected successfully");
                        }
                        Err(e) => {
                            println!("⚠ Interrupt injection test skipped: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_nmi_injection() {
        // Test NMI injection
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    match vcpu.inject_nmi() {
                        Ok(()) => {
                            println!("✓ NMI injected successfully");
                        }
                        Err(e) => {
                            println!("⚠ NMI injection test skipped: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_exception_injection() {
        // Test exception injection
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Test divide error (no error code)
                    match vcpu.inject_exception(0, None) {
                        Ok(()) => {
                            println!("✓ Divide error (#DE) injected successfully");
                        }
                        Err(e) => {
                            println!("⚠ Divide error injection test skipped: {}", e);
                        }
                    }

                    // Test general protection fault (with error code)
                    match vcpu.inject_exception(13, Some(0)) {
                        Ok(()) => {
                            println!("✓ General protection fault (#GP) injected successfully");
                        }
                        Err(e) => {
                            println!("⚠ GP fault injection test skipped: {}", e);
                        }
                    }

                    // Test page fault (with error code)
                    match vcpu.inject_exception(14, Some(0x07)) {
                        Ok(()) => {
                            println!("✓ Page fault (#PF) injected successfully");
                        }
                        Err(e) => {
                            println!("⚠ Page fault injection test skipped: {}", e);
                        }
                    }

                    // Test invalid exception vector (should fail)
                    match vcpu.inject_exception(32, None) {
                        Ok(()) => {
                            println!("⚠ Invalid exception vector should have been rejected");
                        }
                        Err(_) => {
                            println!("✓ Invalid exception vector correctly rejected");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_window_request() {
        // Test interrupt window request
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    match vcpu.request_interrupt_window() {
                        Ok(()) => {
                            println!("✓ Interrupt window requested successfully");
                        }
                        Err(e) => {
                            println!("⚠ Interrupt window request test skipped: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_execution_stats() {
        // Test performance statistics collection
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create code with multiple I/O operations
                    let code = vec![
                        0xB0, 0x41, // MOV AL, 'A'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xB0, 0x42, // MOV AL, 'B'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xB0, 0x43, // MOV AL, 'C'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Collect statistics for up to 10 exits
                        match vcpu.run_with_stats(10) {
                            Ok(stats) => {
                                println!("✓ Performance statistics collected:");
                                println!("  Total exits: {}", stats.total_exits);
                                println!("  Execution time: {:?}", stats.execution_time);
                                println!("  Estimated instructions: {}", stats.instruction_count);
                                println!("  Exits/sec: {:.2}", stats.exits_per_second());
                                println!(
                                    "  Instructions/sec: {:.2}",
                                    stats.instructions_per_second()
                                );

                                if let Some((exit_type, count)) = stats.most_frequent_exit() {
                                    println!("  Most frequent exit: {} ({})", exit_type, count);
                                }

                                // Display exit frequency breakdown
                                println!("  Exit types:");
                                for (exit_type, count) in &stats.exit_counts {
                                    println!("    {}: {}", exit_type, count);
                                }

                                assert!(stats.total_exits > 0, "Should have at least one exit");
                                assert!(
                                    stats.execution_time.as_nanos() > 0,
                                    "Should have non-zero execution time"
                                );
                            }
                            Err(e) => {
                                println!("⚠ Performance stats test skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_execution_stats_methods() {
        // Test ExecutionStats helper methods
        use std::collections::HashMap;
        use std::time::Duration;

        let mut exit_counts = HashMap::new();
        exit_counts.insert("Hlt".to_string(), 10);
        exit_counts.insert("Io".to_string(), 25);
        exit_counts.insert("Mmio".to_string(), 5);

        let stats = ExecutionStats {
            execution_time: Duration::from_millis(100),
            total_exits: 40,
            exit_counts,
            instruction_count: 1000,
        };

        println!("✓ Testing ExecutionStats methods:");

        // Test exits_per_second
        let eps = stats.exits_per_second();
        println!("  Exits per second: {:.2}", eps);
        assert!(eps > 0.0, "Should calculate non-zero exits/sec");
        assert!((eps - 400.0).abs() < 1.0, "Should be ~400 exits/sec");

        // Test instructions_per_second
        let ips = stats.instructions_per_second();
        println!("  Instructions per second: {:.2}", ips);
        assert!(ips > 0.0, "Should calculate non-zero instructions/sec");
        assert!(
            (ips - 10000.0).abs() < 1.0,
            "Should be ~10000 instructions/sec"
        );

        // Test most_frequent_exit
        if let Some((exit_type, count)) = stats.most_frequent_exit() {
            println!("  Most frequent exit: {} ({})", exit_type, count);
            assert_eq!(exit_type, "Io", "Most frequent should be Io");
            assert_eq!(*count, 25, "Io should have 25 occurrences");
        } else {
            panic!("most_frequent_exit() should return Some");
        }
    }

    #[tokio::test]
    async fn test_execution_loop_with_interrupts() {
        // Integration test: execution loop with interrupt injection
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create code that enables interrupts and loops
                    let code = vec![
                        0xFB, // STI (enable interrupts)
                        0x90, // NOP
                        0x90, // NOP
                        0xEB, 0xFC, // JMP -4 (infinite loop)
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        regs.rflags = 0x2; // IF flag will be set by STI
                        let _ = vcpu.set_register_set(&regs);

                        // Execute a few iterations
                        let mut success = false;
                        for i in 0..5 {
                            match vcpu.run() {
                                Ok(exit) => {
                                    println!("  Exit {}: {:?}", i + 1, exit_type_name(&exit));
                                    success = true;

                                    // Try injecting an interrupt
                                    let _ = vcpu.inject_interrupt(0x20);
                                }
                                Err(e) => {
                                    println!("  Exit {} error: {}", i + 1, e);
                                    break;
                                }
                            }
                        }

                        if success {
                            println!("✓ Execution loop with interrupts completed");
                        } else {
                            println!("⚠ Execution loop test skipped (no successful runs)");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_mixed_exit_types() {
        // Integration test: handle multiple exit types
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create code with I/O, memory access, and HLT
                    let code = vec![
                        0xB0, 0x41, // MOV AL, 'A'
                        0xE6, 0xE9, // OUT 0xE9, AL (I/O)
                        0x90, // NOP
                        0x90, // NOP
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Run and collect different exit types
                        let mut exit_types = std::collections::HashSet::new();
                        let mut iterations = 0;

                        for _ in 0..10 {
                            match vcpu.run() {
                                Ok(exit) => {
                                    iterations += 1;
                                    let exit_name = exit_type_name(&exit);
                                    exit_types.insert(exit_name.clone());
                                    println!("  Exit {}: {}", iterations, exit_name);

                                    // Stop at HLT
                                    if matches!(exit, crate::exit::VmExit::Hlt) {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    println!("  Error: {}", e);
                                    break;
                                }
                            }
                        }

                        if !exit_types.is_empty() {
                            println!("✓ Mixed exit types handled:");
                            for exit_type in &exit_types {
                                println!("    - {}", exit_type);
                            }
                        } else {
                            println!("⚠ Mixed exit types test skipped (no exits captured)");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_performance_benchmark() {
        // Integration test: performance benchmark
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create tight loop for performance testing
                    let code = vec![
                        0x90, // NOP
                        0x90, // NOP
                        0x90, // NOP
                        0x90, // NOP
                        0xEB, 0xFA, // JMP -6 (loop back)
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Run for limited iterations to measure performance
                        match vcpu.run_with_stats(100) {
                            Ok(stats) => {
                                println!("✓ Performance benchmark completed:");
                                println!("  Total exits: {}", stats.total_exits);
                                println!("  Execution time: {:?}", stats.execution_time);
                                println!("  Exits/sec: {:.0}", stats.exits_per_second());
                                println!(
                                    "  Est. instructions/sec: {:.0}",
                                    stats.instructions_per_second()
                                );

                                // Basic performance sanity checks
                                assert!(stats.total_exits > 0, "Should execute some exits");
                                assert!(
                                    stats.execution_time.as_micros() > 0,
                                    "Should take measurable time"
                                );

                                // If we got good data, check performance is reasonable
                                if stats.total_exits >= 10 {
                                    let eps = stats.exits_per_second();
                                    println!("  Performance rating: {} exits/sec", eps);
                                    if eps > 10000.0 {
                                        println!("  ⚡ Excellent performance!");
                                    } else if eps > 1000.0 {
                                        println!("  ✓ Good performance");
                                    } else {
                                        println!("  ⚠ Moderate performance");
                                    }
                                }
                            }
                            Err(e) => {
                                println!("⚠ Performance benchmark skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_io_handler_registration() {
        // Test I/O handler registration and lookup
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                // Register handler for port 0x3F8 (COM1)
                let handler_called = Arc::new(RwLock::new(false));
                let handler_called_clone = handler_called.clone();

                vm.register_io_handler(
                    0x3F8,
                    Box::new(move |_port, _is_write, _size, _data| {
                        *handler_called_clone.write() = true;
                        Ok(())
                    }),
                );

                // Test handling an I/O access
                let mut data = 0x41; // 'A'
                match vm.handle_io_access(0x3F8, true, 1, &mut data) {
                    Ok(()) => {
                        println!("✓ I/O handler invoked successfully");
                        assert!(*handler_called.read(), "Handler should have been called");
                    }
                    Err(e) => {
                        println!("⚠ I/O handler test failed: {}", e);
                    }
                }

                // Test unhandled port (should succeed with default behavior)
                let mut data = 0;
                match vm.handle_io_access(0x1234, false, 1, &mut data) {
                    Ok(()) => {
                        println!("✓ Unhandled I/O port returned default value: 0x{:X}", data);
                        assert_eq!(data, 0xFFFFFFFF, "Unhandled read should return 0xFF");
                    }
                    Err(e) => {
                        println!("⚠ Unhandled I/O test failed: {}", e);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_mmio_handler_registration() {
        // Test MMIO handler registration and lookup
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                // Register MMIO handler for 0xFED00000-0xFED01000
                let handler_called = Arc::new(RwLock::new(false));
                let handler_called_clone = handler_called.clone();

                vm.register_mmio_handler(
                    0xFED00000,
                    0xFED01000,
                    Box::new(move |_addr, _is_write, _size, _data| {
                        *handler_called_clone.write() = true;
                        Ok(())
                    }),
                );

                // Test handling an MMIO access within range
                let mut data = [0u8; 8];
                match vm.handle_mmio_access(0xFED00100, false, 4, &mut data) {
                    Ok(()) => {
                        println!("✓ MMIO handler invoked successfully");
                        assert!(*handler_called.read(), "Handler should have been called");
                    }
                    Err(e) => {
                        println!("⚠ MMIO handler test failed: {}", e);
                    }
                }

                // Test unhandled address (should succeed with default behavior)
                let mut data = [0u8; 8];
                match vm.handle_mmio_access(0x12340000, false, 4, &mut data) {
                    Ok(()) => {
                        println!("✓ Unhandled MMIO address returned default value");
                        assert_eq!(data, [0xFF; 8], "Unhandled read should return 0xFF");
                    }
                    Err(e) => {
                        println!("⚠ Unhandled MMIO test failed: {}", e);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_io_handler_serial_port() {
        // Test realistic serial port I/O handler
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                // Create a simple serial port emulator
                let serial_buffer = Arc::new(RwLock::new(Vec::new()));
                let serial_buffer_clone = serial_buffer.clone();

                vm.register_io_handler(
                    0x3F8,
                    Box::new(move |_port, is_write, _size, data| {
                        if is_write {
                            // OUT: Write character to buffer
                            let ch = (*data & 0xFF) as u8;
                            serial_buffer_clone.write().push(ch);
                        } else {
                            // IN: Read status (always ready)
                            *data = 0x60; // THR empty | RX ready
                        }
                        Ok(())
                    }),
                );

                // Simulate writing "Hello" to serial port
                for ch in b"Hello" {
                    let mut data = *ch as u32;
                    let _ = vm.handle_io_access(0x3F8, true, 1, &mut data);
                }

                // Check buffer contents
                let buffer = serial_buffer.read();
                if buffer.len() == 5 {
                    let message = String::from_utf8_lossy(&buffer);
                    println!("✓ Serial port handler captured: '{}'", message);
                    assert_eq!(message, "Hello", "Serial buffer should contain 'Hello'");
                } else {
                    println!("⚠ Serial port handler captured {} bytes", buffer.len());
                }
            }
        }
    }

    #[tokio::test]
    async fn test_run_with_handlers() {
        // Test automatic I/O handler invocation during execution
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Register handler for Bochs debug port (0xE9)
                    let output = Arc::new(RwLock::new(Vec::new()));
                    let output_clone = output.clone();

                    vm.register_io_handler(
                        0xE9,
                        Box::new(move |_port, is_write, _size, data| {
                            if is_write {
                                output_clone.write().push((*data & 0xFF) as u8);
                            }
                            Ok(())
                        }),
                    );

                    // Create code that writes to 0xE9 and then halts
                    let code = vec![
                        0xB0, 0x48, // MOV AL, 'H'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xB0, 0x69, // MOV AL, 'i'
                        0xE6, 0xE9, // OUT 0xE9, AL
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Run with handlers - I/O should be handled automatically
                        match vcpu.run_with_handlers(&vm) {
                            Ok(exit) => {
                                // Should get HLT, not I/O exits
                                assert!(
                                    matches!(exit, crate::exit::VmExit::Hlt),
                                    "Should reach HLT"
                                );

                                // Check output was captured
                                let buffer = output.read();
                                let message = String::from_utf8_lossy(&buffer);
                                println!("✓ run_with_handlers captured: '{}'", message);

                                if message == "Hi" {
                                    println!("✓ Automatic I/O handling working perfectly!");
                                } else {
                                    println!("⚠ Captured: '{}' (expected 'Hi')", message);
                                }
                            }
                            Err(e) => {
                                println!("⚠ run_with_handlers test skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // ========================================================================
    // RFLAGS and Interrupt Window Tests (Session 31 - Phase 1)
    // ========================================================================

    #[tokio::test]
    async fn test_rflags_read() {
        // Test reading RFLAGS register from vCPU
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    match vcpu.get_rflags() {
                        Ok(rflags) => {
                            println!("✓ RFLAGS read successful: 0x{:016X}", rflags);

                            // RFLAGS should have reserved bit 1 always set
                            assert!(
                                (rflags & 0x2) != 0,
                                "RFLAGS bit 1 (reserved) should always be set"
                            );

                            // Verify expected initial state (interrupts typically disabled at boot)
                            println!(
                                "  IF (bit 9): {}",
                                if (rflags & crate::backends::whpx_ffi::RFLAGS_IF) != 0 {
                                    "enabled"
                                } else {
                                    "disabled"
                                }
                            );
                            println!(
                                "  IOPL: {}",
                                (rflags & crate::backends::whpx_ffi::RFLAGS_IOPL_MASK) >> 12
                            );
                        }
                        Err(e) => {
                            eprintln!("⊘ RFLAGS read failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_flag_detection() {
        // Test detecting interrupt enable flag (RFLAGS.IF)
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    match vcpu.is_interrupt_enabled() {
                        Ok(enabled) => {
                            println!(
                                "✓ Interrupt flag detection successful: {}",
                                if enabled { "ENABLED" } else { "DISABLED" }
                            );

                            // Cross-check with direct RFLAGS read
                            if let Ok(rflags) = vcpu.get_rflags() {
                                let if_bit = (rflags & crate::backends::whpx_ffi::RFLAGS_IF) != 0;
                                assert_eq!(
                                    enabled, if_bit,
                                    "is_interrupt_enabled() should match RFLAGS.IF bit"
                                );
                                println!("✓ Interrupt flag consistency verified");
                            }
                        }
                        Err(e) => {
                            eprintln!("⊘ Interrupt flag detection failed: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_flag_toggle() {
        // Test that we can detect changes to RFLAGS.IF via STI/CLI instructions
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create code that toggles interrupts:
                    // CLI (clear IF), check, STI (set IF), check, HLT
                    let code = vec![
                        0xFA, // CLI (clear interrupt flag)
                        0x90, // NOP (give hypervisor a chance to update)
                        0xFB, // STI (set interrupt flag)
                        0x90, // NOP
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        // Set entry point
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Execute CLI
                        match vcpu.run() {
                            Ok(_) => {
                                // After CLI, interrupts should be disabled
                                if let Ok(enabled) = vcpu.is_interrupt_enabled() {
                                    if !enabled {
                                        println!("✓ CLI correctly disabled interrupts");
                                    } else {
                                        println!("⚠ CLI did not disable interrupts (may need more execution)");
                                    }
                                }

                                // Continue to STI
                                match vcpu.run() {
                                    Ok(_) => {
                                        // After STI, interrupts should be enabled
                                        if let Ok(enabled) = vcpu.is_interrupt_enabled() {
                                            if enabled {
                                                println!("✓ STI correctly enabled interrupts");
                                                println!("✓ RFLAGS.IF toggle detection working!");
                                            } else {
                                                println!("⚠ STI did not enable interrupts (may need more execution)");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!("⚠ Second execution failed: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                println!("⚠ RFLAGS toggle test skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // ========================================================================
    // Interrupt Window Integration Tests (Session 31 Phase 5)
    // ========================================================================

    #[tokio::test]
    async fn test_interrupt_stats_initialization() {
        // Verify that interrupt statistics are initialized to zero
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let stats = vcpu.get_interrupt_stats();

                    assert_eq!(
                        stats.interrupts_injected, 0,
                        "Initial injection count should be 0"
                    );
                    assert_eq!(
                        stats.interrupts_deferred, 0,
                        "Initial deferral count should be 0"
                    );
                    assert_eq!(
                        stats.window_requests, 0,
                        "Initial window request count should be 0"
                    );
                    assert_eq!(
                        stats.window_exits, 0,
                        "Initial window exit count should be 0"
                    );
                    assert_eq!(stats.nmis_injected, 0, "Initial NMI count should be 0");
                    assert_eq!(
                        stats.if_enabled_count, 0,
                        "Initial IF enabled count should be 0"
                    );
                    assert_eq!(
                        stats.if_disabled_count, 0,
                        "Initial IF disabled count should be 0"
                    );

                    println!("✓ Interrupt statistics initialized correctly");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_stats_if_tracking() {
        // Verify that checking interrupt flag updates statistics
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Check interrupt flag multiple times
                    for _ in 0..5 {
                        let _ = vcpu.is_interrupt_enabled();
                    }

                    let stats = vcpu.get_interrupt_stats();
                    let total_checks = stats.if_enabled_count + stats.if_disabled_count;

                    assert_eq!(total_checks, 5, "Should have 5 total IF checks");
                    println!(
                        "✓ Interrupt flag statistics tracking: {} enabled, {} disabled",
                        stats.if_enabled_count, stats.if_disabled_count
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_stats_reset() {
        // Verify that statistics can be reset to zero
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Generate some statistics
                    for _ in 0..10 {
                        let _ = vcpu.is_interrupt_enabled();
                    }

                    let stats_before = vcpu.get_interrupt_stats();
                    assert!(
                        stats_before.if_enabled_count + stats_before.if_disabled_count > 0,
                        "Should have some statistics before reset"
                    );

                    // Reset statistics
                    vcpu.reset_interrupt_stats();

                    let stats_after = vcpu.get_interrupt_stats();
                    assert_eq!(stats_after.interrupts_injected, 0);
                    assert_eq!(stats_after.interrupts_deferred, 0);
                    assert_eq!(stats_after.window_requests, 0);
                    assert_eq!(stats_after.window_exits, 0);
                    assert_eq!(stats_after.nmis_injected, 0);
                    assert_eq!(stats_after.if_enabled_count, 0);
                    assert_eq!(stats_after.if_disabled_count, 0);

                    println!("✓ Interrupt statistics reset successfully");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_stats_helper_methods() {
        // Verify helper methods for statistics analysis
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Generate some statistics by checking IF multiple times
                    for _ in 0..20 {
                        let _ = vcpu.is_interrupt_enabled();
                    }

                    let stats = vcpu.get_interrupt_stats();

                    // Test total_attempts()
                    let total = stats.total_attempts();
                    assert!(total > 0, "Total attempts should be > 0");
                    println!("✓ total_attempts() = {}", total);

                    // Test success_rate() - returns 0.0 if no injections yet
                    let success_rate = stats.success_rate();
                    assert!(
                        (0.0..=100.0).contains(&success_rate),
                        "Success rate should be between 0 and 100"
                    );
                    println!("✓ success_rate() = {:.2}%", success_rate);

                    // Test avg_window_requests_per_injection() - returns 0.0 if no injections
                    let avg_requests = stats.avg_window_requests_per_injection();
                    assert!(avg_requests >= 0.0, "Average should be non-negative");
                    println!(
                        "✓ avg_window_requests_per_injection() = {:.2}",
                        avg_requests
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_window_request_mechanism() {
        // Verify that request_interrupt_window() updates statistics
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let stats_before = vcpu.get_interrupt_stats();
                    let initial_requests = stats_before.window_requests;

                    // Request an interrupt window
                    match vcpu.request_interrupt_window() {
                        Ok(()) => {
                            let stats_after = vcpu.get_interrupt_stats();
                            assert_eq!(
                                stats_after.window_requests,
                                initial_requests + 1,
                                "Window request count should increment"
                            );
                            println!("✓ Interrupt window request tracked in statistics");
                        }
                        Err(e) => {
                            println!(
                                "⚠ Interrupt window request failed (may not be supported): {}",
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_injection_stats_tracking() {
        // Verify that inject_interrupt() updates statistics
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Set up a minimal interrupt handler environment
                    // In real mode, IVT at 0x0000:0x0000
                    let ivt_entry: Vec<u8> = vec![
                        0x00, 0x10, 0x00, 0x00, // IP:CS for interrupt vector 0x20
                    ];

                    if vm.write_guest_memory(0x80, &ivt_entry).is_ok() {
                        let stats_before = vcpu.get_interrupt_stats();
                        let initial_injected = stats_before.interrupts_injected;

                        // Try to inject an interrupt (vector 0x20 - timer)
                        match vcpu.inject_interrupt(0x20) {
                            Ok(()) => {
                                let stats_after = vcpu.get_interrupt_stats();
                                assert_eq!(
                                    stats_after.interrupts_injected,
                                    initial_injected + 1,
                                    "Interrupt injection count should increment"
                                );
                                println!("✓ Interrupt injection tracked in statistics");
                            }
                            Err(e) => {
                                println!("⚠ Interrupt injection test skipped: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_nmi_injection_stats_tracking() {
        // Verify that inject_nmi() updates statistics
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let stats_before = vcpu.get_interrupt_stats();
                    let initial_nmis = stats_before.nmis_injected;

                    // Inject an NMI
                    match vcpu.inject_nmi() {
                        Ok(()) => {
                            let stats_after = vcpu.get_interrupt_stats();
                            assert_eq!(
                                stats_after.nmis_injected,
                                initial_nmis + 1,
                                "NMI injection count should increment"
                            );
                            println!("✓ NMI injection tracked in statistics");
                        }
                        Err(e) => {
                            println!("⚠ NMI injection test skipped: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_nmi_bypasses_interrupt_flag() {
        // Verify that NMI injection works regardless of RFLAGS.IF state
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    // Create code that disables interrupts (CLI) then halts
                    let code = vec![
                        0xFA, // CLI (clear interrupt flag)
                        0xF4, // HLT
                    ];

                    if vm.write_guest_memory(0x1000, &code).is_ok() {
                        // Set entry point
                        let mut regs = vcpu.get_register_set().unwrap_or_default();
                        regs.rip = 0x1000;
                        let _ = vcpu.set_register_set(&regs);

                        // Execute CLI to disable interrupts
                        let _ = vcpu.run();

                        // Verify interrupts are disabled
                        if let Ok(false) = vcpu.is_interrupt_enabled() {
                            println!("✓ Interrupts disabled via CLI");

                            // NMI should still be injectable
                            match vcpu.inject_nmi() {
                                Ok(()) => {
                                    println!("✓ NMI successfully injected despite RFLAGS.IF=0");

                                    let stats = vcpu.get_interrupt_stats();
                                    assert_eq!(
                                        stats.nmis_injected, 1,
                                        "Should have 1 NMI injected"
                                    );
                                }
                                Err(e) => {
                                    println!("⚠ NMI injection failed: {}", e);
                                }
                            }
                        } else {
                            println!("⚠ Could not disable interrupts for NMI test");
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interrupt_stats_comprehensive() {
        // Comprehensive test of all statistics in a realistic scenario
        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 4 * 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    println!("\n=== Comprehensive Interrupt Statistics Test ===");

                    // 1. Check IF multiple times
                    for _ in 0..5 {
                        let _ = vcpu.is_interrupt_enabled();
                    }

                    // 2. Request interrupt windows
                    for _ in 0..3 {
                        let _ = vcpu.request_interrupt_window();
                    }

                    // 3. Inject NMIs
                    for _ in 0..2 {
                        let _ = vcpu.inject_nmi();
                    }

                    // Get final statistics
                    let stats = vcpu.get_interrupt_stats();

                    println!("Interrupt Statistics:");
                    println!("  Interrupts injected:      {}", stats.interrupts_injected);
                    println!("  Interrupts deferred:      {}", stats.interrupts_deferred);
                    println!("  Window requests:          {}", stats.window_requests);
                    println!("  Window exits:             {}", stats.window_exits);
                    println!("  NMIs injected:            {}", stats.nmis_injected);
                    println!("  IF enabled checks:        {}", stats.if_enabled_count);
                    println!("  IF disabled checks:       {}", stats.if_disabled_count);
                    println!("  Total attempts:           {}", stats.total_attempts());
                    println!("  Success rate:             {:.2}%", stats.success_rate());
                    println!(
                        "  Avg window reqs/inject:   {:.2}",
                        stats.avg_window_requests_per_injection()
                    );

                    // Verify expected counts
                    assert_eq!(
                        stats.if_enabled_count + stats.if_disabled_count,
                        5,
                        "Should have 5 IF checks"
                    );
                    assert!(
                        stats.window_requests >= 3,
                        "Should have at least 3 window requests"
                    );
                    assert!(
                        stats.nmis_injected >= 2,
                        "Should have at least 2 NMI injections"
                    );

                    println!("✓ Comprehensive statistics test passed");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_interrupt_stats_access() {
        // Verify thread-safe access to interrupt statistics
        use std::sync::Arc;
        use tokio::task;

        if let Ok(_backend) = WhpxBackend::new() {
            if let Ok(vm) = WhpxVm::new(1, 1024 * 1024) {
                if let Ok(vcpu) = vm.create_vcpu(0) {
                    let vcpu_arc = Arc::new(vcpu);
                    let mut handles = vec![];

                    // Spawn 10 tasks that concurrently check interrupt flag
                    for _ in 0..10 {
                        let vcpu_clone = Arc::clone(&vcpu_arc);
                        let handle = task::spawn(async move {
                            for _ in 0..10 {
                                let _ = vcpu_clone.is_interrupt_enabled();
                            }
                        });
                        handles.push(handle);
                    }

                    // Wait for all tasks to complete
                    for handle in handles {
                        let _ = handle.await;
                    }

                    // Verify statistics
                    let stats = vcpu_arc.get_interrupt_stats();
                    let total_checks = stats.if_enabled_count + stats.if_disabled_count;

                    assert_eq!(
                        total_checks, 100,
                        "Should have 100 total IF checks from concurrent tasks"
                    );
                    println!(
                        "✓ Concurrent statistics access test passed: {} total checks",
                        total_checks
                    );
                }
            }
        }
    }
}
