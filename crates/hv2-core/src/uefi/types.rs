//! UEFI Types and Definitions
//!
//! This module provides core UEFI types including GUIDs, handles,
//! status codes, and memory types as defined in the UEFI specification.

use std::fmt;

/// UEFI GUID (Globally Unique Identifier)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Guid {
    /// First 32 bits
    pub data1: u32,
    /// Next 16 bits
    pub data2: u16,
    /// Next 16 bits
    pub data3: u16,
    /// Last 64 bits (8 bytes)
    pub data4: [u8; 8],
}

impl Guid {
    /// Create a new GUID
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }

    /// Create from bytes (little-endian format)
    pub fn from_bytes(bytes: &[u8; 16]) -> Self {
        Self {
            data1: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            data2: u16::from_le_bytes([bytes[4], bytes[5]]),
            data3: u16::from_le_bytes([bytes[6], bytes[7]]),
            data4: [
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ],
        }
    }

    /// Convert to bytes (little-endian format)
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&self.data1.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.data2.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.data3.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.data4);
        bytes
    }

    /// Check if GUID is null (all zeros)
    pub fn is_null(&self) -> bool {
        self.data1 == 0 && self.data2 == 0 && self.data3 == 0 && self.data4 == [0; 8]
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl Default for Guid {
    fn default() -> Self {
        Self::new(0, 0, 0, [0; 8])
    }
}

/// Well-known UEFI GUIDs
pub mod guids {
    use super::Guid;

    /// EFI_GLOBAL_VARIABLE GUID
    pub const EFI_GLOBAL_VARIABLE: Guid = Guid::new(
        0x8BE4DF61,
        0x93CA,
        0x11D2,
        [0xAA, 0x0D, 0x00, 0xE0, 0x98, 0x03, 0x2B, 0x8C],
    );

    /// EFI_ACPI_TABLE_GUID (ACPI 2.0+)
    pub const EFI_ACPI_TABLE: Guid = Guid::new(
        0x8868E871,
        0xE4F1,
        0x11D3,
        [0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81],
    );

    /// ACPI_TABLE_GUID (ACPI 1.0)
    pub const ACPI_TABLE: Guid = Guid::new(
        0xEB9D2D30,
        0x2D88,
        0x11D3,
        [0x9A, 0x16, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D],
    );

    /// EFI_SMBIOS_TABLE_GUID
    pub const EFI_SMBIOS_TABLE: Guid = Guid::new(
        0xEB9D2D31,
        0x2D88,
        0x11D3,
        [0x9A, 0x16, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D],
    );

    /// EFI_SMBIOS3_TABLE_GUID
    pub const EFI_SMBIOS3_TABLE: Guid = Guid::new(
        0xF2FD1544,
        0x9794,
        0x4A2C,
        [0x99, 0x2E, 0xE5, 0xBB, 0xCF, 0x20, 0xE3, 0x94],
    );

    /// EFI_LOADED_IMAGE_PROTOCOL_GUID
    pub const EFI_LOADED_IMAGE_PROTOCOL: Guid = Guid::new(
        0x5B1B31A1,
        0x9562,
        0x11D2,
        [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );

    /// EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID
    pub const EFI_SIMPLE_FILE_SYSTEM_PROTOCOL: Guid = Guid::new(
        0x0964E5B22,
        0x6459,
        0x11D2,
        [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );

    /// EFI_DEVICE_PATH_PROTOCOL_GUID
    pub const EFI_DEVICE_PATH_PROTOCOL: Guid = Guid::new(
        0x09576E91,
        0x6D3F,
        0x11D2,
        [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );

    /// EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID
    pub const EFI_GRAPHICS_OUTPUT_PROTOCOL: Guid = Guid::new(
        0x9042A9DE,
        0x23DC,
        0x4A38,
        [0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A],
    );

    /// EFI_SIMPLE_TEXT_INPUT_PROTOCOL_GUID
    pub const EFI_SIMPLE_TEXT_INPUT_PROTOCOL: Guid = Guid::new(
        0x387477C1,
        0x69C7,
        0x11D2,
        [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );

    /// EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL_GUID
    pub const EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL: Guid = Guid::new(
        0x387477C2,
        0x69C7,
        0x11D2,
        [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );

    /// EFI_BLOCK_IO_PROTOCOL_GUID
    pub const EFI_BLOCK_IO_PROTOCOL: Guid = Guid::new(
        0x964E5B21,
        0x6459,
        0x11D2,
        [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );

    /// EFI_DISK_IO_PROTOCOL_GUID
    pub const EFI_DISK_IO_PROTOCOL: Guid = Guid::new(
        0xCE345171,
        0xBA0B,
        0x11D2,
        [0x8E, 0x4F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    );
}

/// EFI Status code (UINTN sized)
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Status(pub u64);

impl Status {
    /// Success
    pub const SUCCESS: Status = Status(0);

    // Warning codes (bit 63 = 0, nonzero value)
    /// The string contained some characters that could not be rendered
    pub const WARN_UNKNOWN_GLYPH: Status = Status(1);
    /// The handle was closed, but the file was not deleted
    pub const WARN_DELETE_FAILURE: Status = Status(2);
    /// The handle was closed, but the data to the file was not flushed
    pub const WARN_WRITE_FAILURE: Status = Status(3);
    /// The resulting buffer was too small
    pub const WARN_BUFFER_TOO_SMALL: Status = Status(4);
    /// The data has not been updated within the timeframe
    pub const WARN_STALE_DATA: Status = Status(5);
    /// The resulting buffer contains file system
    pub const WARN_FILE_SYSTEM: Status = Status(6);

    // Error codes (bit 63 = 1)
    const ERROR_BIT: u64 = 1 << 63;

    /// The image failed to load
    pub const LOAD_ERROR: Status = Status(Self::ERROR_BIT | 1);
    /// A parameter was incorrect
    pub const INVALID_PARAMETER: Status = Status(Self::ERROR_BIT | 2);
    /// The operation is not supported
    pub const UNSUPPORTED: Status = Status(Self::ERROR_BIT | 3);
    /// The buffer was not the proper size for the request
    pub const BAD_BUFFER_SIZE: Status = Status(Self::ERROR_BIT | 4);
    /// The buffer is not large enough to hold the requested data
    pub const BUFFER_TOO_SMALL: Status = Status(Self::ERROR_BIT | 5);
    /// There is no data pending upon return
    pub const NOT_READY: Status = Status(Self::ERROR_BIT | 6);
    /// The physical device reported an error while attempting the operation
    pub const DEVICE_ERROR: Status = Status(Self::ERROR_BIT | 7);
    /// The device cannot be written to
    pub const WRITE_PROTECTED: Status = Status(Self::ERROR_BIT | 8);
    /// A resource has run out
    pub const OUT_OF_RESOURCES: Status = Status(Self::ERROR_BIT | 9);
    /// An inconsistency was detected on the file system
    pub const VOLUME_CORRUPTED: Status = Status(Self::ERROR_BIT | 10);
    /// There is no more space on the file system
    pub const VOLUME_FULL: Status = Status(Self::ERROR_BIT | 11);
    /// The device does not contain any medium to perform the operation
    pub const NO_MEDIA: Status = Status(Self::ERROR_BIT | 12);
    /// The medium in the device has changed since the last access
    pub const MEDIA_CHANGED: Status = Status(Self::ERROR_BIT | 13);
    /// The item was not found
    pub const NOT_FOUND: Status = Status(Self::ERROR_BIT | 14);
    /// Access was denied
    pub const ACCESS_DENIED: Status = Status(Self::ERROR_BIT | 15);
    /// The server was not found or did not respond to the request
    pub const NO_RESPONSE: Status = Status(Self::ERROR_BIT | 16);
    /// A mapping to a device does not exist
    pub const NO_MAPPING: Status = Status(Self::ERROR_BIT | 17);
    /// The timeout time expired
    pub const TIMEOUT: Status = Status(Self::ERROR_BIT | 18);
    /// The protocol has not been started
    pub const NOT_STARTED: Status = Status(Self::ERROR_BIT | 19);
    /// The protocol has already been started
    pub const ALREADY_STARTED: Status = Status(Self::ERROR_BIT | 20);
    /// The operation was aborted
    pub const ABORTED: Status = Status(Self::ERROR_BIT | 21);
    /// An ICMP error occurred during the network operation
    pub const ICMP_ERROR: Status = Status(Self::ERROR_BIT | 22);
    /// A TFTP error occurred during the network operation
    pub const TFTP_ERROR: Status = Status(Self::ERROR_BIT | 23);
    /// A protocol error occurred during the network operation
    pub const PROTOCOL_ERROR: Status = Status(Self::ERROR_BIT | 24);
    /// The function encountered an internal version mismatch
    pub const INCOMPATIBLE_VERSION: Status = Status(Self::ERROR_BIT | 25);
    /// The function was not performed due to a security violation
    pub const SECURITY_VIOLATION: Status = Status(Self::ERROR_BIT | 26);
    /// A CRC error was detected
    pub const CRC_ERROR: Status = Status(Self::ERROR_BIT | 27);
    /// Beginning or end of media was reached
    pub const END_OF_MEDIA: Status = Status(Self::ERROR_BIT | 28);
    /// The end of the file was reached
    pub const END_OF_FILE: Status = Status(Self::ERROR_BIT | 31);
    /// The language specified was invalid
    pub const INVALID_LANGUAGE: Status = Status(Self::ERROR_BIT | 32);
    /// The security status of the data is unknown
    pub const COMPROMISED_DATA: Status = Status(Self::ERROR_BIT | 33);
    /// There is an address conflict during address allocation
    pub const IP_ADDRESS_CONFLICT: Status = Status(Self::ERROR_BIT | 34);
    /// A HTTP error occurred during the network operation
    pub const HTTP_ERROR: Status = Status(Self::ERROR_BIT | 35);

    /// Check if status is success
    pub fn is_success(&self) -> bool {
        self.0 == 0
    }

    /// Check if status is a warning
    pub fn is_warning(&self) -> bool {
        self.0 != 0 && (self.0 & Self::ERROR_BIT) == 0
    }

    /// Check if status is an error
    pub fn is_error(&self) -> bool {
        (self.0 & Self::ERROR_BIT) != 0
    }

    /// Get error code (without error bit)
    pub fn code(&self) -> u64 {
        self.0 & !Self::ERROR_BIT
    }
}

impl fmt::Debug for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Status::SUCCESS => write!(f, "EFI_SUCCESS"),
            Status::LOAD_ERROR => write!(f, "EFI_LOAD_ERROR"),
            Status::INVALID_PARAMETER => write!(f, "EFI_INVALID_PARAMETER"),
            Status::UNSUPPORTED => write!(f, "EFI_UNSUPPORTED"),
            Status::BAD_BUFFER_SIZE => write!(f, "EFI_BAD_BUFFER_SIZE"),
            Status::BUFFER_TOO_SMALL => write!(f, "EFI_BUFFER_TOO_SMALL"),
            Status::NOT_READY => write!(f, "EFI_NOT_READY"),
            Status::DEVICE_ERROR => write!(f, "EFI_DEVICE_ERROR"),
            Status::OUT_OF_RESOURCES => write!(f, "EFI_OUT_OF_RESOURCES"),
            Status::NOT_FOUND => write!(f, "EFI_NOT_FOUND"),
            Status::ACCESS_DENIED => write!(f, "EFI_ACCESS_DENIED"),
            Status::TIMEOUT => write!(f, "EFI_TIMEOUT"),
            Status::SECURITY_VIOLATION => write!(f, "EFI_SECURITY_VIOLATION"),
            _ => write!(f, "EFI_STATUS(0x{:016X})", self.0),
        }
    }
}

/// EFI Handle
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct Handle(pub u64);

impl Handle {
    /// Null handle
    pub const NULL: Handle = Handle(0);

    /// Create new handle
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Check if handle is null
    pub fn is_null(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle(0x{:016X})", self.0)
    }
}

/// EFI Memory type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryType {
    /// Not usable
    ReservedMemoryType = 0,
    /// The code portions of a loaded UEFI application
    LoaderCode = 1,
    /// The data portions of a loaded UEFI application
    LoaderData = 2,
    /// The code portions of a loaded Boot Services Driver
    BootServicesCode = 3,
    /// The data portions of a loaded Boot Services Driver
    BootServicesData = 4,
    /// The code portions of a loaded Runtime Services Driver
    RuntimeServicesCode = 5,
    /// The data portions of a loaded Runtime Services Driver
    RuntimeServicesData = 6,
    /// Free (unallocated) memory
    ConventionalMemory = 7,
    /// Memory in which errors have been detected
    UnusableMemory = 8,
    /// Memory that holds the ACPI tables
    AcpiReclaimMemory = 9,
    /// Address space reserved for use by the firmware
    AcpiMemoryNvs = 10,
    /// Used by system firmware to request a memory mapped IO region
    MemoryMappedIo = 11,
    /// System memory-mapped IO region used to translate memory cycles to IO cycles
    MemoryMappedIoPortSpace = 12,
    /// Address space reserved by the firmware for code
    PalCode = 13,
    /// Memory region that supports byte-addressable non-volatility
    PersistentMemory = 14,
}

impl MemoryType {
    /// Check if memory type is available after ExitBootServices
    pub fn is_runtime(&self) -> bool {
        matches!(
            self,
            MemoryType::RuntimeServicesCode
                | MemoryType::RuntimeServicesData
                | MemoryType::AcpiReclaimMemory
                | MemoryType::AcpiMemoryNvs
        )
    }

    /// Check if memory type can be used for general allocation
    pub fn is_conventional(&self) -> bool {
        matches!(
            self,
            MemoryType::ConventionalMemory
                | MemoryType::BootServicesCode
                | MemoryType::BootServicesData
                | MemoryType::LoaderCode
                | MemoryType::LoaderData
        )
    }
}

/// Memory attribute flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryAttribute(pub u64);

impl MemoryAttribute {
    /// Memory cacheability attribute: uncacheable
    pub const UC: MemoryAttribute = MemoryAttribute(0x0000000000000001);
    /// Memory cacheability attribute: write combining
    pub const WC: MemoryAttribute = MemoryAttribute(0x0000000000000002);
    /// Memory cacheability attribute: write through
    pub const WT: MemoryAttribute = MemoryAttribute(0x0000000000000004);
    /// Memory cacheability attribute: write back
    pub const WB: MemoryAttribute = MemoryAttribute(0x0000000000000008);
    /// Memory cacheability attribute: uncacheable, exported
    pub const UCE: MemoryAttribute = MemoryAttribute(0x0000000000000010);
    /// Memory is write-protected
    pub const WP: MemoryAttribute = MemoryAttribute(0x0000000000001000);
    /// Memory is read-protected
    pub const RP: MemoryAttribute = MemoryAttribute(0x0000000000002000);
    /// Memory is execute-protected
    pub const XP: MemoryAttribute = MemoryAttribute(0x0000000000004000);
    /// Memory is non-volatile
    pub const NV: MemoryAttribute = MemoryAttribute(0x0000000000008000);
    /// Memory is more reliable
    pub const MORE_RELIABLE: MemoryAttribute = MemoryAttribute(0x0000000000010000);
    /// Memory is read-only
    pub const RO: MemoryAttribute = MemoryAttribute(0x0000000000020000);
    /// Memory is special purpose
    pub const SP: MemoryAttribute = MemoryAttribute(0x0000000000040000);
    /// Memory is CPU cryptographic protected
    pub const CPU_CRYPTO: MemoryAttribute = MemoryAttribute(0x0000000000080000);
    /// Memory must be mapped by runtime
    pub const RUNTIME: MemoryAttribute = MemoryAttribute(0x8000000000000000);

    /// Check if attribute has flag
    pub fn has(&self, flag: MemoryAttribute) -> bool {
        (self.0 & flag.0) != 0
    }

    /// Combine attributes
    pub fn or(self, other: MemoryAttribute) -> MemoryAttribute {
        MemoryAttribute(self.0 | other.0)
    }
}

impl Default for MemoryAttribute {
    fn default() -> Self {
        MemoryAttribute::WB
    }
}

/// EFI Memory descriptor
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryDescriptor {
    /// Type of memory region
    pub memory_type: u32,
    /// Padding
    pub padding: u32,
    /// Physical address of the first byte
    pub physical_start: u64,
    /// Virtual address of the first byte
    pub virtual_start: u64,
    /// Number of 4KB pages
    pub number_of_pages: u64,
    /// Attributes of the memory region
    pub attribute: u64,
}

impl MemoryDescriptor {
    /// Memory descriptor version
    pub const DESCRIPTOR_VERSION: u32 = 1;

    /// Create new descriptor
    pub fn new(
        memory_type: MemoryType,
        physical_start: u64,
        number_of_pages: u64,
        attribute: MemoryAttribute,
    ) -> Self {
        Self {
            memory_type: memory_type as u32,
            padding: 0,
            physical_start,
            virtual_start: 0,
            number_of_pages,
            attribute: attribute.0,
        }
    }

    /// Get memory type
    pub fn get_memory_type(&self) -> Option<MemoryType> {
        match self.memory_type {
            0 => Some(MemoryType::ReservedMemoryType),
            1 => Some(MemoryType::LoaderCode),
            2 => Some(MemoryType::LoaderData),
            3 => Some(MemoryType::BootServicesCode),
            4 => Some(MemoryType::BootServicesData),
            5 => Some(MemoryType::RuntimeServicesCode),
            6 => Some(MemoryType::RuntimeServicesData),
            7 => Some(MemoryType::ConventionalMemory),
            8 => Some(MemoryType::UnusableMemory),
            9 => Some(MemoryType::AcpiReclaimMemory),
            10 => Some(MemoryType::AcpiMemoryNvs),
            11 => Some(MemoryType::MemoryMappedIo),
            12 => Some(MemoryType::MemoryMappedIoPortSpace),
            13 => Some(MemoryType::PalCode),
            14 => Some(MemoryType::PersistentMemory),
            _ => None,
        }
    }

    /// Get end physical address
    pub fn physical_end(&self) -> u64 {
        self.physical_start + self.number_of_pages * 4096
    }

    /// Get size in bytes
    pub fn size(&self) -> u64 {
        self.number_of_pages * 4096
    }
}

impl Default for MemoryDescriptor {
    fn default() -> Self {
        Self {
            memory_type: MemoryType::ConventionalMemory as u32,
            padding: 0,
            physical_start: 0,
            virtual_start: 0,
            number_of_pages: 0,
            attribute: MemoryAttribute::WB.0,
        }
    }
}

/// EFI Time
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Time {
    /// Year (1900 - 9999)
    pub year: u16,
    /// Month (1 - 12)
    pub month: u8,
    /// Day (1 - 31)
    pub day: u8,
    /// Hour (0 - 23)
    pub hour: u8,
    /// Minute (0 - 59)
    pub minute: u8,
    /// Second (0 - 59)
    pub second: u8,
    /// Padding
    pub pad1: u8,
    /// Nanosecond (0 - 999,999,999)
    pub nanosecond: u32,
    /// Timezone (-1440 to 1440 minutes from UTC, or 0x7FF for unspecified)
    pub timezone: i16,
    /// Daylight savings time flags
    pub daylight: u8,
    /// Padding
    pub pad2: u8,
}

impl Time {
    /// Unspecified timezone
    pub const UNSPECIFIED_TIMEZONE: i16 = 0x7FF;

    /// Daylight: adjust for daylight saving time
    pub const ADJUST_DAYLIGHT: u8 = 0x01;
    /// Daylight: currently in daylight saving time
    pub const IN_DAYLIGHT: u8 = 0x02;

    /// Create new time
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            pad1: 0,
            nanosecond: 0,
            timezone: Self::UNSPECIFIED_TIMEZONE,
            daylight: 0,
            pad2: 0,
        }
    }

    /// Create current time (for testing)
    pub fn now() -> Self {
        // Return a fixed time for deterministic testing
        Self::new(2025, 1, 23, 12, 0, 0)
    }
}

/// EFI Time capabilities
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TimeCapabilities {
    /// Resolution in counts per second (1 = 1Hz)
    pub resolution: u32,
    /// Accuracy in parts per million
    pub accuracy: u32,
    /// Whether the time is affected by daylight savings
    pub sets_to_zero: bool,
}

/// EFI Table header
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TableHeader {
    /// Signature identifying the table
    pub signature: u64,
    /// Revision of the table
    pub revision: u32,
    /// Size of entire table including header
    pub header_size: u32,
    /// CRC32 of entire table (zero during calculation)
    pub crc32: u32,
    /// Reserved, must be zero
    pub reserved: u32,
}

impl TableHeader {
    /// EFI System Table signature "IBI SYST"
    pub const SYSTEM_TABLE_SIGNATURE: u64 = 0x5453595320494249;
    /// EFI Boot Services Table signature "BOOTSERV"
    pub const BOOT_SERVICES_SIGNATURE: u64 = 0x56524553544F4F42;
    /// EFI Runtime Services Table signature "RUNTSERV"
    pub const RUNTIME_SERVICES_SIGNATURE: u64 = 0x56524553544E5552;

    /// UEFI 2.10 revision
    pub const UEFI_2_10_REVISION: u32 = (2 << 16) | 100;
    /// UEFI 2.9 revision
    pub const UEFI_2_9_REVISION: u32 = (2 << 16) | 90;
    /// UEFI 2.8 revision
    pub const UEFI_2_8_REVISION: u32 = (2 << 16) | 80;

    /// Create new header
    pub fn new(signature: u64, revision: u32, header_size: u32) -> Self {
        Self {
            signature,
            revision,
            header_size,
            crc32: 0,
            reserved: 0,
        }
    }

    /// Get major version
    pub fn major_version(&self) -> u16 {
        (self.revision >> 16) as u16
    }

    /// Get minor version
    pub fn minor_version(&self) -> u16 {
        (self.revision & 0xFFFF) as u16
    }
}

impl Default for TableHeader {
    fn default() -> Self {
        Self {
            signature: 0,
            revision: TableHeader::UEFI_2_10_REVISION,
            header_size: std::mem::size_of::<TableHeader>() as u32,
            crc32: 0,
            reserved: 0,
        }
    }
}

/// Allocate type for memory allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AllocateType {
    /// Allocate any available range
    AllocateAnyPages = 0,
    /// Allocate range at maximum address
    AllocateMaxAddress = 1,
    /// Allocate range at specified address
    AllocateAddress = 2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guid_creation() {
        let guid = Guid::new(0x12345678, 0x1234, 0x5678, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(guid.data1, 0x12345678);
        assert_eq!(guid.data2, 0x1234);
        assert_eq!(guid.data3, 0x5678);
    }

    #[test]
    fn test_guid_bytes() {
        let guid = Guid::new(
            0x12345678,
            0xABCD,
            0xEF01,
            [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
        );
        let bytes = guid.to_bytes();
        let restored = Guid::from_bytes(&bytes);
        assert_eq!(guid, restored);
    }

    #[test]
    fn test_guid_display() {
        let guid = guids::EFI_GLOBAL_VARIABLE;
        let s = format!("{}", guid);
        assert!(s.contains("8BE4DF61"));
    }

    #[test]
    fn test_guid_is_null() {
        let null = Guid::default();
        assert!(null.is_null());

        let non_null = guids::EFI_ACPI_TABLE;
        assert!(!non_null.is_null());
    }

    #[test]
    fn test_status_success() {
        assert!(Status::SUCCESS.is_success());
        assert!(!Status::SUCCESS.is_error());
        assert!(!Status::SUCCESS.is_warning());
    }

    #[test]
    fn test_status_error() {
        assert!(Status::NOT_FOUND.is_error());
        assert!(!Status::NOT_FOUND.is_success());
        assert_eq!(Status::NOT_FOUND.code(), 14);
    }

    #[test]
    fn test_status_warning() {
        assert!(Status::WARN_BUFFER_TOO_SMALL.is_warning());
        assert!(!Status::WARN_BUFFER_TOO_SMALL.is_error());
        assert!(!Status::WARN_BUFFER_TOO_SMALL.is_success());
    }

    #[test]
    fn test_handle() {
        let null = Handle::NULL;
        assert!(null.is_null());

        let handle = Handle::new(0x1234);
        assert!(!handle.is_null());
    }

    #[test]
    fn test_memory_type() {
        assert!(MemoryType::ConventionalMemory.is_conventional());
        assert!(!MemoryType::ConventionalMemory.is_runtime());

        assert!(MemoryType::RuntimeServicesCode.is_runtime());
        assert!(!MemoryType::RuntimeServicesCode.is_conventional());
    }

    #[test]
    fn test_memory_attribute() {
        let attr = MemoryAttribute::WB.or(MemoryAttribute::RUNTIME);
        assert!(attr.has(MemoryAttribute::WB));
        assert!(attr.has(MemoryAttribute::RUNTIME));
        assert!(!attr.has(MemoryAttribute::UC));
    }

    #[test]
    fn test_memory_descriptor() {
        let desc = MemoryDescriptor::new(
            MemoryType::ConventionalMemory,
            0x100000,
            256,
            MemoryAttribute::WB,
        );
        assert_eq!(desc.physical_start, 0x100000);
        assert_eq!(desc.number_of_pages, 256);
        assert_eq!(desc.size(), 256 * 4096);
        assert_eq!(desc.physical_end(), 0x100000 + 256 * 4096);
    }

    #[test]
    fn test_memory_descriptor_type() {
        let desc =
            MemoryDescriptor::new(MemoryType::BootServicesCode, 0x1000, 1, MemoryAttribute::WB);
        assert_eq!(desc.get_memory_type(), Some(MemoryType::BootServicesCode));
    }

    #[test]
    fn test_time() {
        let time = Time::new(2025, 1, 23, 12, 30, 45);
        assert_eq!(time.year, 2025);
        assert_eq!(time.month, 1);
        assert_eq!(time.day, 23);
        assert_eq!(time.hour, 12);
        assert_eq!(time.minute, 30);
        assert_eq!(time.second, 45);
    }

    #[test]
    fn test_table_header() {
        let header = TableHeader::new(
            TableHeader::SYSTEM_TABLE_SIGNATURE,
            TableHeader::UEFI_2_10_REVISION,
            92,
        );
        assert_eq!(header.major_version(), 2);
        assert_eq!(header.minor_version(), 100);
    }

    #[test]
    fn test_well_known_guids() {
        assert!(!guids::EFI_GLOBAL_VARIABLE.is_null());
        assert!(!guids::EFI_ACPI_TABLE.is_null());
        assert!(!guids::EFI_GRAPHICS_OUTPUT_PROTOCOL.is_null());
    }
}
