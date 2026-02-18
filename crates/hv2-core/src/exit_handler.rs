//! VM Exit Handler Framework
//!
//! This module provides a framework for handling VM exits in a structured way.
//! It defines traits and implementations for processing different exit types
//! including I/O port access, MMIO, interrupts, and CPU exceptions.

use crate::{
    address_space::GuestAddressSpace, cpuid::CpuidEmulator, exit::VmExit, interrupt::Pic8259,
    vcpu::RegisterSet, Error, Result,
};
use std::sync::Arc;

/// Result of handling a VM exit
#[derive(Debug, Clone)]
pub enum ExitHandlerResult {
    /// Continue executing the vCPU
    Continue,
    /// Continue with modified data (for reads)
    ContinueWithData(Vec<u8>),
    /// Inject an interrupt before continuing
    InjectInterrupt(u8),
    /// Request interrupt window exit
    RequestInterruptWindow,
    /// Halt the vCPU (waiting for interrupt)
    Halt,
    /// Shutdown the VM
    Shutdown,
    /// Error occurred
    Error(String),
}

/// Context for exit handling
pub struct ExitContext<'a> {
    /// vCPU registers (read-only snapshot)
    pub registers: &'a RegisterSet,
    /// vCPU ID
    pub vcpu_id: u32,
}

/// Trait for handling VM exits
pub trait VmExitHandler: Send + Sync {
    /// Handle an I/O port read
    fn handle_io_in(&self, port: u16, size: u8, ctx: &ExitContext) -> Result<ExitHandlerResult>;

    /// Handle an I/O port write
    fn handle_io_out(
        &self,
        port: u16,
        size: u8,
        data: u32,
        ctx: &ExitContext,
    ) -> Result<ExitHandlerResult>;

    /// Handle an MMIO read
    fn handle_mmio_read(
        &self,
        addr: u64,
        size: u32,
        ctx: &ExitContext,
    ) -> Result<ExitHandlerResult>;

    /// Handle an MMIO write
    fn handle_mmio_write(
        &self,
        addr: u64,
        data: &[u8],
        ctx: &ExitContext,
    ) -> Result<ExitHandlerResult>;

    /// Handle HLT instruction
    fn handle_hlt(&self, ctx: &ExitContext) -> Result<ExitHandlerResult>;

    /// Handle interrupt window
    fn handle_interrupt_window(&self, ctx: &ExitContext) -> Result<ExitHandlerResult>;

    /// Handle exception
    fn handle_exception(
        &self,
        vector: u8,
        error_code: Option<u32>,
        ctx: &ExitContext,
    ) -> Result<ExitHandlerResult>;

    /// Handle CPUID instruction
    fn handle_cpuid(&self, leaf: u32, subleaf: u32, ctx: &ExitContext)
        -> Result<ExitHandlerResult>;

    /// Handle shutdown request
    fn handle_shutdown(&self, ctx: &ExitContext) -> Result<ExitHandlerResult>;

    /// Handle unknown exit
    fn handle_unknown(&self, reason: u32, ctx: &ExitContext) -> Result<ExitHandlerResult>;
}

/// Standard VM exit handler implementation
pub struct StandardExitHandler {
    /// PIC for interrupt handling
    pic: Arc<Pic8259>,
    /// CPUID emulator
    cpuid: CpuidEmulator,
    /// Address space for MMIO handling
    address_space: Option<Arc<GuestAddressSpace>>,
    /// Pending interrupt injection
    pending_interrupt: parking_lot::Mutex<Option<u8>>,
}

impl StandardExitHandler {
    /// Create a new standard exit handler
    pub fn new(pic: Arc<Pic8259>) -> Self {
        Self {
            pic,
            cpuid: CpuidEmulator::new(),
            address_space: None,
            pending_interrupt: parking_lot::Mutex::new(None),
        }
    }

    /// Set the address space for MMIO handling
    pub fn with_address_space(mut self, space: Arc<GuestAddressSpace>) -> Self {
        self.address_space = Some(space);
        self
    }

    /// Set custom CPUID emulator
    pub fn with_cpuid(mut self, cpuid: CpuidEmulator) -> Self {
        self.cpuid = cpuid;
        self
    }

    /// Get the PIC
    pub fn pic(&self) -> &Arc<Pic8259> {
        &self.pic
    }

    /// Get the CPUID emulator
    pub fn cpuid(&self) -> &CpuidEmulator {
        &self.cpuid
    }

    /// Check if there's a pending interrupt to inject
    pub fn has_pending_interrupt(&self) -> bool {
        self.pending_interrupt.lock().is_some()
    }

    /// Get and clear the pending interrupt
    pub fn take_pending_interrupt(&self) -> Option<u8> {
        self.pending_interrupt.lock().take()
    }

    /// Queue an interrupt for injection
    pub fn queue_interrupt(&self, vector: u8) {
        *self.pending_interrupt.lock() = Some(vector);
    }

    /// Handle a complete VM exit
    pub fn handle_exit(&self, exit: &VmExit, ctx: &ExitContext) -> Result<ExitHandlerResult> {
        match exit {
            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => match direction {
                crate::IoDirection::In => self.handle_io_in(*port, *size, ctx),
                crate::IoDirection::Out => self.handle_io_out(*port, *size, *data, ctx),
            },
            VmExit::Mmio {
                phys_addr,
                data,
                len,
                is_write,
            } => {
                if *is_write {
                    self.handle_mmio_write(*phys_addr, &data[..*len as usize], ctx)
                } else {
                    self.handle_mmio_read(*phys_addr, *len, ctx)
                }
            }
            VmExit::Hlt => self.handle_hlt(ctx),
            VmExit::InterruptWindow => self.handle_interrupt_window(ctx),
            VmExit::Exception { vector, error_code } => {
                self.handle_exception(*vector, *error_code, ctx)
            }
            VmExit::Shutdown => self.handle_shutdown(ctx),
            VmExit::Unknown { reason } => self.handle_unknown(*reason, ctx),
            VmExit::Debug { .. } => Ok(ExitHandlerResult::Continue),
            VmExit::Hypercall { .. }
            | VmExit::SystemEvent { .. }
            | VmExit::Nmi
            | VmExit::Rdmsr { .. }
            | VmExit::Wrmsr { .. }
            | VmExit::IoapicEoi { .. } => Ok(ExitHandlerResult::Continue),
        }
    }
}

impl VmExitHandler for StandardExitHandler {
    fn handle_io_in(&self, port: u16, size: u8, _ctx: &ExitContext) -> Result<ExitHandlerResult> {
        // Handle PIC ports
        match port {
            // Master PIC
            0x20 | 0x21 => {
                let value = self.pic.read_port_sync(port);
                let mut data = vec![0u8; size as usize];
                data[0] = value;
                Ok(ExitHandlerResult::ContinueWithData(data))
            }
            // Slave PIC
            0xA0 | 0xA1 => {
                let value = self.pic.read_port_sync(port);
                let mut data = vec![0u8; size as usize];
                data[0] = value;
                Ok(ExitHandlerResult::ContinueWithData(data))
            }
            // Unknown port - return 0xFF (typical for unhandled ports)
            _ => {
                tracing::trace!("Unhandled IO IN port={:#x} size={}", port, size);
                let data = vec![0xFF; size as usize];
                Ok(ExitHandlerResult::ContinueWithData(data))
            }
        }
    }

    fn handle_io_out(
        &self,
        port: u16,
        _size: u8,
        data: u32,
        _ctx: &ExitContext,
    ) -> Result<ExitHandlerResult> {
        let value = data as u8;

        // Handle PIC ports
        match port {
            // Master PIC
            0x20 | 0x21 => {
                self.pic.write_port_sync(port, value);
                Ok(ExitHandlerResult::Continue)
            }
            // Slave PIC
            0xA0 | 0xA1 => {
                self.pic.write_port_sync(port, value);
                Ok(ExitHandlerResult::Continue)
            }
            // Debug port (common in VMs)
            0x80 => {
                tracing::trace!("Debug port: {:#x}", value);
                Ok(ExitHandlerResult::Continue)
            }
            // Unknown port - ignore
            _ => {
                tracing::trace!("Unhandled IO OUT port={:#x} value={:#x}", port, value);
                Ok(ExitHandlerResult::Continue)
            }
        }
    }

    fn handle_mmio_read(
        &self,
        addr: u64,
        size: u32,
        _ctx: &ExitContext,
    ) -> Result<ExitHandlerResult> {
        // Check if address space is configured
        if let Some(space) = &self.address_space {
            if space.is_mmio(addr) {
                // MMIO access - would be routed to device
                tracing::trace!("MMIO read at {:#x} size={}", addr, size);
                // Return zeros for now (device should handle)
                let data = vec![0u8; size as usize];
                return Ok(ExitHandlerResult::ContinueWithData(data));
            }
        }

        // Default: return zeros
        let data = vec![0u8; size as usize];
        Ok(ExitHandlerResult::ContinueWithData(data))
    }

    fn handle_mmio_write(
        &self,
        addr: u64,
        data: &[u8],
        _ctx: &ExitContext,
    ) -> Result<ExitHandlerResult> {
        // Check if address space is configured
        if let Some(space) = &self.address_space {
            if space.is_mmio(addr) {
                // MMIO access - would be routed to device
                tracing::trace!("MMIO write at {:#x} len={}", addr, data.len());
                return Ok(ExitHandlerResult::Continue);
            }
        }

        // Default: ignore
        Ok(ExitHandlerResult::Continue)
    }

    fn handle_hlt(&self, _ctx: &ExitContext) -> Result<ExitHandlerResult> {
        // Check for pending interrupts
        if let Some(vector) = self.pic.get_pending_interrupt() {
            if let Err(e) = self.pic.acknowledge_interrupt(vector) {
                tracing::warn!(vector, error = %e, "PIC acknowledge failed in HLT handler");
            }
            return Ok(ExitHandlerResult::InjectInterrupt(vector));
        }

        // No interrupt pending, halt until one arrives
        Ok(ExitHandlerResult::Halt)
    }

    fn handle_interrupt_window(&self, _ctx: &ExitContext) -> Result<ExitHandlerResult> {
        // Interrupt window opened - check for pending interrupts
        if let Some(vector) = self.pic.get_pending_interrupt() {
            if let Err(e) = self.pic.acknowledge_interrupt(vector) {
                tracing::warn!(vector, error = %e, "PIC acknowledge failed in interrupt window handler");
            }
            return Ok(ExitHandlerResult::InjectInterrupt(vector));
        }

        // Check if we queued an interrupt manually
        if let Some(vector) = self.take_pending_interrupt() {
            return Ok(ExitHandlerResult::InjectInterrupt(vector));
        }

        Ok(ExitHandlerResult::Continue)
    }

    fn handle_exception(
        &self,
        vector: u8,
        error_code: Option<u32>,
        _ctx: &ExitContext,
    ) -> Result<ExitHandlerResult> {
        match vector {
            // Divide error (#DE)
            0 => {
                tracing::warn!("Divide error exception");
                Ok(ExitHandlerResult::Shutdown)
            }
            // Debug (#DB)
            1 => {
                tracing::trace!("Debug exception");
                Ok(ExitHandlerResult::Continue)
            }
            // Breakpoint (#BP)
            3 => {
                tracing::trace!("Breakpoint exception");
                Ok(ExitHandlerResult::Continue)
            }
            // Invalid opcode (#UD)
            6 => {
                tracing::warn!("Invalid opcode exception");
                Ok(ExitHandlerResult::Shutdown)
            }
            // Device not available (#NM)
            7 => {
                tracing::trace!("Device not available exception");
                Ok(ExitHandlerResult::Continue)
            }
            // Double fault (#DF)
            8 => {
                tracing::error!("Double fault! error_code={:?}", error_code);
                Ok(ExitHandlerResult::Shutdown)
            }
            // Invalid TSS (#TS)
            10 => {
                tracing::warn!("Invalid TSS exception, error_code={:?}", error_code);
                Ok(ExitHandlerResult::Shutdown)
            }
            // Segment not present (#NP)
            11 => {
                tracing::warn!("Segment not present, error_code={:?}", error_code);
                Ok(ExitHandlerResult::Shutdown)
            }
            // Stack-segment fault (#SS)
            12 => {
                tracing::warn!("Stack-segment fault, error_code={:?}", error_code);
                Ok(ExitHandlerResult::Shutdown)
            }
            // General protection fault (#GP)
            13 => {
                tracing::warn!("General protection fault, error_code={:?}", error_code);
                Ok(ExitHandlerResult::Shutdown)
            }
            // Page fault (#PF)
            14 => {
                tracing::trace!("Page fault, error_code={:?}", error_code);
                // In a real hypervisor, we'd handle this based on the fault address
                Ok(ExitHandlerResult::Shutdown)
            }
            // Other exceptions
            _ => {
                tracing::trace!("Exception vector={}, error_code={:?}", vector, error_code);
                Ok(ExitHandlerResult::Continue)
            }
        }
    }

    fn handle_cpuid(
        &self,
        leaf: u32,
        subleaf: u32,
        _ctx: &ExitContext,
    ) -> Result<ExitHandlerResult> {
        let result = self.cpuid.execute(leaf, subleaf);

        // Return CPUID result packed as bytes
        let mut data = Vec::with_capacity(16);
        data.extend_from_slice(&result.eax.to_le_bytes());
        data.extend_from_slice(&result.ebx.to_le_bytes());
        data.extend_from_slice(&result.ecx.to_le_bytes());
        data.extend_from_slice(&result.edx.to_le_bytes());

        Ok(ExitHandlerResult::ContinueWithData(data))
    }

    fn handle_shutdown(&self, _ctx: &ExitContext) -> Result<ExitHandlerResult> {
        tracing::info!("VM shutdown requested");
        Ok(ExitHandlerResult::Shutdown)
    }

    fn handle_unknown(&self, reason: u32, _ctx: &ExitContext) -> Result<ExitHandlerResult> {
        tracing::warn!("Unknown VM exit reason: {}", reason);
        Err(Error::Cpu(format!("Unknown VM exit reason: {}", reason)))
    }
}

/// Interrupt injection state for a vCPU
#[derive(Debug, Clone, Default)]
pub struct InterruptState {
    /// Interrupts are enabled (RFLAGS.IF = 1)
    pub interrupts_enabled: bool,
    /// Currently in interrupt shadow (STI/MOV SS)
    pub interrupt_shadow: bool,
    /// Interrupt pending to inject
    pub pending_interrupt: Option<u8>,
    /// Need to request interrupt window
    pub request_interrupt_window: bool,
}

impl InterruptState {
    /// Check if an interrupt can be injected now
    pub fn can_inject(&self) -> bool {
        self.interrupts_enabled && !self.interrupt_shadow
    }

    /// Try to inject a pending interrupt
    ///
    /// Returns the vector to inject if one is available and injection is possible
    pub fn try_inject(&mut self) -> Option<u8> {
        if self.can_inject() {
            self.pending_interrupt.take()
        } else if self.pending_interrupt.is_some() {
            self.request_interrupt_window = true;
            None
        } else {
            None
        }
    }

    /// Queue an interrupt for injection
    pub fn queue_interrupt(&mut self, vector: u8) {
        self.pending_interrupt = Some(vector);
        if !self.can_inject() {
            self.request_interrupt_window = true;
        }
    }

    /// Update state from RFLAGS
    pub fn update_from_rflags(&mut self, rflags: u64) {
        self.interrupts_enabled = rflags & (1 << 9) != 0; // IF flag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_handler() -> StandardExitHandler {
        StandardExitHandler::new(Arc::new(Pic8259::new()))
    }

    fn create_context() -> (RegisterSet, ExitContext<'static>) {
        let registers = Box::leak(Box::new(RegisterSet::default()));
        let ctx = ExitContext {
            registers,
            vcpu_id: 0,
        };
        (registers.clone(), ctx)
    }

    #[test]
    fn test_io_in_unknown_port() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_io_in(0x1234, 1, &ctx).unwrap();
        if let ExitHandlerResult::ContinueWithData(data) = result {
            assert_eq!(data, vec![0xFF]); // Unknown ports return 0xFF
        } else {
            panic!("Expected ContinueWithData");
        }
    }

    #[test]
    fn test_io_out_debug_port() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_io_out(0x80, 1, 0x42, &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Continue));
    }

    #[test]
    fn test_hlt_no_interrupt() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_hlt(&ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Halt));
    }

    #[test]
    fn test_hlt_with_pending_interrupt() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        // Set up a pending interrupt (unmask all by setting mask to 0)
        handler.pic.set_master_mask(0);
        handler.pic.set_slave_mask(0);
        handler.pic.raise_irq(0).unwrap(); // Timer interrupt

        let result = handler.handle_hlt(&ctx).unwrap();
        if let ExitHandlerResult::InjectInterrupt(vector) = result {
            assert_eq!(vector, 0x20); // Timer vector
        } else {
            panic!("Expected InjectInterrupt");
        }
    }

    #[test]
    fn test_exception_double_fault() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_exception(8, Some(0), &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Shutdown));
    }

    #[test]
    fn test_exception_breakpoint() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_exception(3, None, &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Continue));
    }

    #[test]
    fn test_cpuid_handling() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_cpuid(0, 0, &ctx).unwrap();
        if let ExitHandlerResult::ContinueWithData(data) = result {
            assert_eq!(data.len(), 16); // 4 x u32
        } else {
            panic!("Expected ContinueWithData");
        }
    }

    #[test]
    fn test_shutdown() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        let result = handler.handle_shutdown(&ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Shutdown));
    }

    #[test]
    fn test_interrupt_state_can_inject() {
        let mut state = InterruptState::default();

        // By default, interrupts disabled
        assert!(!state.can_inject());

        // Enable interrupts
        state.interrupts_enabled = true;
        assert!(state.can_inject());

        // Add interrupt shadow
        state.interrupt_shadow = true;
        assert!(!state.can_inject());
    }

    #[test]
    fn test_interrupt_state_try_inject() {
        let mut state = InterruptState {
            interrupts_enabled: true,
            interrupt_shadow: false,
            pending_interrupt: Some(0x20),
            request_interrupt_window: false,
        };

        // Should inject successfully
        let vector = state.try_inject();
        assert_eq!(vector, Some(0x20));
        assert!(state.pending_interrupt.is_none());
    }

    #[test]
    fn test_interrupt_state_deferred_inject() {
        let mut state = InterruptState {
            interrupts_enabled: false, // Interrupts disabled
            interrupt_shadow: false,
            pending_interrupt: Some(0x20),
            request_interrupt_window: false,
        };

        // Cannot inject, should request window
        let vector = state.try_inject();
        assert_eq!(vector, None);
        assert!(state.request_interrupt_window);
        assert!(state.pending_interrupt.is_some()); // Still pending
    }

    #[test]
    fn test_interrupt_state_update_from_rflags() {
        let mut state = InterruptState::default();

        // IF flag (bit 9)
        state.update_from_rflags(0x200); // IF=1
        assert!(state.interrupts_enabled);

        state.update_from_rflags(0x002); // IF=0
        assert!(!state.interrupts_enabled);
    }

    #[test]
    fn test_handler_pic_io() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        // Write to PIC command port
        let result = handler.handle_io_out(0x20, 1, 0x20, &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Continue));

        // Read from PIC status port
        let result = handler.handle_io_in(0x20, 1, &ctx).unwrap();
        if let ExitHandlerResult::ContinueWithData(data) = result {
            assert_eq!(data.len(), 1);
        } else {
            panic!("Expected ContinueWithData");
        }
    }

    #[test]
    fn test_handler_with_address_space() {
        let space = Arc::new(GuestAddressSpace::new());
        space
            .add_region(crate::AddressRegion::mmio(0xFEE0_0000, 0x1000, "APIC"))
            .unwrap();

        let handler = create_handler().with_address_space(space);
        let (_, ctx) = create_context();

        // MMIO read from APIC region
        let result = handler.handle_mmio_read(0xFEE0_0000, 4, &ctx).unwrap();
        if let ExitHandlerResult::ContinueWithData(data) = result {
            assert_eq!(data.len(), 4);
        } else {
            panic!("Expected ContinueWithData");
        }
    }

    #[test]
    fn test_handle_exit_dispatch() {
        let handler = create_handler();
        let (_, ctx) = create_context();

        // Test various exit types
        let exit = VmExit::Hlt;
        let result = handler.handle_exit(&exit, &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Halt));

        let exit = VmExit::Shutdown;
        let result = handler.handle_exit(&exit, &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::Shutdown));

        let exit = VmExit::io_in(0x80, 1);
        let result = handler.handle_exit(&exit, &ctx).unwrap();
        assert!(matches!(result, ExitHandlerResult::ContinueWithData(_)));
    }
}
