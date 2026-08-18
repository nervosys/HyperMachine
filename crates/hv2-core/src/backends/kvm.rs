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
use crate::boot::multiboot::{MultibootLayout, MultibootProtocol};
use crate::boot::BootSetup;
use crate::descriptors::GdtBuilder;
use crate::hypervisor::{
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm,
};
use crate::{Error, IoDirection, Result, VCpu, VmExit};
use async_trait::async_trait;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::ptr::NonNull;
use std::sync::{Arc, RwLock};

// ── Boot-time architectural constants ───────────────────────────────────────
//
// The boot GDT built in `load_boot` is null / code / data, so the code segment
// is at byte offset 8 and the data segment at 16 — which are their selectors.

/// Selector of the flat 32-bit code segment in the boot GDT.
const CODE_SELECTOR: u16 = 0x08;
/// Selector of the flat 32-bit data segment in the boot GDT.
const DATA_SELECTOR: u16 = 0x10;
/// `CR0.PE` — protected mode enable.
const CR0_PE: u64 = 1 << 0;
/// `CR0.ET` — extension type; reads as 1 on every CPU since the 486.
const CR0_ET: u64 = 1 << 4;
/// The always-set reserved bit 1 of `RFLAGS`.
const RFLAGS_RESERVED: u64 = 0x2;

/// Put `sregs` into 32-bit protected mode with flat 4 GB segments and paging
/// off — the machine state both the Linux 32-bit boot protocol and Multiboot
/// require on entry.
///
/// KVM loads the hidden segment descriptors straight from `sregs`, so the guest
/// runs correctly from the first instruction without walking the GDT. The GDT
/// still has to exist and `sregs.gdt` still has to point at it, because a
/// kernel reloads its segments early and would fault on a null GDTR.
#[cfg(target_os = "linux")]
fn apply_flat_protected_mode(sregs: &mut kvm_sregs) {
    let code = kvm_segment {
        base: 0,
        limit: 0xFFFF_FFFF,
        selector: CODE_SELECTOR,
        type_: 0b1011, // execute/read, accessed
        present: 1,
        dpl: 0,
        db: 1, // 32-bit operand size
        s: 1,  // code/data descriptor
        l: 0,  // not 64-bit
        g: 1,  // limit in 4 KB pages
        avl: 0,
        unusable: 0,
        padding: 0,
    };
    let data = kvm_segment {
        selector: DATA_SELECTOR,
        type_: 0b0011, // read/write, accessed
        ..code
    };

    sregs.cs = code;
    sregs.ds = data;
    sregs.es = data;
    sregs.fs = data;
    sregs.gs = data;
    sregs.ss = data;
    sregs.cr0 = CR0_PE | CR0_ET;
    sregs.cr4 = 0;
    sregs.cr3 = 0;
    sregs.efer = 0;
}

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
    /// vCPU lookup: maps VCpu::id() → KvmVcpu
    vcpu_map: RwLock<HashMap<u32, Arc<KvmVcpu>>>,
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
        // SAFETY: All KVM ioctls below operate on file descriptors obtained from
        // `/dev/kvm`. Each call is checked for errors, and the fd is closed on
        // failure paths. The returned `KvmBackend` owns the fd exclusively.
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
                vcpu_map: RwLock::new(HashMap::new()),
            })
        }
    }

    /// Detect KVM capabilities
    fn detect_capabilities(kvm_fd: RawFd) -> Result<HypervisorCapabilities> {
        // SAFETY: `kvm_fd` is a valid KVM file descriptor. `kvm_check_extension`
        // performs a read-only ioctl that cannot corrupt state.
        unsafe {
            let check_cap = |cap: u32| -> bool {
                kvm_check_extension(kvm_fd, cap)
                    .map(|v| v > 0)
                    .unwrap_or(false)
            };

            let query_cap = |cap: u32| -> u32 {
                kvm_check_extension(kvm_fd, cap)
                    .map(|v| v as u32)
                    .unwrap_or(0)
            };

            /// Default maximum vCPUs when KVM_CAP_MAX_VCPUS is not supported.
            const DEFAULT_MAX_VCPUS: u32 = 288;

            let max_vcpus = {
                let v = query_cap(KVM_CAP_MAX_VCPUS);
                if v > 0 {
                    v
                } else {
                    DEFAULT_MAX_VCPUS
                }
            };

            Ok(HypervisorCapabilities {
                max_vcpus,
                max_memory: 4 * 1024 * 1024 * 1024 * 1024, // 4TB
                supports_nested_virt: check_cap(KVM_CAP_NESTED_STATE),
                supports_apic: check_cap(KVM_CAP_IRQCHIP),
                supports_x2apic: check_cap(KVM_CAP_X2APIC_API),
                supports_iommu: check_cap(KVM_CAP_IOMMU),
                supports_gpu_passthrough: false, // Requires VFIO setup
            })
        }
    }
}

impl Drop for KvmBackend {
    fn drop(&mut self) {
        // SAFETY: `self.kvm_fd` is a valid fd opened in `new()`. We own it
        // exclusively, so closing it here is safe.
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
        self.vms
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(kvm_vm.clone());

        // Create vCPUs and register them in the lookup map
        for i in 0..vcpu_count {
            let kvm_vcpu = kvm_vm.create_vcpu(i)?;
            self.vcpu_map
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(i, kvm_vcpu);
        }

        Ok(HypervisorVm::new(
            HypervisorPlatform::Kvm,
            vcpu_count,
            memory_size,
        ))
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };

        // Run the vCPU until it exits — this blocks until a VM exit occurs
        kvm_vcpu.run()
    }

    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };

        kvm_vcpu.inject_interrupt(vector)
    }

    async fn set_io_result(&self, vcpu: &VCpu, data: u32, size: u8) -> Result<()> {
        // KVM handles IO IN data through the kvm_run shared memory region.
        // After an IO IN exit, the hypervisor writes the result to the data
        // buffer at kvm_run.io.data_offset and calls KVM_RUN again.
        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };

        kvm_vcpu.set_io_data(data, size)
    }

    async fn load_boot(&self, vcpu: &VCpu, boot: &crate::boot::source::LoadedBoot) -> Result<()> {
        use crate::boot::source::LoadedBoot;

        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };
        let kvm_vm = self
            .vms
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .last()
            .cloned()
            .ok_or_else(|| {
                Error::Hypervisor("no KVM VM — create_vm must run before load_boot".into())
            })?;

        // Every protocol starts the same way: the images go into guest RAM.
        for (addr, data) in boot.memory_regions()? {
            kvm_vm.write_guest_memory(addr, &data)?;
        }

        match boot {
            LoadedBoot::Linux(params) => {
                // The 32-bit Linux boot protocol: enter at the protected-mode
                // kernel with paging off, flat 4 GB segments, and ESI pointing
                // at boot_params. KVM loads the hidden segment descriptors
                // straight from `sregs`, so the transition needs no GDT walk —
                // but Linux reloads the segments early, so a real GDT is still
                // written into guest memory and GDTR pointed at it.
                let (gdt_base, _idt_base, _pt_base, stack_pointer) =
                    BootSetup::allocate_standard_tables();

                let gdt = GdtBuilder::new()
                    .add_null()
                    .add_code_32bit(0, 0xFFFF_FFFF, 0)
                    .add_data_32bit(0, 0xFFFF_FFFF, 0)
                    .build();
                kvm_vm.write_guest_memory(gdt_base, &gdt)?;

                let mut sregs = kvm_vcpu.get_sregs()?;
                sregs.gdt.base = gdt_base;
                sregs.gdt.limit = (gdt.len() - 1) as u16;
                apply_flat_protected_mode(&mut sregs);
                kvm_vcpu.set_sregs(&sregs)?;

                let mut regs = kvm_vcpu.get_regs()?;
                regs.rip = params.kernel_addr;
                regs.rsi = params.setup_addr; // boot_params, per the protocol
                regs.rsp = stack_pointer;
                regs.rbp = stack_pointer;
                regs.rflags = RFLAGS_RESERVED;
                kvm_vcpu.set_regs(&regs)?;

                tracing::info!(
                    "KVM: Linux kernel loaded at {:#x}, boot_params at {:#x}",
                    params.kernel_addr,
                    params.setup_addr
                );
                Ok(())
            }

            LoadedBoot::Raw { entry, .. } => {
                // A raw image is entered in real mode, where the entry address
                // is a CS:IP pair. Point CS's hidden base at the paragraph and
                // start IP at the remainder, as the reset vector does.
                let segment = (*entry >> 4) as u16;
                let offset = (*entry & 0xF) as u16;

                let mut sregs = kvm_vcpu.get_sregs()?;
                sregs.cs.base = u64::from(segment) << 4;
                sregs.cs.selector = segment;
                sregs.cr0 &= !CR0_PE;
                kvm_vcpu.set_sregs(&sregs)?;

                let mut regs = kvm_vcpu.get_regs()?;
                regs.rip = u64::from(offset);
                regs.rflags = RFLAGS_RESERVED;
                kvm_vcpu.set_regs(&regs)?;

                tracing::info!("KVM: raw image entered at {:04x}:{:04x}", segment, offset);
                Ok(())
            }

            LoadedBoot::Multiboot(info) => {
                // Multiboot hands control to the kernel in 32-bit protected
                // mode with paging off — the same machine state as the Linux
                // entry above — but with EAX holding the bootloader magic and
                // EBX the multiboot_info address, which is how the kernel
                // recognises that it was Multiboot-loaded at all.
                let layout = MultibootLayout::default();
                let (gdt_base, _idt_base, _pt_base, stack_pointer) =
                    BootSetup::allocate_standard_tables();

                let gdt = GdtBuilder::new()
                    .add_null()
                    .add_code_32bit(0, 0xFFFF_FFFF, 0)
                    .add_data_32bit(0, 0xFFFF_FFFF, 0)
                    .build();
                kvm_vm.write_guest_memory(gdt_base, &gdt)?;

                let mut sregs = kvm_vcpu.get_sregs()?;
                sregs.gdt.base = gdt_base;
                sregs.gdt.limit = (gdt.len() - 1) as u16;
                apply_flat_protected_mode(&mut sregs);
                kvm_vcpu.set_sregs(&sregs)?;

                let mut regs = kvm_vcpu.get_regs()?;
                regs.rip = layout.kernel_addr;
                regs.rax = u64::from(MultibootProtocol::bootloader_magic());
                regs.rbx = layout.info_addr;
                regs.rsp = stack_pointer;
                regs.rbp = stack_pointer;
                // Multiboot requires interrupts disabled on entry, which is
                // RFLAGS with only the reserved bit set.
                regs.rflags = RFLAGS_RESERVED;
                kvm_vcpu.set_regs(&regs)?;

                tracing::info!(
                    "KVM: Multiboot kernel at {:#x}, info at {:#x}, {} module(s)",
                    layout.kernel_addr,
                    layout.info_addr,
                    info.modules.len()
                );
                Ok(())
            }
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down KVM backend");

        // Clear vCPU map
        self.vcpu_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        // VMs will be automatically closed when dropped
        self.vms.write().unwrap_or_else(|e| e.into_inner()).clear();

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
        // SAFETY: `kvm_fd` is a valid KVM fd obtained from `KvmBackend::new()`.
        // We create a VM via ioctl, allocate page-aligned memory via the global
        // allocator, and register it with KVM. All resources are cleaned up on
        // error paths (close fd, dealloc memory). The resulting `KvmVm` takes
        // exclusive ownership of the VM fd and guest memory pointer.
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

                match NonNull::new(ptr) {
                    Some(nn) => Some(nn),
                    None => {
                        libc::close(vm_fd);
                        return Err(Error::Hypervisor(
                            "Guest memory allocation returned null pointer".to_string(),
                        ));
                    }
                }
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
        self.vcpus
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(vcpu.clone());
        Ok(vcpu)
    }

    /// Get guest memory pointer
    pub fn guest_memory(&self) -> Option<NonNull<u8>> {
        self.guest_memory
    }

    /// Write `data` into guest physical memory at `addr`.
    ///
    /// The guest memory allocated in `KvmVm::new` is registered with KVM as
    /// slot 0 covering GPA `0..memory_size`, so a host-side write through the
    /// same allocation is visible to the guest immediately.
    ///
    /// # Errors
    ///
    /// Returns an error if guest memory was never allocated, or if the write
    /// would fall outside it.
    pub fn write_guest_memory(&self, addr: u64, data: &[u8]) -> Result<()> {
        let ptr = self
            .guest_memory
            .ok_or_else(|| Error::Memory("Guest memory not allocated".into()))?;

        let end = addr
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::Memory(format!("Write at {:#x} overflows a u64", addr)))?;
        if end > self.memory_size {
            return Err(Error::Memory(format!(
                "Write at {:#x} with length {} exceeds guest memory size {:#x}",
                addr,
                data.len(),
                self.memory_size
            )));
        }

        // SAFETY: The bounds check above guarantees `addr + data.len()` is
        // within the `memory_size` allocation `ptr` points at, and the source
        // and destination cannot overlap (one is host-private, one is the
        // guest allocation).
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                ptr.as_ptr().add(addr as usize),
                data.len(),
            );
        }

        tracing::debug!("Wrote {} bytes to guest memory at {:#x}", data.len(), addr);
        Ok(())
    }

    // ========================================================================
    // IRQ and interrupt management
    // ========================================================================

    /// Set the level of an IRQ line
    ///
    /// `irq` is the IRQ number, `level` is 0 (deassert) or 1 (assert).
    pub fn irq_line(&self, irq: u32, level: u32) -> Result<()> {
        let irq_level = kvm_irq_level { irq, level };
        // SAFETY: `self.vm_fd` is a valid VM fd.
        unsafe {
            kvm_irq_line(self.vm_fd, &irq_level).map_err(|e| {
                Error::Hypervisor(format!("Failed to set IRQ {} level {}: {}", irq, level, e))
            })
        }
    }

    /// Get in-kernel IRQ chip state (PIC or IOAPIC)
    ///
    /// `chip_id`: 0 = PIC master, 1 = PIC slave, 2 = IOAPIC
    pub fn get_irqchip(&self, chip_id: u32) -> Result<kvm_irqchip> {
        let mut chip = kvm_irqchip {
            chip_id,
            pad: 0,
            chip: [0u8; 512],
        };
        // SAFETY: `self.vm_fd` is a valid VM fd. `chip` is properly initialized.
        unsafe {
            kvm_get_irqchip(self.vm_fd, &mut chip).map_err(|e| {
                Error::Hypervisor(format!("Failed to get IRQ chip {}: {}", chip_id, e))
            })?;
        }
        Ok(chip)
    }

    /// Set in-kernel IRQ chip state (PIC or IOAPIC)
    pub fn set_irqchip(&self, chip: &kvm_irqchip) -> Result<()> {
        // SAFETY: `self.vm_fd` is a valid VM fd.
        unsafe {
            kvm_set_irqchip(self.vm_fd, chip).map_err(|e| {
                Error::Hypervisor(format!("Failed to set IRQ chip {}: {}", chip.chip_id, e))
            })
        }
    }

    /// Inject a Message Signaled Interrupt (MSI)
    pub fn signal_msi(&self, msi: &kvm_msi) -> Result<()> {
        // SAFETY: `self.vm_fd` is a valid VM fd. `msi` is properly initialized.
        unsafe {
            kvm_signal_msi(self.vm_fd, msi)
                .map_err(|e| Error::Hypervisor(format!("Failed to signal MSI: {}", e)))
        }
    }

    /// Set GSI (Global System Interrupt) routing table
    ///
    /// Configures how IRQs are routed to the in-kernel irqchip or MSI targets.
    pub fn set_gsi_routing(&self, routing: &kvm_irq_routing) -> Result<()> {
        // SAFETY: `self.vm_fd` is a valid VM fd. `routing` is properly initialized.
        unsafe {
            kvm_set_gsi_routing(self.vm_fd, routing)
                .map_err(|e| Error::Hypervisor(format!("Failed to set GSI routing: {}", e)))
        }
    }

    // ========================================================================
    // Memory management
    // ========================================================================

    /// Set identity map address for EPT/NPT real-mode emulation
    pub fn set_identity_map_addr(&self, addr: u64) -> Result<()> {
        // SAFETY: `self.vm_fd` is a valid VM fd.
        unsafe {
            kvm_set_identity_map_addr(self.vm_fd, addr).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set identity map address {:#x}: {}",
                    addr, e
                ))
            })
        }
    }

    /// Get dirty page log for a memory slot
    ///
    /// Used for live migration and dirty page tracking. The `dirty_bitmap`
    /// must be large enough to hold one bit per page in the memory slot.
    ///
    /// # Safety
    ///
    /// `dirty_bitmap` must point to a valid buffer of sufficient size.
    pub unsafe fn get_dirty_log(&self, slot: u32, dirty_bitmap: *mut u8) -> Result<()> {
        let mut log = kvm_dirty_log {
            slot,
            padding1: 0,
            dirty_bitmap,
        };
        // SAFETY: `self.vm_fd` is a valid VM fd. Caller guarantees
        // `dirty_bitmap` validity.
        unsafe {
            kvm_get_dirty_log(self.vm_fd, &mut log).map_err(|e| {
                Error::Hypervisor(format!("Failed to get dirty log for slot {}: {}", slot, e))
            })
        }
    }

    /// Map additional guest memory into the VM
    ///
    /// Creates a new memory slot mapping `memory_size` bytes of host memory
    /// at `userspace_addr` to guest physical address `guest_phys_addr`.
    pub fn map_memory(
        &self,
        slot: u32,
        guest_phys_addr: u64,
        memory_size: u64,
        userspace_addr: u64,
    ) -> Result<()> {
        let region = kvm_userspace_memory_region {
            slot,
            flags: 0,
            guest_phys_addr,
            memory_size,
            userspace_addr,
        };
        // SAFETY: `self.vm_fd` is a valid VM fd. The caller ensures that
        // `userspace_addr` points to a valid memory region of at least
        // `memory_size` bytes.
        unsafe {
            kvm_set_user_memory_region(self.vm_fd, &region).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to map memory slot {} at {:#x}: {}",
                    slot, guest_phys_addr, e
                ))
            })
        }
    }

    /// Get the VM file descriptor
    #[must_use]
    pub fn vm_fd(&self) -> RawFd {
        self.vm_fd
    }
}

impl Drop for KvmVm {
    fn drop(&mut self) {
        // SAFETY: Resources are released in reverse acquisition order: vCPUs
        // first, then guest memory, then the VM fd. Each was allocated in
        // `create_vm` and is owned exclusively by this `KvmVm`.
        unsafe {
            // vCPUs will be dropped first (RAII order)
            self.vcpus
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .clear();

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

// SAFETY: `KvmVm` holds file descriptors that are safe to transfer between
// threads. All mutable state is behind `RwLock` or `Mutex`.
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
        // SAFETY: `vm_fd` is a valid KVM VM fd. We create a vCPU via ioctl,
        // then mmap the `kvm_run` structure (shared with the kernel). The
        // mmap region is `MAP_SHARED` so the kernel can update exit info.
        // On failure, the vCPU fd is closed before returning. The resulting
        // `KvmVcpu` takes exclusive ownership of the vCPU fd and mmap pointer.
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

            let run = match NonNull::new(ptr as *mut kvm_run) {
                Some(nn) => nn,
                None => {
                    libc::close(vcpu_fd);
                    return Err(Error::Hypervisor(format!(
                        "mmap for vCPU {} returned null pointer",
                        vcpu_id
                    )));
                }
            };

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
    ///
    /// Automatically retries on EINTR (signal interruption), which is
    /// normal during KVM execution and not an actual error.
    pub fn run(&self) -> Result<VmExit> {
        // SAFETY: `self.vcpu_fd` is a valid vCPU fd created in `new()`. The
        // `kvm_run` mmap region is valid for the lifetime of this `KvmVcpu`.
        // EINTR is retried per KVM convention (signals during guest execution).
        unsafe {
            loop {
                match kvm_run(self.vcpu_fd) {
                    Ok(()) => return self.convert_exit(),
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                    Err(e) => {
                        return Err(Error::Hypervisor(format!(
                            "KVM_RUN failed for vCPU {}: {}",
                            self.vcpu_id, e
                        )));
                    }
                }
            }
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

            KVM_EXIT_NMI => Ok(VmExit::Nmi),

            KVM_EXIT_DEBUG => Ok(VmExit::Debug {
                info: format!("KVM debug exit on vCPU {}", self.vcpu_id),
            }),

            KVM_EXIT_IOAPIC_EOI => {
                let eoi = &run.exit_data.eoi;
                Ok(VmExit::IoapicEoi { vector: eoi.vector })
            }

            KVM_EXIT_X86_RDMSR => {
                let msr = &run.exit_data.msr;
                Ok(VmExit::Rdmsr { index: msr.index })
            }

            KVM_EXIT_X86_WRMSR => {
                let msr = &run.exit_data.msr;
                Ok(VmExit::Wrmsr {
                    index: msr.index,
                    data: msr.data,
                })
            }

            KVM_EXIT_SYSTEM_EVENT => {
                let se = &run.exit_data.system_event;
                Ok(VmExit::SystemEvent {
                    type_: se.type_,
                    flags: se.flags,
                })
            }

            KVM_EXIT_HYPERCALL => {
                let hc = &run.exit_data.hypercall;
                Ok(VmExit::Hypercall {
                    nr: hc.nr,
                    args: hc.args,
                })
            }

            _ => Ok(VmExit::Unknown {
                reason: exit_reason,
            }),
        }
    }

    /// Inject an interrupt
    pub fn inject_interrupt(&self, vector: u8) -> Result<()> {
        // SAFETY: `self.vcpu_fd` is a valid vCPU fd. The `kvm_interrupt`
        // struct is stack-allocated and properly initialized with the vector.
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

    /// Write IO IN data to the kvm_run data buffer
    ///
    /// After a `KVM_EXIT_IO` with direction=IN, the hypervisor must write the
    /// read data into the buffer at `kvm_run.io.data_offset` before calling
    /// `KVM_RUN` again. KVM will then load it into guest RAX automatically.
    pub fn set_io_data(&self, data: u32, size: u8) -> Result<()> {
        // SAFETY: `self.run` is a valid mmap'd `kvm_run` page obtained from
        // `KVM_RUN`. The `data_offset` field points within that same mmap'd
        // region at an IO data buffer whose size matches the IO exit width.
        unsafe {
            let run = self.run.as_ref();
            let data_ptr =
                (run as *const kvm_run as usize + run.exit_data.io.data_offset as usize) as *mut u8;

            match size {
                1 => std::ptr::write(data_ptr, data as u8),
                2 => std::ptr::write(data_ptr as *mut u16, data as u16),
                4 => std::ptr::write(data_ptr as *mut u32, data),
                _ => std::ptr::write(data_ptr as *mut u32, data),
            }
        }
        Ok(())
    }

    // ========================================================================
    // Register state accessors
    // ========================================================================

    /// Get general-purpose registers
    pub fn get_regs(&self) -> Result<kvm_regs> {
        let mut regs = kvm_regs::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd. regs is a valid output buffer.
        unsafe {
            kvm_get_regs(self.vcpu_fd, &mut regs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get regs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(regs)
    }

    /// Set general-purpose registers
    pub fn set_regs(&self, regs: &kvm_regs) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd. regs is a valid input.
        unsafe {
            kvm_set_regs(self.vcpu_fd, regs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set regs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get special registers (segment, control, descriptor table)
    pub fn get_sregs(&self) -> Result<kvm_sregs> {
        let mut sregs = kvm_sregs::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_sregs(self.vcpu_fd, &mut sregs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get sregs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(sregs)
    }

    /// Set special registers (segment, control, descriptor table)
    pub fn set_sregs(&self, sregs: &kvm_sregs) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_sregs(self.vcpu_fd, sregs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set sregs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get FPU state (x87, MMX, SSE registers)
    pub fn get_fpu(&self) -> Result<kvm_fpu> {
        let mut fpu = kvm_fpu::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_fpu(self.vcpu_fd, &mut fpu).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get FPU for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(fpu)
    }

    /// Set FPU state (x87, MMX, SSE registers)
    pub fn set_fpu(&self, fpu: &kvm_fpu) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_fpu(self.vcpu_fd, fpu).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set FPU for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get XSAVE state (extended processor state: AVX, AVX-512, etc.)
    pub fn get_xsave(&self) -> Result<kvm_xsave> {
        let mut xsave = kvm_xsave::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_xsave(self.vcpu_fd, &mut xsave).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get XSAVE for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(xsave)
    }

    /// Set XSAVE state (extended processor state: AVX, AVX-512, etc.)
    pub fn set_xsave(&self, xsave: &kvm_xsave) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_xsave(self.vcpu_fd, xsave).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set XSAVE for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get extended control registers (XCR0, etc.)
    pub fn get_xcrs(&self) -> Result<kvm_xcrs> {
        let mut xcrs = kvm_xcrs::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_xcrs(self.vcpu_fd, &mut xcrs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get XCRs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(xcrs)
    }

    /// Set extended control registers (XCR0, etc.)
    pub fn set_xcrs(&self, xcrs: &kvm_xcrs) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_xcrs(self.vcpu_fd, xcrs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set XCRs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get debug registers (DR0-7 and control flags)
    pub fn get_debugregs(&self) -> Result<kvm_debugregs> {
        let mut dbg = kvm_debugregs::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_debugregs(self.vcpu_fd, &mut dbg).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get debug regs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(dbg)
    }

    /// Set debug registers (DR0-7 and control flags)
    pub fn set_debugregs(&self, dbg: &kvm_debugregs) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_debugregs(self.vcpu_fd, dbg).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set debug regs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get LAPIC state
    pub fn get_lapic(&self) -> Result<kvm_lapic_state> {
        let mut lapic = kvm_lapic_state::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_lapic(self.vcpu_fd, &mut lapic).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get LAPIC for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(lapic)
    }

    /// Set LAPIC state
    pub fn set_lapic(&self, lapic: &kvm_lapic_state) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_lapic(self.vcpu_fd, lapic).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set LAPIC for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    // ========================================================================
    // MSR access
    // ========================================================================

    /// Read model-specific registers
    ///
    /// The msrs struct must have nmsrs set to the number of entries,
    /// and each entry's index field set to the MSR index to read.
    /// On return, each entry's data field contains the value read.
    pub fn get_msrs(&self, msrs: &mut kvm_msrs) -> Result<i32> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd. msrs is properly initialized.
        unsafe {
            kvm_get_msrs(self.vcpu_fd, msrs).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get MSRs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Write model-specific registers
    pub fn set_msrs(&self, msrs: &kvm_msrs) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd. msrs is properly initialized.
        unsafe {
            kvm_set_msrs(self.vcpu_fd, msrs).map(|_| ()).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set MSRs for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    // ========================================================================
    // Multiprocessor state
    // ========================================================================

    /// Get vCPU multiprocessor state (runnable, uninitialized, halted, etc.)
    pub fn get_mp_state(&self) -> Result<u32> {
        let mut state = kvm_mp_state { mp_state: 0 };
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_mp_state(self.vcpu_fd, &mut state).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get MP state for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(state.mp_state)
    }

    /// Set vCPU multiprocessor state
    pub fn set_mp_state(&self, mp_state: u32) -> Result<()> {
        let state = kvm_mp_state { mp_state };
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_mp_state(self.vcpu_fd, &state).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set MP state for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    // ========================================================================
    // vCPU events and debugging
    // ========================================================================

    /// Get vCPU events (pending exceptions, interrupts, NMIs, SMIs)
    pub fn get_vcpu_events(&self) -> Result<kvm_vcpu_events> {
        let mut events = kvm_vcpu_events::default();
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_get_vcpu_events(self.vcpu_fd, &mut events).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to get vCPU events for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })?;
        }
        Ok(events)
    }

    /// Set vCPU events (pending exceptions, interrupts, NMIs, SMIs)
    pub fn set_vcpu_events(&self, events: &kvm_vcpu_events) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_vcpu_events(self.vcpu_fd, events).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set vCPU events for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Enable guest debugging with the given control flags
    ///
    /// Use KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_SINGLESTEP for single-stepping,
    /// or KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_USE_HW_BP for hardware breakpoints.
    pub fn set_guest_debug(&self, control: u32) -> Result<()> {
        let debug = kvm_guest_debug {
            control,
            pad: 0,
            arch: kvm_guest_debug_arch::default(),
        };
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_set_guest_debug(self.vcpu_fd, &debug).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set guest debug for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Inject a non-maskable interrupt (NMI) into the vCPU
    pub fn inject_nmi(&self) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd.
        unsafe {
            kvm_nmi(self.vcpu_fd).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to inject NMI into vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Translate a guest virtual address to a guest physical address
    ///
    /// Returns the translation result including the physical address,
    /// validity flag, and page attributes.
    pub fn translate(&self, linear_address: u64) -> Result<kvm_translation> {
        let mut tr = kvm_translation {
            linear_address,
            physical_address: 0,
            valid: 0,
            writeable: 0,
            usermode: 0,
            pad: [0; 5],
        };
        // SAFETY: self.vcpu_fd is a valid vCPU fd. 	r is properly initialized.
        unsafe {
            kvm_translate(self.vcpu_fd, &mut tr).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to translate address {:#x} for vCPU {}: {}",
                    linear_address, self.vcpu_id, e
                ))
            })?;
        }
        Ok(tr)
    }

    /// Set the CPUID entries for this vCPU
    ///
    /// Must be called before the first KVM_RUN. The cpuid struct must
    /// be properly initialized with the desired CPUID leaves.
    pub fn set_cpuid(&self, cpuid: &kvm_cpuid2) -> Result<()> {
        // SAFETY: self.vcpu_fd is a valid vCPU fd. cpuid is properly initialized.
        unsafe {
            kvm_set_cpuid2(self.vcpu_fd, cpuid).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set CPUID for vCPU {}: {}",
                    self.vcpu_id, e
                ))
            })
        }
    }

    /// Get vCPU ID
    pub fn id(&self) -> u32 {
        self.vcpu_id
    }
}

impl KvmBackend {
    /// Get supported CPUID entries from KVM
    ///
    /// Queries the host KVM for the complete set of supported CPUID leaves.
    /// Returns the entries as a Vec<kvm_cpuid_entry2>.
    pub fn get_supported_cpuid(&self) -> Result<Vec<kvm_cpuid_entry2>> {
        const MAX_ENTRIES: usize = 256;
        let header_size = std::mem::size_of::<kvm_cpuid2>();
        let entry_size = std::mem::size_of::<kvm_cpuid_entry2>();
        let total_size = header_size + entry_size * MAX_ENTRIES;

        let mut buf = vec![0u8; total_size];

        // Write nent into the header
        // SAFETY: buf is large enough for the header, and kvm_cpuid2 has nent at offset 0.
        unsafe {
            let header = &mut *(buf.as_mut_ptr() as *mut kvm_cpuid2);
            header.nent = MAX_ENTRIES as u32;

            kvm_get_supported_cpuid(self.kvm_fd, header)
                .map_err(|e| Error::Hypervisor(format!("Failed to get supported CPUID: {}", e)))?;

            let nent = header.nent as usize;
            let entries_ptr = buf.as_ptr().add(header_size) as *const kvm_cpuid_entry2;
            let entries = std::slice::from_raw_parts(entries_ptr, nent);
            Ok(entries.to_vec())
        }
    }
}

impl Drop for KvmVcpu {
    fn drop(&mut self) {
        // SAFETY: `self.run` was mmap'd in `create_vcpu` with `self.mmap_size`
        // bytes. `self.vcpu_fd` was opened by KVM. Both are exclusively owned.
        unsafe {
            // Unmap kvm_run
            libc::munmap(self.run.as_ptr() as *mut _, self.mmap_size);

            // Close vCPU fd
            libc::close(self.vcpu_fd);
        }
    }
}

// SAFETY: `KvmVcpu` holds file descriptors and a mapped pointer that are
// safe to transfer between threads. No thread-local or non-Send state.
unsafe impl Send for KvmVcpu {}
unsafe impl Sync for KvmVcpu {}
