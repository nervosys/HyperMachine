//! PCI Capabilities
//!
//! This module provides PCI capability structures including
//! MSI, MSI-X, Power Management, PCIe, and AER capabilities.

use super::config::{ConfigSpace, PCIE_CONFIG_SIZE};
use std::sync::atomic::{AtomicU64, Ordering};

/// PCI Capability IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CapabilityId {
    /// Power Management
    PowerManagement = 0x01,
    /// AGP
    Agp = 0x02,
    /// Vital Product Data
    Vpd = 0x03,
    /// Slot Identification
    SlotId = 0x04,
    /// MSI
    Msi = 0x05,
    /// CompactPCI Hot Swap
    CompactPciHotSwap = 0x06,
    /// PCI-X
    PciX = 0x07,
    /// HyperTransport
    HyperTransport = 0x08,
    /// Vendor Specific
    VendorSpecific = 0x09,
    /// Debug Port
    DebugPort = 0x0A,
    /// CompactPCI Resource Control
    CompactPciControl = 0x0B,
    /// PCI Hot-Plug
    PciHotPlug = 0x0C,
    /// PCI Bridge Subsystem Vendor ID
    BridgeSubsystem = 0x0D,
    /// AGP 8x
    Agp8x = 0x0E,
    /// Secure Device
    SecureDevice = 0x0F,
    /// PCI Express
    PciExpress = 0x10,
    /// MSI-X
    MsiX = 0x11,
    /// SATA
    Sata = 0x12,
    /// Advanced Features
    AdvancedFeatures = 0x13,
    /// Enhanced Allocation
    EnhancedAllocation = 0x14,
    /// Flattening Portal Bridge
    FpbBridge = 0x15,
}

impl CapabilityId {
    /// Parse from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(CapabilityId::PowerManagement),
            0x02 => Some(CapabilityId::Agp),
            0x03 => Some(CapabilityId::Vpd),
            0x04 => Some(CapabilityId::SlotId),
            0x05 => Some(CapabilityId::Msi),
            0x06 => Some(CapabilityId::CompactPciHotSwap),
            0x07 => Some(CapabilityId::PciX),
            0x08 => Some(CapabilityId::HyperTransport),
            0x09 => Some(CapabilityId::VendorSpecific),
            0x0A => Some(CapabilityId::DebugPort),
            0x0B => Some(CapabilityId::CompactPciControl),
            0x0C => Some(CapabilityId::PciHotPlug),
            0x0D => Some(CapabilityId::BridgeSubsystem),
            0x0E => Some(CapabilityId::Agp8x),
            0x0F => Some(CapabilityId::SecureDevice),
            0x10 => Some(CapabilityId::PciExpress),
            0x11 => Some(CapabilityId::MsiX),
            0x12 => Some(CapabilityId::Sata),
            0x13 => Some(CapabilityId::AdvancedFeatures),
            0x14 => Some(CapabilityId::EnhancedAllocation),
            0x15 => Some(CapabilityId::FpbBridge),
            _ => None,
        }
    }
}

/// PCIe Extended Capability IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ExtendedCapabilityId {
    /// Advanced Error Reporting
    Aer = 0x0001,
    /// Virtual Channel
    VirtualChannel = 0x0002,
    /// Device Serial Number
    DeviceSerialNumber = 0x0003,
    /// Power Budgeting
    PowerBudgeting = 0x0004,
    /// Root Complex Link Declaration
    RcLinkDeclaration = 0x0005,
    /// Root Complex Internal Link Control
    RcInternalLinkControl = 0x0006,
    /// Root Complex Event Collector Endpoint Association
    RcEventCollector = 0x0007,
    /// Multi-Function Virtual Channel
    MfvcVirtualChannel = 0x0008,
    /// Virtual Channel (MFVC)
    VirtualChannelMfvc = 0x0009,
    /// Root Complex Register Block
    RcRegisterBlock = 0x000A,
    /// Vendor-Specific Extended Capability
    VendorSpecific = 0x000B,
    /// Configuration Access Correlation
    ConfigAccessCorrelation = 0x000C,
    /// Access Control Services
    Acs = 0x000D,
    /// Alternative Routing-ID Interpretation
    Ari = 0x000E,
    /// Address Translation Services
    Ats = 0x000F,
    /// Single Root I/O Virtualization
    Sriov = 0x0010,
    /// Multi-Root I/O Virtualization
    Mriov = 0x0011,
    /// Multicast
    Multicast = 0x0012,
    /// Page Request Interface
    Pri = 0x0013,
    /// Resizable BAR
    ResizableBar = 0x0015,
    /// Dynamic Power Allocation
    Dpa = 0x0016,
    /// TPH Requester
    TphRequester = 0x0017,
    /// Latency Tolerance Reporting
    Ltr = 0x0018,
    /// Secondary PCI Express
    SecondaryPcie = 0x0019,
    /// Protocol Multiplexing
    Pmux = 0x001A,
    /// Process Address Space ID
    Pasid = 0x001B,
    /// LN Requester
    LnRequester = 0x001C,
    /// Downstream Port Containment
    Dpc = 0x001D,
    /// L1 PM Substates
    L1PmSubstates = 0x001E,
    /// Precision Time Measurement
    Ptm = 0x001F,
    /// PCI Express over M-PHY
    MPhyPcie = 0x0020,
    /// FRS Queueing
    FrsQueueing = 0x0021,
    /// Readiness Time Reporting
    ReadinessTime = 0x0022,
    /// Designated Vendor-Specific
    DesignatedVendor = 0x0023,
    /// VF Resizable BAR
    VfResizableBar = 0x0024,
    /// Data Link Feature
    DataLinkFeature = 0x0025,
    /// Physical Layer 16.0 GT/s
    PhysicalLayer16 = 0x0026,
    /// Lane Margining at Receiver
    LaneMargining = 0x0027,
    /// Hierarchy ID
    HierarchyId = 0x0028,
    /// Native PCIe Enclosure Management
    Npem = 0x0029,
    /// Physical Layer 32.0 GT/s
    PhysicalLayer32 = 0x002A,
    /// Alternate Protocol
    AlternateProtocol = 0x002B,
    /// System Firmware Intermediary
    Sfi = 0x002C,
}

/// Capability header
#[derive(Debug, Clone, Copy)]
pub struct CapabilityHeader {
    /// Capability ID
    pub id: u8,
    /// Next capability pointer
    pub next: u8,
}

impl CapabilityHeader {
    /// Create new capability header
    pub fn new(id: CapabilityId, next: u8) -> Self {
        Self {
            id: id as u8,
            next,
        }
    }

    /// Parse from config space
    pub fn from_config(config: &ConfigSpace, offset: u8) -> Option<Self> {
        if offset == 0 || offset as usize >= PCIE_CONFIG_SIZE - 1 {
            return None;
        }
        Some(Self {
            id: config.read_u8(offset as u16),
            next: config.read_u8(offset as u16 + 1),
        })
    }

    /// Get capability ID
    pub fn capability_id(&self) -> Option<CapabilityId> {
        CapabilityId::from_u8(self.id)
    }

    /// Write to config space
    pub fn write_to_config(&self, config: &mut ConfigSpace, offset: u8) {
        config.data_mut()[offset as usize] = self.id;
        config.data_mut()[offset as usize + 1] = self.next;
    }
}

/// MSI Message Control
#[derive(Debug, Clone, Copy, Default)]
pub struct MsiControl(pub u16);

impl MsiControl {
    /// MSI Enable
    pub const ENABLE: u16 = 1 << 0;
    /// Multiple Message Capable (3 bits)
    pub const MMC_MASK: u16 = 0x07 << 1;
    /// Multiple Message Enable (3 bits)
    pub const MME_MASK: u16 = 0x07 << 4;
    /// 64-bit Address Capable
    pub const ADDR_64: u16 = 1 << 7;
    /// Per-Vector Masking Capable
    pub const PER_VECTOR_MASKING: u16 = 1 << 8;

    /// Create new MSI control
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    /// Check if MSI is enabled
    pub fn enabled(&self) -> bool {
        self.0 & Self::ENABLE != 0
    }

    /// Enable MSI
    pub fn enable(&mut self) {
        self.0 |= Self::ENABLE;
    }

    /// Disable MSI
    pub fn disable(&mut self) {
        self.0 &= !Self::ENABLE;
    }

    /// Get multiple message capable count (log2)
    pub fn multiple_message_capable(&self) -> u8 {
        ((self.0 & Self::MMC_MASK) >> 1) as u8
    }

    /// Get multiple message enable count (log2)
    pub fn multiple_message_enable(&self) -> u8 {
        ((self.0 & Self::MME_MASK) >> 4) as u8
    }

    /// Set multiple message enable
    pub fn set_multiple_message_enable(&mut self, count: u8) {
        let count = count.min(self.multiple_message_capable());
        self.0 = (self.0 & !Self::MME_MASK) | ((count as u16) << 4);
    }

    /// Check if 64-bit capable
    pub fn is_64bit(&self) -> bool {
        self.0 & Self::ADDR_64 != 0
    }

    /// Check if per-vector masking capable
    pub fn per_vector_masking(&self) -> bool {
        self.0 & Self::PER_VECTOR_MASKING != 0
    }

    /// Get vector count
    pub fn vector_count(&self) -> u32 {
        1 << self.multiple_message_enable()
    }
}

/// MSI Capability
#[derive(Debug, Clone)]
pub struct MsiCapability {
    /// Offset in config space
    pub offset: u8,
    /// Message Control
    pub control: MsiControl,
    /// Message Address (lower 32 bits)
    pub address_lo: u32,
    /// Message Address (upper 32 bits, if 64-bit capable)
    pub address_hi: u32,
    /// Message Data
    pub data: u16,
    /// Mask bits (if per-vector masking)
    pub mask: u32,
    /// Pending bits (if per-vector masking)
    pub pending: u32,
}

impl MsiCapability {
    /// Create new MSI capability
    pub fn new(offset: u8, vectors: u8, is_64bit: bool, per_vector_masking: bool) -> Self {
        let vectors_log2 = (vectors.max(1) - 1).count_ones() as u8;
        let mut control = MsiControl(0);
        control.0 |= (vectors_log2 as u16) << 1; // MMC
        if is_64bit {
            control.0 |= MsiControl::ADDR_64;
        }
        if per_vector_masking {
            control.0 |= MsiControl::PER_VECTOR_MASKING;
        }

        Self {
            offset,
            control,
            address_lo: 0,
            address_hi: 0,
            data: 0,
            mask: 0,
            pending: 0,
        }
    }

    /// Get capability size
    pub fn size(&self) -> u8 {
        let mut size = 10; // Header + Control + Address (32-bit) + Data
        if self.control.is_64bit() {
            size += 4; // Upper address
        }
        if self.control.per_vector_masking() {
            size += 8; // Mask + Pending
        }
        size
    }

    /// Write to config space
    pub fn write_to_config(&self, config: &mut ConfigSpace, next: u8) {
        let header = CapabilityHeader::new(CapabilityId::Msi, next);
        header.write_to_config(config, self.offset);

        let data = config.data_mut();
        let offset = self.offset as usize;

        // Control register
        data[offset + 2..offset + 4].copy_from_slice(&self.control.0.to_le_bytes());

        // Address
        data[offset + 4..offset + 8].copy_from_slice(&self.address_lo.to_le_bytes());

        if self.control.is_64bit() {
            data[offset + 8..offset + 12].copy_from_slice(&self.address_hi.to_le_bytes());
            data[offset + 12..offset + 14].copy_from_slice(&self.data.to_le_bytes());

            if self.control.per_vector_masking() {
                data[offset + 16..offset + 20].copy_from_slice(&self.mask.to_le_bytes());
                data[offset + 20..offset + 24].copy_from_slice(&self.pending.to_le_bytes());
            }
        } else {
            data[offset + 8..offset + 10].copy_from_slice(&self.data.to_le_bytes());

            if self.control.per_vector_masking() {
                data[offset + 12..offset + 16].copy_from_slice(&self.mask.to_le_bytes());
                data[offset + 16..offset + 20].copy_from_slice(&self.pending.to_le_bytes());
            }
        }
    }

    /// Get message address
    pub fn address(&self) -> u64 {
        if self.control.is_64bit() {
            ((self.address_hi as u64) << 32) | (self.address_lo as u64)
        } else {
            self.address_lo as u64
        }
    }

    /// Set message address
    pub fn set_address(&mut self, addr: u64) {
        self.address_lo = addr as u32;
        if self.control.is_64bit() {
            self.address_hi = (addr >> 32) as u32;
        }
    }

    /// Check if vector is masked
    pub fn is_masked(&self, vector: u8) -> bool {
        if !self.control.per_vector_masking() {
            return false;
        }
        self.mask & (1 << vector) != 0
    }

    /// Set vector pending
    pub fn set_pending(&mut self, vector: u8) {
        self.pending |= 1 << vector;
    }

    /// Clear vector pending
    pub fn clear_pending(&mut self, vector: u8) {
        self.pending &= !(1 << vector);
    }
}

/// MSI-X Table Entry
#[derive(Debug, Clone, Copy, Default)]
pub struct MsixTableEntry {
    /// Message Address (lower)
    pub address_lo: u32,
    /// Message Address (upper)
    pub address_hi: u32,
    /// Message Data
    pub data: u32,
    /// Vector Control
    pub control: u32,
}

impl MsixTableEntry {
    /// Mask bit in control
    pub const MASK_BIT: u32 = 1 << 0;

    /// Get message address
    pub fn address(&self) -> u64 {
        ((self.address_hi as u64) << 32) | (self.address_lo as u64)
    }

    /// Set message address
    pub fn set_address(&mut self, addr: u64) {
        self.address_lo = addr as u32;
        self.address_hi = (addr >> 32) as u32;
    }

    /// Check if masked
    pub fn is_masked(&self) -> bool {
        self.control & Self::MASK_BIT != 0
    }

    /// Set masked
    pub fn set_masked(&mut self, masked: bool) {
        if masked {
            self.control |= Self::MASK_BIT;
        } else {
            self.control &= !Self::MASK_BIT;
        }
    }
}

/// MSI-X Message Control
#[derive(Debug, Clone, Copy, Default)]
pub struct MsixControl(pub u16);

impl MsixControl {
    /// Table Size (11 bits, actual size is value + 1)
    pub const TABLE_SIZE_MASK: u16 = 0x07FF;
    /// Function Mask
    pub const FUNCTION_MASK: u16 = 1 << 14;
    /// MSI-X Enable
    pub const ENABLE: u16 = 1 << 15;

    /// Create new MSI-X control
    pub fn new(table_size: u16) -> Self {
        Self((table_size.saturating_sub(1)) & Self::TABLE_SIZE_MASK)
    }

    /// Get table size
    pub fn table_size(&self) -> u16 {
        (self.0 & Self::TABLE_SIZE_MASK) + 1
    }

    /// Check if enabled
    pub fn enabled(&self) -> bool {
        self.0 & Self::ENABLE != 0
    }

    /// Enable MSI-X
    pub fn enable(&mut self) {
        self.0 |= Self::ENABLE;
    }

    /// Disable MSI-X
    pub fn disable(&mut self) {
        self.0 &= !Self::ENABLE;
    }

    /// Check if function masked
    pub fn function_masked(&self) -> bool {
        self.0 & Self::FUNCTION_MASK != 0
    }

    /// Set function mask
    pub fn set_function_mask(&mut self, masked: bool) {
        if masked {
            self.0 |= Self::FUNCTION_MASK;
        } else {
            self.0 &= !Self::FUNCTION_MASK;
        }
    }
}

/// MSI-X Capability
#[derive(Debug, Clone)]
pub struct MsixCapability {
    /// Offset in config space
    pub offset: u8,
    /// Message Control
    pub control: MsixControl,
    /// Table BAR and offset
    pub table_bir: u8,
    /// Table offset
    pub table_offset: u32,
    /// PBA BAR
    pub pba_bir: u8,
    /// PBA offset
    pub pba_offset: u32,
    /// Table entries
    pub table: Vec<MsixTableEntry>,
    /// Pending bit array
    pub pba: Vec<u64>,
}

impl MsixCapability {
    /// Create new MSI-X capability
    pub fn new(offset: u8, table_size: u16, table_bar: u8, pba_bar: u8) -> Self {
        let pba_qwords = (table_size as usize + 63) / 64;
        Self {
            offset,
            control: MsixControl::new(table_size),
            table_bir: table_bar,
            table_offset: 0,
            pba_bir: pba_bar,
            pba_offset: 0,
            table: vec![MsixTableEntry::default(); table_size as usize],
            pba: vec![0; pba_qwords],
        }
    }

    /// Set table offset
    pub fn set_table_offset(&mut self, offset: u32) {
        self.table_offset = offset & !0x07;
    }

    /// Set PBA offset
    pub fn set_pba_offset(&mut self, offset: u32) {
        self.pba_offset = offset & !0x07;
    }

    /// Write to config space
    pub fn write_to_config(&self, config: &mut ConfigSpace, next: u8) {
        let header = CapabilityHeader::new(CapabilityId::MsiX, next);
        header.write_to_config(config, self.offset);

        let data = config.data_mut();
        let offset = self.offset as usize;

        // Control register
        data[offset + 2..offset + 4].copy_from_slice(&self.control.0.to_le_bytes());

        // Table offset and BIR
        let table_value = self.table_offset | (self.table_bir as u32);
        data[offset + 4..offset + 8].copy_from_slice(&table_value.to_le_bytes());

        // PBA offset and BIR
        let pba_value = self.pba_offset | (self.pba_bir as u32);
        data[offset + 8..offset + 12].copy_from_slice(&pba_value.to_le_bytes());
    }

    /// Get table entry
    pub fn table_entry(&self, index: usize) -> Option<&MsixTableEntry> {
        self.table.get(index)
    }

    /// Get mutable table entry
    pub fn table_entry_mut(&mut self, index: usize) -> Option<&mut MsixTableEntry> {
        self.table.get_mut(index)
    }

    /// Check if vector is pending
    pub fn is_pending(&self, vector: u16) -> bool {
        let qword = (vector / 64) as usize;
        let bit = vector % 64;
        self.pba.get(qword).map(|v| v & (1 << bit) != 0).unwrap_or(false)
    }

    /// Set vector pending
    pub fn set_pending(&mut self, vector: u16) {
        let qword = (vector / 64) as usize;
        let bit = vector % 64;
        if let Some(v) = self.pba.get_mut(qword) {
            *v |= 1 << bit;
        }
    }

    /// Clear vector pending
    pub fn clear_pending(&mut self, vector: u16) {
        let qword = (vector / 64) as usize;
        let bit = vector % 64;
        if let Some(v) = self.pba.get_mut(qword) {
            *v &= !(1 << bit);
        }
    }
}

/// Power Management States
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerState {
    /// D0 - Fully on
    D0 = 0,
    /// D1 - Light sleep
    D1 = 1,
    /// D2 - Deeper sleep
    D2 = 2,
    /// D3 Hot - Device off but power present
    D3Hot = 3,
}

/// Power Management Control/Status
#[derive(Debug, Clone, Copy, Default)]
pub struct PmControl(pub u16);

impl PmControl {
    /// Power State (2 bits)
    pub const POWER_STATE_MASK: u16 = 0x03;
    /// No Soft Reset
    pub const NO_SOFT_RESET: u16 = 1 << 3;
    /// PME Enable
    pub const PME_ENABLE: u16 = 1 << 8;
    /// Data Select (4 bits)
    pub const DATA_SELECT_MASK: u16 = 0x0F << 9;
    /// Data Scale (2 bits)
    pub const DATA_SCALE_MASK: u16 = 0x03 << 13;
    /// PME Status
    pub const PME_STATUS: u16 = 1 << 15;

    /// Get power state
    pub fn power_state(&self) -> PowerState {
        match self.0 & Self::POWER_STATE_MASK {
            0 => PowerState::D0,
            1 => PowerState::D1,
            2 => PowerState::D2,
            _ => PowerState::D3Hot,
        }
    }

    /// Set power state
    pub fn set_power_state(&mut self, state: PowerState) {
        self.0 = (self.0 & !Self::POWER_STATE_MASK) | (state as u16);
    }

    /// Check if PME is enabled
    pub fn pme_enabled(&self) -> bool {
        self.0 & Self::PME_ENABLE != 0
    }

    /// Check PME status
    pub fn pme_status(&self) -> bool {
        self.0 & Self::PME_STATUS != 0
    }

    /// Clear PME status
    pub fn clear_pme_status(&mut self) {
        self.0 |= Self::PME_STATUS; // Write 1 to clear
    }
}

/// Power Management Capability
#[derive(Debug, Clone)]
pub struct PmCapability {
    /// Offset in config space
    pub offset: u8,
    /// PM Capabilities
    pub capabilities: u16,
    /// Control/Status
    pub control: PmControl,
    /// Bridge Support Extensions
    pub bridge_ext: u8,
    /// Data register
    pub data: u8,
}

impl PmCapability {
    /// Create new PM capability
    pub fn new(offset: u8, d1_support: bool, d2_support: bool) -> Self {
        let mut caps = 0x0003u16; // Version 3
        if d1_support {
            caps |= 1 << 9;
        }
        if d2_support {
            caps |= 1 << 10;
        }
        caps |= 1 << 11; // PME clock not required
        caps |= 0x1F << 11; // PME support D0-D3

        Self {
            offset,
            capabilities: caps,
            control: PmControl(0),
            bridge_ext: 0,
            data: 0,
        }
    }

    /// Write to config space
    pub fn write_to_config(&self, config: &mut ConfigSpace, next: u8) {
        let header = CapabilityHeader::new(CapabilityId::PowerManagement, next);
        header.write_to_config(config, self.offset);

        let data = config.data_mut();
        let offset = self.offset as usize;

        // PM Capabilities
        data[offset + 2..offset + 4].copy_from_slice(&self.capabilities.to_le_bytes());

        // PM Control/Status
        data[offset + 4..offset + 6].copy_from_slice(&self.control.0.to_le_bytes());

        // Bridge extensions and data
        data[offset + 6] = self.bridge_ext;
        data[offset + 7] = self.data;
    }
}

/// PCIe Device Type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PcieDeviceType {
    /// PCI Express Endpoint
    Endpoint = 0,
    /// Legacy PCI Express Endpoint
    LegacyEndpoint = 1,
    /// Root Port of PCI Express Root Complex
    RootPort = 4,
    /// Upstream Port of PCI Express Switch
    UpstreamPort = 5,
    /// Downstream Port of PCI Express Switch
    DownstreamPort = 6,
    /// PCI Express to PCI/PCI-X Bridge
    PcieToPciBridge = 7,
    /// PCI/PCI-X to PCI Express Bridge
    PciToPcieBridge = 8,
    /// Root Complex Integrated Endpoint
    RcIntegratedEndpoint = 9,
    /// Root Complex Event Collector
    RcEventCollector = 10,
}

/// PCIe Link Speed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PcieLinkSpeed {
    /// 2.5 GT/s (Gen1)
    Gen1 = 1,
    /// 5.0 GT/s (Gen2)
    Gen2 = 2,
    /// 8.0 GT/s (Gen3)
    Gen3 = 3,
    /// 16.0 GT/s (Gen4)
    Gen4 = 4,
    /// 32.0 GT/s (Gen5)
    Gen5 = 5,
    /// 64.0 GT/s (Gen6)
    Gen6 = 6,
}

impl PcieLinkSpeed {
    /// Get speed in GT/s
    pub fn gt_per_second(&self) -> f64 {
        match self {
            PcieLinkSpeed::Gen1 => 2.5,
            PcieLinkSpeed::Gen2 => 5.0,
            PcieLinkSpeed::Gen3 => 8.0,
            PcieLinkSpeed::Gen4 => 16.0,
            PcieLinkSpeed::Gen5 => 32.0,
            PcieLinkSpeed::Gen6 => 64.0,
        }
    }

    /// Get bandwidth per lane in MB/s
    pub fn bandwidth_per_lane(&self) -> u32 {
        match self {
            PcieLinkSpeed::Gen1 => 250,
            PcieLinkSpeed::Gen2 => 500,
            PcieLinkSpeed::Gen3 => 984,
            PcieLinkSpeed::Gen4 => 1969,
            PcieLinkSpeed::Gen5 => 3938,
            PcieLinkSpeed::Gen6 => 7563,
        }
    }
}

/// PCIe Link Width
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PcieLinkWidth {
    /// x1
    X1 = 1,
    /// x2
    X2 = 2,
    /// x4
    X4 = 4,
    /// x8
    X8 = 8,
    /// x12
    X12 = 12,
    /// x16
    X16 = 16,
    /// x32
    X32 = 32,
}

/// PCIe Link Status
#[derive(Debug, Clone, Copy, Default)]
pub struct PcieLinkStatus(pub u16);

impl PcieLinkStatus {
    /// Current Link Speed (4 bits)
    pub const SPEED_MASK: u16 = 0x0F;
    /// Negotiated Link Width (6 bits)
    pub const WIDTH_MASK: u16 = 0x3F << 4;
    /// Link Training
    pub const LINK_TRAINING: u16 = 1 << 11;
    /// Slot Clock Configuration
    pub const SLOT_CLOCK: u16 = 1 << 12;
    /// Data Link Layer Link Active
    pub const DL_ACTIVE: u16 = 1 << 13;
    /// Link Bandwidth Management Status
    pub const BW_MGMT_STATUS: u16 = 1 << 14;
    /// Link Autonomous Bandwidth Status
    pub const AUTO_BW_STATUS: u16 = 1 << 15;

    /// Get current link speed
    pub fn speed(&self) -> u8 {
        (self.0 & Self::SPEED_MASK) as u8
    }

    /// Get negotiated width
    pub fn width(&self) -> u8 {
        ((self.0 & Self::WIDTH_MASK) >> 4) as u8
    }

    /// Check if link is training
    pub fn is_training(&self) -> bool {
        self.0 & Self::LINK_TRAINING != 0
    }

    /// Check if data link layer is active
    pub fn dl_active(&self) -> bool {
        self.0 & Self::DL_ACTIVE != 0
    }
}

/// PCIe Capability
#[derive(Debug, Clone)]
pub struct PcieCapability {
    /// Offset in config space
    pub offset: u8,
    /// PCIe Capabilities
    pub capabilities: u16,
    /// Device Capabilities
    pub device_caps: u32,
    /// Device Control
    pub device_control: u16,
    /// Device Status
    pub device_status: u16,
    /// Link Capabilities
    pub link_caps: u32,
    /// Link Control
    pub link_control: u16,
    /// Link Status
    pub link_status: PcieLinkStatus,
    /// Device type
    pub device_type: PcieDeviceType,
    /// Max link speed
    pub max_speed: PcieLinkSpeed,
    /// Max link width
    pub max_width: PcieLinkWidth,
    /// Current speed
    pub current_speed: PcieLinkSpeed,
    /// Current width
    pub current_width: PcieLinkWidth,
}

impl PcieCapability {
    /// Create new PCIe capability
    pub fn new(
        offset: u8,
        device_type: PcieDeviceType,
        max_speed: PcieLinkSpeed,
        max_width: PcieLinkWidth,
    ) -> Self {
        // PCIe Capabilities register
        let capabilities = 0x0002 | ((device_type as u16) << 4); // Version 2

        // Device Capabilities
        let device_caps = 0x00008000 | // Function Level Reset
            (0x02 << 0) |  // Max Payload Size 512B capable
            (0x01 << 3);   // Phantom Functions supported

        // Link Capabilities
        let link_caps = (max_speed as u32)
            | ((max_width as u32) << 4)
            | (1 << 10) // ASPM L0s Entry Latency
            | (1 << 15) // Clock Power Management
            | (1 << 18) // Surprise Down Error Reporting
            | (1 << 19) // Data Link Layer Active Reporting
            | (1 << 20); // Link Bandwidth Notification

        // Link Status
        let link_status = PcieLinkStatus(
            (max_speed as u16) | ((max_width as u16) << 4) | PcieLinkStatus::SLOT_CLOCK,
        );

        Self {
            offset,
            capabilities,
            device_caps,
            device_control: 0,
            device_status: 0,
            link_caps,
            link_control: 0,
            link_status,
            device_type,
            max_speed,
            max_width,
            current_speed: max_speed,
            current_width: max_width,
        }
    }

    /// Get capability size
    pub fn size(&self) -> u8 {
        match self.device_type {
            PcieDeviceType::Endpoint | PcieDeviceType::LegacyEndpoint => 44,
            _ => 60, // Root Port/Switch includes slot and root capabilities
        }
    }

    /// Write to config space
    pub fn write_to_config(&self, config: &mut ConfigSpace, next: u8) {
        let header = CapabilityHeader::new(CapabilityId::PciExpress, next);
        header.write_to_config(config, self.offset);

        let data = config.data_mut();
        let offset = self.offset as usize;

        // PCIe Capabilities
        data[offset + 2..offset + 4].copy_from_slice(&self.capabilities.to_le_bytes());

        // Device Capabilities
        data[offset + 4..offset + 8].copy_from_slice(&self.device_caps.to_le_bytes());

        // Device Control
        data[offset + 8..offset + 10].copy_from_slice(&self.device_control.to_le_bytes());

        // Device Status
        data[offset + 10..offset + 12].copy_from_slice(&self.device_status.to_le_bytes());

        // Link Capabilities
        data[offset + 12..offset + 16].copy_from_slice(&self.link_caps.to_le_bytes());

        // Link Control
        data[offset + 16..offset + 18].copy_from_slice(&self.link_control.to_le_bytes());

        // Link Status
        data[offset + 18..offset + 20].copy_from_slice(&self.link_status.0.to_le_bytes());
    }

    /// Get total bandwidth in MB/s
    pub fn bandwidth(&self) -> u32 {
        self.current_speed.bandwidth_per_lane() * (self.current_width as u32)
    }

    /// Set link state
    pub fn set_link_state(&mut self, speed: PcieLinkSpeed, width: PcieLinkWidth) {
        self.current_speed = speed;
        self.current_width = width;
        self.link_status.0 =
            (self.link_status.0 & !(PcieLinkStatus::SPEED_MASK | PcieLinkStatus::WIDTH_MASK))
                | (speed as u16)
                | ((width as u16) << 4);
    }
}

/// Capability statistics
#[derive(Debug, Default)]
pub struct CapabilityStats {
    /// MSI interrupts generated
    msi_interrupts: AtomicU64,
    /// MSI-X interrupts generated
    msix_interrupts: AtomicU64,
    /// Power state changes
    power_state_changes: AtomicU64,
}

impl CapabilityStats {
    /// Record MSI interrupt
    pub fn record_msi(&self) {
        self.msi_interrupts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record MSI-X interrupt
    pub fn record_msix(&self) {
        self.msix_interrupts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record power state change
    pub fn record_power_change(&self) {
        self.power_state_changes.fetch_add(1, Ordering::Relaxed);
    }

    /// Get MSI count
    pub fn msi_count(&self) -> u64 {
        self.msi_interrupts.load(Ordering::Relaxed)
    }

    /// Get MSI-X count
    pub fn msix_count(&self) -> u64 {
        self.msix_interrupts.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_id() {
        assert_eq!(CapabilityId::from_u8(0x05), Some(CapabilityId::Msi));
        assert_eq!(CapabilityId::from_u8(0x10), Some(CapabilityId::PciExpress));
        assert_eq!(CapabilityId::from_u8(0x11), Some(CapabilityId::MsiX));
    }

    #[test]
    fn test_capability_header() {
        let header = CapabilityHeader::new(CapabilityId::Msi, 0x50);
        assert_eq!(header.id, 0x05);
        assert_eq!(header.next, 0x50);
    }

    #[test]
    fn test_msi_control() {
        let mut ctrl = MsiControl(0);
        assert!(!ctrl.enabled());

        ctrl.enable();
        assert!(ctrl.enabled());

        ctrl.disable();
        assert!(!ctrl.enabled());
    }

    #[test]
    fn test_msi_capability() {
        let msi = MsiCapability::new(0x40, 4, true, true);
        assert_eq!(msi.control.multiple_message_capable(), 2); // log2(4) = 2
        assert!(msi.control.is_64bit());
        assert!(msi.control.per_vector_masking());
    }

    #[test]
    fn test_msi_address() {
        let mut msi = MsiCapability::new(0x40, 1, true, false);
        msi.set_address(0xFEE00000_12345678);
        assert_eq!(msi.address(), 0xFEE00000_12345678);
    }

    #[test]
    fn test_msix_control() {
        let ctrl = MsixControl::new(16);
        assert_eq!(ctrl.table_size(), 16);
        assert!(!ctrl.enabled());
    }

    #[test]
    fn test_msix_capability() {
        let mut msix = MsixCapability::new(0x60, 32, 2, 2);
        assert_eq!(msix.control.table_size(), 32);
        assert_eq!(msix.table.len(), 32);

        msix.set_pending(5);
        assert!(msix.is_pending(5));
        assert!(!msix.is_pending(6));

        msix.clear_pending(5);
        assert!(!msix.is_pending(5));
    }

    #[test]
    fn test_msix_table_entry() {
        let mut entry = MsixTableEntry::default();
        assert!(!entry.is_masked());

        entry.set_masked(true);
        assert!(entry.is_masked());

        entry.set_address(0xFEE00000);
        assert_eq!(entry.address(), 0xFEE00000);
    }

    #[test]
    fn test_pm_control() {
        let mut ctrl = PmControl(0);
        assert_eq!(ctrl.power_state(), PowerState::D0);

        ctrl.set_power_state(PowerState::D3Hot);
        assert_eq!(ctrl.power_state(), PowerState::D3Hot);
    }

    #[test]
    fn test_pm_capability() {
        let pm = PmCapability::new(0x50, true, false);
        assert!(pm.capabilities & (1 << 9) != 0); // D1 support
        assert!(pm.capabilities & (1 << 10) == 0); // No D2 support
    }

    #[test]
    fn test_pcie_link_speed() {
        assert_eq!(PcieLinkSpeed::Gen1.gt_per_second(), 2.5);
        assert_eq!(PcieLinkSpeed::Gen3.bandwidth_per_lane(), 984);
    }

    #[test]
    fn test_pcie_link_status() {
        let status = PcieLinkStatus(0x0043); // Gen3 x4
        assert_eq!(status.speed(), 3);
        assert_eq!(status.width(), 4);
    }

    #[test]
    fn test_pcie_capability() {
        let pcie = PcieCapability::new(
            0x80,
            PcieDeviceType::Endpoint,
            PcieLinkSpeed::Gen3,
            PcieLinkWidth::X4,
        );

        assert_eq!(pcie.device_type, PcieDeviceType::Endpoint);
        assert_eq!(pcie.max_speed, PcieLinkSpeed::Gen3);
        assert_eq!(pcie.max_width, PcieLinkWidth::X4);
        assert_eq!(pcie.bandwidth(), 984 * 4);
    }

    #[test]
    fn test_pcie_set_link_state() {
        let mut pcie = PcieCapability::new(
            0x80,
            PcieDeviceType::Endpoint,
            PcieLinkSpeed::Gen4,
            PcieLinkWidth::X16,
        );

        pcie.set_link_state(PcieLinkSpeed::Gen3, PcieLinkWidth::X8);
        assert_eq!(pcie.current_speed, PcieLinkSpeed::Gen3);
        assert_eq!(pcie.current_width, PcieLinkWidth::X8);
        assert_eq!(pcie.link_status.speed(), 3);
        assert_eq!(pcie.link_status.width(), 8);
    }

    #[test]
    fn test_capability_stats() {
        let stats = CapabilityStats::default();
        stats.record_msi();
        stats.record_msi();
        stats.record_msix();

        assert_eq!(stats.msi_count(), 2);
        assert_eq!(stats.msix_count(), 1);
    }
}
