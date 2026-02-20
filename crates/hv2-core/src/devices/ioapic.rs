//! I/O Advanced Programmable Interrupt Controller (IOAPIC)
//!
//! This module implements the Intel IOAPIC found in modern x86 systems.
//! The IOAPIC provides interrupt routing from I/O devices to local APICs.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                      I/O Devices                          │
//! │  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐       │
//! │  │ IDE  │  │ USB  │  │ NIC  │  │ SATA │  │ etc  │       │
//! │  └──┬───┘  └──┬───┘  └──┬───┘  └──┬───┘  └──┬───┘       │
//! │     │         │         │         │         │            │
//! │     └────┬────┴────┬────┴────┬────┴────┬────┘            │
//! │          │         │         │         │                 │
//! │     ┌────▼─────────▼─────────▼─────────▼────┐            │
//! │     │              IOAPIC                     │            │
//! │     │  ┌─────────────────────────────────┐  │            │
//! │     │  │  24 Redirection Table Entries   │  │            │
//! │     │  │  (RTE 0-23)                     │  │            │
//! │     │  └─────────────────────────────────┘  │            │
//! │     └────────────────┬──────────────────────┘            │
//! │                      │                                    │
//! │          ┌───────────▼───────────┐                       │
//! │          │    System Bus         │                       │
//! │          └───────────┬───────────┘                       │
//! │                      │                                    │
//! │     ┌────────┬───────┴───────┬────────┐                  │
//! │     ▼        ▼               ▼        ▼                  │
//! │  ┌──────┐ ┌──────┐      ┌──────┐ ┌──────┐               │
//! │  │LAPIC │ │LAPIC │ ···· │LAPIC │ │LAPIC │               │
//! │  │ CPU0 │ │ CPU1 │      │ CPUn │ │ CPUm │               │
//! │  └──────┘ └──────┘      └──────┘ └──────┘               │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! # IOAPIC Registers
//!
//! The IOAPIC is accessed via MMIO at its base address (typically 0xFEC00000):
//! - **IOREGSEL** (0x00): I/O Register Select
//! - **IOWIN** (0x10): I/O Window (data)
//!
//! Indirect registers accessed via IOREGSEL/IOWIN:
//! - **IOAPICID** (0x00): IOAPIC ID
//! - **IOAPICVER** (0x01): IOAPIC Version
//! - **IOAPICARB** (0x02): IOAPIC Arbitration ID
//! - **IOREDTBL[0-23]** (0x10-0x3F): Redirection Table (64-bit each)

use crate::{Error, Result};
use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::RwLock;
#[cfg(test)]
use std::sync::Arc;

/// IOAPIC base address (standard location)
pub const IOAPIC_BASE: u64 = 0xFEC0_0000;

/// IOAPIC MMIO region size
pub const IOAPIC_SIZE: u64 = 0x20;

/// Number of IOAPIC redirection entries (IRQ pins)
pub const IOAPIC_NUM_PINS: usize = 24;

/// IOAPIC register offsets (MMIO)
pub mod regs {
    /// I/O Register Select (write register index here)
    pub const IOREGSEL: u64 = 0x00;
    /// I/O Window (read/write data here)
    pub const IOWIN: u64 = 0x10;
}

/// IOAPIC indirect register indices
pub mod indirect {
    /// IOAPIC ID register
    pub const IOAPICID: u32 = 0x00;
    /// IOAPIC Version register
    pub const IOAPICVER: u32 = 0x01;
    /// IOAPIC Arbitration ID register
    pub const IOAPICARB: u32 = 0x02;
    /// First redirection table entry (low 32 bits)
    pub const IOREDTBL_BASE: u32 = 0x10;
}

/// Redirection Table Entry (RTE) bit fields
pub mod rte {
    /// Interrupt vector (bits 0-7)
    pub const VECTOR_MASK: u64 = 0xFF;
    pub const VECTOR_SHIFT: u64 = 0;

    /// Delivery mode (bits 8-10)
    pub const DELMODE_MASK: u64 = 0x700;
    pub const DELMODE_SHIFT: u64 = 8;

    /// Destination mode (bit 11): 0=physical, 1=logical
    pub const DESTMODE: u64 = 1 << 11;

    /// Delivery status (bit 12): 0=idle, 1=send pending (read-only)
    pub const DELIVS: u64 = 1 << 12;

    /// Interrupt polarity (bit 13): 0=active high, 1=active low
    pub const INTPOL: u64 = 1 << 13;

    /// Remote IRR (bit 14): For level-triggered, 1=LAPIC accepted (read-only)
    pub const REMOTE_IRR: u64 = 1 << 14;

    /// Trigger mode (bit 15): 0=edge, 1=level
    pub const TRIGGER: u64 = 1 << 15;

    /// Interrupt mask (bit 16): 0=enabled, 1=masked
    pub const INTMASK: u64 = 1 << 16;

    /// Destination field (bits 56-63 for physical mode)
    pub const DEST_MASK: u64 = 0xFF00_0000_0000_0000;
    pub const DEST_SHIFT: u64 = 56;
}

/// Delivery modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryMode {
    /// Fixed: deliver to all processors in destination
    Fixed = 0,
    /// Lowest Priority: deliver to lowest priority processor
    LowestPriority = 1,
    /// SMI: System Management Interrupt
    Smi = 2,
    /// Reserved
    Reserved3 = 3,
    /// NMI: Non-Maskable Interrupt
    Nmi = 4,
    /// INIT: Assert INIT signal
    Init = 5,
    /// Reserved
    Reserved6 = 6,
    /// ExtINT: External interrupt (8259 compatible)
    ExtInt = 7,
}

impl DeliveryMode {
    /// Create from bits
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x7 {
            0 => Self::Fixed,
            1 => Self::LowestPriority,
            2 => Self::Smi,
            3 => Self::Reserved3,
            4 => Self::Nmi,
            5 => Self::Init,
            6 => Self::Reserved6,
            7 => Self::ExtInt,
            _ => unreachable!("3-bit mask value exceeded 0..=7"),
        }
    }
}

/// Destination mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationMode {
    /// Physical: destination is APIC ID
    Physical,
    /// Logical: destination is logical APIC ID (cluster/flat)
    Logical,
}

/// Trigger mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    /// Edge-triggered
    Edge,
    /// Level-triggered
    Level,
}

/// Polarity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    /// Active high
    ActiveHigh,
    /// Active low
    ActiveLow,
}

/// Parsed redirection table entry
#[derive(Debug, Clone, Copy)]
pub struct RedirectionEntry {
    /// Raw 64-bit value
    pub raw: u64,
}

impl RedirectionEntry {
    /// Create a new entry (default: masked)
    pub const fn new() -> Self {
        Self {
            raw: rte::INTMASK, // Masked by default
        }
    }

    /// Create from raw value
    pub const fn from_raw(raw: u64) -> Self {
        Self { raw }
    }

    /// Get interrupt vector
    pub fn vector(&self) -> u8 {
        (self.raw & rte::VECTOR_MASK) as u8
    }

    /// Set interrupt vector
    pub fn set_vector(&mut self, vector: u8) {
        self.raw = (self.raw & !rte::VECTOR_MASK) | (vector as u64);
    }

    /// Get delivery mode
    pub fn delivery_mode(&self) -> DeliveryMode {
        DeliveryMode::from_bits(((self.raw & rte::DELMODE_MASK) >> rte::DELMODE_SHIFT) as u8)
    }

    /// Set delivery mode
    pub fn set_delivery_mode(&mut self, mode: DeliveryMode) {
        self.raw = (self.raw & !rte::DELMODE_MASK) | ((mode as u64) << rte::DELMODE_SHIFT);
    }

    /// Get destination mode
    pub fn destination_mode(&self) -> DestinationMode {
        if self.raw & rte::DESTMODE != 0 {
            DestinationMode::Logical
        } else {
            DestinationMode::Physical
        }
    }

    /// Set destination mode
    pub fn set_destination_mode(&mut self, mode: DestinationMode) {
        match mode {
            DestinationMode::Physical => self.raw &= !rte::DESTMODE,
            DestinationMode::Logical => self.raw |= rte::DESTMODE,
        }
    }

    /// Check if delivery is pending
    pub fn is_delivery_pending(&self) -> bool {
        self.raw & rte::DELIVS != 0
    }

    /// Get polarity
    pub fn polarity(&self) -> Polarity {
        if self.raw & rte::INTPOL != 0 {
            Polarity::ActiveLow
        } else {
            Polarity::ActiveHigh
        }
    }

    /// Set polarity
    pub fn set_polarity(&mut self, polarity: Polarity) {
        match polarity {
            Polarity::ActiveHigh => self.raw &= !rte::INTPOL,
            Polarity::ActiveLow => self.raw |= rte::INTPOL,
        }
    }

    /// Check if remote IRR is set (level-triggered only)
    pub fn remote_irr(&self) -> bool {
        self.raw & rte::REMOTE_IRR != 0
    }

    /// Clear remote IRR (called on EOI)
    pub fn clear_remote_irr(&mut self) {
        self.raw &= !rte::REMOTE_IRR;
    }

    /// Set remote IRR
    pub fn set_remote_irr(&mut self) {
        self.raw |= rte::REMOTE_IRR;
    }

    /// Get trigger mode
    pub fn trigger_mode(&self) -> TriggerMode {
        if self.raw & rte::TRIGGER != 0 {
            TriggerMode::Level
        } else {
            TriggerMode::Edge
        }
    }

    /// Set trigger mode
    pub fn set_trigger_mode(&mut self, mode: TriggerMode) {
        match mode {
            TriggerMode::Edge => self.raw &= !rte::TRIGGER,
            TriggerMode::Level => self.raw |= rte::TRIGGER,
        }
    }

    /// Check if masked
    pub fn is_masked(&self) -> bool {
        self.raw & rte::INTMASK != 0
    }

    /// Set mask
    pub fn set_masked(&mut self, masked: bool) {
        if masked {
            self.raw |= rte::INTMASK;
        } else {
            self.raw &= !rte::INTMASK;
        }
    }

    /// Get destination APIC ID
    pub fn destination(&self) -> u8 {
        ((self.raw & rte::DEST_MASK) >> rte::DEST_SHIFT) as u8
    }

    /// Set destination APIC ID
    pub fn set_destination(&mut self, dest: u8) {
        self.raw = (self.raw & !rte::DEST_MASK) | ((dest as u64) << rte::DEST_SHIFT);
    }

    /// Get low 32 bits
    pub fn low(&self) -> u32 {
        self.raw as u32
    }

    /// Get high 32 bits
    pub fn high(&self) -> u32 {
        (self.raw >> 32) as u32
    }

    /// Set low 32 bits
    pub fn set_low(&mut self, value: u32) {
        self.raw = (self.raw & 0xFFFF_FFFF_0000_0000) | (value as u64);
    }

    /// Set high 32 bits
    pub fn set_high(&mut self, value: u32) {
        self.raw = (self.raw & 0x0000_0000_FFFF_FFFF) | ((value as u64) << 32);
    }
}

impl Default for RedirectionEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// IOAPIC interrupt callback
pub type IoApicCallback = Box<dyn Fn(u8, u8) + Send + Sync>;

/// IOAPIC state
pub struct IoApic {
    /// IOAPIC ID (bits 24-27 of IOAPICID register)
    id: u8,
    /// Currently selected register (IOREGSEL)
    regsel: AtomicU32,
    /// Redirection table entries
    redirection_table: RwLock<[RedirectionEntry; IOAPIC_NUM_PINS]>,
    /// IRQ input states (for level-triggered)
    irq_state: [AtomicU32; 1], // Bitmap for 24 IRQs
    /// Callback for interrupt delivery
    interrupt_callback: RwLock<Option<IoApicCallback>>,
}

impl IoApic {
    /// Create a new IOAPIC
    pub fn new(id: u8) -> Self {
        Self {
            id,
            regsel: AtomicU32::new(0),
            redirection_table: RwLock::new([RedirectionEntry::new(); IOAPIC_NUM_PINS]),
            irq_state: [AtomicU32::new(0)],
            interrupt_callback: RwLock::new(None),
        }
    }

    /// Set the interrupt delivery callback
    pub fn set_interrupt_callback<F>(&self, callback: F)
    where
        F: Fn(u8, u8) + Send + Sync + 'static,
    {
        *self.interrupt_callback.write() = Some(Box::new(callback));
    }

    /// Get the IOAPIC ID
    pub fn id(&self) -> u8 {
        self.id
    }

    /// Read from MMIO address
    pub fn read(&self, offset: u64, size: u8) -> Result<u32> {
        match offset {
            regs::IOREGSEL => Ok(self.regsel.load(Ordering::Relaxed)),
            regs::IOWIN => self.read_indirect(),
            _ => {
                // Unaligned or invalid read
                Ok(0)
            }
        }
    }

    /// Write to MMIO address
    pub fn write(&self, offset: u64, value: u32, size: u8) -> Result<()> {
        match offset {
            regs::IOREGSEL => {
                self.regsel.store(value, Ordering::Relaxed);
            }
            regs::IOWIN => {
                self.write_indirect(value)?;
            }
            _ => {
                // Ignore invalid writes
            }
        }
        Ok(())
    }

    /// Read from indirect register
    fn read_indirect(&self) -> Result<u32> {
        let regsel = self.regsel.load(Ordering::Relaxed);

        match regsel {
            indirect::IOAPICID => {
                // IOAPIC ID in bits 24-27
                Ok((self.id as u32) << 24)
            }
            indirect::IOAPICVER => {
                // Version: 0x20 (IOAPIC version 2.0)
                // Max redirection entry in bits 16-23
                Ok(0x20 | (((IOAPIC_NUM_PINS - 1) as u32) << 16))
            }
            indirect::IOAPICARB => {
                // Arbitration ID (same as IOAPIC ID for single IOAPIC)
                Ok((self.id as u32) << 24)
            }
            _ => {
                // Redirection table entries
                if regsel >= indirect::IOREDTBL_BASE && regsel < indirect::IOREDTBL_BASE + 48 {
                    let entry_idx = ((regsel - indirect::IOREDTBL_BASE) / 2) as usize;
                    let is_high = (regsel - indirect::IOREDTBL_BASE) % 2 == 1;

                    if entry_idx < IOAPIC_NUM_PINS {
                        let table = self.redirection_table.read();
                        let entry = &table[entry_idx];

                        if is_high {
                            Ok(entry.high())
                        } else {
                            Ok(entry.low())
                        }
                    } else {
                        Ok(0)
                    }
                } else {
                    Ok(0)
                }
            }
        }
    }

    /// Write to indirect register
    fn write_indirect(&self, value: u32) -> Result<()> {
        let regsel = self.regsel.load(Ordering::Relaxed);

        match regsel {
            indirect::IOAPICID => {
                // IOAPIC ID is typically read-only in emulation
                // Some implementations allow writing bits 24-27
            }
            indirect::IOAPICVER | indirect::IOAPICARB => {
                // Read-only registers
            }
            _ => {
                // Redirection table entries
                if regsel >= indirect::IOREDTBL_BASE && regsel < indirect::IOREDTBL_BASE + 48 {
                    let entry_idx = ((regsel - indirect::IOREDTBL_BASE) / 2) as usize;
                    let is_high = (regsel - indirect::IOREDTBL_BASE) % 2 == 1;

                    if entry_idx < IOAPIC_NUM_PINS {
                        let mut table = self.redirection_table.write();
                        let entry = &mut table[entry_idx];

                        if is_high {
                            entry.set_high(value);
                        } else {
                            // Clear read-only bits (DELIVS, REMOTE_IRR)
                            let clean_value =
                                value & !(rte::DELIVS as u32 | rte::REMOTE_IRR as u32);
                            let old_remote_irr = entry.raw & rte::REMOTE_IRR;
                            entry.set_low(clean_value);
                            // Preserve remote IRR
                            entry.raw |= old_remote_irr;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Raise an IRQ
    pub fn raise_irq(&self, irq: u8) {
        if irq as usize >= IOAPIC_NUM_PINS {
            return;
        }

        let table = self.redirection_table.read();
        let entry = table[irq as usize];

        // Check if masked
        if entry.is_masked() {
            return;
        }

        // For level-triggered, check remote IRR
        if entry.trigger_mode() == TriggerMode::Level {
            if entry.remote_irr() {
                // Already pending, don't re-deliver
                return;
            }

            // Set remote IRR
            drop(table);
            let mut table = self.redirection_table.write();
            table[irq as usize].set_remote_irr();
        }

        // Deliver interrupt
        self.deliver_interrupt(&entry);
    }

    /// Lower an IRQ (for level-triggered)
    pub fn lower_irq(&self, irq: u8) {
        if irq as usize >= IOAPIC_NUM_PINS {
            return;
        }

        // Update IRQ state bitmap
        self.irq_state[0].fetch_and(!(1 << irq), Ordering::Relaxed);
    }

    /// Handle EOI from LAPIC
    pub fn eoi(&self, vector: u8) {
        // Find entry with matching vector and clear remote IRR
        let mut table = self.redirection_table.write();

        for entry in table.iter_mut() {
            if entry.vector() == vector && entry.trigger_mode() == TriggerMode::Level {
                entry.clear_remote_irr();

                // If IRQ is still asserted, re-raise
                // (would need to track IRQ line state)
                break;
            }
        }
    }

    /// Deliver interrupt to LAPIC
    fn deliver_interrupt(&self, entry: &RedirectionEntry) {
        let callback = self.interrupt_callback.read();
        if let Some(ref cb) = *callback {
            cb(entry.destination(), entry.vector());
        }
    }

    /// Get a redirection entry
    pub fn get_entry(&self, irq: u8) -> Option<RedirectionEntry> {
        if irq as usize >= IOAPIC_NUM_PINS {
            return None;
        }

        let table = self.redirection_table.read();
        Some(table[irq as usize])
    }

    /// Set a redirection entry
    pub fn set_entry(&self, irq: u8, entry: RedirectionEntry) -> Result<()> {
        if irq as usize >= IOAPIC_NUM_PINS {
            return Err(Error::VM(format!("Invalid IOAPIC IRQ: {}", irq)));
        }

        let mut table = self.redirection_table.write();
        table[irq as usize] = entry;
        Ok(())
    }

    /// Configure an IRQ
    pub fn configure_irq(
        &self,
        irq: u8,
        vector: u8,
        dest: u8,
        trigger: TriggerMode,
        polarity: Polarity,
        masked: bool,
    ) -> Result<()> {
        if irq as usize >= IOAPIC_NUM_PINS {
            return Err(Error::VM(format!("Invalid IOAPIC IRQ: {}", irq)));
        }

        let mut entry = RedirectionEntry::new();
        entry.set_vector(vector);
        entry.set_destination(dest);
        entry.set_delivery_mode(DeliveryMode::Fixed);
        entry.set_destination_mode(DestinationMode::Physical);
        entry.set_trigger_mode(trigger);
        entry.set_polarity(polarity);
        entry.set_masked(masked);

        self.set_entry(irq, entry)
    }
}

impl std::fmt::Debug for IoApic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IoApic")
            .field("id", &self.id)
            .field("regsel", &self.regsel.load(Ordering::Relaxed))
            .finish()
    }
}

/// Standard IRQ to IOAPIC pin mapping
pub mod irq_mapping {
    /// Timer (PIT or HPET)
    pub const TIMER: u8 = 0;
    /// Keyboard
    pub const KEYBOARD: u8 = 1;
    /// Cascade (not used with IOAPIC)
    pub const CASCADE: u8 = 2;
    /// COM2/COM4
    pub const COM2: u8 = 3;
    /// COM1/COM3
    pub const COM1: u8 = 4;
    /// LPT2 or sound card
    pub const LPT2: u8 = 5;
    /// Floppy
    pub const FLOPPY: u8 = 6;
    /// LPT1
    pub const LPT1: u8 = 7;
    /// RTC
    pub const RTC: u8 = 8;
    /// ACPI / available
    pub const ACPI: u8 = 9;
    /// Available
    pub const AVAIL1: u8 = 10;
    /// Available
    pub const AVAIL2: u8 = 11;
    /// PS/2 Mouse
    pub const MOUSE: u8 = 12;
    /// FPU / coprocessor
    pub const FPU: u8 = 13;
    /// Primary IDE
    pub const IDE_PRIMARY: u8 = 14;
    /// Secondary IDE
    pub const IDE_SECONDARY: u8 = 15;
    /// PCI slot A
    pub const PCI_A: u8 = 16;
    /// PCI slot B
    pub const PCI_B: u8 = 17;
    /// PCI slot C
    pub const PCI_C: u8 = 18;
    /// PCI slot D
    pub const PCI_D: u8 = 19;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioapic_creation() {
        let ioapic = IoApic::new(0);
        assert_eq!(ioapic.id(), 0);
    }

    #[test]
    fn test_ioapic_id_register() {
        let ioapic = IoApic::new(2);

        // Select IOAPICID register
        ioapic.write(regs::IOREGSEL, indirect::IOAPICID, 4).unwrap();
        let id = ioapic.read(regs::IOWIN, 4).unwrap();

        // ID should be in bits 24-27
        assert_eq!((id >> 24) & 0xF, 2);
    }

    #[test]
    fn test_ioapic_version_register() {
        let ioapic = IoApic::new(0);

        // Select IOAPICVER register
        ioapic
            .write(regs::IOREGSEL, indirect::IOAPICVER, 4)
            .unwrap();
        let ver = ioapic.read(regs::IOWIN, 4).unwrap();

        // Version should be 0x20
        assert_eq!(ver & 0xFF, 0x20);

        // Max redirection entry should be 23
        assert_eq!((ver >> 16) & 0xFF, 23);
    }

    #[test]
    fn test_redirection_entry_default() {
        let entry = RedirectionEntry::new();

        // Should be masked by default
        assert!(entry.is_masked());
        assert_eq!(entry.vector(), 0);
        assert_eq!(entry.destination(), 0);
    }

    #[test]
    fn test_redirection_entry_fields() {
        let mut entry = RedirectionEntry::new();

        entry.set_vector(0x42);
        assert_eq!(entry.vector(), 0x42);

        entry.set_destination(7);
        assert_eq!(entry.destination(), 7);

        entry.set_delivery_mode(DeliveryMode::LowestPriority);
        assert_eq!(entry.delivery_mode(), DeliveryMode::LowestPriority);

        entry.set_destination_mode(DestinationMode::Logical);
        assert_eq!(entry.destination_mode(), DestinationMode::Logical);

        entry.set_trigger_mode(TriggerMode::Level);
        assert_eq!(entry.trigger_mode(), TriggerMode::Level);

        entry.set_polarity(Polarity::ActiveLow);
        assert_eq!(entry.polarity(), Polarity::ActiveLow);

        entry.set_masked(false);
        assert!(!entry.is_masked());
    }

    #[test]
    fn test_redirection_table_read_write() {
        let ioapic = IoApic::new(0);

        // Write to first redirection entry (low)
        ioapic
            .write(regs::IOREGSEL, indirect::IOREDTBL_BASE, 4)
            .unwrap();
        ioapic.write(regs::IOWIN, 0x0000_0042, 4).unwrap(); // Vector 0x42

        // Write to first redirection entry (high)
        ioapic
            .write(regs::IOREGSEL, indirect::IOREDTBL_BASE + 1, 4)
            .unwrap();
        ioapic.write(regs::IOWIN, 0x0300_0000, 4).unwrap(); // Destination 3

        // Read back
        ioapic
            .write(regs::IOREGSEL, indirect::IOREDTBL_BASE, 4)
            .unwrap();
        let low = ioapic.read(regs::IOWIN, 4).unwrap();

        ioapic
            .write(regs::IOREGSEL, indirect::IOREDTBL_BASE + 1, 4)
            .unwrap();
        let high = ioapic.read(regs::IOWIN, 4).unwrap();

        assert_eq!(low & 0xFF, 0x42);
        assert_eq!((high >> 24) & 0xFF, 3);
    }

    #[test]
    fn test_configure_irq() {
        let ioapic = IoApic::new(0);

        ioapic
            .configure_irq(
                irq_mapping::KEYBOARD,
                0x21,
                0,
                TriggerMode::Edge,
                Polarity::ActiveHigh,
                false,
            )
            .unwrap();

        let entry = ioapic.get_entry(irq_mapping::KEYBOARD).unwrap();
        assert_eq!(entry.vector(), 0x21);
        assert_eq!(entry.destination(), 0);
        assert_eq!(entry.trigger_mode(), TriggerMode::Edge);
        assert!(!entry.is_masked());
    }

    #[test]
    fn test_raise_irq_masked() {
        let ioapic = IoApic::new(0);
        use std::sync::atomic::AtomicBool;

        let delivered = Arc::new(AtomicBool::new(false));
        let delivered_clone = delivered.clone();

        ioapic.set_interrupt_callback(move |_dest, _vec| {
            delivered_clone.store(true, Ordering::Relaxed);
        });

        // IRQ is masked by default, shouldn't deliver
        ioapic.raise_irq(0);
        assert!(!delivered.load(Ordering::Relaxed));
    }

    #[test]
    fn test_raise_irq_unmasked() {
        let ioapic = IoApic::new(0);
        use std::sync::atomic::AtomicBool;

        let delivered = Arc::new(AtomicBool::new(false));
        let vector_received = Arc::new(AtomicU32::new(0));

        let delivered_clone = delivered.clone();
        let vector_clone = vector_received.clone();

        ioapic.set_interrupt_callback(move |_dest, vec| {
            delivered_clone.store(true, Ordering::Relaxed);
            vector_clone.store(vec as u32, Ordering::Relaxed);
        });

        // Configure and unmask IRQ
        ioapic
            .configure_irq(0, 0x30, 0, TriggerMode::Edge, Polarity::ActiveHigh, false)
            .unwrap();

        // Raise IRQ
        ioapic.raise_irq(0);

        assert!(delivered.load(Ordering::Relaxed));
        assert_eq!(vector_received.load(Ordering::Relaxed), 0x30);
    }

    #[test]
    fn test_level_triggered_remote_irr() {
        let ioapic = IoApic::new(0);

        // Configure level-triggered IRQ
        ioapic
            .configure_irq(0, 0x30, 0, TriggerMode::Level, Polarity::ActiveHigh, false)
            .unwrap();

        let delivered = Arc::new(AtomicU32::new(0));
        let delivered_clone = delivered.clone();

        ioapic.set_interrupt_callback(move |_dest, _vec| {
            delivered_clone.fetch_add(1, Ordering::Relaxed);
        });

        // First raise should deliver
        ioapic.raise_irq(0);
        assert_eq!(delivered.load(Ordering::Relaxed), 1);

        // Second raise should NOT deliver (remote IRR set)
        ioapic.raise_irq(0);
        assert_eq!(delivered.load(Ordering::Relaxed), 1);

        // EOI should clear remote IRR
        ioapic.eoi(0x30);

        // Now it should deliver again
        ioapic.raise_irq(0);
        assert_eq!(delivered.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_delivery_mode() {
        assert_eq!(DeliveryMode::from_bits(0), DeliveryMode::Fixed);
        assert_eq!(DeliveryMode::from_bits(1), DeliveryMode::LowestPriority);
        assert_eq!(DeliveryMode::from_bits(4), DeliveryMode::Nmi);
        assert_eq!(DeliveryMode::from_bits(7), DeliveryMode::ExtInt);
    }

    #[test]
    fn test_invalid_irq() {
        let ioapic = IoApic::new(0);

        // IRQ 24+ should be invalid
        assert!(ioapic
            .configure_irq(24, 0x30, 0, TriggerMode::Edge, Polarity::ActiveHigh, false)
            .is_err());
        assert!(ioapic.get_entry(24).is_none());
    }

    #[test]
    fn test_entry_low_high() {
        let mut entry = RedirectionEntry::new();
        entry.set_vector(0x42);
        entry.set_destination(5);

        let low = entry.low();
        let high = entry.high();

        // Reconstruct
        let mut entry2 = RedirectionEntry::new();
        entry2.set_low(low);
        entry2.set_high(high);

        assert_eq!(entry2.vector(), 0x42);
        assert_eq!(entry2.destination(), 5);
    }
}
