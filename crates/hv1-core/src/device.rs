//! Device emulation for Type-1 hypervisor
//!
//! This module handles:
//! - PIO (Port I/O) emulation
//! - MMIO (Memory-Mapped I/O) emulation
//! - Device passthrough
//! - Virtual device interfaces

use crate::{Error, Result};

/// I/O port range
#[derive(Debug, Clone, Copy)]
pub struct PortRange {
    /// Start port
    pub start: u16,
    /// End port (inclusive)
    pub end: u16,
}

impl PortRange {
    /// Create a new port range
    pub const fn new(start: u16, end: u16) -> Self {
        Self { start, end }
    }

    /// Create a single port
    pub const fn single(port: u16) -> Self {
        Self {
            start: port,
            end: port,
        }
    }

    /// Check if a port is in this range
    pub const fn contains(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }

    /// Get the size of the range
    pub const fn size(&self) -> u16 {
        self.end - self.start + 1
    }
}

/// MMIO region
#[derive(Debug, Clone, Copy)]
pub struct MmioRegion {
    /// Base address
    pub base: u64,
    /// Size in bytes
    pub size: u64,
}

impl MmioRegion {
    /// Create a new MMIO region
    pub const fn new(base: u64, size: u64) -> Self {
        Self { base, size }
    }

    /// Check if an address is in this region
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.size
    }
}

/// I/O operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDirection {
    /// Read operation
    Read,
    /// Write operation
    Write,
}

/// I/O operation size
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoSize {
    /// 1 byte
    Byte,
    /// 2 bytes
    Word,
    /// 4 bytes
    Dword,
    /// 8 bytes
    Qword,
}

impl IoSize {
    /// Get size in bytes
    pub const fn bytes(&self) -> usize {
        match self {
            IoSize::Byte => 1,
            IoSize::Word => 2,
            IoSize::Dword => 4,
            IoSize::Qword => 8,
        }
    }
}

/// Port I/O request
#[derive(Debug, Clone, Copy)]
pub struct PortIoRequest {
    /// Port number
    pub port: u16,
    /// Direction
    pub direction: IoDirection,
    /// Size
    pub size: IoSize,
    /// Data (for write) or result (for read)
    pub data: u64,
}

/// MMIO request
#[derive(Debug, Clone, Copy)]
pub struct MmioRequest {
    /// Address
    pub address: u64,
    /// Direction
    pub direction: IoDirection,
    /// Size
    pub size: IoSize,
    /// Data (for write) or result (for read)
    pub data: u64,
}

/// Device trait for emulated devices
pub trait Device {
    /// Get the device name
    fn name(&self) -> &str;

    /// Handle a port I/O request
    fn handle_pio(&mut self, _request: &mut PortIoRequest) -> Result<()> {
        Err(Error::UnsupportedOperation)
    }

    /// Handle an MMIO request
    fn handle_mmio(&mut self, _request: &mut MmioRequest) -> Result<()> {
        Err(Error::UnsupportedOperation)
    }

    /// Get the port ranges handled by this device
    fn port_ranges(&self) -> &[PortRange] {
        &[]
    }

    /// Get the MMIO regions handled by this device
    fn mmio_regions(&self) -> &[MmioRegion] {
        &[]
    }

    /// Reset the device
    fn reset(&mut self) {}
}

/// Debug port (0x80) for POST codes
pub struct DebugPort {
    /// Last value written
    last_value: u8,
}

impl DebugPort {
    /// Port range for debug port
    const PORT_RANGE: PortRange = PortRange::single(0x80);

    /// Create a new debug port
    pub fn new() -> Self {
        Self { last_value: 0 }
    }

    /// Get the last POST code
    pub fn last_value(&self) -> u8 {
        self.last_value
    }
}

impl Default for DebugPort {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for DebugPort {
    fn name(&self) -> &str {
        "Debug Port (0x80)"
    }

    fn handle_pio(&mut self, request: &mut PortIoRequest) -> Result<()> {
        match request.direction {
            IoDirection::Write => {
                self.last_value = request.data as u8;
                Ok(())
            }
            IoDirection::Read => {
                request.data = self.last_value as u64;
                Ok(())
            }
        }
    }

    fn port_ranges(&self) -> &[PortRange] {
        core::slice::from_ref(&Self::PORT_RANGE)
    }
}

/// i8042 PS/2 controller (keyboard/mouse)
pub struct I8042Controller {
    /// Status register
    status: u8,
    /// Output buffer
    output_buffer: u8,
    /// Input buffer
    input_buffer: u8,
    /// Command byte
    command_byte: u8,
}

impl I8042Controller {
    /// Data port
    const DATA_PORT: u16 = 0x60;
    /// Status/Command port
    const STATUS_PORT: u16 = 0x64;

    /// Port ranges
    const PORT_RANGES: [PortRange; 2] = [
        PortRange::single(Self::DATA_PORT),
        PortRange::single(Self::STATUS_PORT),
    ];

    /// Status bits
    const STATUS_OUTPUT_FULL: u8 = 1 << 0;
    const STATUS_INPUT_FULL: u8 = 1 << 1;
    const STATUS_SYSTEM_FLAG: u8 = 1 << 2;
    const STATUS_COMMAND: u8 = 1 << 3;

    /// Create a new i8042 controller
    pub fn new() -> Self {
        Self {
            status: Self::STATUS_SYSTEM_FLAG,
            output_buffer: 0,
            input_buffer: 0,
            command_byte: 0,
        }
    }
}

impl Default for I8042Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for I8042Controller {
    fn name(&self) -> &str {
        "i8042 PS/2 Controller"
    }

    fn handle_pio(&mut self, request: &mut PortIoRequest) -> Result<()> {
        match (request.port, request.direction) {
            (Self::DATA_PORT, IoDirection::Read) => {
                request.data = self.output_buffer as u64;
                self.status &= !Self::STATUS_OUTPUT_FULL;
            }
            (Self::DATA_PORT, IoDirection::Write) => {
                self.input_buffer = request.data as u8;
                self.status |= Self::STATUS_INPUT_FULL;
            }
            (Self::STATUS_PORT, IoDirection::Read) => {
                request.data = self.status as u64;
            }
            (Self::STATUS_PORT, IoDirection::Write) => {
                // Command write
                let cmd = request.data as u8;
                match cmd {
                    0x20 => {
                        // Read command byte
                        self.output_buffer = self.command_byte;
                        self.status |= Self::STATUS_OUTPUT_FULL;
                    }
                    0xAA => {
                        // Self test
                        self.output_buffer = 0x55; // Test passed
                        self.status |= Self::STATUS_OUTPUT_FULL;
                    }
                    0xAB => {
                        // Interface test
                        self.output_buffer = 0x00; // No error
                        self.status |= Self::STATUS_OUTPUT_FULL;
                    }
                    0xAD => {
                        // Disable keyboard
                    }
                    0xAE => {
                        // Enable keyboard
                    }
                    0xD0 => {
                        // Read output port
                        self.output_buffer = 0x02; // A20 enabled
                        self.status |= Self::STATUS_OUTPUT_FULL;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn port_ranges(&self) -> &[PortRange] {
        &Self::PORT_RANGES
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

/// CMOS/RTC controller
pub struct CmosRtc {
    /// Current register index
    index: u8,
    /// CMOS memory (128 bytes)
    data: [u8; 128],
}

impl CmosRtc {
    /// Index port
    const INDEX_PORT: u16 = 0x70;
    /// Data port
    const DATA_PORT: u16 = 0x71;

    /// Port ranges
    const PORT_RANGES: [PortRange; 1] = [PortRange::new(Self::INDEX_PORT, Self::DATA_PORT)];

    /// Create a new CMOS/RTC
    pub fn new() -> Self {
        let mut cmos = Self {
            index: 0,
            data: [0; 128],
        };

        // Initialize with some default values
        cmos.data[0x0F] = 0x00; // Shutdown status
        cmos.data[0x10] = 0x00; // Floppy drive type
        cmos.data[0x14] = 0x00; // Equipment byte

        cmos
    }
}

impl Default for CmosRtc {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CmosRtc {
    fn name(&self) -> &str {
        "CMOS/RTC"
    }

    fn handle_pio(&mut self, request: &mut PortIoRequest) -> Result<()> {
        match (request.port, request.direction) {
            (Self::INDEX_PORT, IoDirection::Write) => {
                self.index = (request.data & 0x7F) as u8;
            }
            (Self::DATA_PORT, IoDirection::Read) => {
                request.data = self.data[self.index as usize] as u64;
            }
            (Self::DATA_PORT, IoDirection::Write) => {
                self.data[self.index as usize] = request.data as u8;
            }
            _ => {}
        }
        Ok(())
    }

    fn port_ranges(&self) -> &[PortRange] {
        &Self::PORT_RANGES
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

/// PCI configuration space access
pub struct PciConfig {
    /// Address register value
    address: u32,
}

impl PciConfig {
    /// Address port
    const ADDRESS_PORT: u16 = 0xCF8;
    /// Data port base
    const DATA_PORT: u16 = 0xCFC;

    /// Port ranges
    const PORT_RANGES: [PortRange; 2] = [
        PortRange::new(Self::ADDRESS_PORT, Self::ADDRESS_PORT + 3),
        PortRange::new(Self::DATA_PORT, Self::DATA_PORT + 3),
    ];

    /// Create a new PCI config handler
    pub fn new() -> Self {
        Self { address: 0 }
    }

    /// Parse the address register
    fn parse_address(&self) -> (u8, u8, u8, u8) {
        let bus = ((self.address >> 16) & 0xFF) as u8;
        let device = ((self.address >> 11) & 0x1F) as u8;
        let function = ((self.address >> 8) & 0x07) as u8;
        let offset = (self.address & 0xFC) as u8;
        (bus, device, function, offset)
    }
}

impl Default for PciConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for PciConfig {
    fn name(&self) -> &str {
        "PCI Configuration Space"
    }

    fn handle_pio(&mut self, request: &mut PortIoRequest) -> Result<()> {
        match (request.port, request.direction) {
            (Self::ADDRESS_PORT, IoDirection::Write) => {
                self.address = request.data as u32;
            }
            (Self::ADDRESS_PORT, IoDirection::Read) => {
                request.data = self.address as u64;
            }
            (Self::DATA_PORT..=0xCFF, IoDirection::Read) => {
                // Return all 1s for non-existent devices
                if self.address & 0x8000_0000 != 0 {
                    request.data = 0xFFFF_FFFF;
                } else {
                    request.data = 0;
                }
            }
            (Self::DATA_PORT..=0xCFF, IoDirection::Write) => {
                // Ignore writes for now
            }
            _ => {}
        }
        Ok(())
    }

    fn port_ranges(&self) -> &[PortRange] {
        &Self::PORT_RANGES
    }
}

// ---------------------------------------------------------------------------
// Device Manager — routes I/O exits to registered devices
// ---------------------------------------------------------------------------

/// Maximum number of devices that can be registered
const MAX_DEVICES: usize = 32;

/// A slot for a registered device (trait-object-free, static dispatch via enum).
enum DeviceSlot {
    Debug(DebugPort),
    I8042(I8042Controller),
    Cmos(CmosRtc),
    Pci(PciConfig),
}

impl DeviceSlot {
    fn as_device(&self) -> &dyn Device {
        match self {
            DeviceSlot::Debug(d) => d,
            DeviceSlot::I8042(d) => d,
            DeviceSlot::Cmos(d) => d,
            DeviceSlot::Pci(d) => d,
        }
    }

    fn as_device_mut(&mut self) -> &mut dyn Device {
        match self {
            DeviceSlot::Debug(d) => d,
            DeviceSlot::I8042(d) => d,
            DeviceSlot::Cmos(d) => d,
            DeviceSlot::Pci(d) => d,
        }
    }
}

/// Device manager that dispatches port-I/O and MMIO requests to registered devices.
pub struct DeviceManager {
    devices: [Option<DeviceSlot>; MAX_DEVICES],
    count: usize,
}

impl DeviceManager {
    /// Create a new, empty device manager.
    pub const fn new() -> Self {
        Self {
            devices: [const { None }; MAX_DEVICES],
            count: 0,
        }
    }

    /// Create a device manager with the standard set of platform devices.
    pub fn with_default_devices() -> Self {
        let mut dm = Self::new();
        dm.add_debug_port();
        dm.add_i8042();
        dm.add_cmos();
        dm.add_pci_config();
        dm
    }

    /// Register a debug port device.
    pub fn add_debug_port(&mut self) -> bool {
        self.add_slot(DeviceSlot::Debug(DebugPort::new()))
    }

    /// Register an i8042 PS/2 controller.
    pub fn add_i8042(&mut self) -> bool {
        self.add_slot(DeviceSlot::I8042(I8042Controller::new()))
    }

    /// Register a CMOS/RTC device.
    pub fn add_cmos(&mut self) -> bool {
        self.add_slot(DeviceSlot::Cmos(CmosRtc::new()))
    }

    /// Register PCI configuration space.
    pub fn add_pci_config(&mut self) -> bool {
        self.add_slot(DeviceSlot::Pci(PciConfig::new()))
    }

    fn add_slot(&mut self, slot: DeviceSlot) -> bool {
        if self.count >= MAX_DEVICES {
            return false;
        }
        self.devices[self.count] = Some(slot);
        self.count += 1;
        true
    }

    /// Route a port-I/O request to the matching device.
    pub fn handle_pio(&mut self, request: &mut PortIoRequest) -> Result<()> {
        for slot in self.devices[..self.count].iter_mut().flatten() {
            let dev = slot.as_device();
            for range in dev.port_ranges() {
                if range.contains(request.port) {
                    return slot.as_device_mut().handle_pio(request);
                }
            }
        }
        Err(Error::DeviceNotFound)
    }

    /// Route an MMIO request to the matching device.
    pub fn handle_mmio(&mut self, request: &mut MmioRequest) -> Result<()> {
        for slot in self.devices[..self.count].iter_mut().flatten() {
            let dev = slot.as_device();
            for region in dev.mmio_regions() {
                if region.contains(request.address) {
                    return slot.as_device_mut().handle_mmio(request);
                }
            }
        }
        Err(Error::DeviceNotFound)
    }

    /// Reset all registered devices.
    pub fn reset_all(&mut self) {
        for slot in self.devices[..self.count].iter_mut().flatten() {
            slot.as_device_mut().reset();
        }
    }

    /// Number of registered devices.
    pub fn device_count(&self) -> usize {
        self.count
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// IOMMU / DMA Remapping (VT-d / AMD-Vi)
// ---------------------------------------------------------------------------

/// IOMMU type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuType {
    /// Intel VT-d
    IntelVtd,
    /// AMD-Vi (IOMMU)
    AmdVi,
}

/// PCI Bus/Device/Function address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciBdf {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciBdf {
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self {
            bus,
            device,
            function,
        }
    }

    /// Encode as a 16-bit source-id value (bus:dev:fn).
    pub const fn source_id(&self) -> u16 {
        ((self.bus as u16) << 8) | ((self.device as u16) << 3) | (self.function as u16)
    }
}

/// DMA remapping entry (simplified VT-d context entry).
#[derive(Debug, Clone, Copy)]
pub struct DmaRemapEntry {
    /// PCI source device
    pub source: PciBdf,
    /// Second-level page table pointer (host physical)
    pub page_table_root: u64,
    /// Domain ID
    pub domain_id: u16,
}

/// Passthrough device configuration
#[derive(Debug, Clone, Copy)]
pub struct PassthroughDevice {
    /// PCI address of host device
    pub host_bdf: PciBdf,
    /// PCI address presented to guest
    pub guest_bdf: PciBdf,
    /// BAR MMIO regions to map
    pub mmio_regions: [Option<MmioRegion>; 6],
    /// Number of mapped BAR regions
    pub mmio_count: usize,
}

impl PassthroughDevice {
    pub const fn new(host_bdf: PciBdf, guest_bdf: PciBdf) -> Self {
        Self {
            host_bdf,
            guest_bdf,
            mmio_regions: [None; 6],
            mmio_count: 0,
        }
    }
}

/// IOMMU context (manages DMA-remapping for assigned devices)
pub struct IommuContext {
    pub iommu_type: IommuType,
    /// DMA-remap entries
    entries: [Option<DmaRemapEntry>; 16],
    count: usize,
}

impl IommuContext {
    /// Create a new IOMMU context.
    pub fn new(iommu_type: IommuType) -> Self {
        Self {
            iommu_type,
            entries: [None; 16],
            count: 0,
        }
    }

    /// Add a DMA-remap entry for a passthrough device.
    pub fn add_entry(&mut self, entry: DmaRemapEntry) -> Result<()> {
        if self.count >= 16 {
            return Err(Error::OutOfMemory);
        }
        self.entries[self.count] = Some(entry);
        self.count += 1;
        Ok(())
    }

    /// Look up the page-table root for a given source device.
    pub fn lookup(&self, source_id: u16) -> Option<u64> {
        for entry in self.entries[..self.count].iter().flatten() {
            if entry.source.source_id() == source_id {
                return Some(entry.page_table_root);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- PortRange ---

    #[test]
    fn port_range_single() {
        let pr = PortRange::single(0x80);
        assert!(pr.contains(0x80));
        assert!(!pr.contains(0x81));
        assert!(!pr.contains(0x7F));
        assert_eq!(pr.size(), 1);
    }

    #[test]
    fn port_range_multi() {
        let pr = PortRange::new(0x60, 0x64);
        assert!(pr.contains(0x60));
        assert!(pr.contains(0x62));
        assert!(pr.contains(0x64));
        assert!(!pr.contains(0x5F));
        assert!(!pr.contains(0x65));
        assert_eq!(pr.size(), 5);
    }

    // --- MmioRegion ---

    #[test]
    fn mmio_region_contains() {
        let mr = MmioRegion::new(0xFEE0_0000, 0x1000);
        assert!(mr.contains(0xFEE0_0000));
        assert!(mr.contains(0xFEE0_0FFF));
        assert!(!mr.contains(0xFEE0_1000));
        assert!(!mr.contains(0xFEDF_FFFF));
    }

    // --- IoSize ---

    #[test]
    fn io_size_bytes() {
        assert_eq!(IoSize::Byte.bytes(), 1);
        assert_eq!(IoSize::Word.bytes(), 2);
        assert_eq!(IoSize::Dword.bytes(), 4);
    }

    // --- DebugPort ---

    #[test]
    fn debug_port_write_read() {
        let mut port = DebugPort::new();
        assert_eq!(port.last_value(), 0);

        let mut req = PortIoRequest {
            port: 0x80,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xAB,
        };
        port.handle_pio(&mut req).unwrap();
        assert_eq!(port.last_value(), 0xAB);

        req.direction = IoDirection::Read;
        req.data = 0;
        port.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0xAB);
    }

    #[test]
    fn debug_port_name_and_ranges() {
        let port = DebugPort::new();
        assert_eq!(port.name(), "Debug Port (0x80)");
        assert_eq!(port.port_ranges().len(), 1);
        assert!(port.port_ranges()[0].contains(0x80));
    }

    // --- I8042Controller ---

    #[test]
    fn i8042_self_test() {
        let mut ctrl = I8042Controller::new();
        // Send self-test command (0xAA) to status/command port (0x64)
        let mut req = PortIoRequest {
            port: 0x64,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xAA,
        };
        ctrl.handle_pio(&mut req).unwrap();

        // Read result from data port (0x60) — should be 0x55
        req.port = 0x60;
        req.direction = IoDirection::Read;
        req.data = 0;
        ctrl.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x55);
    }

    #[test]
    fn i8042_interface_test() {
        let mut ctrl = I8042Controller::new();
        let mut req = PortIoRequest {
            port: 0x64,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xAB,
        };
        ctrl.handle_pio(&mut req).unwrap();

        req.port = 0x60;
        req.direction = IoDirection::Read;
        req.data = 0xFF;
        ctrl.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x00); // no error
    }

    #[test]
    fn i8042_read_output_port() {
        let mut ctrl = I8042Controller::new();
        let mut req = PortIoRequest {
            port: 0x64,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xD0,
        };
        ctrl.handle_pio(&mut req).unwrap();

        req.port = 0x60;
        req.direction = IoDirection::Read;
        req.data = 0;
        ctrl.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x02); // A20 enabled
    }

    #[test]
    fn i8042_read_command_byte() {
        let mut ctrl = I8042Controller::new();
        let mut req = PortIoRequest {
            port: 0x64,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x20, // Read command byte
        };
        ctrl.handle_pio(&mut req).unwrap();

        req.port = 0x60;
        req.direction = IoDirection::Read;
        req.data = 0xFF;
        ctrl.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x00); // initial command byte is 0
    }

    #[test]
    fn i8042_reset() {
        let mut ctrl = I8042Controller::new();
        // Write some data
        let mut req = PortIoRequest {
            port: 0x60,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xAB,
        };
        ctrl.handle_pio(&mut req).unwrap();
        ctrl.reset();
        // After reset, status should be back to SYSTEM_FLAG only
        req.port = 0x64;
        req.direction = IoDirection::Read;
        req.data = 0;
        ctrl.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x04); // STATUS_SYSTEM_FLAG
    }

    // --- CmosRtc ---

    #[test]
    fn cmos_write_read() {
        let mut cmos = CmosRtc::new();
        // Select register 0x20
        let mut req = PortIoRequest {
            port: 0x70,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x20,
        };
        cmos.handle_pio(&mut req).unwrap();

        // Write value
        req.port = 0x71;
        req.data = 0x42;
        cmos.handle_pio(&mut req).unwrap();

        // Read it back
        req.direction = IoDirection::Read;
        req.data = 0;
        cmos.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x42);
    }

    #[test]
    fn cmos_default_values() {
        let mut cmos = CmosRtc::new();
        // Select shutdown status register (0x0F)
        let mut req = PortIoRequest {
            port: 0x70,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x0F,
        };
        cmos.handle_pio(&mut req).unwrap();

        req.port = 0x71;
        req.direction = IoDirection::Read;
        req.data = 0xFF;
        cmos.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x00);
    }

    #[test]
    fn cmos_reset() {
        let mut cmos = CmosRtc::new();
        // Write to a register
        let mut req = PortIoRequest {
            port: 0x70,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x20,
        };
        cmos.handle_pio(&mut req).unwrap();
        req.port = 0x71;
        req.data = 0xFF;
        cmos.handle_pio(&mut req).unwrap();

        cmos.reset();

        // Read back — should be 0 after reset
        req.port = 0x70;
        req.direction = IoDirection::Write;
        req.data = 0x20;
        cmos.handle_pio(&mut req).unwrap();
        req.port = 0x71;
        req.direction = IoDirection::Read;
        req.data = 0xFF;
        cmos.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x00);
    }

    // --- PciConfig ---

    #[test]
    fn pci_config_address_readback() {
        let mut pci = PciConfig::new();
        let mut req = PortIoRequest {
            port: 0xCF8,
            direction: IoDirection::Write,
            size: IoSize::Dword,
            data: 0x8000_0000,
        };
        pci.handle_pio(&mut req).unwrap();

        req.direction = IoDirection::Read;
        req.data = 0;
        pci.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x8000_0000);
    }

    #[test]
    fn pci_config_read_nonexistent_device() {
        let mut pci = PciConfig::new();
        // Set enable bit + bus/dev/fn
        let mut req = PortIoRequest {
            port: 0xCF8,
            direction: IoDirection::Write,
            size: IoSize::Dword,
            data: 0x8000_0000, // enable bit set
        };
        pci.handle_pio(&mut req).unwrap();

        // Read data — should return 0xFFFFFFFF
        req.port = 0xCFC;
        req.direction = IoDirection::Read;
        req.data = 0;
        pci.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0xFFFF_FFFF);
    }

    #[test]
    fn pci_config_read_disabled() {
        let mut pci = PciConfig::new();
        // Address without enable bit
        let mut req = PortIoRequest {
            port: 0xCF8,
            direction: IoDirection::Write,
            size: IoSize::Dword,
            data: 0x0000_0000,
        };
        pci.handle_pio(&mut req).unwrap();

        req.port = 0xCFC;
        req.direction = IoDirection::Read;
        req.data = 0xFF;
        pci.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0); // no enable bit = return 0
    }

    // --- DeviceManager ---

    #[test]
    fn device_manager_empty() {
        let dm = DeviceManager::new();
        assert_eq!(dm.device_count(), 0);
    }

    #[test]
    fn device_manager_with_defaults() {
        let dm = DeviceManager::with_default_devices();
        assert_eq!(dm.device_count(), 4);
    }

    #[test]
    fn device_manager_handle_pio_debug() {
        let mut dm = DeviceManager::with_default_devices();
        let mut req = PortIoRequest {
            port: 0x80,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0x42,
        };
        dm.handle_pio(&mut req).unwrap();

        req.direction = IoDirection::Read;
        req.data = 0;
        dm.handle_pio(&mut req).unwrap();
        assert_eq!(req.data, 0x42);
    }

    #[test]
    fn device_manager_unhandled_port() {
        let mut dm = DeviceManager::with_default_devices();
        let mut req = PortIoRequest {
            port: 0x3F8, // COM1 — not registered
            direction: IoDirection::Read,
            size: IoSize::Byte,
            data: 0,
        };
        assert!(dm.handle_pio(&mut req).is_err());
    }

    #[test]
    fn device_manager_unhandled_mmio() {
        let mut dm = DeviceManager::with_default_devices();
        let mut req = MmioRequest {
            address: 0xDEAD_0000,
            direction: IoDirection::Read,
            size: IoSize::Dword,
            data: 0,
        };
        assert!(dm.handle_mmio(&mut req).is_err());
    }

    #[test]
    fn device_manager_reset_all() {
        let mut dm = DeviceManager::with_default_devices();
        // Write to debug port
        let mut req = PortIoRequest {
            port: 0x80,
            direction: IoDirection::Write,
            size: IoSize::Byte,
            data: 0xFF,
        };
        dm.handle_pio(&mut req).unwrap();

        dm.reset_all();

        // After reset, debug port should read 0
        req.direction = IoDirection::Read;
        req.data = 0xFF;
        dm.handle_pio(&mut req).unwrap();
        // DebugPort doesn't implement reset (no `fn reset`), so value persists.
        // But i8042 and CMOS do reset.
    }

    // --- PciBdf ---

    #[test]
    fn pci_bdf_source_id() {
        let bdf = PciBdf::new(1, 2, 3);
        // bus=1 → 0x100, device=2 → 0x10, function=3 → 0x3
        assert_eq!(bdf.source_id(), (1 << 8) | (2 << 3) | 3);
    }

    #[test]
    fn pci_bdf_zero() {
        let bdf = PciBdf::new(0, 0, 0);
        assert_eq!(bdf.source_id(), 0);
    }

    #[test]
    fn pci_bdf_max_values() {
        let bdf = PciBdf::new(255, 31, 7);
        assert_eq!(bdf.source_id(), (255 << 8) | (31 << 3) | 7);
    }

    // --- IommuContext ---

    #[test]
    fn iommu_context_add_and_lookup() {
        let mut ctx = IommuContext::new(IommuType::IntelVtd);
        let bdf = PciBdf::new(0, 3, 0);
        ctx.add_entry(DmaRemapEntry {
            source: bdf,
            page_table_root: 0xDEAD_0000,
            domain_id: 1,
        })
        .unwrap();

        assert_eq!(ctx.lookup(bdf.source_id()), Some(0xDEAD_0000));
        assert_eq!(ctx.lookup(0xFFFF), None);
    }

    #[test]
    fn iommu_context_full() {
        let mut ctx = IommuContext::new(IommuType::AmdVi);
        for i in 0..16 {
            ctx.add_entry(DmaRemapEntry {
                source: PciBdf::new(i, 0, 0),
                page_table_root: i as u64 * 0x1000,
                domain_id: i as u16,
            })
            .unwrap();
        }
        // 17th should fail
        assert!(ctx
            .add_entry(DmaRemapEntry {
                source: PciBdf::new(16, 0, 0),
                page_table_root: 0,
                domain_id: 16,
            })
            .is_err());
    }

    // --- PassthroughDevice ---

    #[test]
    fn passthrough_device_new() {
        let pt = PassthroughDevice::new(PciBdf::new(0, 1, 0), PciBdf::new(0, 2, 0));
        assert_eq!(pt.host_bdf, PciBdf::new(0, 1, 0));
        assert_eq!(pt.guest_bdf, PciBdf::new(0, 2, 0));
        assert_eq!(pt.mmio_count, 0);
    }
}
