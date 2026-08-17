//! Apple Hypervisor Framework (HVF) backend
//!
//! This module provides hardware-accelerated virtualization on macOS
//! using the Hypervisor.framework API. Supports both Intel (VMX) and
//! Apple Silicon architectures.
//!
//! # Architecture
//!
//! - [HvfBackend] implements [HypervisorBackend] for macOS.
//! - FFI bindings live in the sibling [super::hvf_ffi] module.
//! - One VM per process (Hypervisor.framework limitation).
//! - vCPUs are created lazily during [HypervisorBackend::create_vm].
//!
//! # Thread Safety
//!
//! HvfBackend is Send + Sync. All mutable state is protected by
//! RwLock or atomic operations.

use crate::boot::multiboot::{MultibootLayout, MultibootProtocol};
use crate::boot::BootSetup;
use crate::descriptors::GdtBuilder;
use crate::hypervisor::{
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm,
};
use crate::{Error, IoDirection, Result, VCpu, VmExit};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::hvf_ffi::*;

// -- Internal types --

/// Per-vCPU state tracked by the HVF backend.
struct VCpuState {
    hv_vcpu: HvVcpuId,
    interrupt_pending: AtomicBool,
    pending_vector: AtomicU64,
}

/// A guest physical memory mapping.
#[derive(Debug)]
struct MemoryMapping {
    guest_addr: u64,
    size: u64,
    host_ptr: *mut u8,
}

/// The host allocation backing guest RAM, owned by the backend.
#[derive(Debug)]
struct GuestAllocation {
    ptr: *mut u8,
    size: u64,
}

// SAFETY: `ptr` is a plain owned allocation. The backend serialises access to
// it through the `RwLock` holding this struct, and Hypervisor.framework accepts
// the mapping from any thread in the process.
unsafe impl Send for GuestAllocation {}
unsafe impl Sync for GuestAllocation {}

/// `RFLAGS` with only the always-set reserved bit 1, i.e. interrupts disabled.
const RFLAGS_RESERVED: u64 = 0x2;

/// `CR0.PE` — protected mode enable.
const CR0_PE: u64 = 1 << 0;

/// `CR0.ET` — extension type; reads as 1 on every CPU since the 486.
const CR0_ET: u64 = 1 << 4;

/// Segment access rights for a flat 32-bit code segment: type 0xB
/// (execute/read, accessed), S=1, P=1, D/B=1, G=1.
const AR_FLAT_CODE32: u64 = 0xC09B;

/// Segment access rights for a flat 32-bit data segment: type 0x3
/// (read/write, accessed), S=1, P=1, D/B=1, G=1.
const AR_FLAT_DATA32: u64 = 0xC093;

/// Segment access rights for a real-mode code segment: type 0xB, S=1, P=1,
/// with 16-bit default size and byte granularity.
const AR_REAL_MODE_CODE: u64 = 0x9B;

/// Access rights marking a segment unusable (bit 16).
const AR_UNUSABLE: u64 = 1 << 16;

/// Write an architectural register, mapping the FFI status to an `Error`.
fn write_reg(hv_vcpu: HvVcpuId, reg: HvX86Reg, value: u64) -> Result<()> {
    // SAFETY: FFI call with a vCPU handle obtained from `hv_vcpu_create` and a
    // register id from the `HvX86Reg` enum, both valid by construction.
    let result = unsafe { hv_vcpu_write_register(hv_vcpu, reg as u32, value) };
    if result != HV_SUCCESS {
        return Err(Error::Hypervisor(format!(
            "Failed to write {:?}: {}",
            reg, result
        )));
    }
    Ok(())
}

/// Read an architectural register, mapping the FFI status to an `Error`.
fn read_reg(hv_vcpu: HvVcpuId, reg: HvX86Reg) -> Result<u64> {
    let mut value = 0u64;
    // SAFETY: FFI call with a valid vCPU handle, a register id from the
    // `HvX86Reg` enum, and a valid pointer to a live local.
    let result = unsafe { hv_vcpu_read_register(hv_vcpu, reg as u32, &mut value) };
    if result != HV_SUCCESS {
        return Err(Error::Hypervisor(format!(
            "Failed to read {:?}: {}",
            reg, result
        )));
    }
    Ok(value)
}

/// Write a VMCS field, mapping the FFI status to an `Error`.
fn write_vmcs(hv_vcpu: HvVcpuId, field: VmcsField, value: u64) -> Result<()> {
    // SAFETY: FFI call with a valid vCPU handle and a field encoding from the
    // `VmcsField` enum.
    let result = unsafe { hv_vmx_vcpu_write_vmcs(hv_vcpu, field as u32, value) };
    if result != HV_SUCCESS {
        return Err(Error::Hypervisor(format!(
            "Failed to write VMCS field {:?}: {}",
            field, result
        )));
    }
    Ok(())
}

// -- HvfBackend --

/// Apple Hypervisor Framework backend.
///
/// Wraps the macOS Hypervisor.framework, providing VM creation, vCPU
/// management, memory mapping, interrupt injection, and VMX exit handling.
///
/// # VMX Basic Exit Reasons (Intel SDM Vol. 3, Appendix C)
///
/// These constants define the exit reason numbers read from VMCS field 0x4402.
const VMX_EXIT_REASON_EXCEPTION_NMI: u64 = 0;
const VMX_EXIT_REASON_EXTERNAL_INTERRUPT: u64 = 1;
const VMX_EXIT_REASON_TRIPLE_FAULT: u64 = 2;
const VMX_EXIT_REASON_CPUID: u64 = 10;
const VMX_EXIT_REASON_HLT: u64 = 12;
const VMX_EXIT_REASON_VMCALL: u64 = 18;
const VMX_EXIT_REASON_IO_INSTRUCTION: u64 = 30;
const VMX_EXIT_REASON_RDMSR: u64 = 31;
const VMX_EXIT_REASON_WRMSR: u64 = 32;
const VMX_EXIT_REASON_EPT_VIOLATION: u64 = 48;
const VMX_EXIT_REASON_EPT_MISCONFIG: u64 = 49;
pub struct HvfBackend {
    capabilities: HypervisorCapabilities,
    vm_created: AtomicBool,
    vcpu_states: RwLock<HashMap<u32, VCpuState>>,
    memory_mappings: RwLock<Vec<MemoryMapping>>,
    /// Host allocation backing guest RAM, created by `create_vm`.
    guest_memory: RwLock<Option<GuestAllocation>>,
    initialized: AtomicBool,
}

impl HvfBackend {
    /// Create a new HVF backend.
    ///
    /// Probes Hypervisor.framework availability by creating (then immediately
    /// destroying) a temporary VM.
    ///
    /// # Errors
    ///
    /// Returns an error if Hypervisor.framework is not available on the host.
    pub fn new() -> Result<Self> {
        // Probe availability by creating a throwaway VM.
        // SAFETY: FFI call to hv_vm_create to probe HVF availability. flags=0 for default config.
        // If successful, the VM is destroyed immediately below.
        let result = unsafe { hv_vm_create(0) };
        if result == HV_SUCCESS {
            // SAFETY: FFI call to hv_vm_destroy; the VM was just successfully created above.
            unsafe { hv_vm_destroy() };
            tracing::info!("HVF hypervisor detected and available");
        } else {
            return Err(Error::Hypervisor(format!(
                "Apple Hypervisor Framework is not available: {}",
                result
            )));
        }

        // Determine capabilities based on architecture.
        #[cfg(target_arch = "aarch64")]
        let caps = HypervisorCapabilities {
            max_vcpus: 128,
            max_memory: 1024 * 1024 * 1024 * 1024, // 1 TB
            supports_nested_virt: false,
            supports_apic: false, // No APIC on ARM
            supports_x2apic: false,
            supports_iommu: false,
            supports_gpu_passthrough: false,
        };

        #[cfg(target_arch = "x86_64")]
        let caps = HypervisorCapabilities {
            max_vcpus: 64,
            max_memory: 512 * 1024 * 1024 * 1024, // 512 GB
            supports_nested_virt: false,
            supports_apic: true,
            supports_x2apic: true,
            supports_iommu: false,
            supports_gpu_passthrough: false,
        };

        Ok(Self {
            capabilities: caps,
            vm_created: AtomicBool::new(false),
            vcpu_states: RwLock::new(HashMap::new()),
            memory_mappings: RwLock::new(Vec::new()),
            guest_memory: RwLock::new(None),
            initialized: AtomicBool::new(false),
        })
    }

    /// Write `data` into guest physical memory at `addr`.
    ///
    /// The allocation mapped at GPA 0 by `create_vm` backs the whole guest
    /// physical address space, so a host-side write through it is immediately
    /// visible to the guest.
    ///
    /// # Errors
    ///
    /// Returns an error if guest memory was never allocated, or if the write
    /// would fall outside it.
    pub fn write_guest_memory(&self, addr: u64, data: &[u8]) -> Result<()> {
        let guard = self.guest_memory.read();
        let allocation = guard
            .as_ref()
            .ok_or_else(|| Error::Memory("Guest memory not allocated".into()))?;

        let end = addr
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::Memory(format!("Write at {:#x} overflows a u64", addr)))?;
        if end > allocation.size {
            return Err(Error::Memory(format!(
                "Write at {:#x} with length {} exceeds guest memory size {:#x}",
                addr,
                data.len(),
                allocation.size
            )));
        }

        // SAFETY: The bounds check above guarantees the destination lies within
        // the `allocation.size` region `ptr` owns, and the host-private source
        // cannot overlap the guest allocation.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                allocation.ptr.add(addr as usize),
                data.len(),
            );
        }

        Ok(())
    }

    /// Put a vCPU into 32-bit protected mode with flat 4 GB segments and
    /// paging off — the entry state both the Linux 32-bit boot protocol and
    /// Multiboot require.
    ///
    /// Each segment's selector, base, limit, and access rights are written
    /// together, because VM entry loads the hidden descriptor cache from the
    /// VMCS rather than walking the GDT. LDTR is marked unusable and TR is
    /// given a minimal busy-TSS descriptor, both of which VM entry checks.
    fn enter_flat_protected_mode(
        &self,
        hv_vcpu: HvVcpuId,
        gdt_base: u64,
        gdt_len: u64,
    ) -> Result<()> {
        const CODE_SELECTOR: u64 = 0x08;
        const DATA_SELECTOR: u64 = 0x10;

        write_vmcs(hv_vcpu, VmcsField::GuestCsSelector, CODE_SELECTOR)?;
        write_vmcs(hv_vcpu, VmcsField::GuestCsBase, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestCsLimit, 0xFFFF_FFFF)?;
        write_vmcs(hv_vcpu, VmcsField::GuestCsAccessRights, AR_FLAT_CODE32)?;

        for (selector, base, limit, ar) in [
            (
                VmcsField::GuestDsSelector,
                VmcsField::GuestDsBase,
                VmcsField::GuestDsLimit,
                VmcsField::GuestDsAccessRights,
            ),
            (
                VmcsField::GuestEsSelector,
                VmcsField::GuestEsBase,
                VmcsField::GuestEsLimit,
                VmcsField::GuestEsAccessRights,
            ),
            (
                VmcsField::GuestFsSelector,
                VmcsField::GuestFsBase,
                VmcsField::GuestFsLimit,
                VmcsField::GuestFsAccessRights,
            ),
            (
                VmcsField::GuestGsSelector,
                VmcsField::GuestGsBase,
                VmcsField::GuestGsLimit,
                VmcsField::GuestGsAccessRights,
            ),
            (
                VmcsField::GuestSsSelector,
                VmcsField::GuestSsBase,
                VmcsField::GuestSsLimit,
                VmcsField::GuestSsAccessRights,
            ),
        ] {
            write_vmcs(hv_vcpu, selector, DATA_SELECTOR)?;
            write_vmcs(hv_vcpu, base, 0)?;
            write_vmcs(hv_vcpu, limit, 0xFFFF_FFFF)?;
            write_vmcs(hv_vcpu, ar, AR_FLAT_DATA32)?;
        }

        // VM entry rejects a usable LDTR that does not describe a real LDT, so
        // say plainly that there isn't one.
        write_vmcs(hv_vcpu, VmcsField::GuestLdtrSelector, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestLdtrBase, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestLdtrLimit, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestLdtrAccessRights, AR_UNUSABLE)?;

        // TR, by contrast, must be usable: type 11 (busy 32-bit TSS), present.
        write_vmcs(hv_vcpu, VmcsField::GuestTrSelector, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestTrBase, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestTrLimit, 0xFFFF)?;
        write_vmcs(hv_vcpu, VmcsField::GuestTrAccessRights, 0x8B)?;

        write_vmcs(hv_vcpu, VmcsField::GuestGdtrBase, gdt_base)?;
        write_vmcs(
            hv_vcpu,
            VmcsField::GuestGdtrLimit,
            gdt_len.saturating_sub(1),
        )?;
        write_vmcs(hv_vcpu, VmcsField::GuestIdtrBase, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestIdtrLimit, 0xFFFF)?;

        write_reg(hv_vcpu, HvX86Reg::Cr0, CR0_PE | CR0_ET)?;
        write_reg(hv_vcpu, HvX86Reg::Cr3, 0)?;
        write_reg(hv_vcpu, HvX86Reg::Cr4, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestCr0, CR0_PE | CR0_ET)?;
        write_vmcs(hv_vcpu, VmcsField::GuestCr3, 0)?;
        write_vmcs(hv_vcpu, VmcsField::GuestCr4, 0)?;

        Ok(())
    }

    // -- Internal helpers --

    /// Create the HVF VM (idempotent).
    fn create_vm_internal(&self) -> Result<()> {
        if self.vm_created.swap(true, Ordering::SeqCst) {
            return Ok(()); // Already created
        }

        // SAFETY: FFI call to hv_vm_create with flags=0. The vm_created flag ensures this
        // is only called once.
        let result = unsafe { hv_vm_create(0) };
        if result != HV_SUCCESS {
            self.vm_created.store(false, Ordering::SeqCst);
            return Err(Error::Hypervisor(format!(
                "Failed to create HVF VM: {}",
                result
            )));
        }

        tracing::info!("HVF VM created");
        Ok(())
    }

    /// Create a vCPU and register it in vcpu_states.
    fn create_vcpu(&self, vcpu_id: u32) -> Result<HvVcpuId> {
        let mut hv_vcpu: HvVcpuId = 0;
        // SAFETY: FFI call to hv_vcpu_create with valid mutable pointer to receive the vCPU handle.
        // The VM has been created and flags=0 for default configuration.
        let result = unsafe { hv_vcpu_create(&mut hv_vcpu, 0) };

        if result != HV_SUCCESS {
            return Err(Error::Hypervisor(format!(
                "Failed to create HVF vCPU {}: {}",
                vcpu_id, result
            )));
        }

        self.vcpu_states.write().insert(
            vcpu_id,
            VCpuState {
                hv_vcpu,
                interrupt_pending: AtomicBool::new(false),
                pending_vector: AtomicU64::new(0),
            },
        );

        tracing::debug!("Created HVF vCPU {} (handle={})", vcpu_id, hv_vcpu);
        Ok(hv_vcpu)
    }

    /// Map guest physical memory.
    fn map_memory(&self, guest_addr: u64, size: u64, host_ptr: *mut u8) -> Result<()> {
        let flags = HV_MEMORY_READ | HV_MEMORY_WRITE | HV_MEMORY_EXEC;

        // SAFETY: FFI call to hv_vm_map with valid host_ptr. The caller guarantees host_ptr
        // points to a valid memory region of at least size bytes. VM has been created.
        let result = unsafe { hv_vm_map(host_ptr as *mut c_void, guest_addr, size, flags) };

        if result != HV_SUCCESS {
            return Err(Error::Hypervisor(format!(
                "Failed to map HVF memory at 0x{:x}: {}",
                guest_addr, result
            )));
        }

        self.memory_mappings.write().push(MemoryMapping {
            guest_addr,
            size,
            host_ptr,
        });

        tracing::debug!("Mapped {} bytes at GPA 0x{:x}", size, guest_addr);
        Ok(())
    }

    /// Inject a pending interrupt into the given vCPU via VMCS.
    fn inject_pending_interrupt(&self, vcpu_id: u32) -> Result<()> {
        let states = self.vcpu_states.read();
        if let Some(state) = states.get(&vcpu_id) {
            if state.interrupt_pending.swap(false, Ordering::SeqCst) {
                let vector = state.pending_vector.load(Ordering::SeqCst) as u32;

                // VM-entry interruption-information: Valid(31) | Vector(7:0)
                let interrupt_info: u64 = 0x8000_0000 | (vector as u64);

                // SAFETY: FFI call to hv_vmx_vcpu_write_vmcs with valid vCPU handle obtained
                // from hv_vcpu_create. Writing VM-entry interruption info to inject an interrupt.
                let result = unsafe {
                    hv_vmx_vcpu_write_vmcs(
                        state.hv_vcpu,
                        VmcsField::VmEntryInterruptionInfo as u32,
                        interrupt_info,
                    )
                };

                if result != HV_SUCCESS {
                    return Err(Error::Hypervisor(format!(
                        "Failed to inject interrupt: {}",
                        result
                    )));
                }

                tracing::debug!("Injected interrupt {} into vCPU {}", vector, vcpu_id);
            }
        }
        Ok(())
    }

    /// Read VM-exit reason from the VMCS.
    fn read_exit_reason(&self, hv_vcpu: HvVcpuId) -> Result<u64> {
        let mut reason: u64 = 0;
        // SAFETY: FFI call to hv_vmx_vcpu_read_vmcs with valid vCPU handle and valid mutable
        // pointer to reason. Reading VmExitReason field from VMCS after vCPU has exited.
        let result =
            unsafe { hv_vmx_vcpu_read_vmcs(hv_vcpu, VmcsField::VmExitReason as u32, &mut reason) };
        if result != HV_SUCCESS {
            return Err(Error::Hypervisor(format!(
                "Failed to read exit reason: {}",
                result
            )));
        }
        Ok(reason)
    }

    /// Read VM-exit qualification from the VMCS.
    fn read_exit_qualification(&self, hv_vcpu: HvVcpuId) -> Result<u64> {
        let mut qual: u64 = 0;
        // SAFETY: FFI call to hv_vmx_vcpu_read_vmcs with valid vCPU handle and valid mutable
        // pointer to qual. Reading VmExitQualification from VMCS after vCPU has exited.
        let result = unsafe {
            hv_vmx_vcpu_read_vmcs(hv_vcpu, VmcsField::VmExitQualification as u32, &mut qual)
        };
        if result != HV_SUCCESS {
            return Err(Error::Hypervisor(format!(
                "Failed to read exit qualification: {}",
                result
            )));
        }
        Ok(qual)
    }
}

impl Default for HvfBackend {
    fn default() -> Self {
        Self::new().expect("Failed to create HVF backend")
    }
}

// -- HypervisorBackend impl --

#[async_trait]
impl HypervisorBackend for HvfBackend {
    fn platform(&self) -> HypervisorPlatform {
        HypervisorPlatform::Hvf
    }

    fn capabilities(&self) -> HypervisorCapabilities {
        self.capabilities.clone()
    }

    async fn init(&mut self) -> Result<()> {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.create_vm_internal()?;
        tracing::info!("Initialized HVF backend");
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

        // Allocate and map guest RAM at GPA 0. Without this the VM has no
        // memory at all, so there is nowhere to put a guest.
        if memory_size > 0 && self.guest_memory.read().is_none() {
            let layout = std::alloc::Layout::from_size_align(memory_size as usize, 4096)
                .map_err(|e| Error::Memory(format!("Invalid memory layout: {}", e)))?;

            // SAFETY: `layout` has a non-zero size (checked above) and a valid
            // 4 KB alignment. The allocation is owned by this backend and freed
            // in `Drop`.
            let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
            if ptr.is_null() {
                return Err(Error::Memory("Failed to allocate guest memory".into()));
            }

            if let Err(e) = self.map_memory(0, memory_size, ptr) {
                // SAFETY: `ptr` came from `alloc_zeroed` with this same layout
                // and has not been freed or handed out.
                unsafe { std::alloc::dealloc(ptr, layout) };
                return Err(e);
            }

            *self.guest_memory.write() = Some(GuestAllocation {
                ptr,
                size: memory_size,
            });
        }

        // Create vCPUs
        for i in 0..vcpu_count {
            self.create_vcpu(i)?;
        }

        Ok(HypervisorVm::new(
            HypervisorPlatform::Hvf,
            vcpu_count,
            memory_size,
        ))
    }

    async fn load_boot(&self, vcpu: &VCpu, boot: &crate::boot::source::LoadedBoot) -> Result<()> {
        use crate::boot::source::LoadedBoot;

        let hv_vcpu = {
            let states = self.vcpu_states.read();
            states
                .get(&vcpu.id())
                .ok_or_else(|| Error::Hypervisor(format!("HVF vCPU {} not found", vcpu.id())))?
                .hv_vcpu
        };

        // Every protocol starts the same way: the images go into guest RAM.
        for (addr, data) in boot.memory_regions()? {
            self.write_guest_memory(addr, &data)?;
        }

        match boot {
            LoadedBoot::Linux(params) => {
                let (gdt_base, _idt_base, _pt_base, stack_pointer) =
                    BootSetup::allocate_standard_tables();
                let gdt = GdtBuilder::new()
                    .add_null()
                    .add_code_32bit(0, 0xFFFF_FFFF, 0)
                    .add_data_32bit(0, 0xFFFF_FFFF, 0)
                    .build();
                self.write_guest_memory(gdt_base, &gdt)?;

                self.enter_flat_protected_mode(hv_vcpu, gdt_base, gdt.len() as u64)?;

                write_reg(hv_vcpu, HvX86Reg::Rip, params.kernel_addr)?;
                // The 32-bit Linux boot protocol passes boot_params in ESI.
                write_reg(hv_vcpu, HvX86Reg::Rsi, params.setup_addr)?;
                write_reg(hv_vcpu, HvX86Reg::Rsp, stack_pointer)?;
                write_reg(hv_vcpu, HvX86Reg::Rbp, stack_pointer)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRip, params.kernel_addr)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRsp, stack_pointer)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRflags, RFLAGS_RESERVED)?;

                tracing::info!(
                    "HVF: Linux kernel at {:#x}, boot_params at {:#x}",
                    params.kernel_addr,
                    params.setup_addr
                );
                Ok(())
            }

            LoadedBoot::Multiboot(info) => {
                let layout = MultibootLayout::default();
                let (gdt_base, _idt_base, _pt_base, stack_pointer) =
                    BootSetup::allocate_standard_tables();
                let gdt = GdtBuilder::new()
                    .add_null()
                    .add_code_32bit(0, 0xFFFF_FFFF, 0)
                    .add_data_32bit(0, 0xFFFF_FFFF, 0)
                    .build();
                self.write_guest_memory(gdt_base, &gdt)?;

                self.enter_flat_protected_mode(hv_vcpu, gdt_base, gdt.len() as u64)?;

                write_reg(hv_vcpu, HvX86Reg::Rip, layout.kernel_addr)?;
                write_reg(
                    hv_vcpu,
                    HvX86Reg::Rax,
                    u64::from(MultibootProtocol::bootloader_magic()),
                )?;
                write_reg(hv_vcpu, HvX86Reg::Rbx, layout.info_addr)?;
                write_reg(hv_vcpu, HvX86Reg::Rsp, stack_pointer)?;
                write_reg(hv_vcpu, HvX86Reg::Rbp, stack_pointer)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRip, layout.kernel_addr)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRsp, stack_pointer)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRflags, RFLAGS_RESERVED)?;

                tracing::info!(
                    "HVF: Multiboot kernel at {:#x}, info at {:#x}, {} module(s)",
                    layout.kernel_addr,
                    layout.info_addr,
                    info.modules.len()
                );
                Ok(())
            }

            LoadedBoot::Raw { entry, .. } => {
                // A raw image is entered in real mode, where the entry address
                // is a CS:IP pair rather than a linear address.
                let segment = (*entry >> 4) as u16;
                let offset = *entry & 0xF;

                write_vmcs(hv_vcpu, VmcsField::GuestCsSelector, u64::from(segment))?;
                write_vmcs(hv_vcpu, VmcsField::GuestCsBase, u64::from(segment) << 4)?;
                write_vmcs(hv_vcpu, VmcsField::GuestCsLimit, 0xFFFF)?;
                write_vmcs(hv_vcpu, VmcsField::GuestCsAccessRights, AR_REAL_MODE_CODE)?;

                let cr0 = read_reg(hv_vcpu, HvX86Reg::Cr0)?;
                write_reg(hv_vcpu, HvX86Reg::Cr0, cr0 & !CR0_PE)?;
                write_reg(hv_vcpu, HvX86Reg::Rip, offset)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRip, offset)?;
                write_vmcs(hv_vcpu, VmcsField::GuestRflags, RFLAGS_RESERVED)?;

                tracing::info!("HVF: raw image entered at {:04x}:{:04x}", segment, offset);
                Ok(())
            }
        }
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        let states = self.vcpu_states.read();
        let state = states
            .get(&vcpu.id())
            .ok_or(Error::Hypervisor(format!("vCPU {} not found", vcpu.id())))?;
        let hv_vcpu = state.hv_vcpu;
        drop(states); // Release lock before running

        // Inject any pending interrupt
        self.inject_pending_interrupt(vcpu.id())?;

        // Run the vCPU
        // SAFETY: FFI call to hv_vcpu_run with valid vCPU handle obtained from hv_vcpu_create.
        // The vCPU has been properly initialized and any pending interrupts have been injected.
        let result = unsafe { hv_vcpu_run(hv_vcpu) };

        if result != HV_SUCCESS {
            return Err(Error::Hypervisor(format!(
                "Failed to run HVF vCPU: {}",
                result
            )));
        }

        // Read exit reason
        let exit_reason = self.read_exit_reason(hv_vcpu)?;

        // VMX basic exit reasons (Intel SDM Vol. 3, Appendix C)
        match exit_reason & 0xFFFF {
            VMX_EXIT_REASON_EXCEPTION_NMI => {
                let qual = self.read_exit_qualification(hv_vcpu)?;
                let vector = (qual & 0xFF) as u8;
                let has_error_code = (qual & 0x800) != 0;
                let error_code = if has_error_code {
                    Some(((qual >> 32) & 0xFFFF_FFFF) as u32)
                } else {
                    None
                };
                Ok(VmExit::Exception { vector, error_code })
            }
            VMX_EXIT_REASON_EXTERNAL_INTERRUPT => Ok(VmExit::InterruptWindow),
            VMX_EXIT_REASON_TRIPLE_FAULT => Ok(VmExit::Shutdown),
            VMX_EXIT_REASON_CPUID => Ok(VmExit::Unknown { reason: 10 }),
            VMX_EXIT_REASON_HLT => Ok(VmExit::Hlt),
            VMX_EXIT_REASON_VMCALL => {
                // Read hypercall number from RAX and first 6 args from RBX, RCX, RDX, RSI, RDI, RBP
                let mut rax: u64 = 0;
                // SAFETY: FFI call to hv_vcpu_read_register with valid vCPU handle and valid
                // mutable pointer. Reading RAX register for hypercall number.
                unsafe {
                    hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rax as u32, &mut rax);
                }
                Ok(VmExit::Hypercall {
                    nr: rax,
                    args: [0; 6],
                })
            }
            VMX_EXIT_REASON_IO_INSTRUCTION => {
                let qual = self.read_exit_qualification(hv_vcpu)?;
                let size = ((qual & 0x7) + 1) as u8;
                let is_out = (qual & 0x8) == 0;
                let port = ((qual >> 16) & 0xFFFF) as u16;

                // Read RAX for data
                let mut rax: u64 = 0;
                // SAFETY: FFI call to hv_vcpu_read_register with valid vCPU handle and valid
                // mutable pointer to rax. Reading RAX register value after I/O port exit.
                unsafe {
                    hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rax as u32, &mut rax);
                }

                Ok(VmExit::Io {
                    port,
                    direction: if is_out {
                        IoDirection::Out
                    } else {
                        IoDirection::In
                    },
                    size,
                    data: rax as u32,
                })
            }
            VMX_EXIT_REASON_RDMSR => {
                // MSR index is in ECX
                let mut rcx: u64 = 0;
                // SAFETY: FFI call to hv_vcpu_read_register with valid vCPU handle and valid
                // mutable pointer. Reading RCX for MSR index.
                unsafe {
                    hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rcx as u32, &mut rcx);
                }
                Ok(VmExit::Rdmsr { index: rcx as u32 })
            }
            VMX_EXIT_REASON_WRMSR => {
                // MSR index in ECX, value in EDX:EAX
                let mut rcx: u64 = 0;
                let mut rax: u64 = 0;
                let mut rdx: u64 = 0;
                // SAFETY: FFI calls to hv_vcpu_read_register with valid vCPU handle and valid
                // mutable pointers. Reading RCX (MSR index), RAX and RDX (MSR value).
                unsafe {
                    hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rcx as u32, &mut rcx);
                    hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rax as u32, &mut rax);
                    hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rdx as u32, &mut rdx);
                }
                Ok(VmExit::Wrmsr {
                    index: rcx as u32,
                    data: ((rdx & 0xFFFF_FFFF) << 32) | (rax & 0xFFFF_FFFF),
                })
            }
            VMX_EXIT_REASON_EPT_VIOLATION | VMX_EXIT_REASON_EPT_MISCONFIG => {
                let qual = self.read_exit_qualification(hv_vcpu)?;
                let is_write = (qual & 0x2) != 0;

                // Read guest physical address from VMCS
                let mut gpa: u64 = 0;
                // SAFETY: FFI call to hv_vmx_vcpu_read_vmcs with valid vCPU handle and valid
                // mutable pointer to gpa. Reading GUEST_PHYSICAL_ADDRESS from VMCS after EPT violation.
                unsafe {
                    hv_vmx_vcpu_read_vmcs(hv_vcpu, VMCS_GUEST_PHYSICAL_ADDRESS, &mut gpa);
                }

                let data = [0u8; 8];

                Ok(VmExit::Mmio {
                    phys_addr: gpa,
                    data,
                    len: 8,
                    is_write,
                })
            }
            _ => Ok(VmExit::Unknown {
                reason: (exit_reason & 0xFFFF) as u32,
            }),
        }
    }

    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
        let states = self.vcpu_states.read();
        if let Some(state) = states.get(&vcpu.id()) {
            state.pending_vector.store(vector as u64, Ordering::SeqCst);
            state.interrupt_pending.store(true, Ordering::SeqCst);

            // Also try to wake up the vCPU if it's halted
            // SAFETY: FFI call to hv_vcpu_interrupt with valid vCPU handle reference.
            // Waking the vCPU so it can process the queued interrupt.
            unsafe {
                hv_vcpu_interrupt(&state.hv_vcpu, 1);
            }

            tracing::debug!("Queued interrupt {} for vCPU {}", vector, vcpu.id());
        }
        Ok(())
    }

    async fn inject_exception(
        &self,
        vcpu: &VCpu,
        vector: u8,
        error_code: Option<u32>,
    ) -> Result<()> {
        let states = self.vcpu_states.read();
        let state = states.get(&vcpu.id()).ok_or(Error::Hypervisor(format!(
            "HVF vCPU {} not found",
            vcpu.id()
        )))?;

        // VMX VM-entry interruption-info format (VMCS 0x4016):
        //   Bits  7:0   Vector
        //   Bits 10:8   Type (3 = hardware exception)
        //   Bit  11     Deliver error code
        //   Bit  31     Valid
        let mut info: u64 = 0x8000_0000 | 0x300 | (vector as u64);

        if error_code.is_some() {
            info |= 0x800; // Deliver error code bit
        }

        // SAFETY: FFI calls to hv_vmx_vcpu_write_vmcs with valid vCPU handle from state.
        // Writing VM-entry interruption info and optional error code to VMCS fields.
        unsafe {
            let hr = hv_vmx_vcpu_write_vmcs(
                state.hv_vcpu,
                VmcsField::VmEntryInterruptionInfo as u32,
                info,
            );
            if hr != 0 {
                return Err(Error::Hypervisor(format!(
                    "Failed to write VM-entry interruption info: 0x{:08X}",
                    hr
                )));
            }

            // Set error code if present
            if let Some(code) = error_code {
                let hr = hv_vmx_vcpu_write_vmcs(
                    state.hv_vcpu,
                    VmcsField::VmEntryExceptionErrorCode as u32,
                    code as u64,
                );
                if hr != 0 {
                    return Err(Error::Hypervisor(format!(
                        "Failed to write exception error code: 0x{:08X}",
                        hr
                    )));
                }
            }
        }

        tracing::debug!(
            "Injected exception: vector={} error_code={:?} into vCPU {}",
            vector,
            error_code,
            vcpu.id()
        );
        Ok(())
    }

    async fn set_io_result(&self, vcpu: &VCpu, data: u32, size: u8) -> Result<()> {
        let states = self.vcpu_states.read();
        let state = states.get(&vcpu.id()).ok_or(Error::Hypervisor(format!(
            "HVF vCPU {} not found",
            vcpu.id()
        )))?;

        // Mask the data by access size
        let masked: u64 = match size {
            1 => (data & 0xFF) as u64,
            2 => (data & 0xFFFF) as u64,
            4 => data as u64,
            _ => data as u64,
        };

        // SAFETY: FFI call to hv_vcpu_write_register with valid vCPU handle from state.
        // Writing the masked I/O result value into the RAX register.
        let hr = unsafe { hv_vcpu_write_register(state.hv_vcpu, HvX86Reg::Rax as u32, masked) };

        if hr != 0 {
            return Err(Error::Hypervisor(format!(
                "Failed to set IO result (RAX) for vCPU {}: 0x{:08X}",
                vcpu.id(),
                hr
            )));
        }

        tracing::trace!(
            "Set IO IN result: vCPU={} data={:#x} size={}",
            vcpu.id(),
            masked,
            size
        );
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Destroy vCPUs
        for (_, state) in self.vcpu_states.write().drain() {
            // SAFETY: FFI call to hv_vcpu_destroy with valid vCPU handle from state.
            // Each handle was obtained from a successful hv_vcpu_create call.
            unsafe { hv_vcpu_destroy(state.hv_vcpu) };
        }

        // Unmap memory
        for mapping in self.memory_mappings.write().drain(..) {
            // SAFETY: FFI call to hv_vm_unmap with guest address and size that were previously
            // registered via hv_vm_map.
            unsafe { hv_vm_unmap(mapping.guest_addr, mapping.size) };
        }

        // Free guest RAM — only after unmapping it, so the hypervisor is never
        // left holding a mapping to memory the allocator has reclaimed.
        if let Some(allocation) = self.guest_memory.write().take() {
            // SAFETY: `ptr` came from `alloc_zeroed` in `create_vm` with this
            // same size and 4 KB alignment, was unmapped just above, and is not
            // referenced anywhere else.
            unsafe {
                let layout =
                    std::alloc::Layout::from_size_align_unchecked(allocation.size as usize, 4096);
                std::alloc::dealloc(allocation.ptr, layout);
            }
        }

        // Destroy VM
        if self.vm_created.swap(false, Ordering::SeqCst) {
            // SAFETY: FFI call to hv_vm_destroy. The vm_created flag confirms a VM exists,
            // and all vCPUs and memory mappings have been cleaned up above.
            unsafe { hv_vm_destroy() };
            tracing::info!("HVF VM destroyed");
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// SAFETY: HvfBackend can be sent between threads because the HVF vCPU handles are
// thread-safe and all mutable state is protected by RwLock or atomic operations.
unsafe impl Send for HvfBackend {}
// SAFETY: HvfBackend can be shared between threads because all mutable state
// (vcpu_states, memory_mappings) is protected by RwLock or AtomicBool.
unsafe impl Sync for HvfBackend {}
