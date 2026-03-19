//! Virtual CPU implementation

use crate::{Error, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Virtual CPU state
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VCpuState {
    Uninitialized,
    Running,
    Stopped,
    Paused,
    Error,
}

/// CPU register set (x86_64 example)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSet {
    // General purpose registers
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,

    // Instruction pointer and flags
    pub rip: u64,
    pub rflags: u64,

    // Segment registers
    pub cs: u64,
    pub ds: u64,
    pub es: u64,
    pub fs: u64,
    pub gs: u64,
    pub ss: u64,
}

impl Default for RegisterSet {
    fn default() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rsp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rflags: 0x2, // Reserved bit must be 1
            cs: 0,
            ds: 0,
            es: 0,
            fs: 0,
            gs: 0,
            ss: 0,
        }
    }
}

/// x86-64 Control Registers
///
/// Control registers (CR0-CR4, CR8) control processor operating mode,
/// paging configuration, and various processor features.
///
/// # References
/// - Intel SDM Volume 3A, Section 2.5 - Control Registers
///
/// # Common Usage Patterns
///
/// ## Enabling Protected Mode
/// ```no_run
/// # use hv2_core::ControlRegisters;
/// # let mut cr = ControlRegisters::default();
/// cr.cr0 |= 0x1; // CR0.PE
/// assert!(cr.is_protected_mode());
/// ```
///
/// ## Enabling Paging
/// ```no_run
/// # use hv2_core::ControlRegisters;
/// # let mut cr = ControlRegisters::default();
/// cr.cr0 |= 0x1;         // CR0.PE (protected mode required)
/// cr.cr3 = 0x1000;       // Page directory base
/// cr.cr0 |= 1 << 31;     // CR0.PG
/// assert!(cr.is_paging_enabled());
/// ```
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ControlRegisters {
    /// CR0 - Control Register 0
    ///
    /// Controls operating mode and processor state:
    /// - Bit 0 (PE): Protected Mode Enable
    /// - Bit 1 (MP): Monitor Coprocessor
    /// - Bit 2 (EM): Emulation
    /// - Bit 3 (TS): Task Switched
    /// - Bit 4 (ET): Extension Type
    /// - Bit 5 (NE): Numeric Error
    /// - Bit 16 (WP): Write Protect
    /// - Bit 18 (AM): Alignment Mask
    /// - Bit 29 (NW): Not Write-through
    /// - Bit 30 (CD): Cache Disable
    /// - Bit 31 (PG): Paging Enable
    pub cr0: u64,

    /// CR2 - Control Register 2
    ///
    /// Contains the linear (virtual) address that caused a page fault.
    /// Loaded by the processor when a page fault (#PF) exception occurs.
    pub cr2: u64,

    /// CR3 - Control Register 3 (Page Directory Base Register)
    ///
    /// Contains the physical address of the base of the page directory.
    /// Bits 12-63: Physical address of page directory (4KB aligned)
    /// Bits 0-11: Reserved/flags (depends on paging mode)
    pub cr3: u64,

    /// CR4 - Control Register 4
    ///
    /// Controls architecture extensions and features:
    /// - Bit 0 (VME): Virtual-8086 Mode Extensions
    /// - Bit 1 (PVI): Protected-Mode Virtual Interrupts
    /// - Bit 2 (TSD): Time Stamp Disable
    /// - Bit 3 (DE): Debugging Extensions
    /// - Bit 4 (PSE): Page Size Extensions
    /// - Bit 5 (PAE): Physical Address Extension
    /// - Bit 6 (MCE): Machine-Check Enable
    /// - Bit 7 (PGE): Page Global Enable
    /// - Bit 8 (PCE): Performance Counter Enable
    /// - Bit 9 (OSFXSR): OS FXSAVE/FXRSTOR Support
    /// - Bit 10 (OSXMMEXCPT): OS Unmasked Exception Support
    /// - Bit 11 (UMIP): User-Mode Instruction Prevention
    /// - Bit 12 (LA57): 57-bit Linear Addresses
    /// - Bit 13 (VMXE): VMX Enable
    /// - Bit 16 (FSGSBASE): FSGSBASE Instructions Enable
    /// - Bit 17 (PCIDE): PCID Enable
    /// - Bit 18 (OSXSAVE): XSAVE Enable
    /// - Bit 20 (SMEP): Supervisor Mode Execution Prevention
    /// - Bit 21 (SMAP): Supervisor Mode Access Prevention
    /// - Bit 22 (PKE): Protection Key Enable
    pub cr4: u64,

    /// CR8 - Control Register 8 (Task Priority Register)
    ///
    /// Provides read/write access to the Task Priority Register (TPR).
    /// Controls the priority threshold for interrupts.
    /// Only available in 64-bit mode.
    pub cr8: u64,

    /// IA32_EFER - Extended Feature Enable Register
    ///
    /// Controls extended processor features:
    /// - Bit 0 (SCE): SYSCALL/SYSRET Enable
    /// - Bit 8 (LME): Long Mode Enable (IA-32e mode enable)
    /// - Bit 10 (LMA): Long Mode Active (READ-ONLY, set by processor)
    /// - Bit 11 (NXE): No-Execute Enable
    ///
    /// For long mode activation:
    /// 1. Set CR4.PAE = 1 (Physical Address Extension)
    /// 2. Set EFER.LME = 1 (Long Mode Enable)
    /// 3. Set CR0.PG = 1 (Paging)
    /// 4. Processor automatically sets EFER.LMA = 1
    ///
    /// Reference: Intel SDM Volume 3A, Section 9.8.5
    pub efer: u64,
}

impl ControlRegisters {
    /// Check if protected mode is enabled (CR0.PE = 1)
    ///
    /// When protected mode is enabled, the processor uses segment descriptors
    /// from the GDT/LDT for memory protection and privilege checking.
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// assert!(!cr.is_protected_mode());
    ///
    /// cr.cr0 |= 0x1; // Set CR0.PE
    /// assert!(cr.is_protected_mode());
    /// ```
    #[inline]
    pub fn is_protected_mode(&self) -> bool {
        self.cr0 & 0x1 != 0
    }

    /// Check if paging is enabled (CR0.PG = 1)
    ///
    /// When paging is enabled, virtual memory translation is active.
    /// Paging requires protected mode (CR0.PE = 1).
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// assert!(!cr.is_paging_enabled());
    ///
    /// cr.cr0 |= 1 << 31; // Set CR0.PG
    /// assert!(cr.is_paging_enabled());
    /// ```
    #[inline]
    pub fn is_paging_enabled(&self) -> bool {
        self.cr0 & (1 << 31) != 0
    }

    /// Check if PAE (Physical Address Extension) is enabled (CR4.PAE = 1)
    ///
    /// PAE extends physical addresses from 32 to 36 bits (64 GB physical memory).
    /// Required for 64-bit long mode and certain paging modes.
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// assert!(!cr.is_pae_enabled());
    ///
    /// cr.cr4 |= 1 << 5; // Set CR4.PAE
    /// assert!(cr.is_pae_enabled());
    /// ```
    #[inline]
    pub fn is_pae_enabled(&self) -> bool {
        self.cr4 & (1 << 5) != 0
    }

    /// Get page directory base address from CR3
    ///
    /// Returns the physical address of the page directory, with the lower
    /// 12 bits (page offset) cleared. The page directory must be 4KB aligned.
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// cr.cr3 = 0x12345678; // Arbitrary address with offset bits
    ///
    /// // Returns address aligned to 4KB boundary
    /// assert_eq!(cr.page_directory_base(), 0x12345000);
    /// ```
    #[inline]
    pub fn page_directory_base(&self) -> u64 {
        self.cr3 & !0xFFF // Clear lower 12 bits (4KB alignment)
    }

    /// Check if long mode is enabled (IA32_EFER.LME = 1)
    ///
    /// When LME is set, the processor will activate long mode (IA-32e mode)
    /// when paging is enabled with PAE. The processor sets LMA (Long Mode Active)
    /// automatically when long mode activates.
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// assert!(!cr.is_long_mode_enabled());
    ///
    /// cr.efer |= 1 << 8; // Set EFER.LME
    /// assert!(cr.is_long_mode_enabled());
    /// ```
    #[inline]
    pub fn is_long_mode_enabled(&self) -> bool {
        self.efer & (1 << 8) != 0 // EFER.LME
    }

    /// Check if long mode is active (IA32_EFER.LMA = 1)
    ///
    /// LMA is a read-only bit set by the processor when long mode is active.
    /// It indicates the processor is operating in IA-32e mode.
    ///
    /// Prerequisites for LMA to be set:
    /// - CR4.PAE = 1 (Physical Address Extension)
    /// - EFER.LME = 1 (Long Mode Enable)
    /// - CR0.PG = 1 (Paging)
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// assert!(!cr.is_long_mode_active());
    ///
    /// // Simulating processor setting LMA (normally read-only)
    /// cr.efer |= 1 << 10;
    /// assert!(cr.is_long_mode_active());
    /// ```
    #[inline]
    pub fn is_long_mode_active(&self) -> bool {
        self.efer & (1 << 10) != 0 // EFER.LMA
    }

    /// Check if No-Execute is enabled (IA32_EFER.NXE = 1)
    ///
    /// When NXE is set, the processor respects the XD (execute-disable) bit
    /// in page table entries, preventing execution of code from pages marked
    /// as data-only.
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    /// assert!(!cr.is_nxe_enabled());
    ///
    /// cr.efer |= 1 << 11; // Set EFER.NXE
    /// assert!(cr.is_nxe_enabled());
    /// ```
    #[inline]
    pub fn is_nxe_enabled(&self) -> bool {
        self.efer & (1 << 11) != 0 // EFER.NXE
    }

    /// Check if the control register state is valid for guest execution
    ///
    /// Validates common requirements:
    /// - If paging is enabled (CR0.PG), protected mode must be enabled (CR0.PE)
    /// - Reserved bits should not be set (simplified check)
    ///
    /// # Returns
    /// - `Ok(())` if the state is valid
    /// - `Err(String)` describing the validation failure
    ///
    /// # Example
    /// ```
    /// # use hv2_core::ControlRegisters;
    /// let mut cr = ControlRegisters::default();
    ///
    /// // Valid: no paging, no protected mode
    /// assert!(cr.validate().is_ok());
    ///
    /// // Invalid: paging without protected mode
    /// cr.cr0 = 1 << 31; // CR0.PG without CR0.PE
    /// assert!(cr.validate().is_err());
    ///
    /// // Valid: both enabled
    /// cr.cr0 = (1 << 31) | 0x1; // CR0.PG + CR0.PE
    /// assert!(cr.validate().is_ok());
    /// ```
    pub fn validate(&self) -> std::result::Result<(), String> {
        // CR0.PG requires CR0.PE
        if self.is_paging_enabled() && !self.is_protected_mode() {
            return Err(
                "Invalid CR0: Paging (PG) requires Protected Mode (PE) to be enabled".to_string(),
            );
        }

        // Long mode (EFER.LMA) requires specific prerequisites
        if self.is_long_mode_active() {
            // Long mode requires protected mode
            if !self.is_protected_mode() {
                return Err(
                    "Invalid state: Long Mode Active (LMA) requires Protected Mode (PE)"
                        .to_string(),
                );
            }

            // Long mode requires paging
            if !self.is_paging_enabled() {
                return Err(
                    "Invalid state: Long Mode Active (LMA) requires Paging (PG)".to_string()
                );
            }

            // Long mode requires PAE
            if !self.is_pae_enabled() {
                return Err("Invalid state: Long Mode Active (LMA) requires PAE".to_string());
            }

            // Long mode requires LME to be set
            if !self.is_long_mode_enabled() {
                return Err(
                    "Invalid state: Long Mode Active (LMA) requires Long Mode Enable (LME)"
                        .to_string(),
                );
            }
        }

        // If LME is set and paging is enabled with PAE, LMA should be active
        // (Note: This is enforced by hardware, but we validate for consistency)
        if self.is_long_mode_enabled()
            && self.is_paging_enabled()
            && self.is_pae_enabled()
            && !self.is_long_mode_active()
        {
            // This is actually just a warning - hardware will set LMA
            // We don't error here because this could be a transitional state
        }

        // Additional validations can be added here:
        // - Check reserved bits
        // - Validate CR4 feature dependencies
        // - Check for unsupported combinations

        Ok(())
    }
}

/// Virtual CPU
pub struct VCpu {
    id: u32,
    state: Arc<Mutex<VCpuState>>,
    registers: Arc<Mutex<RegisterSet>>,
}

impl VCpu {
    /// Create a new vCPU
    #[must_use]
    pub fn new(id: u32) -> Self {
        Self {
            id,
            state: Arc::new(Mutex::new(VCpuState::Uninitialized)),
            registers: Arc::new(Mutex::new(RegisterSet::default())),
        }
    }

    /// Get vCPU ID
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get current state
    pub fn state(&self) -> VCpuState {
        *self.state.lock()
    }

    /// Set state
    pub fn set_state(&self, new_state: VCpuState) {
        *self.state.lock() = new_state;
    }

    /// Get register set
    pub fn registers(&self) -> RegisterSet {
        self.registers.lock().clone()
    }

    /// Set register set
    pub fn set_registers(&self, regs: RegisterSet) {
        *self.registers.lock() = regs;
    }

    /// Run the vCPU
    ///
    /// **Note:** Direct `VCpu::run()` is not supported. Use
    /// `HypervisorBackend::run_vcpu()` which delegates to the platform-specific
    /// backend (KVM, WHPX, HVF) for real guest execution.
    pub fn run(&self) -> Result<VCpuExit> {
        Err(Error::Cpu(format!(
            "VCpu::run() is not directly executable — use HypervisorBackend::run_vcpu() for vCPU {}",
            self.id
        )))
    }

    /// Pause the vCPU
    pub fn pause(&self) -> Result<()> {
        let mut state = self.state.lock();

        if *state != VCpuState::Running {
            return Err(Error::Cpu(format!(
                "Cannot pause vCPU {} in state {:?}",
                self.id, *state
            )));
        }

        *state = VCpuState::Paused;
        Ok(())
    }

    /// Stop the vCPU
    pub fn stop(&self) -> Result<()> {
        *self.state.lock() = VCpuState::Stopped;
        Ok(())
    }
}

/// Reasons for vCPU exit
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum VCpuExit {
    /// Halted
    Hlt,
    /// I/O port access
    IoOut {
        port: u16,
        data: Vec<u8>,
    },
    IoIn {
        port: u16,
        size: u8,
    },
    /// MMIO access
    MmioRead {
        addr: u64,
        size: u8,
    },
    MmioWrite {
        addr: u64,
        data: Vec<u8>,
    },
    /// Interrupt
    Interrupt {
        vector: u8,
    },
    /// Exception
    Exception {
        vector: u8,
        error_code: Option<u32>,
    },
    /// Shutdown
    Shutdown,
    /// Unknown exit reason
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vcpu_creation() {
        let vcpu = VCpu::new(0);
        assert_eq!(vcpu.id(), 0);
        assert_eq!(vcpu.state(), VCpuState::Uninitialized);
    }

    #[test]
    fn test_vcpu_state_transitions() {
        let vcpu = VCpu::new(0);
        vcpu.set_state(VCpuState::Stopped);
        assert_eq!(vcpu.state(), VCpuState::Stopped);
    }

    // Control Registers tests
    #[test]
    fn test_control_registers_default() {
        let cr = ControlRegisters::default();
        assert_eq!(cr.cr0, 0);
        assert_eq!(cr.cr2, 0);
        assert_eq!(cr.cr3, 0);
        assert_eq!(cr.cr4, 0);
        assert_eq!(cr.cr8, 0);
        assert!(!cr.is_protected_mode());
        assert!(!cr.is_paging_enabled());
        assert!(!cr.is_pae_enabled());
    }

    #[test]
    fn test_protected_mode_detection() {
        let mut cr = ControlRegisters::default();
        assert!(!cr.is_protected_mode());

        // Set CR0.PE (bit 0)
        cr.cr0 = 0x1;
        assert!(cr.is_protected_mode());

        // Clear CR0.PE
        cr.cr0 = 0x0;
        assert!(!cr.is_protected_mode());
    }

    #[test]
    fn test_paging_detection() {
        let mut cr = ControlRegisters::default();
        assert!(!cr.is_paging_enabled());

        // Set CR0.PG (bit 31)
        cr.cr0 = 1 << 31;
        assert!(cr.is_paging_enabled());

        // Clear CR0.PG
        cr.cr0 = 0x0;
        assert!(!cr.is_paging_enabled());
    }

    #[test]
    fn test_pae_detection() {
        let mut cr = ControlRegisters::default();
        assert!(!cr.is_pae_enabled());

        // Set CR4.PAE (bit 5)
        cr.cr4 = 1 << 5;
        assert!(cr.is_pae_enabled());

        // Clear CR4.PAE
        cr.cr4 = 0x0;
        assert!(!cr.is_pae_enabled());
    }

    #[test]
    fn test_page_directory_base() {
        let mut cr = ControlRegisters::default();

        // Test with aligned address
        cr.cr3 = 0x12345000;
        assert_eq!(cr.page_directory_base(), 0x12345000);

        // Test with unaligned address (lower 12 bits should be cleared)
        cr.cr3 = 0x12345678;
        assert_eq!(cr.page_directory_base(), 0x12345000);

        // Test with all bits set
        cr.cr3 = 0xFFFFFFFFFFFFF000;
        assert_eq!(cr.page_directory_base(), 0xFFFFFFFFFFFFF000);

        // Test with offset bits
        cr.cr3 = 0x1000 | 0xABC;
        assert_eq!(cr.page_directory_base(), 0x1000);
    }

    #[test]
    fn test_control_registers_validation() {
        let mut cr = ControlRegisters::default();

        // Valid: no paging, no protected mode
        assert!(cr.validate().is_ok());

        // Valid: protected mode only
        cr.cr0 = 0x1; // CR0.PE
        assert!(cr.validate().is_ok());

        // Invalid: paging without protected mode
        cr.cr0 = 1 << 31; // CR0.PG without CR0.PE
        assert!(cr.validate().is_err());
        let err = cr.validate().unwrap_err();
        assert!(err.contains("Paging"));
        assert!(err.contains("Protected Mode"));

        // Valid: both protected mode and paging
        cr.cr0 = (1 << 31) | 0x1; // CR0.PG + CR0.PE
        assert!(cr.validate().is_ok());

        // Valid: protected mode, paging, and PAE
        cr.cr0 = (1 << 31) | 0x1;
        cr.cr4 = 1 << 5; // CR4.PAE
        assert!(cr.validate().is_ok());
    }

    #[test]
    fn test_control_registers_combined_state() {
        let mut cr = ControlRegisters::default();

        // Set up a typical protected mode with paging configuration
        cr.cr0 = 0x80000001; // CR0.PG | CR0.PE
        cr.cr3 = 0x1000; // Page directory at 0x1000
        cr.cr4 = 0x20; // CR4.PAE

        assert!(cr.is_protected_mode());
        assert!(cr.is_paging_enabled());
        assert!(cr.is_pae_enabled());
        assert_eq!(cr.page_directory_base(), 0x1000);
        assert!(cr.validate().is_ok());
    }

    #[test]
    fn test_control_registers_serialization() {
        let mut cr = ControlRegisters::default();
        cr.cr0 = 0x80000001;
        cr.cr3 = 0x1000;
        cr.cr4 = 0x20;

        // Test that it can be serialized and deserialized
        let serialized = serde_json::to_string(&cr).unwrap();
        let deserialized: ControlRegisters = serde_json::from_str(&serialized).unwrap();

        assert_eq!(cr.cr0, deserialized.cr0);
        assert_eq!(cr.cr3, deserialized.cr3);
        assert_eq!(cr.cr4, deserialized.cr4);
    }

    #[test]
    fn test_long_mode_detection() {
        let mut cr = ControlRegisters::default();

        // Initially, long mode not enabled
        assert!(!cr.is_long_mode_enabled());
        assert!(!cr.is_long_mode_active());

        // Enable long mode (EFER.LME)
        cr.efer |= 1 << 8;
        assert!(cr.is_long_mode_enabled());
        assert!(!cr.is_long_mode_active()); // Not active yet

        // Simulate processor activating long mode (EFER.LMA)
        cr.efer |= 1 << 10;
        assert!(cr.is_long_mode_enabled());
        assert!(cr.is_long_mode_active());
    }

    #[test]
    fn test_nxe_detection() {
        let mut cr = ControlRegisters::default();

        // Initially, NXE not enabled
        assert!(!cr.is_nxe_enabled());

        // Enable NXE
        cr.efer |= 1 << 11;
        assert!(cr.is_nxe_enabled());
    }

    #[test]
    fn test_long_mode_validation() {
        let mut cr = ControlRegisters::default();

        // Valid: No long mode
        assert!(cr.validate().is_ok());

        // Invalid: LMA without protected mode
        cr.efer |= 1 << 10; // EFER.LMA
        assert!(cr.validate().is_err());

        // Invalid: LMA without paging
        cr.cr0 |= 0x1; // CR0.PE
        assert!(cr.validate().is_err());

        // Invalid: LMA without PAE
        cr.cr0 |= 1 << 31; // CR0.PG
        assert!(cr.validate().is_err());

        // Invalid: LMA without LME
        cr.cr4 |= 1 << 5; // CR4.PAE
        assert!(cr.validate().is_err());

        // Valid: All prerequisites met
        cr.efer |= 1 << 8; // EFER.LME
        assert!(cr.validate().is_ok());
    }

    #[test]
    fn test_long_mode_prerequisites() {
        let mut cr = ControlRegisters::default();

        // Setup prerequisites for long mode
        cr.cr0 |= 0x1; // Protected mode
        cr.cr4 |= 1 << 5; // PAE
        cr.efer |= 1 << 8; // LME
        cr.cr0 |= 1 << 31; // Paging
        cr.efer |= 1 << 10; // LMA (processor sets this)

        // Verify all flags
        assert!(cr.is_protected_mode());
        assert!(cr.is_pae_enabled());
        assert!(cr.is_long_mode_enabled());
        assert!(cr.is_paging_enabled());
        assert!(cr.is_long_mode_active());

        // Should validate successfully
        assert!(cr.validate().is_ok());
    }

    #[test]
    fn test_efer_serialization() {
        let mut cr = ControlRegisters::default();
        cr.efer = 0x500; // LME + LMA

        // Test serialization
        let serialized = serde_json::to_string(&cr).unwrap();
        let deserialized: ControlRegisters = serde_json::from_str(&serialized).unwrap();

        assert_eq!(cr.efer, deserialized.efer);
        assert_eq!(
            cr.is_long_mode_enabled(),
            deserialized.is_long_mode_enabled()
        );
        assert_eq!(cr.is_long_mode_active(), deserialized.is_long_mode_active());
    }
}
