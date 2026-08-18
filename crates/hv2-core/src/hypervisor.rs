//! Hypervisor backend abstraction

use crate::{Error, IoDirection, Result, VCpu, VmExit};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Arc;

/// Hypervisor platform
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypervisorPlatform {
    /// KVM (Linux)
    Kvm,
    /// Windows Hypervisor Platform (Windows)
    Whpx,
    /// Hypervisor Framework (macOS)
    Hvf,
    /// Software emulation (fallback)
    Tcg,
}

impl std::fmt::Display for HypervisorPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kvm => write!(f, "Kvm"),
            Self::Whpx => write!(f, "Whpx"),
            Self::Hvf => write!(f, "Hvf"),
            Self::Tcg => write!(f, "Tcg"),
        }
    }
}

impl HypervisorPlatform {
    /// Detect the best available hypervisor platform
    pub fn detect() -> Self {
        #[cfg(target_os = "linux")]
        {
            if Self::is_kvm_available() {
                return Self::Kvm;
            }
        }

        #[cfg(target_os = "windows")]
        {
            if Self::is_whpx_available() {
                return Self::Whpx;
            }
        }

        // x86_64 only -- the HVF backend cannot be built on Apple Silicon,
        // so reporting Hvf there would name a platform we then fall back
        // out of. Apple Silicon detects as Tcg, which is what it gets.
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            if Self::is_hvf_available() {
                return Self::Hvf;
            }
        }

        // Fallback to TCG
        Self::Tcg
    }

    #[cfg(target_os = "linux")]
    fn is_kvm_available() -> bool {
        std::path::Path::new("/dev/kvm").exists()
    }

    #[cfg(target_os = "windows")]
    fn is_whpx_available() -> bool {
        // Check if WHPX is available by querying the capability
        // Try to load WHvGetCapability from WinHvPlatform.dll
        use std::ffi::c_void;

        #[link(name = "WinHvPlatform")]
        extern "system" {
            fn WHvGetCapability(
                capability_code: u32,
                capability_buffer: *mut c_void,
                capability_buffer_size: u32,
                written_size: *mut u32,
            ) -> i32;
        }

        // WHvCapabilityCodeHypervisorPresent = 0x00000000
        let mut result: u32 = 0;
        let mut written: u32 = 0;

        // SAFETY: Passing valid pointers and sizes to the WHPX capability query API.
        // `result` and `written` are local stack variables with correct size.
        let hr = unsafe {
            WHvGetCapability(
                0,
                &mut result as *mut _ as *mut c_void,
                std::mem::size_of::<u32>() as u32,
                &mut written,
            )
        };

        hr == 0 && result != 0
    }

    // Only `detect` calls this, and only on x86_64 macOS, so gating it the same
    // way keeps it from becoming dead code on Apple Silicon.
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    fn is_hvf_available() -> bool {
        // Check if HVF is available by calling hv_vm_create and immediately destroying
        // On modern macOS (10.15+), HVF is generally available on Apple Silicon
        // and Intel Macs with hardware virtualization support

        #[link(name = "Hypervisor", kind = "framework")]
        extern "C" {
            fn hv_vm_create(flags: u64) -> i32;
            fn hv_vm_destroy() -> i32;
        }

        // Try to create and destroy a VM to check availability
        // HV_SUCCESS = 0
        // SAFETY: FFI call to Hypervisor.framework hv_vm_create to probe HVF availability.
        // Passing flags=0 for default VM; if creation succeeds, we destroy it immediately below.
        let create_result = unsafe { hv_vm_create(0) };
        if create_result == 0 {
            // SAFETY: FFI call to hv_vm_destroy; the VM was just successfully created by hv_vm_create above.
            unsafe { hv_vm_destroy() };
            true
        } else {
            false
        }
    }
}

/// Hypervisor capabilities
#[derive(Debug, Clone, Default)]
pub struct HypervisorCapabilities {
    pub max_vcpus: u32,
    pub max_memory: u64,
    pub supports_nested_virt: bool,
    pub supports_apic: bool,
    pub supports_x2apic: bool,
    pub supports_iommu: bool,
    pub supports_gpu_passthrough: bool,
}

/// Hypervisor backend trait
#[async_trait]
pub trait HypervisorBackend: Send + Sync {
    /// Get the platform type
    fn platform(&self) -> HypervisorPlatform;

    /// Get hypervisor capabilities
    fn capabilities(&self) -> HypervisorCapabilities;

    /// Initialize the hypervisor
    async fn init(&mut self) -> Result<()>;

    /// Create a VM instance
    async fn create_vm(&self, vcpu_count: u32, memory_size: u64) -> Result<HypervisorVm>;

    /// Run a vCPU until it exits
    ///
    /// This is the core execution method. It runs the vCPU until a VM exit occurs,
    /// then returns the exit reason for the hypervisor to handle.
    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit>;

    /// Inject an interrupt into a vCPU
    ///
    /// This queues an interrupt to be delivered to the guest when interrupts are enabled.
    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()>;

    /// Inject an exception into a vCPU
    ///
    /// Delivers a CPU exception (vector 0-31) to the guest, optionally with an
    /// error code. Used for re-injecting exceptions like #GP, #PF, #UD, etc.
    ///
    /// The default implementation falls back to `inject_interrupt` without
    /// the error code, which works for simple exception types.
    async fn inject_exception(
        &self,
        vcpu: &VCpu,
        vector: u8,
        error_code: Option<u32>,
    ) -> Result<()> {
        let _ = error_code;
        self.inject_interrupt(vcpu, vector).await
    }

    /// Load a boot source into this backend's guest memory and prepare `vcpu`
    /// to execute it.
    ///
    /// Called once per VM, after [`Self::create_vm`], for a VM configured with
    /// a boot source. Implementations write the images into guest physical
    /// memory and set up whatever architectural state the protocol requires
    /// (GDT, page tables, CPU mode, and the protocol's entry registers).
    ///
    /// The caller sets the generic [`VCpu`] instruction pointer to
    /// [`crate::boot::source::LoadedBoot::entry_point`] afterwards, so a backend whose vCPU state
    /// lives entirely in the shared `VCpu` need only write memory.
    ///
    /// The default implementation reports [`Error::NotSupported`]: a backend
    /// that cannot boot a guest says so rather than silently starting a VM
    /// that will never execute.
    async fn load_boot(&self, vcpu: &VCpu, boot: &crate::boot::source::LoadedBoot) -> Result<()> {
        let _ = vcpu;
        Err(Error::NotSupported(format!(
            "{} backend cannot boot a {} guest",
            self.platform(),
            boot.protocol()
        )))
    }

    /// Shutdown the hypervisor
    async fn shutdown(&mut self) -> Result<()>;

    /// Set the result of an I/O IN operation
    ///
    /// After handling a `VmExit::Io` with `IoDirection::In`, the hypervisor must
    /// write the read data back into the guest's EAX register (masked by size)
    /// before resuming execution.
    ///
    /// The default implementation is a no-op for backends that handle this
    /// internally or don't yet support it.
    async fn set_io_result(&self, _vcpu: &VCpu, _data: u32, _size: u8) -> Result<()> {
        Ok(())
    }

    /// Allow downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Trait for backend-specific memory mapping.
///
/// Backends implement this to provide their own guest memory mapping logic.
/// The TCG backend uses a flat `Vec<u8>` while hardware backends (KVM, WHPX)
/// use kernel ioctls.
pub(crate) trait MemoryMapper: Send + Sync {
    /// Write `data` into guest physical memory starting at `guest_addr`.
    fn map_region(&self, guest_addr: u64, data: &[u8]) -> Result<()>;
}

/// Hypervisor VM instance
pub struct HypervisorVm {
    pub(crate) platform: HypervisorPlatform,
    pub(crate) vcpu_count: u32,
    pub(crate) memory_size: u64,
    /// Optional backend-provided memory mapper.
    memory_mapper: Option<Arc<dyn MemoryMapper>>,
}

impl HypervisorVm {
    /// Create a new hypervisor VM (without memory mapper).
    pub fn new(platform: HypervisorPlatform, vcpu_count: u32, memory_size: u64) -> Self {
        Self {
            platform,
            vcpu_count,
            memory_size,
            memory_mapper: None,
        }
    }

    /// Create a new hypervisor VM with a backend-provided memory mapper.
    pub fn with_mapper(
        platform: HypervisorPlatform,
        vcpu_count: u32,
        memory_size: u64,
        mapper: Arc<dyn MemoryMapper>,
    ) -> Self {
        Self {
            platform,
            vcpu_count,
            memory_size,
            memory_mapper: Some(mapper),
        }
    }

    /// Get the platform
    pub fn platform(&self) -> HypervisorPlatform {
        self.platform
    }

    /// Map data into guest physical memory.
    ///
    /// Routes through the backend-specific memory mapper when available.
    /// For hardware backends (KVM, WHPX, HVF) that don't set a mapper,
    /// use the backend-specific VM type directly (e.g., `KvmVm::map_memory`).
    pub fn map_memory(&self, guest_addr: u64, data: &[u8]) -> Result<()> {
        match &self.memory_mapper {
            Some(mapper) => mapper.map_region(guest_addr, data),
            None => Err(Error::VM(
                "No memory mapper configured — use backend-specific VM for memory mapping".into(),
            )),
        }
    }
}

/// `MemoryMapper` implementation for the TCG backend, backed by a flat `Vec<u8>`.
struct TcgMemoryMapper(Arc<parking_lot::Mutex<Vec<u8>>>);

impl MemoryMapper for TcgMemoryMapper {
    fn map_region(&self, guest_addr: u64, data: &[u8]) -> Result<()> {
        let mut mem = self.0.lock();
        let end = guest_addr as usize + data.len();
        if mem.len() < end {
            mem.resize(end, 0);
        }
        mem[guest_addr as usize..end].copy_from_slice(data);
        Ok(())
    }
}

/// TCG (software emulation) backend
pub struct TcgBackend {
    capabilities: HypervisorCapabilities,
    /// Flat guest physical memory
    guest_memory: Arc<parking_lot::Mutex<Vec<u8>>>,
    /// Pending interrupt vectors waiting for delivery
    pending_interrupts: Arc<parking_lot::Mutex<VecDeque<u8>>>,
}

impl TcgBackend {
    pub fn new() -> Self {
        Self {
            capabilities: HypervisorCapabilities {
                max_vcpus: 256,
                max_memory: 1024 * 1024 * 1024 * 1024, // 1TB
                supports_nested_virt: false,
                supports_apic: true,
                supports_x2apic: false,
                supports_iommu: false,
                supports_gpu_passthrough: false,
            },
            guest_memory: Arc::new(parking_lot::Mutex::new(Vec::new())),
            pending_interrupts: Arc::new(parking_lot::Mutex::new(VecDeque::new())),
        }
    }

    /// Map guest memory by copying `data` into the flat address space at `guest_addr`.
    pub fn map_memory(&self, guest_addr: u64, data: &[u8]) {
        let mut mem = self.guest_memory.lock();
        let end = guest_addr as usize + data.len();
        if mem.len() < end {
            mem.resize(end, 0);
        }
        mem[guest_addr as usize..end].copy_from_slice(data);
    }

    /// Read a byte from guest memory, returning `None` if out of range.
    fn fetch_byte(&self, addr: u64) -> Option<u8> {
        let mem = self.guest_memory.lock();
        mem.get(addr as usize).copied()
    }
}

impl Default for TcgBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HypervisorBackend for TcgBackend {
    fn platform(&self) -> HypervisorPlatform {
        HypervisorPlatform::Tcg
    }

    fn capabilities(&self) -> HypervisorCapabilities {
        self.capabilities.clone()
    }

    async fn init(&mut self) -> Result<()> {
        tracing::warn!(
            "TCG backend (software emulation) is a non-functional fallback. \
             Guest code will NOT execute. Use KVM, WHPX, or HVF for real virtualization."
        );
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

        Ok(HypervisorVm::with_mapper(
            HypervisorPlatform::Tcg,
            vcpu_count,
            memory_size,
            Arc::new(TcgMemoryMapper(self.guest_memory.clone())),
        ))
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        // Check for pending interrupts first (when IF flag is set)
        {
            let regs = vcpu.registers();
            let if_set = (regs.rflags & (1 << 9)) != 0;
            if if_set {
                let mut q = self.pending_interrupts.lock();
                if let Some(vector) = q.pop_front() {
                    tracing::debug!("TCG: delivering interrupt {} to vCPU {}", vector, vcpu.id());
                    return Ok(VmExit::InterruptWindow);
                }
            }
        }

        let regs = vcpu.registers();
        let rip = regs.rip;

        // Fetch opcode byte from guest memory
        let opcode = match self.fetch_byte(rip) {
            Some(b) => b,
            None => {
                tracing::warn!("TCG: vCPU {} fetch fault at RIP={:#x}", vcpu.id(), rip);
                return Ok(VmExit::Shutdown);
            }
        };

        tracing::debug!(
            "TCG: vCPU {} executing opcode {:#04x} at RIP={:#x}",
            vcpu.id(),
            opcode,
            rip
        );

        match opcode {
            // NOP
            0x90 => {
                let mut r = regs;
                r.rip = rip.wrapping_add(1);
                vcpu.set_registers(r);
                Ok(VmExit::Hlt) // yield after each instruction
            }
            // HLT
            0xF4 => {
                let mut r = regs;
                r.rip = rip.wrapping_add(1);
                vcpu.set_registers(r);
                Ok(VmExit::Hlt)
            }
            // CLI — clear interrupt flag
            0xFA => {
                let mut r = regs;
                r.rflags &= !(1 << 9);
                r.rip = rip.wrapping_add(1);
                vcpu.set_registers(r);
                Ok(VmExit::Hlt)
            }
            // STI — set interrupt flag
            0xFB => {
                let mut r = regs;
                r.rflags |= 1 << 9;
                r.rip = rip.wrapping_add(1);
                vcpu.set_registers(r);
                Ok(VmExit::Hlt)
            }
            // IN AL, imm8
            0xE4 => {
                let port = self.fetch_byte(rip + 1).unwrap_or(0);
                let mut r = regs;
                r.rip = rip.wrapping_add(2);
                vcpu.set_registers(r);
                Ok(VmExit::Io {
                    port: port as u16,
                    direction: IoDirection::In,
                    size: 1,
                    data: 0,
                })
            }
            // OUT imm8, AL
            0xE6 => {
                let port = self.fetch_byte(rip + 1).unwrap_or(0);
                let al = (regs.rax & 0xFF) as u32;
                let mut r = regs;
                r.rip = rip.wrapping_add(2);
                vcpu.set_registers(r);
                Ok(VmExit::Io {
                    port: port as u16,
                    direction: IoDirection::Out,
                    size: 1,
                    data: al,
                })
            }
            // IN AL, DX
            0xEC => {
                let dx = (regs.rdx & 0xFFFF) as u16;
                let mut r = regs;
                r.rip = rip.wrapping_add(1);
                vcpu.set_registers(r);
                Ok(VmExit::Io {
                    port: dx,
                    direction: IoDirection::In,
                    size: 1,
                    data: 0,
                })
            }
            // OUT DX, AL
            0xEE => {
                let dx = (regs.rdx & 0xFFFF) as u16;
                let al = (regs.rax & 0xFF) as u32;
                let mut r = regs;
                r.rip = rip.wrapping_add(1);
                vcpu.set_registers(r);
                Ok(VmExit::Io {
                    port: dx,
                    direction: IoDirection::Out,
                    size: 1,
                    data: al,
                })
            }
            // Unrecognised — #UD exception
            _ => Ok(VmExit::Exception {
                vector: 6,
                error_code: Some(0),
            }),
        }
    }

    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
        tracing::debug!("TCG: Queuing interrupt {} for vCPU {}", vector, vcpu.id());
        self.pending_interrupts.lock().push_back(vector);
        Ok(())
    }

    async fn load_boot(&self, vcpu: &VCpu, boot: &crate::boot::source::LoadedBoot) -> Result<()> {
        // TCG has no architectural state of its own — it reads and writes the
        // shared `VCpu` — so loading the images into the flat address space is
        // the whole job. The caller sets RIP to the entry point.
        for (addr, data) in boot.memory_regions()? {
            self.map_memory(addr, &data);
        }

        tracing::warn!(
            "TCG: loaded a {} boot image for vCPU {} into emulated memory, but the TCG \
             interpreter executes only a handful of opcodes — this guest will not run. \
             Use KVM, WHPX, or HVF.",
            boot.protocol(),
            vcpu.id()
        );

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down TCG backend");
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Create the best available hypervisor backend.
pub fn create_backend() -> Result<Box<dyn HypervisorBackend>> {
    let platform = HypervisorPlatform::detect();

    tracing::info!("Detected hypervisor platform: {:?}", platform);

    match platform {
        #[cfg(target_os = "linux")]
        HypervisorPlatform::Kvm => {
            use crate::backends::kvm::KvmBackend;
            Ok(Box::new(KvmBackend::new()?))
        }
        #[cfg(target_os = "windows")]
        HypervisorPlatform::Whpx => {
            use crate::backends::whpx::WhpxBackend;
            Ok(Box::new(WhpxBackend::new()?))
        }
        // x86_64 only: the HVF backend uses Hypervisor.framework's VMX API,
        // which does not exist on Apple Silicon. On aarch64 macOS this arm is
        // absent and `Hvf` falls through to TCG below.
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        HypervisorPlatform::Hvf => {
            use crate::backends::hvf::HvfBackend;
            Ok(Box::new(HvfBackend::new()?))
        }
        HypervisorPlatform::Tcg | _ => Ok(Box::new(TcgBackend::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_display() {
        assert_eq!(HypervisorPlatform::Kvm.to_string(), "Kvm");
        assert_eq!(HypervisorPlatform::Whpx.to_string(), "Whpx");
        assert_eq!(HypervisorPlatform::Hvf.to_string(), "Hvf");
        assert_eq!(HypervisorPlatform::Tcg.to_string(), "Tcg");
    }

    #[test]
    fn test_platform_detect_returns_value() {
        // detect() should always return a valid platform (at minimum Tcg fallback)
        let platform = HypervisorPlatform::detect();
        let valid = matches!(
            platform,
            HypervisorPlatform::Kvm
                | HypervisorPlatform::Whpx
                | HypervisorPlatform::Hvf
                | HypervisorPlatform::Tcg
        );
        assert!(
            valid,
            "detect() returned unexpected platform: {:?}",
            platform
        );
    }

    #[test]
    fn test_tcg_backend_defaults() {
        let backend = TcgBackend::new();
        assert_eq!(backend.platform(), HypervisorPlatform::Tcg);
        let caps = backend.capabilities();
        assert!(caps.max_vcpus > 0);
        assert!(caps.max_memory > 0);
        assert!(caps.supports_apic);
        assert!(!caps.supports_nested_virt);
    }

    #[test]
    fn test_hypervisor_vm_new() {
        let vm = HypervisorVm::new(HypervisorPlatform::Tcg, 4, 8192);
        assert_eq!(vm.platform(), HypervisorPlatform::Tcg);
        assert_eq!(vm.vcpu_count, 4);
        assert_eq!(vm.memory_size, 8192);
    }

    #[tokio::test]
    async fn test_tcg_create_vm() {
        let backend = TcgBackend::new();
        let vm = backend
            .create_vm(2, 1024)
            .await
            .expect("create_vm should succeed");
        assert_eq!(vm.platform(), HypervisorPlatform::Tcg);
    }

    #[tokio::test]
    async fn test_tcg_create_vm_too_many_vcpus() {
        let backend = TcgBackend::new();
        let result = backend.create_vm(999_999, 1024).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_capabilities_default() {
        let caps = HypervisorCapabilities::default();
        assert_eq!(caps.max_vcpus, 0);
        assert_eq!(caps.max_memory, 0);
    }

    #[test]
    fn test_platform_equality() {
        assert_eq!(HypervisorPlatform::Kvm, HypervisorPlatform::Kvm);
        assert_ne!(HypervisorPlatform::Kvm, HypervisorPlatform::Whpx);
    }

    #[tokio::test]
    async fn test_tcg_run_vcpu_hlt() {
        let backend = TcgBackend::new();
        // Load a HLT instruction (0xF4) at address 0
        backend.map_memory(0, &[0xF4]);
        let vcpu = VCpu::new(0);
        // set RIP to 0
        let mut regs = vcpu.registers();
        regs.rip = 0;
        vcpu.set_registers(regs);

        let exit = backend.run_vcpu(&vcpu).await.unwrap();
        assert!(matches!(exit, VmExit::Hlt));
        // RIP should advance past the instruction
        assert_eq!(vcpu.registers().rip, 1);
    }

    #[tokio::test]
    async fn test_tcg_run_vcpu_io_out() {
        let backend = TcgBackend::new();
        // OUT 0x80, AL  → 0xE6 0x80
        backend.map_memory(0, &[0xE6, 0x80]);
        let vcpu = VCpu::new(0);
        let mut regs = vcpu.registers();
        regs.rip = 0;
        regs.rax = 0x42;
        vcpu.set_registers(regs);

        let exit = backend.run_vcpu(&vcpu).await.unwrap();
        match exit {
            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                assert_eq!(port, 0x80);
                assert_eq!(direction, IoDirection::Out);
                assert_eq!(size, 1);
                assert_eq!(data, 0x42);
            }
            other => panic!("expected Io exit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_tcg_run_vcpu_cli_sti() {
        let backend = TcgBackend::new();
        // CLI (0xFA), STI (0xFB)
        backend.map_memory(0, &[0xFA, 0xFB]);
        let vcpu = VCpu::new(0);
        let mut regs = vcpu.registers();
        regs.rip = 0;
        regs.rflags = 1 << 9; // IF set
        vcpu.set_registers(regs);

        // CLI should clear IF
        backend.run_vcpu(&vcpu).await.unwrap();
        assert_eq!(vcpu.registers().rflags & (1 << 9), 0);

        // STI should set IF
        backend.run_vcpu(&vcpu).await.unwrap();
        assert_ne!(vcpu.registers().rflags & (1 << 9), 0);
    }

    #[tokio::test]
    async fn test_tcg_inject_interrupt() {
        let backend = TcgBackend::new();
        // NOP at address 0 so run_vcpu has something to fetch
        backend.map_memory(0, &[0x90]);
        let vcpu = VCpu::new(0);
        let mut regs = vcpu.registers();
        regs.rip = 0;
        regs.rflags = 1 << 9; // IF set
        vcpu.set_registers(regs);

        // Inject interrupt
        backend.inject_interrupt(&vcpu, 0x20).await.unwrap();
        assert_eq!(backend.pending_interrupts.lock().len(), 1);

        // run_vcpu should deliver the interrupt (InterruptWindow exit)
        let exit = backend.run_vcpu(&vcpu).await.unwrap();
        assert!(matches!(exit, VmExit::InterruptWindow));
        // Queue should now be empty
        assert!(backend.pending_interrupts.lock().is_empty());
    }

    #[tokio::test]
    async fn test_tcg_unknown_opcode_exception() {
        let backend = TcgBackend::new();
        // 0x0F 0x0B = UD2 — but our TCG doesn't know 0x0F prefix, so it's #UD
        backend.map_memory(0, &[0x0F]);
        let vcpu = VCpu::new(0);
        let mut regs = vcpu.registers();
        regs.rip = 0;
        vcpu.set_registers(regs);

        let exit = backend.run_vcpu(&vcpu).await.unwrap();
        match exit {
            VmExit::Exception { vector, .. } => assert_eq!(vector, 6), // #UD
            other => panic!("expected Exception, got {:?}", other),
        }
    }

    #[test]
    fn test_tcg_map_memory() {
        let backend = TcgBackend::new();
        backend.map_memory(0x100, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(backend.fetch_byte(0x100), Some(0xAA));
        assert_eq!(backend.fetch_byte(0x102), Some(0xCC));
        assert_eq!(backend.fetch_byte(0x103), None);
    }

    #[tokio::test]
    async fn test_hypervisor_vm_map_memory_tcg() {
        let backend = TcgBackend::new();
        let vm = backend.create_vm(1, 1024).await.unwrap();
        // TCG VMs should have a mapper
        assert!(vm.map_memory(0x1000, &[0xDE, 0xAD]).is_ok());
    }

    #[test]
    fn test_hypervisor_vm_map_memory_no_mapper() {
        let vm = HypervisorVm::new(HypervisorPlatform::Kvm, 1, 1024);
        // Without a mapper, map_memory should return an error
        assert!(vm.map_memory(0, &[0]).is_err());
    }
}
