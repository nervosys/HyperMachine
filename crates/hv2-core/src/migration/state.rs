//! VM state serialization for live migration and snapshots
//!
//! This module provides serialization infrastructure for capturing and
//! restoring complete VM state including CPU registers, memory, and devices.

use std::collections::HashMap;
use std::io::{Read, Write};

/// Serialization format version
pub const FORMAT_VERSION: u32 = 1;

/// Magic number for state files
pub const STATE_MAGIC: u32 = 0x5648_5632; // "VHV2"

/// State section types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SectionType {
    /// Header section
    Header = 0,
    /// CPU state
    Cpu = 1,
    /// Memory regions
    Memory = 2,
    /// Device state
    Device = 3,
    /// Timer state
    Timer = 4,
    /// Interrupt controller state
    Interrupt = 5,
    /// Custom extension
    Custom = 255,
}

impl SectionType {
    /// Create from raw value
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Header),
            1 => Some(Self::Cpu),
            2 => Some(Self::Memory),
            3 => Some(Self::Device),
            4 => Some(Self::Timer),
            5 => Some(Self::Interrupt),
            255 => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Error type for serialization operations
#[derive(Debug, Clone)]
pub enum SerializeError {
    /// I/O error
    Io(String),
    /// Invalid format
    InvalidFormat(String),
    /// Version mismatch
    VersionMismatch { expected: u32, found: u32 },
    /// Missing required section
    MissingSection(SectionType),
    /// Invalid section data
    InvalidSection(String),
    /// Checksum mismatch
    ChecksumMismatch,
    /// State not found
    NotFound(String),
}

impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "I/O error: {}", msg),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            Self::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "Version mismatch: expected {}, found {}",
                    expected, found
                )
            }
            Self::MissingSection(s) => write!(f, "Missing section: {:?}", s),
            Self::InvalidSection(msg) => write!(f, "Invalid section: {}", msg),
            Self::ChecksumMismatch => write!(f, "Checksum mismatch"),
            Self::NotFound(name) => write!(f, "State not found: {}", name),
        }
    }
}

impl std::error::Error for SerializeError {}

/// Result type for serialization operations
pub type SerializeResult<T> = Result<T, SerializeError>;

/// Trait for types that can be serialized to/from migration state
pub trait Migratable {
    /// Get the name of this component for state storage
    fn name(&self) -> &str;

    /// Serialize state to bytes
    fn save_state(&self) -> SerializeResult<Vec<u8>>;

    /// Restore state from bytes
    fn restore_state(&mut self, data: &[u8]) -> SerializeResult<()>;

    /// Get the section type for this component
    fn section_type(&self) -> SectionType {
        SectionType::Custom
    }
}

/// CPU register state for x86-64
#[derive(Debug, Clone, Default)]
pub struct CpuState {
    // General purpose registers
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,

    // Instruction pointer and flags
    pub rip: u64,
    pub rflags: u64,

    // Segment registers
    pub cs: SegmentRegister,
    pub ds: SegmentRegister,
    pub es: SegmentRegister,
    pub fs: SegmentRegister,
    pub gs: SegmentRegister,
    pub ss: SegmentRegister,

    // Descriptor tables
    pub gdt: DescriptorTable,
    pub idt: DescriptorTable,
    pub ldt: SegmentRegister,
    pub tr: SegmentRegister,

    // Control registers
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,

    // Debug registers
    pub dr0: u64,
    pub dr1: u64,
    pub dr2: u64,
    pub dr3: u64,
    pub dr6: u64,
    pub dr7: u64,

    // Model-specific registers
    pub apic_base: u64,
    pub pat: u64,
    pub sysenter_cs: u64,
    pub sysenter_esp: u64,
    pub sysenter_eip: u64,
    pub star: u64,
    pub lstar: u64,
    pub cstar: u64,
    pub sfmask: u64,
    pub kernel_gs_base: u64,
    pub tsc: u64,

    // FPU/SSE/AVX state
    pub fpu_state: Vec<u8>,
}

/// Segment register state
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentRegister {
    pub selector: u16,
    pub base: u64,
    pub limit: u32,
    pub access_rights: u32,
}

impl SegmentRegister {
    /// Create a new segment register
    pub fn new(selector: u16, base: u64, limit: u32, access_rights: u32) -> Self {
        Self {
            selector,
            base,
            limit,
            access_rights,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 22] {
        let mut bytes = [0u8; 22];
        bytes[0..2].copy_from_slice(&self.selector.to_le_bytes());
        bytes[2..10].copy_from_slice(&self.base.to_le_bytes());
        bytes[10..14].copy_from_slice(&self.limit.to_le_bytes());
        bytes[14..18].copy_from_slice(&self.access_rights.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 18 {
            return None;
        }
        Some(Self {
            selector: u16::from_le_bytes([bytes[0], bytes[1]]),
            base: u64::from_le_bytes([
                bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
            ]),
            limit: u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]),
            access_rights: u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]),
        })
    }
}

/// Descriptor table register state (GDT/IDT)
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorTable {
    pub base: u64,
    pub limit: u16,
}

impl DescriptorTable {
    /// Create a new descriptor table
    pub fn new(base: u64, limit: u16) -> Self {
        Self { base, limit }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 10] {
        let mut bytes = [0u8; 10];
        bytes[0..8].copy_from_slice(&self.base.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.limit.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 10 {
            return None;
        }
        Some(Self {
            base: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            limit: u16::from_le_bytes([bytes[8], bytes[9]]),
        })
    }
}

impl CpuState {
    /// Create a new CPU state with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1024);

        // General purpose registers (16 * 8 = 128 bytes)
        bytes.extend_from_slice(&self.rax.to_le_bytes());
        bytes.extend_from_slice(&self.rbx.to_le_bytes());
        bytes.extend_from_slice(&self.rcx.to_le_bytes());
        bytes.extend_from_slice(&self.rdx.to_le_bytes());
        bytes.extend_from_slice(&self.rsi.to_le_bytes());
        bytes.extend_from_slice(&self.rdi.to_le_bytes());
        bytes.extend_from_slice(&self.rsp.to_le_bytes());
        bytes.extend_from_slice(&self.rbp.to_le_bytes());
        bytes.extend_from_slice(&self.r8.to_le_bytes());
        bytes.extend_from_slice(&self.r9.to_le_bytes());
        bytes.extend_from_slice(&self.r10.to_le_bytes());
        bytes.extend_from_slice(&self.r11.to_le_bytes());
        bytes.extend_from_slice(&self.r12.to_le_bytes());
        bytes.extend_from_slice(&self.r13.to_le_bytes());
        bytes.extend_from_slice(&self.r14.to_le_bytes());
        bytes.extend_from_slice(&self.r15.to_le_bytes());

        // RIP and RFLAGS
        bytes.extend_from_slice(&self.rip.to_le_bytes());
        bytes.extend_from_slice(&self.rflags.to_le_bytes());

        // Segment registers
        bytes.extend_from_slice(&self.cs.to_bytes());
        bytes.extend_from_slice(&self.ds.to_bytes());
        bytes.extend_from_slice(&self.es.to_bytes());
        bytes.extend_from_slice(&self.fs.to_bytes());
        bytes.extend_from_slice(&self.gs.to_bytes());
        bytes.extend_from_slice(&self.ss.to_bytes());

        // Descriptor tables
        bytes.extend_from_slice(&self.gdt.to_bytes());
        bytes.extend_from_slice(&self.idt.to_bytes());
        bytes.extend_from_slice(&self.ldt.to_bytes());
        bytes.extend_from_slice(&self.tr.to_bytes());

        // Control registers
        bytes.extend_from_slice(&self.cr0.to_le_bytes());
        bytes.extend_from_slice(&self.cr2.to_le_bytes());
        bytes.extend_from_slice(&self.cr3.to_le_bytes());
        bytes.extend_from_slice(&self.cr4.to_le_bytes());
        bytes.extend_from_slice(&self.cr8.to_le_bytes());
        bytes.extend_from_slice(&self.efer.to_le_bytes());

        // Debug registers
        bytes.extend_from_slice(&self.dr0.to_le_bytes());
        bytes.extend_from_slice(&self.dr1.to_le_bytes());
        bytes.extend_from_slice(&self.dr2.to_le_bytes());
        bytes.extend_from_slice(&self.dr3.to_le_bytes());
        bytes.extend_from_slice(&self.dr6.to_le_bytes());
        bytes.extend_from_slice(&self.dr7.to_le_bytes());

        // MSRs
        bytes.extend_from_slice(&self.apic_base.to_le_bytes());
        bytes.extend_from_slice(&self.pat.to_le_bytes());
        bytes.extend_from_slice(&self.sysenter_cs.to_le_bytes());
        bytes.extend_from_slice(&self.sysenter_esp.to_le_bytes());
        bytes.extend_from_slice(&self.sysenter_eip.to_le_bytes());
        bytes.extend_from_slice(&self.star.to_le_bytes());
        bytes.extend_from_slice(&self.lstar.to_le_bytes());
        bytes.extend_from_slice(&self.cstar.to_le_bytes());
        bytes.extend_from_slice(&self.sfmask.to_le_bytes());
        bytes.extend_from_slice(&self.kernel_gs_base.to_le_bytes());
        bytes.extend_from_slice(&self.tsc.to_le_bytes());

        // FPU state length and data
        bytes.extend_from_slice(&(self.fpu_state.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.fpu_state);

        bytes
    }

    /// Get total serialized size
    pub fn serialized_size(&self) -> usize {
        // Fixed size parts + FPU state
        128 + 16 + 132 + 20 + 48 + 48 + 88 + 4 + self.fpu_state.len()
    }
}

/// Memory region state
#[derive(Debug, Clone)]
pub struct MemoryRegionState {
    /// Guest physical address
    pub gpa: u64,
    /// Size in bytes
    pub size: u64,
    /// Memory flags
    pub flags: u32,
    /// Memory data (may be compressed)
    pub data: Vec<u8>,
    /// Whether data is compressed
    pub compressed: bool,
}

impl MemoryRegionState {
    /// Create a new memory region state
    pub fn new(gpa: u64, size: u64, flags: u32, data: Vec<u8>) -> Self {
        Self {
            gpa,
            size,
            flags,
            data,
            compressed: false,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24 + self.data.len());
        bytes.extend_from_slice(&self.gpa.to_le_bytes());
        bytes.extend_from_slice(&self.size.to_le_bytes());
        bytes.extend_from_slice(&self.flags.to_le_bytes());
        bytes.push(if self.compressed { 1 } else { 0 });
        bytes.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> SerializeResult<Self> {
        if bytes.len() < 25 {
            return Err(SerializeError::InvalidFormat(
                "Memory region too short".into(),
            ));
        }

        let gpa = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let size = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let flags = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let compressed = bytes[20] != 0;
        let data_len = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]) as usize;

        if bytes.len() < 25 + data_len {
            return Err(SerializeError::InvalidFormat(
                "Memory data truncated".into(),
            ));
        }

        Ok(Self {
            gpa,
            size,
            flags,
            data: bytes[25..25 + data_len].to_vec(),
            compressed,
        })
    }
}

/// Device state container
#[derive(Debug, Clone)]
pub struct DeviceState {
    /// Device name/identifier
    pub name: String,
    /// Device type
    pub device_type: String,
    /// Serialized state data
    pub data: Vec<u8>,
}

impl DeviceState {
    /// Create a new device state
    pub fn new(name: impl Into<String>, device_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            device_type: device_type.into(),
            data,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.name.as_bytes();
        let type_bytes = self.device_type.as_bytes();
        let mut bytes =
            Vec::with_capacity(8 + name_bytes.len() + type_bytes.len() + self.data.len());

        bytes.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(&(type_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(type_bytes);
        bytes.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.data);

        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> SerializeResult<Self> {
        let mut offset = 0;

        if bytes.len() < 4 {
            return Err(SerializeError::InvalidFormat(
                "Device state too short".into(),
            ));
        }

        let name_len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if bytes.len() < offset + name_len {
            return Err(SerializeError::InvalidFormat(
                "Device name truncated".into(),
            ));
        }
        let name = String::from_utf8_lossy(&bytes[offset..offset + name_len]).into_owned();
        offset += name_len;

        if bytes.len() < offset + 4 {
            return Err(SerializeError::InvalidFormat(
                "Device type length missing".into(),
            ));
        }
        let type_len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if bytes.len() < offset + type_len {
            return Err(SerializeError::InvalidFormat(
                "Device type truncated".into(),
            ));
        }
        let device_type = String::from_utf8_lossy(&bytes[offset..offset + type_len]).into_owned();
        offset += type_len;

        if bytes.len() < offset + 4 {
            return Err(SerializeError::InvalidFormat(
                "Device data length missing".into(),
            ));
        }
        let data_len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if bytes.len() < offset + data_len {
            return Err(SerializeError::InvalidFormat(
                "Device data truncated".into(),
            ));
        }

        Ok(Self {
            name,
            device_type,
            data: bytes[offset..offset + data_len].to_vec(),
        })
    }
}

/// Section header for state file
#[derive(Debug, Clone)]
pub struct SectionHeader {
    /// Section type
    pub section_type: SectionType,
    /// Section name (for custom sections)
    pub name: String,
    /// Data length
    pub length: u64,
    /// Checksum of data
    pub checksum: u32,
}

impl SectionHeader {
    /// Create a new section header
    pub fn new(section_type: SectionType, name: impl Into<String>, length: u64) -> Self {
        Self {
            section_type,
            name: name.into(),
            length,
            checksum: 0,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.name.as_bytes();
        let mut bytes = Vec::with_capacity(20 + name_bytes.len());

        bytes.extend_from_slice(&(self.section_type as u32).to_le_bytes());
        bytes.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(&self.length.to_le_bytes());
        bytes.extend_from_slice(&self.checksum.to_le_bytes());

        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> SerializeResult<(Self, usize)> {
        if bytes.len() < 8 {
            return Err(SerializeError::InvalidFormat(
                "Section header too short".into(),
            ));
        }

        let type_val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let section_type = SectionType::from_u32(type_val).ok_or_else(|| {
            SerializeError::InvalidFormat(format!("Unknown section type: {}", type_val))
        })?;

        let name_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;

        if bytes.len() < 8 + name_len + 12 {
            return Err(SerializeError::InvalidFormat(
                "Section header truncated".into(),
            ));
        }

        let name = String::from_utf8_lossy(&bytes[8..8 + name_len]).into_owned();
        let offset = 8 + name_len;

        let length = u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);

        let checksum = u32::from_le_bytes([
            bytes[offset + 8],
            bytes[offset + 9],
            bytes[offset + 10],
            bytes[offset + 11],
        ]);

        Ok((
            Self {
                section_type,
                name,
                length,
                checksum,
            },
            offset + 12,
        ))
    }
}

/// Complete VM state container
#[derive(Debug, Default)]
pub struct VmState {
    /// CPU states (one per vCPU)
    pub cpus: Vec<CpuState>,
    /// Memory region states
    pub memory: Vec<MemoryRegionState>,
    /// Device states
    pub devices: HashMap<String, DeviceState>,
    /// Custom state sections
    pub custom: HashMap<String, Vec<u8>>,
}

impl VmState {
    /// Create a new empty VM state
    pub fn new() -> Self {
        Self::default()
    }

    /// Add CPU state
    pub fn add_cpu(&mut self, cpu: CpuState) {
        self.cpus.push(cpu);
    }

    /// Add memory region
    pub fn add_memory(&mut self, region: MemoryRegionState) {
        self.memory.push(region);
    }

    /// Add device state
    pub fn add_device(&mut self, device: DeviceState) {
        self.devices.insert(device.name.clone(), device);
    }

    /// Add custom section
    pub fn add_custom(&mut self, name: impl Into<String>, data: Vec<u8>) {
        self.custom.insert(name.into(), data);
    }

    /// Get device state by name
    pub fn get_device(&self, name: &str) -> Option<&DeviceState> {
        self.devices.get(name)
    }

    /// Get custom section by name
    pub fn get_custom(&self, name: &str) -> Option<&[u8]> {
        self.custom.get(name).map(|v| v.as_slice())
    }
}

/// Calculate CRC32 checksum
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// State serializer
#[derive(Debug)]
pub struct StateSerializer {
    /// Accumulated data
    data: Vec<u8>,
}

impl StateSerializer {
    /// Create a new serializer
    pub fn new() -> Self {
        let mut data = Vec::new();
        // Write magic and version
        data.extend_from_slice(&STATE_MAGIC.to_le_bytes());
        data.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        Self { data }
    }

    /// Write a section
    pub fn write_section(&mut self, section_type: SectionType, name: &str, data: &[u8]) {
        let checksum = crc32(data);
        let header = SectionHeader {
            section_type,
            name: name.to_string(),
            length: data.len() as u64,
            checksum,
        };
        self.data.extend_from_slice(&header.to_bytes());
        self.data.extend_from_slice(data);
    }

    /// Write CPU state
    pub fn write_cpu(&mut self, cpu_id: u32, state: &CpuState) {
        let name = format!("cpu{}", cpu_id);
        let data = state.to_bytes();
        self.write_section(SectionType::Cpu, &name, &data);
    }

    /// Write memory region
    pub fn write_memory(&mut self, region: &MemoryRegionState) {
        let name = format!("mem_{:016x}", region.gpa);
        let data = region.to_bytes();
        self.write_section(SectionType::Memory, &name, &data);
    }

    /// Write device state
    pub fn write_device(&mut self, device: &DeviceState) {
        let data = device.to_bytes();
        self.write_section(SectionType::Device, &device.name, &data);
    }

    /// Finish serialization and return data
    pub fn finish(self) -> Vec<u8> {
        self.data
    }

    /// Get current size
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

impl Default for StateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

/// State deserializer
#[derive(Debug)]
pub struct StateDeserializer<'a> {
    /// Input data
    data: &'a [u8],
    /// Current offset
    offset: usize,
}

impl<'a> StateDeserializer<'a> {
    /// Create a new deserializer
    pub fn new(data: &'a [u8]) -> SerializeResult<Self> {
        if data.len() < 8 {
            return Err(SerializeError::InvalidFormat("Data too short".into()));
        }

        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != STATE_MAGIC {
            return Err(SerializeError::InvalidFormat(format!(
                "Invalid magic: {:08x}",
                magic
            )));
        }

        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != FORMAT_VERSION {
            return Err(SerializeError::VersionMismatch {
                expected: FORMAT_VERSION,
                found: version,
            });
        }

        Ok(Self { data, offset: 8 })
    }

    /// Read next section
    pub fn read_section(&mut self) -> SerializeResult<Option<(SectionHeader, Vec<u8>)>> {
        if self.offset >= self.data.len() {
            return Ok(None);
        }

        let (header, header_size) = SectionHeader::from_bytes(&self.data[self.offset..])?;
        self.offset += header_size;

        if self.offset + header.length as usize > self.data.len() {
            return Err(SerializeError::InvalidFormat(
                "Section data truncated".into(),
            ));
        }

        let section_data = self.data[self.offset..self.offset + header.length as usize].to_vec();
        self.offset += header.length as usize;

        // Verify checksum
        let checksum = crc32(&section_data);
        if checksum != header.checksum {
            return Err(SerializeError::ChecksumMismatch);
        }

        Ok(Some((header, section_data)))
    }

    /// Read all sections into a VM state
    pub fn read_all(&mut self) -> SerializeResult<VmState> {
        let mut state = VmState::new();

        while let Some((header, data)) = self.read_section()? {
            match header.section_type {
                SectionType::Cpu => {
                    // Parse CPU state (simplified for now)
                    state.add_cpu(CpuState::default());
                }
                SectionType::Memory => {
                    let region = MemoryRegionState::from_bytes(&data)?;
                    state.add_memory(region);
                }
                SectionType::Device => {
                    let device = DeviceState::from_bytes(&data)?;
                    state.add_device(device);
                }
                SectionType::Custom => {
                    state.add_custom(header.name, data);
                }
                _ => {
                    // Skip unknown sections
                }
            }
        }

        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_type_roundtrip() {
        for ty in [
            SectionType::Header,
            SectionType::Cpu,
            SectionType::Memory,
            SectionType::Device,
            SectionType::Timer,
            SectionType::Interrupt,
            SectionType::Custom,
        ] {
            let val = ty as u32;
            assert_eq!(SectionType::from_u32(val), Some(ty));
        }
    }

    #[test]
    fn test_segment_register_roundtrip() {
        let seg = SegmentRegister::new(0x0010, 0x7C00_0000, 0xFFFF_FFFF, 0xC09B);
        let bytes = seg.to_bytes();
        let restored = SegmentRegister::from_bytes(&bytes).unwrap();

        assert_eq!(seg.selector, restored.selector);
        assert_eq!(seg.base, restored.base);
        assert_eq!(seg.limit, restored.limit);
        assert_eq!(seg.access_rights, restored.access_rights);
    }

    #[test]
    fn test_descriptor_table_roundtrip() {
        let dt = DescriptorTable::new(0x1234_5678_9ABC_DEF0, 0x1FFF);
        let bytes = dt.to_bytes();
        let restored = DescriptorTable::from_bytes(&bytes).unwrap();

        assert_eq!(dt.base, restored.base);
        assert_eq!(dt.limit, restored.limit);
    }

    #[test]
    fn test_cpu_state_serialization() {
        let mut cpu = CpuState::new();
        cpu.rax = 0x1234567890ABCDEF;
        cpu.rip = 0x7C00;
        cpu.cr0 = 0x80000011;

        let bytes = cpu.to_bytes();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_memory_region_roundtrip() {
        let region = MemoryRegionState::new(0x1000, 0x4000, 0x07, vec![0xAA; 100]);
        let bytes = region.to_bytes();
        let restored = MemoryRegionState::from_bytes(&bytes).unwrap();

        assert_eq!(region.gpa, restored.gpa);
        assert_eq!(region.size, restored.size);
        assert_eq!(region.flags, restored.flags);
        assert_eq!(region.data, restored.data);
    }

    #[test]
    fn test_device_state_roundtrip() {
        let device = DeviceState::new("serial0", "uart", vec![0x01, 0x02, 0x03]);
        let bytes = device.to_bytes();
        let restored = DeviceState::from_bytes(&bytes).unwrap();

        assert_eq!(device.name, restored.name);
        assert_eq!(device.device_type, restored.device_type);
        assert_eq!(device.data, restored.data);
    }

    #[test]
    fn test_section_header_roundtrip() {
        let header = SectionHeader {
            section_type: SectionType::Device,
            name: "test_device".to_string(),
            length: 1024,
            checksum: 0xDEADBEEF,
        };

        let bytes = header.to_bytes();
        let (restored, size) = SectionHeader::from_bytes(&bytes).unwrap();

        assert_eq!(header.section_type, restored.section_type);
        assert_eq!(header.name, restored.name);
        assert_eq!(header.length, restored.length);
        assert_eq!(header.checksum, restored.checksum);
        assert_eq!(size, bytes.len());
    }

    #[test]
    fn test_crc32() {
        let data = b"Hello, World!";
        let checksum = crc32(data);
        assert_ne!(checksum, 0);

        // Same data should produce same checksum
        assert_eq!(checksum, crc32(data));

        // Different data should produce different checksum
        assert_ne!(checksum, crc32(b"Hello, World?"));
    }

    #[test]
    fn test_serializer_deserializer() {
        let mut serializer = StateSerializer::new();

        // Write some test data
        let device = DeviceState::new("test", "test_type", vec![1, 2, 3, 4]);
        serializer.write_device(&device);

        let data = serializer.finish();

        // Deserialize
        let mut deserializer = StateDeserializer::new(&data).unwrap();
        let (header, section_data) = deserializer.read_section().unwrap().unwrap();

        assert_eq!(header.section_type, SectionType::Device);
        assert_eq!(header.name, "test");

        let restored = DeviceState::from_bytes(&section_data).unwrap();
        assert_eq!(restored.name, "test");
    }

    #[test]
    fn test_vm_state() {
        let mut state = VmState::new();

        state.add_cpu(CpuState::new());
        state.add_memory(MemoryRegionState::new(0, 0x1000, 0x07, vec![0; 0x1000]));
        state.add_device(DeviceState::new("uart0", "uart", vec![]));
        state.add_custom("test", vec![1, 2, 3]);

        assert_eq!(state.cpus.len(), 1);
        assert_eq!(state.memory.len(), 1);
        assert!(state.get_device("uart0").is_some());
        assert!(state.get_custom("test").is_some());
    }

    #[test]
    fn test_invalid_magic() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let result = StateDeserializer::new(&data);
        assert!(matches!(result, Err(SerializeError::InvalidFormat(_))));
    }

    #[test]
    fn test_version_mismatch() {
        let mut data = Vec::new();
        data.extend_from_slice(&STATE_MAGIC.to_le_bytes());
        data.extend_from_slice(&999u32.to_le_bytes()); // Wrong version

        let result = StateDeserializer::new(&data);
        assert!(matches!(
            result,
            Err(SerializeError::VersionMismatch {
                expected: 1,
                found: 999
            })
        ));
    }

    #[test]
    fn test_serialize_multiple_cpus() {
        let mut serializer = StateSerializer::new();

        for i in 0..4 {
            let mut cpu = CpuState::new();
            cpu.rax = i as u64;
            serializer.write_cpu(i, &cpu);
        }

        let data = serializer.finish();
        assert!(data.len() > 8); // More than just header
    }

    #[test]
    fn test_checksum_verification() {
        let mut serializer = StateSerializer::new();
        serializer.write_section(SectionType::Custom, "test", b"test data");
        let mut data = serializer.finish();

        // Corrupt the section data
        if data.len() > 20 {
            let last_idx = data.len() - 1;
            data[last_idx] ^= 0xFF;
        }

        let mut deserializer = StateDeserializer::new(&data).unwrap();
        let result = deserializer.read_section();
        assert!(matches!(result, Err(SerializeError::ChecksumMismatch)));
    }
}
