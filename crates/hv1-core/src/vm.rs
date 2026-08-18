//! Virtual Machine management for Type-1 hypervisor
//!
//! This module handles:
//! - VM creation and lifecycle
//! - Multi-vCPU coordination
//! - Guest memory management
//! - Device assignment

use crate::device::{DeviceManager, IoDirection, IoSize, MmioRequest, PortIoRequest};
use crate::interrupt::VirtualApic;
use crate::memory::{FrameAllocator, GuestMemoryMapper};
use crate::vcpu::Vcpu;
use crate::{CpuVendor, Error, Result};
use alloc::{boxed::Box, vec::Vec};
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
    /// vCPUs. Heap-allocated (boxed slice of length `MAX_VCPUS_PER_VM`) so the
    /// `Vm` struct stays small on the stack — inlining the array made `Vm`
    /// multi-megabyte, overflowing the stack when returned by value.
    vcpus: Box<[Option<Vcpu>]>,
    /// Number of active vCPUs
    vcpu_count: usize,
    /// Guest memory mapper
    memory_mapper: GuestMemoryMapper,
    /// Frame allocator for page table construction
    frame_allocator: FrameAllocator,
    /// EPT pointer (Intel) or NCR3 (AMD)
    ept_pointer: u64,
    /// Per-vCPU virtual APICs. Heap-allocated for the same reason as `vcpus`.
    vapics: Box<[Option<VirtualApic>]>,
    /// Device manager for I/O exit routing
    device_manager: DeviceManager,
}

impl Vm {
    /// Create a new VM
    pub fn new(vendor: CpuVendor, config: VmConfig) -> Result<Self> {
        if config.vcpu_count > MAX_VCPUS_PER_VM {
            return Err(Error::InvalidConfiguration);
        }

        let id = VM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

        // Allocate the per-vCPU slots on the heap. Building through a `Vec`
        // (rather than a `[None; N]` literal) keeps the large array off the
        // stack, which is essential on the constrained bare-metal stack.
        let mut vcpus: Vec<Option<Vcpu>> = Vec::new();
        vcpus.resize_with(MAX_VCPUS_PER_VM, || None);
        let mut vapics: Vec<Option<VirtualApic>> = Vec::new();
        vapics.resize_with(MAX_VCPUS_PER_VM, || None);

        Ok(Self {
            id,
            state: VmState::Uninitialized,
            config,
            vendor,
            vcpus: vcpus.into_boxed_slice(),
            vcpu_count: 0,
            memory_mapper: GuestMemoryMapper::new(),
            frame_allocator: FrameAllocator::new(),
            ept_pointer: 0,
            vapics: vapics.into_boxed_slice(),
            device_manager: DeviceManager::new(),
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

    /// Initialize the frame allocator with a memory region for page-table pages.
    pub fn init_frame_allocator(&mut self, start: u64, end: u64) -> Result<()> {
        self.frame_allocator.init(start, end)
    }

    /// Initialize the VM
    pub fn initialize(&mut self) -> Result<()> {
        // Create vCPUs
        for i in 0..self.config.vcpu_count {
            let mut vcpu = Vcpu::new(self.vendor);
            vcpu.initialize()?;
            self.vcpus[i] = Some(vcpu);
            self.vapics[i] = Some(VirtualApic::new(i as u8));
            self.vcpu_count += 1;
        }

        // Set up guest memory (EPT/NPT)
        self.setup_memory()?;

        // Configure each vCPU's hardware backend
        for i in 0..self.vcpu_count {
            if let Some(vcpu) = self.vcpus[i].as_mut() {
                match self.vendor {
                    #[cfg(feature = "intel")]
                    CpuVendor::Intel => {
                        unsafe { vcpu.setup_vmx(self.ept_pointer)? };
                    }
                    #[cfg(feature = "amd")]
                    CpuVendor::Amd => {
                        vcpu.setup_svm(self.ept_pointer, (i + 1) as u32)?;
                    }
                    _ => return Err(Error::NoHardwareSupport),
                }
            }
        }

        self.state = VmState::Created;
        Ok(())
    }

    /// Set up guest memory — builds EPT (Intel) or NPT (AMD) from mapped regions.
    fn setup_memory(&mut self) -> Result<()> {
        match self.vendor {
            #[cfg(feature = "intel")]
            CpuVendor::Intel => {
                let eptp =
                    crate::memory::build_ept(&mut self.frame_allocator, &self.memory_mapper)?;
                self.ept_pointer = eptp;
            }
            #[cfg(feature = "amd")]
            CpuVendor::Amd => {
                let ncr3 =
                    crate::memory::build_npt(&mut self.frame_allocator, &self.memory_mapper)?;
                self.ept_pointer = ncr3;
            }
            _ => return Err(Error::NoHardwareSupport),
        }
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

    /// Get the device manager
    pub fn device_manager(&self) -> &DeviceManager {
        &self.device_manager
    }

    /// Get mutable access to the device manager
    pub fn device_manager_mut(&mut self) -> &mut DeviceManager {
        &mut self.device_manager
    }

    /// Get a vCPU's virtual APIC
    pub fn vapic(&self, vcpu_index: usize) -> Option<&VirtualApic> {
        self.vapics.get(vcpu_index).and_then(|v| v.as_ref())
    }

    /// Get mutable access to a vCPU's virtual APIC
    pub fn vapic_mut(&mut self, vcpu_index: usize) -> Option<&mut VirtualApic> {
        self.vapics.get_mut(vcpu_index).and_then(|v| v.as_mut())
    }

    /// Run a vCPU in a loop, handling exits until halt or shutdown.
    ///
    /// # Safety
    /// Performs VM entry/exit.
    pub unsafe fn run_vcpu(
        &mut self,
        vcpu_index: usize,
        handler: &mut dyn VmExitHandler,
    ) -> Result<VmExitAction> {
        loop {
            // Before VM entry, try to inject any pending interrupt.
            if let Some(vapic) = self.vapics[vcpu_index].as_mut() {
                if let Some(vector) = vapic.get_pending_interrupt() {
                    if let Some(vcpu) = self.vcpus[vcpu_index].as_mut() {
                        // Check that RFLAGS.IF == 1 (guest can accept interrupts).
                        if vcpu.registers().gp.rflags & (1 << 9) != 0 {
                            let _ = vcpu.inject_interrupt(vector);
                            vapic.set_isr(vector); // move from IRR → ISR
                        }
                    }
                }
            }

            // VM entry → guest execution → VM exit
            let exit_info = match self.vcpus[vcpu_index].as_mut() {
                Some(vcpu) => vcpu.run()?,
                None => return Err(Error::InvalidState),
            };

            // Dispatch exit to handler
            let action = handler.handle_exit(self, vcpu_index, &exit_info)?;

            match action {
                VmExitAction::Continue => continue,
                other => return Ok(other),
            }
        }
    }
}

/// VM exit handler trait.
///
/// Implementors receive VM exits and decide how to proceed.  The
/// handler is called inside the vCPU run loop ([`Vm::run_vcpu`]) and
/// must return a [`VmExitAction`] that determines whether the loop
/// continues, halts, or shuts down.
pub trait VmExitHandler {
    /// Handle a VM exit
    fn handle_exit(
        &mut self,
        vm: &mut Vm,
        vcpu_index: usize,
        exit_info: &crate::vcpu::VmExitInfo,
    ) -> Result<VmExitAction>;
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

/// Simple VM exit handler that handles common exits
pub struct DefaultExitHandler;

impl VmExitHandler for DefaultExitHandler {
    fn handle_exit(
        &mut self,
        vm: &mut Vm,
        vcpu_index: usize,
        exit_info: &crate::vcpu::VmExitInfo,
    ) -> Result<VmExitAction> {
        match exit_info.reason {
            // CPUID
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

                    regs.gp.rip += exit_info.instruction_length as u64;
                }
                Ok(VmExitAction::Continue)
            }
            // HLT
            12 => Ok(VmExitAction::Halt),
            // Triple fault
            2 => Ok(VmExitAction::Shutdown),
            // I/O instruction (VMX reason 30, SVM exit 0x7B)
            30 | 0x7B => Self::handle_io_exit(vm, vcpu_index, exit_info),
            // EPT violation (VMX reason 48) / NPT fault (SVM exit 0x400)
            48 | 0x400 => Self::handle_memory_exit(vm, vcpu_index, exit_info),
            // MSR read (VMX reason 31)
            31 => {
                if let Some(vcpu) = vm.vcpu_mut(vcpu_index) {
                    let regs = vcpu.registers_mut();
                    let msr = regs.gp.rcx as u32;
                    let value = unsafe { crate::cpu::msr::read(msr) };
                    regs.gp.rax = value & 0xFFFF_FFFF;
                    regs.gp.rdx = value >> 32;
                    regs.gp.rip += exit_info.instruction_length as u64;
                }
                Ok(VmExitAction::Continue)
            }
            // MSR write (VMX reason 32)
            32 => {
                let (msr, value) = {
                    if let Some(vcpu) = vm.vcpu_mut(vcpu_index) {
                        let regs = vcpu.registers_mut();
                        let msr = regs.gp.rcx as u32;
                        let value = (regs.gp.rdx << 32) | (regs.gp.rax & 0xFFFF_FFFF);
                        regs.gp.rip += exit_info.instruction_length as u64;
                        (msr, value)
                    } else {
                        return Ok(VmExitAction::Continue);
                    }
                };

                // Intercept APIC EOI via the x2APIC MSR (0x80B)
                if msr == 0x80B {
                    if let Some(vapic) = vm.vapic_mut(vcpu_index) {
                        vapic.eoi();
                    }
                } else {
                    unsafe { crate::cpu::msr::write(msr, value) };
                }
                Ok(VmExitAction::Continue)
            }
            // External interrupt (VMX reason 1)
            1 => Ok(VmExitAction::Continue),
            // Interrupt window (VMX reason 7) — re-open injection window
            7 => Ok(VmExitAction::Continue),
            _ => Ok(VmExitAction::Error),
        }
    }
}

impl DefaultExitHandler {
    /// Handle a port-I/O VM exit.
    fn handle_io_exit(
        vm: &mut Vm,
        vcpu_index: usize,
        exit_info: &crate::vcpu::VmExitInfo,
    ) -> Result<VmExitAction> {
        let qualification = exit_info.qualification;

        // Intel I/O exit qualification encoding (SDM Vol 3, Table 28-5):
        //   bits 15:0  — port number
        //   bit 3      — direction (0 = OUT, 1 = IN)
        //   bits 2:0   — size (0 = 1 byte, 1 = 2, 3 = 4)
        let port = (qualification & 0xFFFF) as u16;
        let is_in = qualification & (1 << 3) != 0;
        let size = match qualification & 0x7 {
            0 => IoSize::Byte,
            1 => IoSize::Word,
            3 => IoSize::Dword,
            _ => IoSize::Byte,
        };

        let direction = if is_in {
            IoDirection::Read
        } else {
            IoDirection::Write
        };

        // Get the data from the guest (RAX for OUT)
        let data = vm
            .vcpu(vcpu_index)
            .map(|v| v.registers().gp.rax)
            .unwrap_or(0);

        let mut request = PortIoRequest {
            port,
            direction,
            size,
            data,
        };

        // Route through device manager
        let _ = vm.device_manager.handle_pio(&mut request);

        // For IN instructions, write the result back to RAX
        if is_in {
            if let Some(vcpu) = vm.vcpu_mut(vcpu_index) {
                let rax = &mut vcpu.registers_mut().gp.rax;
                match size {
                    IoSize::Byte => *rax = (*rax & !0xFF) | (request.data & 0xFF),
                    IoSize::Word => *rax = (*rax & !0xFFFF) | (request.data & 0xFFFF),
                    IoSize::Dword => *rax = request.data & 0xFFFF_FFFF,
                    IoSize::Qword => *rax = request.data,
                }
            }
        }

        // Advance RIP
        if let Some(vcpu) = vm.vcpu_mut(vcpu_index) {
            vcpu.registers_mut().gp.rip += exit_info.instruction_length as u64;
        }
        Ok(VmExitAction::Continue)
    }

    /// Handle an EPT violation / NPT fault — route to MMIO devices.
    fn handle_memory_exit(
        vm: &mut Vm,
        vcpu_index: usize,
        exit_info: &crate::vcpu::VmExitInfo,
    ) -> Result<VmExitAction> {
        let guest_phys = exit_info.guest_physical_addr.unwrap_or(0);

        // If none of our devices claim this MMIO range, report an error
        let mut request = MmioRequest {
            address: guest_phys,
            direction: if exit_info.qualification & 1 != 0 {
                IoDirection::Read
            } else {
                IoDirection::Write
            },
            size: IoSize::Dword,
            data: 0,
        };

        if vm.device_manager.handle_mmio(&mut request).is_ok() {
            // Advance RIP
            if let Some(vcpu) = vm.vcpu_mut(vcpu_index) {
                vcpu.registers_mut().gp.rip += exit_info.instruction_length as u64;
            }
            Ok(VmExitAction::Continue)
        } else {
            Ok(VmExitAction::Error)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    /// Run a closure on a thread with a 4 MB stack so that the large `Vm` struct
    /// (256-element arrays of Option<Vcpu> + Option<VirtualApic>) does not overflow
    /// the default test-thread stack.
    fn with_big_stack<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(f)
            .expect("thread spawn")
            .join()
            .expect("thread join");
    }

    // --- VmConfig ---

    #[test]
    fn vm_config_default() {
        let cfg = VmConfig::default();
        assert_eq!(cfg.vcpu_count, 1);
        assert_eq!(cfg.memory_size, 512 * 1024 * 1024);
        assert!(!cfg.nested);
        assert_eq!(cfg.name, [0u8; 32]);
    }

    #[test]
    fn vm_config_new() {
        let cfg = VmConfig::new(4, 1024 * 1024 * 1024);
        assert_eq!(cfg.vcpu_count, 4);
        assert_eq!(cfg.memory_size, 1 << 30);
    }

    #[test]
    fn vm_config_with_name() {
        let cfg = VmConfig::default().with_name("test-vm");
        assert_eq!(&cfg.name[..7], b"test-vm");
        assert_eq!(cfg.name[7], 0);
    }

    #[test]
    fn vm_config_name_truncation() {
        let long = "a".repeat(64);
        let cfg = VmConfig::default().with_name(&long);
        // name is [u8; 32], max copy len is 31
        assert_eq!(cfg.name[31], 0);
    }

    #[test]
    fn vm_config_with_nested() {
        let cfg = VmConfig::default().with_nested(true);
        assert!(cfg.nested);
    }

    // --- Vm creation ---

    #[test]
    fn vm_new_creates_uninitialized() {
        with_big_stack(|| {
            let vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            assert_eq!(vm.state(), VmState::Uninitialized);
            assert_eq!(vm.vcpu_count(), 0);
            assert_eq!(vm.ept_pointer(), 0);
        });
    }

    #[test]
    fn vm_new_too_many_vcpus() {
        let cfg = VmConfig::new(MAX_VCPUS_PER_VM + 1, 512 * 1024 * 1024);
        assert!(Vm::new(CpuVendor::Intel, cfg).is_err());
    }

    #[test]
    fn vm_ids_increment() {
        with_big_stack(|| {
            let vm1 = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            let vm2 = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            assert!(vm2.id() > vm1.id());
        });
    }

    // --- State machine ---

    #[test]
    fn vm_start_from_uninitialized_fails() {
        with_big_stack(|| {
            let mut vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            assert!(vm.start().is_err());
        });
    }

    #[test]
    fn vm_pause_from_uninitialized_fails() {
        with_big_stack(|| {
            let mut vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            assert!(vm.pause().is_err());
        });
    }

    #[test]
    fn vm_stop_always_succeeds() {
        with_big_stack(|| {
            let mut vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            vm.stop().unwrap();
            assert_eq!(vm.state(), VmState::Stopped);
        });
    }

    // --- Memory mapping ---

    #[test]
    fn vm_map_and_translate() {
        with_big_stack(|| {
            let mut vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            vm.map_memory(0x0, 0x100_0000, 0x10_0000).unwrap();
            assert_eq!(vm.translate_address(0x500), Some(0x100_0500));
            assert_eq!(vm.translate_address(0x10_0000), None);
        });
    }

    #[test]
    fn vm_ept_pointer_accessors() {
        with_big_stack(|| {
            let mut vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            vm.set_ept_pointer(0xDEAD_BEEF);
            assert_eq!(vm.ept_pointer(), 0xDEAD_BEEF);
        });
    }

    // --- Device manager access ---

    #[test]
    fn vm_device_manager_default() {
        with_big_stack(|| {
            let vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            assert_eq!(vm.device_manager().device_count(), 0);
        });
    }

    // --- Frame allocator ---

    #[test]
    fn vm_init_frame_allocator() {
        with_big_stack(|| {
            let mut vm = Vm::new(CpuVendor::Intel, VmConfig::default()).unwrap();
            vm.init_frame_allocator(0x100_0000, 0x200_0000)
                .expect("frame allocator should initialise over a valid range");
        });
    }

    // --- Config access ---

    #[test]
    fn vm_config_accessor() {
        with_big_stack(|| {
            let cfg = VmConfig::new(2, 1 << 30);
            let vm = Vm::new(CpuVendor::Amd, cfg).unwrap();
            assert_eq!(vm.config().vcpu_count, 2);
            assert_eq!(vm.config().memory_size, 1 << 30);
        });
    }

    // --- VmExitAction ---

    #[test]
    fn vm_exit_action_equality() {
        assert_eq!(VmExitAction::Continue, VmExitAction::Continue);
        assert_ne!(VmExitAction::Halt, VmExitAction::Shutdown);
    }

    // --- VmState ---

    #[test]
    fn vm_state_equality() {
        assert_eq!(VmState::Running, VmState::Running);
        assert_ne!(VmState::Created, VmState::Paused);
    }
}
