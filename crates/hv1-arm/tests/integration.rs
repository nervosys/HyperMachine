//! Cross-module integration tests for hv1-arm
//!
//! These tests exercise full VM lifecycle flows spanning multiple modules:
//! VM creation -> stage-2 mapping -> vCPU init -> exit handling -> sysreg emulation.

// ESR/ISS bit-pattern constructions below intentionally include explicit
// zero-shift terms (e.g. `(0 << 17)` and trailing `| 0`) to document the
// architectural field layout; allow clippy::identity_op here.
#![allow(clippy::identity_op)]

extern crate alloc;

use hv1_arm::el2::{self, HcrEl2, TrapReason};
use hv1_arm::stage2::{Stage2Attrs, Stage2Mapping, PAGE_SIZE};
use hv1_arm::sysreg::SysregId;
use hv1_arm::vcpu::VcpuState;
use hv1_arm::vgic::InterruptState;
use hv1_arm::vm::{Vm, VmState};
use hv1_arm::Error;

// ============================================================================
// Full VM lifecycle
// ============================================================================

#[test]
fn vm_lifecycle_create_init_map_run() {
    let mut vm = Vm::new(2).expect("2-vCPU VM");
    assert_eq!(vm.state(), VmState::Created);
    assert_eq!(vm.num_vcpus(), 2);

    // Map a 4 KB guest page
    let mapping = Stage2Mapping {
        ipa: 0x4000_0000,
        hpa: 0x8000_0000,
        size: PAGE_SIZE as u64,
        attrs: Stage2Attrs::VALID
            | Stage2Attrs::TABLE_OR_PAGE
            | Stage2Attrs::S2AP_READ
            | Stage2Attrs::S2AP_WRITE,
    };
    vm.map_memory(mapping).expect("stage2 map");

    // Initialize the VM (transitions Created -> Initialized, inits GIC)
    vm.initialize().expect("init");
    assert_eq!(vm.state(), VmState::Initialized);

    // Init each vCPU with distinct entry points
    vm.init_vcpu(0, 0x4000_0000, 0x4001_0000).expect("vcpu0");
    vm.init_vcpu(1, 0x4000_0000, 0x4002_0000).expect("vcpu1");

    // vCPUs should be Ready after initialization
    assert_eq!(vm.vcpu(0).unwrap().state(), VcpuState::Ready);
    assert_eq!(vm.vcpu(1).unwrap().state(), VcpuState::Ready);
}

#[test]
fn vm_double_init_is_rejected() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("first init");
    assert!(matches!(vm.initialize(), Err(Error::InvalidGuestState)));
}

// ============================================================================
// Stage-2 + VM integration
// ============================================================================

#[test]
fn vm_multiple_memory_regions() {
    let mut vm = Vm::new(1).expect("VM");

    // Map RAM region
    let ram = Stage2Mapping {
        ipa: 0x4000_0000,
        hpa: 0x8000_0000,
        size: PAGE_SIZE as u64,
        attrs: Stage2Attrs::VALID
            | Stage2Attrs::TABLE_OR_PAGE
            | Stage2Attrs::S2AP_READ
            | Stage2Attrs::S2AP_WRITE,
    };
    vm.map_memory(ram).expect("RAM map");

    // Map read-only firmware region
    let firmware = Stage2Mapping {
        ipa: 0x0000_0000,
        hpa: 0x1000_0000,
        size: PAGE_SIZE as u64,
        attrs: Stage2Attrs::VALID | Stage2Attrs::TABLE_OR_PAGE | Stage2Attrs::S2AP_READ,
    };
    vm.map_memory(firmware).expect("firmware map");

    // Verify both regions are tracked
    assert_eq!(vm.stage2.mapping_count(), 2);
}

// ============================================================================
// Exit handling: sysreg traps
// ============================================================================

#[test]
fn handle_exit_sysreg_read_advances_pc() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");

    // Put vCPU into Running state so exit handling works naturally
    vm.vcpu_mut(0).unwrap().enter().expect("enter");

    let pc_before = vm.vcpu(0).unwrap().sysregs.elr_el2;

    // Construct ESR for EC=0x18 (MSR/MRS trap)
    // ISS encoding for MIDR_EL1 (Op0=3, Op1=0, CRn=0, CRm=0, Op2=0), Rt=1, direction=1 (read)
    // Bit layout: Op0[21:20]=3, Op2[19:17]=0, Op1[16:14]=0, CRn[13:10]=0,
    //             Rt[9:5]=1, CRm[4:1]=0, direction[0]=1
    let ec: u64 = 0x18 << 26;
    let iss: u64 = (3 << 20) | (0 << 17) | (0 << 14) | (0 << 10) | (1 << 5) | (0 << 1) | 1;
    let esr = ec | iss;

    let should_continue = vm.handle_exit(0, esr).expect("handle sysreg trap");
    assert!(should_continue, "VM should continue after sysreg emulation");

    let pc_after = vm.vcpu(0).unwrap().sysregs.elr_el2;
    assert_eq!(pc_after, pc_before + 4, "PC should advance by 4");
}

// ============================================================================
// Exit handling: WFI/WFE traps
// ============================================================================

#[test]
fn handle_exit_wfi_halts_vcpu() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");

    // Put vCPU into Running so halt() transitions to Halted
    vm.vcpu_mut(0).unwrap().enter().expect("enter");

    // EC=0x01 (WF* trap), ISS bit 0 = 0 (WFI)
    let esr: u64 = (0x01 << 26) | 0;
    let should_continue = vm.handle_exit(0, esr).expect("handle WFI");
    assert!(should_continue);
    assert_eq!(vm.vcpu(0).unwrap().state(), VcpuState::Halted);
}

#[test]
fn handle_exit_wfe_does_not_halt() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");

    // Put vCPU into Running
    vm.vcpu_mut(0).unwrap().enter().expect("enter");

    // EC=0x01 (WF* trap), ISS bit 0 = 1 (WFE)
    let esr: u64 = (0x01 << 26) | 1;
    let should_continue = vm.handle_exit(0, esr).expect("handle WFE");
    assert!(should_continue);
    // WFE should NOT halt the vCPU; it remains Running
    assert_eq!(vm.vcpu(0).unwrap().state(), VcpuState::Running);
}

// ============================================================================
// Exit handling: HVC / SMC traps
// ============================================================================

#[test]
fn handle_exit_hvc_continues() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");
    vm.vcpu_mut(0).unwrap().enter().expect("enter");

    let pc_before = vm.vcpu(0).unwrap().sysregs.elr_el2;

    // EC=0x16 (HVC from AArch64), imm16=0x42
    let esr: u64 = (0x16 << 26) | 0x42;
    let should_continue = vm.handle_exit(0, esr).expect("handle HVC");
    assert!(should_continue);
    assert_eq!(vm.vcpu(0).unwrap().sysregs.elr_el2, pc_before + 4);
}

#[test]
fn handle_exit_smc_continues() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");
    vm.vcpu_mut(0).unwrap().enter().expect("enter");

    let pc_before = vm.vcpu(0).unwrap().sysregs.elr_el2;

    // EC=0x17 (SMC from AArch64), imm16=0
    let esr: u64 = (0x17 << 26) | 0;
    let should_continue = vm.handle_exit(0, esr).expect("handle SMC");
    assert!(should_continue);
    assert_eq!(vm.vcpu(0).unwrap().sysregs.elr_el2, pc_before + 4);
}

// ============================================================================
// Exit handling: fatal traps
// ============================================================================

#[test]
fn handle_exit_serror_stops_vm() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");

    // EC=0x2F (SError)
    let esr: u64 = 0x2F << 26;
    let should_continue = vm.handle_exit(0, esr).expect("handle SError");
    assert!(!should_continue, "SError should stop VM");
}

#[test]
fn handle_exit_unknown_ec_stops_vm() {
    let mut vm = Vm::new(1).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu");

    // EC=0x3F (undefined in spec)
    let esr: u64 = 0x3F << 26;
    let should_continue = vm.handle_exit(0, esr).expect("handle unknown trap");
    assert!(!should_continue, "unknown EC should stop VM");
}

// ============================================================================
// vGIC + VM integration
// ============================================================================

#[test]
fn vm_inject_interrupt_via_gic() {
    let mut vm = Vm::new(2).expect("VM");
    vm.initialize().expect("init");

    // Enable SPI #33 then set it pending via the distributor
    vm.gic.distributor.enable_irq(33).expect("enable irq 33");
    vm.gic.distributor.set_pending(33).expect("set pending 33");

    let cfg = vm.gic.distributor.irq_config(33).expect("irq config");
    assert_eq!(cfg.state, InterruptState::Pending);
}

#[test]
fn vm_gic_respects_vcpu_count() {
    let mut vm = Vm::new(4).expect("VM");
    vm.initialize().expect("init");

    // SGI from CPU 0 to each of the 4 CPUs
    for cpu in 0..4u8 {
        vm.gic.inject_sgi(0, cpu, 0).expect("SGI to valid CPU");
    }
}

// ============================================================================
// Error paths
// ============================================================================

#[test]
fn vm_zero_vcpus_rejected() {
    assert!(matches!(Vm::new(0), Err(Error::InvalidParameter)));
}

#[test]
fn vm_vcpu_oob_rejected() {
    let vm = Vm::new(1).expect("VM");
    assert!(matches!(vm.vcpu(1), Err(Error::InvalidParameter)));
}

#[test]
fn vm_init_vcpu_before_initialize_ok() {
    // init_vcpu should work in Created state since it is just setting registers
    let mut vm = Vm::new(1).expect("VM");
    vm.init_vcpu(0, 0x1000, 0x2000)
        .expect("init_vcpu in Created state");
}

// ============================================================================
// Multi-vCPU coordination
// ============================================================================

#[test]
fn multi_vcpu_independent_register_state() {
    let mut vm = Vm::new(4).expect("VM");
    vm.initialize().expect("init");

    // Give each vCPU a different entry point and stack
    for i in 0..4 {
        let entry = 0x1_0000 * (i as u64 + 1);
        let stack = 0x2_0000 * (i as u64 + 1);
        vm.init_vcpu(i, entry, stack).expect("init_vcpu");
    }

    // Verify each vCPU got independent state
    for i in 0..4 {
        let vcpu = vm.vcpu(i).unwrap();
        let expected_entry = 0x1_0000 * (i as u64 + 1);
        let expected_stack = 0x2_0000 * (i as u64 + 1);
        assert_eq!(vcpu.sysregs.elr_el2, expected_entry);
        assert_eq!(vcpu.sysregs.sp_el1, expected_stack);
    }
}

#[test]
fn multi_vcpu_independent_exit_handling() {
    let mut vm = Vm::new(2).expect("VM");
    vm.initialize().expect("init");
    vm.init_vcpu(0, 0x1000, 0x2000).expect("vcpu0");
    vm.init_vcpu(1, 0x3000, 0x4000).expect("vcpu1");

    // Put both vCPUs into Running state
    vm.vcpu_mut(0).unwrap().enter().expect("enter vcpu0");
    vm.vcpu_mut(1).unwrap().enter().expect("enter vcpu1");

    // WFI on vCPU 0 -- should halt only vCPU 0
    let wfi: u64 = (0x01 << 26) | 0;
    vm.handle_exit(0, wfi).expect("WFI on vcpu0");
    assert_eq!(vm.vcpu(0).unwrap().state(), VcpuState::Halted);
    assert_eq!(vm.vcpu(1).unwrap().state(), VcpuState::Running);
}

// ============================================================================
// EL2 configuration
// ============================================================================

#[test]
fn hcr_el2_hypervisor_default_flags() {
    let hcr = HcrEl2::hypervisor_default();
    // Must trap interrupts to EL2
    assert!(hcr.contains(HcrEl2::IMO));
    assert!(hcr.contains(HcrEl2::FMO));
    assert!(hcr.contains(HcrEl2::AMO));
    // Must enable stage-2 translation
    assert!(hcr.contains(HcrEl2::VM));
    // Must set EL1 to AArch64
    assert!(hcr.contains(HcrEl2::RW));
}

#[test]
fn trap_decode_roundtrip() {
    // Encode a system register trap and verify decode
    let ec: u64 = 0x18 << 26;
    let iss: u64 = (3 << 20) | (0 << 17) | (0 << 14) | (0 << 10) | (0 << 1) | 1; // MIDR_EL1 read
    let esr = ec | iss;

    let trap = el2::decode_trap(esr);
    assert!(matches!(
        trap,
        TrapReason::SystemRegisterAccess {
            is_write: false,
            ..
        }
    ));
}

// ============================================================================
// Stage-2 page table isolation
// ============================================================================

#[test]
fn separate_vms_get_distinct_vmids() {
    let vm1 = Vm::new(1).expect("VM1");
    let vm2 = Vm::new(1).expect("VM2");
    assert_ne!(vm1.vmid(), vm2.vmid(), "VMIDs must be unique");
}

#[test]
fn separate_vms_have_independent_page_tables() {
    let mut vm1 = Vm::new(1).expect("VM1");
    let mut vm2 = Vm::new(1).expect("VM2");

    // Map different HPA for the same IPA in each VM
    let m1 = Stage2Mapping {
        ipa: 0x4000_0000,
        hpa: 0x8000_0000,
        size: PAGE_SIZE as u64,
        attrs: Stage2Attrs::VALID | Stage2Attrs::TABLE_OR_PAGE | Stage2Attrs::S2AP_READ,
    };
    let m2 = Stage2Mapping {
        ipa: 0x4000_0000,
        hpa: 0xA000_0000,
        size: PAGE_SIZE as u64,
        attrs: Stage2Attrs::VALID | Stage2Attrs::TABLE_OR_PAGE | Stage2Attrs::S2AP_READ,
    };

    vm1.map_memory(m1).expect("VM1 map");
    vm2.map_memory(m2).expect("VM2 map");

    // Each VM has its own VMID and independent page table
    assert_ne!(vm1.stage2.vmid(), vm2.stage2.vmid());
}

// ============================================================================
// Sysreg module
// ============================================================================

#[test]
fn sysreg_id_decode_encode_roundtrip() {
    // SCTLR_EL1: Op0=3, Op1=0, CRn=1, CRm=0, Op2=0
    let original = SysregId::new(3, 0, 1, 0, 0);
    let packed = original.as_u32();
    let decoded = SysregId::from_iss(packed);
    assert_eq!(original, decoded);
}
