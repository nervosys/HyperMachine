//! Boot Sector and BIOS Boot Support
//!
//! This module provides functionality for loading and executing boot sectors
//! from disk images, supporting traditional BIOS boot sequences.
//!
//! # Boot Sector Overview
//!
//! A boot sector is the first 512 bytes of a bootable disk. It contains:
//! - Boot code (up to 446 bytes for MBR)
//! - Partition table (64 bytes for MBR)
//! - Boot signature (0xAA55 at offset 510)
//!
//! # Boot Sequence
//!
//! 1. BIOS loads boot sector to 0x7C00
//! 2. Sets DL to boot drive number
//! 3. Jumps to 0x0000:0x7C00 in real mode
//! 4. Boot sector loads more code (second stage)
//! 5. Eventually transitions to protected/long mode

use crate::{Error, Result};

/// Boot sector load address (standard BIOS location)
pub const BOOT_SECTOR_ADDR: u64 = 0x7C00;

/// Boot sector size
pub const BOOT_SECTOR_SIZE: usize = 512;

/// Boot signature offset
pub const BOOT_SIG_OFFSET: usize = 510;

/// Boot signature value
pub const BOOT_SIGNATURE: u16 = 0xAA55;

/// MBR partition table offset
pub const MBR_PARTITION_OFFSET: usize = 446;

/// MBR partition entry size
pub const MBR_PARTITION_SIZE: usize = 16;

/// Number of MBR partition entries
pub const MBR_PARTITION_COUNT: usize = 4;

/// BIOS drive numbers
pub mod drives {
    /// First floppy drive
    pub const FLOPPY_A: u8 = 0x00;
    /// Second floppy drive
    pub const FLOPPY_B: u8 = 0x01;
    /// First hard drive
    pub const HDD_0: u8 = 0x80;
    /// Second hard drive
    pub const HDD_1: u8 = 0x81;
    /// CD-ROM drive
    pub const CDROM: u8 = 0xE0;
}

/// Boot drive type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootDriveType {
    /// Floppy disk
    Floppy,
    /// Hard disk
    HardDisk,
    /// CD-ROM (El Torito)
    CdRom,
}

impl BootDriveType {
    /// Get the BIOS drive number for this type
    pub fn drive_number(&self, index: u8) -> u8 {
        match self {
            BootDriveType::Floppy => index,
            BootDriveType::HardDisk => 0x80 + index,
            BootDriveType::CdRom => 0xE0 + index,
        }
    }
}

/// MBR Partition entry
#[derive(Debug, Clone, Copy, Default)]
pub struct MbrPartition {
    /// Boot indicator (0x80 = bootable, 0x00 = not bootable)
    pub bootable: bool,
    /// Starting head
    pub start_head: u8,
    /// Starting sector (bits 0-5) and cylinder high (bits 6-7)
    pub start_sector_cyl: u16,
    /// Partition type
    pub partition_type: u8,
    /// Ending head
    pub end_head: u8,
    /// Ending sector (bits 0-5) and cylinder high (bits 6-7)
    pub end_sector_cyl: u16,
    /// Starting LBA
    pub start_lba: u32,
    /// Total sectors
    pub total_sectors: u32,
}

impl MbrPartition {
    /// Parse partition entry from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }

        Some(Self {
            bootable: data[0] == 0x80,
            start_head: data[1],
            start_sector_cyl: u16::from_le_bytes([data[2], data[3]]),
            partition_type: data[4],
            end_head: data[5],
            end_sector_cyl: u16::from_le_bytes([data[6], data[7]]),
            start_lba: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            total_sectors: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        })
    }

    /// Check if partition is empty/unused
    pub fn is_empty(&self) -> bool {
        self.partition_type == 0 && self.total_sectors == 0
    }

    /// Get starting CHS cylinder
    pub fn start_cylinder(&self) -> u16 {
        ((self.start_sector_cyl >> 6) & 0x03) | ((self.start_sector_cyl >> 8) << 2)
    }

    /// Get starting CHS sector
    pub fn start_sector(&self) -> u8 {
        (self.start_sector_cyl & 0x3F) as u8
    }
}

/// Common partition types
pub mod partition_types {
    pub const EMPTY: u8 = 0x00;
    pub const FAT12: u8 = 0x01;
    pub const FAT16_SMALL: u8 = 0x04;
    pub const EXTENDED: u8 = 0x05;
    pub const FAT16: u8 = 0x06;
    pub const NTFS: u8 = 0x07;
    pub const FAT32: u8 = 0x0B;
    pub const FAT32_LBA: u8 = 0x0C;
    pub const FAT16_LBA: u8 = 0x0E;
    pub const EXTENDED_LBA: u8 = 0x0F;
    pub const LINUX_SWAP: u8 = 0x82;
    pub const LINUX: u8 = 0x83;
    pub const LINUX_EXTENDED: u8 = 0x85;
    pub const LINUX_LVM: u8 = 0x8E;
    pub const GPT_PROTECTIVE: u8 = 0xEE;
    pub const EFI_SYSTEM: u8 = 0xEF;
}

/// Boot sector validation result
#[derive(Debug, Clone)]
pub struct BootSectorInfo {
    /// Whether the boot signature is valid
    pub valid_signature: bool,
    /// Whether this looks like an MBR
    pub is_mbr: bool,
    /// Partition table (if MBR)
    pub partitions: Vec<MbrPartition>,
    /// Active/bootable partition index (if any)
    pub active_partition: Option<usize>,
}

/// Validate and parse a boot sector
pub fn parse_boot_sector(data: &[u8]) -> Result<BootSectorInfo> {
    if data.len() < BOOT_SECTOR_SIZE {
        return Err(Error::VM(format!(
            "Boot sector too small: {} bytes (need {})",
            data.len(),
            BOOT_SECTOR_SIZE
        )));
    }

    // Check boot signature
    let signature = u16::from_le_bytes([data[BOOT_SIG_OFFSET], data[BOOT_SIG_OFFSET + 1]]);
    let valid_signature = signature == BOOT_SIGNATURE;

    // Parse partition table
    let mut partitions = Vec::new();
    let mut active_partition = None;
    let mut has_valid_partitions = false;

    for i in 0..MBR_PARTITION_COUNT {
        let offset = MBR_PARTITION_OFFSET + i * MBR_PARTITION_SIZE;
        if let Some(part) = MbrPartition::from_bytes(&data[offset..]) {
            if !part.is_empty() {
                has_valid_partitions = true;
                if part.bootable && active_partition.is_none() {
                    active_partition = Some(i);
                }
            }
            partitions.push(part);
        }
    }

    Ok(BootSectorInfo {
        valid_signature,
        is_mbr: has_valid_partitions,
        partitions,
        active_partition,
    })
}

/// BIOS Data Area (BDA) layout
///
/// The BDA is located at 0x400-0x4FF and contains system information
/// that the BIOS maintains for the operating system.
pub mod bda {
    /// BDA base address
    pub const BASE: u64 = 0x400;

    /// COM1 port address (offset 0x00)
    pub const COM1_PORT: u64 = BASE + 0x00;
    /// COM2 port address (offset 0x02)
    pub const COM2_PORT: u64 = BASE + 0x02;
    /// COM3 port address (offset 0x04)
    pub const COM3_PORT: u64 = BASE + 0x04;
    /// COM4 port address (offset 0x06)
    pub const COM4_PORT: u64 = BASE + 0x06;
    /// LPT1 port address (offset 0x08)
    pub const LPT1_PORT: u64 = BASE + 0x08;
    /// Equipment flags (offset 0x10)
    pub const EQUIPMENT: u64 = BASE + 0x10;
    /// Memory size in KB (offset 0x13)
    pub const MEMORY_SIZE: u64 = BASE + 0x13;
    /// Keyboard flags (offset 0x17)
    pub const KB_FLAGS: u64 = BASE + 0x17;
    /// Video mode (offset 0x49)
    pub const VIDEO_MODE: u64 = BASE + 0x49;
    /// Number of screen columns (offset 0x4A)
    pub const SCREEN_COLS: u64 = BASE + 0x4A;
    /// Video page size (offset 0x4C)
    pub const VIDEO_PAGE_SIZE: u64 = BASE + 0x4C;
    /// Cursor position (offset 0x50)
    pub const CURSOR_POS: u64 = BASE + 0x50;
    /// Active video page (offset 0x62)
    pub const ACTIVE_PAGE: u64 = BASE + 0x62;
    /// Video I/O port (offset 0x63)
    pub const VIDEO_PORT: u64 = BASE + 0x63;
    /// Timer tick count (offset 0x6C)
    pub const TIMER_TICKS: u64 = BASE + 0x6C;
    /// Number of hard drives (offset 0x75)
    pub const NUM_HDD: u64 = BASE + 0x75;
    /// Keyboard buffer head (offset 0x1A)
    pub const KB_BUFFER_HEAD: u64 = BASE + 0x1A;
    /// Keyboard buffer tail (offset 0x1C)
    pub const KB_BUFFER_TAIL: u64 = BASE + 0x1C;
}

/// Create BIOS Data Area contents
pub fn create_bda(memory_kb: u16, num_hdd: u8) -> Vec<u8> {
    let mut bda = vec![0u8; 256];

    // COM ports (standard addresses)
    bda[0x00..0x02].copy_from_slice(&0x03F8u16.to_le_bytes()); // COM1
    bda[0x02..0x04].copy_from_slice(&0x02F8u16.to_le_bytes()); // COM2

    // Equipment flags
    // Bit 0: floppy present
    // Bit 1: math coprocessor
    // Bits 4-5: initial video mode (2 = 80x25 color)
    // Bits 6-7: number of floppy drives - 1
    let equipment: u16 = 0x0021; // Floppy + VGA
    bda[0x10..0x12].copy_from_slice(&equipment.to_le_bytes());

    // Memory size (in KB, max 640)
    let mem = memory_kb.min(640);
    bda[0x13..0x15].copy_from_slice(&mem.to_le_bytes());

    // Video mode: 3 = 80x25 16-color text
    bda[0x49] = 0x03;

    // Screen columns
    bda[0x4A..0x4C].copy_from_slice(&80u16.to_le_bytes());

    // Video page size
    bda[0x4C..0x4E].copy_from_slice(&4000u16.to_le_bytes()); // 80*25*2

    // Video I/O port
    bda[0x63..0x65].copy_from_slice(&0x03D4u16.to_le_bytes());

    // Number of hard drives
    bda[0x75] = num_hdd;

    // Keyboard buffer (empty, head == tail)
    bda[0x1A..0x1C].copy_from_slice(&0x001Eu16.to_le_bytes());
    bda[0x1C..0x1E].copy_from_slice(&0x001Eu16.to_le_bytes());

    bda
}

/// Extended BIOS Data Area (EBDA) base address
pub const EBDA_BASE: u64 = 0x9FC00;

/// Create a minimal EBDA
pub fn create_ebda() -> Vec<u8> {
    let mut ebda = vec![0u8; 1024];

    // EBDA size in KB (first byte)
    ebda[0] = 1;

    ebda
}

/// Interrupt Vector Table entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IvtEntry {
    /// Offset within segment
    pub offset: u16,
    /// Segment selector
    pub segment: u16,
}

impl IvtEntry {
    /// Create an IVT entry pointing to the given real-mode address
    pub fn new(segment: u16, offset: u16) -> Self {
        Self { segment, offset }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.offset.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.segment.to_le_bytes());
        bytes
    }
}

/// Create a basic Interrupt Vector Table for real mode
///
/// The IVT is located at 0x0000-0x03FF (256 entries * 4 bytes)
pub fn create_basic_ivt() -> Vec<u8> {
    let mut ivt = vec![0u8; 1024];

    // Point all vectors to a dummy handler at F000:FF53 (IRET)
    // This is the traditional BIOS "do nothing" handler location
    let dummy_handler = IvtEntry::new(0xF000, 0xFF53);
    let entry_bytes = dummy_handler.to_bytes();

    for i in 0..256 {
        let offset = i * 4;
        ivt[offset..offset + 4].copy_from_slice(&entry_bytes);
    }

    ivt
}

/// Boot configuration for BIOS boot
#[derive(Debug, Clone)]
pub struct BiosBootConfig {
    /// Boot sector data (512 bytes)
    pub boot_sector: Vec<u8>,
    /// Boot drive type
    pub drive_type: BootDriveType,
    /// Drive index (0 for first drive of type)
    pub drive_index: u8,
    /// Conventional memory size in KB (max 640)
    pub conventional_memory_kb: u16,
    /// Extended memory size in KB
    pub extended_memory_kb: u64,
}

impl Default for BiosBootConfig {
    fn default() -> Self {
        Self {
            boot_sector: vec![0; BOOT_SECTOR_SIZE],
            drive_type: BootDriveType::HardDisk,
            drive_index: 0,
            conventional_memory_kb: 640,
            extended_memory_kb: 15 * 1024, // 15 MB extended
        }
    }
}

/// Memory map for boot setup
#[derive(Debug, Clone)]
pub struct BootMemoryMap {
    /// Memory regions to set up
    pub regions: Vec<BootMemoryRegion>,
}

/// A memory region for boot setup
#[derive(Debug, Clone)]
pub struct BootMemoryRegion {
    /// Guest physical address
    pub address: u64,
    /// Data to write
    pub data: Vec<u8>,
    /// Description
    pub description: &'static str,
}

impl BootMemoryMap {
    /// Create a standard BIOS boot memory map
    pub fn bios_boot(config: &BiosBootConfig) -> Self {
        let mut regions = Vec::new();

        // IVT at 0x0000
        regions.push(BootMemoryRegion {
            address: 0,
            data: create_basic_ivt(),
            description: "Interrupt Vector Table",
        });

        // BDA at 0x0400
        regions.push(BootMemoryRegion {
            address: 0x400,
            data: create_bda(
                config.conventional_memory_kb,
                if config.drive_type == BootDriveType::HardDisk {
                    1
                } else {
                    0
                },
            ),
            description: "BIOS Data Area",
        });

        // Boot sector at 0x7C00
        regions.push(BootMemoryRegion {
            address: BOOT_SECTOR_ADDR,
            data: config.boot_sector.clone(),
            description: "Boot Sector",
        });

        // EBDA at 0x9FC00
        regions.push(BootMemoryRegion {
            address: EBDA_BASE,
            data: create_ebda(),
            description: "Extended BIOS Data Area",
        });

        Self { regions }
    }

    /// Get total memory needed
    pub fn total_size(&self) -> usize {
        self.regions.iter().map(|r| r.data.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_sector_validation() {
        let mut sector = vec![0u8; 512];

        // Invalid without signature
        let info = parse_boot_sector(&sector).unwrap();
        assert!(!info.valid_signature);

        // Add signature
        sector[510] = 0x55;
        sector[511] = 0xAA;
        let info = parse_boot_sector(&sector).unwrap();
        assert!(info.valid_signature);
    }

    #[test]
    fn test_mbr_partition_parse() {
        let mut entry = [0u8; 16];
        entry[0] = 0x80; // Bootable
        entry[4] = partition_types::LINUX;
        entry[8..12].copy_from_slice(&100u32.to_le_bytes()); // Start LBA
        entry[12..16].copy_from_slice(&2048u32.to_le_bytes()); // Size

        let part = MbrPartition::from_bytes(&entry).unwrap();
        assert!(part.bootable);
        assert_eq!(part.partition_type, partition_types::LINUX);
        assert_eq!(part.start_lba, 100);
        assert_eq!(part.total_sectors, 2048);
    }

    #[test]
    fn test_boot_drive_type() {
        assert_eq!(BootDriveType::Floppy.drive_number(0), 0x00);
        assert_eq!(BootDriveType::Floppy.drive_number(1), 0x01);
        assert_eq!(BootDriveType::HardDisk.drive_number(0), 0x80);
        assert_eq!(BootDriveType::HardDisk.drive_number(1), 0x81);
        assert_eq!(BootDriveType::CdRom.drive_number(0), 0xE0);
    }

    #[test]
    fn test_bda_creation() {
        let bda = create_bda(640, 1);

        // Check memory size
        let mem = u16::from_le_bytes([bda[0x13], bda[0x14]]);
        assert_eq!(mem, 640);

        // Check HDD count
        assert_eq!(bda[0x75], 1);

        // Check video mode
        assert_eq!(bda[0x49], 0x03);
    }

    #[test]
    fn test_ivt_creation() {
        let ivt = create_basic_ivt();
        assert_eq!(ivt.len(), 1024);

        // Check first entry
        let offset = u16::from_le_bytes([ivt[0], ivt[1]]);
        let segment = u16::from_le_bytes([ivt[2], ivt[3]]);
        assert_eq!(segment, 0xF000);
        assert_eq!(offset, 0xFF53);
    }

    #[test]
    fn test_ivt_entry() {
        let entry = IvtEntry::new(0xF000, 0x1234);
        let bytes = entry.to_bytes();

        assert_eq!(bytes[0], 0x34);
        assert_eq!(bytes[1], 0x12);
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0xF0);
    }

    #[test]
    fn test_boot_memory_map() {
        let config = BiosBootConfig::default();
        let map = BootMemoryMap::bios_boot(&config);

        // Should have IVT, BDA, boot sector, EBDA
        assert_eq!(map.regions.len(), 4);

        // Check boot sector region
        let boot_region = map
            .regions
            .iter()
            .find(|r| r.address == BOOT_SECTOR_ADDR)
            .unwrap();
        assert_eq!(boot_region.data.len(), BOOT_SECTOR_SIZE);
    }

    #[test]
    fn test_parse_boot_sector_with_partitions() {
        let mut sector = vec![0u8; 512];

        // Add boot signature
        sector[510] = 0x55;
        sector[511] = 0xAA;

        // Add a bootable partition
        sector[446] = 0x80; // Bootable
        sector[450] = partition_types::LINUX;
        sector[454..458].copy_from_slice(&2048u32.to_le_bytes()); // Start LBA
        sector[458..462].copy_from_slice(&1000000u32.to_le_bytes()); // Size

        let info = parse_boot_sector(&sector).unwrap();
        assert!(info.valid_signature);
        assert!(info.is_mbr);
        assert_eq!(info.active_partition, Some(0));
        assert_eq!(info.partitions.len(), 4);
        assert!(info.partitions[0].bootable);
    }

    #[test]
    fn test_empty_partition() {
        let part = MbrPartition::default();
        assert!(part.is_empty());
    }

    #[test]
    fn test_ebda_creation() {
        let ebda = create_ebda();
        assert_eq!(ebda.len(), 1024);
        assert_eq!(ebda[0], 1); // Size in KB
    }
}
