//! Hypervisor backend abstraction

use crate::{Error, Result, VCpu, VmExit};
use async_trait::async_trait;

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

        #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "macos")]
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

/// Hypervisor VM instance
pub struct HypervisorVm {
    pub(crate) platform: HypervisorPlatform,
    pub(crate) vcpu_count: u32,
    pub(crate) memory_size: u64,
}

impl HypervisorVm {
    /// Create a new hypervisor VM
    pub fn new(platform: HypervisorPlatform, vcpu_count: u32, memory_size: u64) -> Self {
        Self {
            platform,
            vcpu_count,
            memory_size,
        }
    }

    /// Get the platform
    pub fn platform(&self) -> HypervisorPlatform {
        self.platform
    }

    /// Map guest memory
    ///
    /// Note: Memory mapping is handled through the backend-specific implementations
    /// (e.g., `WhpxBackend::map_memory`, `KvmVm::map_memory`). This method is
    /// provided for future use when `HypervisorVm` holds a backend reference.
    #[allow(dead_code)]
    pub async fn map_memory(&self, _guest_addr: u64, _size: u64, _host_ptr: *mut u8) -> Result<()> {
        Err(Error::VM(
            "HypervisorVm::map_memory is not yet wired — use backend-specific map_memory (e.g., KvmVm::map_memory, WhpxVm::map_memory)".into()
        ))
    }
}

/// TCG (software emulation) backend
pub struct TcgBackend {
    capabilities: HypervisorCapabilities,
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
        }
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

        Ok(HypervisorVm::new(
            HypervisorPlatform::Tcg,
            vcpu_count,
            memory_size,
        ))
    }

    async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
        // TCG (software emulation) execution
        // In a full implementation, this would:
        // 1. Fetch instruction from memory at RIP
        // 2. Decode and execute instruction
        // 3. Update CPU state
        // 4. Return exit reason when appropriate

        // For now, return a simple HLT exit to avoid infinite loops
        // This will be properly implemented when we integrate with hv2-cpu
        tracing::debug!("TCG: vCPU {} executing", vcpu.id());
        Ok(VmExit::Hlt)
    }

    async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
        tracing::debug!(
            "TCG: Injecting interrupt {} into vCPU {}",
            vector,
            vcpu.id()
        );
        // In a full implementation, this would set the interrupt pending flag
        // and deliver it when the guest enables interrupts
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
        #[cfg(target_os = "macos")]
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
        assert!(valid, "detect() returned unexpected platform: {:?}", platform);
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
        let vm = backend.create_vm(2, 1024).await.expect("create_vm should succeed");
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
}
