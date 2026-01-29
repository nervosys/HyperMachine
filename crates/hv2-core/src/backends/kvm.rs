//! KVM (Kernel-based Virtual Machine) backend
//!
//! This module provides a hypervisor backend that uses Linux KVM for
//! hardware-accelerated virtualization.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         AetherVM Application            │
//! ├─────────────────────────────────────────┤
//! │      HypervisorBackend Trait            │
//! ├─────────────────────────────────────────┤
//! │         KvmBackend (this file)          │
//! │  ┌─────────────────────────────────┐    │
//! │  │   KvmVm                          │    │
//! │  │  ┌──────────────────────────┐   │    │
//! │  │  │  KvmVcpu (per-vCPU)      │   │    │
//! │  │  │  - vcpu_fd               │   │    │
//! │  │  │  - run (kvm_run* mmap)   │   │    │
//! │  │  └──────────────────────────┘   │    │
//! │  └─────────────────────────────────┘    │
//! ├─────────────────────────────────────────┤
//! │           KVM FFI bindings              │
//! ├─────────────────────────────────────────┤
//! │        Linux KVM kernel module          │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use hv2_core::backends::kvm::KvmBackend;
//! use hv2_core::hypervisor::HypervisorBackend;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut backend = KvmBackend::new()?;
//! backend.init().await?;
//!
//! let vm = backend.create_vm(4, 1024 * 1024 * 1024).await?; // 4 vCPUs, 1GB RAM
//! # Ok(())
//! # }
//! ```

use super::kvm_ffi::*;
use crate::hypervisor::{
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm,
};
use crate::{Error, IoDirection, Result, VCpu, VmExit};
use async_trait::async_trait;
use std::os::unix::io::RawFd;
use std::ptr::NonNull;
use std::sync::{Arc, RwLock};

/// KVM hypervisor backend
///
/// This backend uses the Linux Kernel-based Virtual Machine (KVM) API
/// for hardware-accelerated virtualization.
///
/// # Requirements
///
/// - Linux kernel with KVM support (`CONFIG_KVM=y` or `=m`)
/// - `/dev/kvm` device must be accessible (usually requires `kvm` group membership)
/// - CPU with hardware virtualization support (Intel VT-x or AMD-V)
///
/// # Thread Safety
///
/// This struct is thread-safe (`Send + Sync`). The underlying KVM file
/// descriptors are safe to use from multiple threads.
pub struct KvmBackend {
    /// File descriptor for /dev/kvm
    kvm_fd: RawFd,
    /// Detected capabilities
    capabilities: HypervisorCapabilities,
    /// Size of kvm_run mmap region
    run_mmap_size: usize,
    /// Active VMs (for cleanup on drop)
    vms: Arc<RwLock<Vec<Arc<KvmVm>>>>,
}

impl KvmBackend {
    /// Create a new KVM backend
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `/dev/kvm` cannot be opened (permission denied, not found)
    /// - KVM API version is incompatible
    /// - Required capabilities are missing
    pub fn new() -> Result<Self> {
        unsafe {
            // Open /dev/kvm
            let kvm_fd = kvm_open().map_err(|e| {
                Error::Hypervisor(format!("Failed to open /dev/kvm: {}. Make sure KVM is enabled and you have permission to access it.", e))
            })?;

            // Check API version
            let api_version = kvm_get_api_version(kvm_fd).map_err(|e| {
                libc::close(kvm_fd);
                Error::Hypervisor(format!("Failed to get KVM API version: {}", e))
            })?;

            if api_version != KVM_API_VERSION as i32 {
                libc::close(kvm_fd);
                return Err(Error::Hypervisor(format!(
                    "KVM API version mismatch: expected {}, got {}",
                    KVM_API_VERSION, api_version
                )));
            }

            // Get mmap size for kvm_run
            let run_mmap_size = kvm_get_vcpu_mmap_size(kvm_fd).map_err(|e| {
                libc::close(kvm_fd);
                Error::Hypervisor(format!("Failed to get vCPU mmap size: {}", e))
            })?;

            // Detect capabilities
            let capabilities = Self::detect_capabilities(kvm_fd)?;

            Ok(Self {
                kvm_fd,
                capabilities,
                run_mmap_size,
                vms: Arc::new(RwLock::new(Vec::new())),
            })
        }
    }

    /// Detect KVM capabilities
    fn detect_capabilities(kvm_fd: RawFd) -> Result<HypervisorCapabilities> {
        unsafe {
            let check_cap = |cap: u32| -> bool {
                kvm_check_extension(kvm_fd, cap)
                    .map(|v| v > 0)
                    .unwrap_or(false)
            };

            Ok(HypervisorCapabilities {
                max_vcpus: 288, // KVM's default max (can be queried via KVM_CAP_MAX_VCPUS)
                max_memory: 4 * 1024 * 1024 * 1024 * 1024, // 4TB
                supports_nested_virt: check_cap(85), // KVM_CAP_NESTED_STATE
                supports_apic: check_cap(KVM_CAP_IRQCHIP),
                supports_x2apic: check_cap(117), // KVM_CAP_X2APIC_API
                supports_iommu: check_cap(196),  // KVM_CAP_IOMMU
                supports_gpu_passthrough: false, // Requires VFIO setup
            })
        }
    }
}

impl Drop for KvmBackend {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.kvm_fd);
        }
    }
}

#[async_trait]
impl HypervisorBackend for KvmBackend {
    fn platform(&self) -> HypervisorPlatform {
        HypervisorPlatform::Kvm
    }

    fn capabilities(&self) -> HypervisorCapabilities {
        self.capabilities.clone()
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!("Initialized KVM backend (API version {})", KVM_API_VERSION);
        tracing::debug!("Capabilities: {:?}", self.capabilities);
        Ok(())
    }

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

        // Create KVM VM instance
        let kvm_vm = Arc::new(KvmVm::new(
            self.kvm_fd,
            vcpu_count,
            memory_size,
            self.run_mmap_size,
        )?);

        // Track VM for cleanup
        self.vms.write().unwrap().push(kvm_vm.clone());

        Ok(HypervisorVm::new(
            HypervisorPlatform::Kvm,
            vcpu_count,
            memory_size,
        ))
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        // For now, return a stub implementation
        // Full implementation requires integrating with KvmVm/KvmVcpu
        tracing::debug!("KVM: Running vCPU {}", vcpu.id());

        // TODO: Implement full KVM vCPU execution
        // This will be completed after we integrate KvmVm/KvmVcpu with the VM structure

        Ok(VmExit::Hlt)
    }

    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
        tracing::debug!(
            "KVM: Injecting interrupt {} into vCPU {}",
            vector,
            vcpu.id()
        );

        // TODO: Implement interrupt injection
        // This requires access to the vCPU file descriptor

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down KVM backend");

        // VMs will be automatically closed when dropped
        self.vms.write().unwrap().clear();

        Ok(())
    }
}

/// KVM VM instance
///
/// Represents a single virtual machine managed by KVM.
/// Owns the VM file descriptor and associated vCPUs.
pub struct KvmVm {
    /// VM file descriptor
    vm_fd: RawFd,
    /// Number of vCPUs
    vcpu_count: u32,
    /// Memory size in bytes
    memory_size: u64,
    /// Guest memory (allocated on host)
    guest_memory: Option<NonNull<u8>>,
    /// vCPUs
    vcpus: RwLock<Vec<Arc<KvmVcpu>>>,
    /// Size of kvm_run mmap region
    run_mmap_size: usize,
}

impl KvmVm {
    /// Create a new KVM VM
    fn new(kvm_fd: RawFd, vcpu_count: u32, memory_size: u64, run_mmap_size: usize) -> Result<Self> {
        unsafe {
            // Create VM
            let vm_fd = kvm_create_vm(kvm_fd, 0)
                .map_err(|e| Error::Hypervisor(format!("Failed to create KVM VM: {}", e)))?;

            // Allocate guest memory
            let guest_memory = if memory_size > 0 {
                let layout = std::alloc::Layout::from_size_align(memory_size as usize, 4096)
                    .map_err(|e| {
                        libc::close(vm_fd);
                        Error::Memory(format!("Invalid memory layout: {}", e))
                    })?;

                let ptr = std::alloc::alloc_zeroed(layout);
                if ptr.is_null() {
                    libc::close(vm_fd);
                    return Err(Error::Memory("Failed to allocate guest memory".into()));
                }

                // Map guest memory into KVM
                let region = kvm_userspace_memory_region {
                    slot: 0,
                    flags: 0,
                    guest_phys_addr: 0,
                    memory_size,
                    userspace_addr: ptr as u64,
                };

                if let Err(e) = kvm_set_user_memory_region(vm_fd, &region) {
                    std::alloc::dealloc(ptr, layout);
                    libc::close(vm_fd);
                    return Err(Error::Hypervisor(format!(
                        "Failed to set user memory region: {}",
                        e
                    )));
                }

                Some(NonNull::new(ptr).unwrap())
            } else {
                None
            };

            // Set TSS address (required by KVM for x86)
            if let Err(e) = kvm_set_tss_addr(vm_fd, 0xfffbd000) {
                if let Some(ptr) = guest_memory {
                    let layout =
                        std::alloc::Layout::from_size_align_unchecked(memory_size as usize, 4096);
                    std::alloc::dealloc(ptr.as_ptr(), layout);
                }
                libc::close(vm_fd);
                return Err(Error::Hypervisor(format!(
                    "Failed to set TSS address: {}",
                    e
                )));
            }

            // Create IRQ chip (PIC, IOAPIC, LAPIC)
            if let Err(e) = kvm_create_irqchip(vm_fd) {
                tracing::warn!("Failed to create IRQ chip: {}. Interrupts may not work.", e);
                // Non-fatal: some setups work without IRQ chip
            }

            // Create PIT (timer)
            let pit_config = kvm_pit_config {
                flags: 0,
                pad: [0; 15],
            };
            if let Err(e) = kvm_create_pit2(vm_fd, &pit_config) {
                tracing::warn!("Failed to create PIT: {}. Timer may not work.", e);
                // Non-fatal: some setups work without PIT
            }

            Ok(Self {
                vm_fd,
                vcpu_count,
                memory_size,
                guest_memory,
                vcpus: RwLock::new(Vec::new()),
                run_mmap_size,
            })
        }
    }

    /// Create a vCPU
    pub fn create_vcpu(&self, vcpu_id: u32) -> Result<Arc<KvmVcpu>> {
        if vcpu_id >= self.vcpu_count {
            return Err(Error::Config(format!(
                "vCPU ID {} exceeds count {}",
                vcpu_id, self.vcpu_count
            )));
        }

        let vcpu = Arc::new(KvmVcpu::new(self.vm_fd, vcpu_id, self.run_mmap_size)?);
        self.vcpus.write().unwrap().push(vcpu.clone());
        Ok(vcpu)
    }

    /// Get guest memory pointer
    pub fn guest_memory(&self) -> Option<NonNull<u8>> {
        self.guest_memory
    }
}

impl Drop for KvmVm {
    fn drop(&mut self) {
        unsafe {
            // vCPUs will be dropped first (RAII order)
            self.vcpus.write().unwrap().clear();

            // Free guest memory
            if let Some(ptr) = self.guest_memory {
                let layout =
                    std::alloc::Layout::from_size_align_unchecked(self.memory_size as usize, 4096);
                std::alloc::dealloc(ptr.as_ptr(), layout);
            }

            // Close VM fd
            libc::close(self.vm_fd);
        }
    }
}

// Safety: KvmVm can be safely sent between threads
unsafe impl Send for KvmVm {}
unsafe impl Sync for KvmVm {}

/// KVM vCPU
///
/// Represents a single virtual CPU managed by KVM.
pub struct KvmVcpu {
    /// vCPU file descriptor
    vcpu_fd: RawFd,
    /// vCPU ID
    vcpu_id: u32,
    /// Pointer to mmap'd kvm_run structure
    run: NonNull<kvm_run>,
    /// Size of mmap region
    mmap_size: usize,
}

impl KvmVcpu {
    /// Create a new vCPU
    fn new(vm_fd: RawFd, vcpu_id: u32, mmap_size: usize) -> Result<Self> {
        unsafe {
            // Create vCPU
            let vcpu_fd = kvm_create_vcpu(vm_fd, vcpu_id).map_err(|e| {
                Error::Hypervisor(format!("Failed to create vCPU {}: {}", vcpu_id, e))
            })?;

            // mmap kvm_run structure
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                mmap_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                vcpu_fd,
                0,
            );

            if ptr == libc::MAP_FAILED {
                let err = std::io::Error::last_os_error();
                libc::close(vcpu_fd);
                return Err(Error::Hypervisor(format!(
                    "Failed to mmap kvm_run for vCPU {}: {}",
                    vcpu_id, err
                )));
            }

            let run = NonNull::new(ptr as *mut kvm_run).unwrap();

            // Initialize vCPU to real mode
            Self::init_real_mode(vcpu_fd)?;

            Ok(Self {
                vcpu_fd,
                vcpu_id,
                run,
                mmap_size,
            })
        }
    }

    /// Initialize vCPU to 16-bit real mode (like a PC at boot)
    unsafe fn init_real_mode(vcpu_fd: RawFd) -> Result<()> {
        // Set up registers for real mode boot
        let mut regs = kvm_regs::default();
        regs.rip = 0xfff0; // Reset vector
        regs.rflags = 0x2; // Reserved bit must be 1

        kvm_set_regs(vcpu_fd, &regs)
            .map_err(|e| Error::Hypervisor(format!("Failed to set registers: {}", e)))?;

        // Set up segment registers for real mode
        let mut sregs = kvm_sregs::default();

        // CS: base=0xFFFF0000, limit=0xFFFF, selector=0xF000
        sregs.cs.base = 0xFFFF0000;
        sregs.cs.limit = 0xFFFF;
        sregs.cs.selector = 0xF000;
        sregs.cs.type_ = 11; // Execute/read, accessed
        sregs.cs.present = 1;
        sregs.cs.dpl = 0;
        sregs.cs.db = 0;
        sregs.cs.s = 1;
        sregs.cs.l = 0;
        sregs.cs.g = 0;

        // Set up other segments
        let init_segment = |seg: &mut kvm_segment| {
            seg.base = 0;
            seg.limit = 0xFFFF;
            seg.selector = 0;
            seg.type_ = 3; // Read/write, accessed
            seg.present = 1;
            seg.dpl = 0;
            seg.db = 0;
            seg.s = 1;
            seg.l = 0;
            seg.g = 0;
        };

        init_segment(&mut sregs.ds);
        init_segment(&mut sregs.es);
        init_segment(&mut sregs.fs);
        init_segment(&mut sregs.gs);
        init_segment(&mut sregs.ss);

        // CR0: PE=0 (real mode), no paging
        sregs.cr0 = 0x60000010; // ET (extension type) + reserved bits
        sregs.cr4 = 0;
        sregs.efer = 0;

        kvm_set_sregs(vcpu_fd, &sregs)
            .map_err(|e| Error::Hypervisor(format!("Failed to set special registers: {}", e)))?;

        Ok(())
    }

    /// Run the vCPU until it exits
    pub fn run(&self) -> Result<VmExit> {
        unsafe {
            // Execute vCPU
            kvm_run(self.vcpu_fd).map_err(|e| {
                Error::Hypervisor(format!("KVM_RUN failed for vCPU {}: {}", self.vcpu_id, e))
            })?;

            // Convert KVM exit to VmExit
            self.convert_exit()
        }
    }

    /// Convert KVM exit reason to VmExit
    unsafe fn convert_exit(&self) -> Result<VmExit> {
        let run = self.run.as_ref();
        let exit_reason = run.exit_reason;

        match exit_reason {
            KVM_EXIT_HLT => Ok(VmExit::Hlt),

            KVM_EXIT_SHUTDOWN => Ok(VmExit::Shutdown),

            KVM_EXIT_IO => {
                let io = &run.exit_data.io;
                let direction = if io.direction == KVM_EXIT_IO_IN {
                    IoDirection::In
                } else {
                    IoDirection::Out
                };

                // Read data from the buffer (at offset from kvm_run start)
                let data_ptr =
                    (run as *const kvm_run as usize + io.data_offset as usize) as *const u32;
                let data = std::ptr::read(data_ptr);

                Ok(VmExit::Io {
                    port: io.port,
                    direction,
                    size: io.size,
                    data,
                })
            }

            KVM_EXIT_MMIO => {
                let mmio = &run.exit_data.mmio;
                Ok(VmExit::Mmio {
                    phys_addr: mmio.phys_addr,
                    data: mmio.data,
                    len: mmio.len,
                    is_write: mmio.is_write != 0,
                })
            }

            KVM_EXIT_IRQ_WINDOW_OPEN => Ok(VmExit::InterruptWindow),

            KVM_EXIT_EXCEPTION => {
                let ex = &run.exit_data.ex;
                Ok(VmExit::Exception {
                    vector: ex.exception as u8,
                    error_code: Some(ex.error_code),
                })
            }

            KVM_EXIT_INTERNAL_ERROR => {
                let internal = &run.exit_data.internal;
                Err(Error::Hypervisor(format!(
                    "KVM internal error: suberror={}, ndata={}",
                    internal.suberror, internal.ndata
                )))
            }

            KVM_EXIT_FAIL_ENTRY => {
                let fail = &run.exit_data.fail_entry;
                Err(Error::Hypervisor(format!(
                    "KVM failed to enter guest: reason={:#x}",
                    fail.hardware_entry_failure_reason
                )))
            }

            _ => Ok(VmExit::Unknown {
                reason: exit_reason,
            }),
        }
    }

    /// Inject an interrupt
    pub fn inject_interrupt(&self, vector: u8) -> Result<()> {
        unsafe {
            let irq = kvm_interrupt { irq: vector as u32 };
            kvm_interrupt(self.vcpu_fd, &irq).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to inject interrupt {} into vCPU {}: {}",
                    vector, self.vcpu_id, e
                ))
            })
        }
    }

    /// Get vCPU ID
    pub fn id(&self) -> u32 {
        self.vcpu_id
    }
}

impl Drop for KvmVcpu {
    fn drop(&mut self) {
        unsafe {
            // Unmap kvm_run
            libc::munmap(self.run.as_ptr() as *mut _, self.mmap_size);

            // Close vCPU fd
            libc::close(self.vcpu_fd);
        }
    }
}

// Safety: KvmVcpu can be safely sent between threads
unsafe impl Send for KvmVcpu {}
unsafe impl Sync for KvmVcpu {}
