# Type-1 Hypervisor

The Type-1 (bare-metal) hypervisor runs directly on hardware without a host operating system.

## Overview

```
┌─────────────────────────────────────────────────────────┐
│                     Guest VMs                            │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐           │
│  │   VM 1    │  │   VM 2    │  │   VM N    │           │
│  │  (Linux)  │  │ (Windows) │  │  (Guest)  │           │
│  └───────────┘  └───────────┘  └───────────┘           │
├─────────────────────────────────────────────────────────┤
│              HyperMachine Type-1 Hypervisor             │
│         (Direct VMX/SVM control, no host OS)            │
├─────────────────────────────────────────────────────────┤
│                      Hardware                            │
│    CPU (VT-x/AMD-V)  •  RAM  •  GPU  •  NIC  •  Storage│
└─────────────────────────────────────────────────────────┘
```

## Boot Process

### UEFI Boot

1. **Firmware Initialization** - UEFI firmware loads HyperMachine bootloader
2. **Memory Detection** - Detect and map physical memory via UEFI memory map
3. **CPU Initialization** - Enable VMX/SVM on all cores
4. **Hypervisor Setup** - Initialize VMCS/VMCB structures
5. **Service VM Boot** - Launch management VM for administration

### Boot Sequence

```rust
// Simplified boot sequence
fn hypervisor_main(boot_info: &BootInfo) {
    // Initialize memory management
    let mut memory = PhysicalMemory::from_uefi_map(&boot_info.memory_map);
    
    // Enable VMX on all cores
    for cpu in 0..boot_info.cpu_count {
        vmx::enable_vmx(cpu)?;
        vmx::setup_vmcs(cpu)?;
    }
    
    // Initialize device passthrough
    iommu::initialize()?;
    
    // Launch service VM
    let service_vm = Vm::create_service_vm(&memory)?;
    service_vm.start()?;
}
```

## VMX/SVM Implementation

### Intel VT-x (VMX)

```rust
pub struct Vmcs {
    guest_state: GuestState,
    host_state: HostState,
    vm_execution_controls: ExecutionControls,
    vm_exit_controls: ExitControls,
    vm_entry_controls: EntryControls,
}

impl Vmcs {
    pub fn vm_enter(&mut self) -> VmExitReason {
        // Save host state
        self.save_host_state();
        
        // Load guest state
        self.load_guest_state();
        
        // Execute VMLAUNCH/VMRESUME
        unsafe {
            vmx::vmlaunch()
        }
    }
    
    pub fn handle_vm_exit(&mut self, reason: VmExitReason) {
        match reason {
            VmExitReason::IoInstruction(port, direction) => {
                self.emulate_io(port, direction);
            }
            VmExitReason::Msr(msr, direction) => {
                self.emulate_msr(msr, direction);
            }
            VmExitReason::Ept(violation) => {
                self.handle_ept_violation(violation);
            }
            // ... other exit reasons
        }
    }
}
```

### AMD-V (SVM)

```rust
pub struct Vmcb {
    control_area: VmcbControl,
    state_save_area: VmcbStateSave,
}

impl Vmcb {
    pub fn vm_run(&mut self) -> VmExitCode {
        unsafe {
            svm::vmrun(&mut self.control_area, &mut self.state_save_area)
        }
    }
}
```

## Memory Virtualization

### Extended Page Tables (EPT) - Intel

```rust
pub struct Ept {
    pml4: Box<EptPml4>,
}

impl Ept {
    pub fn map_guest_memory(
        &mut self,
        guest_physical: u64,
        host_physical: u64,
        permissions: EptPermissions,
    ) {
        let pml4_idx = (guest_physical >> 39) & 0x1FF;
        let pdpt_idx = (guest_physical >> 30) & 0x1FF;
        let pd_idx = (guest_physical >> 21) & 0x1FF;
        let pt_idx = (guest_physical >> 12) & 0x1FF;
        
        // Walk and create page tables as needed
        // Map guest physical to host physical with permissions
    }
}
```

### Nested Page Tables (NPT) - AMD

Similar structure to EPT, using AMD's nested paging implementation.

## Device Passthrough

### IOMMU (VT-d / AMD-Vi)

```rust
pub struct Iommu {
    dmar_units: Vec<DmarUnit>,
    device_mappings: HashMap<PciAddress, VmId>,
}

impl Iommu {
    pub fn assign_device_to_vm(
        &mut self,
        device: PciAddress,
        vm: VmId,
    ) -> Result<()> {
        // Program IOMMU to redirect device DMA to VM's memory space
        let domain = self.get_or_create_domain(vm);
        domain.map_device(device)?;
        
        // Update interrupt remapping
        self.configure_interrupt_remapping(device, vm)?;
        
        Ok(())
    }
}
```

## Security Considerations

### Isolation Guarantees

- **Memory Isolation** - EPT/NPT prevents VMs from accessing each other's memory
- **Device Isolation** - IOMMU prevents DMA attacks
- **Interrupt Isolation** - Interrupt remapping prevents interrupt injection

### Attack Surface

The Type-1 hypervisor has a minimal attack surface:

- No host OS kernel vulnerabilities
- Direct hardware control
- Small trusted computing base (TCB)

## Use Cases

- **High-Security Environments** - Government, financial services
- **Cloud Infrastructure** - Multi-tenant isolation
- **Embedded Systems** - Automotive, industrial control
- **AI Compute Clusters** - Isolated GPU workloads

## Next Steps

- [Type-2 Hypervisor](./type-2.md) - Hosted mode for development
- [Memory Management](./memory.md) - Memory subsystem details
- [Security Overview](../security/overview.md) - Security architecture
