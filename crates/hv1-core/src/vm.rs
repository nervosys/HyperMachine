//! Virtual Machine management for Type-1 hypervisor
//!
//! This module handles:
//! - VM creation and lifecycle
//! - Multi-vCPU coordination
//! - Guest memory management
//! - Device assignment

use crate::memory::GuestMemoryMapper;
use crate::vcpu::Vcpu;
use crate::{CpuVendor, Error, Result};
use core::sync::atomic::{AtomicU32, Ordering};

/// VM ID counter
static VM_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Maximum vCPUs per VM
pub const MAX_VCPUS_PER_VM: usize = 256;

/// VM state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM is not initialized
    Uninitialized,
    /// VM is created but not running
    Created,
    /// VM is running
    Running,
    /// VM is paused
    Paused,
    /// VM is stopped
    Stopped,
    /// VM has crashed
    Crashed,
}

/// VM configuration
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Number of vCPUs
    pub vcpu_count: usize,
    /// Memory size in bytes
    pub memory_size: u64,
    /// Enable nested virtualization
    pub nested: bool,
    /// VM name (for debugging)
    pub name: [u8; 32],
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            vcpu_count: 1,
            memory_size: 512 * 1024 * 1024, // 512MB
            nested: false,
            name: [0; 32],
        }
    }
}

impl VmConfig {
    /// Create a new VM configuration
    pub fn new(vcpu_count: usize, memory_size: u64) -> Self {
        Self {
            vcpu_count,
            memory_size,
            ..Default::default()
        }
    }

    /// Set the VM name
    pub fn with_name(mut self, name: &str) -> Self {
        let bytes = name.as_bytes();
        let len = bytes.len().min(31);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self
    }

    /// Enable nested virtualization
    pub fn with_nested(mut self, nested: bool) -> Self {
        self.nested = nested;
        self
    }
}

/// Virtual Machine
pub struct Vm {
    /// VM ID
    id: u32,
    /// VM state
    state: VmState,
    /// Configuration
    config: VmConfig,
    /// CPU vendor
    vendor: CpuVendor,
    /// vCPUs (using array instead of Vec for no_std)
    vcpus: [Option<Vcpu>; MAX_VCPUS_PER_VM],
    /// Number of active vCPUs
    vcpu_count: usize,
    /// Guest memory mapper
    memory_mapper: GuestMemoryMapper,
    /// EPT pointer (Intel) or NCR3 (AMD)
    ept_pointer: u64,
}

impl Vm {
    /// Create a new VM
    pub fn new(vendor: CpuVendor, config: VmConfig) -> Result<Self> {
        if config.vcpu_count > MAX_VCPUS_PER_VM {
            return Err(Error::InvalidConfiguration);
        }

        let id = VM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        
        // Initialize empty vcpu array
        const NONE_VCPU: Option<Vcpu> = None;
        
        Ok(Self {
            id,
            state: VmState::Uninitialized,
            config,
            vendor,
            vcpus: [NONE_VCPU; MAX_VCPUS_PER_VM],
            vcpu_count: 0,
            memory_mapper: GuestMemoryMapper::new(),
            ept_pointer: 0,
        })
    }

    /// Get the VM ID
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the VM state
    pub fn state(&self) -> VmState {
        self.state
    }

    /// Get the configuration
    pub fn config(&self) -> &VmConfig {
        &self.config
    }

    /// Get the number of vCPUs
    pub fn vcpu_count(&self) -> usize {
        self.vcpu_count
    }

    /// Initialize the VM
    pub fn initialize(&mut self) -> Result<()> {
        // Create vCPUs
        for i in 0..self.config.vcpu_count {
            let mut vcpu = Vcpu::new(self.vendor);
            vcpu.initialize()?;
            self.vcpus[i] = Some(vcpu);
            self.vcpu_count += 1;
        }

        // Set up guest memory
        self.setup_memory()?;

        self.state = VmState::Created;
        Ok(())
    }

    /// Set up guest memory
    fn setup_memory(&mut self) -> Result<()> {
        // This would allocate physical memory and set up EPT/NPT
        // For now, just mark as configured
        Ok(())
    }

    /// Get a vCPU by index
    pub fn vcpu(&self, index: usize) -> Option<&Vcpu> {
        if index < self.vcpu_count {
            self.vcpus[index].as_ref()
        } else {
            None
        }
    }

    /// Get a mutable vCPU by index
    pub fn vcpu_mut(&mut self, index: usize) -> Option<&mut Vcpu> {
        if index < self.vcpu_count {
            self.vcpus[index].as_mut()
        } else {
            None
        }
    }

    /// Start the VM
    pub fn start(&mut self) -> Result<()> {
        if self.state != VmState::Created && self.state != VmState::Paused {
            return Err(Error::InvalidState);
        }

        self.state = VmState::Running;
        Ok(())
    }

    /// Pause the VM
    pub fn pause(&mut self) -> Result<()> {
        if self.state != VmState::Running {
            return Err(Error::InvalidState);
        }

        self.state = VmState::Paused;
        Ok(())
    }

    /// Stop the VM
    pub fn stop(&mut self) -> Result<()> {
        self.state = VmState::Stopped;
        Ok(())
    }

    /// Get the EPT pointer (Intel) or NCR3 (AMD)
    pub fn ept_pointer(&self) -> u64 {
        self.ept_pointer
    }

    /// Set the EPT pointer
    pub fn set_ept_pointer(&mut self, pointer: u64) {
        self.ept_pointer = pointer;
    }

    /// Get the memory mapper
    pub fn memory_mapper(&self) -> &GuestMemoryMapper {
        &self.memory_mapper
    }

    /// Get mutable access to the memory mapper
    pub fn memory_mapper_mut(&mut self) -> &mut GuestMemoryMapper {
        &mut self.memory_mapper
    }

    /// Map guest physical memory
    pub fn map_memory(&mut self, guest_phys: u64, host_phys: u64, size: u64) -> Result<()> {
        use crate::memory::GuestMemoryRegion;
        
        self.memory_mapper.map_region(GuestMemoryRegion {
            guest_phys_addr: guest_phys,
            host_phys_addr: host_phys,
            size,
            writable: true,
            executable: true,
        })
    }

    /// Translate guest physical address to host physical address
    pub fn translate_address(&self, guest_phys: u64) -> Option<u64> {
        self.memory_mapper.translate(guest_phys)
    }
}

/// VM exit handler trait
pub trait VmExitHandler {
    /// Handle a VM exit
    fn handle_exit(&mut self, vm: &mut Vm, vcpu_index: usize, exit_info: &crate::vcpu::VmExitInfo) -> Result<VmExitAction>;
}

/// Action to take after handling a VM exit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmExitAction {
    /// Continue running the vCPU
    Continue,
    /// Halt the vCPU
    Halt,
    /// Shutdown the VM
    Shutdown,
    /// Report an error
    Error,
}

/// Simple VM exit handler that handles basic exits
pub struct DefaultExitHandler;

impl VmExitHandler for DefaultExitHandler {
    fn handle_exit(&mut self, vm: &mut Vm, vcpu_index: usize, exit_info: &crate::vcpu::VmExitInfo) -> Result<VmExitAction> {
        // Handle common exits
        match exit_info.reason {
            // CPUID - emulate
            10 => {
                if let Some(vcpu) = vm.vcpu_mut(vcpu_index) {
                    let regs = vcpu.registers_mut();
                    let leaf = regs.gp.rax as u32;
                    let subleaf = regs.gp.rcx as u32;
                    
                    let result = crate::cpu::virtualize_cpuid(leaf, subleaf, false);
                    regs.gp.rax = result.eax as u64;
                    regs.gp.rbx = result.ebx as u64;
                    regs.gp.rcx = result.ecx as u64;
                    regs.gp.rdx = result.edx as u64;
                    
                    // Advance RIP past CPUID instruction
                    regs.gp.rip += exit_info.instruction_length as u64;
                }
                Ok(VmExitAction::Continue)
            }
            // HLT
            12 => {
                Ok(VmExitAction::Halt)
            }
            // Triple fault
            2 => {
                Ok(VmExitAction::Shutdown)
            }
            _ => {
                // Unhandled exit
                Ok(VmExitAction::Error)
            }
        }
    }
}
