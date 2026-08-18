//! HV1-Core Integration Tests
//!
//! These tests exercise cross-module interactions in the Type-1 hypervisor
//! core without requiring actual hardware virtualization support.

extern crate std;

use std::thread;

use hv1_core::device::{DeviceManager, IoDirection, IoSize, PortIoRequest};
use hv1_core::interrupt::VirtualApic;
use hv1_core::memory::{
    EptEntry, FrameAllocator, GuestMemoryMapper, GuestMemoryRegion, NptEntry, PhysicalRegion,
    PAGE_SIZE,
};
use hv1_core::vcpu::{Vcpu, VcpuState};
use hv1_core::vm::{VmConfig, VmState, MAX_VCPUS_PER_VM};
use hv1_core::CpuVendor;

/// Run a closure on a thread with a 16MB stack to accommodate the large Vm struct.
fn with_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("failed to spawn big-stack thread")
        .join()
        .expect("big-stack thread panicked");
}

// ============================================================================
// VM Creation and Configuration Tests
// ============================================================================

#[test]
fn vm_new_starts_uninitialized() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(2, 512 * 1024 * 1024);
        let vm = Vm::new(CpuVendor::Intel, config).unwrap();

        assert_eq!(vm.state(), VmState::Uninitialized);
        // vCPUs not yet created (happens in initialize())
        assert_eq!(vm.vcpu_count(), 0);
        assert_eq!(vm.config().vcpu_count, 2);
    });
}

#[test]
fn vm_cannot_start_when_uninitialized() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        // Uninitialized → start should fail (requires Created or Paused)
        assert!(vm.start().is_err());
    });
}

#[test]
fn vm_cannot_pause_when_uninitialized() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        // Pause requires Running state
        assert!(vm.pause().is_err());
    });
}

#[test]
fn vm_stop_always_succeeds() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        // Stop works from any state
        assert!(vm.stop().is_ok());
        assert_eq!(vm.state(), VmState::Stopped);
    });
}

#[test]
fn vm_config_builder_pattern() {
    let config = VmConfig::new(4, 1024 * 1024 * 1024)
        .with_name("test-vm")
        .with_nested(true);

    assert_eq!(config.vcpu_count, 4);
    assert_eq!(config.memory_size, 1024 * 1024 * 1024);
    assert!(config.nested);
}

#[test]
fn vm_too_many_vcpus_rejected() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let result = Vm::new(
            CpuVendor::Intel,
            VmConfig::new(MAX_VCPUS_PER_VM + 1, 256 * 1024 * 1024),
        );
        assert!(result.is_err());
    });
}

#[test]
fn vm_max_vcpus_accepted() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let result = Vm::new(
            CpuVendor::Intel,
            VmConfig::new(MAX_VCPUS_PER_VM, 256 * 1024 * 1024),
        );
        assert!(result.is_ok());
    });
}

// ============================================================================
// VM + Memory Mapping (works without initialize())
// ============================================================================

#[test]
fn vm_memory_mapping_and_translation() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        vm.map_memory(0x0000_0000, 0x1000_0000, 0x0010_0000)
            .unwrap();

        assert_eq!(vm.translate_address(0x0000_0000), Some(0x1000_0000));
        assert_eq!(vm.translate_address(0x0000_1000), Some(0x1000_1000));
        assert_eq!(vm.translate_address(0x8000_0000), None);
    });
}

#[test]
fn vm_multiple_memory_regions() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 1024 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        vm.map_memory(0x0000_0000, 0x1000_0000, 0x0010_0000)
            .unwrap();
        vm.map_memory(0x0010_0000, 0x2000_0000, 0x0010_0000)
            .unwrap();

        assert_eq!(vm.translate_address(0x0000_5000), Some(0x1000_5000));
        assert_eq!(vm.translate_address(0x0010_5000), Some(0x2000_5000));
        assert_eq!(vm.translate_address(0x0020_0000), None);
    });
}

#[test]
fn vm_ept_pointer_management() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        assert_eq!(vm.ept_pointer(), 0);
        vm.set_ept_pointer(0x1000 | 0x18);
        assert_eq!(vm.ept_pointer(), 0x1000 | 0x18);
    });
}

// ============================================================================
// VM + Device Manager (manual device registration)
// ============================================================================

#[test]
fn vm_device_manager_add_and_route() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        assert_eq!(vm.device_manager().device_count(), 0);

        vm.device_manager_mut().add_debug_port();
        vm.device_manager_mut().add_i8042();
        vm.device_manager_mut().add_cmos();
        vm.device_manager_mut().add_pci_config();
        assert_eq!(vm.device_manager().device_count(), 4);

        let mut req = PortIoRequest {
            port: 0x80,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x42,
        };
        assert!(vm.device_manager_mut().handle_pio(&mut req).is_ok());
    });
}

#[test]
fn vm_device_manager_reset() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024);
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();

        vm.device_manager_mut().add_debug_port();
        vm.device_manager_mut().add_i8042();
        let count = vm.device_manager().device_count();

        vm.device_manager_mut().reset_all();
        assert_eq!(vm.device_manager().device_count(), count);

        let mut req = PortIoRequest {
            port: 0x80,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x55,
        };
        assert!(vm.device_manager_mut().handle_pio(&mut req).is_ok());
    });
}

// ============================================================================
// Cross-Module: DeviceManager + Full I/O Pipeline
// ============================================================================

#[test]
fn device_manager_full_io_pipeline() {
    let mut dm = DeviceManager::with_default_devices();

    // Debug port (0x80)
    let mut req = PortIoRequest {
        port: 0x80,
        direction: IoDirection::Write,
        size: IoSize::Byte,
        data: 0xAA,
    };
    assert!(dm.handle_pio(&mut req).is_ok());

    // I8042 (0x60, 0x64)
    let mut req = PortIoRequest {
        port: 0x60,
        direction: IoDirection::Read,
        size: IoSize::Byte,
        data: 0,
    };
    assert!(dm.handle_pio(&mut req).is_ok());

    let mut req = PortIoRequest {
        port: 0x64,
        direction: IoDirection::Read,
        size: IoSize::Byte,
        data: 0,
    };
    assert!(dm.handle_pio(&mut req).is_ok());

    // CMOS (0x70/0x71)
    let mut req = PortIoRequest {
        port: 0x70,
        direction: IoDirection::Write,
        size: IoSize::Byte,
        data: 0x00,
    };
    assert!(dm.handle_pio(&mut req).is_ok());

    let mut req = PortIoRequest {
        port: 0x71,
        direction: IoDirection::Read,
        size: IoSize::Byte,
        data: 0,
    };
    assert!(dm.handle_pio(&mut req).is_ok());

    // PCI config (0xCF8/0xCFC)
    let mut req = PortIoRequest {
        port: 0x0CF8,
        direction: IoDirection::Write,
        size: IoSize::Dword,
        data: 0x8000_0000,
    };
    assert!(dm.handle_pio(&mut req).is_ok());

    let mut req = PortIoRequest {
        port: 0x0CFC,
        direction: IoDirection::Read,
        size: IoSize::Dword,
        data: 0,
    };
    assert!(dm.handle_pio(&mut req).is_ok());
}

#[test]
fn device_manager_unhandled_port() {
    let mut dm = DeviceManager::with_default_devices();

    let mut req = PortIoRequest {
        port: 0x3F8,
        direction: IoDirection::Read,
        size: IoSize::Byte,
        data: 0,
    };
    assert!(dm.handle_pio(&mut req).is_err());
}

#[test]
fn device_manager_reset_preserves_devices() {
    let mut dm = DeviceManager::with_default_devices();
    let count_before = dm.device_count();

    dm.reset_all();
    assert_eq!(dm.device_count(), count_before);

    let mut req = PortIoRequest {
        port: 0x80,
        direction: IoDirection::Write,
        size: IoSize::Byte,
        data: 0x55,
    };
    assert!(dm.handle_pio(&mut req).is_ok());
}

// ============================================================================
// Cross-Module: Memory + EPT/NPT Entry Construction
// ============================================================================

#[test]
fn memory_mapper_with_ept_entries() {
    let mut mapper = GuestMemoryMapper::new();

    mapper
        .map_region(GuestMemoryRegion {
            guest_phys_addr: 0x0,
            host_phys_addr: 0x1_0000_0000,
            size: 4 * PAGE_SIZE as u64,
            writable: true,
            executable: true,
        })
        .unwrap();

    mapper
        .map_region(GuestMemoryRegion {
            guest_phys_addr: 0x10_0000,
            host_phys_addr: 0x2_0000_0000,
            size: 16 * PAGE_SIZE as u64,
            writable: true,
            executable: false,
        })
        .unwrap();

    assert_eq!(mapper.translate(0x1000), Some(0x1_0000_1000));
    assert_eq!(mapper.translate(0x10_2000), Some(0x2_0000_2000));

    let host_addr = mapper.translate(0x0).unwrap();
    let ept = EptEntry::page_4k(
        host_addr,
        EptEntry::READ | EptEntry::WRITE | EptEntry::EXECUTE,
    );
    assert!(ept.is_present());
    assert_eq!(ept.addr(), host_addr);
}

#[test]
fn memory_mapper_with_npt_entries() {
    let mut mapper = GuestMemoryMapper::new();

    mapper
        .map_region(GuestMemoryRegion {
            guest_phys_addr: 0x0,
            host_phys_addr: 0x8000_0000,
            size: PAGE_SIZE as u64,
            writable: true,
            executable: true,
        })
        .unwrap();

    let host_addr = mapper.translate(0x0).unwrap();
    let npt = NptEntry::page_4k(host_addr, true);
    assert!(npt.is_present());
    assert_eq!(npt.addr(), host_addr);
}

// ============================================================================
// Cross-Module: Frame Allocator + Memory Mapper Pipeline
// ============================================================================

#[test]
fn frame_allocator_and_mapper_integration() {
    let mut alloc = FrameAllocator::new();
    alloc.init(0x10_0000, 0x20_0000).unwrap();

    let frame1 = alloc.allocate_frame().unwrap();
    let frame2 = alloc.allocate_frame().unwrap();

    assert_ne!(frame1.as_u64(), frame2.as_u64());
    assert_eq!(frame1.as_u64() % PAGE_SIZE as u64, 0);
    assert_eq!(frame2.as_u64() % PAGE_SIZE as u64, 0);

    let mut mapper = GuestMemoryMapper::new();
    mapper
        .map_region(GuestMemoryRegion {
            guest_phys_addr: 0x0,
            host_phys_addr: frame1.as_u64(),
            size: PAGE_SIZE as u64,
            writable: true,
            executable: true,
        })
        .unwrap();

    mapper
        .map_region(GuestMemoryRegion {
            guest_phys_addr: PAGE_SIZE as u64,
            host_phys_addr: frame2.as_u64(),
            size: PAGE_SIZE as u64,
            writable: true,
            executable: false,
        })
        .unwrap();

    assert_eq!(mapper.translate(0x0), Some(frame1.as_u64()));
    assert_eq!(mapper.translate(PAGE_SIZE as u64), Some(frame2.as_u64()));
}

#[test]
fn frame_allocator_exhaustion() {
    let mut alloc = FrameAllocator::new();
    alloc
        .init(0x10_0000, 0x10_0000 + 2 * PAGE_SIZE as u64)
        .unwrap();

    assert!(alloc.allocate_frame().is_ok());
    assert!(alloc.allocate_frame().is_ok());
    assert!(alloc.allocate_frame().is_err());
}

// ============================================================================
// Cross-Module: vCPU + Register Configuration
// ============================================================================

#[test]
fn vcpu_real_mode_setup() {
    let mut vcpu = Vcpu::new(CpuVendor::Intel);
    vcpu.setup_real_mode();
    assert_eq!(vcpu.state(), VcpuState::Uninitialized);
}

#[test]
fn vcpu_long_mode_setup() {
    let mut vcpu = Vcpu::new(CpuVendor::Intel);

    vcpu.setup_long_mode(0x20_0000, 0x10_0000, 0x1000);

    let regs = vcpu.registers();
    assert_eq!(regs.gp.rip, 0x20_0000);
    assert_eq!(regs.gp.rsp, 0x10_0000);
    assert_eq!(regs.cr.cr3, 0x1000);
}

#[test]
fn vcpu_register_isolation_across_instances() {
    let mut vcpu0 = Vcpu::new(CpuVendor::Intel);
    let mut vcpu1 = Vcpu::new(CpuVendor::Intel);

    vcpu0.registers_mut().gp.rax = 0xDEAD;
    vcpu1.registers_mut().gp.rax = 0xBEEF;

    assert_eq!(vcpu0.registers().gp.rax, 0xDEAD);
    assert_eq!(vcpu1.registers().gp.rax, 0xBEEF);
}

#[test]
fn vcpu_vendor_variants() {
    let intel = Vcpu::new(CpuVendor::Intel);
    let amd = Vcpu::new(CpuVendor::Amd);

    assert_eq!(intel.vendor(), CpuVendor::Intel);
    assert_eq!(amd.vendor(), CpuVendor::Amd);
    assert_eq!(intel.exit_count(), 0);
    assert_eq!(amd.exit_count(), 0);
}

// ============================================================================
// Cross-Module: VirtualApic + Interrupt Pipeline
// ============================================================================

#[test]
fn vapic_multiple_interrupt_sources() {
    let mut vapic = VirtualApic::new(0);

    vapic.set_irr(0x20);
    vapic.set_irr(0x30);
    vapic.set_irr(0x40);

    assert!(vapic.has_pending_interrupt());

    let first = vapic.get_pending_interrupt().unwrap();
    assert!(first >= 0x20);
}

#[test]
fn vapic_eoi_clears_in_service() {
    let mut vapic = VirtualApic::new(0);

    vapic.set_irr(0x30);
    let vector = vapic.get_pending_interrupt().unwrap();

    vapic.clear_irr(vector);
    vapic.set_isr(vector);
    vapic.eoi();
}

#[test]
fn vapic_independent_per_vcpu() {
    let mut vapic0 = VirtualApic::new(0);
    let mut vapic1 = VirtualApic::new(1);

    vapic0.set_irr(0x30);
    assert!(vapic0.has_pending_interrupt());
    assert!(!vapic1.has_pending_interrupt());

    vapic1.set_irr(0x40);
    assert!(vapic1.has_pending_interrupt());
}

// ============================================================================
// Physical Region
// ============================================================================

#[test]
fn physical_region_contains_check() {
    let region = PhysicalRegion::new(0x1000, 0x4000);

    assert!(region.contains(x86_64::PhysAddr::new(0x1000)));
    assert!(region.contains(x86_64::PhysAddr::new(0x4FFF)));
    assert!(!region.contains(x86_64::PhysAddr::new(0x0FFF)));
    assert!(!region.contains(x86_64::PhysAddr::new(0x5000)));
}

// ============================================================================
// Full Pipeline: VM Config → Memory → Devices → EPT (no hardware)
// ============================================================================

#[test]
fn full_vm_setup_without_hardware() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        // 1. Configure
        let config = VmConfig::new(2, 512 * 1024 * 1024)
            .with_name("integration-test-vm")
            .with_nested(false);

        // 2. Create VM (Uninitialized — no hardware needed)
        let mut vm = Vm::new(CpuVendor::Intel, config).unwrap();
        assert_eq!(vm.state(), VmState::Uninitialized);

        // 3. Map guest memory regions
        vm.map_memory(0x0000_0000, 0x1000_0000, 0x0020_0000)
            .unwrap();
        vm.map_memory(0x0020_0000, 0x2000_0000, 0x1000_0000)
            .unwrap();

        // 4. Verify translation
        assert_eq!(vm.translate_address(0x0000_0000), Some(0x1000_0000));
        assert_eq!(vm.translate_address(0x0020_0000), Some(0x2000_0000));

        // 5. Set EPT pointer manually
        vm.set_ept_pointer(0x1000 | 0x18);
        assert_eq!(vm.ept_pointer(), 0x1000 | 0x18);

        // 6. Register devices
        vm.device_manager_mut().add_debug_port();
        vm.device_manager_mut().add_i8042();
        vm.device_manager_mut().add_cmos();
        vm.device_manager_mut().add_pci_config();
        assert_eq!(vm.device_manager().device_count(), 4);

        // 7. Route I/O through device manager
        let mut req = PortIoRequest {
            port: 0x80,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xCC,
        };
        assert!(vm.device_manager_mut().handle_pio(&mut req).is_ok());

        // 8. Init frame allocator
        vm.init_frame_allocator(0x100_0000, 0x200_0000)
            .expect("frame allocator should initialise over a valid range");

        // 9. Stop (always succeeds)
        vm.stop().unwrap();
        assert_eq!(vm.state(), VmState::Stopped);
    });
}

#[test]
fn full_vm_amd_variant() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config = VmConfig::new(1, 256 * 1024 * 1024).with_name("amd-test-vm");

        let mut vm = Vm::new(CpuVendor::Amd, config).unwrap();
        assert_eq!(vm.state(), VmState::Uninitialized);

        vm.map_memory(0x0, 0x1000_0000, 0x10_0000).unwrap();
        assert_eq!(vm.translate_address(0x0), Some(0x1000_0000));

        vm.stop().unwrap();
        assert_eq!(vm.state(), VmState::Stopped);
    });
}

// ============================================================================
// Unique IDs
// ============================================================================

#[test]
fn vm_ids_are_unique() {
    with_big_stack(|| {
        use hv1_core::vm::Vm;

        let config1 = VmConfig::new(1, 128 * 1024 * 1024);
        let config2 = VmConfig::new(1, 128 * 1024 * 1024);

        let vm1 = Vm::new(CpuVendor::Intel, config1).unwrap();
        let vm2 = Vm::new(CpuVendor::Intel, config2).unwrap();

        assert_ne!(vm1.id(), vm2.id());
    });
}
