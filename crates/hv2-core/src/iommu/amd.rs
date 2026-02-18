//! AMD IOMMU (AMD-Vi) Support
//!
//! This module provides AMD IOMMU support including device table,
//! I/O page tables, event logging, and command buffer.

use super::types::{
    AddressWidth, DeviceId, DomainId, FaultReason, FaultRecord, IommuStats,
    PageTableFlags, TranslationType,
};
#[cfg(test)]
use super::types::PAGE_SIZE_4K;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// AMD IOMMU MMIO register offsets
pub mod registers {
    /// Device Table Base Address Register
    pub const DEV_TAB_BASE: u32 = 0x00;
    /// Command Buffer Base Address Register
    pub const CMD_BUF_BASE: u32 = 0x08;
    /// Event Log Base Address Register
    pub const EVT_LOG_BASE: u32 = 0x10;
    /// Control Register
    pub const CONTROL: u32 = 0x18;
    /// Exclusion Base Register
    pub const EXCL_BASE: u32 = 0x20;
    /// Exclusion Limit Register
    pub const EXCL_LIMIT: u32 = 0x28;
    /// Extended Feature Register
    pub const EXT_FEAT: u32 = 0x30;
    /// PPR Log Base Address Register
    pub const PPR_LOG_BASE: u32 = 0x38;
    /// Hardware Event Upper Register
    pub const HW_EVT_HI: u32 = 0x40;
    /// Hardware Event Lower Register
    pub const HW_EVT_LO: u32 = 0x48;
    /// Hardware Event Status Register
    pub const HW_EVT_STATUS: u32 = 0x50;
    /// SMI Filter Register
    pub const SMI_FILTER: u32 = 0x60;
    /// Guest Virtual APIC Log Base Address
    pub const GA_LOG_BASE: u32 = 0xE0;
    /// Guest Virtual APIC Log Tail Address
    pub const GA_LOG_TAIL: u32 = 0xE8;
    /// PPR Log B Base Address Register
    pub const PPR_LOG_B_BASE: u32 = 0xF0;
    /// Event Log B Base Address Register
    pub const EVT_LOG_B_BASE: u32 = 0xF8;
    /// Device Table Segment Registers (0-7)
    pub const DEV_TAB_SEG_BASE: u32 = 0x100;
    /// Command Buffer Head Pointer Register
    pub const CMD_BUF_HEAD: u32 = 0x2000;
    /// Command Buffer Tail Pointer Register
    pub const CMD_BUF_TAIL: u32 = 0x2008;
    /// Event Log Head Pointer Register
    pub const EVT_LOG_HEAD: u32 = 0x2010;
    /// Event Log Tail Pointer Register
    pub const EVT_LOG_TAIL: u32 = 0x2018;
    /// Status Register
    pub const STATUS: u32 = 0x2020;
    /// PPR Log Head Pointer Register
    pub const PPR_LOG_HEAD: u32 = 0x2030;
    /// PPR Log Tail Pointer Register
    pub const PPR_LOG_TAIL: u32 = 0x2038;
    /// GA Log Head Pointer Register
    pub const GA_LOG_HEAD: u32 = 0x2040;
    /// GA Log Tail Pointer Register
    pub const GA_LOG_TAIL_PTR: u32 = 0x2048;
    /// PPR Log B Head Pointer Register
    pub const PPR_LOG_B_HEAD: u32 = 0x2050;
    /// PPR Log B Tail Pointer Register
    pub const PPR_LOG_B_TAIL: u32 = 0x2058;
    /// Event Log B Head Pointer Register
    pub const EVT_LOG_B_HEAD: u32 = 0x2070;
    /// Event Log B Tail Pointer Register
    pub const EVT_LOG_B_TAIL: u32 = 0x2078;
    /// Capability Header
    pub const CAP_HEADER: u32 = 0x00;
    /// Capability Range
    pub const CAP_RANGE: u32 = 0x04;
    /// Capability Misc
    pub const CAP_MISC: u32 = 0x10;
}

/// Control register bits
pub mod control {
    /// IOMMU Enable
    pub const IOMMU_EN: u64 = 1 << 0;
    /// HT Tunnel Translation Enable
    pub const HT_TUN_EN: u64 = 1 << 1;
    /// Event Log Enable
    pub const EVT_LOG_EN: u64 = 1 << 2;
    /// Event Interrupt Enable
    pub const EVT_INT_EN: u64 = 1 << 3;
    /// Completion Wait Interrupt Enable
    pub const COM_WAIT_INT_EN: u64 = 1 << 4;
    /// Invalidation Timeout (bits 5-7)
    pub const INV_TIMEOUT_MASK: u64 = 0x07 << 5;
    /// Pass Posted Write
    pub const PASS_PW: u64 = 1 << 8;
    /// Response Pass Posted Write
    pub const RES_PASS_PW: u64 = 1 << 9;
    /// Coherent
    pub const COHERENT: u64 = 1 << 10;
    /// Isochronous
    pub const ISOC: u64 = 1 << 11;
    /// Command Buffer Enable
    pub const CMD_BUF_EN: u64 = 1 << 12;
    /// PPR Log Enable
    pub const PPR_LOG_EN: u64 = 1 << 13;
    /// PPR Interrupt Enable
    pub const PPR_INT_EN: u64 = 1 << 14;
    /// PPR Enable
    pub const PPR_EN: u64 = 1 << 15;
    /// Guest Translation Enable
    pub const GT_EN: u64 = 1 << 16;
    /// Guest APIC Enable
    pub const GA_EN: u64 = 1 << 17;
    /// SMI Filter Enable
    pub const SMIF_EN: u64 = 1 << 22;
    /// Self Write Back Disable
    pub const SELF_WB_DIS: u64 = 1 << 23;
    /// SMI Filter Log Enable
    pub const SMIF_LOG_EN: u64 = 1 << 24;
    /// GA Update Disable
    pub const GA_UPDATE_DIS: u64 = 1 << 28;
    /// GA Log Enable
    pub const GA_LOG_EN: u64 = 1 << 29;
    /// GA Interrupt Enable
    pub const GA_INT_EN: u64 = 1 << 30;
    /// Dual PPR Log Enable
    pub const DUAL_PPR_LOG_EN: u64 = 1 << 32;
    /// Dual Event Log Enable
    pub const DUAL_EVT_LOG_EN: u64 = 1 << 33;
    /// Device Table Segmentation Enable
    pub const DEV_TAB_SEG_EN_MASK: u64 = 0x07 << 36;
    /// Privilege Abort Enable
    pub const PRIV_ABORT_EN: u64 = 1 << 39;
    /// PPR Auto Response Enable
    pub const PPR_AUTO_RSP_EN: u64 = 1 << 40;
    /// MARC Enable
    pub const MARC_EN: u64 = 1 << 41;
    /// Block StopMark messages Enable
    pub const BLK_STOP_MRK_EN: u64 = 1 << 42;
    /// PPR Auto Response Always On Enable
    pub const PPR_AUTO_RSP_AON: u64 = 1 << 43;
    /// EPH Enable
    pub const EPH_EN: u64 = 1 << 45;
    /// HAdisable
    pub const HA_DIS: u64 = 1 << 46;
    /// Guest IO Protection Enable
    pub const GIO_PROT_EN: u64 = 1 << 48;
    /// Extended Feature Enable
    pub const XT_EN: u64 = 1 << 50;
    /// Interrupt Remap Enable
    pub const INT_MAP_EN: u64 = 1 << 51;
    /// Virtual CID Enable
    pub const VCID_EN: u64 = 1 << 52;
}

/// Status register bits
pub mod status {
    /// Event Overflow
    pub const EVT_OF: u64 = 1 << 0;
    /// Event Log Interrupt
    pub const EVT_LOG_INT: u64 = 1 << 1;
    /// Completion Wait Interrupt
    pub const COM_WAIT_INT: u64 = 1 << 2;
    /// Event Log Running
    pub const EVT_LOG_RUN: u64 = 1 << 3;
    /// Command Buffer Running
    pub const CMD_BUF_RUN: u64 = 1 << 4;
    /// PPR Overflow
    pub const PPR_OF: u64 = 1 << 5;
    /// PPR Interrupt
    pub const PPR_INT: u64 = 1 << 6;
    /// PPR Log Running
    pub const PPR_LOG_RUN: u64 = 1 << 7;
    /// GA Log Running
    pub const GA_LOG_RUN: u64 = 1 << 8;
    /// GA Log Overflow
    pub const GA_LOG_OF: u64 = 1 << 9;
    /// GA Log Interrupt
    pub const GA_LOG_INT: u64 = 1 << 10;
    /// PPR Log B Overflow
    pub const PPR_LOG_B_OF: u64 = 1 << 11;
    /// PPR Log Active
    pub const PPR_LOG_ACT: u64 = 1 << 12;
    /// Event Log B Overflow
    pub const EVT_LOG_B_OF: u64 = 1 << 15;
    /// Event Log Active
    pub const EVT_LOG_ACT: u64 = 1 << 16;
    /// PPR Log B Overflow Early Warning
    pub const PPR_OF_B_EW: u64 = 1 << 17;
    /// PPR Log Overflow Early Warning
    pub const PPR_OF_EW: u64 = 1 << 18;
}

/// Device Table Entry (DTE)
#[derive(Debug, Clone, Copy)]
pub struct DeviceTableEntry {
    /// Data words (4 x 64-bit)
    data: [u64; 4],
}

impl DeviceTableEntry {
    // DTE flags (first quadword)
    const VALID: u64 = 1 << 0;
    const TV: u64 = 1 << 1; // Translation valid
    const INT_TAB_LEN_MASK: u64 = 0x0F << 36;
    const INT_TAB_LEN_SHIFT: u64 = 36;
    const IG: u64 = 1 << 40; // Ignore unmapped interrupts
    const INT_VALID: u64 = 1 << 47; // Interrupt table valid
    const INT_TAB_ROOT_MASK: u64 = 0x000F_FFFF_FFFF_F800;
    const IO_CTL_MASK: u64 = 0x03 << 48;
    const IO_CTL_SHIFT: u64 = 48;
    const SA: u64 = 1 << 50; // Suppress all I/O page faults
    const SE: u64 = 1 << 51; // Suppress all event log
    const SD: u64 = 1 << 52; // Supress all device-IOTLB
    const PAGE_TAB_ROOT_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    const HAD_MASK: u64 = 0x03 << 54;
    const HAD_SHIFT: u64 = 54;
    const PAGE_MODE_MASK: u64 = 0x07 << 57;
    const PAGE_MODE_SHIFT: u64 = 57;
    const GV: u64 = 1 << 62; // Guest translation valid
    const GLX_MASK: u64 = 0x03 << 62;
    const GLX_SHIFT: u64 = 62;

    // DTE flags (second quadword)
    const DOMAIN_ID_MASK: u64 = 0xFFFF;
    const GCPTR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    const I: u64 = 1 << 61; // IOTLB enable
    const EX: u64 = 1 << 62; // Allow exclusion
    const SYS_MGT_MASK: u64 = 0x03 << 16;

    /// Create empty/invalid entry
    pub const fn empty() -> Self {
        Self { data: [0; 4] }
    }

    /// Create entry for identity mapping (passthrough)
    pub fn identity(domain_id: DomainId) -> Self {
        let mut dte = Self::empty();
        // Set valid, no translation (passthrough mode)
        dte.data[0] = Self::VALID;
        dte.data[1] = (domain_id.0 as u64) & Self::DOMAIN_ID_MASK;
        dte
    }

    /// Create entry for translated device
    pub fn translated(
        page_table_addr: u64,
        domain_id: DomainId,
        address_width: AddressWidth,
    ) -> Self {
        let mut dte = Self::empty();
        let mode = match address_width {
            AddressWidth::Bits30 => 3,
            AddressWidth::Bits39 => 4,
            AddressWidth::Bits48 => 5,
            AddressWidth::Bits57 => 6,
        };
        dte.data[0] = Self::VALID
            | Self::TV
            | (page_table_addr & Self::PAGE_TAB_ROOT_MASK)
            | ((mode as u64) << Self::PAGE_MODE_SHIFT);
        dte.data[1] = (domain_id.0 as u64) & Self::DOMAIN_ID_MASK;
        dte
    }

    /// Check if entry is valid
    pub const fn is_valid(&self) -> bool {
        (self.data[0] & Self::VALID) != 0
    }

    /// Check if translation is enabled
    pub const fn is_translation_enabled(&self) -> bool {
        (self.data[0] & Self::TV) != 0
    }

    /// Get translation type
    pub fn translation_type(&self) -> TranslationType {
        if !self.is_valid() {
            TranslationType::Reserved
        } else if self.is_translation_enabled() {
            TranslationType::Translated
        } else {
            TranslationType::Identity
        }
    }

    /// Get page table root address
    pub const fn page_table_addr(&self) -> u64 {
        self.data[0] & Self::PAGE_TAB_ROOT_MASK
    }

    /// Get domain ID
    pub fn domain_id(&self) -> DomainId {
        DomainId::new((self.data[1] & Self::DOMAIN_ID_MASK) as u16)
    }

    /// Get page mode (address width levels)
    pub fn page_mode(&self) -> u8 {
        ((self.data[0] & Self::PAGE_MODE_MASK) >> Self::PAGE_MODE_SHIFT) as u8
    }

    /// Get address width
    pub fn address_width(&self) -> Option<AddressWidth> {
        match self.page_mode() {
            3 => Some(AddressWidth::Bits30),
            4 => Some(AddressWidth::Bits39),
            5 => Some(AddressWidth::Bits48),
            6 => Some(AddressWidth::Bits57),
            _ => None,
        }
    }

    /// Check if interrupt remapping is enabled
    pub const fn is_interrupt_valid(&self) -> bool {
        (self.data[0] & Self::INT_VALID) != 0
    }

    /// Get interrupt table address
    pub const fn interrupt_table_addr(&self) -> u64 {
        self.data[0] & Self::INT_TAB_ROOT_MASK
    }

    /// Set interrupt table
    pub fn set_interrupt_table(&mut self, addr: u64, len: u8) {
        self.data[0] = (self.data[0] & !Self::INT_TAB_ROOT_MASK & !Self::INT_TAB_LEN_MASK)
            | (addr & Self::INT_TAB_ROOT_MASK)
            | (((len as u64) & 0x0F) << Self::INT_TAB_LEN_SHIFT)
            | Self::INT_VALID;
    }

    /// Get raw data
    pub const fn raw(&self) -> &[u64; 4] {
        &self.data
    }
}

/// Event log entry types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventType {
    /// Illegal Device Table Entry
    IllegalDte = 0x01,
    /// IO Page Fault
    IoPageFault = 0x02,
    /// Device Table Hardware Error
    DevTabHwError = 0x03,
    /// Page Table Hardware Error
    PageTabHwError = 0x04,
    /// Illegal Command Error
    IllegalCmdError = 0x05,
    /// Command Hardware Error
    CmdHwError = 0x06,
    /// IOTLB Invalidation Timeout
    IotlbInvTimeout = 0x07,
    /// Invalid Device Request
    InvalidDevRequest = 0x08,
    /// Invalid PPR Request
    InvalidPprRequest = 0x09,
    /// Event Counter Zero
    EventCounterZero = 0x10,
}

impl EventType {
    /// Convert from raw value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(EventType::IllegalDte),
            0x02 => Some(EventType::IoPageFault),
            0x03 => Some(EventType::DevTabHwError),
            0x04 => Some(EventType::PageTabHwError),
            0x05 => Some(EventType::IllegalCmdError),
            0x06 => Some(EventType::CmdHwError),
            0x07 => Some(EventType::IotlbInvTimeout),
            0x08 => Some(EventType::InvalidDevRequest),
            0x09 => Some(EventType::InvalidPprRequest),
            0x10 => Some(EventType::EventCounterZero),
            _ => None,
        }
    }
}

/// Event log entry
#[derive(Debug, Clone)]
pub struct EventLogEntry {
    /// Event type
    pub event_type: EventType,
    /// Device ID
    pub device_id: DeviceId,
    /// Domain ID
    pub domain_id: DomainId,
    /// Flags
    pub flags: u16,
    /// Address
    pub address: u64,
}

impl EventLogEntry {
    /// Create new event log entry
    pub fn new(
        event_type: EventType,
        device_id: DeviceId,
        domain_id: DomainId,
        address: u64,
    ) -> Self {
        Self {
            event_type,
            device_id,
            domain_id,
            flags: 0,
            address,
        }
    }

    /// Encode to bytes (16 bytes)
    pub fn encode(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];

        // Word 0: Device ID, Event Type
        let word0 = (self.device_id.source_id() as u64)
            | ((self.event_type as u64) << 28)
            | ((self.flags as u64) << 44);
        bytes[0..8].copy_from_slice(&word0.to_le_bytes());

        // Word 1: Domain ID, Address
        let word1 = (self.domain_id.0 as u64) | (self.address & 0xFFFF_FFFF_FFFF_0000);
        bytes[8..16].copy_from_slice(&word1.to_le_bytes());

        bytes
    }
}

/// Command buffer entry types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommandType {
    /// Completion Wait
    CompletionWait = 0x01,
    /// Invalidate Device Table Entry
    InvalidateDevTab = 0x02,
    /// Invalidate IOMMU Pages
    InvalidatePages = 0x03,
    /// Invalidate IOTLB
    InvalidateIotlb = 0x04,
    /// Invalidate Interrupt Table
    InvalidateIntTab = 0x05,
    /// Prefetch IOMMU Pages
    PrefetchPages = 0x06,
    /// Complete PPR
    CompletePpr = 0x07,
    /// Invalidate All
    InvalidateAll = 0x08,
}

/// Command buffer entry
#[derive(Debug, Clone, Copy)]
pub struct CommandEntry {
    /// Data words (2 x 64-bit)
    data: [u64; 2],
}

impl CommandEntry {
    /// Create completion wait command
    pub fn completion_wait(address: u64, data: u64) -> Self {
        Self {
            data: [
                (CommandType::CompletionWait as u64) << 60 | (1 << 0) | (address >> 3),
                data,
            ],
        }
    }

    /// Create invalidate device table entry command
    pub fn invalidate_device_table(device_id: DeviceId) -> Self {
        Self {
            data: [
                (CommandType::InvalidateDevTab as u64) << 60 | (device_id.source_id() as u64),
                0,
            ],
        }
    }

    /// Create invalidate pages command
    pub fn invalidate_pages(domain_id: DomainId, address: u64, size: bool) -> Self {
        Self {
            data: [
                (CommandType::InvalidatePages as u64) << 60
                    | (domain_id.0 as u64)
                    | if size { 1 << 16 } else { 0 },
                address & !0xFFF,
            ],
        }
    }

    /// Create invalidate IOTLB command
    pub fn invalidate_iotlb(device_id: DeviceId, domain_id: DomainId, address: u64) -> Self {
        Self {
            data: [
                (CommandType::InvalidateIotlb as u64) << 60
                    | (device_id.source_id() as u64)
                    | ((domain_id.0 as u64) << 16),
                address & !0xFFF,
            ],
        }
    }

    /// Create invalidate all command
    pub fn invalidate_all() -> Self {
        Self {
            data: [(CommandType::InvalidateAll as u64) << 60, 0],
        }
    }

    /// Get command type
    pub fn command_type(&self) -> Option<CommandType> {
        match (self.data[0] >> 60) as u8 {
            0x01 => Some(CommandType::CompletionWait),
            0x02 => Some(CommandType::InvalidateDevTab),
            0x03 => Some(CommandType::InvalidatePages),
            0x04 => Some(CommandType::InvalidateIotlb),
            0x05 => Some(CommandType::InvalidateIntTab),
            0x06 => Some(CommandType::PrefetchPages),
            0x07 => Some(CommandType::CompletePpr),
            0x08 => Some(CommandType::InvalidateAll),
            _ => None,
        }
    }

    /// Get raw data
    pub const fn raw(&self) -> &[u64; 2] {
        &self.data
    }
}

/// IOTLB entry for AMD IOMMU
#[derive(Debug, Clone)]
pub struct AmdIotlbEntry {
    /// Device ID
    pub device: DeviceId,
    /// Domain ID
    pub domain: DomainId,
    /// Virtual address (page aligned)
    pub iova: u64,
    /// Physical address (page aligned)
    pub hpa: u64,
    /// Page size
    pub page_size: u64,
    /// Permissions
    pub flags: PageTableFlags,
}

impl AmdIotlbEntry {
    /// Create new IOTLB entry
    pub fn new(
        device: DeviceId,
        domain: DomainId,
        iova: u64,
        hpa: u64,
        page_size: u64,
        flags: PageTableFlags,
    ) -> Self {
        Self {
            device,
            domain,
            iova: iova & !(page_size - 1),
            hpa: hpa & !(page_size - 1),
            page_size,
            flags,
        }
    }

    /// Check if matches lookup
    pub fn matches(&self, device: &DeviceId, domain: DomainId, iova: u64) -> bool {
        self.device == *device
            && self.domain == domain
            && iova >= self.iova
            && iova < self.iova + self.page_size
    }

    /// Translate address
    pub fn translate(&self, iova: u64) -> u64 {
        self.hpa + (iova - self.iova)
    }
}

/// AMD IOMMU Unit
pub struct AmdIommu {
    /// Base MMIO address
    base_address: u64,
    /// PCI segment
    segment: u16,
    /// Device table base address
    device_table_base: u64,
    /// Device table (64K entries max)
    device_table: HashMap<u16, DeviceTableEntry>,
    /// Command buffer
    command_buffer: Vec<CommandEntry>,
    /// Command buffer head
    cmd_head: u32,
    /// Command buffer tail
    cmd_tail: u32,
    /// Event log
    event_log: Vec<EventLogEntry>,
    /// Control register
    control: u64,
    /// Status register
    status: u64,
    /// IOMMU enabled
    enabled: AtomicBool,
    /// IOTLB cache
    iotlb: HashMap<(u16, u16, u64), AmdIotlbEntry>,
    /// Statistics
    stats: IommuStats,
    /// Address width
    address_width: AddressWidth,
}

impl Default for AmdIommu {
    fn default() -> Self {
        Self::new(0xFEB80000, 0)
    }
}

impl AmdIommu {
    /// Create new AMD IOMMU
    pub fn new(base_address: u64, segment: u16) -> Self {
        Self {
            base_address,
            segment,
            device_table_base: 0,
            device_table: HashMap::new(),
            command_buffer: Vec::with_capacity(256),
            cmd_head: 0,
            cmd_tail: 0,
            event_log: Vec::new(),
            control: 0,
            status: 0,
            enabled: AtomicBool::new(false),
            iotlb: HashMap::new(),
            stats: IommuStats::default(),
            address_width: AddressWidth::Bits48,
        }
    }

    /// Get base address
    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Get segment
    pub fn segment(&self) -> u16 {
        self.segment
    }

    /// Get statistics
    pub fn stats(&self) -> &IommuStats {
        &self.stats
    }

    /// Read register
    pub fn read_register(&self, offset: u32) -> u64 {
        match offset {
            registers::DEV_TAB_BASE => self.device_table_base,
            registers::CONTROL => self.control,
            registers::STATUS => self.status,
            registers::CMD_BUF_HEAD => self.cmd_head as u64,
            registers::CMD_BUF_TAIL => self.cmd_tail as u64,
            registers::EXT_FEAT => {
                // Extended features
                0x0000_0000_0004_0003 // Basic features
            }
            _ => 0,
        }
    }

    /// Write register
    pub fn write_register(&mut self, offset: u32, value: u64) {
        match offset {
            registers::DEV_TAB_BASE => {
                self.device_table_base = value & 0xFFFF_FFFF_FFFF_F000;
            }
            registers::CONTROL => {
                self.control = value;
                if value & control::IOMMU_EN != 0 {
                    self.enable();
                } else {
                    self.disable();
                }
            }
            registers::STATUS => {
                // Write-1-to-clear bits
                self.status &= !value;
            }
            registers::CMD_BUF_TAIL => {
                self.cmd_tail = value as u32;
                self.process_commands();
            }
            _ => {}
        }
    }

    /// Enable IOMMU
    pub fn enable(&mut self) {
        self.enabled.store(true, Ordering::SeqCst);
        self.status |= status::CMD_BUF_RUN | status::EVT_LOG_RUN;
    }

    /// Disable IOMMU
    pub fn disable(&mut self) {
        self.enabled.store(false, Ordering::SeqCst);
        self.status &= !(status::CMD_BUF_RUN | status::EVT_LOG_RUN);
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Set device table entry
    pub fn set_device_entry(&mut self, device_id: u16, entry: DeviceTableEntry) {
        self.device_table.insert(device_id, entry);
    }

    /// Get device table entry
    pub fn device_entry(&self, device_id: u16) -> Option<&DeviceTableEntry> {
        self.device_table.get(&device_id)
    }

    /// Configure device for passthrough
    pub fn configure_passthrough(&mut self, device: &DeviceId, domain: DomainId) {
        let entry = DeviceTableEntry::identity(domain);
        self.set_device_entry(device.source_id(), entry);
    }

    /// Configure device for translation
    pub fn configure_translation(
        &mut self,
        device: &DeviceId,
        domain: DomainId,
        page_table_addr: u64,
    ) {
        let entry = DeviceTableEntry::translated(page_table_addr, domain, self.address_width);
        self.set_device_entry(device.source_id(), entry);
    }

    /// Process command buffer
    fn process_commands(&mut self) {
        while self.cmd_head != self.cmd_tail {
            if let Some(cmd) = self.command_buffer.get(self.cmd_head as usize) {
                match cmd.command_type() {
                    Some(CommandType::InvalidateAll) => {
                        self.iotlb.clear();
                        self.stats.record_iotlb_invalidation();
                    }
                    Some(CommandType::InvalidateIotlb) => {
                        // Invalidate specific entries
                        self.stats.record_iotlb_invalidation();
                    }
                    Some(CommandType::InvalidateDevTab) => {
                        self.stats.record_context_invalidation();
                    }
                    _ => {}
                }
            }
            self.cmd_head = (self.cmd_head + 1) % self.command_buffer.capacity() as u32;
        }
    }

    /// Translate DMA address
    pub fn translate(
        &self,
        device: &DeviceId,
        iova: u64,
        is_write: bool,
    ) -> Result<u64, FaultRecord> {
        if !self.is_enabled() {
            return Ok(iova); // Pass-through when disabled
        }

        // Get device table entry
        let dte = match self.device_entry(device.source_id()) {
            Some(dte) if dte.is_valid() => dte,
            _ => {
                self.stats.record_fault();
                return Err(FaultRecord::new(
                    *device,
                    iova,
                    FaultReason::ContextNotPresent,
                    is_write,
                ));
            }
        };

        match dte.translation_type() {
            TranslationType::Identity => {
                self.stats.record_translation(false);
                Ok(iova)
            }
            TranslationType::Translated => {
                // Check IOTLB
                let key = (device.source_id(), dte.domain_id().0, iova >> 12);
                if let Some(entry) = self.iotlb.get(&key) {
                    if entry.matches(device, dte.domain_id(), iova) {
                        if is_write && !entry.flags.is_writable() {
                            self.stats.record_fault();
                            return Err(FaultRecord::new(
                                *device,
                                iova,
                                FaultReason::WriteBlocked,
                                is_write,
                            ));
                        }
                        self.stats.record_translation(true);
                        return Ok(entry.translate(iova));
                    }
                }

                // Would walk page tables here
                self.stats.record_translation(false);
                self.stats.record_page_walk();

                Err(FaultRecord::new(
                    *device,
                    iova,
                    FaultReason::PageNotPresent,
                    is_write,
                ))
            }
            TranslationType::Reserved => {
                self.stats.record_fault();
                Err(FaultRecord::new(
                    *device,
                    iova,
                    FaultReason::InvalidContextEntry,
                    is_write,
                ))
            }
        }
    }

    /// Log event
    pub fn log_event(&mut self, entry: EventLogEntry) {
        self.event_log.push(entry);
        self.status |= status::EVT_LOG_INT;
    }

    /// Get event log
    pub fn event_log(&self) -> &[EventLogEntry] {
        &self.event_log
    }

    /// Clear event log
    pub fn clear_event_log(&mut self) {
        self.event_log.clear();
    }

    /// Invalidate IOTLB
    pub fn invalidate_iotlb(&mut self) {
        self.iotlb.clear();
        self.stats.record_iotlb_invalidation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_table_entry_empty() {
        let dte = DeviceTableEntry::empty();
        assert!(!dte.is_valid());
    }

    #[test]
    fn test_device_table_entry_identity() {
        let dte = DeviceTableEntry::identity(DomainId::new(1));
        assert!(dte.is_valid());
        assert!(!dte.is_translation_enabled());
        assert_eq!(dte.translation_type(), TranslationType::Identity);
    }

    #[test]
    fn test_device_table_entry_translated() {
        let dte = DeviceTableEntry::translated(0x1000_0000, DomainId::new(5), AddressWidth::Bits48);
        assert!(dte.is_valid());
        assert!(dte.is_translation_enabled());
        assert_eq!(dte.translation_type(), TranslationType::Translated);
        assert_eq!(dte.page_table_addr(), 0x1000_0000);
    }

    #[test]
    fn test_event_log_entry() {
        let device = DeviceId::new(0, 1, 2, 0);
        let entry = EventLogEntry::new(EventType::IoPageFault, device, DomainId::new(1), 0x1000);
        assert_eq!(entry.event_type, EventType::IoPageFault);
    }

    #[test]
    fn test_event_log_entry_encode() {
        let device = DeviceId::new(0, 1, 2, 0);
        let entry = EventLogEntry::new(EventType::IoPageFault, device, DomainId::new(1), 0x1000);
        let bytes = entry.encode();
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn test_command_entry() {
        let cmd = CommandEntry::invalidate_all();
        assert_eq!(cmd.command_type(), Some(CommandType::InvalidateAll));
    }

    #[test]
    fn test_command_completion_wait() {
        let cmd = CommandEntry::completion_wait(0x1000, 0xDEAD_BEEF);
        assert_eq!(cmd.command_type(), Some(CommandType::CompletionWait));
    }

    #[test]
    fn test_amd_iotlb_entry() {
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);
        let entry = AmdIotlbEntry::new(
            device,
            domain,
            0x1000,
            0x5000,
            PAGE_SIZE_4K,
            PageTableFlags::read_write(),
        );

        assert!(entry.matches(&device, domain, 0x1000));
        assert_eq!(entry.translate(0x1500), 0x5500);
    }

    #[test]
    fn test_amd_iommu_creation() {
        let iommu = AmdIommu::new(0xFEB80000, 0);
        assert_eq!(iommu.base_address(), 0xFEB80000);
        assert!(!iommu.is_enabled());
    }

    #[test]
    fn test_amd_iommu_enable_disable() {
        let mut iommu = AmdIommu::default();
        assert!(!iommu.is_enabled());

        iommu.enable();
        assert!(iommu.is_enabled());

        iommu.disable();
        assert!(!iommu.is_enabled());
    }

    #[test]
    fn test_amd_iommu_configure_passthrough() {
        let mut iommu = AmdIommu::default();
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        iommu.configure_passthrough(&device, domain);

        let dte = iommu.device_entry(device.source_id()).unwrap();
        assert!(dte.is_valid());
        assert_eq!(dte.translation_type(), TranslationType::Identity);
    }

    #[test]
    fn test_amd_iommu_translate_passthrough() {
        let mut iommu = AmdIommu::default();
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        iommu.configure_passthrough(&device, domain);
        iommu.enable();

        let result = iommu.translate(&device, 0x1000, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0x1000);
    }

    #[test]
    fn test_amd_iommu_translate_disabled() {
        let iommu = AmdIommu::default();
        let device = DeviceId::new(0, 1, 2, 0);

        let result = iommu.translate(&device, 0xDEAD_BEEF, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn test_amd_iommu_registers() {
        let iommu = AmdIommu::default();
        let ext_feat = iommu.read_register(registers::EXT_FEAT);
        assert!(ext_feat != 0);
    }

    #[test]
    fn test_amd_iommu_write_control() {
        let mut iommu = AmdIommu::default();
        iommu.write_register(registers::CONTROL, control::IOMMU_EN);
        assert!(iommu.is_enabled());
    }

    #[test]
    fn test_amd_iommu_statistics() {
        let mut iommu = AmdIommu::default();
        let device = DeviceId::new(0, 1, 2, 0);
        let domain = DomainId::new(1);

        iommu.configure_passthrough(&device, domain);
        iommu.enable();
        let _ = iommu.translate(&device, 0x1000, false);

        let stats = iommu.stats().snapshot();
        assert!(stats.translations > 0);
    }
}
