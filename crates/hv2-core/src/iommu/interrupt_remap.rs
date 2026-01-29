//! Interrupt Remapping Support
//!
//! This module provides interrupt remapping support for both
//! Intel VT-d and AMD IOMMU platforms.

use super::types::{DeviceId, DomainId, IommuStats};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Interrupt Remapping Table Entry (IRTE) - Intel format
#[derive(Debug, Clone, Copy)]
pub struct IntelIrte {
    /// Lower 64 bits
    lo: u64,
    /// Upper 64 bits
    hi: u64,
}

impl IntelIrte {
    // IRTE low bits
    const PRESENT: u64 = 1 << 0;
    const FPD: u64 = 1 << 1; // Fault processing disable
    const DM_MASK: u64 = 1 << 2; // Destination mode
    const RH: u64 = 1 << 3; // Redirection hint
    const TM: u64 = 1 << 4; // Trigger mode (0=edge, 1=level)
    const DLM_MASK: u64 = 0x07 << 5; // Delivery mode
    const DLM_SHIFT: u64 = 5;
    const AVAIL_MASK: u64 = 0x0F << 8; // Available for software
    const VECTOR_MASK: u64 = 0xFF << 16; // Interrupt vector
    const VECTOR_SHIFT: u64 = 16;
    const DST_MASK: u64 = 0xFFFF_FFFF << 32; // Destination ID

    // IRTE high bits
    const SID_MASK: u64 = 0xFFFF; // Source ID
    const SQ_MASK: u64 = 0x03 << 16; // Source ID qualifier
    const SVT_MASK: u64 = 0x03 << 18; // Source validation type

    /// Create empty entry
    pub const fn empty() -> Self {
        Self { lo: 0, hi: 0 }
    }

    /// Create present entry
    pub fn new(vector: u8, destination: u32, delivery_mode: DeliveryMode) -> Self {
        Self {
            lo: Self::PRESENT
                | ((vector as u64) << Self::VECTOR_SHIFT)
                | ((destination as u64) << 32)
                | ((delivery_mode as u64) << Self::DLM_SHIFT),
            hi: 0,
        }
    }

    /// Create with source validation
    pub fn with_source(mut self, source_id: u16, validation: SourceValidation) -> Self {
        self.hi = (source_id as u64) | ((validation as u64) << 18);
        self
    }

    /// Check if present
    pub const fn is_present(&self) -> bool {
        (self.lo & Self::PRESENT) != 0
    }

    /// Get vector
    pub const fn vector(&self) -> u8 {
        ((self.lo & Self::VECTOR_MASK) >> Self::VECTOR_SHIFT) as u8
    }

    /// Get destination
    pub const fn destination(&self) -> u32 {
        ((self.lo & Self::DST_MASK) >> 32) as u32
    }

    /// Get delivery mode
    pub fn delivery_mode(&self) -> DeliveryMode {
        DeliveryMode::from_u8(((self.lo & Self::DLM_MASK) >> Self::DLM_SHIFT) as u8)
    }

    /// Get source ID
    pub const fn source_id(&self) -> u16 {
        (self.hi & Self::SID_MASK) as u16
    }

    /// Set vector
    pub fn set_vector(&mut self, vector: u8) {
        self.lo = (self.lo & !Self::VECTOR_MASK) | ((vector as u64) << Self::VECTOR_SHIFT);
    }

    /// Set destination
    pub fn set_destination(&mut self, dest: u32) {
        self.lo = (self.lo & !Self::DST_MASK) | ((dest as u64) << 32);
    }

    /// Check if fault processing disabled
    pub const fn is_fpd(&self) -> bool {
        (self.lo & Self::FPD) != 0
    }

    /// Set fault processing disable
    pub fn set_fpd(&mut self, fpd: bool) {
        if fpd {
            self.lo |= Self::FPD;
        } else {
            self.lo &= !Self::FPD;
        }
    }

    /// Get raw lower bits
    pub const fn lo(&self) -> u64 {
        self.lo
    }

    /// Get raw upper bits
    pub const fn hi(&self) -> u64 {
        self.hi
    }
}

/// AMD Interrupt Remapping Table Entry
#[derive(Debug, Clone, Copy)]
pub struct AmdIrte {
    /// Data (32 bits)
    data: u32,
    /// Reserved/Guest mode fields
    guest: u32,
}

impl AmdIrte {
    // IRTE fields
    const REMAP_EN: u32 = 1 << 0;
    const SUP_IO_PF: u32 = 1 << 1; // Suppress IO page fault
    const INT_TYPE_MASK: u32 = 0x07 << 2;
    const INT_TYPE_SHIFT: u32 = 2;
    const RQ_EOI: u32 = 1 << 5; // Request EOI
    const DM: u32 = 1 << 6; // Destination mode
    const GUEST_MODE: u32 = 1 << 7;
    const DEST_MASK: u32 = 0xFF << 8; // Destination APIC ID
    const DEST_SHIFT: u32 = 8;
    const VECTOR_MASK: u32 = 0xFF << 16;
    const VECTOR_SHIFT: u32 = 16;

    /// Create empty entry
    pub const fn empty() -> Self {
        Self { data: 0, guest: 0 }
    }

    /// Create remapped entry
    pub fn new(vector: u8, destination: u8, int_type: InterruptType) -> Self {
        Self {
            data: Self::REMAP_EN
                | ((int_type as u32) << Self::INT_TYPE_SHIFT)
                | ((destination as u32) << Self::DEST_SHIFT)
                | ((vector as u32) << Self::VECTOR_SHIFT),
            guest: 0,
        }
    }

    /// Check if remapping enabled
    pub const fn is_enabled(&self) -> bool {
        (self.data & Self::REMAP_EN) != 0
    }

    /// Get vector
    pub const fn vector(&self) -> u8 {
        ((self.data & Self::VECTOR_MASK) >> Self::VECTOR_SHIFT) as u8
    }

    /// Get destination
    pub const fn destination(&self) -> u8 {
        ((self.data & Self::DEST_MASK) >> Self::DEST_SHIFT) as u8
    }

    /// Get interrupt type
    pub fn interrupt_type(&self) -> InterruptType {
        InterruptType::from_u8(((self.data & Self::INT_TYPE_MASK) >> Self::INT_TYPE_SHIFT) as u8)
    }

    /// Set vector
    pub fn set_vector(&mut self, vector: u8) {
        self.data = (self.data & !Self::VECTOR_MASK) | ((vector as u32) << Self::VECTOR_SHIFT);
    }

    /// Set destination
    pub fn set_destination(&mut self, dest: u8) {
        self.data = (self.data & !Self::DEST_MASK) | ((dest as u32) << Self::DEST_SHIFT);
    }

    /// Check guest mode
    pub const fn is_guest_mode(&self) -> bool {
        (self.data & Self::GUEST_MODE) != 0
    }

    /// Get raw data
    pub const fn raw(&self) -> u32 {
        self.data
    }
}

/// Interrupt delivery mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryMode {
    /// Fixed delivery
    Fixed = 0,
    /// Lowest priority
    LowestPriority = 1,
    /// SMI
    Smi = 2,
    /// Reserved
    Reserved1 = 3,
    /// NMI
    Nmi = 4,
    /// INIT
    Init = 5,
    /// Reserved
    Reserved2 = 6,
    /// ExtINT
    ExtInt = 7,
}

impl DeliveryMode {
    /// Convert from raw value
    pub fn from_u8(value: u8) -> Self {
        match value & 0x07 {
            0 => DeliveryMode::Fixed,
            1 => DeliveryMode::LowestPriority,
            2 => DeliveryMode::Smi,
            3 => DeliveryMode::Reserved1,
            4 => DeliveryMode::Nmi,
            5 => DeliveryMode::Init,
            6 => DeliveryMode::Reserved2,
            7 => DeliveryMode::ExtInt,
            _ => DeliveryMode::Fixed,
        }
    }
}

/// AMD interrupt type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterruptType {
    /// Fixed delivery
    Fixed = 0,
    /// Arbitrated (lowest priority)
    Arbitrated = 1,
    /// Reserved
    Reserved1 = 2,
    /// Reserved
    Reserved2 = 3,
    /// NMI
    Nmi = 4,
    /// Reset
    Reset = 5,
    /// Reserved
    Reserved3 = 6,
    /// ExtINT
    ExtInt = 7,
}

impl InterruptType {
    /// Convert from raw value
    pub fn from_u8(value: u8) -> Self {
        match value & 0x07 {
            0 => InterruptType::Fixed,
            1 => InterruptType::Arbitrated,
            2 => InterruptType::Reserved1,
            3 => InterruptType::Reserved2,
            4 => InterruptType::Nmi,
            5 => InterruptType::Reset,
            6 => InterruptType::Reserved3,
            7 => InterruptType::ExtInt,
            _ => InterruptType::Fixed,
        }
    }
}

/// Source validation type (Intel)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SourceValidation {
    /// No validation
    None = 0,
    /// Validate source ID
    ValidateSourceId = 1,
    /// Validate bus range
    ValidateBusRange = 2,
    /// Reserved
    Reserved = 3,
}

/// MSI/MSI-X message format
#[derive(Debug, Clone, Copy)]
pub struct MsiMessage {
    /// Address (typically 0xFEExxxxx)
    pub address: u64,
    /// Data (vector, delivery mode, etc.)
    pub data: u32,
}

impl MsiMessage {
    // Address fields (Intel format)
    const ADDR_BASE: u64 = 0xFEE00000;
    const DEST_ID_MASK: u64 = 0xFF << 12;
    const DEST_ID_SHIFT: u64 = 12;
    const RH: u64 = 1 << 3; // Redirection hint
    const DM: u64 = 1 << 2; // Destination mode

    // Data fields
    const VECTOR_MASK: u32 = 0xFF;
    const DELIVERY_MODE_MASK: u32 = 0x07 << 8;
    const DELIVERY_MODE_SHIFT: u32 = 8;
    const LEVEL: u32 = 1 << 14; // Level for level-triggered
    const TRIGGER_MODE: u32 = 1 << 15; // 0=edge, 1=level

    /// Create MSI message
    pub fn new(destination: u8, vector: u8, delivery_mode: DeliveryMode) -> Self {
        Self {
            address: Self::ADDR_BASE | ((destination as u64) << Self::DEST_ID_SHIFT),
            data: (vector as u32) | ((delivery_mode as u32) << Self::DELIVERY_MODE_SHIFT),
        }
    }

    /// Get destination ID
    pub const fn destination(&self) -> u8 {
        ((self.address & Self::DEST_ID_MASK) >> Self::DEST_ID_SHIFT) as u8
    }

    /// Get vector
    pub const fn vector(&self) -> u8 {
        (self.data & Self::VECTOR_MASK) as u8
    }

    /// Get delivery mode
    pub fn delivery_mode(&self) -> DeliveryMode {
        DeliveryMode::from_u8(((self.data & Self::DELIVERY_MODE_MASK) >> Self::DELIVERY_MODE_SHIFT) as u8)
    }

    /// Check if level triggered
    pub const fn is_level(&self) -> bool {
        (self.data & Self::TRIGGER_MODE) != 0
    }

    /// Set destination
    pub fn set_destination(&mut self, dest: u8) {
        self.address = (self.address & !Self::DEST_ID_MASK) | ((dest as u64) << Self::DEST_ID_SHIFT);
    }

    /// Set vector
    pub fn set_vector(&mut self, vector: u8) {
        self.data = (self.data & !Self::VECTOR_MASK) | (vector as u32);
    }
}

/// Interrupt remapping statistics
#[derive(Debug, Default)]
pub struct InterruptRemapStats {
    /// Remapped interrupts
    pub remapped: AtomicU64,
    /// Blocked interrupts
    pub blocked: AtomicU64,
    /// Passthrough interrupts
    pub passthrough: AtomicU64,
    /// Invalid requests
    pub invalid: AtomicU64,
}

impl InterruptRemapStats {
    /// Record remapped interrupt
    pub fn record_remapped(&self) {
        self.remapped.fetch_add(1, Ordering::Relaxed);
    }

    /// Record blocked interrupt
    pub fn record_blocked(&self) {
        self.blocked.fetch_add(1, Ordering::Relaxed);
    }

    /// Record passthrough interrupt
    pub fn record_passthrough(&self) {
        self.passthrough.fetch_add(1, Ordering::Relaxed);
    }

    /// Record invalid request
    pub fn record_invalid(&self) {
        self.invalid.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> InterruptRemapStatsSnapshot {
        InterruptRemapStatsSnapshot {
            remapped: self.remapped.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            passthrough: self.passthrough.load(Ordering::Relaxed),
            invalid: self.invalid.load(Ordering::Relaxed),
        }
    }
}

/// Statistics snapshot
#[derive(Debug, Clone, Default)]
pub struct InterruptRemapStatsSnapshot {
    /// Remapped interrupts
    pub remapped: u64,
    /// Blocked interrupts
    pub blocked: u64,
    /// Passthrough interrupts
    pub passthrough: u64,
    /// Invalid requests
    pub invalid: u64,
}

/// Intel Interrupt Remapping Table
pub struct IntelInterruptRemapTable {
    /// Table base address
    base_address: u64,
    /// Table entries
    entries: Vec<IntelIrte>,
    /// Extended mode (x2APIC)
    extended_mode: bool,
    /// Enabled
    enabled: AtomicBool,
    /// Statistics
    stats: InterruptRemapStats,
}

impl Default for IntelInterruptRemapTable {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl IntelInterruptRemapTable {
    /// Create new table
    pub fn new(size: usize) -> Self {
        Self {
            base_address: 0,
            entries: vec![IntelIrte::empty(); size],
            extended_mode: false,
            enabled: AtomicBool::new(false),
            stats: InterruptRemapStats::default(),
        }
    }

    /// Set base address
    pub fn set_base_address(&mut self, addr: u64) {
        self.base_address = addr;
    }

    /// Get base address
    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Enable remapping
    pub fn enable(&mut self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Disable remapping
    pub fn disable(&mut self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Set extended mode (x2APIC)
    pub fn set_extended_mode(&mut self, enabled: bool) {
        self.extended_mode = enabled;
    }

    /// Get entry
    pub fn entry(&self, index: usize) -> Option<&IntelIrte> {
        self.entries.get(index)
    }

    /// Set entry
    pub fn set_entry(&mut self, index: usize, entry: IntelIrte) {
        if index < self.entries.len() {
            self.entries[index] = entry;
        }
    }

    /// Remap interrupt
    pub fn remap(&self, source: &DeviceId, index: u16) -> Option<MsiMessage> {
        if !self.is_enabled() {
            self.stats.record_passthrough();
            return None;
        }

        let irte = self.entries.get(index as usize)?;
        if !irte.is_present() {
            self.stats.record_invalid();
            return None;
        }

        // Validate source if required
        if irte.source_id() != 0 && irte.source_id() != source.source_id() {
            self.stats.record_blocked();
            return None;
        }

        self.stats.record_remapped();

        Some(MsiMessage::new(
            irte.destination() as u8,
            irte.vector(),
            irte.delivery_mode(),
        ))
    }

    /// Get statistics
    pub fn stats(&self) -> &InterruptRemapStats {
        &self.stats
    }

    /// Get table size
    pub fn size(&self) -> usize {
        self.entries.len()
    }
}

/// AMD Interrupt Remapping Table
pub struct AmdInterruptRemapTable {
    /// Table base address
    base_address: u64,
    /// Table entries (indexed by device ID)
    entries: HashMap<u16, Vec<AmdIrte>>,
    /// Enabled
    enabled: AtomicBool,
    /// Statistics
    stats: InterruptRemapStats,
}

impl Default for AmdInterruptRemapTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AmdInterruptRemapTable {
    /// Create new table
    pub fn new() -> Self {
        Self {
            base_address: 0,
            entries: HashMap::new(),
            enabled: AtomicBool::new(false),
            stats: InterruptRemapStats::default(),
        }
    }

    /// Set base address
    pub fn set_base_address(&mut self, addr: u64) {
        self.base_address = addr;
    }

    /// Enable remapping
    pub fn enable(&mut self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Disable remapping
    pub fn disable(&mut self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Set device interrupt table
    pub fn set_device_table(&mut self, device_id: u16, entries: Vec<AmdIrte>) {
        self.entries.insert(device_id, entries);
    }

    /// Get entry for device
    pub fn entry(&self, device_id: u16, index: usize) -> Option<&AmdIrte> {
        self.entries.get(&device_id)?.get(index)
    }

    /// Set entry for device
    pub fn set_entry(&mut self, device_id: u16, index: usize, entry: AmdIrte) {
        if let Some(table) = self.entries.get_mut(&device_id) {
            if index < table.len() {
                table[index] = entry;
            }
        }
    }

    /// Remap interrupt
    pub fn remap(&self, source: &DeviceId, index: u8) -> Option<MsiMessage> {
        if !self.is_enabled() {
            self.stats.record_passthrough();
            return None;
        }

        let irte = self.entry(source.source_id(), index as usize)?;
        if !irte.is_enabled() {
            self.stats.record_invalid();
            return None;
        }

        self.stats.record_remapped();

        let delivery_mode = match irte.interrupt_type() {
            InterruptType::Fixed => DeliveryMode::Fixed,
            InterruptType::Arbitrated => DeliveryMode::LowestPriority,
            InterruptType::Nmi => DeliveryMode::Nmi,
            InterruptType::ExtInt => DeliveryMode::ExtInt,
            _ => DeliveryMode::Fixed,
        };

        Some(MsiMessage::new(irte.destination(), irte.vector(), delivery_mode))
    }

    /// Get statistics
    pub fn stats(&self) -> &InterruptRemapStats {
        &self.stats
    }
}

/// Posted interrupt descriptor
#[derive(Debug, Clone)]
pub struct PostedInterruptDescriptor {
    /// Posted interrupt requests (256 bits = 4 x u64)
    pir: [u64; 4],
    /// Outstanding notification
    on: bool,
    /// Suppress notification
    sn: bool,
    /// Notification vector
    nv: u8,
    /// Notification destination
    ndst: u32,
}

impl Default for PostedInterruptDescriptor {
    fn default() -> Self {
        Self::new()
    }
}

impl PostedInterruptDescriptor {
    /// Create new descriptor
    pub fn new() -> Self {
        Self {
            pir: [0; 4],
            on: false,
            sn: false,
            nv: 0,
            ndst: 0,
        }
    }

    /// Set notification vector and destination
    pub fn set_notification(&mut self, vector: u8, destination: u32) {
        self.nv = vector;
        self.ndst = destination;
    }

    /// Post interrupt
    pub fn post(&mut self, vector: u8) -> bool {
        let idx = (vector / 64) as usize;
        let bit = vector % 64;

        self.pir[idx] |= 1 << bit;

        if !self.on && !self.sn {
            self.on = true;
            true // Need to send notification
        } else {
            false
        }
    }

    /// Clear posted interrupt
    pub fn clear(&mut self, vector: u8) {
        let idx = (vector / 64) as usize;
        let bit = vector % 64;

        self.pir[idx] &= !(1 << bit);
    }

    /// Check if vector is pending
    pub fn is_pending(&self, vector: u8) -> bool {
        let idx = (vector / 64) as usize;
        let bit = vector % 64;

        (self.pir[idx] & (1 << bit)) != 0
    }

    /// Get highest pending vector
    pub fn highest_pending(&self) -> Option<u8> {
        for i in (0..4).rev() {
            if self.pir[i] != 0 {
                let bit = 63 - self.pir[i].leading_zeros();
                return Some((i * 64 + bit as usize) as u8);
            }
        }
        None
    }

    /// Clear outstanding notification
    pub fn clear_notification(&mut self) {
        self.on = false;
    }

    /// Get notification vector
    pub fn notification_vector(&self) -> u8 {
        self.nv
    }

    /// Get notification destination
    pub fn notification_destination(&self) -> u32 {
        self.ndst
    }

    /// Set suppress notification
    pub fn set_suppress(&mut self, suppress: bool) {
        self.sn = suppress;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intel_irte_empty() {
        let irte = IntelIrte::empty();
        assert!(!irte.is_present());
    }

    #[test]
    fn test_intel_irte_creation() {
        let irte = IntelIrte::new(0x30, 0x01, DeliveryMode::Fixed);
        assert!(irte.is_present());
        assert_eq!(irte.vector(), 0x30);
        assert_eq!(irte.destination(), 0x01);
    }

    #[test]
    fn test_intel_irte_with_source() {
        let irte = IntelIrte::new(0x40, 0x02, DeliveryMode::LowestPriority)
            .with_source(0x1234, SourceValidation::ValidateSourceId);
        assert_eq!(irte.source_id(), 0x1234);
    }

    #[test]
    fn test_intel_irte_modify() {
        let mut irte = IntelIrte::new(0x30, 0x01, DeliveryMode::Fixed);
        irte.set_vector(0x50);
        irte.set_destination(0x05);
        assert_eq!(irte.vector(), 0x50);
        assert_eq!(irte.destination(), 0x05);
    }

    #[test]
    fn test_amd_irte_empty() {
        let irte = AmdIrte::empty();
        assert!(!irte.is_enabled());
    }

    #[test]
    fn test_amd_irte_creation() {
        let irte = AmdIrte::new(0x40, 0x03, InterruptType::Fixed);
        assert!(irte.is_enabled());
        assert_eq!(irte.vector(), 0x40);
        assert_eq!(irte.destination(), 0x03);
    }

    #[test]
    fn test_delivery_mode() {
        assert_eq!(DeliveryMode::from_u8(0), DeliveryMode::Fixed);
        assert_eq!(DeliveryMode::from_u8(1), DeliveryMode::LowestPriority);
        assert_eq!(DeliveryMode::from_u8(4), DeliveryMode::Nmi);
    }

    #[test]
    fn test_msi_message() {
        let msg = MsiMessage::new(0x05, 0x30, DeliveryMode::Fixed);
        assert_eq!(msg.destination(), 0x05);
        assert_eq!(msg.vector(), 0x30);
        assert_eq!(msg.delivery_mode(), DeliveryMode::Fixed);
    }

    #[test]
    fn test_msi_message_modify() {
        let mut msg = MsiMessage::new(0x01, 0x20, DeliveryMode::Fixed);
        msg.set_destination(0x10);
        msg.set_vector(0x50);
        assert_eq!(msg.destination(), 0x10);
        assert_eq!(msg.vector(), 0x50);
    }

    #[test]
    fn test_intel_interrupt_remap_table() {
        let mut table = IntelInterruptRemapTable::new(256);
        let irte = IntelIrte::new(0x30, 0x01, DeliveryMode::Fixed);
        table.set_entry(0, irte);
        table.enable();

        let device = DeviceId::new(0, 1, 0, 0);
        let result = table.remap(&device, 0);
        assert!(result.is_some());

        let msg = result.unwrap();
        assert_eq!(msg.vector(), 0x30);
    }

    #[test]
    fn test_intel_interrupt_remap_disabled() {
        let table = IntelInterruptRemapTable::new(256);
        let device = DeviceId::new(0, 1, 0, 0);
        let result = table.remap(&device, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_amd_interrupt_remap_table() {
        let mut table = AmdInterruptRemapTable::new();
        let device = DeviceId::new(0, 1, 2, 0);

        let irte = AmdIrte::new(0x40, 0x02, InterruptType::Fixed);
        table.set_device_table(device.source_id(), vec![irte]);
        table.enable();

        let result = table.remap(&device, 0);
        assert!(result.is_some());

        let msg = result.unwrap();
        assert_eq!(msg.vector(), 0x40);
    }

    #[test]
    fn test_posted_interrupt_descriptor() {
        let mut pid = PostedInterruptDescriptor::new();
        pid.set_notification(0xF0, 0x01);

        assert!(!pid.is_pending(0x30));

        let notify = pid.post(0x30);
        assert!(notify);
        assert!(pid.is_pending(0x30));

        assert_eq!(pid.highest_pending(), Some(0x30));

        pid.clear(0x30);
        assert!(!pid.is_pending(0x30));
    }

    #[test]
    fn test_posted_interrupt_multiple() {
        let mut pid = PostedInterruptDescriptor::new();

        pid.post(0x20);
        pid.post(0x40);
        pid.post(0x80);

        assert_eq!(pid.highest_pending(), Some(0x80));

        pid.clear(0x80);
        assert_eq!(pid.highest_pending(), Some(0x40));
    }

    #[test]
    fn test_interrupt_remap_stats() {
        let stats = InterruptRemapStats::default();
        stats.record_remapped();
        stats.record_remapped();
        stats.record_blocked();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.remapped, 2);
        assert_eq!(snapshot.blocked, 1);
    }
}
