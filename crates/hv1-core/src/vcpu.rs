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
                // Additional VMCS setup would go here
            }
            #[cfg(feature = "amd")]
            CpuVendor::Amd => {
                self.vmcb = Some(crate::svm::Vmcb::new());
                // Additional VMCB setup would go here
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

    /// Run the vCPU (VM entry)
    /// 
    /// # Safety
    /// This function performs VM entry which has significant side effects.
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

    #[cfg(feature = "intel")]
    unsafe fn run_vmx(&mut self) -> Result<VmExitInfo> {
        // Sync registers to VMCS
        // Execute VMLAUNCH/VMRESUME
        // Sync registers from VMCS
        // Return exit info
        
        // Placeholder - actual implementation would use vmx module
        Err(Error::VmlaunchFailed)
    }

    #[cfg(feature = "amd")]
    unsafe fn run_svm(&mut self) -> Result<VmExitInfo> {
        // Sync registers to VMCB
        // Execute VMRUN
        // Sync registers from VMCB
        // Return exit info
        
        // Placeholder - actual implementation would use svm module
        Err(Error::VmrunFailed)
    }

    /// Inject an interrupt into the guest
    pub fn inject_interrupt(&mut self, vector: u8) -> Result<()> {
        match self.vendor {
            #[cfg(feature = "intel")]
            CpuVendor::Intel => {
                // Set up VM-entry interruption-information field
            }
            #[cfg(feature = "amd")]
            CpuVendor::Amd => {
                // Set up VMCB event injection
            }
            _ => return Err(Error::NoHardwareSupport),
        }
        Ok(())
    }
}
