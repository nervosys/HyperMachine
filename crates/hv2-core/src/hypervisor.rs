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

        // Safe because we're passing valid pointers and sizes
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
        let create_result = unsafe { hv_vm_create(0) };
        if create_result == 0 {
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
        tracing::warn!(
            "HypervisorVm::map_memory is a stub — use backend-specific map_memory instead"
        );
        Ok(())
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
        tracing::info!("Initializing TCG backend (software emulation)");
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

#[cfg(target_os = "windows")]
pub mod whpx {
    //! Windows Hypervisor Platform (WHPX) backend
    //!
    //! This module provides hardware-accelerated virtualization on Windows 10+
    //! using the Windows Hypervisor Platform API.

    use super::*;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    // WHPX Constants
    const WHV_SUCCESS: i32 = 0;
    const WHV_CAPABILITY_HYPERVISOR_PRESENT: u32 = 0x00000000;
    const WHV_CAPABILITY_EXTENDED_VM_EXITS: u32 = 0x00000004;

    // Run exit reasons
    const WHV_RUN_VP_EXIT_REASON_NONE: u32 = 0x00000000;
    const WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS: u32 = 0x00000001;
    const WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS: u32 = 0x00000002;
    const WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION: u32 = 0x00000004;
    const WHV_RUN_VP_EXIT_REASON_INVALID_VP_STATE: u32 = 0x00000005;
    const WHV_RUN_VP_EXIT_REASON_UNSUPPORTED_FEATURE: u32 = 0x00000006;
    const WHV_RUN_VP_EXIT_REASON_X64_INTERRUPTION_DELIVERABLE: u32 = 0x00000007;
    const WHV_RUN_VP_EXIT_REASON_X64_HALT: u32 = 0x00000008;
    const WHV_RUN_VP_EXIT_REASON_CANCELED: u32 = 0x00000009;

    // Register names for x86_64
    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub enum WhvRegisterName {
        // General purpose
        Rax = 0x00000000,
        Rcx = 0x00000001,
        Rdx = 0x00000002,
        Rbx = 0x00000003,
        Rsp = 0x00000004,
        Rbp = 0x00000005,
        Rsi = 0x00000006,
        Rdi = 0x00000007,
        R8 = 0x00000008,
        R9 = 0x00000009,
        R10 = 0x0000000A,
        R11 = 0x0000000B,
        R12 = 0x0000000C,
        R13 = 0x0000000D,
        R14 = 0x0000000E,
        R15 = 0x0000000F,
        Rip = 0x00000010,
        Rflags = 0x00000011,
        // Segments
        Es = 0x00000012,
        Cs = 0x00000013,
        Ss = 0x00000014,
        Ds = 0x00000015,
        Fs = 0x00000016,
        Gs = 0x00000017,
        Ldtr = 0x00000018,
        Tr = 0x00000019,
        // Control registers
        Cr0 = 0x00000030,
        Cr2 = 0x00000031,
        Cr3 = 0x00000032,
        Cr4 = 0x00000033,
        Cr8 = 0x00000034,
        // Interrupt state
        PendingInterruption = 0x000000C0,
        InterruptState = 0x000000C1,
        PendingEvent = 0x000000C2,
        DeliverabilityNotifications = 0x000000C3,
    }

    /// WHPX VM handle (opaque)
    type WhvPartitionHandle = *mut c_void;

    /// Memory mapping in WHPX partition
    #[derive(Debug)]
    struct MemoryMapping {
        guest_addr: u64,
        size: u64,
        host_ptr: *mut u8,
    }

    /// Per-vCPU state in WHPX
    struct VCpuState {
        /// Whether interrupt injection is pending
        interrupt_pending: AtomicBool,
        /// Pending interrupt vector
        pending_vector: AtomicU64,
    }

    impl VCpuState {
        fn new() -> Self {
            Self {
                interrupt_pending: AtomicBool::new(false),
                pending_vector: AtomicU64::new(0),
            }
        }
    }

    /// WHPX (Windows Hypervisor Platform) backend
    pub struct WhpxBackend {
        capabilities: HypervisorCapabilities,
        partition: Option<WhvPartitionHandle>,
        memory_mappings: RwLock<Vec<MemoryMapping>>,
        vcpu_states: RwLock<HashMap<u32, VCpuState>>,
        initialized: AtomicBool,
    }

    // WHPX API bindings
    #[link(name = "WinHvPlatform")]
    extern "system" {
        fn WHvGetCapability(
            capability_code: u32,
            capability_buffer: *mut c_void,
            capability_buffer_size: u32,
            written_size: *mut u32,
        ) -> i32;

        fn WHvCreatePartition(partition: *mut WhvPartitionHandle) -> i32;

        fn WHvSetupPartition(partition: WhvPartitionHandle) -> i32;

        fn WHvDeletePartition(partition: WhvPartitionHandle) -> i32;

        fn WHvSetPartitionProperty(
            partition: WhvPartitionHandle,
            property_code: u32,
            property_buffer: *const c_void,
            property_buffer_size: u32,
        ) -> i32;

        fn WHvMapGpaRange(
            partition: WhvPartitionHandle,
            source_address: *mut c_void,
            guest_address: u64,
            size: u64,
            flags: u32,
        ) -> i32;

        fn WHvUnmapGpaRange(partition: WhvPartitionHandle, guest_address: u64, size: u64) -> i32;

        fn WHvCreateVirtualProcessor(
            partition: WhvPartitionHandle,
            vp_index: u32,
            flags: u32,
        ) -> i32;

        fn WHvDeleteVirtualProcessor(partition: WhvPartitionHandle, vp_index: u32) -> i32;

        fn WHvRunVirtualProcessor(
            partition: WhvPartitionHandle,
            vp_index: u32,
            exit_context: *mut c_void,
            exit_context_size: u32,
        ) -> i32;

        fn WHvCancelRunVirtualProcessor(partition: WhvPartitionHandle, vp_index: u32) -> i32;

        fn WHvGetVirtualProcessorRegisters(
            partition: WhvPartitionHandle,
            vp_index: u32,
            register_names: *const u32,
            register_count: u32,
            register_values: *mut c_void,
        ) -> i32;

        fn WHvSetVirtualProcessorRegisters(
            partition: WhvPartitionHandle,
            vp_index: u32,
            register_names: *const u32,
            register_count: u32,
            register_values: *const c_void,
        ) -> i32;
    }

    // WHPX property codes
    const WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT: u32 = 0x00001fff;
    const WHV_PARTITION_PROPERTY_CODE_EXTENDED_VM_EXITS: u32 = 0x00001005;

    // Memory mapping flags
    const WHV_MAP_GPA_RANGE_READ: u32 = 0x00000001;
    const WHV_MAP_GPA_RANGE_WRITE: u32 = 0x00000002;
    const WHV_MAP_GPA_RANGE_EXECUTE: u32 = 0x00000004;

    // Exit context size (large enough for all exit types)
    const EXIT_CONTEXT_SIZE: u32 = 256;

    impl WhpxBackend {
        pub fn new() -> Result<Self> {
            // Query capabilities
            let mut hypervisor_present: u32 = 0;
            let mut written: u32 = 0;

            let hr = unsafe {
                WHvGetCapability(
                    WHV_CAPABILITY_HYPERVISOR_PRESENT,
                    &mut hypervisor_present as *mut _ as *mut c_void,
                    std::mem::size_of::<u32>() as u32,
                    &mut written,
                )
            };

            if hr != WHV_SUCCESS || hypervisor_present == 0 {
                return Err(Error::Hypervisor(
                    "Windows Hypervisor Platform is not available".to_string(),
                ));
            }

            tracing::info!("WHPX hypervisor detected");

            Ok(Self {
                capabilities: HypervisorCapabilities {
                    max_vcpus: 64,
                    max_memory: 512 * 1024 * 1024 * 1024, // 512GB
                    supports_nested_virt: true,
                    supports_apic: true,
                    supports_x2apic: true,
                    supports_iommu: true,
                    supports_gpu_passthrough: false,
                },
                partition: None,
                memory_mappings: RwLock::new(Vec::new()),
                vcpu_states: RwLock::new(HashMap::new()),
                initialized: AtomicBool::new(false),
            })
        }

        /// Create a partition (VM container)
        fn create_partition(&mut self, vcpu_count: u32) -> Result<()> {
            let mut partition: WhvPartitionHandle = std::ptr::null_mut();

            // Create partition
            let hr = unsafe { WHvCreatePartition(&mut partition) };
            if hr != WHV_SUCCESS {
                return Err(Error::Hypervisor(format!(
                    "Failed to create WHPX partition: 0x{:08X}",
                    hr
                )));
            }

            // Set processor count
            let hr = unsafe {
                WHvSetPartitionProperty(
                    partition,
                    WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT,
                    &vcpu_count as *const _ as *const c_void,
                    std::mem::size_of::<u32>() as u32,
                )
            };
            if hr != WHV_SUCCESS {
                unsafe { WHvDeletePartition(partition) };
                return Err(Error::Hypervisor(format!(
                    "Failed to set WHPX processor count: 0x{:08X}",
                    hr
                )));
            }

            // Enable extended VM exits for interrupt window
            let extended_exits: u64 = 0x0000000000000008; // InterruptWindow
            let hr = unsafe {
                WHvSetPartitionProperty(
                    partition,
                    WHV_PARTITION_PROPERTY_CODE_EXTENDED_VM_EXITS,
                    &extended_exits as *const _ as *const c_void,
                    std::mem::size_of::<u64>() as u32,
                )
            };
            if hr != WHV_SUCCESS {
                tracing::warn!(
                    "Failed to enable extended VM exits: 0x{:08X}, continuing anyway",
                    hr
                );
            }

            // Setup the partition
            let hr = unsafe { WHvSetupPartition(partition) };
            if hr != WHV_SUCCESS {
                unsafe { WHvDeletePartition(partition) };
                return Err(Error::Hypervisor(format!(
                    "Failed to setup WHPX partition: 0x{:08X}",
                    hr
                )));
            }

            // Create virtual processors
            for i in 0..vcpu_count {
                let hr = unsafe { WHvCreateVirtualProcessor(partition, i, 0) };
                if hr != WHV_SUCCESS {
                    // Cleanup already created VPs
                    for j in 0..i {
                        unsafe { WHvDeleteVirtualProcessor(partition, j) };
                    }
                    unsafe { WHvDeletePartition(partition) };
                    return Err(Error::Hypervisor(format!(
                        "Failed to create WHPX vCPU {}: 0x{:08X}",
                        i, hr
                    )));
                }
                self.vcpu_states.write().insert(i, VCpuState::new());
            }

            self.partition = Some(partition);
            tracing::info!("Created WHPX partition with {} vCPUs", vcpu_count);

            Ok(())
        }

        /// Map guest physical memory
        fn map_memory(&self, guest_addr: u64, size: u64, host_ptr: *mut u8) -> Result<()> {
            let partition = self
                .partition
                .ok_or(Error::Hypervisor("WHPX partition not created".to_string()))?;

            let flags =
                WHV_MAP_GPA_RANGE_READ | WHV_MAP_GPA_RANGE_WRITE | WHV_MAP_GPA_RANGE_EXECUTE;

            let hr = unsafe {
                WHvMapGpaRange(partition, host_ptr as *mut c_void, guest_addr, size, flags)
            };

            if hr != WHV_SUCCESS {
                return Err(Error::Hypervisor(format!(
                    "Failed to map WHPX memory at 0x{:x}: 0x{:08X}",
                    guest_addr, hr
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

        /// Inject pending interrupt if any
        fn inject_pending_interrupt(&self, vcpu_id: u32) -> Result<()> {
            let states = self.vcpu_states.read();
            if let Some(state) = states.get(&vcpu_id) {
                if state.interrupt_pending.swap(false, Ordering::SeqCst) {
                    let vector = state.pending_vector.load(Ordering::SeqCst) as u8;
                    let partition = self
                        .partition
                        .ok_or(Error::Hypervisor("WHPX partition not created".to_string()))?;

                    // Set pending interruption register
                    // Format: Valid(1) | Type(3) | DeliverErrorCode(1) | Vector(8)
                    let pending: u64 = 0x80000000 | (vector as u64);
                    let reg_name = WhvRegisterName::PendingInterruption as u32;

                    let hr = unsafe {
                        WHvSetVirtualProcessorRegisters(
                            partition,
                            vcpu_id,
                            &reg_name,
                            1,
                            &pending as *const _ as *const c_void,
                        )
                    };

                    if hr != WHV_SUCCESS {
                        return Err(Error::Hypervisor(format!(
                            "Failed to inject interrupt: 0x{:08X}",
                            hr
                        )));
                    }

                    tracing::debug!("Injected interrupt {} into vCPU {}", vector, vcpu_id);
                }
            }
            Ok(())
        }
    }

    impl Default for WhpxBackend {
        fn default() -> Self {
            Self::new().expect("Failed to create WHPX backend")
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
            if self.initialized.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            tracing::info!("Initialized WHPX backend");
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
                HypervisorPlatform::Whpx,
                vcpu_count,
                memory_size,
            ))
        }

        async fn run_vcpu(&self, vcpu: &VCpu) -> Result<VmExit> {
            let partition = self
                .partition
                .ok_or(Error::Hypervisor("WHPX partition not created".to_string()))?;

            // Inject any pending interrupt
            self.inject_pending_interrupt(vcpu.id())?;

            // Run the vCPU
            let mut exit_context = [0u8; EXIT_CONTEXT_SIZE as usize];
            let hr = unsafe {
                WHvRunVirtualProcessor(
                    partition,
                    vcpu.id(),
                    exit_context.as_mut_ptr() as *mut c_void,
                    EXIT_CONTEXT_SIZE,
                )
            };

            if hr != WHV_SUCCESS {
                return Err(Error::Hypervisor(format!(
                    "Failed to run WHPX vCPU: 0x{:08X}",
                    hr
                )));
            }

            // Parse exit reason (first 4 bytes)
            let exit_reason = u32::from_le_bytes([
                exit_context[0],
                exit_context[1],
                exit_context[2],
                exit_context[3],
            ]);

            match exit_reason {
                WHV_RUN_VP_EXIT_REASON_NONE => Ok(VmExit::Unknown {
                    reason: WHV_RUN_VP_EXIT_REASON_NONE,
                }),
                WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS => {
                    // Parse MMIO exit info (offset 8)
                    let gpa = u64::from_le_bytes([
                        exit_context[8],
                        exit_context[9],
                        exit_context[10],
                        exit_context[11],
                        exit_context[12],
                        exit_context[13],
                        exit_context[14],
                        exit_context[15],
                    ]);
                    let is_write = exit_context[24] != 0;
                    let len = exit_context[25] as u32;
                    let mut data = [0u8; 8];
                    data.copy_from_slice(&exit_context[32..40]);

                    Ok(VmExit::Mmio {
                        phys_addr: gpa,
                        data,
                        len,
                        is_write,
                    })
                }
                WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS => {
                    // Parse I/O exit info
                    let port = u16::from_le_bytes([exit_context[8], exit_context[9]]);
                    let is_write = exit_context[10] != 0;
                    let size = exit_context[11];
                    let data = u32::from_le_bytes([
                        exit_context[16],
                        exit_context[17],
                        exit_context[18],
                        exit_context[19],
                    ]);

                    Ok(VmExit::Io {
                        port,
                        direction: if is_write {
                            crate::IoDirection::Out
                        } else {
                            crate::IoDirection::In
                        },
                        size,
                        data,
                    })
                }
                WHV_RUN_VP_EXIT_REASON_X64_HALT => Ok(VmExit::Hlt),
                WHV_RUN_VP_EXIT_REASON_X64_INTERRUPTION_DELIVERABLE => Ok(VmExit::InterruptWindow),
                WHV_RUN_VP_EXIT_REASON_CANCELED => Ok(VmExit::Shutdown),
                WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION => Ok(VmExit::Exception {
                    vector: 8, // Double fault
                    error_code: None,
                }),
                WHV_RUN_VP_EXIT_REASON_INVALID_VP_STATE => Ok(VmExit::Exception {
                    vector: 6, // Invalid opcode
                    error_code: None,
                }),
                _ => Ok(VmExit::Unknown {
                    reason: exit_reason,
                }),
            }
        }

        async fn inject_interrupt(&self, vcpu: &VCpu, vector: u8) -> Result<()> {
            let states = self.vcpu_states.read();
            if let Some(state) = states.get(&vcpu.id()) {
                state.pending_vector.store(vector as u64, Ordering::SeqCst);
                state.interrupt_pending.store(true, Ordering::SeqCst);
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
            let partition = self
                .partition
                .ok_or(Error::Hypervisor("WHPX partition not created".to_string()))?;

            // WHV pending interruption format (64-bit register):
            //   Bits 7:0:   InterruptionVector
            //   Bits 10:8:  InterruptionType (3 = hardware exception)
            //   Bit 11:     DeliverErrorCode
            //   Bit 31:     InterruptionPending (valid)
            //   Bits 63:32: ErrorCode (when DeliverErrorCode is set)
            let mut pending: u64 = 0x80000000 | 0x300 | (vector as u64);

            if let Some(code) = error_code {
                pending |= 0x800; // DeliverErrorCode bit
                pending |= (code as u64) << 32; // Error code in upper 32 bits
            }

            let reg_name = WhvRegisterName::PendingInterruption as u32;
            let hr = unsafe {
                WHvSetVirtualProcessorRegisters(
                    partition,
                    vcpu.id(),
                    &reg_name,
                    1,
                    &pending as *const _ as *const c_void,
                )
            };

            if hr != WHV_SUCCESS {
                return Err(Error::Hypervisor(format!(
                    "Failed to inject exception: 0x{:08X}",
                    hr
                )));
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
            let partition = self
                .partition
                .ok_or(Error::Hypervisor("WHPX partition not created".to_string()))?;

            // Mask the data by access size
            let masked: u64 = match size {
                1 => (data & 0xFF) as u64,
                2 => (data & 0xFFFF) as u64,
                4 => data as u64,
                _ => data as u64,
            };

            let reg_name = WhvRegisterName::Rax as u32;
            let hr = unsafe {
                WHvSetVirtualProcessorRegisters(
                    partition,
                    vcpu.id(),
                    &reg_name,
                    1,
                    &masked as *const _ as *const c_void,
                )
            };

            if hr != WHV_SUCCESS {
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
            if let Some(partition) = self.partition.take() {
                // Cleanup virtual processors
                for vcpu_id in self.vcpu_states.read().keys() {
                    unsafe { WHvDeleteVirtualProcessor(partition, *vcpu_id) };
                }

                // Unmap memory
                for mapping in self.memory_mappings.read().iter() {
                    unsafe { WHvUnmapGpaRange(partition, mapping.guest_addr, mapping.size) };
                }

                // Delete partition
                unsafe { WHvDeletePartition(partition) };
                tracing::info!("WHPX partition destroyed");
            }
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    unsafe impl Send for WhpxBackend {}
    unsafe impl Sync for WhpxBackend {}
}

// KVM backend is now in backends::kvm module

/// macOS Hypervisor Framework (HVF) backend
#[cfg(target_os = "macos")]
pub mod hvf {
    //! Apple Hypervisor Framework (HVF) backend
    //!
    //! This module provides hardware-accelerated virtualization on macOS
    //! using the Hypervisor.framework API. Supports both Intel and Apple Silicon.

    use super::*;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    // HVF return codes
    const HV_SUCCESS: i32 = 0;
    const HV_ERROR: i32 = -85377023;
    const HV_BUSY: i32 = -85377022;
    const HV_BAD_ARGUMENT: i32 = -85377021;
    const HV_NO_RESOURCES: i32 = -85377020;
    const HV_NO_DEVICE: i32 = -85377019;
    const HV_UNSUPPORTED: i32 = -85377018;

    // VM exit reasons
    const HV_EXIT_REASON_EXCEPTION: u64 = 0;
    const HV_EXIT_REASON_EPT_VIOLATION: u64 = 1;
    const HV_EXIT_REASON_IO_INSTRUCTION: u64 = 2;
    const HV_EXIT_REASON_HLT: u64 = 3;
    const HV_EXIT_REASON_INTERRUPT_WINDOW: u64 = 4;
    const HV_EXIT_REASON_TRIPLE_FAULT: u64 = 5;

    // x86_64 register IDs
    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub enum HvX86Reg {
        Rip = 0,
        Rflags = 1,
        Rax = 2,
        Rcx = 3,
        Rdx = 4,
        Rbx = 5,
        Rsp = 6,
        Rbp = 7,
        Rsi = 8,
        Rdi = 9,
        R8 = 10,
        R9 = 11,
        R10 = 12,
        R11 = 13,
        R12 = 14,
        R13 = 15,
        R14 = 16,
        R15 = 17,
        Cr0 = 18,
        Cr2 = 19,
        Cr3 = 20,
        Cr4 = 21,
    }

    // VMCS fields
    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub enum VmcsField {
        GuestRip = 0x681e,
        GuestRsp = 0x681c,
        GuestRflags = 0x6820,
        GuestCr0 = 0x6800,
        GuestCr3 = 0x6802,
        GuestCr4 = 0x6804,
        VmExitReason = 0x4402,
        VmExitQualification = 0x6400,
        VmEntryInterruptionInfo = 0x4016,
        VmEntryExceptionErrorCode = 0x4018,
        GuestInterruptibilityState = 0x4824,
        GuestActivityState = 0x4826,
    }

    // HVF opaque types
    type HvVcpuId = u64;

    /// Per-vCPU state in HVF
    struct VCpuState {
        hv_vcpu: HvVcpuId,
        interrupt_pending: AtomicBool,
        pending_vector: AtomicU64,
    }

    /// Memory mapping in HVF
    #[derive(Debug)]
    struct MemoryMapping {
        guest_addr: u64,
        size: u64,
        host_ptr: *mut u8,
    }

    // HVF API bindings
    #[link(name = "Hypervisor", kind = "framework")]
    extern "C" {
        fn hv_vm_create(flags: u64) -> i32;
        fn hv_vm_destroy() -> i32;
        fn hv_vm_map(uva: *mut c_void, gpa: u64, size: u64, flags: u64) -> i32;
        fn hv_vm_unmap(gpa: u64, size: u64) -> i32;

        fn hv_vcpu_create(vcpu: *mut HvVcpuId, flags: u64) -> i32;
        fn hv_vcpu_destroy(vcpu: HvVcpuId) -> i32;
        fn hv_vcpu_run(vcpu: HvVcpuId) -> i32;
        fn hv_vcpu_interrupt(vcpu: *const HvVcpuId, count: u32) -> i32;

        fn hv_vcpu_read_register(vcpu: HvVcpuId, reg: u32, value: *mut u64) -> i32;
        fn hv_vcpu_write_register(vcpu: HvVcpuId, reg: u32, value: u64) -> i32;

        fn hv_vmx_vcpu_read_vmcs(vcpu: HvVcpuId, field: u32, value: *mut u64) -> i32;
        fn hv_vmx_vcpu_write_vmcs(vcpu: HvVcpuId, field: u32, value: u64) -> i32;
    }

    // Memory mapping flags
    const HV_MEMORY_READ: u64 = 1 << 0;
    const HV_MEMORY_WRITE: u64 = 1 << 1;
    const HV_MEMORY_EXEC: u64 = 1 << 2;

    /// Apple Hypervisor Framework backend
    pub struct HvfBackend {
        capabilities: HypervisorCapabilities,
        vm_created: AtomicBool,
        vcpu_states: RwLock<HashMap<u32, VCpuState>>,
        memory_mappings: RwLock<Vec<MemoryMapping>>,
        initialized: AtomicBool,
    }

    impl HvfBackend {
        pub fn new() -> Result<Self> {
            // Try to create a VM to check if HVF is available
            let result = unsafe { hv_vm_create(0) };
            if result == HV_SUCCESS {
                // VM created successfully, destroy it and return
                unsafe { hv_vm_destroy() };
                tracing::info!("HVF hypervisor detected and available");
            } else {
                return Err(Error::Hypervisor(format!(
                    "Apple Hypervisor Framework is not available: {}",
                    result
                )));
            }

            // Determine capabilities based on architecture
            #[cfg(target_arch = "aarch64")]
            let caps = HypervisorCapabilities {
                max_vcpus: 128,
                max_memory: 1024 * 1024 * 1024 * 1024, // 1TB
                supports_nested_virt: false,
                supports_apic: false, // No APIC on ARM
                supports_x2apic: false,
                supports_iommu: false,
                supports_gpu_passthrough: false,
            };

            #[cfg(target_arch = "x86_64")]
            let caps = HypervisorCapabilities {
                max_vcpus: 64,
                max_memory: 512 * 1024 * 1024 * 1024, // 512GB
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
                initialized: AtomicBool::new(false),
            })
        }

        /// Create the VM
        fn create_vm(&self) -> Result<()> {
            if self.vm_created.swap(true, Ordering::SeqCst) {
                return Ok(()); // Already created
            }

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

        /// Create a vCPU
        fn create_vcpu(&self, vcpu_id: u32) -> Result<HvVcpuId> {
            let mut hv_vcpu: HvVcpuId = 0;
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

        /// Map guest physical memory
        fn map_memory(&self, guest_addr: u64, size: u64, host_ptr: *mut u8) -> Result<()> {
            let flags = HV_MEMORY_READ | HV_MEMORY_WRITE | HV_MEMORY_EXEC;

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

        /// Inject pending interrupt
        fn inject_pending_interrupt(&self, vcpu_id: u32) -> Result<()> {
            let states = self.vcpu_states.read();
            if let Some(state) = states.get(&vcpu_id) {
                if state.interrupt_pending.swap(false, Ordering::SeqCst) {
                    let vector = state.pending_vector.load(Ordering::SeqCst) as u32;

                    // Write to VM-entry interruption-information field
                    // Format: Valid(1) | Type(3) | DeliverErrorCode(1) | Reserved | Vector(8)
                    let interrupt_info: u64 = 0x80000000 | (vector as u64);

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

        /// Read exit reason from VMCS
        fn read_exit_reason(&self, hv_vcpu: HvVcpuId) -> Result<u64> {
            let mut reason: u64 = 0;
            let result = unsafe {
                hv_vmx_vcpu_read_vmcs(hv_vcpu, VmcsField::VmExitReason as u32, &mut reason)
            };
            if result != HV_SUCCESS {
                return Err(Error::Hypervisor(format!(
                    "Failed to read exit reason: {}",
                    result
                )));
            }
            Ok(reason)
        }

        /// Read exit qualification from VMCS
        fn read_exit_qualification(&self, hv_vcpu: HvVcpuId) -> Result<u64> {
            let mut qual: u64 = 0;
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
            self.create_vm()?;
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
            let result = unsafe { hv_vcpu_run(hv_vcpu) };

            if result != HV_SUCCESS {
                return Err(Error::Hypervisor(format!(
                    "Failed to run HVF vCPU: {}",
                    result
                )));
            }

            // Read exit reason
            let exit_reason = self.read_exit_reason(hv_vcpu)?;

            // VMX exit reasons (Intel)
            match exit_reason & 0xFFFF {
                0 => {
                    // Exception or NMI
                    let qual = self.read_exit_qualification(hv_vcpu)?;
                    let vector = (qual & 0xFF) as u8;
                    let has_error_code = (qual & 0x800) != 0;
                    let error_code = if has_error_code {
                        Some(((qual >> 32) & 0xFFFFFFFF) as u32)
                    } else {
                        None
                    };
                    Ok(VmExit::Exception { vector, error_code })
                }
                1 => {
                    // External interrupt
                    Ok(VmExit::InterruptWindow)
                }
                10 => {
                    // CPUID
                    // For now, let the default handler deal with it
                    Ok(VmExit::Unknown {
                        reason: 10, // VMX exit reason for CPUID
                    })
                }
                12 => {
                    // HLT
                    Ok(VmExit::Hlt)
                }
                30 => {
                    // I/O instruction
                    let qual = self.read_exit_qualification(hv_vcpu)?;
                    let size = ((qual & 0x7) + 1) as u8;
                    let is_out = (qual & 0x8) == 0;
                    let port = ((qual >> 16) & 0xFFFF) as u16;

                    // Read RAX for data
                    let mut rax: u64 = 0;
                    unsafe {
                        hv_vcpu_read_register(hv_vcpu, HvX86Reg::Rax as u32, &mut rax);
                    }

                    Ok(VmExit::Io {
                        port,
                        direction: if is_out {
                            crate::IoDirection::Out
                        } else {
                            crate::IoDirection::In
                        },
                        size,
                        data: rax as u32,
                    })
                }
                48 | 49 => {
                    // EPT violation (memory access)
                    let qual = self.read_exit_qualification(hv_vcpu)?;
                    let is_write = (qual & 0x2) != 0;

                    // Read guest physical address from VMCS
                    let mut gpa: u64 = 0;
                    unsafe {
                        hv_vmx_vcpu_read_vmcs(hv_vcpu, 0x2400, &mut gpa); // GUEST_PHYSICAL_ADDRESS
                    }

                    let mut data = [0u8; 8];
                    // For writes, we'd need to decode the instruction to get the data
                    // For now, read from RAX as a simplification

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
            //   Bits 7:0:   Vector
            //   Bits 10:8:  Type (3 = hardware exception)
            //   Bit 11:     Deliver error code
            //   Bit 31:     Valid
            let mut info: u64 = 0x80000000 | 0x300 | (vector as u64);

            if error_code.is_some() {
                info |= 0x800; // Deliver error code bit
            }

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
                unsafe { hv_vcpu_destroy(state.hv_vcpu) };
            }

            // Unmap memory
            for mapping in self.memory_mappings.read().iter() {
                unsafe { hv_vm_unmap(mapping.guest_addr, mapping.size) };
            }

            // Destroy VM
            if self.vm_created.swap(false, Ordering::SeqCst) {
                unsafe { hv_vm_destroy() };
                tracing::info!("HVF VM destroyed");
            }

            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    unsafe impl Send for HvfBackend {}
    unsafe impl Sync for HvfBackend {}
}

/// Create the best available hypervisor backend
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
        HypervisorPlatform::Whpx => Ok(Box::new(whpx::WhpxBackend::new()?)),
        #[cfg(target_os = "macos")]
        HypervisorPlatform::Hvf => Ok(Box::new(hvf::HvfBackend::new()?)),
        HypervisorPlatform::Tcg | _ => Ok(Box::new(TcgBackend::new())),
    }
}
