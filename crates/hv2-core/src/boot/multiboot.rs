//! Multiboot specification implementation
//!
//! This module implements the Multiboot 1.0 boot protocol for loading
//! kernels that conform to the Multiboot specification.
//!
//! # Multiboot Overview
//!
//! The Multiboot specification allows bootloaders to load operating systems
//! in a standardized way. Key features:
//!
//! - **Magic number**: Kernel must contain magic value 0x1BADB002
//! - **Boot information**: Passed via multiboot_info structure
//! - **Modules**: Support for loading additional modules
//! - **Memory map**: Provides memory layout to kernel
//!
//! # Boot State
//!
//! When the kernel receives control:
//! - **EAX**: Contains magic value 0x2BADB002
//! - **EBX**: Contains physical address of multiboot_info structure
//! - **CS**: 32-bit code segment with base 0, limit 0xFFFFFFFF
//! - **DS/ES/FS/GS/SS**: 32-bit data segment with base 0, limit 0xFFFFFFFF
//! - **A20 gate**: Enabled
//! - **CR0**: PG bit cleared, PE bit set (protected mode)
//! - **EFLAGS**: IF bit cleared (interrupts disabled)
//!
//! # References
//!
//! - Multiboot Specification 1.0: <https://www.gnu.org/software/grub/manual/multiboot/multiboot.html>

use crate::{Error, Result};

/// Multiboot magic number in kernel header
const MULTIBOOT_HEADER_MAGIC: u32 = 0x1BADB002;

/// Multiboot magic number passed to kernel in EAX
const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002;

/// Multiboot information passed to kernel
#[derive(Debug, Clone)]
pub struct MultibootInfo {
    /// Kernel image bytes (ELF or raw binary)
    pub kernel_image: Vec<u8>,

    /// Additional modules to load
    pub modules: Vec<MultibootModule>,

    /// Kernel command line
    pub cmdline: String,

    /// Memory map entries: (start_address, length)
    pub memory_map: Vec<(u64, u64)>,
}

impl Default for MultibootInfo {
    fn default() -> Self {
        Self {
            kernel_image: Vec::new(),
            modules: Vec::new(),
            cmdline: String::new(),
            memory_map: vec![
                (0, 640 * 1024),                  // Lower memory (0-640KB)
                (1024 * 1024, 127 * 1024 * 1024), // Upper memory (1MB-128MB)
            ],
        }
    }
}

/// Multiboot module information
#[derive(Debug, Clone)]
pub struct MultibootModule {
    /// Module data
    pub data: Vec<u8>,

    /// Module command line/name
    pub cmdline: String,
}

/// Multiboot protocol implementation
pub struct MultibootProtocol;

impl MultibootProtocol {
    /// Search for Multiboot header in kernel image
    ///
    /// The Multiboot header must be in the first 8KB of the kernel image
    /// and must be 32-bit aligned. It contains:
    /// - magic: 0x1BADB002
    /// - flags: feature flags
    /// - checksum: -(magic + flags)
    ///
    /// The sum of magic + flags + checksum must equal zero.
    pub fn find_header(kernel_image: &[u8]) -> Result<MultibootHeader> {
        // Header must be in first 8KB
        let search_limit = kernel_image.len().min(8192);

        // Search for magic number on 4-byte boundaries
        for offset in (0..search_limit - 12).step_by(4) {
            let magic = u32::from_le_bytes([
                kernel_image[offset],
                kernel_image[offset + 1],
                kernel_image[offset + 2],
                kernel_image[offset + 3],
            ]);

            if magic == MULTIBOOT_HEADER_MAGIC {
                let flags = u32::from_le_bytes([
                    kernel_image[offset + 4],
                    kernel_image[offset + 5],
                    kernel_image[offset + 6],
                    kernel_image[offset + 7],
                ]);

                let checksum = u32::from_le_bytes([
                    kernel_image[offset + 8],
                    kernel_image[offset + 9],
                    kernel_image[offset + 10],
                    kernel_image[offset + 11],
                ]);

                // Verify checksum: magic + flags + checksum must equal 0
                let sum = magic.wrapping_add(flags).wrapping_add(checksum);
                if sum == 0 {
                    return Ok(MultibootHeader {
                        offset,
                        flags,
                        checksum,
                    });
                }
            }
        }

        Err(Error::VM(
            "Multiboot header not found in kernel image".into(),
        ))
    }

    /// Create multiboot_info structure
    ///
    /// The multiboot_info structure is passed to the kernel in EBX.
    /// It contains information about the boot environment.
    ///
    /// # Structure Layout (simplified)
    ///
    /// ```text
    /// Offset  Size  Field
    /// 0       4     flags
    /// 4       4     mem_lower (KB of lower memory)
    /// 8       4     mem_upper (KB of upper memory)
    /// 12      4     boot_device
    /// 16      4     cmdline (pointer to command line string)
    /// 20      4     mods_count
    /// 24      4     mods_addr (pointer to module list)
    /// 28-40   -     (symbol table - unused)
    /// 44      4     mmap_length
    /// 48      4     mmap_addr (pointer to memory map)
    /// ```
    pub fn create_multiboot_info(
        info: &MultibootInfo,
        info_addr: u64,
        cmdline_addr: u64,
        mods_addr: Option<u64>,
        mmap_addr: u64,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 1024]; // 1KB for multiboot_info + extras

        // flags: indicate what fields are valid
        let mut flags = 0u32;
        flags |= 1 << 0; // mem_lower and mem_upper valid
        flags |= 1 << 2; // cmdline valid
        flags |= 1 << 6; // mmap valid

        if !info.modules.is_empty() {
            flags |= 1 << 3; // mods valid
        }

        data[0..4].copy_from_slice(&flags.to_le_bytes());

        // mem_lower (KB below 1MB)
        let mem_lower = 640u32; // Standard lower memory
        data[4..8].copy_from_slice(&mem_lower.to_le_bytes());

        // mem_upper (KB above 1MB)
        let mem_upper = if info.memory_map.len() > 1 {
            (info.memory_map[1].1 / 1024) as u32
        } else {
            127 * 1024 // Default 127MB
        };
        data[8..12].copy_from_slice(&mem_upper.to_le_bytes());

        // boot_device (unused, set to 0xFFFFFFFF)
        data[12..16].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());

        // cmdline address
        data[16..20].copy_from_slice(&(cmdline_addr as u32).to_le_bytes());

        // mods_count and mods_addr
        if let Some(mods) = mods_addr {
            data[20..24].copy_from_slice(&(info.modules.len() as u32).to_le_bytes());
            data[24..28].copy_from_slice(&(mods as u32).to_le_bytes());
        }

        // mmap_length and mmap_addr
        let mmap_length = info.memory_map.len() * 24; // Each entry is 24 bytes
        data[44..48].copy_from_slice(&(mmap_length as u32).to_le_bytes());
        data[48..52].copy_from_slice(&(mmap_addr as u32).to_le_bytes());

        data
    }

    /// Create memory map structure
    ///
    /// Each memory map entry describes a region of physical memory.
    ///
    /// # Entry Format
    ///
    /// ```text
    /// Offset  Size  Field
    /// 0       4     size (of this structure minus 4)
    /// 4       8     base_addr (physical address)
    /// 12      8     length (size in bytes)
    /// 20      4     type (1=available, others=reserved)
    /// ```
    pub fn create_memory_map(memory_map: &[(u64, u64)]) -> Vec<u8> {
        let mut data = Vec::with_capacity(memory_map.len() * 24);

        for (base, length) in memory_map {
            // size field (structure size minus 4)
            data.extend_from_slice(&20u32.to_le_bytes());
            // base_addr (u64)
            data.extend_from_slice(&base.to_le_bytes());
            // length (u64)
            data.extend_from_slice(&length.to_le_bytes());
            // type (1 = available RAM)
            data.extend_from_slice(&1u32.to_le_bytes());
        }

        data
    }

    /// Validate multiboot parameters
    pub fn validate_params(info: &MultibootInfo) -> Result<()> {
        // Validate kernel image
        if info.kernel_image.is_empty() {
            return Err(Error::VM("Kernel image is empty".into()));
        }

        // Find and validate header
        let _header = Self::find_header(&info.kernel_image)?;

        // Validate command line length
        if info.cmdline.len() > 4096 {
            return Err(Error::VM(
                "Command line exceeds maximum length of 4KB".into(),
            ));
        }

        // Validate memory map
        if info.memory_map.is_empty() {
            return Err(Error::VM("Memory map is empty".into()));
        }

        Ok(())
    }

    /// Get the bootloader magic value to pass in EAX
    pub const fn bootloader_magic() -> u32 {
        MULTIBOOT_BOOTLOADER_MAGIC
    }
}

/// Parsed Multiboot header information
#[derive(Debug, Clone)]
pub struct MultibootHeader {
    /// Offset of header in kernel image
    pub offset: usize,

    /// Feature flags
    pub flags: u32,

    /// Checksum value
    pub checksum: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_multiboot_kernel() -> Vec<u8> {
        let mut image = vec![0u8; 1024];

        // Place header at offset 0x100
        let offset = 0x100;

        // magic
        image[offset..offset + 4].copy_from_slice(&MULTIBOOT_HEADER_MAGIC.to_le_bytes());

        // flags
        let flags = 0u32;
        image[offset + 4..offset + 8].copy_from_slice(&flags.to_le_bytes());

        // checksum = -(magic + flags)
        let checksum = (-(MULTIBOOT_HEADER_MAGIC as i32 + flags as i32)) as u32;
        image[offset + 8..offset + 12].copy_from_slice(&checksum.to_le_bytes());

        image
    }

    #[test]
    fn test_find_header_valid() {
        let image = create_multiboot_kernel();
        let header = MultibootProtocol::find_header(&image).unwrap();

        assert_eq!(header.offset, 0x100);
        assert_eq!(header.flags, 0);

        // Verify checksum
        let sum = MULTIBOOT_HEADER_MAGIC
            .wrapping_add(header.flags)
            .wrapping_add(header.checksum);
        assert_eq!(sum, 0);
    }

    #[test]
    fn test_find_header_not_found() {
        let image = vec![0u8; 1024];
        assert!(MultibootProtocol::find_header(&image).is_err());
    }

    #[test]
    fn test_find_header_invalid_checksum() {
        let mut image = create_multiboot_kernel();
        // Corrupt the checksum
        image[0x108] = 0xFF;
        assert!(MultibootProtocol::find_header(&image).is_err());
    }

    #[test]
    fn test_create_multiboot_info() {
        let info = MultibootInfo {
            kernel_image: create_multiboot_kernel(),
            modules: Vec::new(),
            cmdline: "root=/dev/sda1".to_string(),
            memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
        };

        let data = MultibootProtocol::create_multiboot_info(
            &info, 0x10000, // info_addr
            0x11000, // cmdline_addr
            None,    // no modules
            0x12000, // mmap_addr
        );

        // Check flags
        let flags = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        assert_eq!(flags & 1, 1); // mem valid
        assert_eq!(flags & (1 << 2), 1 << 2); // cmdline valid
        assert_eq!(flags & (1 << 6), 1 << 6); // mmap valid

        // Check mem_lower
        let mem_lower = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        assert_eq!(mem_lower, 640);

        // Check cmdline address
        let cmdline = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        assert_eq!(cmdline, 0x11000);
    }

    #[test]
    fn test_create_memory_map() {
        let memory_map = vec![(0u64, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)];

        let data = MultibootProtocol::create_memory_map(&memory_map);

        assert_eq!(data.len(), 2 * 24); // 2 entries * 24 bytes each

        // Check first entry
        let size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        assert_eq!(size, 20);

        let base = u64::from_le_bytes([
            data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
        ]);
        assert_eq!(base, 0);

        let length = u64::from_le_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ]);
        assert_eq!(length, 640 * 1024);
    }

    #[test]
    fn test_validate_params() {
        let info = MultibootInfo {
            kernel_image: create_multiboot_kernel(),
            modules: Vec::new(),
            cmdline: "root=/dev/sda1".to_string(),
            memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
        };

        assert!(MultibootProtocol::validate_params(&info).is_ok());
    }

    #[test]
    fn test_validate_params_empty_kernel() {
        let info = MultibootInfo {
            kernel_image: Vec::new(),
            ..Default::default()
        };

        assert!(MultibootProtocol::validate_params(&info).is_err());
    }

    #[test]
    fn test_validate_params_no_header() {
        let info = MultibootInfo {
            kernel_image: vec![0u8; 1024], // No valid header
            ..Default::default()
        };

        assert!(MultibootProtocol::validate_params(&info).is_err());
    }

    #[test]
    fn test_bootloader_magic() {
        assert_eq!(MultibootProtocol::bootloader_magic(), 0x2BADB002);
    }
}
