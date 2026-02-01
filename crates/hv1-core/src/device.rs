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
        Self { start: port, end: port }
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
    const PORT_RANGES: [PortRange; 1] = [
        PortRange::new(Self::INDEX_PORT, Self::DATA_PORT),
    ];

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
