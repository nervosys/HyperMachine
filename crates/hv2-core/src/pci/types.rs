//! PCI Types and Addressing
//!
//! This module provides core PCI types including addresses, device IDs,
//! class codes, and BAR (Base Address Register) definitions.

use std::fmt;

/// PCI Bus/Device/Function address
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciAddress {
    /// Segment group (for PCIe)
    pub segment: u16,
    /// Bus number (0-255)
    pub bus: u8,
    /// Device number (0-31)
    pub device: u8,
    /// Function number (0-7)
    pub function: u8,
}

impl PciAddress {
    /// Create new PCI address
    #[must_use]
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        Self {
            segment: 0,
            bus,
            device,
            function,
        }
    }

    /// Create with segment
    #[must_use]
    pub fn with_segment(segment: u16, bus: u8, device: u8, function: u8) -> Self {
        Self {
            segment,
            bus,
            device,
            function,
        }
    }

    /// Parse from BDF format (bus:device.function)
    pub fn from_bdf(bdf: u16) -> Self {
        Self {
            segment: 0,
            bus: ((bdf >> 8) & 0xFF) as u8,
            device: ((bdf >> 3) & 0x1F) as u8,
            function: (bdf & 0x07) as u8,
        }
    }

    /// Convert to BDF format
    pub fn to_bdf(&self) -> u16 {
        ((self.bus as u16) << 8) | ((self.device as u16) << 3) | (self.function as u16)
    }

    /// Get configuration space offset for legacy PCI (I/O ports)
    pub fn config_address(&self, offset: u8) -> u32 {
        0x80000000
            | ((self.bus as u32) << 16)
            | ((self.device as u32) << 11)
            | ((self.function as u32) << 8)
            | ((offset as u32) & 0xFC)
    }

    /// Get PCIe ECAM (Enhanced Configuration Access Mechanism) offset
    pub fn ecam_offset(&self, register: u16) -> u64 {
        ((self.segment as u64) << 28)
            | ((self.bus as u64) << 20)
            | ((self.device as u64) << 15)
            | ((self.function as u64) << 12)
            | ((register as u64) & 0xFFF)
    }

    /// Check if this is a valid address
    pub fn is_valid(&self) -> bool {
        self.device <= 31 && self.function <= 7
    }

    /// Check if this is a multi-function device address
    pub fn is_multifunction(&self) -> bool {
        self.function > 0
    }

    /// Get next function address
    pub fn next_function(&self) -> Option<Self> {
        if self.function < 7 {
            Some(Self {
                function: self.function + 1,
                ..*self
            })
        } else {
            None
        }
    }

    /// Get next device address
    pub fn next_device(&self) -> Option<Self> {
        if self.device < 31 {
            Some(Self {
                device: self.device + 1,
                function: 0,
                ..*self
            })
        } else {
            None
        }
    }
}

impl fmt::Display for PciAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segment != 0 {
            write!(
                f,
                "{:04x}:{:02x}:{:02x}.{:x}",
                self.segment, self.bus, self.device, self.function
            )
        } else {
            write!(
                f,
                "{:02x}:{:02x}.{:x}",
                self.bus, self.device, self.function
            )
        }
    }
}

/// PCI Vendor ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VendorId(pub u16);

impl VendorId {
    /// Invalid vendor ID (device not present)
    pub const INVALID: VendorId = VendorId(0xFFFF);

    /// Intel Corporation
    pub const INTEL: VendorId = VendorId(0x8086);
    /// AMD
    pub const AMD: VendorId = VendorId(0x1022);
    /// NVIDIA
    pub const NVIDIA: VendorId = VendorId(0x10DE);
    /// Red Hat (VirtIO)
    pub const RED_HAT: VendorId = VendorId(0x1AF4);
    /// QEMU
    pub const QEMU: VendorId = VendorId(0x1234);

    /// Check if vendor ID is valid (device present)
    pub fn is_valid(&self) -> bool {
        self.0 != 0xFFFF
    }
}

impl From<u16> for VendorId {
    fn from(value: u16) -> Self {
        VendorId(value)
    }
}

impl fmt::Display for VendorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)
    }
}

/// PCI Device ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(pub u16);

impl DeviceId {
    /// Invalid device ID
    pub const INVALID: DeviceId = DeviceId(0xFFFF);

    /// VirtIO Network Device (transitional)
    pub const VIRTIO_NET: DeviceId = DeviceId(0x1000);
    /// VirtIO Block Device (transitional)
    pub const VIRTIO_BLK: DeviceId = DeviceId(0x1001);
    /// VirtIO Console
    pub const VIRTIO_CONSOLE: DeviceId = DeviceId(0x1003);
    /// VirtIO GPU
    pub const VIRTIO_GPU: DeviceId = DeviceId(0x1050);

    /// Check if device ID is valid
    pub fn is_valid(&self) -> bool {
        self.0 != 0xFFFF
    }
}

impl From<u16> for DeviceId {
    fn from(value: u16) -> Self {
        DeviceId(value)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)
    }
}

/// PCI Class Code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassCode {
    /// Base class
    pub base: u8,
    /// Sub class
    pub sub: u8,
    /// Programming interface
    pub prog_if: u8,
}

impl ClassCode {
    /// Create new class code
    pub fn new(base: u8, sub: u8, prog_if: u8) -> Self {
        Self { base, sub, prog_if }
    }

    /// Create from 24-bit value
    pub fn from_u32(value: u32) -> Self {
        Self {
            base: ((value >> 16) & 0xFF) as u8,
            sub: ((value >> 8) & 0xFF) as u8,
            prog_if: (value & 0xFF) as u8,
        }
    }

    /// Convert to 24-bit value
    pub fn to_u32(&self) -> u32 {
        ((self.base as u32) << 16) | ((self.sub as u32) << 8) | (self.prog_if as u32)
    }

    // Common class codes

    /// Unclassified device
    pub const UNCLASSIFIED: ClassCode = ClassCode {
        base: 0x00,
        sub: 0x00,
        prog_if: 0x00,
    };

    /// Mass storage - IDE controller
    pub const IDE_CONTROLLER: ClassCode = ClassCode {
        base: 0x01,
        sub: 0x01,
        prog_if: 0x8A,
    };

    /// Mass storage - SATA controller (AHCI)
    pub const SATA_AHCI: ClassCode = ClassCode {
        base: 0x01,
        sub: 0x06,
        prog_if: 0x01,
    };

    /// Mass storage - NVMe controller
    pub const NVME: ClassCode = ClassCode {
        base: 0x01,
        sub: 0x08,
        prog_if: 0x02,
    };

    /// Network controller - Ethernet
    pub const ETHERNET: ClassCode = ClassCode {
        base: 0x02,
        sub: 0x00,
        prog_if: 0x00,
    };

    /// Display controller - VGA
    pub const VGA: ClassCode = ClassCode {
        base: 0x03,
        sub: 0x00,
        prog_if: 0x00,
    };

    /// Display controller - 3D
    pub const DISPLAY_3D: ClassCode = ClassCode {
        base: 0x03,
        sub: 0x02,
        prog_if: 0x00,
    };

    /// Multimedia - Audio
    pub const AUDIO: ClassCode = ClassCode {
        base: 0x04,
        sub: 0x01,
        prog_if: 0x00,
    };

    /// Multimedia - HD Audio
    pub const HD_AUDIO: ClassCode = ClassCode {
        base: 0x04,
        sub: 0x03,
        prog_if: 0x00,
    };

    /// Bridge - Host bridge
    pub const HOST_BRIDGE: ClassCode = ClassCode {
        base: 0x06,
        sub: 0x00,
        prog_if: 0x00,
    };

    /// Bridge - ISA bridge
    pub const ISA_BRIDGE: ClassCode = ClassCode {
        base: 0x06,
        sub: 0x01,
        prog_if: 0x00,
    };

    /// Bridge - PCI-to-PCI bridge
    pub const PCI_BRIDGE: ClassCode = ClassCode {
        base: 0x06,
        sub: 0x04,
        prog_if: 0x00,
    };

    /// Bridge - PCI-to-PCI bridge (subtractive decode)
    pub const PCI_BRIDGE_SUBTRACTIVE: ClassCode = ClassCode {
        base: 0x06,
        sub: 0x04,
        prog_if: 0x01,
    };

    /// Serial bus - USB UHCI
    pub const USB_UHCI: ClassCode = ClassCode {
        base: 0x0C,
        sub: 0x03,
        prog_if: 0x00,
    };

    /// Serial bus - USB OHCI
    pub const USB_OHCI: ClassCode = ClassCode {
        base: 0x0C,
        sub: 0x03,
        prog_if: 0x10,
    };

    /// Serial bus - USB EHCI
    pub const USB_EHCI: ClassCode = ClassCode {
        base: 0x0C,
        sub: 0x03,
        prog_if: 0x20,
    };

    /// Serial bus - USB xHCI
    pub const USB_XHCI: ClassCode = ClassCode {
        base: 0x0C,
        sub: 0x03,
        prog_if: 0x30,
    };

    /// Check if this is a bridge device
    pub fn is_bridge(&self) -> bool {
        self.base == 0x06
    }

    /// Check if this is a PCI-to-PCI bridge
    pub fn is_pci_bridge(&self) -> bool {
        self.base == 0x06 && self.sub == 0x04
    }

    /// Check if this is a storage device
    pub fn is_storage(&self) -> bool {
        self.base == 0x01
    }

    /// Check if this is a network device
    pub fn is_network(&self) -> bool {
        self.base == 0x02
    }

    /// Check if this is a display device
    pub fn is_display(&self) -> bool {
        self.base == 0x03
    }
}

impl fmt::Display for ClassCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}{:02x}{:02x}", self.base, self.sub, self.prog_if)
    }
}

/// PCI Header Type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HeaderType {
    /// Standard device (Type 0)
    Standard = 0x00,
    /// PCI-to-PCI bridge (Type 1)
    PciBridge = 0x01,
    /// CardBus bridge (Type 2)
    CardBusBridge = 0x02,
}

impl HeaderType {
    /// Parse from register value
    pub fn from_register(value: u8) -> Option<Self> {
        match value & 0x7F {
            0x00 => Some(HeaderType::Standard),
            0x01 => Some(HeaderType::PciBridge),
            0x02 => Some(HeaderType::CardBusBridge),
            _ => None,
        }
    }

    /// Check if multi-function bit is set
    pub fn is_multifunction(value: u8) -> bool {
        value & 0x80 != 0
    }
}

/// BAR (Base Address Register) type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarType {
    /// Memory BAR (32-bit)
    Memory32 {
        /// Prefetchable
        prefetchable: bool,
        /// Base address
        address: u32,
        /// Size in bytes
        size: u32,
    },
    /// Memory BAR (64-bit)
    Memory64 {
        /// Prefetchable
        prefetchable: bool,
        /// Base address
        address: u64,
        /// Size in bytes
        size: u64,
    },
    /// I/O BAR
    Io {
        /// Base address
        address: u32,
        /// Size in bytes
        size: u32,
    },
    /// Unused BAR
    Unused,
}

impl BarType {
    /// Parse BAR from register value
    pub fn from_register(value: u32, size_mask: u32) -> Self {
        if value == 0 && size_mask == 0 {
            return BarType::Unused;
        }

        if value & 0x01 != 0 {
            // I/O BAR
            let address = value & 0xFFFFFFFC;
            let size = (!size_mask & 0xFFFFFFFC).wrapping_add(1);
            BarType::Io {
                address,
                size: if size == 0 { 0 } else { size },
            }
        } else {
            // Memory BAR
            let prefetchable = value & 0x08 != 0;
            let mem_type = (value >> 1) & 0x03;

            match mem_type {
                0x00 => {
                    // 32-bit
                    let address = value & 0xFFFFFFF0;
                    let size = (!size_mask & 0xFFFFFFF0).wrapping_add(1);
                    BarType::Memory32 {
                        prefetchable,
                        address,
                        size: if size == 0 { 0 } else { size },
                    }
                }
                0x02 => {
                    // 64-bit (lower half)
                    let address = (value & 0xFFFFFFF0) as u64;
                    let size = ((!size_mask & 0xFFFFFFF0) as u64).wrapping_add(1);
                    BarType::Memory64 {
                        prefetchable,
                        address,
                        size: if size == 0 { 0 } else { size },
                    }
                }
                _ => BarType::Unused,
            }
        }
    }

    /// Check if BAR is memory type
    pub fn is_memory(&self) -> bool {
        matches!(self, BarType::Memory32 { .. } | BarType::Memory64 { .. })
    }

    /// Check if BAR is I/O type
    pub fn is_io(&self) -> bool {
        matches!(self, BarType::Io { .. })
    }

    /// Check if BAR is 64-bit
    pub fn is_64bit(&self) -> bool {
        matches!(self, BarType::Memory64 { .. })
    }

    /// Get base address
    pub fn address(&self) -> Option<u64> {
        match self {
            BarType::Memory32 { address, .. } => Some(*address as u64),
            BarType::Memory64 { address, .. } => Some(*address),
            BarType::Io { address, .. } => Some(*address as u64),
            BarType::Unused => None,
        }
    }

    /// Get size
    pub fn size(&self) -> Option<u64> {
        match self {
            BarType::Memory32 { size, .. } => Some(*size as u64),
            BarType::Memory64 { size, .. } => Some(*size),
            BarType::Io { size, .. } => Some(*size as u64),
            BarType::Unused => None,
        }
    }

    /// Check if prefetchable
    pub fn is_prefetchable(&self) -> bool {
        match self {
            BarType::Memory32 { prefetchable, .. } => *prefetchable,
            BarType::Memory64 { prefetchable, .. } => *prefetchable,
            _ => false,
        }
    }
}

/// PCI Command Register bits
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandRegister(pub u16);

impl CommandRegister {
    /// I/O Space Enable
    pub const IO_SPACE: u16 = 1 << 0;
    /// Memory Space Enable
    pub const MEMORY_SPACE: u16 = 1 << 1;
    /// Bus Master Enable
    pub const BUS_MASTER: u16 = 1 << 2;
    /// Special Cycles Enable
    pub const SPECIAL_CYCLES: u16 = 1 << 3;
    /// Memory Write and Invalidate Enable
    pub const MWI_ENABLE: u16 = 1 << 4;
    /// VGA Palette Snoop
    pub const VGA_PALETTE_SNOOP: u16 = 1 << 5;
    /// Parity Error Response
    pub const PARITY_ERROR_RESPONSE: u16 = 1 << 6;
    /// SERR# Enable
    pub const SERR_ENABLE: u16 = 1 << 8;
    /// Fast Back-to-Back Enable
    pub const FAST_B2B_ENABLE: u16 = 1 << 9;
    /// Interrupt Disable
    pub const INTERRUPT_DISABLE: u16 = 1 << 10;

    /// Create new command register
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    /// Check if I/O space is enabled
    pub fn io_enabled(&self) -> bool {
        self.0 & Self::IO_SPACE != 0
    }

    /// Check if memory space is enabled
    pub fn memory_enabled(&self) -> bool {
        self.0 & Self::MEMORY_SPACE != 0
    }

    /// Check if bus mastering is enabled
    pub fn bus_master_enabled(&self) -> bool {
        self.0 & Self::BUS_MASTER != 0
    }

    /// Check if interrupts are disabled
    pub fn interrupts_disabled(&self) -> bool {
        self.0 & Self::INTERRUPT_DISABLE != 0
    }

    /// Enable I/O space
    pub fn enable_io(&mut self) {
        self.0 |= Self::IO_SPACE;
    }

    /// Enable memory space
    pub fn enable_memory(&mut self) {
        self.0 |= Self::MEMORY_SPACE;
    }

    /// Enable bus mastering
    pub fn enable_bus_master(&mut self) {
        self.0 |= Self::BUS_MASTER;
    }

    /// Disable interrupts
    pub fn disable_interrupts(&mut self) {
        self.0 |= Self::INTERRUPT_DISABLE;
    }
}

/// PCI Status Register bits
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusRegister(pub u16);

impl StatusRegister {
    /// Interrupt Status
    pub const INTERRUPT_STATUS: u16 = 1 << 3;
    /// Capabilities List
    pub const CAPABILITIES_LIST: u16 = 1 << 4;
    /// 66 MHz Capable
    pub const MHZ_66_CAPABLE: u16 = 1 << 5;
    /// Fast Back-to-Back Capable
    pub const FAST_B2B_CAPABLE: u16 = 1 << 7;
    /// Master Data Parity Error
    pub const MASTER_DATA_PARITY_ERROR: u16 = 1 << 8;
    /// DEVSEL Timing (2 bits)
    pub const DEVSEL_MASK: u16 = 0x03 << 9;
    /// Signaled Target Abort
    pub const SIGNALED_TARGET_ABORT: u16 = 1 << 11;
    /// Received Target Abort
    pub const RECEIVED_TARGET_ABORT: u16 = 1 << 12;
    /// Received Master Abort
    pub const RECEIVED_MASTER_ABORT: u16 = 1 << 13;
    /// Signaled System Error
    pub const SIGNALED_SYSTEM_ERROR: u16 = 1 << 14;
    /// Detected Parity Error
    pub const DETECTED_PARITY_ERROR: u16 = 1 << 15;

    /// Create new status register
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    /// Check if capabilities list is present
    pub fn has_capabilities(&self) -> bool {
        self.0 & Self::CAPABILITIES_LIST != 0
    }

    /// Check interrupt status
    pub fn interrupt_pending(&self) -> bool {
        self.0 & Self::INTERRUPT_STATUS != 0
    }

    /// Clear write-1-to-clear bits
    pub fn clear_errors(&mut self, mask: u16) {
        // These bits are cleared by writing 1
        let clearable = Self::MASTER_DATA_PARITY_ERROR
            | Self::SIGNALED_TARGET_ABORT
            | Self::RECEIVED_TARGET_ABORT
            | Self::RECEIVED_MASTER_ABORT
            | Self::SIGNALED_SYSTEM_ERROR
            | Self::DETECTED_PARITY_ERROR;
        self.0 &= !(mask & clearable);
    }
}

/// Interrupt pin
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterruptPin {
    /// No interrupt
    None = 0,
    /// INTA#
    IntA = 1,
    /// INTB#
    IntB = 2,
    /// INTC#
    IntC = 3,
    /// INTD#
    IntD = 4,
}

impl InterruptPin {
    /// Parse from register value
    pub fn from_register(value: u8) -> Self {
        match value {
            1 => InterruptPin::IntA,
            2 => InterruptPin::IntB,
            3 => InterruptPin::IntC,
            4 => InterruptPin::IntD,
            _ => InterruptPin::None,
        }
    }

    /// Swizzle interrupt for bridge
    pub fn swizzle(&self, device: u8) -> Self {
        if *self == InterruptPin::None {
            return InterruptPin::None;
        }
        let pin = (*self as u8 - 1 + device) % 4 + 1;
        InterruptPin::from_register(pin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_address_creation() {
        let addr = PciAddress::new(0, 1, 0);
        assert_eq!(addr.bus, 0);
        assert_eq!(addr.device, 1);
        assert_eq!(addr.function, 0);
        assert!(addr.is_valid());
    }

    #[test]
    fn test_pci_address_bdf() {
        let addr = PciAddress::new(1, 2, 3);
        let bdf = addr.to_bdf();
        let restored = PciAddress::from_bdf(bdf);
        assert_eq!(restored.bus, addr.bus);
        assert_eq!(restored.device, addr.device);
        assert_eq!(restored.function, addr.function);
    }

    #[test]
    fn test_pci_address_display() {
        let addr = PciAddress::new(0, 31, 2);
        assert_eq!(format!("{}", addr), "00:1f.2");

        let addr_seg = PciAddress::with_segment(1, 0, 31, 2);
        assert_eq!(format!("{}", addr_seg), "0001:00:1f.2");
    }

    #[test]
    fn test_pci_address_config() {
        let addr = PciAddress::new(0, 2, 0);
        let config = addr.config_address(0);
        assert_eq!(config & 0x80000000, 0x80000000); // Enable bit
    }

    #[test]
    fn test_pci_address_ecam() {
        let addr = PciAddress::new(0, 1, 0);
        let offset = addr.ecam_offset(0);
        assert_eq!(offset, 0x8000); // Bus 0, Dev 1, Func 0
    }

    #[test]
    fn test_pci_address_next() {
        let addr = PciAddress::new(0, 1, 0);

        let next_func = addr.next_function().unwrap();
        assert_eq!(next_func.function, 1);

        let next_dev = addr.next_device().unwrap();
        assert_eq!(next_dev.device, 2);
        assert_eq!(next_dev.function, 0);
    }

    #[test]
    fn test_vendor_id() {
        assert!(!VendorId::INVALID.is_valid());
        assert!(VendorId::INTEL.is_valid());
        assert_eq!(format!("{}", VendorId::INTEL), "8086");
    }

    #[test]
    fn test_device_id() {
        assert!(!DeviceId::INVALID.is_valid());
        assert!(DeviceId::VIRTIO_NET.is_valid());
    }

    #[test]
    fn test_class_code() {
        let class = ClassCode::ETHERNET;
        assert!(class.is_network());
        assert!(!class.is_storage());

        let bridge = ClassCode::PCI_BRIDGE;
        assert!(bridge.is_bridge());
        assert!(bridge.is_pci_bridge());
    }

    #[test]
    fn test_class_code_conversion() {
        let class = ClassCode::new(0x01, 0x06, 0x01);
        let value = class.to_u32();
        let restored = ClassCode::from_u32(value);
        assert_eq!(restored.base, class.base);
        assert_eq!(restored.sub, class.sub);
        assert_eq!(restored.prog_if, class.prog_if);
    }

    #[test]
    fn test_header_type() {
        assert_eq!(HeaderType::from_register(0x00), Some(HeaderType::Standard));
        assert_eq!(HeaderType::from_register(0x01), Some(HeaderType::PciBridge));
        assert_eq!(HeaderType::from_register(0x81), Some(HeaderType::PciBridge));
        assert!(HeaderType::is_multifunction(0x80));
    }

    #[test]
    fn test_bar_type_memory32() {
        let bar = BarType::from_register(0xFEB00000, 0xFFFF0000);
        assert!(bar.is_memory());
        assert!(!bar.is_64bit());
        assert!(bar.address().is_some());
    }

    #[test]
    fn test_bar_type_io() {
        let bar = BarType::from_register(0x0001, 0xFFFC);
        assert!(bar.is_io());
        assert!(!bar.is_memory());
    }

    #[test]
    fn test_bar_type_unused() {
        let bar = BarType::from_register(0, 0);
        assert!(matches!(bar, BarType::Unused));
    }

    #[test]
    fn test_command_register() {
        let mut cmd = CommandRegister::new(0);
        assert!(!cmd.io_enabled());
        assert!(!cmd.memory_enabled());

        cmd.enable_io();
        cmd.enable_memory();
        cmd.enable_bus_master();

        assert!(cmd.io_enabled());
        assert!(cmd.memory_enabled());
        assert!(cmd.bus_master_enabled());
    }

    #[test]
    fn test_status_register() {
        let status = StatusRegister::new(StatusRegister::CAPABILITIES_LIST);
        assert!(status.has_capabilities());
    }

    #[test]
    fn test_interrupt_pin() {
        assert_eq!(InterruptPin::from_register(1), InterruptPin::IntA);
        assert_eq!(InterruptPin::from_register(0), InterruptPin::None);

        // Swizzle test
        let pin = InterruptPin::IntA;
        let swizzled = pin.swizzle(1);
        assert_eq!(swizzled, InterruptPin::IntB);
    }

    #[test]
    fn test_interrupt_pin_swizzle() {
        let pin = InterruptPin::IntA;

        // Device 0: A -> A
        assert_eq!(pin.swizzle(0), InterruptPin::IntA);
        // Device 1: A -> B
        assert_eq!(pin.swizzle(1), InterruptPin::IntB);
        // Device 2: A -> C
        assert_eq!(pin.swizzle(2), InterruptPin::IntC);
        // Device 3: A -> D
        assert_eq!(pin.swizzle(3), InterruptPin::IntD);
        // Device 4: A -> A (wraps)
        assert_eq!(pin.swizzle(4), InterruptPin::IntA);
    }
}
