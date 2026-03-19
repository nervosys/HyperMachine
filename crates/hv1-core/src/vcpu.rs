//! Virtual CPU (vCPU) management for Type-1 hypervisor
//!
//! This module handles:
//! - vCPU creation and lifecycle
//! - Guest register state management
//! - vCPU scheduling
//! - VM entry/exit handling

use crate::{CpuVendor, Error, Result};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// vCPU ID counter
static VCPU_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// vCPU state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpuState {
    /// vCPU is not initialized
    Uninitialized,
    /// vCPU is ready to run
    Ready,
    /// vCPU is currently running
    Running,
    /// vCPU is halted (HLT instruction)
    Halted,
    /// vCPU is waiting for an event
    Waiting,
    /// vCPU has exited due to an error
    Error,
}

/// General purpose registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GeneralRegisters {
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
    pub rip: u64,
    pub rflags: u64,
}

/// Segment register state
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentRegister {
    /// Selector
    pub selector: u16,
    /// Attributes
    pub attributes: u16,
    /// Limit
    pub limit: u32,
    /// Base address
    pub base: u64,
}

/// Control registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlRegisters {
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
}

/// Debug registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugRegisters {
    pub dr0: u64,
    pub dr1: u64,
    pub dr2: u64,
    pub dr3: u64,
    pub dr6: u64,
    pub dr7: u64,
}

/// Segment registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentRegisters {
    pub cs: SegmentRegister,
    pub ds: SegmentRegister,
    pub es: SegmentRegister,
    pub fs: SegmentRegister,
    pub gs: SegmentRegister,
    pub ss: SegmentRegister,
    pub tr: SegmentRegister,
    pub ldtr: SegmentRegister,
}

/// Descriptor table registers
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorTableRegisters {
    pub gdtr_base: u64,
    pub gdtr_limit: u16,
    pub idtr_base: u64,
    pub idtr_limit: u16,
}

/// Complete vCPU register state
#[derive(Debug, Clone, Default)]
pub struct VcpuRegisters {
    /// General purpose registers
    pub gp: GeneralRegisters,
    /// Control registers
    pub cr: ControlRegisters,
    /// Debug registers
    pub dr: DebugRegisters,
    /// Segment registers
    pub seg: SegmentRegisters,
    /// Descriptor table registers
    pub dt: DescriptorTableRegisters,
}

/// VM exit information
#[derive(Debug, Clone)]
pub struct VmExitInfo {
    /// Exit reason
    pub reason: u32,
    /// Exit qualification
    pub qualification: u64,
    /// Guest physical address (for EPT violations)
    pub guest_physical_addr: Option<u64>,
    /// Guest linear address
    pub guest_linear_addr: Option<u64>,
    /// Instruction length
    pub instruction_length: u32,
    /// Instruction info
    pub instruction_info: u32,
}

/// Virtual CPU
pub struct Vcpu {
    /// vCPU ID
    id: u32,
    /// vCPU state
    state: VcpuState,
    /// CPU vendor (determines VMX vs SVM)
    vendor: CpuVendor,
    /// Guest registers
    registers: VcpuRegisters,
    /// VMX VMCS region (Intel)
    #[cfg(feature = "intel")]
    vmcs: Option<crate::vmx::VmcsRegion>,
    /// SVM VMCB (AMD)
    #[cfg(feature = "amd")]
    vmcb: Option<crate::svm::Vmcb>,
    /// Whether VMLAUNCH has been executed (VMX only)
    launched: bool,
    /// Exit count
    exit_count: AtomicU64,
}

impl Vcpu {
    /// Create a new vCPU
    pub fn new(vendor: CpuVendor) -> Self {
        let id = VCPU_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

        Self {
            id,
            state: VcpuState::Uninitialized,
            vendor,
            registers: VcpuRegisters::default(),
            #[cfg(feature = "intel")]
            vmcs: None,
            #[cfg(feature = "amd")]
            vmcb: None,
            launched: false,
            exit_count: AtomicU64::new(0),
        }
    }

    /// Get the vCPU ID
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the vCPU state
    pub fn state(&self) -> VcpuState {
        self.state
    }

    /// Get the CPU vendor
    pub fn vendor(&self) -> CpuVendor {
        self.vendor
    }

    /// Get the exit count
    pub fn exit_count(&self) -> u64 {
        self.exit_count.load(Ordering::Relaxed)
    }

    /// Initialize the vCPU
    pub fn initialize(&mut self) -> Result<()> {
        match self.vendor {
            #[cfg(feature = "intel")]
            CpuVendor::Intel => {
                self.vmcs = Some(crate::vmx::VmcsRegion::new());
            }
            #[cfg(feature = "amd")]
            CpuVendor::Amd => {
                self.vmcb = Some(crate::svm::Vmcb::new());
            }
            _ => return Err(Error::NoHardwareSupport),
        }

        self.state = VcpuState::Ready;
        Ok(())
    }

    /// Set up the vCPU for real mode boot
    pub fn setup_real_mode(&mut self) {
        // Set up registers for real mode
        self.registers.cr.cr0 = 0x0000_0010; // ET bit set
        self.registers.gp.rflags = 0x0000_0002; // Reserved bit set

        // CS:IP = 0xFFFF:0x0000 (reset vector at 0xFFFF0)
        self.registers.seg.cs.selector = 0xF000;
        self.registers.seg.cs.base = 0xFFFF_0000;
        self.registers.seg.cs.limit = 0xFFFF;
        self.registers.seg.cs.attributes = 0x009B; // Present, code, readable

        self.registers.gp.rip = 0xFFF0;

        // Data segments
        for seg in [
            &mut self.registers.seg.ds,
            &mut self.registers.seg.es,
            &mut self.registers.seg.fs,
            &mut self.registers.seg.gs,
            &mut self.registers.seg.ss,
        ] {
            seg.selector = 0;
            seg.base = 0;
            seg.limit = 0xFFFF;
            seg.attributes = 0x0093; // Present, data, writable
        }

        // GDTR and IDTR
        self.registers.dt.gdtr_base = 0;
        self.registers.dt.gdtr_limit = 0xFFFF;
        self.registers.dt.idtr_base = 0;
        self.registers.dt.idtr_limit = 0xFFFF;
    }

    /// Set up the vCPU for long mode (64-bit)
    pub fn setup_long_mode(&mut self, entry_point: u64, stack_pointer: u64, cr3: u64) {
        // Enable long mode
        self.registers.cr.cr0 = 0x8000_0011; // PG, PE, ET
        self.registers.cr.cr4 = 0x0000_0020; // PAE
        self.registers.cr.efer = 0x0000_0500; // LME, LMA
        self.registers.cr.cr3 = cr3;

        // Set up RIP and RSP
        self.registers.gp.rip = entry_point;
        self.registers.gp.rsp = stack_pointer;
        self.registers.gp.rflags = 0x0000_0002;

        // 64-bit code segment
        self.registers.seg.cs.selector = 0x08;
        self.registers.seg.cs.base = 0;
        self.registers.seg.cs.limit = 0xFFFF_FFFF;
        self.registers.seg.cs.attributes = 0xA09B; // Long mode, code, readable

        // Data segments
        for seg in [
            &mut self.registers.seg.ds,
            &mut self.registers.seg.es,
            &mut self.registers.seg.fs,
            &mut self.registers.seg.gs,
            &mut self.registers.seg.ss,
        ] {
            seg.selector = 0x10;
            seg.base = 0;
            seg.limit = 0xFFFF_FFFF;
            seg.attributes = 0xC093; // Data, writable
        }
    }

    /// Get the guest registers
    pub fn registers(&self) -> &VcpuRegisters {
        &self.registers
    }

    /// Get mutable access to guest registers
    pub fn registers_mut(&mut self) -> &mut VcpuRegisters {
        &mut self.registers
    }

    /// Configure VMX VMCS for this vCPU (call once after initialize, before first run).
    ///
    /// # Safety
    /// Requires VMX to be enabled on this CPU.
    #[cfg(feature = "intel")]
    pub unsafe fn setup_vmx(&mut self, ept_pointer: u64) -> Result<()> {
        let vmcs = self.vmcs.as_ref().ok_or(Error::InvalidState)?;

        // Clear and load the VMCS
        crate::vmx::vmclear(vmcs)?;
        crate::vmx::vmptrld(vmcs)?;

        // Set up execution controls
        crate::vmx::setup_vmcs_controls(ept_pointer)?;

        // Write guest state
        crate::vmx::setup_vmcs_guest_state(&self.registers)?;

        self.launched = false;
        Ok(())
    }

    /// Configure SVM VMCB for this vCPU (call once after initialize, before first run).
    #[cfg(feature = "amd")]
    pub fn setup_svm(&mut self, ncr3: u64, asid: u32) -> Result<()> {
        let vmcb = self.vmcb.as_mut().ok_or(Error::InvalidState)?;
        crate::svm::setup_vmcb_controls(vmcb, ncr3, asid);
        crate::svm::setup_vmcb_guest_state(vmcb, &self.registers);
        Ok(())
    }

    /// Run the vCPU (VM entry).
    ///
    /// Performs a full VM-entry → guest execution → VM-exit cycle and returns
    /// the exit information. Guest GP registers in `self.registers` are
    /// updated with the state at the time of the VM exit.
    ///
    /// # Safety
    /// The vCPU must be fully set up (`setup_vmx`/`setup_svm` called).
    pub unsafe fn run(&mut self) -> Result<VmExitInfo> {
        if self.state != VcpuState::Ready {
            return Err(Error::InvalidState);
        }

        self.state = VcpuState::Running;

        let exit_info = match self.vendor {
            #[cfg(feature = "intel")]
            CpuVendor::Intel => self.run_vmx()?,
            #[cfg(feature = "amd")]
            CpuVendor::Amd => self.run_svm()?,
            _ => return Err(Error::NoHardwareSupport),
        };

        self.exit_count.fetch_add(1, Ordering::Relaxed);
        self.state = VcpuState::Ready;

        Ok(exit_info)
    }

    /// VMX VM-entry → exit cycle.
    #[cfg(feature = "intel")]
    unsafe fn run_vmx(&mut self) -> Result<VmExitInfo> {
        let vmcs = self.vmcs.as_ref().ok_or(Error::VmlaunchFailed)?;

        // Sync guest RIP/RSP/RFLAGS/CRx to VMCS
        crate::vmx::sync_guest_state_to_vmcs(&self.registers)?;

        // Execute VM entry with full GP register save/restore
        crate::vmx::vmx_run_with_exit(vmcs, &mut self.registers.gp, self.launched)?;

        // After a successful launch, all future entries use VMRESUME
        self.launched = true;

        // Read back guest state from VMCS
        crate::vmx::sync_guest_state_from_vmcs(&mut self.registers)?;

        // Read exit info
        crate::vmx::read_exit_info()
    }

    /// SVM VMRUN → exit cycle.
    #[cfg(feature = "amd")]
    unsafe fn run_svm(&mut self) -> Result<VmExitInfo> {
        let vmcb = self.vmcb.as_mut().ok_or(Error::VmrunFailed)?;

        // Sync guest state to VMCB save area
        crate::svm::setup_vmcb_guest_state(vmcb, &self.registers);

        // Execute VMRUN with full GP register save/restore
        crate::svm::svm_run(vmcb, &mut self.registers.gp)?;

        // Read back guest state from VMCB
        crate::svm::sync_vmcb_to_registers(vmcb, &mut self.registers);

        // Read exit info from VMCB control area
        Ok(crate::svm::read_exit_info(vmcb))
    }

    /// Inject an interrupt into the guest.
    ///
    /// For VMX: writes the VM-entry interruption-info field.
    /// For SVM: writes the VMCB event_inject field.
    ///
    /// # Safety
    /// For VMX, a VMCS must be current.
    pub unsafe fn inject_interrupt(&mut self, vector: u8) -> Result<()> {
        match self.vendor {
            #[cfg(feature = "intel")]
            CpuVendor::Intel => {
                crate::vmx::inject_interrupt(vector, false)?;
            }
            #[cfg(feature = "amd")]
            CpuVendor::Amd => {
                let vmcb = self.vmcb.as_mut().ok_or(Error::InterruptInjectionFailed)?;
                crate::svm::inject_interrupt(vmcb, vector, false);
            }
            _ => return Err(Error::NoHardwareSupport),
        }
        Ok(())
    }

    /// Inject a hardware exception into the guest.
    ///
    /// # Safety
    /// For VMX, a VMCS must be current.
    pub unsafe fn inject_exception(&mut self, vector: u8, error_code: Option<u32>) -> Result<()> {
        match self.vendor {
            #[cfg(feature = "intel")]
            CpuVendor::Intel => {
                crate::vmx::inject_exception(vector, error_code)?;
            }
            #[cfg(feature = "amd")]
            CpuVendor::Amd => {
                let vmcb = self.vmcb.as_mut().ok_or(Error::InterruptInjectionFailed)?;
                crate::svm::inject_exception(vmcb, vector, error_code);
            }
            _ => return Err(Error::NoHardwareSupport),
        }
        Ok(())
    }

    /// Get a reference to the VMX VMCS region.
    #[cfg(feature = "intel")]
    pub fn vmcs(&self) -> Option<&crate::vmx::VmcsRegion> {
        self.vmcs.as_ref()
    }

    /// Get a reference to the SVM VMCB.
    #[cfg(feature = "amd")]
    pub fn vmcb(&self) -> Option<&crate::svm::Vmcb> {
        self.vmcb.as_ref()
    }

    /// Get a mutable reference to the SVM VMCB.
    #[cfg(feature = "amd")]
    pub fn vmcb_mut(&mut self) -> Option<&mut crate::svm::Vmcb> {
        self.vmcb.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Register defaults ---

    #[test]
    fn general_registers_default() {
        let gp = GeneralRegisters::default();
        assert_eq!(gp.rax, 0);
        assert_eq!(gp.rip, 0);
        assert_eq!(gp.rflags, 0);
        assert_eq!(gp.r15, 0);
    }

    #[test]
    fn control_registers_default() {
        let cr = ControlRegisters::default();
        assert_eq!(cr.cr0, 0);
        assert_eq!(cr.cr3, 0);
        assert_eq!(cr.efer, 0);
    }

    #[test]
    fn vcpu_registers_default() {
        let regs = VcpuRegisters::default();
        assert_eq!(regs.gp.rax, 0);
        assert_eq!(regs.cr.cr0, 0);
        assert_eq!(regs.seg.cs.selector, 0);
        assert_eq!(regs.dt.gdtr_limit, 0);
    }

    // --- Vcpu creation ---

    #[test]
    fn vcpu_new_is_uninitialized() {
        let vcpu = Vcpu::new(CpuVendor::Intel);
        assert_eq!(vcpu.state(), VcpuState::Uninitialized);
        assert_eq!(vcpu.vendor(), CpuVendor::Intel);
        assert_eq!(vcpu.exit_count(), 0);
    }

    #[test]
    fn vcpu_ids_increment() {
        let v1 = Vcpu::new(CpuVendor::Amd);
        let v2 = Vcpu::new(CpuVendor::Amd);
        assert!(v2.id() > v1.id());
    }

    // --- Setup real mode ---

    #[test]
    fn vcpu_setup_real_mode() {
        let mut vcpu = Vcpu::new(CpuVendor::Intel);
        vcpu.setup_real_mode();

        let regs = vcpu.registers();
        assert_eq!(regs.cr.cr0, 0x10); // ET bit
        assert_eq!(regs.gp.rflags, 0x02); // Reserved bit
        assert_eq!(regs.seg.cs.selector, 0xF000);
        assert_eq!(regs.seg.cs.base, 0xFFFF_0000);
        assert_eq!(regs.seg.cs.limit, 0xFFFF);
        assert_eq!(regs.seg.cs.attributes, 0x009B);
        assert_eq!(regs.gp.rip, 0xFFF0);

        // Data segments
        assert_eq!(regs.seg.ds.selector, 0);
        assert_eq!(regs.seg.ds.base, 0);
        assert_eq!(regs.seg.ds.limit, 0xFFFF);
        assert_eq!(regs.seg.ds.attributes, 0x0093);
        assert_eq!(regs.seg.ss.attributes, 0x0093);

        // Descriptor tables
        assert_eq!(regs.dt.gdtr_limit, 0xFFFF);
        assert_eq!(regs.dt.idtr_limit, 0xFFFF);
    }

    // --- Setup long mode ---

    #[test]
    fn vcpu_setup_long_mode() {
        let mut vcpu = Vcpu::new(CpuVendor::Intel);
        vcpu.setup_long_mode(0xDEAD_0000, 0xBEEF_0000, 0xCAFE_0000);

        let regs = vcpu.registers();
        assert_eq!(regs.gp.rip, 0xDEAD_0000);
        assert_eq!(regs.gp.rsp, 0xBEEF_0000);
        assert_eq!(regs.cr.cr3, 0xCAFE_0000);
        assert_eq!(regs.cr.cr0, 0x8000_0011); // PG + PE + ET
        assert_eq!(regs.cr.cr4, 0x20); // PAE
        assert_eq!(regs.cr.efer, 0x500); // LME + LMA
        assert_eq!(regs.gp.rflags, 0x02);

        // Code segment: 64-bit
        assert_eq!(regs.seg.cs.selector, 0x08);
        assert_eq!(regs.seg.cs.attributes, 0xA09B);
        assert_eq!(regs.seg.cs.limit, 0xFFFF_FFFF);

        // Data segments
        assert_eq!(regs.seg.ds.selector, 0x10);
        assert_eq!(regs.seg.ds.attributes, 0xC093);
    }

    // --- Registers access ---

    #[test]
    fn vcpu_registers_mut() {
        let mut vcpu = Vcpu::new(CpuVendor::Intel);
        vcpu.registers_mut().gp.rax = 0x42;
        assert_eq!(vcpu.registers().gp.rax, 0x42);
    }

    // --- VcpuState ---

    #[test]
    fn vcpu_state_equality() {
        assert_eq!(VcpuState::Ready, VcpuState::Ready);
        assert_ne!(VcpuState::Running, VcpuState::Halted);
    }

    // --- VmExitInfo ---

    #[test]
    fn vm_exit_info_construction() {
        let info = VmExitInfo {
            reason: 48,
            qualification: 0x1234,
            guest_physical_addr: Some(0xDEAD_0000),
            guest_linear_addr: None,
            instruction_length: 3,
            instruction_info: 0,
        };
        assert_eq!(info.reason, 48);
        assert_eq!(info.guest_physical_addr, Some(0xDEAD_0000));
        assert!(info.guest_linear_addr.is_none());
    }

    // --- SegmentRegister ---

    #[test]
    fn segment_register_default() {
        let seg = SegmentRegister::default();
        assert_eq!(seg.selector, 0);
        assert_eq!(seg.attributes, 0);
        assert_eq!(seg.limit, 0);
        assert_eq!(seg.base, 0);
    }
}
