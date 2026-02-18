//! Message Signaled Interrupts (MSI/MSI-X)
//!
//! This module implements MSI and MSI-X interrupt support for PCI devices.
//! MSI/MSI-X allows devices to signal interrupts by writing to special memory addresses
//! rather than using dedicated interrupt lines.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                        PCI Device                                 │
//! │  ┌────────────────────────────────────────────────────────────┐  │
//! │  │                   MSI Capability                            │  │
//! │  │  ┌──────────────────────────────────────────────────────┐  │  │
//! │  │  │ Message Address: 0xFEE00000 | (APIC_ID << 12)        │  │  │
//! │  │  │ Message Data: vector | (delivery_mode << 8)          │  │  │
//! │  │  └──────────────────────────────────────────────────────┘  │  │
//! │  └────────────────────────────────────────────────────────────┘  │
//! │  ┌────────────────────────────────────────────────────────────┐  │
//! │  │                  MSI-X Capability                           │  │
//! │  │  ┌────────────────────────────────────────────────────────┐│  │
//! │  │  │ Table (BAR-relative):                                  ││  │
//! │  │  │   Entry 0: addr_lo, addr_hi, data, vector_ctrl        ││  │
//! │  │  │   Entry 1: addr_lo, addr_hi, data, vector_ctrl        ││  │
//! │  │  │   ...                                                  ││  │
//! │  │  └────────────────────────────────────────────────────────┘│  │
//! │  │  ┌────────────────────────────────────────────────────────┐│  │
//! │  │  │ PBA (Pending Bit Array): pending[0..n]                 ││  │
//! │  │  └────────────────────────────────────────────────────────┘│  │
//! │  └────────────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼  Memory Write
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                      LAPIC (0xFEE00000)                           │
//! │                  Receives interrupt message                       │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # MSI Address Format
//!
//! | Bits    | Field              | Description                        |
//! |---------|--------------------|------------------------------------|
//! | 31:20   | 0xFEE              | Fixed prefix (LAPIC address range) |
//! | 19:12   | Destination ID     | Target APIC ID                     |
//! | 11:4    | Reserved           | Must be 0                          |
//! | 3       | RH                 | Redirection Hint                   |
//! | 2       | DM                 | Destination Mode                   |
//! | 1:0     | Reserved           | Must be 0                          |
//!
//! # MSI Data Format
//!
//! | Bits    | Field              | Description                        |
//! |---------|--------------------|------------------------------------|
//! | 15      | Trigger Mode       | 0=Edge, 1=Level                    |
//! | 14      | Level              | For level: 0=Deassert, 1=Assert    |
//! | 13:11   | Reserved           | Must be 0                          |
//! | 10:8    | Delivery Mode      | 0=Fixed, 1=LowPri, etc.            |
//! | 7:0     | Vector             | Interrupt vector                   |

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::RwLock;

/// MSI address base (LAPIC range)
pub const MSI_ADDR_BASE: u64 = 0xFEE0_0000;

/// MSI address mask (bits 31:20)
pub const MSI_ADDR_MASK: u64 = 0xFFF0_0000;

/// Maximum MSI vectors per device
pub const MSI_MAX_VECTORS: usize = 32;

/// Maximum MSI-X vectors per device
pub const MSIX_MAX_VECTORS: usize = 2048;

/// MSI address field masks and shifts
pub mod msi_addr {
    /// Destination ID (bits 19:12)
    pub const DEST_ID_MASK: u64 = 0x000F_F000;
    pub const DEST_ID_SHIFT: u64 = 12;

    /// Redirection Hint (bit 3)
    pub const REDIRECTION_HINT: u64 = 1 << 3;

    /// Destination Mode (bit 2): 0=Physical, 1=Logical
    pub const DEST_MODE: u64 = 1 << 2;
}

/// MSI data field masks and shifts
pub mod msi_data {
    /// Vector (bits 7:0)
    pub const VECTOR_MASK: u32 = 0xFF;

    /// Delivery Mode (bits 10:8)
    pub const DELIVERY_MODE_MASK: u32 = 0x700;
    pub const DELIVERY_MODE_SHIFT: u32 = 8;

    /// Level (bit 14)
    pub const LEVEL: u32 = 1 << 14;

    /// Trigger Mode (bit 15): 0=Edge, 1=Level
    pub const TRIGGER_MODE: u32 = 1 << 15;
}

/// MSI delivery modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsiDeliveryMode {
    /// Fixed delivery to destination
    Fixed = 0,
    /// Lowest priority processor
    LowestPriority = 1,
    /// System Management Interrupt
    Smi = 2,
    /// Reserved
    Reserved = 3,
    /// Non-Maskable Interrupt
    Nmi = 4,
    /// INIT signal
    Init = 5,
    /// Reserved
    Reserved2 = 6,
    /// External interrupt
    ExtInt = 7,
}

impl MsiDeliveryMode {
    /// Create from bits
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x7 {
            0 => Self::Fixed,
            1 => Self::LowestPriority,
            2 => Self::Smi,
            3 => Self::Reserved,
            4 => Self::Nmi,
            5 => Self::Init,
            6 => Self::Reserved2,
            _ => Self::ExtInt,
        }
    }
}

/// MSI destination mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsiDestMode {
    /// Physical APIC ID
    Physical,
    /// Logical APIC ID
    Logical,
}

/// Parsed MSI message
#[derive(Debug, Clone, Copy)]
pub struct MsiMessage {
    /// Target address (includes destination)
    pub address: u64,
    /// Message data (includes vector)
    pub data: u32,
}

impl MsiMessage {
    /// Create a new MSI message
    pub const fn new(address: u64, data: u32) -> Self {
        Self { address, data }
    }

    /// Create MSI message for a specific destination and vector
    pub fn create(
        dest_id: u8,
        vector: u8,
        delivery_mode: MsiDeliveryMode,
        dest_mode: MsiDestMode,
    ) -> Self {
        let mut address = MSI_ADDR_BASE;
        address |= (dest_id as u64) << msi_addr::DEST_ID_SHIFT;
        if dest_mode == MsiDestMode::Logical {
            address |= msi_addr::DEST_MODE;
        }

        let mut data = vector as u32;
        data |= (delivery_mode as u32) << msi_data::DELIVERY_MODE_SHIFT;

        Self { address, data }
    }

    /// Get destination APIC ID
    pub fn dest_id(&self) -> u8 {
        ((self.address & msi_addr::DEST_ID_MASK) >> msi_addr::DEST_ID_SHIFT) as u8
    }

    /// Get destination mode
    pub fn dest_mode(&self) -> MsiDestMode {
        if self.address & msi_addr::DEST_MODE != 0 {
            MsiDestMode::Logical
        } else {
            MsiDestMode::Physical
        }
    }

    /// Get redirection hint
    pub fn redirection_hint(&self) -> bool {
        self.address & msi_addr::REDIRECTION_HINT != 0
    }

    /// Get interrupt vector
    pub fn vector(&self) -> u8 {
        (self.data & msi_data::VECTOR_MASK) as u8
    }

    /// Get delivery mode
    pub fn delivery_mode(&self) -> MsiDeliveryMode {
        MsiDeliveryMode::from_bits(
            ((self.data & msi_data::DELIVERY_MODE_MASK) >> msi_data::DELIVERY_MODE_SHIFT) as u8,
        )
    }

    /// Get trigger mode (edge=false, level=true)
    pub fn is_level_triggered(&self) -> bool {
        self.data & msi_data::TRIGGER_MODE != 0
    }

    /// Check if this is a valid MSI address
    pub fn is_valid_address(&self) -> bool {
        (self.address & MSI_ADDR_MASK) == MSI_ADDR_BASE
    }
}

impl Default for MsiMessage {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// MSI capability for PCI devices
pub struct MsiCapability {
    /// MSI enabled
    enabled: AtomicBool,
    /// 64-bit address capable
    is_64bit: bool,
    /// Per-vector masking capable
    can_mask: bool,
    /// Number of vectors requested (log2)
    multiple_message_capable: u8,
    /// Number of vectors enabled (log2)
    multiple_message_enable: AtomicU32,
    /// Message address (low 32 bits)
    address_lo: AtomicU32,
    /// Message address (high 32 bits, if 64-bit capable)
    address_hi: AtomicU32,
    /// Message data
    data: AtomicU32,
    /// Vector mask (if masking capable)
    mask: AtomicU32,
    /// Pending bits (if masking capable)
    pending: AtomicU32,
}

impl MsiCapability {
    /// Create a new MSI capability
    pub fn new(num_vectors: usize, is_64bit: bool, can_mask: bool) -> Self {
        let mmc = match num_vectors {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            32 => 5,
            _ => 0,
        };

        Self {
            enabled: AtomicBool::new(false),
            is_64bit,
            can_mask,
            multiple_message_capable: mmc,
            multiple_message_enable: AtomicU32::new(0),
            address_lo: AtomicU32::new(0),
            address_hi: AtomicU32::new(0),
            data: AtomicU32::new(0),
            mask: AtomicU32::new(0),
            pending: AtomicU32::new(0),
        }
    }

    /// Check if MSI is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Enable/disable MSI
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Get number of enabled vectors
    pub fn num_vectors(&self) -> usize {
        1 << self.multiple_message_enable.load(Ordering::Relaxed)
    }

    /// Set number of enabled vectors (log2)
    pub fn set_multiple_message_enable(&self, mme: u8) {
        let mme = std::cmp::min(mme, self.multiple_message_capable);
        self.multiple_message_enable
            .store(mme as u32, Ordering::Relaxed);
    }

    /// Get the message for a specific vector
    pub fn get_message(&self, vector_offset: u8) -> MsiMessage {
        let address = if self.is_64bit {
            ((self.address_hi.load(Ordering::Relaxed) as u64) << 32)
                | (self.address_lo.load(Ordering::Relaxed) as u64)
        } else {
            self.address_lo.load(Ordering::Relaxed) as u64
        };

        let data = self.data.load(Ordering::Relaxed);
        // Vector offset is added to base data
        let data = (data & !msi_data::VECTOR_MASK as u32)
            | ((data & msi_data::VECTOR_MASK as u32) + vector_offset as u32);

        MsiMessage::new(address, data)
    }

    /// Set message address
    pub fn set_address(&self, address: u64) {
        self.address_lo.store(address as u32, Ordering::Relaxed);
        if self.is_64bit {
            self.address_hi
                .store((address >> 32) as u32, Ordering::Relaxed);
        }
    }

    /// Set message data
    pub fn set_data(&self, data: u32) {
        self.data.store(data, Ordering::Relaxed);
    }

    /// Check if a vector is masked
    pub fn is_masked(&self, vector: u8) -> bool {
        if !self.can_mask {
            return false;
        }
        self.mask.load(Ordering::Relaxed) & (1 << vector) != 0
    }

    /// Set vector mask
    pub fn set_mask(&self, mask: u32) {
        self.mask.store(mask, Ordering::Relaxed);
    }

    /// Get pending bits
    pub fn get_pending(&self) -> u32 {
        self.pending.load(Ordering::Relaxed)
    }

    /// Set pending bit
    pub fn set_pending(&self, vector: u8) {
        self.pending.fetch_or(1 << vector, Ordering::Relaxed);
    }

    /// Clear pending bit
    pub fn clear_pending(&self, vector: u8) {
        self.pending.fetch_and(!(1 << vector), Ordering::Relaxed);
    }

    /// Read PCI config space
    pub fn read_config(&self, offset: u8) -> u32 {
        match offset {
            0x00 => {
                // Message Control
                let mut ctrl = 0u32;
                if self.enabled.load(Ordering::Relaxed) {
                    ctrl |= 1 << 0; // MSI Enable
                }
                ctrl |= (self.multiple_message_capable as u32) << 1;
                ctrl |= (self.multiple_message_enable.load(Ordering::Relaxed)) << 4;
                if self.is_64bit {
                    ctrl |= 1 << 7;
                }
                if self.can_mask {
                    ctrl |= 1 << 8;
                }
                ctrl
            }
            0x04 => self.address_lo.load(Ordering::Relaxed),
            0x08 if self.is_64bit => self.address_hi.load(Ordering::Relaxed),
            0x08 if !self.is_64bit => self.data.load(Ordering::Relaxed),
            0x0C if self.is_64bit => self.data.load(Ordering::Relaxed),
            0x10 if self.can_mask => self.mask.load(Ordering::Relaxed),
            0x14 if self.can_mask => self.pending.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Write PCI config space
    pub fn write_config(&self, offset: u8, value: u32) {
        match offset {
            0x00 => {
                // Message Control
                self.enabled.store(value & 1 != 0, Ordering::Relaxed);
                let mme = ((value >> 4) & 0x7) as u8;
                self.set_multiple_message_enable(mme);
            }
            0x04 => {
                self.address_lo
                    .store(value & 0xFFFF_FFFC, Ordering::Relaxed);
            }
            0x08 if self.is_64bit => {
                self.address_hi.store(value, Ordering::Relaxed);
            }
            0x08 if !self.is_64bit => {
                self.data.store(value, Ordering::Relaxed);
            }
            0x0C if self.is_64bit => {
                self.data.store(value, Ordering::Relaxed);
            }
            0x10 if self.can_mask => {
                self.mask.store(value, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

impl std::fmt::Debug for MsiCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MsiCapability")
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .field("is_64bit", &self.is_64bit)
            .field("can_mask", &self.can_mask)
            .field("num_vectors", &self.num_vectors())
            .finish()
    }
}

/// MSI-X table entry
#[derive(Debug, Clone, Copy)]
pub struct MsixTableEntry {
    /// Message address (low)
    pub addr_lo: u32,
    /// Message address (high)
    pub addr_hi: u32,
    /// Message data
    pub data: u32,
    /// Vector control (bit 0 = masked)
    pub vector_ctrl: u32,
}

impl MsixTableEntry {
    /// Create a new masked entry
    pub const fn new() -> Self {
        Self {
            addr_lo: 0,
            addr_hi: 0,
            data: 0,
            vector_ctrl: 1, // Masked by default
        }
    }

    /// Get the MSI message
    pub fn get_message(&self) -> MsiMessage {
        let address = ((self.addr_hi as u64) << 32) | (self.addr_lo as u64);
        MsiMessage::new(address, self.data)
    }

    /// Check if masked
    pub fn is_masked(&self) -> bool {
        self.vector_ctrl & 1 != 0
    }

    /// Set masked state
    pub fn set_masked(&mut self, masked: bool) {
        if masked {
            self.vector_ctrl |= 1;
        } else {
            self.vector_ctrl &= !1;
        }
    }
}

impl Default for MsixTableEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// MSI-X capability for PCI devices
pub struct MsixCapability {
    /// MSI-X enabled
    enabled: AtomicBool,
    /// Function masked
    function_masked: AtomicBool,
    /// Number of table entries (0-2047)
    table_size: usize,
    /// Table BAR index
    table_bir: u8,
    /// Table offset within BAR
    table_offset: u32,
    /// PBA BAR index
    pba_bir: u8,
    /// PBA offset within BAR
    pba_offset: u32,
    /// Table entries
    table: RwLock<Vec<MsixTableEntry>>,
    /// Pending bit array
    pending: RwLock<Vec<u64>>,
}

impl MsixCapability {
    /// Create a new MSI-X capability
    pub fn new(
        num_vectors: usize,
        table_bir: u8,
        table_offset: u32,
        pba_bir: u8,
        pba_offset: u32,
    ) -> Self {
        let table_size = std::cmp::min(num_vectors, MSIX_MAX_VECTORS);
        let pba_size = table_size.div_ceil(64);

        Self {
            enabled: AtomicBool::new(false),
            function_masked: AtomicBool::new(true),
            table_size,
            table_bir,
            table_offset,
            pba_bir,
            pba_offset,
            table: RwLock::new(vec![MsixTableEntry::new(); table_size]),
            pending: RwLock::new(vec![0u64; pba_size]),
        }
    }

    /// Check if MSI-X is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Enable/disable MSI-X
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Check if function is masked
    pub fn is_function_masked(&self) -> bool {
        self.function_masked.load(Ordering::Relaxed)
    }

    /// Set function mask
    pub fn set_function_masked(&self, masked: bool) {
        self.function_masked.store(masked, Ordering::Relaxed);
    }

    /// Get number of vectors
    pub fn num_vectors(&self) -> usize {
        self.table_size
    }

    /// Get table BAR and offset
    pub fn table_location(&self) -> (u8, u32) {
        (self.table_bir, self.table_offset)
    }

    /// Get PBA BAR and offset
    pub fn pba_location(&self) -> (u8, u32) {
        (self.pba_bir, self.pba_offset)
    }

    /// Read table entry
    pub fn read_table(&self, index: usize, offset: usize) -> u32 {
        if index >= self.table_size {
            return 0;
        }

        let table = self.table.read().unwrap_or_else(|e| e.into_inner());
        match offset {
            0 => table[index].addr_lo,
            4 => table[index].addr_hi,
            8 => table[index].data,
            12 => table[index].vector_ctrl,
            _ => 0,
        }
    }

    /// Write table entry
    pub fn write_table(&self, index: usize, offset: usize, value: u32) {
        if index >= self.table_size {
            return;
        }

        let mut table = self.table.write().unwrap_or_else(|e| e.into_inner());
        match offset {
            0 => table[index].addr_lo = value & 0xFFFF_FFFC,
            4 => table[index].addr_hi = value,
            8 => table[index].data = value,
            12 => table[index].vector_ctrl = value & 1, // Only bit 0 writable
            _ => {}
        }
    }

    /// Get the message for a vector
    pub fn get_message(&self, vector: usize) -> Option<MsiMessage> {
        if vector >= self.table_size {
            return None;
        }

        let table = self.table.read().unwrap_or_else(|e| e.into_inner());
        Some(table[vector].get_message())
    }

    /// Check if a vector is masked
    pub fn is_vector_masked(&self, vector: usize) -> bool {
        if vector >= self.table_size {
            return true;
        }

        if self.function_masked.load(Ordering::Relaxed) {
            return true;
        }

        let table = self.table.read().unwrap_or_else(|e| e.into_inner());
        table[vector].is_masked()
    }

    /// Set pending bit
    pub fn set_pending(&self, vector: usize) {
        if vector >= self.table_size {
            return;
        }

        let mut pending = self.pending.write().unwrap_or_else(|e| e.into_inner());
        let qword = vector / 64;
        let bit = vector % 64;
        pending[qword] |= 1 << bit;
    }

    /// Clear pending bit
    pub fn clear_pending(&self, vector: usize) {
        if vector >= self.table_size {
            return;
        }

        let mut pending = self.pending.write().unwrap_or_else(|e| e.into_inner());
        let qword = vector / 64;
        let bit = vector % 64;
        pending[qword] &= !(1 << bit);
    }

    /// Check if pending
    pub fn is_pending(&self, vector: usize) -> bool {
        if vector >= self.table_size {
            return false;
        }

        let pending = self.pending.read().unwrap_or_else(|e| e.into_inner());
        let qword = vector / 64;
        let bit = vector % 64;
        pending[qword] & (1 << bit) != 0
    }

    /// Read PBA
    pub fn read_pba(&self, qword_index: usize) -> u64 {
        let pending = self.pending.read().unwrap_or_else(|e| e.into_inner());
        if qword_index < pending.len() {
            pending[qword_index]
        } else {
            0
        }
    }

    /// Read PCI config space
    pub fn read_config(&self, offset: u8) -> u32 {
        match offset {
            0x00 => {
                // Message Control
                let mut ctrl = ((self.table_size - 1) as u32) & 0x7FF;
                if self.function_masked.load(Ordering::Relaxed) {
                    ctrl |= 1 << 14;
                }
                if self.enabled.load(Ordering::Relaxed) {
                    ctrl |= 1 << 15;
                }
                ctrl
            }
            0x04 => {
                // Table Offset / BIR
                self.table_offset | (self.table_bir as u32)
            }
            0x08 => {
                // PBA Offset / BIR
                self.pba_offset | (self.pba_bir as u32)
            }
            _ => 0,
        }
    }

    /// Write PCI config space
    pub fn write_config(&self, offset: u8, value: u32) {
        match offset {
            0x00 => {
                // Message Control (only bits 14-15 writable)
                self.function_masked
                    .store(value & (1 << 14) != 0, Ordering::Relaxed);
                self.enabled
                    .store(value & (1 << 15) != 0, Ordering::Relaxed);
            }
            _ => {} // BIR/offset are read-only
        }
    }
}

impl std::fmt::Debug for MsixCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MsixCapability")
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .field(
                "function_masked",
                &self.function_masked.load(Ordering::Relaxed),
            )
            .field("table_size", &self.table_size)
            .field("table_bir", &self.table_bir)
            .field("pba_bir", &self.pba_bir)
            .finish()
    }
}

/// MSI/MSI-X interrupt callback
pub type MsiCallback = Box<dyn Fn(MsiMessage) + Send + Sync>;

/// MSI controller for delivering MSI/MSI-X interrupts
pub struct MsiController {
    /// Callback for interrupt delivery
    callback: RwLock<Option<MsiCallback>>,
}

impl MsiController {
    /// Create a new MSI controller
    pub fn new() -> Self {
        Self {
            callback: RwLock::new(None),
        }
    }

    /// Set interrupt delivery callback
    pub fn set_callback<F>(&self, callback: F)
    where
        F: Fn(MsiMessage) + Send + Sync + 'static,
    {
        *self.callback.write().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(callback));
    }

    /// Deliver an MSI interrupt
    pub fn deliver(&self, message: MsiMessage) {
        if let Some(ref cb) = *self.callback.read().unwrap_or_else(|e| e.into_inner()) {
            cb(message);
        }
    }

    /// Try to deliver MSI from capability
    pub fn deliver_msi(&self, cap: &MsiCapability, vector_offset: u8) -> bool {
        if !cap.is_enabled() {
            return false;
        }

        if cap.is_masked(vector_offset) {
            cap.set_pending(vector_offset);
            return false;
        }

        let message = cap.get_message(vector_offset);
        self.deliver(message);
        true
    }

    /// Try to deliver MSI-X from capability
    pub fn deliver_msix(&self, cap: &MsixCapability, vector: usize) -> bool {
        if !cap.is_enabled() {
            return false;
        }

        if cap.is_vector_masked(vector) {
            cap.set_pending(vector);
            return false;
        }

        if let Some(message) = cap.get_message(vector) {
            cap.clear_pending(vector);
            self.deliver(message);
            true
        } else {
            false
        }
    }
}

impl Default for MsiController {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MsiController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MsiController").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msi_message_create() {
        let msg = MsiMessage::create(5, 0x42, MsiDeliveryMode::Fixed, MsiDestMode::Physical);

        assert_eq!(msg.dest_id(), 5);
        assert_eq!(msg.vector(), 0x42);
        assert_eq!(msg.delivery_mode(), MsiDeliveryMode::Fixed);
        assert_eq!(msg.dest_mode(), MsiDestMode::Physical);
        assert!(msg.is_valid_address());
    }

    #[test]
    fn test_msi_message_parse() {
        let msg = MsiMessage::new(
            MSI_ADDR_BASE | (3 << 12) | msi_addr::DEST_MODE,
            0x30 | (MsiDeliveryMode::LowestPriority as u32) << 8,
        );

        assert_eq!(msg.dest_id(), 3);
        assert_eq!(msg.vector(), 0x30);
        assert_eq!(msg.delivery_mode(), MsiDeliveryMode::LowestPriority);
        assert_eq!(msg.dest_mode(), MsiDestMode::Logical);
    }

    #[test]
    fn test_msi_capability_create() {
        let cap = MsiCapability::new(4, true, true);

        assert!(!cap.is_enabled());
        assert_eq!(cap.num_vectors(), 1); // MME starts at 0
    }

    #[test]
    fn test_msi_capability_enable() {
        let cap = MsiCapability::new(4, true, true);

        cap.set_enabled(true);
        assert!(cap.is_enabled());

        cap.set_multiple_message_enable(2); // 4 vectors
        assert_eq!(cap.num_vectors(), 4);
    }

    #[test]
    fn test_msi_capability_message() {
        let cap = MsiCapability::new(4, true, true);

        cap.set_address(0xFEE0_5000);
        cap.set_data(0x42);
        cap.set_multiple_message_enable(2);

        let msg0 = cap.get_message(0);
        assert_eq!(msg0.vector(), 0x42);

        let msg2 = cap.get_message(2);
        assert_eq!(msg2.vector(), 0x44);
    }

    #[test]
    fn test_msi_capability_masking() {
        let cap = MsiCapability::new(4, true, true);

        assert!(!cap.is_masked(0));

        cap.set_mask(0b0101);
        assert!(cap.is_masked(0));
        assert!(!cap.is_masked(1));
        assert!(cap.is_masked(2));
    }

    #[test]
    fn test_msi_config_read_write() {
        let cap = MsiCapability::new(8, true, true);

        // Write enable + MME=2
        cap.write_config(0x00, 0x21);
        assert!(cap.is_enabled());
        assert_eq!(cap.num_vectors(), 4);

        // Read back
        let ctrl = cap.read_config(0x00);
        assert!(ctrl & 1 != 0); // Enabled
        assert_eq!((ctrl >> 4) & 0x7, 2); // MME
    }

    #[test]
    fn test_msix_table_entry() {
        let mut entry = MsixTableEntry::new();

        // Default is masked
        assert!(entry.is_masked());

        entry.addr_lo = 0xFEE0_5000;
        entry.addr_hi = 0;
        entry.data = 0x42;
        entry.set_masked(false);

        let msg = entry.get_message();
        assert_eq!(msg.vector(), 0x42);
        assert!(!entry.is_masked());
    }

    #[test]
    fn test_msix_capability_create() {
        let cap = MsixCapability::new(64, 0, 0x2000, 0, 0x3000);

        assert!(!cap.is_enabled());
        assert!(cap.is_function_masked());
        assert_eq!(cap.num_vectors(), 64);

        let (bir, offset) = cap.table_location();
        assert_eq!(bir, 0);
        assert_eq!(offset, 0x2000);
    }

    #[test]
    fn test_msix_table_access() {
        let cap = MsixCapability::new(4, 0, 0, 0, 0x100);

        // Write entry 0
        cap.write_table(0, 0, 0xFEE0_5000);
        cap.write_table(0, 4, 0);
        cap.write_table(0, 8, 0x42);
        cap.write_table(0, 12, 0); // Unmask

        // Read back
        assert_eq!(cap.read_table(0, 0), 0xFEE0_5000);
        assert_eq!(cap.read_table(0, 8), 0x42);
        assert_eq!(cap.read_table(0, 12), 0);
    }

    #[test]
    fn test_msix_masking() {
        let cap = MsixCapability::new(4, 0, 0, 0, 0x100);
        cap.set_enabled(true);
        cap.set_function_masked(false);

        // Entry is masked by default
        assert!(cap.is_vector_masked(0));

        // Unmask via table write
        cap.write_table(0, 12, 0);
        assert!(!cap.is_vector_masked(0));

        // Function mask overrides
        cap.set_function_masked(true);
        assert!(cap.is_vector_masked(0));
    }

    #[test]
    fn test_msix_pending_bits() {
        let cap = MsixCapability::new(128, 0, 0, 0, 0x800);

        assert!(!cap.is_pending(42));

        cap.set_pending(42);
        assert!(cap.is_pending(42));

        let pba = cap.read_pba(0);
        assert!(pba & (1 << 42) != 0);

        cap.clear_pending(42);
        assert!(!cap.is_pending(42));
    }

    #[test]
    fn test_msix_config() {
        let cap = MsixCapability::new(256, 2, 0x4000, 2, 0x5000);

        // Read table size
        let ctrl = cap.read_config(0x00);
        assert_eq!(ctrl & 0x7FF, 255); // table_size - 1

        // Read BIR/offset
        let table = cap.read_config(0x04);
        assert_eq!(table & 0x7, 2);
        assert_eq!(table & !0x7, 0x4000);

        // Enable
        cap.write_config(0x00, 1 << 15);
        assert!(cap.is_enabled());
        assert!(!cap.is_function_masked());
    }

    #[test]
    fn test_msi_controller_deliver() {
        let controller = MsiController::new();
        use std::sync::atomic::AtomicU8;

        let received_vector = Arc::new(AtomicU8::new(0));
        let received_clone = received_vector.clone();

        controller.set_callback(move |msg| {
            received_clone.store(msg.vector(), Ordering::Relaxed);
        });

        let msg = MsiMessage::create(0, 0x42, MsiDeliveryMode::Fixed, MsiDestMode::Physical);
        controller.deliver(msg);

        assert_eq!(received_vector.load(Ordering::Relaxed), 0x42);
    }

    #[test]
    fn test_controller_deliver_msi() {
        let controller = MsiController::new();
        let cap = MsiCapability::new(4, true, false);

        cap.set_enabled(true);
        cap.set_address(0xFEE0_0000);
        cap.set_data(0x30);

        let delivered = Arc::new(AtomicBool::new(false));
        let delivered_clone = delivered.clone();

        controller.set_callback(move |_msg| {
            delivered_clone.store(true, Ordering::Relaxed);
        });

        assert!(controller.deliver_msi(&cap, 0));
        assert!(delivered.load(Ordering::Relaxed));
    }

    #[test]
    fn test_controller_deliver_msix() {
        let controller = MsiController::new();
        let cap = MsixCapability::new(4, 0, 0, 0, 0x100);

        cap.set_enabled(true);
        cap.set_function_masked(false);
        cap.write_table(0, 0, 0xFEE0_0000);
        cap.write_table(0, 8, 0x30);
        cap.write_table(0, 12, 0); // Unmask

        let delivered = Arc::new(AtomicBool::new(false));
        let delivered_clone = delivered.clone();

        controller.set_callback(move |_msg| {
            delivered_clone.store(true, Ordering::Relaxed);
        });

        assert!(controller.deliver_msix(&cap, 0));
        assert!(delivered.load(Ordering::Relaxed));
    }

    #[test]
    fn test_masked_sets_pending() {
        let controller = MsiController::new();
        let cap = MsiCapability::new(4, true, true);

        cap.set_enabled(true);
        cap.set_mask(0x1); // Mask vector 0

        // Delivery should fail but set pending
        assert!(!controller.deliver_msi(&cap, 0));
        assert!(cap.get_pending() & 1 != 0);
    }

    #[test]
    fn test_delivery_mode_from_bits() {
        assert_eq!(MsiDeliveryMode::from_bits(0), MsiDeliveryMode::Fixed);
        assert_eq!(
            MsiDeliveryMode::from_bits(1),
            MsiDeliveryMode::LowestPriority
        );
        assert_eq!(MsiDeliveryMode::from_bits(4), MsiDeliveryMode::Nmi);
    }

    use std::sync::Arc;
}
