//! ARM64 VM (Virtual Machine) management
//!
//! Ties together vCPUs, stage-2 page tables, and the virtual GIC into
//! a coherent VM abstraction.

use crate::el2::{self, TrapReason};
use crate::stage2::{Stage2Mapping, Stage2PageTable};
use crate::sysreg::{self, EmulationResult, SysregTrap};
use crate::vcpu::{Vcpu, VcpuState};
use crate::vgic::VirtualGic;
use crate::{Error, Result};

use core::sync::atomic::{AtomicU16, Ordering};

/// VM ID counter (monotonically increasing, wraps as VMID width allows).
static VM_ID_COUNTER: AtomicU16 = AtomicU16::new(1);

/// Maximum vCPUs per VM.
pub const MAX_VCPUS: usize = 256;

/// VM lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM has been created but not yet initialized
    Created,
    /// VM is initialized and ready to run
    Initialized,
    /// VM is currently running (at least one vCPU active)
    Running,
    /// VM is paused
    Paused,
    /// VM has been shut down
    ShutDown,
}

/// An ARM64 virtual machine.
#[derive(Debug)]
pub struct Vm {
    /// VMID — used in VTTBR_EL2 and TLB tags
    vmid: u16,
    /// Lifecycle state
    state: VmState,
    /// Stage-2 page table
    pub stage2: Stage2PageTable,
    /// Virtual GIC
    pub gic: VirtualGic,
    /// vCPUs
    vcpus: alloc::vec::Vec<Vcpu>,
}

impl Vm {
    /// Create a new VM with the given number of vCPUs.
    pub fn new(num_vcpus: u8) -> Result<Self> {
        if num_vcpus == 0 || num_vcpus as usize > MAX_VCPUS {
            return Err(Error::InvalidParameter);
        }

        let vmid = VM_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let gic = VirtualGic::new(num_vcpus)?;
        let stage2 = Stage2PageTable::new(vmid);

        let mut vcpus = alloc::vec::Vec::new();
        for _ in 0..num_vcpus {
            vcpus.push(Vcpu::new(vmid));
        }

        Ok(Self {
            vmid,
            state: VmState::Created,
            stage2,
            gic,
            vcpus,
        })
    }

    /// Get the VMID.
    pub fn vmid(&self) -> u16 {
        self.vmid
    }

    /// Get the current VM state.
    pub fn state(&self) -> VmState {
        self.state
    }

    /// Number of vCPUs.
    pub fn num_vcpus(&self) -> usize {
        self.vcpus.len()
    }

    /// Get a reference to a vCPU by index.
    pub fn vcpu(&self, index: usize) -> Result<&Vcpu> {
        self.vcpus.get(index).ok_or(Error::InvalidParameter)
    }

    /// Get a mutable reference to a vCPU by index.
    pub fn vcpu_mut(&mut self, index: usize) -> Result<&mut Vcpu> {
        self.vcpus.get_mut(index).ok_or(Error::InvalidParameter)
    }

    /// Initialize the VM: initialize the GIC and transition to Initialized.
    pub fn initialize(&mut self) -> Result<()> {
        if self.state != VmState::Created {
            return Err(Error::InvalidGuestState);
        }
        self.gic.initialize()?;
        self.state = VmState::Initialized;
        Ok(())
    }

    /// Map a guest IPA region in stage-2 page tables.
    pub fn map_memory(&mut self, mapping: Stage2Mapping) -> Result<()> {
        self.stage2.map_region(mapping)
    }

    /// Initialize a vCPU with an entry point and stack.
    pub fn init_vcpu(
        &mut self,
        vcpu_index: usize,
        entry_point: u64,
        stack_pointer: u64,
    ) -> Result<()> {
        self.vcpu_mut(vcpu_index)?
            .initialize(entry_point, stack_pointer)
    }

    /// Handle a VM exit (trap from EL1 to EL2) for the given vCPU.
    ///
    /// Returns `true` if the VM should continue running, `false` to stop.
    pub fn handle_exit(&mut self, vcpu_index: usize, esr: u64) -> Result<bool> {
        let trap = el2::decode_trap(esr);

        match trap {
            TrapReason::SystemRegisterAccess { iss, is_write } => {
                let sysreg_trap = SysregTrap::from_iss(iss);
                let vcpu = self.vcpu_mut(vcpu_index)?;
                match sysreg::emulate_sysreg(vcpu, &sysreg_trap)? {
                    EmulationResult::Handled => {
                        vcpu.advance_pc();
                        Ok(true)
                    }
                    EmulationResult::Unhandled => {
                        // Inject undefined exception into guest
                        Ok(true)
                    }
                }
            }
            TrapReason::HypervisorCall { imm } => {
                let vcpu = self.vcpu_mut(vcpu_index)?;
                // PSCI-like calls could be handled here
                // For now, advance PC and continue
                vcpu.advance_pc();
                Ok(true)
            }
            TrapReason::SecureMonitorCall { imm } => {
                let vcpu = self.vcpu_mut(vcpu_index)?;
                // SMC trapped because HCR_EL2.TSC=1; forward to secure monitor
                // or handle PSCI
                vcpu.advance_pc();
                Ok(true)
            }
            TrapReason::WaitForEvent { is_wfe } => {
                let vcpu = self.vcpu_mut(vcpu_index)?;
                if !is_wfe {
                    // WFI — halt vCPU until next interrupt
                    vcpu.halt();
                }
                vcpu.advance_pc();
                Ok(true)
            }
            TrapReason::DataAbort {
                ipa,
                is_write,
                access_size,
                srt,
                ..
            } => {
                // Could be MMIO — check if the IPA falls in a device region
                // For now, report as unhandled
                Ok(true)
            }
            TrapReason::InstructionAbort { .. } => {
                // Stage-2 instruction abort — could be demand paging
                Ok(true)
            }
            TrapReason::Interrupt => {
                // Physical interrupt routed to EL2 — handle in host
                Ok(true)
            }
            TrapReason::SError { .. } => {
                // SError — typically fatal
                Ok(false)
            }
            TrapReason::Unknown { esr } => {
                // Unknown trap — stop VM
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage2::Stage2Attrs;

    #[test]
    fn vm_creation() {
        let vm = Vm::new(4).unwrap();
        assert_eq!(vm.num_vcpus(), 4);
        assert_eq!(vm.state(), VmState::Created);
    }

    #[test]
    fn vm_zero_vcpus_fails() {
        assert!(matches!(Vm::new(0), Err(Error::InvalidParameter)));
    }

    #[test]
    fn vm_initialize() {
        let mut vm = Vm::new(2).unwrap();
        assert!(vm.initialize().is_ok());
        assert_eq!(vm.state(), VmState::Initialized);
    }

    #[test]
    fn vm_double_initialize_fails() {
        let mut vm = Vm::new(1).unwrap();
        vm.initialize().unwrap();
        assert_eq!(vm.initialize(), Err(Error::InvalidGuestState));
    }

    #[test]
    fn vm_map_memory() {
        let mut vm = Vm::new(1).unwrap();
        vm.initialize().unwrap();
        let mapping = Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0x10_0000,
            attrs: Stage2Attrs::normal_ram(),
        };
        assert!(vm.map_memory(mapping).is_ok());
        assert_eq!(vm.stage2.mapping_count(), 1);
    }

    #[test]
    fn vm_init_vcpu() {
        let mut vm = Vm::new(2).unwrap();
        vm.initialize().unwrap();
        assert!(vm.init_vcpu(0, 0x8_0000, 0x10_0000).is_ok());
        assert_eq!(vm.vcpu(0).unwrap().state(), VcpuState::Ready);
    }

    #[test]
    fn vm_vcpu_out_of_range() {
        let vm = Vm::new(1).unwrap();
        assert!(matches!(vm.vcpu(1), Err(Error::InvalidParameter)));
    }

    #[test]
    fn vm_handle_wfi_exit() {
        let mut vm = Vm::new(1).unwrap();
        vm.initialize().unwrap();
        vm.init_vcpu(0, 0x8_0000, 0x10_0000).unwrap();
        vm.vcpu_mut(0).unwrap().enter().unwrap();

        // ESR for WFI: EC=0x01, ISS bit[0]=0 (WFI)
        let esr = 0x01u64 << 26;
        let cont = vm.handle_exit(0, esr).unwrap();
        assert!(cont);
        assert_eq!(vm.vcpu(0).unwrap().state(), VcpuState::Halted);
    }

    #[test]
    fn vm_handle_hvc_exit() {
        let mut vm = Vm::new(1).unwrap();
        vm.initialize().unwrap();
        vm.init_vcpu(0, 0x8_0000, 0x10_0000).unwrap();
        vm.vcpu_mut(0).unwrap().enter().unwrap();

        // ESR for HVC64: EC=0x16, imm=0x42
        let esr = (0x16u64 << 26) | 0x42;
        let cont = vm.handle_exit(0, esr).unwrap();
        assert!(cont);
    }

    #[test]
    fn vm_handle_unknown_exit_stops() {
        let mut vm = Vm::new(1).unwrap();
        vm.initialize().unwrap();
        vm.init_vcpu(0, 0x8_0000, 0x10_0000).unwrap();
        vm.vcpu_mut(0).unwrap().enter().unwrap();

        // Unknown EC = 0x3F
        let esr = 0x3Fu64 << 26;
        let cont = vm.handle_exit(0, esr).unwrap();
        assert!(!cont);
    }

    #[test]
    fn vm_handle_serror_stops() {
        let mut vm = Vm::new(1).unwrap();
        vm.initialize().unwrap();
        vm.init_vcpu(0, 0x8_0000, 0x10_0000).unwrap();
        vm.vcpu_mut(0).unwrap().enter().unwrap();

        // EC=0x2F (SError)
        let esr = 0x2Fu64 << 26;
        let cont = vm.handle_exit(0, esr).unwrap();
        assert!(!cont);
    }

    #[test]
    fn vm_vmid_is_unique() {
        let vm1 = Vm::new(1).unwrap();
        let vm2 = Vm::new(1).unwrap();
        assert_ne!(vm1.vmid(), vm2.vmid());
    }
}
