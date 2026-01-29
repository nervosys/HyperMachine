//! Debug Module
//!
//! This module provides debugging and introspection capabilities for VMs,
//! including GDB stub support and memory/CPU inspection tools.

pub mod gdb;
pub mod introspection;

pub use gdb::{
    Breakpoint, BreakpointType, GdbError, GdbRegister, GdbRegisters, GdbResult, GdbStats, GdbStub,
    GdbTarget, PacketParser, PacketState, StopReason,
};

pub use introspection::{
    CpuInspector, CpuMode, CpuState, GateType, IdtInspector, InterruptDescriptor,
    IntrospectionEvent, MemoryAttributes, MemoryInspector, MemoryRegion, MemoryRegionType,
    PageTableEntry, PageTableWalker, PageWalkError, PageWalkResult, SegmentState, TableState,
};

use std::sync::{Arc, RwLock};

/// Debug manager for coordinating debugging operations
#[derive(Debug)]
pub struct DebugManager {
    /// GDB stub
    gdb_stub: Arc<GdbStub>,
    /// Memory inspector
    memory_inspector: Arc<RwLock<MemoryInspector>>,
    /// CPU inspector
    cpu_inspector: Arc<CpuInspector>,
    /// Page table walker
    page_walker: Arc<RwLock<PageTableWalker>>,
    /// IDT inspector
    idt_inspector: Arc<RwLock<IdtInspector>>,
    /// Debug enabled
    enabled: std::sync::atomic::AtomicBool,
}

impl DebugManager {
    /// Create new debug manager
    pub fn new() -> Self {
        Self {
            gdb_stub: Arc::new(GdbStub::new()),
            memory_inspector: Arc::new(RwLock::new(MemoryInspector::new())),
            cpu_inspector: Arc::new(CpuInspector::new()),
            page_walker: Arc::new(RwLock::new(PageTableWalker::new())),
            idt_inspector: Arc::new(RwLock::new(IdtInspector::new())),
            enabled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Enable debugging
    pub fn enable(&self) {
        self.enabled
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Disable debugging
    pub fn disable(&self) {
        self.enabled
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// Check if debugging is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Get GDB stub
    pub fn gdb_stub(&self) -> &Arc<GdbStub> {
        &self.gdb_stub
    }

    /// Get memory inspector
    pub fn memory_inspector(&self) -> &Arc<RwLock<MemoryInspector>> {
        &self.memory_inspector
    }

    /// Get CPU inspector
    pub fn cpu_inspector(&self) -> &Arc<CpuInspector> {
        &self.cpu_inspector
    }

    /// Get page table walker
    pub fn page_walker(&self) -> &Arc<RwLock<PageTableWalker>> {
        &self.page_walker
    }

    /// Get IDT inspector
    pub fn idt_inspector(&self) -> &Arc<RwLock<IdtInspector>> {
        &self.idt_inspector
    }

    /// Add breakpoint
    pub fn add_breakpoint(&self, bp_type: BreakpointType, addr: u64, size: u64) -> u64 {
        self.gdb_stub.add_breakpoint(bp_type, addr, size)
    }

    /// Remove breakpoint
    pub fn remove_breakpoint(&self, addr: u64) -> Option<Breakpoint> {
        self.gdb_stub.remove_breakpoint(addr)
    }

    /// Check if address has breakpoint
    pub fn has_breakpoint(&self, addr: u64) -> bool {
        self.gdb_stub.has_breakpoint(addr)
    }

    /// Log introspection event
    pub fn log_event(&self, event: IntrospectionEvent) {
        self.cpu_inspector.log_event(event);
    }

    /// Update CPU state for vCPU
    pub fn update_cpu_state(&self, vcpu_id: u32, state: CpuState) {
        self.cpu_inspector.update_state(vcpu_id, state);
    }

    /// Get CPU state for vCPU
    pub fn get_cpu_state(&self, vcpu_id: u32) -> Option<CpuState> {
        self.cpu_inspector.get_state(vcpu_id)
    }

    /// Add memory region
    pub fn add_memory_region(&self, region: MemoryRegion) {
        self.memory_inspector.write().unwrap().add_region(region);
    }

    /// Walk page tables
    pub fn walk_page_tables(&self, cr3: u64, vaddr: u64, is_long_mode: bool) -> PageWalkResult {
        self.page_walker
            .read()
            .unwrap()
            .walk(cr3, vaddr, is_long_mode)
    }

    /// Read interrupt descriptor
    pub fn read_idt_entry(
        &self,
        idtr: &TableState,
        vector: u8,
        is_long_mode: bool,
    ) -> Option<InterruptDescriptor> {
        self.idt_inspector
            .read()
            .unwrap()
            .read_descriptor(idtr, vector, is_long_mode)
    }

    /// Get debug statistics
    pub fn stats(&self) -> DebugStats {
        let gdb_stats = self.gdb_stub.stats();
        let regions = self.memory_inspector.read().unwrap().regions().len();
        let vcpus = self.cpu_inspector.all_states().len();
        let events = self.cpu_inspector.event_count();

        DebugStats {
            enabled: self.is_enabled(),
            gdb_connected: gdb_stats.connected,
            breakpoint_count: gdb_stats.breakpoint_count,
            memory_regions: regions,
            tracked_vcpus: vcpus,
            event_count: events,
        }
    }
}

impl Default for DebugManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Debug statistics
#[derive(Debug, Clone)]
pub struct DebugStats {
    /// Debug enabled
    pub enabled: bool,
    /// GDB connected
    pub gdb_connected: bool,
    /// Number of breakpoints
    pub breakpoint_count: usize,
    /// Number of memory regions
    pub memory_regions: usize,
    /// Number of tracked vCPUs
    pub tracked_vcpus: usize,
    /// Total events logged
    pub event_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_manager_creation() {
        let manager = DebugManager::new();
        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_debug_manager_enable_disable() {
        let manager = DebugManager::new();

        manager.enable();
        assert!(manager.is_enabled());

        manager.disable();
        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_debug_manager_breakpoints() {
        let manager = DebugManager::new();

        let id = manager.add_breakpoint(BreakpointType::Software, 0x1000, 1);
        assert!(id > 0);
        assert!(manager.has_breakpoint(0x1000));

        manager.remove_breakpoint(0x1000);
        assert!(!manager.has_breakpoint(0x1000));
    }

    #[test]
    fn test_debug_manager_cpu_state() {
        let manager = DebugManager::new();

        let mut state = CpuState::default();
        state.rip = 0xDEADBEEF;
        manager.update_cpu_state(0, state);

        let retrieved = manager.get_cpu_state(0).unwrap();
        assert_eq!(retrieved.rip, 0xDEADBEEF);
    }

    #[test]
    fn test_debug_manager_event_logging() {
        let manager = DebugManager::new();

        manager.log_event(IntrospectionEvent::Breakpoint(0x1000));
        manager.log_event(IntrospectionEvent::Exception {
            vector: 14,
            error_code: Some(0x2),
        });

        let stats = manager.stats();
        assert_eq!(stats.event_count, 2);
    }

    #[test]
    fn test_debug_manager_memory_regions() {
        let manager = DebugManager::new();

        manager.add_memory_region(MemoryRegion::new(
            0x0,
            0x1000,
            MemoryRegionType::Ram,
            "low_mem",
        ));

        let stats = manager.stats();
        assert_eq!(stats.memory_regions, 1);
    }

    #[test]
    fn test_debug_stats() {
        let manager = DebugManager::new();
        manager.enable();
        manager.add_breakpoint(BreakpointType::Software, 0x1000, 1);
        manager.add_breakpoint(BreakpointType::HardwareExec, 0x2000, 1);

        let stats = manager.stats();
        assert!(stats.enabled);
        assert_eq!(stats.breakpoint_count, 2);
    }

    #[test]
    fn test_debug_manager_accessors() {
        let manager = DebugManager::new();

        // Verify all accessors return valid references
        let _ = manager.gdb_stub();
        let _ = manager.memory_inspector();
        let _ = manager.cpu_inspector();
        let _ = manager.page_walker();
        let _ = manager.idt_inspector();
    }
}
