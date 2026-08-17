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

    /// Build the module list the `mods_addr` field points at.
    ///
    /// This is *not* the module data — it is the array of descriptors the
    /// kernel walks to find each module. Pointing `mods_addr` straight at the
    /// module bytes instead makes the kernel read the module's contents as if
    /// they were addresses, which is how a kernel silently fails to find its
    /// initrd.
    ///
    /// # Entry Format (16 bytes each)
    ///
    /// ```text
    /// Offset  Size  Field
    /// 0       4     mod_start (physical address of the first byte)
    /// 4       4     mod_end   (physical address of the last byte + 1)
    /// 8       4     string    (physical address of a null-terminated string, or 0)
    /// 12      4     reserved  (must be 0)
    /// ```
    pub fn create_module_list(modules: &[ModulePlacement]) -> Vec<u8> {
        let mut data = Vec::with_capacity(modules.len() * MODULE_ENTRY_SIZE);

        for module in modules {
            data.extend_from_slice(&(module.start as u32).to_le_bytes());
            data.extend_from_slice(&(module.end as u32).to_le_bytes());
            data.extend_from_slice(&(module.cmdline_addr as u32).to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes()); // reserved
        }

        data
    }

    /// Lay out a complete Multiboot boot environment in guest memory.
    ///
    /// Returns the `(guest_physical_address, bytes)` regions a backend must
    /// write, having placed the kernel, every module, the module descriptor
    /// list, the command lines, the memory map, and the `multiboot_info`
    /// structure that ties them together.
    ///
    /// Backends share this so their guest memory images cannot drift apart.
    /// After writing the regions, a backend enters 32-bit protected mode at
    /// [`MultibootLayout::kernel_addr`] with `EAX` =
    /// [`Self::bootloader_magic`] and `EBX` = [`MultibootLayout::info_addr`].
    ///
    /// # Errors
    ///
    /// Returns an error if the kernel fails Multiboot validation, or if the
    /// command lines and module list would not fit in the space the layout
    /// reserves for them.
    pub fn prepare_guest_memory(
        info: &MultibootInfo,
        layout: &MultibootLayout,
    ) -> Result<Vec<(u64, Vec<u8>)>> {
        Self::validate_params(info)?;

        let mut regions: Vec<(u64, Vec<u8>)> = Vec::new();

        // Kernel.
        regions.push((layout.kernel_addr, info.kernel_image.clone()));

        // Module data, each 4 KB aligned, plus the descriptor for each.
        let mut placements = Vec::with_capacity(info.modules.len());
        let mut module_addr = layout.first_module_addr;
        // Module command-line strings are packed together in their own region.
        let mut strings = Vec::new();

        for module in &info.modules {
            let start = module_addr;
            let end = start + module.data.len() as u64;
            regions.push((start, module.data.clone()));

            let cmdline_addr = if module.cmdline.is_empty() {
                0
            } else {
                let addr = layout.module_strings_addr + strings.len() as u64;
                strings.extend_from_slice(module.cmdline.as_bytes());
                strings.push(0);
                addr
            };

            placements.push(ModulePlacement {
                start,
                end,
                cmdline_addr,
            });

            // Align the next module to a 4 KB boundary.
            module_addr = end.div_ceil(4096) * 4096;
        }

        let module_list = Self::create_module_list(&placements);
        if !module_list.is_empty() {
            if layout.module_list_addr + module_list.len() as u64 > layout.module_strings_addr {
                return Err(Error::VM(format!(
                    "{} Multiboot modules need a larger module-list region",
                    info.modules.len()
                )));
            }
            regions.push((layout.module_list_addr, module_list));
        }

        if !strings.is_empty() {
            if layout.module_strings_addr + strings.len() as u64 > layout.first_module_addr {
                return Err(Error::VM(
                    "Multiboot module command lines exceed their reserved region".into(),
                ));
            }
            regions.push((layout.module_strings_addr, strings));
        }

        // Kernel command line.
        if !info.cmdline.is_empty() {
            let mut cmdline = info.cmdline.as_bytes().to_vec();
            cmdline.push(0);
            if layout.cmdline_addr + cmdline.len() as u64 > layout.mmap_addr {
                return Err(Error::VM(
                    "Multiboot kernel command line exceeds its reserved region".into(),
                ));
            }
            regions.push((layout.cmdline_addr, cmdline));
        }

        // Memory map.
        let mmap = Self::create_memory_map(&info.memory_map);
        if layout.mmap_addr + mmap.len() as u64 > layout.module_list_addr {
            return Err(Error::VM(
                "Multiboot memory map exceeds its reserved region".into(),
            ));
        }
        regions.push((layout.mmap_addr, mmap));

        // The info structure last, now that every address it references is fixed.
        let multiboot_info = Self::create_multiboot_info(
            info,
            layout.info_addr,
            layout.cmdline_addr,
            (!placements.is_empty()).then_some(layout.module_list_addr),
            layout.mmap_addr,
        );
        regions.push((layout.info_addr, multiboot_info));

        Ok(regions)
    }
}

/// Size of one entry in the Multiboot module list.
const MODULE_ENTRY_SIZE: usize = 16;

/// Where a module ended up in guest memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModulePlacement {
    /// Physical address of the module's first byte.
    pub start: u64,
    /// Physical address one past the module's last byte.
    pub end: u64,
    /// Physical address of the module's null-terminated command line, or 0.
    pub cmdline_addr: u64,
}

/// Guest physical addresses for the parts of a Multiboot boot environment.
///
/// Every region below 1 MB sits in the conventional-memory hole above the BIOS
/// data area and below the kernel, where no Multiboot kernel expects to be
/// loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultibootLayout {
    /// The `multiboot_info` structure (kernel receives this in `EBX`).
    pub info_addr: u64,
    /// The kernel's null-terminated command line.
    pub cmdline_addr: u64,
    /// The memory map entries.
    pub mmap_addr: u64,
    /// The module descriptor array.
    pub module_list_addr: u64,
    /// Packed null-terminated module command lines.
    pub module_strings_addr: u64,
    /// Where the kernel image is loaded, and where execution begins.
    pub kernel_addr: u64,
    /// Where the first module's data is loaded.
    pub first_module_addr: u64,
}

impl Default for MultibootLayout {
    fn default() -> Self {
        Self {
            info_addr: 0x9000,
            cmdline_addr: 0x9400,
            mmap_addr: 0x9800,
            module_list_addr: 0x9C00,
            module_strings_addr: 0xA000,
            kernel_addr: 0x100000,
            first_module_addr: 0x200000,
        }
    }
}

impl MultibootLayout {
    /// The conventional layout, with the kernel at 1 MB.
    pub fn new() -> Self {
        Self::default()
    }

    /// The same layout with the kernel (and hence the entry point) moved.
    #[must_use]
    pub fn with_kernel_addr(mut self, addr: u64) -> Self {
        self.kernel_addr = addr;
        self
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

    /// Read a little-endian u32 out of the region covering `addr`.
    fn read_u32(regions: &[(u64, Vec<u8>)], addr: u64) -> u32 {
        for (base, data) in regions {
            if addr >= *base && addr + 4 <= *base + data.len() as u64 {
                let offset = (addr - *base) as usize;
                return u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            }
        }
        panic!("no region covers {addr:#x}");
    }

    fn info_with_modules(modules: Vec<MultibootModule>) -> MultibootInfo {
        MultibootInfo {
            kernel_image: create_multiboot_kernel(),
            modules,
            cmdline: "root=/dev/sda1".to_string(),
            ..MultibootInfo::default()
        }
    }

    #[test]
    fn module_list_entries_are_sixteen_bytes_of_addresses() {
        let list = MultibootProtocol::create_module_list(&[
            ModulePlacement {
                start: 0x20_0000,
                end: 0x20_1000,
                cmdline_addr: 0xA000,
            },
            ModulePlacement {
                start: 0x20_1000,
                end: 0x20_1800,
                cmdline_addr: 0,
            },
        ]);

        assert_eq!(list.len(), 32, "two 16-byte entries");
        assert_eq!(
            u32::from_le_bytes(list[0..4].try_into().unwrap()),
            0x20_0000
        );
        assert_eq!(
            u32::from_le_bytes(list[4..8].try_into().unwrap()),
            0x20_1000
        );
        assert_eq!(u32::from_le_bytes(list[8..12].try_into().unwrap()), 0xA000);
        assert_eq!(
            u32::from_le_bytes(list[12..16].try_into().unwrap()),
            0,
            "the reserved word must be zero"
        );
        assert_eq!(
            u32::from_le_bytes(list[24..28].try_into().unwrap()),
            0,
            "a module with no command line gets a null string pointer"
        );
    }

    #[test]
    fn mods_addr_points_at_the_descriptor_list_not_the_module_data() {
        // This is the bug the shared layout exists to prevent: pointing
        // mods_addr at the module bytes makes a kernel read the module's
        // contents as if they were addresses, and it silently finds no initrd.
        let layout = MultibootLayout::default();
        let info = info_with_modules(vec![MultibootModule {
            data: vec![0xAA; 4096],
            cmdline: "initrd".to_string(),
        }]);

        let regions = MultibootProtocol::prepare_guest_memory(&info, &layout).unwrap();

        let mods_count = read_u32(&regions, layout.info_addr + 20);
        let mods_addr = read_u32(&regions, layout.info_addr + 24);
        assert_eq!(mods_count, 1);
        assert_eq!(
            u64::from(mods_addr),
            layout.module_list_addr,
            "mods_addr must reference the descriptor array"
        );
        assert_ne!(
            u64::from(mods_addr),
            layout.first_module_addr,
            "mods_addr must NOT reference the module data itself"
        );

        // And the descriptor must point back at the module's real extent.
        let mod_start = read_u32(&regions, layout.module_list_addr);
        let mod_end = read_u32(&regions, layout.module_list_addr + 4);
        assert_eq!(u64::from(mod_start), layout.first_module_addr);
        assert_eq!(u64::from(mod_end), layout.first_module_addr + 4096);
    }

    #[test]
    fn a_module_command_line_is_reachable_from_its_descriptor() {
        let layout = MultibootLayout::default();
        let info = info_with_modules(vec![MultibootModule {
            data: vec![1, 2, 3],
            cmdline: "initrd.img".to_string(),
        }]);

        let regions = MultibootProtocol::prepare_guest_memory(&info, &layout).unwrap();
        let string_addr = u64::from(read_u32(&regions, layout.module_list_addr + 8));

        let (base, data) = regions
            .iter()
            .find(|(base, data)| string_addr >= *base && string_addr < *base + data.len() as u64)
            .expect("the string address must land inside a written region");
        let offset = (string_addr - *base) as usize;
        let text: Vec<u8> = data[offset..]
            .iter()
            .copied()
            .take_while(|b| *b != 0)
            .collect();

        assert_eq!(String::from_utf8(text).unwrap(), "initrd.img");
    }

    #[test]
    fn modules_are_page_aligned_and_do_not_overlap() {
        let layout = MultibootLayout::default();
        // A module that is not a whole number of pages must still leave the
        // next one page-aligned.
        let info = info_with_modules(vec![
            MultibootModule {
                data: vec![0xAA; 5000],
                cmdline: String::new(),
            },
            MultibootModule {
                data: vec![0xBB; 100],
                cmdline: String::new(),
            },
        ]);

        let regions = MultibootProtocol::prepare_guest_memory(&info, &layout).unwrap();

        let first_start = u64::from(read_u32(&regions, layout.module_list_addr));
        let first_end = u64::from(read_u32(&regions, layout.module_list_addr + 4));
        let second_start = u64::from(read_u32(&regions, layout.module_list_addr + 16));

        assert_eq!(first_start, layout.first_module_addr);
        assert_eq!(first_end, first_start + 5000);
        assert!(
            second_start >= first_end,
            "modules must not overlap: {second_start:#x} < {first_end:#x}"
        );
        assert_eq!(second_start % 4096, 0, "modules must be page aligned");
    }

    #[test]
    fn the_info_structure_points_at_the_cmdline_and_memory_map() {
        let layout = MultibootLayout::default();
        let info = info_with_modules(Vec::new());

        let regions = MultibootProtocol::prepare_guest_memory(&info, &layout).unwrap();

        assert_eq!(
            u64::from(read_u32(&regions, layout.info_addr + 16)),
            layout.cmdline_addr
        );
        assert_eq!(
            u64::from(read_u32(&regions, layout.info_addr + 48)),
            layout.mmap_addr
        );
        assert_eq!(
            read_u32(&regions, layout.info_addr + 20),
            0,
            "no modules means mods_count is zero"
        );

        // The mmap flag (bit 6) and cmdline flag (bit 2) must be set, or the
        // kernel ignores those fields entirely.
        let flags = read_u32(&regions, layout.info_addr);
        assert_ne!(flags & (1 << 2), 0, "cmdline flag");
        assert_ne!(flags & (1 << 6), 0, "mmap flag");
        assert_eq!(
            flags & (1 << 3),
            0,
            "mods flag must be clear with no modules"
        );
    }

    #[test]
    fn prepare_guest_memory_rejects_an_invalid_kernel() {
        let layout = MultibootLayout::default();
        let info = MultibootInfo {
            kernel_image: vec![0u8; 1024], // no Multiboot header
            ..MultibootInfo::default()
        };

        assert!(MultibootProtocol::prepare_guest_memory(&info, &layout).is_err());
    }

    #[test]
    fn prepare_guest_memory_rejects_a_module_list_that_would_not_fit() {
        // The layout reserves 0x9C00..0xA000 — 1 KB, so 64 descriptors.
        let layout = MultibootLayout::default();
        let modules = (0..65)
            .map(|_| MultibootModule {
                data: vec![0u8; 16],
                cmdline: String::new(),
            })
            .collect();

        let err = MultibootProtocol::prepare_guest_memory(&info_with_modules(modules), &layout)
            .expect_err("65 modules must not silently overrun the strings region");
        assert!(err.to_string().contains("module-list"), "got: {err}");
    }

    #[test]
    fn the_kernel_lands_where_the_layout_says() {
        let layout = MultibootLayout::default().with_kernel_addr(0x40_0000);
        let info = info_with_modules(Vec::new());

        let regions = MultibootProtocol::prepare_guest_memory(&info, &layout).unwrap();

        assert!(
            regions
                .iter()
                .any(|(addr, data)| *addr == 0x40_0000 && data.len() == info.kernel_image.len()),
            "the kernel should be written at the relocated address"
        );
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
