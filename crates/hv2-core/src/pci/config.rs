//! PCI Configuration Space
//!
//! This module provides PCI configuration space emulation including
//! standard headers (Type 0, 1, 2) and PCIe extended configuration.

use super::types::{
    BarType, ClassCode, CommandRegister, DeviceId, HeaderType, InterruptPin, StatusRegister,
    VendorId,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// PCI configuration space size
pub const PCI_CONFIG_SIZE: usize = 256;
/// PCIe extended configuration space size
pub const PCIE_CONFIG_SIZE: usize = 4096;

/// PCI configuration space register offsets
pub mod registers {
    /// Vendor ID (16-bit)
    pub const VENDOR_ID: u8 = 0x00;
    /// Device ID (16-bit)
    pub const DEVICE_ID: u8 = 0x02;
    /// Command (16-bit)
    pub const COMMAND: u8 = 0x04;
    /// Status (16-bit)
    pub const STATUS: u8 = 0x06;
    /// Revision ID (8-bit)
    pub const REVISION_ID: u8 = 0x08;
    /// Programming Interface (8-bit)
    pub const PROG_IF: u8 = 0x09;
    /// Sub Class (8-bit)
    pub const SUBCLASS: u8 = 0x0A;
    /// Base Class (8-bit)
    pub const CLASS_CODE: u8 = 0x0B;
    /// Cache Line Size (8-bit)
    pub const CACHE_LINE_SIZE: u8 = 0x0C;
    /// Latency Timer (8-bit)
    pub const LATENCY_TIMER: u8 = 0x0D;
    /// Header Type (8-bit)
    pub const HEADER_TYPE: u8 = 0x0E;
    /// BIST (8-bit)
    pub const BIST: u8 = 0x0F;

    // Type 0 specific
    /// BAR0 (32-bit)
    pub const BAR0: u8 = 0x10;
    /// BAR1 (32-bit)
    pub const BAR1: u8 = 0x14;
    /// BAR2 (32-bit)
    pub const BAR2: u8 = 0x18;
    /// BAR3 (32-bit)
    pub const BAR3: u8 = 0x1C;
    /// BAR4 (32-bit)
    pub const BAR4: u8 = 0x20;
    /// BAR5 (32-bit)
    pub const BAR5: u8 = 0x24;
    /// CardBus CIS Pointer (32-bit)
    pub const CARDBUS_CIS: u8 = 0x28;
    /// Subsystem Vendor ID (16-bit)
    pub const SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
    /// Subsystem ID (16-bit)
    pub const SUBSYSTEM_ID: u8 = 0x2E;
    /// Expansion ROM Base Address (32-bit)
    pub const ROM_ADDRESS: u8 = 0x30;
    /// Capabilities Pointer (8-bit)
    pub const CAPABILITIES_PTR: u8 = 0x34;
    /// Interrupt Line (8-bit)
    pub const INTERRUPT_LINE: u8 = 0x3C;
    /// Interrupt Pin (8-bit)
    pub const INTERRUPT_PIN: u8 = 0x3D;
    /// Min Grant (8-bit)
    pub const MIN_GNT: u8 = 0x3E;
    /// Max Latency (8-bit)
    pub const MAX_LAT: u8 = 0x3F;

    // Type 1 (PCI Bridge) specific
    /// Primary Bus Number (8-bit)
    pub const PRIMARY_BUS: u8 = 0x18;
    /// Secondary Bus Number (8-bit)
    pub const SECONDARY_BUS: u8 = 0x19;
    /// Subordinate Bus Number (8-bit)
    pub const SUBORDINATE_BUS: u8 = 0x1A;
    /// Secondary Latency Timer (8-bit)
    pub const SECONDARY_LATENCY: u8 = 0x1B;
    /// I/O Base (8-bit)
    pub const IO_BASE: u8 = 0x1C;
    /// I/O Limit (8-bit)
    pub const IO_LIMIT: u8 = 0x1D;
    /// Secondary Status (16-bit)
    pub const SECONDARY_STATUS: u8 = 0x1E;
    /// Memory Base (16-bit)
    pub const MEMORY_BASE: u8 = 0x20;
    /// Memory Limit (16-bit)
    pub const MEMORY_LIMIT: u8 = 0x22;
    /// Prefetchable Memory Base (16-bit)
    pub const PREF_MEMORY_BASE: u8 = 0x24;
    /// Prefetchable Memory Limit (16-bit)
    pub const PREF_MEMORY_LIMIT: u8 = 0x26;
    /// Prefetchable Base Upper 32 Bits (32-bit)
    pub const PREF_BASE_UPPER: u8 = 0x28;
    /// Prefetchable Limit Upper 32 Bits (32-bit)
    pub const PREF_LIMIT_UPPER: u8 = 0x2C;
    /// I/O Base Upper 16 Bits (16-bit)
    pub const IO_BASE_UPPER: u8 = 0x30;
    /// I/O Limit Upper 16 Bits (16-bit)
    pub const IO_LIMIT_UPPER: u8 = 0x32;
    /// Bridge Control (16-bit)
    pub const BRIDGE_CONTROL: u8 = 0x3E;
}

/// Bridge control register bits
pub mod bridge_control {
    /// Parity Error Response Enable
    pub const PARITY_ERROR_RESPONSE: u16 = 1 << 0;
    /// SERR# Enable
    pub const SERR_ENABLE: u16 = 1 << 1;
    /// ISA Enable
    pub const ISA_ENABLE: u16 = 1 << 2;
    /// VGA Enable
    pub const VGA_ENABLE: u16 = 1 << 3;
    /// Master Abort Mode
    pub const MASTER_ABORT_MODE: u16 = 1 << 5;
    /// Secondary Bus Reset
    pub const SECONDARY_BUS_RESET: u16 = 1 << 6;
    /// Fast Back-to-Back Enable
    pub const FAST_B2B_ENABLE: u16 = 1 << 7;
    /// Primary Discard Timer
    pub const PRIMARY_DISCARD_TIMER: u16 = 1 << 8;
    /// Secondary Discard Timer
    pub const SECONDARY_DISCARD_TIMER: u16 = 1 << 9;
    /// Discard Timer Status
    pub const DISCARD_TIMER_STATUS: u16 = 1 << 10;
    /// Discard Timer SERR# Enable
    pub const DISCARD_TIMER_SERR: u16 = 1 << 11;
}

/// Configuration space statistics
#[derive(Debug, Default)]
pub struct ConfigStats {
    /// Read operations
    reads: AtomicU64,
    /// Write operations
    writes: AtomicU64,
    /// BAR accesses
    bar_accesses: AtomicU64,
}

impl ConfigStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record read
    pub fn record_read(&self) {
        self.reads.fetch_add(1, Ordering::Relaxed);
    }

    /// Record write
    pub fn record_write(&self) {
        self.writes.fetch_add(1, Ordering::Relaxed);
    }

    /// Record BAR access
    pub fn record_bar_access(&self) {
        self.bar_accesses.fetch_add(1, Ordering::Relaxed);
    }

    /// Get read count
    pub fn read_count(&self) -> u64 {
        self.reads.load(Ordering::Relaxed)
    }

    /// Get write count
    pub fn write_count(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }
}

/// BAR configuration
#[derive(Debug, Clone, Default)]
pub struct BarConfig {
    /// Current value
    pub value: u32,
    /// Size mask (for size detection)
    pub size_mask: u32,
    /// Whether this is the upper half of a 64-bit BAR
    pub is_upper: bool,
    /// Whether BAR is enabled
    pub enabled: bool,
}

impl BarConfig {
    /// Create new BAR with size
    pub fn new_memory32(size: u32, prefetchable: bool) -> Self {
        let size_mask = if size > 0 { !(size - 1) | 0x0F } else { 0 };
        Self {
            value: if prefetchable { 0x08 } else { 0x00 },
            size_mask,
            is_upper: false,
            enabled: size > 0,
        }
    }

    /// Create 64-bit memory BAR (lower half)
    pub fn new_memory64_lower(size: u64, prefetchable: bool) -> Self {
        let size_mask = if size > 0 {
            !((size - 1) as u32) | 0x0F
        } else {
            0
        };
        Self {
            value: 0x04 | if prefetchable { 0x08 } else { 0x00 },
            size_mask,
            is_upper: false,
            enabled: size > 0,
        }
    }

    /// Create 64-bit memory BAR (upper half)
    pub fn new_memory64_upper(size: u64) -> Self {
        let size_mask = if size > 0xFFFFFFFF {
            !((size >> 32) as u32 - 1)
        } else {
            0xFFFFFFFF
        };
        Self {
            value: 0,
            size_mask,
            is_upper: true,
            enabled: true,
        }
    }

    /// Create I/O BAR
    pub fn new_io(size: u32) -> Self {
        let size_mask = if size > 0 { !(size - 1) | 0x03 } else { 0 };
        Self {
            value: 0x01,
            size_mask,
            is_upper: false,
            enabled: size > 0,
        }
    }

    /// Get BAR type
    pub fn bar_type(&self) -> BarType {
        BarType::from_register(self.value, self.size_mask)
    }

    /// Handle write to BAR
    pub fn write(&mut self, value: u32) {
        if self.is_upper {
            self.value = value & self.size_mask;
        } else if self.value & 0x01 != 0 {
            // I/O BAR
            self.value = (value & self.size_mask) | 0x01;
        } else {
            // Memory BAR - preserve type bits
            let type_bits = self.value & 0x0F;
            self.value = (value & self.size_mask) | type_bits;
        }
    }

    /// Read BAR (for size detection)
    pub fn read(&self, sizing: bool) -> u32 {
        if sizing {
            self.size_mask
        } else {
            self.value
        }
    }
}

/// PCI Configuration Space (Type 0 - Standard Device)
#[derive(Debug)]
pub struct ConfigSpace {
    /// Raw configuration data
    data: [u8; PCIE_CONFIG_SIZE],
    /// BAR configurations
    bars: [BarConfig; 6],
    /// ROM BAR configuration
    rom_bar: BarConfig,
    /// Capabilities pointer
    cap_ptr: u8,
    /// Write mask (which bits are writable)
    write_mask: [u8; PCIE_CONFIG_SIZE],
    /// Is sizing mode active
    sizing_active: bool,
    /// Statistics
    stats: ConfigStats,
}

impl Default for ConfigSpace {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigSpace {
    /// Create new configuration space
    pub fn new() -> Self {
        let mut space = Self {
            data: [0; PCIE_CONFIG_SIZE],
            bars: Default::default(),
            rom_bar: BarConfig::default(),
            cap_ptr: 0,
            write_mask: [0; PCIE_CONFIG_SIZE],
            sizing_active: false,
            stats: ConfigStats::new(),
        };

        // Set default vendor/device to invalid
        space.set_u16(registers::VENDOR_ID, 0xFFFF);
        space.set_u16(registers::DEVICE_ID, 0xFFFF);

        // Initialize write masks for standard registers
        space.init_write_masks();

        space
    }

    /// Initialize with device info
    pub fn with_device(
        vendor_id: VendorId,
        device_id: DeviceId,
        class_code: ClassCode,
        revision: u8,
    ) -> Self {
        let mut space = Self::new();
        space.set_u16(registers::VENDOR_ID, vendor_id.0);
        space.set_u16(registers::DEVICE_ID, device_id.0);
        space.data[registers::REVISION_ID as usize] = revision;
        space.data[registers::PROG_IF as usize] = class_code.prog_if;
        space.data[registers::SUBCLASS as usize] = class_code.sub;
        space.data[registers::CLASS_CODE as usize] = class_code.base;
        space.data[registers::HEADER_TYPE as usize] = 0x00; // Type 0
        space
    }

    /// Initialize write masks for standard Type 0 header
    fn init_write_masks(&mut self) {
        // Command register - most bits writable
        self.write_mask[registers::COMMAND as usize] = 0xFF;
        self.write_mask[registers::COMMAND as usize + 1] = 0x07;

        // Status register - write-1-to-clear bits
        self.write_mask[registers::STATUS as usize] = 0x00;
        self.write_mask[registers::STATUS as usize + 1] = 0xF9;

        // Cache line size, latency timer
        self.write_mask[registers::CACHE_LINE_SIZE as usize] = 0xFF;
        self.write_mask[registers::LATENCY_TIMER as usize] = 0xFF;

        // BIST
        self.write_mask[registers::BIST as usize] = 0x40;

        // BARs - handled specially
        for i in 0..6 {
            let offset = registers::BAR0 as usize + i * 4;
            self.write_mask[offset] = 0xFF;
            self.write_mask[offset + 1] = 0xFF;
            self.write_mask[offset + 2] = 0xFF;
            self.write_mask[offset + 3] = 0xFF;
        }

        // ROM address
        self.write_mask[registers::ROM_ADDRESS as usize] = 0x01;
        self.write_mask[registers::ROM_ADDRESS as usize + 1] = 0xF8;
        self.write_mask[registers::ROM_ADDRESS as usize + 2] = 0xFF;
        self.write_mask[registers::ROM_ADDRESS as usize + 3] = 0xFF;

        // Interrupt line
        self.write_mask[registers::INTERRUPT_LINE as usize] = 0xFF;
    }

    /// Get statistics
    pub fn stats(&self) -> &ConfigStats {
        &self.stats
    }

    /// Read 8-bit value
    pub fn read_u8(&self, offset: u16) -> u8 {
        self.stats.record_read();
        if (offset as usize) < PCIE_CONFIG_SIZE {
            self.data[offset as usize]
        } else {
            0xFF
        }
    }

    /// Read 16-bit value
    pub fn read_u16(&self, offset: u16) -> u16 {
        self.stats.record_read();
        let offset = offset as usize & !1;
        if offset + 1 < PCIE_CONFIG_SIZE {
            u16::from_le_bytes([self.data[offset], self.data[offset + 1]])
        } else {
            0xFFFF
        }
    }

    /// Read 32-bit value
    pub fn read_u32(&self, offset: u16) -> u32 {
        self.stats.record_read();
        let offset = offset as usize & !3;

        // Handle BAR reads specially
        if offset >= registers::BAR0 as usize && offset <= registers::BAR5 as usize + 3 {
            let bar_index = (offset - registers::BAR0 as usize) / 4;
            if bar_index < 6 {
                self.stats.record_bar_access();
                return self.bars[bar_index].read(self.sizing_active);
            }
        }

        if offset >= registers::ROM_ADDRESS as usize && offset < registers::ROM_ADDRESS as usize + 4
        {
            return self.rom_bar.read(self.sizing_active);
        }

        if offset + 3 < PCIE_CONFIG_SIZE {
            u32::from_le_bytes([
                self.data[offset],
                self.data[offset + 1],
                self.data[offset + 2],
                self.data[offset + 3],
            ])
        } else {
            0xFFFFFFFF
        }
    }

    /// Write 8-bit value
    pub fn write_u8(&mut self, offset: u16, value: u8) {
        self.stats.record_write();
        let offset = offset as usize;
        if offset < PCIE_CONFIG_SIZE {
            let mask = self.write_mask[offset];
            self.data[offset] = (self.data[offset] & !mask) | (value & mask);
        }
    }

    /// Write 16-bit value
    pub fn write_u16(&mut self, offset: u16, value: u16) {
        self.stats.record_write();
        let offset = offset as usize & !1;
        if offset + 1 < PCIE_CONFIG_SIZE {
            let bytes = value.to_le_bytes();
            for (i, &byte) in bytes.iter().enumerate().take(2) {
                let mask = self.write_mask[offset + i];
                self.data[offset + i] = (self.data[offset + i] & !mask) | (byte & mask);
            }
        }
    }

    /// Write 32-bit value
    pub fn write_u32(&mut self, offset: u16, value: u32) {
        self.stats.record_write();
        let offset = offset as usize & !3;

        // Handle BAR writes specially
        if offset >= registers::BAR0 as usize && offset <= registers::BAR5 as usize + 3 {
            let bar_index = (offset - registers::BAR0 as usize) / 4;
            if bar_index < 6 {
                self.stats.record_bar_access();
                // Detect sizing operation (writing all 1s)
                if value == 0xFFFFFFFF {
                    self.sizing_active = true;
                } else {
                    self.sizing_active = false;
                    self.bars[bar_index].write(value);
                }
                // Update data array for reads
                let bar_value = self.bars[bar_index].read(self.sizing_active);
                self.data[offset..offset + 4].copy_from_slice(&bar_value.to_le_bytes());
                return;
            }
        }

        if offset >= registers::ROM_ADDRESS as usize && offset < registers::ROM_ADDRESS as usize + 4
        {
            if value == 0xFFFFFFFF || value == 0xFFFFFFFE {
                self.sizing_active = true;
            } else {
                self.sizing_active = false;
                self.rom_bar.write(value);
            }
            return;
        }

        if offset + 3 < PCIE_CONFIG_SIZE {
            let bytes = value.to_le_bytes();
            for (i, &byte) in bytes.iter().enumerate().take(4) {
                let mask = self.write_mask[offset + i];
                self.data[offset + i] = (self.data[offset + i] & !mask) | (byte & mask);
            }
        }
    }

    /// Set 16-bit value directly (bypasses write mask)
    pub fn set_u16(&mut self, offset: u8, value: u16) {
        let offset = offset as usize;
        if offset + 1 < PCIE_CONFIG_SIZE {
            self.data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// Set 32-bit value directly (bypasses write mask)
    pub fn set_u32(&mut self, offset: u8, value: u32) {
        let offset = offset as usize;
        if offset + 3 < PCIE_CONFIG_SIZE {
            self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// Get vendor ID
    pub fn vendor_id(&self) -> VendorId {
        VendorId(self.read_u16(registers::VENDOR_ID as u16))
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        DeviceId(self.read_u16(registers::DEVICE_ID as u16))
    }

    /// Get class code
    pub fn class_code(&self) -> ClassCode {
        ClassCode::new(
            self.data[registers::CLASS_CODE as usize],
            self.data[registers::SUBCLASS as usize],
            self.data[registers::PROG_IF as usize],
        )
    }

    /// Get command register
    pub fn command(&self) -> CommandRegister {
        CommandRegister::new(self.read_u16(registers::COMMAND as u16))
    }

    /// Set command register
    pub fn set_command(&mut self, cmd: CommandRegister) {
        self.write_u16(registers::COMMAND as u16, cmd.0);
    }

    /// Get status register
    pub fn status(&self) -> StatusRegister {
        StatusRegister::new(self.read_u16(registers::STATUS as u16))
    }

    /// Get header type
    pub fn header_type(&self) -> Option<HeaderType> {
        HeaderType::from_register(self.data[registers::HEADER_TYPE as usize])
    }

    /// Check if multi-function
    pub fn is_multifunction(&self) -> bool {
        HeaderType::is_multifunction(self.data[registers::HEADER_TYPE as usize])
    }

    /// Set multi-function bit
    pub fn set_multifunction(&mut self, multi: bool) {
        if multi {
            self.data[registers::HEADER_TYPE as usize] |= 0x80;
        } else {
            self.data[registers::HEADER_TYPE as usize] &= 0x7F;
        }
    }

    /// Get interrupt line
    pub fn interrupt_line(&self) -> u8 {
        self.data[registers::INTERRUPT_LINE as usize]
    }

    /// Set interrupt line
    pub fn set_interrupt_line(&mut self, line: u8) {
        self.data[registers::INTERRUPT_LINE as usize] = line;
    }

    /// Get interrupt pin
    pub fn interrupt_pin(&self) -> InterruptPin {
        InterruptPin::from_register(self.data[registers::INTERRUPT_PIN as usize])
    }

    /// Set interrupt pin
    pub fn set_interrupt_pin(&mut self, pin: InterruptPin) {
        self.data[registers::INTERRUPT_PIN as usize] = pin as u8;
    }

    /// Get subsystem vendor ID
    pub fn subsystem_vendor_id(&self) -> VendorId {
        VendorId(self.read_u16(registers::SUBSYSTEM_VENDOR_ID as u16))
    }

    /// Set subsystem IDs
    pub fn set_subsystem(&mut self, vendor: VendorId, device: DeviceId) {
        self.set_u16(registers::SUBSYSTEM_VENDOR_ID, vendor.0);
        self.set_u16(registers::SUBSYSTEM_ID, device.0);
    }

    /// Configure BAR
    pub fn configure_bar(&mut self, index: usize, config: BarConfig) {
        if index < 6 {
            self.bars[index] = config;
            // Update data array
            let offset = registers::BAR0 as usize + index * 4;
            self.data[offset..offset + 4].copy_from_slice(&self.bars[index].value.to_le_bytes());
        }
    }

    /// Get BAR configuration
    pub fn bar(&self, index: usize) -> Option<&BarConfig> {
        self.bars.get(index)
    }

    /// Get BAR type
    pub fn bar_type(&self, index: usize) -> Option<BarType> {
        self.bars.get(index).map(|b| b.bar_type())
    }

    /// Set capabilities pointer
    pub fn set_capabilities_ptr(&mut self, ptr: u8) {
        self.cap_ptr = ptr;
        self.data[registers::CAPABILITIES_PTR as usize] = ptr;
        // Enable capabilities list in status
        let status = self.read_u16(registers::STATUS as u16);
        self.set_u16(
            registers::STATUS,
            status | StatusRegister::CAPABILITIES_LIST,
        );
    }

    /// Get capabilities pointer
    pub fn capabilities_ptr(&self) -> u8 {
        self.data[registers::CAPABILITIES_PTR as usize]
    }

    /// Check if device is present
    pub fn is_present(&self) -> bool {
        self.vendor_id().is_valid()
    }

    /// Get raw data slice
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get mutable raw data slice
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

/// PCI Bridge Configuration Space (Type 1)
#[derive(Debug)]
pub struct BridgeConfigSpace {
    /// Base configuration space
    base: ConfigSpace,
    /// Primary bus number
    primary_bus: u8,
    /// Secondary bus number
    secondary_bus: u8,
    /// Subordinate bus number
    subordinate_bus: u8,
    /// Bridge control
    bridge_control: u16,
}

impl Default for BridgeConfigSpace {
    fn default() -> Self {
        Self::new()
    }
}

impl BridgeConfigSpace {
    /// Create new bridge configuration space
    pub fn new() -> Self {
        let mut space = Self {
            base: ConfigSpace::new(),
            primary_bus: 0,
            secondary_bus: 0,
            subordinate_bus: 0,
            bridge_control: 0,
        };

        // Set header type to bridge
        space.base.data[registers::HEADER_TYPE as usize] = 0x01;

        // Initialize bridge-specific write masks
        space.init_bridge_write_masks();

        space
    }

    /// Create with device info
    pub fn with_device(vendor_id: VendorId, device_id: DeviceId, revision: u8) -> Self {
        let mut space = Self::new();
        space.base.set_u16(registers::VENDOR_ID, vendor_id.0);
        space.base.set_u16(registers::DEVICE_ID, device_id.0);
        space.base.data[registers::REVISION_ID as usize] = revision;
        space.base.data[registers::CLASS_CODE as usize] = ClassCode::PCI_BRIDGE.base;
        space.base.data[registers::SUBCLASS as usize] = ClassCode::PCI_BRIDGE.sub;
        space.base.data[registers::PROG_IF as usize] = ClassCode::PCI_BRIDGE.prog_if;
        space
    }

    /// Initialize bridge-specific write masks
    fn init_bridge_write_masks(&mut self) {
        // Bus numbers
        self.base.write_mask[registers::PRIMARY_BUS as usize] = 0xFF;
        self.base.write_mask[registers::SECONDARY_BUS as usize] = 0xFF;
        self.base.write_mask[registers::SUBORDINATE_BUS as usize] = 0xFF;
        self.base.write_mask[registers::SECONDARY_LATENCY as usize] = 0xFF;

        // I/O base/limit
        self.base.write_mask[registers::IO_BASE as usize] = 0xF0;
        self.base.write_mask[registers::IO_LIMIT as usize] = 0xF0;

        // Memory base/limit
        self.base.write_mask[registers::MEMORY_BASE as usize] = 0xF0;
        self.base.write_mask[registers::MEMORY_BASE as usize + 1] = 0xFF;
        self.base.write_mask[registers::MEMORY_LIMIT as usize] = 0xF0;
        self.base.write_mask[registers::MEMORY_LIMIT as usize + 1] = 0xFF;

        // Prefetchable base/limit
        self.base.write_mask[registers::PREF_MEMORY_BASE as usize] = 0xF0;
        self.base.write_mask[registers::PREF_MEMORY_BASE as usize + 1] = 0xFF;
        self.base.write_mask[registers::PREF_MEMORY_LIMIT as usize] = 0xF0;
        self.base.write_mask[registers::PREF_MEMORY_LIMIT as usize + 1] = 0xFF;

        // Bridge control
        self.base.write_mask[registers::BRIDGE_CONTROL as usize] = 0xFF;
        self.base.write_mask[registers::BRIDGE_CONTROL as usize + 1] = 0x0F;
    }

    /// Get base configuration space
    pub fn base(&self) -> &ConfigSpace {
        &self.base
    }

    /// Get mutable base configuration space
    pub fn base_mut(&mut self) -> &mut ConfigSpace {
        &mut self.base
    }

    /// Read 32-bit value
    pub fn read_u32(&self, offset: u16) -> u32 {
        self.base.read_u32(offset)
    }

    /// Write 32-bit value
    pub fn write_u32(&mut self, offset: u16, value: u32) {
        let offset_u8 = offset as u8;

        // Handle bridge-specific registers
        match offset_u8 {
            registers::PRIMARY_BUS => {
                self.primary_bus = value as u8;
                self.secondary_bus = (value >> 8) as u8;
                self.subordinate_bus = (value >> 16) as u8;
            }
            registers::BRIDGE_CONTROL => {
                self.bridge_control = (value >> 16) as u16;
            }
            _ => {}
        }

        self.base.write_u32(offset, value);
    }

    /// Get primary bus
    pub fn primary_bus(&self) -> u8 {
        self.primary_bus
    }

    /// Get secondary bus
    pub fn secondary_bus(&self) -> u8 {
        self.secondary_bus
    }

    /// Get subordinate bus
    pub fn subordinate_bus(&self) -> u8 {
        self.subordinate_bus
    }

    /// Set bus numbers
    pub fn set_bus_numbers(&mut self, primary: u8, secondary: u8, subordinate: u8) {
        self.primary_bus = primary;
        self.secondary_bus = secondary;
        self.subordinate_bus = subordinate;

        self.base.data[registers::PRIMARY_BUS as usize] = primary;
        self.base.data[registers::SECONDARY_BUS as usize] = secondary;
        self.base.data[registers::SUBORDINATE_BUS as usize] = subordinate;
    }

    /// Get bridge control
    pub fn bridge_control(&self) -> u16 {
        self.bridge_control
    }

    /// Set bridge control
    pub fn set_bridge_control(&mut self, control: u16) {
        self.bridge_control = control;
        self.base.set_u16(registers::BRIDGE_CONTROL, control);
    }

    /// Check if VGA forwarding is enabled
    pub fn vga_enabled(&self) -> bool {
        self.bridge_control & bridge_control::VGA_ENABLE != 0
    }

    /// Check if ISA forwarding is enabled
    pub fn isa_enabled(&self) -> bool {
        self.bridge_control & bridge_control::ISA_ENABLE != 0
    }

    /// Get memory window
    pub fn memory_window(&self) -> (u32, u32) {
        let base_reg = self.base.read_u16(registers::MEMORY_BASE as u16);
        let limit_reg = self.base.read_u16(registers::MEMORY_LIMIT as u16);

        // Base is stored as bits [31:20], with lower 20 bits zero
        let base = ((base_reg as u32) & 0xFFF0) << 16;
        // Limit is stored as bits [31:20], with lower 20 bits all ones
        let limit = (((limit_reg as u32) & 0xFFF0) << 16) | 0xFFFFF;
        (base, limit)
    }

    /// Set memory window
    pub fn set_memory_window(&mut self, base: u32, limit: u32) {
        // Extract bits [31:20] for the registers
        let base_reg = ((base >> 16) & 0xFFF0) as u16;
        let limit_reg = ((limit >> 16) & 0xFFF0) as u16;
        self.base.set_u16(registers::MEMORY_BASE, base_reg);
        self.base.set_u16(registers::MEMORY_LIMIT, limit_reg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_space_creation() {
        let config = ConfigSpace::new();
        assert!(!config.is_present()); // Invalid vendor ID
    }

    #[test]
    fn test_config_space_with_device() {
        let config =
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01);
        assert!(config.is_present());
        assert_eq!(config.vendor_id(), VendorId::INTEL);
        assert!(config.class_code().is_network());
    }

    #[test]
    fn test_config_space_read_write() {
        let mut config = ConfigSpace::new();

        config.write_u32(registers::COMMAND as u16, 0x0007);
        let cmd = config.command();
        assert!(cmd.io_enabled());
        assert!(cmd.memory_enabled());
        assert!(cmd.bus_master_enabled());
    }

    #[test]
    fn test_config_space_bar() {
        let mut config = ConfigSpace::new();
        config.configure_bar(0, BarConfig::new_memory32(0x1000, false));

        // Write all 1s for size detection
        config.write_u32(registers::BAR0 as u16, 0xFFFFFFFF);
        let sizing_read = config.read_u32(registers::BAR0 as u16);
        assert_ne!(sizing_read, 0xFFFFFFFF);

        // Write actual address
        config.write_u32(registers::BAR0 as u16, 0xFEB00000);
        let bar_type = config.bar_type(0).unwrap();
        assert!(bar_type.is_memory());
    }

    #[test]
    fn test_config_space_io_bar() {
        let mut config = ConfigSpace::new();
        config.configure_bar(0, BarConfig::new_io(0x100));

        let bar_type = config.bar_type(0).unwrap();
        assert!(bar_type.is_io());
    }

    #[test]
    fn test_config_space_64bit_bar() {
        let mut config = ConfigSpace::new();
        config.configure_bar(0, BarConfig::new_memory64_lower(0x100000000, true));
        config.configure_bar(1, BarConfig::new_memory64_upper(0x100000000));

        let bar_type = config.bar_type(0).unwrap();
        assert!(bar_type.is_64bit());
        assert!(bar_type.is_prefetchable());
    }

    #[test]
    fn test_config_space_interrupt() {
        let mut config = ConfigSpace::new();
        config.set_interrupt_line(10);
        config.set_interrupt_pin(InterruptPin::IntA);

        assert_eq!(config.interrupt_line(), 10);
        assert_eq!(config.interrupt_pin(), InterruptPin::IntA);
    }

    #[test]
    fn test_config_space_subsystem() {
        let mut config = ConfigSpace::new();
        config.set_subsystem(VendorId(0x1234), DeviceId(0x5678));

        assert_eq!(config.subsystem_vendor_id(), VendorId(0x1234));
    }

    #[test]
    fn test_config_space_capabilities() {
        let mut config = ConfigSpace::new();
        config.set_capabilities_ptr(0x40);

        assert_eq!(config.capabilities_ptr(), 0x40);
        assert!(config.status().has_capabilities());
    }

    #[test]
    fn test_config_space_multifunction() {
        let mut config = ConfigSpace::new();
        assert!(!config.is_multifunction());

        config.set_multifunction(true);
        assert!(config.is_multifunction());
    }

    #[test]
    fn test_bridge_config_creation() {
        let bridge = BridgeConfigSpace::new();
        assert_eq!(bridge.base().header_type(), Some(HeaderType::PciBridge));
    }

    #[test]
    fn test_bridge_config_bus_numbers() {
        let mut bridge = BridgeConfigSpace::new();
        bridge.set_bus_numbers(0, 1, 5);

        assert_eq!(bridge.primary_bus(), 0);
        assert_eq!(bridge.secondary_bus(), 1);
        assert_eq!(bridge.subordinate_bus(), 5);
    }

    #[test]
    fn test_bridge_config_memory_window() {
        let mut bridge = BridgeConfigSpace::new();
        bridge.set_memory_window(0xE0000000, 0xEFFFFFFF);

        let (base, limit) = bridge.memory_window();
        assert_eq!(base, 0xE0000000);
        assert!(limit >= 0xEFFFFFFF);
    }

    #[test]
    fn test_bridge_control() {
        let mut bridge = BridgeConfigSpace::new();
        bridge.set_bridge_control(bridge_control::VGA_ENABLE | bridge_control::ISA_ENABLE);

        assert!(bridge.vga_enabled());
        assert!(bridge.isa_enabled());
    }

    #[test]
    fn test_bar_config_memory32() {
        let bar = BarConfig::new_memory32(0x10000, true);
        assert!(bar.enabled);
        assert!(bar.bar_type().is_prefetchable());
    }

    #[test]
    fn test_bar_config_io() {
        let bar = BarConfig::new_io(0x100);
        assert!(bar.enabled);
        assert!(bar.bar_type().is_io());
    }

    #[test]
    fn test_config_stats() {
        let mut config = ConfigSpace::new();

        config.read_u32(0);
        config.read_u32(0);
        config.write_u32(0, 0);

        assert_eq!(config.stats().read_count(), 2);
        assert_eq!(config.stats().write_count(), 1);
    }
}
