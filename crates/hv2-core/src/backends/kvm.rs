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
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm, VCpuDiagnostic,
    VCpuInterruptState, VCpuRunRetries,
};
use crate::snapshot::vcpu::{
    DescriptorTable, FpuState, GeneralRegisters, Msr, RunState, Segment, SystemRegisters,
    VCpuSnapshot,
};
use crate::{Error, IoDirection, Result, VCpu, VmExit};
use async_trait::async_trait;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Once, RwLock};

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

/// Tell vCPU `vcpu_id` of `vcpu_count` who it is: its initial APIC ID and the
/// package's logical processor count in leaf 1 (with HTT, which says that
/// count means something), and its x2APIC ID in every subleaf of leaves 0xB
/// and 0x1F. KVM gives vCPU `i` local APIC ID `i`, and the MP table lists it
/// so; CPUID has to agree.
#[cfg(target_os = "linux")]
fn patch_topology(entries: &mut [kvm_cpuid_entry2], vcpu_id: u32, vcpu_count: u32) {
    for entry in entries {
        match entry.function {
            1 => {
                entry.ebx = (entry.ebx & 0x0000_FFFF)
                    | ((vcpu_id & 0xFF) << 24)
                    | ((vcpu_count.min(0xFF)) << 16);
                if vcpu_count == 1 {
                    entry.edx &= !(1 << 28);
                } else {
                    entry.edx |= 1 << 28;
                }
            }
            0xB | 0x1F | 0x8000_0026 => {
                entry.edx = vcpu_id;
                if vcpu_count == 1 && entry.ebx & 0xffff != 0 {
                    // Preserve supported level types and terminating subleaves.
                    entry.eax &= !0x1f;
                    entry.ebx = (entry.ebx & !0xffff) | 1;
                }
            }
            4 | 0x8000_001D if vcpu_count == 1 && entry.eax & 0x1f != 0 => {
                // Retain cache geometry; only topology/sharing changes.
                entry.eax &= !(0xfff << 14);
                if entry.function == 4 {
                    entry.eax &= !(0x3f << 26);
                }
            }
            0x8000_0008 if vcpu_count == 1 => entry.ecx &= !0xf0ff,
            0x8000_001E if vcpu_count == 1 => {
                entry.eax = vcpu_id;
                entry.ebx = 0; // core 0, one thread per core
                entry.ecx = 0; // node 0, one node per package
            }
            _ => {}
        }
    }
}

fn interrupt_state(events: kvm_vcpu_events) -> VCpuInterruptState {
    // asm/kvm.h: fields marked optional must not be interpreted without flags.
    const VALID_NMI_PENDING: u32 = 1;
    const VALID_SHADOW: u32 = 4;
    const VALID_PAYLOAD: u32 = 0x10;
    VCpuInterruptState {
        flags: events.flags,
        injected: events.interrupt.injected,
        vector: events.interrupt.nr,
        soft: events.interrupt.soft,
        shadow: (events.flags & VALID_SHADOW != 0).then_some(events.interrupt.shadow),
        exception_injected: events.exception.injected,
        exception_vector: events.exception.nr,
        exception_pending: (events.flags & VALID_PAYLOAD != 0).then_some(events.exception.pending),
        nmi_injected: events.nmi.injected,
        nmi_pending: (events.flags & VALID_NMI_PENDING != 0).then_some(events.nmi.pending),
        nmi_masked: events.nmi.masked,
    }
}

/// Encode every initialized ABI field explicitly, without reading Rust padding.
fn event_bytes(events: &kvm_vcpu_events) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend([
        events.exception.injected,
        events.exception.nr,
        events.exception.has_error_code,
        events.exception.pending,
    ]);
    bytes.extend(events.exception.error_code.to_le_bytes());
    bytes.extend([
        events.interrupt.injected,
        events.interrupt.nr,
        events.interrupt.soft,
        events.interrupt.shadow,
        events.nmi.injected,
        events.nmi.pending,
        events.nmi.masked,
        events.nmi.pad,
    ]);
    bytes.extend(events.sipi_vector.to_le_bytes());
    bytes.extend(events.flags.to_le_bytes());
    bytes.extend([
        events.smi.smm,
        events.smi.pending,
        events.smi.smm_inside_nmi,
        events.smi.latched_init,
    ]);
    bytes.extend(events.reserved);
    bytes.push(events.exception_has_payload);
    bytes.extend(events.exception_payload.to_le_bytes());
    bytes
}

fn events_from_bytes(bytes: &[u8]) -> Result<kvm_vcpu_events> {
    let bytes: &[u8; 64] = bytes.try_into().map_err(|_| {
        Error::InvalidState(format!(
            "this snapshot's KVM vCPU events are {} bytes, not 64",
            bytes.len()
        ))
    })?;
    let word = |at| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let mut reserved = [0; 27];
    reserved.copy_from_slice(&bytes[28..55]);
    let mut payload = [0; 8];
    payload.copy_from_slice(&bytes[56..64]);
    Ok(kvm_vcpu_events {
        exception: kvm_vcpu_events_exception {
            injected: bytes[0],
            nr: bytes[1],
            has_error_code: bytes[2],
            pending: bytes[3],
            error_code: word(4),
        },
        interrupt: kvm_vcpu_events_interrupt {
            injected: bytes[8],
            nr: bytes[9],
            soft: bytes[10],
            shadow: bytes[11],
        },
        nmi: kvm_vcpu_events_nmi {
            injected: bytes[12],
            pending: bytes[13],
            masked: bytes[14],
            pad: bytes[15],
        },
        sipi_vector: word(16),
        flags: word(20),
        smi: kvm_vcpu_events_smi {
            smm: bytes[24],
            pending: bytes[25],
            smm_inside_nmi: bytes[26],
            latched_init: bytes[27],
        },
        reserved,
        exception_has_payload: bytes[55],
        exception_payload: u64::from_le_bytes(payload),
    })
}

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
    /// The one VM this backend owns, if `create_vm` has run.
    ///
    /// At most one. The `HypervisorBackend` trait identifies a vCPU by its
    /// bare id, so a second VM would bring its own vCPU 0 and every lookup
    /// here — `run_vcpu`, `load_boot`, interrupt injection — would silently
    /// resolve to whichever VM was created last. `create_vm` refuses the
    /// second VM rather than allow that. A caller who wants two VMs builds
    /// two backends, which is what `VM::new` does.
    vm: Arc<RwLock<Option<Arc<KvmVm>>>>,
    /// vCPU lookup: maps VCpu::id() → KvmVcpu.
    ///
    /// Keyed by bare vCPU id, which is unambiguous only because of the
    /// one-VM invariant documented on `vm`.
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
        // Everything below is x86: general and special registers, CPUID, the
        // PIC/IOAPIC irqchip, the bzImage and multiboot loaders. On an arm64
        // host the crate compiles -- `cargo check` for aarch64 passes -- and
        // the first of those ioctls would fail with an error naming neither
        // the architecture nor the reason. Refuse here instead.
        if cfg!(not(target_arch = "x86_64")) {
            return Err(Error::NotSupported(
                "the KVM backend is x86_64-only: an arm64 guest needs vCPU init by \
                 KVM_ARM_VCPU_INIT, a GICv3 through KVM_CREATE_DEVICE, a device tree \
                 and the arm64 Image boot protocol, none of which exist here yet"
                    .into(),
            ));
        }
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
                vm: Arc::new(RwLock::new(None)),
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

        // Hold the slot for the whole of creation so two concurrent callers
        // cannot both pass the check. Lock order is `vm` then `vcpu_map`;
        // every other path releases `vcpu_map` before touching `vm`.
        let mut slot = self.vm.write().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err(Error::Hypervisor(
                "this KVM backend already owns a VM. A backend owns at most one: vCPUs \
                 are looked up by bare id, so a second VM's vCPU 0 would collide with the \
                 first's. Build a second backend for a second VM."
                    .into(),
            ));
        }

        // Create KVM VM instance
        let kvm_vm = Arc::new(KvmVm::new(
            self.kvm_fd,
            vcpu_count,
            memory_size,
            self.run_mmap_size,
        )?);

        // Create vCPUs and register them in the lookup map
        for i in 0..vcpu_count {
            let kvm_vcpu = kvm_vm.create_vcpu(i)?;
            self.vcpu_map
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(i, kvm_vcpu);
        }

        *slot = Some(kvm_vm);
        drop(slot);

        Ok(HypervisorVm::new(
            HypervisorPlatform::Kvm,
            vcpu_count,
            memory_size,
        ))
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        let kvm_vcpu = self.kvm_vcpu(vcpu)?;

        // Run the vCPU until it exits — this blocks until a VM exit occurs
        kvm_vcpu.run()
    }

    async fn save_vcpu(&self, vcpu: &VCpu) -> Result<VCpuSnapshot> {
        let kvm_vcpu = self.kvm_vcpu(vcpu)?;
        let fd = kvm_vcpu.fd();

        let mut regs = kvm_regs::default();
        let mut sregs = kvm_sregs::default();
        let mut fpu = kvm_fpu::default();
        let mut mp_state = kvm_mp_state::default();

        // SAFETY: `fd` is this vCPU's file descriptor, held alive by the
        // `KvmVcpu` above, and each target is a correctly-sized struct this
        // function owns. The guest is not executing: the caller pauses first,
        // and reading these from a running vCPU is what makes a snapshot
        // describe a machine that no longer exists.
        unsafe {
            kvm_get_regs(fd, &mut regs)
                .map_err(|e| Error::Hypervisor(format!("KVM_GET_REGS: {e}")))?;
            kvm_get_sregs(fd, &mut sregs)
                .map_err(|e| Error::Hypervisor(format!("KVM_GET_SREGS: {e}")))?;
            kvm_get_fpu(fd, &mut fpu)
                .map_err(|e| Error::Hypervisor(format!("KVM_GET_FPU: {e}")))?;
            kvm_get_mp_state(fd, &mut mp_state)
                .map_err(|e| Error::Hypervisor(format!("KVM_GET_MP_STATE: {e}")))?;
        }

        // One at a time, and a failure on one is not a failure of the
        // snapshot: `SNAPSHOT_MSRS` is what a guest might use, not what every
        // host implements, and an MSR this processor does not have is not part
        // of this guest's state. A restore only writes back what was read.
        let clock_msrs = crate::snapshot::machine::CLOCK_MSRS;
        let mut msrs = Vec::with_capacity(SNAPSHOT_MSRS.len() + clock_msrs.len());
        // The clock MSRs are captured every time and restored only on request
        // -- see `ClockOnRestore`. Capturing is free; deciding is the caller's.
        for index in SNAPSHOT_MSRS.iter().chain(clock_msrs) {
            // SAFETY: `fd` is this vCPU's descriptor, as above.
            match unsafe { kvm_get_msr(fd, *index) } {
                Ok(value) => msrs.push(Msr {
                    index: *index,
                    value,
                }),
                Err(e) => {
                    tracing::debug!("vCPU {}: MSR {index:#x} not readable: {e}", vcpu.id());
                }
            }
        }

        // Both are optional in the same sense the MSRs are: a host with no
        // in-kernel APIC has none to read, and a processor without XSAVE has
        // no area. A snapshot that records neither is still a usable snapshot
        // of a guest that used neither, so a failure here is logged and the
        // field left empty rather than failing the capture.
        let mut lapic_state = kvm_lapic_state::default();
        // SAFETY: `fd` is this vCPU's descriptor, as above.
        let lapic = match unsafe { kvm_get_lapic(fd, &mut lapic_state) } {
            Ok(()) => lapic_state.regs.to_vec(),
            Err(e) => {
                tracing::debug!("vCPU {}: no LAPIC state: {e}", vcpu.id());
                Vec::new()
            }
        };

        let mut xsave_state = kvm_xsave::default();
        // SAFETY: as above.
        let xsave = match unsafe { kvm_get_xsave(fd, &mut xsave_state) } {
            Ok(()) => xsave_state
                .region
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect(),
            Err(e) => {
                tracing::debug!("vCPU {}: no XSAVE area: {e}", vcpu.id());
                Vec::new()
            }
        };

        let mut xcr_state = kvm_xcrs::default();
        // SAFETY: as above.
        let xcrs = match unsafe { kvm_get_xcrs(fd, &mut xcr_state) } {
            Ok(()) => xcr_state
                .xcrs
                .iter()
                .take((xcr_state.nr_xcrs as usize).min(KVM_MAX_XCRS))
                .map(|x| Msr {
                    index: x.xcr,
                    value: x.value,
                })
                .collect(),
            Err(e) => {
                tracing::debug!("vCPU {}: no XCRs: {e}", vcpu.id());
                Vec::new()
            }
        };

        // Event injection is separate from the LAPIC's pending/in-service
        // registers. Failing this read must not silently produce a snapshot
        // that drops an interrupt or exception already handed to the vCPU.
        let mut events = kvm_vcpu_events::default();
        // SAFETY: the paused vCPU's descriptor and an initialized owned payload.
        unsafe { kvm_get_vcpu_events(fd, &mut events) }
            .map_err(|e| Error::Hypervisor(format!("KVM_GET_VCPU_EVENTS: {e}")))?;

        Ok(VCpuSnapshot {
            id: vcpu.id(),
            general: general_from(&regs),
            system: system_from(&sregs),
            fpu: fpu_from(&fpu),
            msrs,
            lapic,
            xsave,
            xcrs,
            kvm_events: event_bytes(&events),
            run_state: match mp_state.mp_state {
                KVM_MP_STATE_RUNNABLE => RunState::Runnable,
                KVM_MP_STATE_HALTED => RunState::Halted,
                other => RunState::Other(other),
            },
        })
    }

    async fn inspect_vcpu(&self, vcpu: &VCpu) -> Result<VCpuDiagnostic> {
        let architecture = self.save_vcpu(vcpu).await?;
        let interrupts = self
            .kvm_vcpu(vcpu)?
            .get_vcpu_events()
            .map(|events| Some(interrupt_state(events)));
        Ok(VCpuDiagnostic {
            architecture,
            interrupts,
            run_retries: Some(VCpuRunRetries {
                eintr: self.kvm_vcpu(vcpu)?.retry_eintr.load(Ordering::Relaxed),
                eagain: self.kvm_vcpu(vcpu)?.retry_eagain.load(Ordering::Relaxed),
            }),
        })
    }

    async fn restore_vcpu(&self, vcpu: &VCpu, state: &VCpuSnapshot) -> Result<()> {
        let kvm_vcpu = self.kvm_vcpu(vcpu)?;
        let fd = kvm_vcpu.fd();
        let events = if state.kvm_events.is_empty() {
            None // Legacy snapshots retain their prior event-restore behavior.
        } else {
            Some(events_from_bytes(&state.kvm_events)?)
        };

        let regs = general_into(&state.general);
        let sregs = system_into(&state.system);
        let fpu = fpu_into(&state.fpu)?;
        let mp_state = kvm_mp_state {
            mp_state: match state.run_state {
                RunState::Runnable => KVM_MP_STATE_RUNNABLE,
                RunState::Halted => KVM_MP_STATE_HALTED,
                RunState::Other(other) => other,
            },
        };

        // SAFETY: as in `save_vcpu`, with structs this function built and owns.
        //
        // Order matters: the special registers decide what the general ones
        // mean -- paging mode, privilege level, where every segment points --
        // so they go first. Writing RIP into a vCPU that is still in the
        // restorer's idea of long mode, and only then switching modes, is a
        // guest that resumes at an address the hardware reads differently.
        unsafe {
            kvm_set_sregs(fd, &sregs)
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_SREGS: {e}")))?;
            kvm_set_regs(fd, &regs).map_err(|e| Error::Hypervisor(format!("KVM_SET_REGS: {e}")))?;
            kvm_set_fpu(fd, &fpu).map_err(|e| Error::Hypervisor(format!("KVM_SET_FPU: {e}")))?;
            kvm_set_mp_state(fd, &mp_state)
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_MP_STATE: {e}")))?;
        }

        // The XSAVE area supersedes the legacy FPU written above: it is the
        // same registers plus everything the processor added since, so it goes
        // second and wins. Writing only the legacy view over a guest that was
        // using AVX would leave half its register file from the snapshot and
        // half from whatever this vCPU had.
        // XCR0 before the XSAVE area: the kernel validates the area against
        // the components XCR0 enables, so the other order refuses AVX state
        // the guest was using.
        if !state.xcrs.is_empty() {
            let mut xcrs = kvm_xcrs::default();
            for (slot, xcr) in xcrs.xcrs.iter_mut().zip(state.xcrs.iter()) {
                slot.xcr = xcr.index;
                slot.value = xcr.value;
            }
            xcrs.nr_xcrs = state.xcrs.len().min(KVM_MAX_XCRS) as u32;
            // SAFETY: `fd` is this vCPU's descriptor; `xcrs` is a struct this
            // function owns.
            unsafe { kvm_set_xcrs(fd, &xcrs) }
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_XCRS: {e}")))?;
        }

        if !state.xsave.is_empty() {
            let mut area = kvm_xsave::default();
            if state.xsave.len() != area.region.len() * 4 {
                return Err(Error::Hypervisor(format!(
                    "this snapshot's XSAVE area is {} bytes and this host's is {}",
                    state.xsave.len(),
                    area.region.len() * 4
                )));
            }
            let (words, _) = state.xsave.as_chunks::<4>();
            for (word, chunk) in area.region.iter_mut().zip(words) {
                *word = u32::from_le_bytes(*chunk);
            }
            // SAFETY: `fd` is this vCPU's descriptor; `area` is a correctly
            // sized struct this function owns.
            unsafe { kvm_set_xsave(fd, &area) }
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_XSAVE: {e}")))?;
        }

        if !state.lapic.is_empty() {
            let mut lapic = kvm_lapic_state::default();
            if state.lapic.len() != lapic.regs.len() {
                return Err(Error::Hypervisor(format!(
                    "this snapshot's LAPIC page is {} bytes and this host's is {}",
                    state.lapic.len(),
                    lapic.regs.len()
                )));
            }
            lapic.regs.copy_from_slice(&state.lapic);
            // SAFETY: as above.
            unsafe { kvm_set_lapic(fd, &lapic) }
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_LAPIC: {e}")))?;
        }

        // Unlike the read, a write that fails is fatal. Every MSR here was
        // readable on the machine that took the snapshot, so one this host
        // refuses means the two processors disagree about what the guest is --
        // and a guest resumed without its SYSCALL entry point does not survive
        // its next system call. Better to refuse the restore than to produce a
        // VM that runs for a microsecond.
        for msr in &state.msrs {
            // SAFETY: `fd` is this vCPU's descriptor, as above.
            unsafe { kvm_set_msr(fd, msr.index, msr.value) }.map_err(|e| {
                Error::Hypervisor(format!(
                    "KVM_SET_MSRS for {:#x}: {e}. The snapshot was taken on a host that has \
                     this register and this one does not.",
                    msr.index
                ))
            })?;
        }
        if let Some(mut events) = events {
            // All vCPUs are paused. GET always supplies NMI pending and SIPI
            // state; SET requires explicit validity bits to restore them.
            // Preserve GET's capability-dependent shadow/SMM/payload flags.
            events.flags |= KVM_VCPUEVENT_VALID_NMI_PENDING | KVM_VCPUEVENT_VALID_SIPI_VECTOR;
            // Apply after special registers, LAPIC and clock MSRs, which can
            // otherwise reset or replace parts of the injection state.
            // SAFETY: a paused vCPU fd and a validated, initialized ABI payload.
            unsafe { kvm_set_vcpu_events(fd, &events) }
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_VCPU_EVENTS: {e}")))?;
        }
        Ok(())
    }

    /// True. `create_vm` maps guest RAM with `MAP_ANONYMOUS`, which the kernel
    /// guarantees is zero, and `load_boot` is the first thing to write to it.
    ///
    /// This is the whole justification, and it is narrow on purpose: it is a
    /// statement about this backend's allocation path and not about KVM. A
    /// backend that restored a snapshot into existing memory would have to
    /// answer differently, which is why the trait's default is `false`.
    fn guest_memory_starts_zeroed(&self) -> bool {
        true
    }

    async fn save_machine(&self) -> Result<Option<crate::snapshot::machine::MachineState>> {
        let Some(kvm_vm) = self.vm.read().unwrap_or_else(|e| e.into_inner()).clone() else {
            return Ok(None);
        };
        let chip = |id| kvm_vm.get_irqchip(id).map(|c| c.chip.to_vec());
        let mut pit = kvm_pit_state2::default();
        // SAFETY: a valid VM fd, and `create_vm` always creates the PIT.
        unsafe { kvm_get_pit2(kvm_vm.vm_fd, &mut pit) }
            .map_err(|e| Error::Hypervisor(format!("KVM_GET_PIT2: {e}")))?;
        let mut clock = kvm_clock_data::default();
        // SAFETY: a valid VM fd and a struct this function owns.
        let clock_ns = match unsafe { kvm_get_clock(kvm_vm.vm_fd, &mut clock) } {
            Ok(()) => Some(clock.clock),
            Err(e) => {
                tracing::debug!("no kvmclock to capture: {e}");
                None
            }
        };
        Ok(Some(crate::snapshot::machine::MachineState {
            backend: "kvm".to_string(),
            pic_master: chip(KVM_IRQCHIP_PIC_MASTER)?,
            pic_slave: chip(KVM_IRQCHIP_PIC_SLAVE)?,
            ioapic: chip(KVM_IRQCHIP_IOAPIC)?,
            pit: pit.bytes.to_vec(),
            clock_ns,
        }))
    }

    async fn restore_machine(
        &self,
        state: &crate::snapshot::machine::MachineState,
        restore_clock: bool,
    ) -> Result<()> {
        if state.backend != "kvm" {
            return Err(Error::InvalidState(format!(
                "this snapshot's machine state is for the {} backend",
                state.backend
            )));
        }
        let kvm_vm = self
            .vm
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| Error::Hypervisor("no KVM VM to restore machine state into".into()))?;

        for (id, bytes) in [
            (KVM_IRQCHIP_PIC_MASTER, &state.pic_master),
            (KVM_IRQCHIP_PIC_SLAVE, &state.pic_slave),
            (KVM_IRQCHIP_IOAPIC, &state.ioapic),
        ] {
            let mut chip = kvm_irqchip {
                chip_id: id,
                pad: 0,
                chip: [0u8; 512],
            };
            if bytes.len() != chip.chip.len() {
                return Err(Error::InvalidState(format!(
                    "irqchip {id} in this snapshot is {} bytes, not {}",
                    bytes.len(),
                    chip.chip.len()
                )));
            }
            chip.chip.copy_from_slice(bytes);
            kvm_vm.set_irqchip(&chip)?;
        }

        let mut pit = kvm_pit_state2::default();
        if state.pit.len() != pit.bytes.len() {
            return Err(Error::InvalidState(format!(
                "the PIT in this snapshot is {} bytes, not {}",
                state.pit.len(),
                pit.bytes.len()
            )));
        }
        pit.bytes.copy_from_slice(&state.pit);
        // SAFETY: a valid VM fd, and a struct this function owns.
        unsafe { kvm_set_pit2(kvm_vm.vm_fd, &pit) }
            .map_err(|e| Error::Hypervisor(format!("KVM_SET_PIT2: {e}")))?;

        if let (true, Some(ns)) = (restore_clock, state.clock_ns) {
            let clock = kvm_clock_data {
                clock: ns,
                ..Default::default()
            };
            // SAFETY: as above.
            unsafe { kvm_set_clock(kvm_vm.vm_fd, &clock) }
                .map_err(|e| Error::Hypervisor(format!("KVM_SET_CLOCK: {e}")))?;
        }
        Ok(())
    }

    async fn map_shared_rom(&self, guest_addr: u64, host_addr: u64, len: u64) -> Result<()> {
        let kvm_vm = self
            .vm
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                Error::Hypervisor(
                    "no KVM VM — create_vm must run before a shared region can be mapped".into(),
                )
            })?;

        // SAFETY: `KVM_CHECK_EXTENSION` is a system ioctl on `/dev/kvm` with no
        // side effects.
        let read_only_supported = unsafe { kvm_check_extension(self.kvm_fd, KVM_CAP_READONLY_MEM) }
            .map(|v| v > 0)
            .unwrap_or(false);
        if !read_only_supported {
            return Err(Error::NotSupported(
                "this KVM does not support read-only memory slots, so a shared region could \
                 not be protected from the guests reading it"
                    .into(),
            ));
        }

        // Slots 0 and 1 are the guest's own RAM, either side of the hole
        // below 4 GiB. Shared regions start at 2 and there is one per VM,
        // which is all the model-weights case needs.
        kvm_vm.map_memory_with_flags(2, guest_addr, len, host_addr, KVM_MEM_READONLY)
    }

    async fn kick_vcpu(&self, vcpu: &VCpu) -> Result<()> {
        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };

        kvm_vcpu.kick();
        Ok(())
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

    fn guest_memory_host_addr(&self) -> Option<u64> {
        self.vm
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|vm| vm.guest_memory())
            .map(|ptr| ptr.as_ptr() as u64)
    }

    /// Yes, by discarding the pages -- see `KvmVm::discard_guest_memory`.
    ///
    /// This backend can answer honestly because it owns the mapping and knows
    /// how it was made. `Ok(false)` from here would not be wrong, only slower.
    fn reset_guest_memory_to_zero(&self) -> Result<bool> {
        let vm = self.vm.read().unwrap_or_else(|e| e.into_inner());
        let Some(vm) = vm.as_ref() else {
            // No VM yet means no guest memory to hold anything, but saying
            // "done" would be a claim about memory that does not exist.
            return Ok(false);
        };
        if vm.guest_memory().is_none() {
            return Ok(false);
        }
        vm.discard_guest_memory()?;
        Ok(true)
    }

    fn map_guest_memory_from(&self, file: &std::fs::File) -> Result<bool> {
        let vm = self.vm.read().unwrap_or_else(|e| e.into_inner());
        let Some(vm) = vm.as_ref() else {
            return Ok(false);
        };
        vm.map_guest_memory_file(file)?;
        Ok(true)
    }

    /// `KVM_PRE_FAULT_MEMORY`, Linux 6.10 and later. A kernel without it, or
    /// a range it will not take, is reported as `Ok(false)` -- the guest then
    /// faults those pages in itself, which is only slower.
    fn prefault_guest_memory(&self, vcpu: &VCpu, ranges: &[(u64, u64)]) -> Result<bool> {
        let kvm_vcpu = self.kvm_vcpu(vcpu)?;
        let fd = kvm_vcpu.fd();
        for &(gpa, size) in ranges {
            let mut range = kvm_pre_fault_memory {
                gpa,
                size,
                ..Default::default()
            };
            while range.size > 0 {
                // SAFETY: `fd` is this vCPU's descriptor, and `range` a struct
                // this function owns, of the size the ioctl number encodes.
                match unsafe { kvm_pre_fault_memory(fd, &mut range) } {
                    Ok(()) => {}
                    Err(e) if matches!(e.raw_os_error(), Some(libc::EINTR | libc::EAGAIN)) => {}
                    Err(e) => {
                        tracing::debug!(
                            "KVM_PRE_FAULT_MEMORY at {:#x} (+{:#x}): {e}; the guest will fault \
                             the rest in itself",
                            range.gpa,
                            range.size
                        );
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    async fn set_mmio_result(&self, vcpu: &VCpu, data: &[u8]) -> Result<()> {
        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };

        kvm_vcpu.set_mmio_data(data)
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
            .vm
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                Error::Hypervisor("no KVM VM — create_vm must run before load_boot".into())
            })?;

        // Every protocol starts the same way: the images go into guest RAM.
        //
        // The bytes, and then the `.bss` only if it needs writing. Guest RAM
        // here is a fresh anonymous mapping made in `create_vm` and nothing has
        // touched it since, so it already reads as zero and writing zeros over
        // it would achieve nothing except making every page of it resident.
        for (addr, data) in boot.data_regions_borrowed()? {
            kvm_vm.write_guest_memory(addr, &data)?;
        }
        if !self.guest_memory_starts_zeroed() {
            for (addr, len) in boot.zero_ranges()? {
                kvm_vm.write_guest_memory(addr, &vec![0u8; len as usize])?;
            }
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

                // Describe the I/O APIC even with a single processor. Omitting
                // this table leaves singleton Linux guests on the PIC virtual
                // wire instead of describing the interrupt controller we made.
                // This is cold-boot setup; restored guests retain the tables
                // and interrupt-controller state from their snapshot.
                use crate::boot::mptable;
                if kvm_vm.vcpu_count > mptable::MAX_CPUS {
                    return Err(Error::Config(format!(
                        "{} vCPUs: a Linux guest here has at most {}",
                        kvm_vm.vcpu_count,
                        mptable::MAX_CPUS
                    )));
                }
                kvm_vm.write_guest_memory(
                    mptable::MPTABLE_ADDR,
                    &mptable::build(kvm_vm.vcpu_count),
                )?;
                // A PCI-less guest is also described as a hardware-reduced
                // ACPI platform, which Linux prefers to the MP table: no legacy
                // PIC, and no 24-entry I/O APIC mask pass at boot.
                if let Some(devices) = &params.hw_reduced_acpi {
                    use crate::boot::acpi_tables;
                    kvm_vm.write_guest_memory(
                        acpi_tables::ACPI_ADDR,
                        &acpi_tables::build(kvm_vm.vcpu_count, devices),
                    )?;
                }

                let mut sregs = kvm_vcpu.get_sregs()?;
                sregs.gdt.base = gdt_base;
                sregs.gdt.limit = (gdt.len() - 1) as u16;
                apply_flat_protected_mode(&mut sregs);
                kvm_vcpu.set_sregs(&sregs)?;

                let mut regs = kvm_vcpu.get_regs()?;
                regs.rip = params.kernel_addr;
                regs.rsi = params.setup_addr; // boot_params, per the protocol
                regs.rsp = stack_pointer;
                // The 32-bit boot protocol requires these three to be zero
                // (Documentation/x86/boot.rst). The kernel overwrites all of
                // them within its first few instructions, so this is not what
                // makes a boot work — but leaving a stack pointer in %ebp was
                // a documented requirement quietly unmet, and those are worth
                // fixing before the ones that are only suspected.
                regs.rbp = 0;
                regs.rdi = 0;
                regs.rbx = 0;
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
                // Where the image asked to be entered, which is only
                // `layout.kernel_addr` for a flat image. The bytes went to the
                // matching addresses in the loop above, via `memory_regions`.
                let entry = boot.entry_point()?;
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
                regs.rip = entry;
                regs.rax = u64::from(MultibootProtocol::bootloader_magic());
                regs.rbx = layout.info_addr;
                regs.rsp = stack_pointer;
                regs.rbp = stack_pointer;
                // Multiboot requires interrupts disabled on entry, which is
                // RFLAGS with only the reserved bit set.
                regs.rflags = RFLAGS_RESERVED;
                kvm_vcpu.set_regs(&regs)?;

                tracing::info!(
                    "KVM: Multiboot kernel entered at {:#x}, info at {:#x}, {} module(s)",
                    entry,
                    layout.info_addr,
                    info.modules.len()
                );
                Ok(())
            }
        }
    }

    async fn single_step_trace(
        &self,
        vcpu: &VCpu,
        max_steps: u64,
    ) -> Result<crate::hypervisor::SingleStepTrace> {
        use crate::hypervisor::{SingleStepTrace, TRACE_TAIL};

        let kvm_vcpu = {
            let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
            map.get(&vcpu.id())
                .cloned()
                .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))?
        };

        kvm_vcpu.set_guest_debug(KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_SINGLESTEP)?;

        let mut tail: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
        let mut steps = 0u64;
        let mut final_exit = None;

        while steps < max_steps {
            // Read the address before stepping, not after: a triple fault
            // resets the vCPU on the way out, and a read afterwards reports the
            // reset vector for every guest that ever fails.
            let rip = match kvm_vcpu.get_regs() {
                Ok(regs) => regs.rip,
                Err(e) => {
                    let _ = kvm_vcpu.set_guest_debug(0);
                    return Err(e);
                }
            };
            if tail.len() == TRACE_TAIL {
                tail.pop_front();
            }
            tail.push_back(rip);
            steps += 1;

            match kvm_vcpu.run() {
                Ok(VmExit::Debug { .. }) => {}
                Ok(other) => {
                    final_exit = Some(other);
                    break;
                }
                Err(e) => {
                    let _ = kvm_vcpu.set_guest_debug(0);
                    return Err(e);
                }
            }
        }

        // Leave the vCPU as it was found, so a caller can trace and then run
        // normally without every instruction trapping.
        kvm_vcpu.set_guest_debug(0)?;

        Ok(SingleStepTrace {
            steps,
            tail: tail.into_iter().collect(),
            final_exit,
        })
    }

    async fn set_irq_line(&self, irq: u32, level: bool) -> Result<()> {
        let kvm_vm = self
            .vm
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                Error::Hypervisor(
                    "no KVM VM — create_vm must run before an IRQ can be raised".into(),
                )
            })?;

        kvm_vm.irq_line(irq, u32::from(level))
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down KVM backend");

        // Clear vCPU map
        self.vcpu_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        // The VM is automatically closed when dropped
        *self.vm.write().unwrap_or_else(|e| e.into_inner()) = None;

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
    /// `/dev/kvm`, kept because the supported CPUID set is queried from it.
    ///
    /// Owned by [`KvmBackend`] and not closed here.
    kvm_fd: RawFd,
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
    /// Whether `KVM_CREATE_IRQCHIP` succeeded, so the PIC, IOAPIC and LAPIC
    /// are inside the kernel.
    ///
    /// Recorded rather than assumed because it decides which of two mutually
    /// exclusive ways of delivering an interrupt is the one that works, and
    /// the failure when they are confused is `ENXIO` from an ioctl several
    /// layers below whoever made the choice. See
    /// [`KvmVcpu::inject_interrupt`].
    irqchip_in_kernel: bool,
}

/// Transparent huge page size on x86-64.
const THP_SIZE: usize = 2 * 1024 * 1024;

/// Map `size` bytes of zero-filled guest RAM, aligned to 2 MiB and advised for
/// transparent huge pages.
///
/// Nearly every exit during a cold boot is a nested page fault: the guest
/// touching a 4 KiB page for the first time, which the host then has to fault
/// in. With huge pages one fault maps 2 MiB, so a boot takes a few hundred of
/// them instead of ~22,000. KVM can only use a 2 MiB EPT entry where the host
/// address and the guest physical address agree modulo 2 MiB, and the slots
/// start at 2 MiB-aligned guest addresses, so the host base must be aligned
/// too. Newer kernels align large anonymous mappings by themselves; this does
/// not rely on it.
///
/// The advice is best-effort. Hosts with THP set to `never` keep 4 KiB pages,
/// and the mapping is still lazy either way: nothing is resident until the
/// guest touches it.
///
/// # Safety
///
/// Returns a fresh mapping that the caller owns and must `munmap` with `size`.
unsafe fn map_guest_ram(size: usize) -> std::io::Result<*mut u8> {
    let span = size + THP_SIZE;
    let raw = libc::mmap(
        std::ptr::null_mut(),
        span,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
        -1,
        0,
    );
    if raw == libc::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }
    // Trim the over-allocation so exactly `size` bytes remain, starting at
    // the first 2 MiB boundary.
    let raw = raw as usize;
    let base = (raw + THP_SIZE - 1) & !(THP_SIZE - 1);
    let head = base - raw;
    let tail = span - head - size;
    if head > 0 {
        libc::munmap(raw as *mut libc::c_void, head);
    }
    if tail > 0 {
        libc::munmap((base + size) as *mut libc::c_void, tail);
    }
    if libc::madvise(base as *mut libc::c_void, size, libc::MADV_HUGEPAGE) != 0 {
        tracing::debug!(
            "guest RAM stays on 4 KiB pages: MADV_HUGEPAGE: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(base as *mut u8)
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

            // Allocate guest memory.
            //
            // `mmap` rather than `alloc_zeroed`, which cost most of a cold
            // start. Rust's `alloc_zeroed` only forwards to `calloc` when the
            // alignment is at most `MIN_ALIGN` (16 on x86-64); KVM needs a
            // page-aligned address, so a 4096 alignment took the other branch
            // -- `aligned_alloc` followed by `write_bytes(ptr, 0, size)`. That
            // is a full memset of the guest's RAM, and it is pure waste: the
            // kernel already guarantees anonymous pages are zero.
            //
            // Measured on this host: `calloc` of 1 GiB is 0.0ms, `memset` of
            // 1 GiB is 848ms at 1.27 GB/s, and a 1 GiB launch took 944ms of
            // which `build` and `channel` were 1.6ms together. The memset was
            // the cold start.
            //
            // It also decided the memory footprint. Writing every page
            // materialises the whole guest allocation immediately, so a 1 GiB
            // VM cost 1 GiB of host RAM before the guest had executed one
            // instruction. Mapped lazily, a VM costs what its guest has
            // actually touched.
            let guest_memory = if memory_size > 0 {
                let ptr = match map_guest_ram(memory_size as usize) {
                    Ok(ptr) => ptr,
                    Err(e) => {
                        libc::close(vm_fd);
                        return Err(Error::Memory(format!(
                            "Failed to map {memory_size} bytes of guest memory: {e}"
                        )));
                    }
                };

                // Map guest memory into KVM: one slot per RAM range, either side
                // of the hole below 4 GiB, both from this one buffer (see
                // `crate::memory::ram_ranges`). Slot 0 is the low range; slot 1
                // the high one, when there is one.
                let mut offset = 0u64;
                for (slot, (guest_phys_addr, size)) in
                    (0u32..).zip(crate::memory::ram_ranges(memory_size))
                {
                    let region = kvm_userspace_memory_region {
                        slot,
                        flags: 0,
                        guest_phys_addr,
                        memory_size: size,
                        userspace_addr: ptr as u64 + offset,
                    };
                    offset += size;

                    if let Err(e) = kvm_set_user_memory_region(vm_fd, &region) {
                        libc::munmap(ptr as *mut libc::c_void, memory_size as usize);
                        libc::close(vm_fd);
                        return Err(Error::Hypervisor(format!(
                            "Failed to set user memory region: {}",
                            e
                        )));
                    }
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
                    libc::munmap(ptr.as_ptr() as *mut libc::c_void, memory_size as usize);
                }
                libc::close(vm_fd);
                return Err(Error::Hypervisor(format!(
                    "Failed to set TSS address: {}",
                    e
                )));
            }

            // Create IRQ chip (PIC, IOAPIC, LAPIC)
            let irqchip_in_kernel = match kvm_create_irqchip(vm_fd) {
                Ok(_) => true,
                Err(e) => {
                    tracing::warn!("Failed to create IRQ chip: {}. Interrupts may not work.", e);
                    // Non-fatal: some setups work without IRQ chip
                    false
                }
            };

            // Create PIT (timer)
            let pit_config = kvm_pit_config {
                // Port 0x61 exposes channel 2's gate/output during Linux timer
                // calibration. Without the KVM stub it reaches our unmapped
                // I/O fallback (0xff), instead of the PIT's actual state.
                flags: KVM_PIT_SPEAKER_DUMMY,
                pad: [0; 15],
            };
            if let Err(e) = kvm_create_pit2(vm_fd, &pit_config) {
                tracing::warn!("Failed to create PIT: {}. Timer may not work.", e);
                // Non-fatal: some setups work without PIT
            }

            Ok(Self {
                vm_fd,
                kvm_fd,
                vcpu_count,
                memory_size,
                guest_memory,
                vcpus: RwLock::new(Vec::new()),
                run_mmap_size,
                irqchip_in_kernel,
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

        let vcpu = Arc::new(KvmVcpu::new(
            self.vm_fd,
            vcpu_id,
            self.run_mmap_size,
            self.irqchip_in_kernel,
        )?);

        // A freshly created vCPU has no CPUID configuration at all, and KVM
        // does not supply one: the guest's CPUID instruction reports a CPU with
        // no features, no vendor and no leaves. Anything that asks what it is
        // running on -- which a Linux kernel does within its first few dozen
        // instructions -- gets an answer that is not merely wrong but
        // impossible. `set_cpuid` and `get_supported_cpuid` were both written
        // and neither was ever called.
        //
        // Handing the guest the host's supported set is the same choice the
        // established VMMs make for a VM with no CPU model configured.
        vcpu.apply_supported_cpuid(self.kvm_fd, self.vcpu_count)?;

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

    /// Bytes of guest RAM registered with KVM as slot 0.
    pub fn memory_size(&self) -> u64 {
        self.memory_size
    }

    /// Discard every page of guest RAM, so it reads back as zero.
    ///
    /// `MADV_DONTNEED` on a `MAP_PRIVATE | MAP_ANONYMOUS` range frees the
    /// pages behind it, and the kernel's documented behaviour for such a range
    /// is that the next access gets a zero-fill-on-demand page. So this both
    /// zeroes the memory and *un-allocates* it: the guest's footprint on the
    /// host drops to nothing until it touches something again.
    ///
    /// That is the opposite of writing zeroes, which makes every page it
    /// touches resident. It is why this is worth doing at all -- see the
    /// measurements on snapshot restore.
    ///
    /// The mapping itself is untouched, which is the reason for `madvise`
    /// rather than an `munmap`/`mmap` pair: the address stays valid, so KVM's
    /// slot 0 registration, `guest_memory_host_addr`, and every `host_addr`
    /// the device model is holding all remain correct. KVM learns the host
    /// PTEs went away through its MMU notifier, the same path that already
    /// handles the host swapping or migrating a guest's pages.
    fn discard_guest_memory(&self) -> Result<()> {
        let Some(ptr) = self.guest_memory else {
            return Err(Error::Memory("Guest memory not allocated".into()));
        };
        if self.memory_size == 0 {
            return Ok(());
        }

        // SAFETY: `ptr` is the base of the `mmap` made in `KvmVm::new` with
        // `MAP_PRIVATE | MAP_ANONYMOUS`, and `memory_size` is the length that
        // call was given. `MADV_DONTNEED` neither unmaps nor resizes, so the
        // pointer stays valid for the lifetime of this `KvmVm`.
        let rc = unsafe {
            libc::madvise(
                ptr.as_ptr() as *mut libc::c_void,
                self.memory_size as usize,
                libc::MADV_DONTNEED,
            )
        };
        if rc != 0 {
            return Err(Error::Memory(format!(
                "discarding {} bytes of guest memory: {}",
                self.memory_size,
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Replace guest RAM with a private, copy-on-write mapping of `file`.
    ///
    /// `MAP_FIXED` over the existing allocation, so the address -- KVM's slot
    /// 0, `guest_memory_host_addr`, every pointer the device model holds --
    /// does not move. Only safe before the guest has run on this memory,
    /// which is when a restore does it.
    ///
    /// Private, so each VM's writes are its own; the pages it only reads stay
    /// in the page cache, shared by every VM mapping the same file. That is
    /// the whole density argument: N sandboxes from one template cost the
    /// template once plus what each of them writes.
    fn map_guest_memory_file(&self, file: &std::fs::File) -> Result<()> {
        use std::os::unix::io::AsRawFd;
        let Some(ptr) = self.guest_memory else {
            return Err(Error::Memory("Guest memory not allocated".into()));
        };
        let len = file
            .metadata()
            .map_err(|e| Error::Memory(format!("reading the memory image's size: {e}")))?
            .len();
        if len != self.memory_size {
            return Err(Error::Memory(format!(
                "the memory image is {len} bytes and this guest has {}",
                self.memory_size
            )));
        }
        // SAFETY: `ptr`/`memory_size` are exactly the mapping `KvmVm::new`
        // made, which `MAP_FIXED` replaces in place; `file` is open for
        // reading and at least `memory_size` long, checked above.
        let mapped = unsafe {
            libc::mmap(
                ptr.as_ptr() as *mut libc::c_void,
                self.memory_size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_FIXED | libc::MAP_NORESERVE,
                file.as_raw_fd(),
                0,
            )
        };
        if mapped != ptr.as_ptr() as *mut libc::c_void {
            return Err(Error::Memory(format!(
                "mapping the memory image over guest RAM: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Write `data` into guest physical memory at `addr`.
    ///
    /// The guest memory allocated in `KvmVm::new` is registered with KVM as
    /// the RAM ranges of `crate::memory::ram_ranges`, so a host-side write
    /// through the same allocation is visible to the guest immediately. A
    /// write may not span the hole below 4 GiB.
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
        let host = crate::memory::host_offset(self.memory_size, addr);
        let last = end
            .checked_sub(1)
            .and_then(|l| crate::memory::host_offset(self.memory_size, l));
        let Some(host) =
            host.filter(|h| data.is_empty() || last == Some(h + data.len() as u64 - 1))
        else {
            return Err(Error::Memory(format!(
                "Write at {:#x} with length {} is not within guest RAM of {:#x} bytes",
                addr,
                data.len(),
                self.memory_size
            )));
        };

        // SAFETY: The bounds check above guarantees `addr + data.len()` is
        // within the `memory_size` allocation `ptr` points at, and the source
        // and destination cannot overlap (one is host-private, one is the
        // guest allocation).
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                ptr.as_ptr().add(host as usize),
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
        self.map_memory_with_flags(slot, guest_phys_addr, memory_size, userspace_addr, 0)
    }

    /// The same, with slot flags — [`KVM_MEM_READONLY`] being the one that
    /// matters here.
    ///
    /// Nothing stops several VMs being given the same `userspace_addr`. They
    /// are all mappings of one host allocation in one process, so the pages
    /// behind them are the same physical pages: the host pays for the region
    /// once however many guests see it. That is the whole mechanism behind
    /// sharing a model's weights across a fleet of agents, and it is why the
    /// read-only flag is not optional — one writable copy shared by a thousand
    /// guests is a thousand guests able to rewrite each other's model.
    pub fn map_memory_with_flags(
        &self,
        slot: u32,
        guest_phys_addr: u64,
        memory_size: u64,
        userspace_addr: u64,
        flags: u32,
    ) -> Result<()> {
        let region = kvm_userspace_memory_region {
            slot,
            flags,
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

            // Free guest memory. Mapped with `mmap`, so unmapped with
            // `munmap` -- freeing it through the Rust allocator would be
            // undefined behaviour.
            if let Some(ptr) = self.guest_memory {
                libc::munmap(ptr.as_ptr() as *mut libc::c_void, self.memory_size as usize);
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

// ── Getting a vCPU back out of KVM_RUN ──────────────────────────────────────
//
// `KVM_RUN` blocks. A halted guest sits in `kvm_vcpu_block` and a spinning one
// never leaves the guest at all, so a vCPU thread is unreachable from the rest
// of the process: clearing a flag it will not read, or sending it a message on
// a channel it will not poll, changes nothing. Every shutdown path in this
// crate ultimately waits on those threads, which is why `VM::stop()` never
// returned for any guest.
//
// Getting one out takes three things, and it is three rather than one because
// each covers a different moment:
//
//   1. A signal whose handler is installed *without* `SA_RESTART`, so the
//      kernel returns `EINTR` from the ioctl instead of restarting it. The
//      handler itself does nothing; being delivered is the whole point.
//   2. `kvm_run->immediate_exit`, which KVM checks on the way into the guest
//      and answers with `EINTR` immediately. This covers the vCPU that has not
//      entered the ioctl yet, so a signal aimed at it would land on nothing.
//   3. A per-vCPU flag the `EINTR` arm consults before retrying. Without it,
//      `run()` retries unconditionally and walks straight back into the guest —
//      which is what it did, and why the vCPU was uninterruptible by
//      construction rather than by accident.
//
// The ordering in `kick()` is what makes the race benign: the flag and
// `immediate_exit` are both set before the signal is sent, and `run()` checks
// the flag after storing its thread id and before entering the ioctl. A kick
// that lands anywhere in that window is seen at the next entry rather than
// lost.

/// The signal used to kick a vCPU out of `KVM_RUN`.
///
/// A real-time signal, not `SIGUSR1` or `SIGUSR2`: those belong to whoever
/// embeds this crate, and a hypervisor stealing one is the kind of thing that
/// is only discovered in someone else's process. `SIGRTMIN` is what the other
/// VMMs use for the same job.
fn kick_signal() -> libc::c_int {
    libc::SIGRTMIN()
}

/// Delivered to a vCPU thread to interrupt `KVM_RUN`. Deliberately empty.
extern "C" fn kick_handler(_signum: libc::c_int) {}

/// Install [`kick_handler`], once per process.
///
/// `SA_RESTART` is left off on purpose: with it, the kernel restarts the
/// interrupted ioctl itself and userspace never learns the signal arrived.
fn install_kick_handler() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // SAFETY: `action` is a fully initialised `sigaction` with a valid
        // handler and an empty mask; `sigaction` is called once, before any
        // vCPU thread exists, and does not retain the pointer.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = kick_handler as *const () as usize;
            action.sa_flags = 0; // no SA_RESTART
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(kick_signal(), &action, std::ptr::null_mut()) != 0 {
                tracing::error!(
                    "KVM: could not install the vCPU kick handler: {}.                      Stopping a halted or spinning guest will hang.",
                    std::io::Error::last_os_error()
                );
            }
        }
    });
}

/// This thread's kernel thread id, which is what `tgkill` addresses.
///
/// `pthread_self()` would do as well via `pthread_kill`, but a raw tid is
/// storable in an atomic and comparable to what `/proc` reports, which matters
/// when the question is "which thread is stuck in the ioctl".
fn current_tid() -> libc::pid_t {
    // SAFETY: `gettid` takes no arguments and cannot fail.
    unsafe { libc::syscall(libc::SYS_gettid) as libc::pid_t }
}

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
    /// Set by [`KvmVcpu::kick`]; read by the `EINTR` arm of [`KvmVcpu::run`]
    /// before it retries the ioctl.
    kick: AtomicBool,
    /// Whether this VM's interrupt controller is in the kernel, copied from
    /// the VM that made this vCPU.
    ///
    /// Carried here rather than reached for through the VM because
    /// [`KvmVcpu::inject_interrupt`] is the one place it decides anything, and
    /// a vCPU that could not answer the question would have to refuse or guess.
    irqchip_in_kernel: bool,
    /// The thread currently inside `KVM_RUN`, or `0` when none is.
    ///
    /// Cleared on the way out so a kick can never signal a thread that has
    /// since exited — tids are reused, and the one that inherits it would be
    /// some unrelated thread of this process.
    tid: AtomicI32,
    retry_eintr: AtomicU64,
    retry_eagain: AtomicU64,
}

/// Clears the published tid however `run()` returns, including on an error
/// path, so a later kick never signals a thread that has moved on.
struct TidGuard<'a>(&'a AtomicI32);

impl Drop for TidGuard<'_> {
    fn drop(&mut self) {
        self.0.store(0, Ordering::SeqCst);
    }
}

impl KvmVcpu {
    /// This vCPU's file descriptor.
    ///
    /// Borrowed for the life of `self`, which owns it and closes it on drop.
    /// Returned as a raw descriptor because every state ioctl takes one; a
    /// caller must not close it or keep it past this `KvmVcpu`.
    pub(crate) fn fd(&self) -> RawFd {
        self.vcpu_fd
    }

    /// Create a new vCPU
    fn new(vm_fd: RawFd, vcpu_id: u32, mmap_size: usize, irqchip_in_kernel: bool) -> Result<Self> {
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

            install_kick_handler();

            Ok(Self {
                vcpu_fd,
                vcpu_id,
                run,
                mmap_size,
                irqchip_in_kernel,
                kick: AtomicBool::new(false),
                tid: AtomicI32::new(0),
                retry_eintr: AtomicU64::new(0),
                retry_eagain: AtomicU64::new(0),
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

        // Set up segment registers for real mode.
        //
        // Read back rather than built from `default()`. `kvm_sregs` carries
        // `apic_base` alongside the segments, and KVM has already put
        // 0xFEE00900 there -- the architectural base, plus the enable bit and
        // the bootstrap-processor bit. A zeroed struct written back turns the
        // vCPU's local APIC *off* and unmaps its page, so a guest that reads
        // IA32_APIC_BASE sees zero and a guest that reads 0xFEE00020 sees
        // 0xFFFFFFFF, which is unassigned MMIO.
        //
        // That is exactly how it presented: hv1, booted here, reported
        // "apic 0x0 DISABLED, application processor, id 255" on a vCPU that
        // KVM had made a bootstrap processor with an APIC. Every field this
        // function means to set is set below; the ones it does not mention are
        // now KVM's rather than zero.
        let mut sregs = kvm_sregs::default();
        kvm_get_sregs(vcpu_fd, &mut sregs)
            .map_err(|e| Error::Hypervisor(format!("Failed to read special registers: {}", e)))?;

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

    /// Run the vCPU until it exits.
    ///
    /// `EINTR` is retried, which is the KVM convention — a signal arriving
    /// while a guest runs is ordinary and means nothing to the guest. The one
    /// exception is a kick from [`KvmVcpu::kick`]: that signal was sent *to
    /// end this call*, so the retry consults the kick flag first and reports
    /// [`VmExit::Interrupted`] instead of re-entering the guest. Retrying
    /// unconditionally, as this did, is what made a halted or spinning vCPU
    /// impossible to stop.
    ///
    /// Returns [`VmExit::Interrupted`] without having entered the guest if a
    /// kick is already pending on entry.
    pub fn run(&self) -> Result<VmExit> {
        // Publish which thread to signal before the first flag check, so a
        // kick racing with entry either finds this tid or is caught by the
        // check below. Cleared on every exit path.
        self.tid.store(current_tid(), Ordering::SeqCst);
        let _clear_tid = TidGuard(&self.tid);

        // SAFETY: `self.vcpu_fd` is a valid vCPU fd created in `new()`. The
        // `kvm_run` mmap region is valid for the lifetime of this `KvmVcpu`,
        // and `immediate_exit` is a `u8` the kernel reads on entry and never
        // writes, so a plain volatile store is the whole synchronisation it
        // needs.
        unsafe {
            loop {
                if self.take_kick() {
                    return Ok(VmExit::Interrupted);
                }
                match kvm_run(self.vcpu_fd) {
                    Ok(()) => return self.convert_exit(),
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => {
                        self.retry_eintr.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // An application processor that has not been started
                    // blocks in KVM_RUN until an INIT or startup IPI reaches
                    // it, then returns EAGAIN for the VMM to run it again --
                    // which is how a guest's second CPU comes up. Treated as
                    // an error, it ended that vCPU's thread at the moment the
                    // guest woke it, and Linux gave up waiting ("CPU1 failed
                    // to report alive state") ten seconds later.
                    Err(e) if e.raw_os_error() == Some(libc::EAGAIN) => {
                        self.retry_eagain.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
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

    /// Consume a pending kick, clearing `immediate_exit` with it.
    ///
    /// Clearing matters: `immediate_exit` left set would make every subsequent
    /// `KVM_RUN` return without running the guest, which looks exactly like a
    /// guest that has stopped making progress.
    unsafe fn take_kick(&self) -> bool {
        if !self.kick.swap(false, Ordering::SeqCst) {
            return false;
        }
        std::ptr::write_volatile(&mut (*self.run.as_ptr()).immediate_exit, 0);
        tracing::debug!("KVM: vCPU {} left the guest on a kick", self.vcpu_id);
        true
    }

    /// Ask this vCPU to leave `KVM_RUN` at the next opportunity.
    ///
    /// Safe to call from any thread, including when the vCPU is not running:
    /// the request is latched and honoured at the next entry. It does not stop
    /// the vCPU — it makes the thread reachable, and what happens next is the
    /// run loop's decision.
    pub fn kick(&self) {
        // Order matters. The flag and `immediate_exit` are both visible before
        // the signal is sent, so the vCPU sees the request whether it is
        // already inside the ioctl (the signal interrupts it), about to enter
        // (`immediate_exit` returns it), or between calls (the flag catches it
        // at the next entry).
        self.kick.store(true, Ordering::SeqCst);

        // SAFETY: `self.run` points at the live `kvm_run` mmap owned by this
        // vCPU. `immediate_exit` is a single byte the kernel only reads.
        unsafe {
            std::ptr::write_volatile(&mut (*self.run.as_ptr()).immediate_exit, 1);
        }

        let tid = self.tid.load(Ordering::SeqCst);
        if tid != 0 {
            // SAFETY: `tgkill` is addressed at this process and a tid that was
            // published by a thread inside `run()` and cleared on its way out,
            // so it is either live or already gone — in which case `tgkill`
            // reports ESRCH rather than reaching an unrelated thread. Delivery
            // to a thread that has just left the ioctl is harmless: the
            // handler does nothing.
            unsafe {
                libc::syscall(libc::SYS_tgkill, libc::getpid(), tid, kick_signal());
            }
        }
    }

    /// Convert KVM exit reason to VmExit
    unsafe fn convert_exit(&self) -> Result<VmExit> {
        let run = self.run.as_ref();
        let exit_reason = run.exit_reason;

        // Opt-in diagnostics run on the owning thread after KVM_RUN returns.
        // Register ioctls and trace output change timing; disable for benchmarks.
        if tracing::enabled!(target: "hv2_core::backends::kvm::boot", tracing::Level::TRACE) {
            match self.get_regs() {
                Ok(regs) => tracing::trace!(
                    target: "hv2_core::backends::kvm::boot",
                    vcpu = self.vcpu_id,
                    exit_reason,
                    io_port = if exit_reason == KVM_EXIT_IO { Some(run.exit_data.io.port) } else { None },
                    rip = format_args!("{:#x}", regs.rip),
                    rflags = format_args!("{:#x}", regs.rflags),
                    "KVM boot exit"
                ),
                Err(error) => tracing::trace!(
                    target: "hv2_core::backends::kvm::boot",
                    vcpu = self.vcpu_id,
                    exit_reason,
                    %error,
                    "KVM boot register read failed"
                ),
            }
        }

        match exit_reason {
            KVM_EXIT_HLT => Ok(VmExit::Hlt),

            // A triple fault on x86, almost always. Do not read the registers
            // here hoping to find where it happened: on SVM, KVM resets the
            // vCPU before returning this exit, so `KVM_GET_REGS` reports the
            // reset vector (`rip=0xfff0`, `rflags=0x2`) no matter what the
            // guest was doing. Single-stepping is the way to locate one.
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
                // Where it faulted, which unlike a shutdown exit is still
                // readable here: KVM does not reset the vCPU on the way out of
                // an exception, so the registers still describe the guest.
                let rip = self.get_regs().map(|regs| regs.rip).unwrap_or_default();
                tracing::debug!(
                    "KVM: vCPU {} exception vector={} error_code={:#x} at rip={:#x}",
                    self.vcpu_id,
                    ex.exception,
                    ex.error_code,
                    rip
                );
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

    /// Inject an interrupt directly into this vCPU.
    ///
    /// # This cannot work on a VM with an in-kernel irqchip
    ///
    /// `KVM_INTERRUPT` puts a vector into a vCPU's own interrupt queue, which
    /// KVM permits only when the interrupt controller lives in *userspace*.
    /// With the controller in the kernel &mdash; which is what
    /// `KvmVm::new` creates, and what every VM this backend builds has
    /// &mdash; the kernel owns the vectoring, and the ioctl answers `ENXIO`.
    ///
    /// The two are not a preference. A vector is what a *controller* produces
    /// from a line, and which vector a line produces is the guest's choice:
    /// it programs the PIC's offset. So a host holding a line number cannot
    /// convert it to a vector, and a host holding a vector is describing a
    /// decision the in-kernel controller has already made differently.
    ///
    /// What works instead is to raise the *line* and let the controller do its
    /// job: [`HypervisorBackend::set_irq_line`],
    /// which is what `vm.rs` uses for every device in this repository.
    ///
    /// # Errors
    ///
    /// Refuses, naming the alternative, when the irqchip is in the kernel.
    /// Otherwise propagates what `KVM_INTERRUPT` says.
    pub fn inject_interrupt(&self, vector: u8) -> Result<()> {
        if self.irqchip_in_kernel {
            return Err(Error::Hypervisor(format!(
                "cannot inject vector {vector:#x} directly: this VM's interrupt controller is \
                 in the kernel, so KVM_INTERRUPT is refused (ENXIO). Raise the interrupt line \
                 instead -- set_irq_line(irq, true) -- and let the in-kernel PIC decide the \
                 vector, which is the guest's choice rather than the host's"
            )));
        }
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

    /// Size of `kvm_run.mmio.data`, which the API fixes at 8 bytes.
    const MMIO_DATA_LIMIT: usize = 8;

    /// Write the result of an MMIO read back into the shared `kvm_run` page.
    ///
    /// KVM reads the answer out of `kvm_run.mmio.data` when the vCPU is
    /// resumed, so it has to land in the mapped page rather than in the copy
    /// the exit handed out.
    pub fn set_mmio_data(&self, data: &[u8]) -> Result<()> {
        // SAFETY: `self.run` is the valid mmap'd `kvm_run` page from `KVM_RUN`,
        // and `mmio.data` is an 8-byte array inside that mapping. Written
        // through a raw pointer for the same reason `set_io_data` is: the page
        // is shared with the kernel and this method takes `&self`. The copy is
        // bounded by both the array and the caller's slice.
        unsafe {
            let run = self.run.as_ptr();
            let dst = (*run).exit_data.mmio.data.as_mut_ptr();
            let len = data.len().min(Self::MMIO_DATA_LIMIT);
            std::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
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

    /// Give this vCPU the CPUID leaves the host's KVM supports.
    ///
    /// Must happen before the first `KVM_RUN`: KVM starts a vCPU with an empty
    /// CPUID configuration, so until this runs the guest sees a CPU that
    /// reports no vendor, no features and a maximum leaf of zero.
    ///
    /// `kvm_fd` is `/dev/kvm` -- `KVM_GET_SUPPORTED_CPUID` is a system ioctl,
    /// not a vCPU one, so the answer is the same for every vCPU.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Hypervisor`] if either ioctl fails.
    ///
    /// In a VM of more than one vCPU, each is told its own APIC ID and how
    /// many share the package (leaf 1, and the x2APIC ID of leaves 0xB and
    /// 0x1F): the supported set describes whichever host CPU answered, the
    /// same for every vCPU, and a guest reading one APIC ID from all of them
    /// cannot tell them apart. A one-vCPU VM also receives consistent singleton
    /// topology, including AMD extended leaves and cache-sharing identifiers.
    pub fn apply_supported_cpuid(&self, kvm_fd: RawFd, vcpu_count: u32) -> Result<()> {
        // KVM_GET_SUPPORTED_CPUID and KVM_SET_CPUID2 take the same layout: a
        // header whose `nent` counts the entries that follow it. Filling one
        // buffer and handing it straight back avoids rebuilding the tail.
        const MAX_ENTRIES: usize = 256;
        let header_size = std::mem::size_of::<kvm_cpuid2>();
        let entry_size = std::mem::size_of::<kvm_cpuid_entry2>();
        let mut buf = vec![0u8; header_size + entry_size * MAX_ENTRIES];

        // SAFETY: `buf` holds a `kvm_cpuid2` header followed by room for
        // MAX_ENTRIES entries, which is the layout both ioctls expect. `nent`
        // is set to the capacity before the get, and KVM lowers it to the
        // number it wrote.
        unsafe {
            let header = &mut *(buf.as_mut_ptr() as *mut kvm_cpuid2);
            header.nent = MAX_ENTRIES as u32;

            kvm_get_supported_cpuid(kvm_fd, header)
                .map_err(|e| Error::Hypervisor(format!("Failed to get supported CPUID: {e}")))?;

            let entries = header.nent;
            let first = buf.as_mut_ptr().add(header_size) as *mut kvm_cpuid_entry2;
            let table = std::slice::from_raw_parts_mut(first, entries as usize);
            patch_topology(table, self.vcpu_id, vcpu_count);
            let header = &mut *(buf.as_mut_ptr() as *mut kvm_cpuid2);
            kvm_set_cpuid2(self.vcpu_fd, header).map_err(|e| {
                Error::Hypervisor(format!(
                    "Failed to set CPUID for vCPU {}: {e}",
                    self.vcpu_id
                ))
            })?;

            tracing::debug!(
                "KVM: vCPU {} configured with {entries} CPUID leaves",
                self.vcpu_id
            );
        }
        Ok(())
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

/// The `KvmVcpu` behind a `VCpu`, and the conversions between this crate's
/// snapshot types and KVM's structs.
///
/// Separate functions rather than `From` impls: `kvm_regs` and friends are
/// this module's FFI types and `VCpuSnapshot` is a public API type, so a
/// conversion between them belongs to neither and would have to live here
/// anyway.
impl KvmBackend {
    /// Look up the backend's vCPU for `vcpu`.
    fn kvm_vcpu(&self, vcpu: &VCpu) -> Result<Arc<KvmVcpu>> {
        let map = self.vcpu_map.read().unwrap_or_else(|e| e.into_inner());
        map.get(&vcpu.id())
            .cloned()
            .ok_or_else(|| Error::Hypervisor(format!("KVM vCPU {} not found", vcpu.id())))
    }
}

fn general_from(regs: &kvm_regs) -> GeneralRegisters {
    GeneralRegisters {
        rax: regs.rax,
        rbx: regs.rbx,
        rcx: regs.rcx,
        rdx: regs.rdx,
        rsi: regs.rsi,
        rdi: regs.rdi,
        rsp: regs.rsp,
        rbp: regs.rbp,
        r8: regs.r8,
        r9: regs.r9,
        r10: regs.r10,
        r11: regs.r11,
        r12: regs.r12,
        r13: regs.r13,
        r14: regs.r14,
        r15: regs.r15,
        rip: regs.rip,
        rflags: regs.rflags,
    }
}

fn general_into(general: &GeneralRegisters) -> kvm_regs {
    kvm_regs {
        rax: general.rax,
        rbx: general.rbx,
        rcx: general.rcx,
        rdx: general.rdx,
        rsi: general.rsi,
        rdi: general.rdi,
        rsp: general.rsp,
        rbp: general.rbp,
        r8: general.r8,
        r9: general.r9,
        r10: general.r10,
        r11: general.r11,
        r12: general.r12,
        r13: general.r13,
        r14: general.r14,
        r15: general.r15,
        rip: general.rip,
        rflags: general.rflags,
    }
}

fn segment_from(segment: &kvm_segment) -> Segment {
    Segment {
        base: segment.base,
        limit: segment.limit,
        selector: segment.selector,
        type_: segment.type_,
        present: segment.present,
        dpl: segment.dpl,
        db: segment.db,
        s: segment.s,
        l: segment.l,
        g: segment.g,
        avl: segment.avl,
    }
}

fn segment_into(segment: &Segment) -> kvm_segment {
    kvm_segment {
        base: segment.base,
        limit: segment.limit,
        selector: segment.selector,
        type_: segment.type_,
        present: segment.present,
        dpl: segment.dpl,
        db: segment.db,
        s: segment.s,
        l: segment.l,
        g: segment.g,
        avl: segment.avl,
        // Not captured: KVM derives `unusable` from `present`, and a segment
        // restored with both set inconsistently is rejected by KVM_SET_SREGS
        // rather than silently misbehaving.
        unusable: 0,
        padding: 0,
    }
}

fn system_from(sregs: &kvm_sregs) -> SystemRegisters {
    SystemRegisters {
        cs: segment_from(&sregs.cs),
        ds: segment_from(&sregs.ds),
        es: segment_from(&sregs.es),
        fs: segment_from(&sregs.fs),
        gs: segment_from(&sregs.gs),
        ss: segment_from(&sregs.ss),
        tr: segment_from(&sregs.tr),
        ldt: segment_from(&sregs.ldt),
        gdt: DescriptorTable {
            base: sregs.gdt.base,
            limit: sregs.gdt.limit,
        },
        idt: DescriptorTable {
            base: sregs.idt.base,
            limit: sregs.idt.limit,
        },
        cr0: sregs.cr0,
        cr2: sregs.cr2,
        cr3: sregs.cr3,
        cr4: sregs.cr4,
        cr8: sregs.cr8,
        efer: sregs.efer,
        apic_base: sregs.apic_base,
    }
}

fn system_into(system: &SystemRegisters) -> kvm_sregs {
    kvm_sregs {
        cs: segment_into(&system.cs),
        ds: segment_into(&system.ds),
        es: segment_into(&system.es),
        fs: segment_into(&system.fs),
        gs: segment_into(&system.gs),
        ss: segment_into(&system.ss),
        tr: segment_into(&system.tr),
        ldt: segment_into(&system.ldt),
        gdt: kvm_dtable {
            base: system.gdt.base,
            limit: system.gdt.limit,
            padding: [0; 3],
        },
        idt: kvm_dtable {
            base: system.idt.base,
            limit: system.idt.limit,
            padding: [0; 3],
        },
        cr0: system.cr0,
        cr2: system.cr2,
        cr3: system.cr3,
        cr4: system.cr4,
        cr8: system.cr8,
        efer: system.efer,
        apic_base: system.apic_base,
        // KVM_SET_VCPU_EVENTS restores injection state after special registers.
        // Older snapshots without that payload retain their prior omission.
        interrupt_bitmap: [0; 4],
    }
}

fn fpu_from(fpu: &kvm_fpu) -> FpuState {
    FpuState {
        fpr: fpu.fpr.iter().flatten().copied().collect(),
        xmm: fpu.xmm.iter().flatten().copied().collect(),
        fcw: fpu.fcw,
        fsw: fpu.fsw,
        ftwx: fpu.ftwx,
        last_opcode: fpu.last_opcode,
        last_ip: fpu.last_ip,
        last_dp: fpu.last_dp,
        mxcsr: fpu.mxcsr,
    }
}

fn fpu_into(state: &FpuState) -> Result<kvm_fpu> {
    // Checked rather than padded: a short buffer here means the snapshot was
    // written by something that disagrees about the register file's shape,
    // and filling the difference with zeroes would restore a vCPU with
    // half its floating-point state silently cleared.
    if state.fpr.len() != 8 * 16 || state.xmm.len() != 16 * 16 {
        return Err(Error::Hypervisor(format!(
            "FPU state is the wrong shape: {} bytes of x87 and {} of SSE, expected {} and {}",
            state.fpr.len(),
            state.xmm.len(),
            8 * 16,
            16 * 16
        )));
    }

    let mut fpu = kvm_fpu {
        fcw: state.fcw,
        fsw: state.fsw,
        ftwx: state.ftwx,
        last_opcode: state.last_opcode,
        last_ip: state.last_ip,
        last_dp: state.last_dp,
        mxcsr: state.mxcsr,
        ..Default::default()
    };
    let (fpr, _) = state.fpr.as_chunks::<16>();
    for (slot, chunk) in fpu.fpr.iter_mut().zip(fpr) {
        slot.copy_from_slice(chunk);
    }
    let (xmm, _) = state.xmm.as_chunks::<16>();
    for (slot, chunk) in fpu.xmm.iter_mut().zip(xmm) {
        slot.copy_from_slice(chunk);
    }
    Ok(fpu)
}

#[cfg(test)]
mod tests {
    /// Guest RAM must start on a 2 MiB boundary, or KVM cannot back the
    /// guest with 2 MiB EPT entries however the host pages are sized.
    #[test]
    fn guest_ram_is_huge_page_aligned_zeroed_and_writable() {
        for size in [THP_SIZE, 3 * THP_SIZE, 64 * 1024 * 1024 + 4096] {
            // SAFETY: the mapping is owned here and unmapped with `size`.
            unsafe {
                let ptr = map_guest_ram(size).unwrap();
                assert_eq!(ptr as usize % THP_SIZE, 0, "size {size}");
                assert_eq!((*ptr, *ptr.add(size - 1)), (0, 0));
                *ptr = 0xa5;
                *ptr.add(size - 1) = 0x5a;
                assert_eq!((*ptr, *ptr.add(size - 1)), (0xa5, 0x5a));
                assert_eq!(libc::munmap(ptr as *mut libc::c_void, size), 0);
            }
        }
    }

    /// Isolate deadline-timer wakeup from Linux, vsock and daemon retries.
    #[tokio::test]
    #[ignore = "requires /dev/kvm; run explicitly with --ignored --nocapture"]
    async fn restored_deadline_timer_wakes_halted_guest() {
        let source = KvmBackend::new().expect("KVM is required for this explicit test");
        source.create_vm(1, 2 * 1024 * 1024).await.unwrap();
        let vcpu = VCpu::new(0);
        let owned = source.kvm_vcpu(&vcpu).unwrap();
        let mut sregs = owned.get_sregs().unwrap();
        sregs.cs.base = 0;
        sregs.cs.selector = 0;
        sregs.idt.base = 0;
        sregs.idt.limit = 0x3ff;
        owned.set_sregs(&sregs).unwrap();
        let mut regs = owned.get_regs().unwrap();
        regs.rip = 0x400;
        regs.rsp = 0x1000;
        regs.rflags = 0x202;
        owned.set_regs(&regs).unwrap();
        let mut lapic = kvm_lapic_state::default();
        // SAFETY: owned idle vCPU descriptor and initialized ABI structures.
        unsafe {
            kvm_get_lapic(owned.fd(), &mut lapic).unwrap();
            lapic.regs[0xf0..0xf4].copy_from_slice(&0x1ffu32.to_le_bytes());
            lapic.regs[0x320..0x324].copy_from_slice(&0x40022u32.to_le_bytes());
            kvm_set_lapic(owned.fd(), &lapic).unwrap();
            kvm_set_msr(owned.fd(), 0x10, 1 << 40).unwrap();
            kvm_set_msr(owned.fd(), 0x6e0, (1 << 40) + 1_000_000_000).unwrap();
            kvm_set_mp_state(
                owned.fd(),
                &kvm_mp_state {
                    mp_state: KVM_MP_STATE_HALTED,
                },
            )
            .unwrap();
        }
        let captured = source.save_vcpu(&vcpu).await.unwrap();
        assert_eq!(captured.run_state, RunState::Halted);
        assert_eq!(
            captured
                .msrs
                .iter()
                .find(|m| m.index == 0x6e0)
                .unwrap()
                .value,
            (1 << 40) + 1_000_000_000
        );
        for (omit_deadline, delayed_entry) in [(false, false), (false, true), (true, false)] {
            let destination = KvmBackend::new().unwrap();
            destination.create_vm(1, 2 * 1024 * 1024).await.unwrap();
            let vm = destination.vm.read().unwrap().clone().unwrap();
            vm.write_guest_memory(0x22 * 4, &[0, 5, 0, 0]).unwrap();
            vm.write_guest_memory(0x400, &[0xb0, 0x11, 0xe6, 0xe9, 0xf4])
                .unwrap();
            vm.write_guest_memory(0x500, &[0xb0, 0x22, 0xe6, 0xe9, 0xcf])
                .unwrap();
            let mut state = captured.clone();
            if omit_deadline {
                state.msrs.retain(|m| m.index != 0x6e0);
            }
            destination.restore_vcpu(&vcpu, &state).await.unwrap();
            let target = destination.kvm_vcpu(&vcpu).unwrap();
            if delayed_entry {
                // Let the restored timer expire before the first KVM_RUN.
                // Read the guest counters to verify this is an expired-deadline
                // case on this host, rather than infer it from a wall delay.
                std::thread::sleep(std::time::Duration::from_secs(1));
                // SAFETY: an owned idle vCPU descriptor, before runner entry.
                let tsc = unsafe { kvm_get_msr(target.fd(), 0x10) }.unwrap();
                let captured_deadline = (1 << 40) + 1_000_000_000;
                assert!(tsc >= captured_deadline, "guest deadline must have elapsed");
                println!(
                    "KVM_DEADLINE_DELAY_EVIDENCE tsc={tsc} captured_deadline={captured_deadline}"
                );
            }
            let runner = target.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || send.send(runner.run()).unwrap());
            let result = receive.recv_timeout(std::time::Duration::from_secs(2));
            if result.is_err() {
                target.kick();
            }
            thread.join().unwrap();
            println!("KVM_DEADLINE_EVIDENCE omit_deadline={omit_deadline} delayed_entry={delayed_entry} result={result:?}");
            if omit_deadline {
                assert!(
                    result.is_err(),
                    "control must remain halted without a timer"
                );
            } else {
                match result
                    .expect("restored timer must wake within two seconds")
                    .unwrap()
                {
                    VmExit::Io {
                        port,
                        direction,
                        size,
                        data,
                    } => assert_eq!(
                        (port, direction, size, data),
                        (0xe9, IoDirection::Out, 1, 0x22)
                    ),
                    other => panic!("expected timer interrupt handler, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn event_payload_matches_the_x86_abi_and_rejects_truncation() {
        assert_eq!(std::mem::size_of::<kvm_vcpu_events>(), 64);
        assert_eq!(std::mem::offset_of!(kvm_vcpu_events, flags), 20);
        assert_eq!(std::mem::offset_of!(kvm_vcpu_events, exception_payload), 56);
        let bytes: Vec<u8> = (0..64).collect();
        let events = events_from_bytes(&bytes).unwrap();
        assert_eq!(events.interrupt.nr, 9);
        assert_eq!(events.exception.error_code, 0x07060504);
        assert_eq!(events.exception_payload, 0x3f3e3d3c3b3a3938);
        assert_eq!(event_bytes(&events), bytes);
        for size in [0, 1, 63, 65, 128] {
            assert!(events_from_bytes(&vec![0; size]).is_err());
        }
    }

    /// Explicitly run on a KVM host; no unavailable-host success fallback.
    #[tokio::test]
    #[ignore = "requires /dev/kvm; run explicitly with --ignored --nocapture"]
    async fn captured_kvm_events_restore_pending_handoffs() {
        fn irq_guest_memory(backend: &KvmBackend) {
            let vm = backend.vm.read().unwrap().clone().unwrap();
            // Real-mode vector 0x22 -> 0000:0500. The handler writes 0x22;
            // the uninterrupted main path writes 0x11 to the same I/O port.
            vm.write_guest_memory(0x22 * 4, &[0, 5, 0, 0]).unwrap();
            vm.write_guest_memory(0x400, &[0xb0, 0x11, 0xe6, 0xe9, 0xf4])
                .unwrap();
            vm.write_guest_memory(0x500, &[0xb0, 0x22, 0xe6, 0xe9, 0xcf])
                .unwrap();
        }

        let mut cases = Vec::new();
        let mut irq = kvm_vcpu_events::default();
        irq.interrupt.injected = 1;
        irq.interrupt.nr = 0x22;
        cases.push(("interrupt", irq));
        let mut nmi = kvm_vcpu_events::default();
        nmi.nmi.pending = 1;
        nmi.nmi.masked = 1;
        nmi.flags = KVM_VCPUEVENT_VALID_NMI_PENDING;
        cases.push(("nmi", nmi));
        let mut shadow = kvm_vcpu_events::default();
        shadow.interrupt.shadow = 1;
        shadow.flags = 4; // KVM_VCPUEVENT_VALID_SHADOW
        cases.push(("shadow", shadow));
        let mut exception = kvm_vcpu_events::default();
        exception.exception.injected = 1;
        exception.exception.nr = 13;
        exception.exception.has_error_code = 1;
        exception.exception.error_code = 0x1234;
        cases.push(("exception", exception));

        for (name, events) in cases {
            let source = KvmBackend::new().expect("KVM must be available for this explicit test");
            source.create_vm(1, 2 * 1024 * 1024).await.unwrap();
            let vcpu = VCpu::new(0);
            let owned = source.kvm_vcpu(&vcpu).unwrap();
            if name == "interrupt" {
                irq_guest_memory(&source);
                let mut sregs = owned.get_sregs().unwrap();
                sregs.cs.base = 0;
                sregs.cs.selector = 0;
                sregs.idt.base = 0;
                sregs.idt.limit = 0x3ff;
                owned.set_sregs(&sregs).unwrap();
                let mut regs = owned.get_regs().unwrap();
                regs.rip = 0x400;
                regs.rsp = 0x1000;
                regs.rflags = 0x202;
                owned.set_regs(&regs).unwrap();
            }
            // SAFETY: an owned idle vCPU fd and initialized event payload.
            unsafe { kvm_set_vcpu_events(owned.fd(), &events) }.unwrap();
            let before = owned.get_vcpu_events().unwrap();
            let captured = source.save_vcpu(&vcpu).await.unwrap();
            assert_eq!(captured.kvm_events, event_bytes(&before));

            let destination = KvmBackend::new().unwrap();
            destination.create_vm(1, 2 * 1024 * 1024).await.unwrap();
            if name == "interrupt" {
                irq_guest_memory(&destination);
            }
            destination.restore_vcpu(&vcpu, &captured).await.unwrap();
            let after = destination
                .kvm_vcpu(&vcpu)
                .unwrap()
                .get_vcpu_events()
                .unwrap();
            assert_eq!(event_bytes(&after), event_bytes(&before), "{name}");
            let mut evidence = serde_json::json!({
                "case": name, "captured": event_bytes(&before), "restored": event_bytes(&after)
            });
            if name == "interrupt" {
                let legacy = KvmBackend::new().unwrap();
                legacy.create_vm(1, 2 * 1024 * 1024).await.unwrap();
                irq_guest_memory(&legacy);
                let mut without_events = captured.clone();
                without_events.kvm_events.clear();
                legacy.restore_vcpu(&vcpu, &without_events).await.unwrap();
                let omitted = legacy.kvm_vcpu(&vcpu).unwrap().get_vcpu_events().unwrap();
                assert_eq!(before.interrupt.injected, 1);
                assert_eq!(omitted.interrupt.injected, 0);
                evidence["legacy_restored"] = serde_json::json!(event_bytes(&omitted));
                for (backend, expected) in [(&destination, 0x22), (&legacy, 0x11)] {
                    match backend.run_vcpu(&vcpu).await.unwrap() {
                        VmExit::Io {
                            port,
                            direction,
                            size,
                            data,
                        } => {
                            assert_eq!(
                                (port, direction, size, data),
                                (0xe9, IoDirection::Out, 1, expected)
                            );
                        }
                        other => panic!("expected guest handler/main I/O marker, got {other:?}"),
                    }
                }
                evidence["restored_guest_marker"] = serde_json::json!(0x22);
                evidence["legacy_guest_marker"] = serde_json::json!(0x11);
            }
            println!("KVM_EVENTS_EVIDENCE {evidence}");
        }
    }

    #[test]
    fn interrupt_events_require_validity_flags_for_optional_fields() {
        let mut events = kvm_vcpu_events::default();
        events.interrupt.shadow = 3;
        events.interrupt.injected = 1;
        events.interrupt.nr = 0x30;
        events.nmi.pending = 1;
        events.exception.pending = 1;
        let invalid = interrupt_state(events);
        assert_eq!(invalid.shadow, None);
        assert_eq!(invalid.nmi_pending, None);
        assert_eq!(invalid.exception_pending, None);
        assert_eq!((invalid.injected, invalid.vector), (1, 0x30));
        events.flags = 5;
        let valid = interrupt_state(events);
        assert_eq!(valid.shadow, Some(3));
        assert_eq!(valid.nmi_pending, Some(1));
        assert_eq!(valid.exception_pending, None);
        events.flags |= 0x10;
        assert_eq!(interrupt_state(events).exception_pending, Some(1));
    }
    #[test]
    fn singleton_topology_preserves_cache_geometry_and_level_terminators() {
        let mut entries = vec![
            kvm_cpuid_entry2 {
                function: 1,
                ebx: 0x1f200800,
                edx: 1 << 28,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 0xb,
                eax: 5,
                ebx: 32,
                ecx: 0x201,
                edx: 31,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 0xb,
                index: 2,
                ecx: 2,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 4,
                eax: 0xffffc121,
                ebx: 0x12345678,
                ecx: 4095,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 0x80000008,
                ecx: 0x1234501f,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 0x8000001e,
                eax: 31,
                ebx: 0x107,
                ecx: 0x102,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 0x8000001d,
                eax: 0x03ffc121,
                ebx: 0x87654321,
                ..Default::default()
            },
            kvm_cpuid_entry2 {
                function: 0x80000026,
                eax: 5,
                ebx: 32,
                ecx: 0x401,
                edx: 31,
                ..Default::default()
            },
        ];
        patch_topology(&mut entries, 0, 1);
        assert_eq!(entries[0].ebx, 0x00010800);
        assert_eq!(entries[0].edx & (1 << 28), 0);
        assert_eq!(
            (
                entries[1].eax,
                entries[1].ebx,
                entries[1].ecx,
                entries[1].edx
            ),
            (0, 1, 0x201, 0)
        );
        assert_eq!((entries[2].eax, entries[2].ebx, entries[2].ecx), (0, 0, 2));
        assert_eq!(
            (entries[3].eax, entries[3].ebx, entries[3].ecx),
            (0x121, 0x12345678, 4095)
        );
        assert_eq!(entries[4].ecx, 0x12340000);
        assert_eq!((entries[5].eax, entries[5].ebx, entries[5].ecx), (0, 0, 0));
        assert_eq!((entries[6].eax, entries[6].ebx), (0x121, 0x87654321));
        assert_eq!(
            (
                entries[7].eax,
                entries[7].ebx,
                entries[7].ecx,
                entries[7].edx
            ),
            (0, 1, 0x401, 0)
        );
    }
    use super::*;
    use crate::hypervisor::HypervisorBackend;

    /// A backend hands out one VM. The second request must fail loudly:
    /// before this check it succeeded and quietly aliased the first, because
    /// both VMs' vCPU 0 land on the same `vcpu_map` key and `load_boot` then
    /// resolved whichever VM was created last.
    #[tokio::test]
    async fn a_second_vm_on_one_backend_is_refused() {
        let Ok(backend) = KvmBackend::new() else {
            eprintln!("KVM not available — skipping");
            return;
        };
        if backend.create_vm(1, 1024 * 1024).await.is_err() {
            eprintln!("KVM VM creation unavailable (check /dev/kvm permissions) — skipping");
            return;
        }

        let err = match backend.create_vm(1, 1024 * 1024).await {
            Ok(_) => panic!("the second VM must be refused, not aliased"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("already owns a VM"),
            "the error should say why one backend owns one VM, got: {err}"
        );
    }

    /// `shutdown` releases the VM, so the backend can be reused.
    #[tokio::test]
    async fn shutdown_frees_the_backend_to_own_a_vm_again() {
        let Ok(mut backend) = KvmBackend::new() else {
            eprintln!("KVM not available — skipping");
            return;
        };
        if backend.create_vm(1, 1024 * 1024).await.is_err() {
            eprintln!("KVM VM creation unavailable (check /dev/kvm permissions) — skipping");
            return;
        }

        backend.shutdown().await.expect("shutdown should succeed");
        backend
            .create_vm(1, 1024 * 1024)
            .await
            .expect("after shutdown the backend owns no VM, so this must succeed");
    }

    /// `reset_guest_memory_to_zero` must actually zero the memory, not just
    /// say so.
    ///
    /// Snapshot restore uses the return value to *skip* writing absent pages
    /// entirely, so a backend that answered `true` without doing the work
    /// would leave the previous guest's bytes in place and the next guest
    /// running on top of them. Writing a pattern first is what makes this a
    /// test rather than a call that returns `Ok`.
    #[tokio::test]
    async fn resetting_guest_memory_really_zeroes_it() {
        let Ok(backend) = KvmBackend::new() else {
            eprintln!("KVM not available — skipping");
            return;
        };
        // Two pages, so the check spans a page boundary rather than sitting
        // inside the first one.
        let size = 8 * 1024 * 1024;
        if backend.create_vm(1, size).await.is_err() {
            eprintln!("KVM VM creation unavailable (check /dev/kvm permissions) — skipping");
            return;
        }

        let vm = backend
            .vm
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .expect("the VM was just created");

        // Dirty the first and last page: a reset that only handled the start
        // of the mapping would still pass on the first alone.
        let pattern = [0xa5u8; 4096];
        vm.write_guest_memory(0, &pattern).expect("write low page");
        vm.write_guest_memory(size - 4096, &pattern)
            .expect("write high page");

        let host = backend
            .guest_memory_host_addr()
            .expect("KVM owns this guest's RAM");
        // SAFETY: `host` is the base of the backend's guest mapping, which is
        // `size` bytes and outlives this borrow.
        let before = unsafe { std::slice::from_raw_parts(host as *const u8, size as usize) };
        assert!(
            before[..4096].iter().all(|b| *b == 0xa5),
            "low page written"
        );
        assert!(
            before[size as usize - 4096..].iter().all(|b| *b == 0xa5),
            "high page written"
        );

        assert!(
            backend
                .reset_guest_memory_to_zero()
                .expect("reset should not fail on a VM that owns memory"),
            "the KVM backend can do this, so it must not answer false"
        );

        // SAFETY: as above; `madvise` does not unmap or resize.
        let after = unsafe { std::slice::from_raw_parts(host as *const u8, size as usize) };
        assert!(
            after.iter().all(|b| *b == 0),
            "every byte must read zero after a reset that claimed success"
        );
    }

    /// A backend with no VM owns no memory, and must say so rather than
    /// claiming it zeroed something that does not exist.
    #[tokio::test]
    async fn resetting_guest_memory_without_a_vm_reports_false() {
        let Ok(backend) = KvmBackend::new() else {
            eprintln!("KVM not available — skipping");
            return;
        };
        assert!(
            !backend
                .reset_guest_memory_to_zero()
                .expect("no VM is not an error"),
            "with no VM there is no memory to have zeroed"
        );
    }
}
