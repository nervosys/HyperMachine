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

/// Bit 16 of the header flags. When set, the header carries its own load
/// addresses at offsets 12..32 and they are authoritative; when clear, the
/// image's ELF headers say where it goes. The specification calls this the
/// a.out kludge, because it exists for images whose own format cannot say.
const MULTIBOOT_AOUT_KLUDGE: u32 = 1 << 16;

/// `\x7fELF`, the first four bytes of any ELF file.
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
/// `e_ident[EI_CLASS]` for a 32-bit ELF.
const ELFCLASS32: u8 = 1;
/// `p_type` of a segment that is to be loaded into memory.
const PT_LOAD: u32 = 1;
/// Size of one ELF32 program header entry.
const ELF32_PHENT_SIZE: usize = 32;

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

        // Search for magic number on 4-byte boundaries. Saturating, because an
        // image shorter than a header is a thing a caller can hand us -- and
        // subtracting from a `usize` that is already smaller wraps to a search
        // over the whole address space, which indexes out of bounds and panics
        // rather than reporting an unusable image.
        for offset in (0..search_limit.saturating_sub(12)).step_by(4) {
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
                    let addresses = if flags & MULTIBOOT_AOUT_KLUDGE != 0 {
                        Some(Self::read_addresses(kernel_image, offset)?)
                    } else {
                        None
                    };
                    return Ok(MultibootHeader {
                        offset,
                        flags,
                        checksum,
                        addresses,
                    });
                }
            }
        }

        Err(Error::VM(
            "Multiboot header not found in kernel image".into(),
        ))
    }

    /// Read the five address fields that follow a header with bit 16 set.
    fn read_addresses(image: &[u8], offset: usize) -> Result<MultibootAddresses> {
        const FIELDS: usize = 32;
        if image.len() < offset + FIELDS {
            return Err(Error::VM(format!(
                "Multiboot header at offset {offset:#x} sets the address flag but the image \
                 ends after {} bytes, before the address fields it promises",
                image.len()
            )));
        }
        let word = |at: usize| -> u32 {
            u32::from_le_bytes([
                image[offset + at],
                image[offset + at + 1],
                image[offset + at + 2],
                image[offset + at + 3],
            ])
        };
        Ok(MultibootAddresses {
            header_addr: word(12),
            load_addr: word(16),
            load_end_addr: word(20),
            bss_end_addr: word(24),
            entry_addr: word(28),
        })
    }

    /// Decide where a kernel image is loaded and where execution begins.
    ///
    /// This is the part of the specification that distinguishes a bootloader
    /// from a `memcpy`. A Multiboot image says for itself where it belongs, in
    /// one of two ways, and only the third case -- an image that says nothing --
    /// goes to the conventional 1 MB:
    ///
    /// 1. **The header's own address fields**, when flags bit 16 is set. They
    ///    are authoritative and override everything, which is what lets an
    ///    image in a format with no addresses of its own be loaded correctly.
    /// 2. **The ELF program headers**, otherwise, for an image that is an ELF.
    ///    Each `PT_LOAD` segment goes to its physical address and any tail
    ///    beyond the file contents is zeroed, which is where a `.bss` lives.
    /// 3. **Flat**, for anything else: the whole file at
    ///    [`MultibootLayout::kernel_addr`], entered at its first byte.
    ///
    /// Only the third case used to exist. An ELF was written to 1 MB verbatim
    /// and entered at its first byte -- which is `\x7fELF`, not code -- so a
    /// compiled kernel launched, executed the bytes of its own file header, and
    /// produced nothing. That is the shape of every "it boots and says nothing"
    /// report against this protocol, and it is why an image assembled by hand
    /// as flat bytes worked while anything from a linker did not.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VM`] if no Multiboot header is present, if the address
    /// fields describe a region outside the file, or if the image is an ELF
    /// this loader cannot enter -- a 64-bit one, or one for another machine.
    pub fn place_kernel(image: &[u8], layout: &MultibootLayout) -> Result<KernelPlacement> {
        let header = Self::find_header(image)?;

        if let Some(addresses) = header.addresses {
            return Self::place_by_addresses(image, header.offset, &addresses);
        }

        if let Some(placement) = Self::place_elf32(image)? {
            return Ok(placement);
        }

        Ok(KernelPlacement {
            regions: vec![(layout.kernel_addr, image.to_vec())],
            zeroed: Vec::new(),
            entry: layout.kernel_addr,
        })
    }

    /// Place an image by the address fields in its Multiboot header.
    fn place_by_addresses(
        image: &[u8],
        header_offset: usize,
        addr: &MultibootAddresses,
    ) -> Result<KernelPlacement> {
        // The header sits at `header_addr` in memory and at `header_offset` in
        // the file, so those two differ by a constant -- and that constant is
        // how the file offset of `load_addr` is found. This is the whole trick
        // of the a.out kludge, and getting it backwards loads the image off by
        // the size of whatever precedes its header.
        if addr.load_addr > addr.header_addr {
            return Err(Error::VM(format!(
                "Multiboot header declares load_addr {:#x} above header_addr {:#x}; the \
                 header cannot precede the text it belongs to",
                addr.load_addr, addr.header_addr
            )));
        }
        let delta = (addr.header_addr - addr.load_addr) as usize;
        if delta > header_offset {
            return Err(Error::VM(format!(
                "Multiboot header at file offset {header_offset:#x} declares load_addr \
                 {:#x}, which would begin {} bytes before the start of the file",
                addr.load_addr,
                delta - header_offset
            )));
        }
        let text_offset = header_offset - delta;

        // load_end_addr of zero means "to the end of the file".
        let text_end = if addr.load_end_addr == 0 {
            image.len()
        } else {
            if addr.load_end_addr < addr.load_addr {
                return Err(Error::VM(format!(
                    "Multiboot header declares load_end_addr {:#x} below load_addr {:#x}",
                    addr.load_end_addr, addr.load_addr
                )));
            }
            text_offset + (addr.load_end_addr - addr.load_addr) as usize
        };
        if text_end > image.len() {
            return Err(Error::VM(format!(
                "Multiboot header declares {} bytes of text but the image holds {} from \
                 that offset",
                text_end - text_offset,
                image.len() - text_offset
            )));
        }

        let loaded_end = u64::from(addr.load_addr) + (text_end - text_offset) as u64;
        let regions = vec![(
            u64::from(addr.load_addr),
            image[text_offset..text_end].to_vec(),
        )];

        // The .bss, which is in the image's address space but not in its bytes.
        // Recorded as a range that must read as zero rather than as a block of
        // zeros: guest RAM is zero at boot, but a snapshot restored into it is
        // not, and neither is the memory under a second kernel loaded over the
        // first. Only the caller knows which case it is in, so the caller is
        // told rather than guessed at.
        let mut zeroed = Vec::new();
        if u64::from(addr.bss_end_addr) > loaded_end {
            zeroed.push((loaded_end, u64::from(addr.bss_end_addr) - loaded_end));
        }

        Ok(KernelPlacement {
            regions,
            zeroed,
            entry: u64::from(addr.entry_addr),
        })
    }

    /// Place an ELF32 image by its program headers, or `None` if it is not one.
    ///
    /// Segments go to `p_paddr`, not `p_vaddr`: a kernel is linked for the
    /// virtual addresses it will use once it has paging, and it is loaded at
    /// the physical ones it has before that. For an identity-linked kernel the
    /// two agree, which is exactly why using the wrong one is a mistake that
    /// survives testing until someone links a higher-half kernel.
    fn place_elf32(image: &[u8]) -> Result<Option<KernelPlacement>> {
        if image.len() < 52 || image[..4] != ELF_MAGIC {
            return Ok(None);
        }
        if image[4] != ELFCLASS32 {
            return Err(Error::VM(
                "Multiboot kernel is a 64-bit ELF; the protocol enters in 32-bit protected \
                 mode, so the image must be an ELF32 or carry the header address fields"
                    .into(),
            ));
        }

        let word = |at: usize| -> u32 {
            u32::from_le_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]])
        };
        let half = |at: usize| -> u16 { u16::from_le_bytes([image[at], image[at + 1]]) };

        let entry = word(0x18);
        let phoff = word(0x1C) as usize;
        let phentsize = half(0x2A) as usize;
        let phnum = half(0x2C) as usize;

        if phnum == 0 {
            return Err(Error::VM(
                "Multiboot kernel is an ELF32 with no program headers, so nothing says what \
                 to load"
                    .into(),
            ));
        }
        if phentsize < ELF32_PHENT_SIZE {
            return Err(Error::VM(format!(
                "Multiboot kernel declares {phentsize}-byte program headers; ELF32 requires \
                 at least {ELF32_PHENT_SIZE}"
            )));
        }
        let table_end = phoff.saturating_add(phnum.saturating_mul(phentsize));
        if table_end > image.len() {
            return Err(Error::VM(format!(
                "Multiboot kernel's program header table runs to {table_end:#x}, past the \
                 end of a {}-byte image",
                image.len()
            )));
        }

        let mut regions = Vec::new();
        let mut zeroed = Vec::new();
        for i in 0..phnum {
            let ph = phoff + i * phentsize;
            if word(ph) != PT_LOAD {
                continue;
            }
            let offset = word(ph + 4) as usize;
            let paddr = u64::from(word(ph + 12));
            let filesz = word(ph + 16) as usize;
            let memsz = u64::from(word(ph + 20));

            let end = offset.saturating_add(filesz);
            if end > image.len() {
                return Err(Error::VM(format!(
                    "Multiboot kernel's segment {i} claims {filesz} bytes at file offset \
                     {offset:#x}, past the end of a {}-byte image",
                    image.len()
                )));
            }
            if filesz > 0 {
                regions.push((paddr, image[offset..end].to_vec()));
            }
            // Anything the segment occupies beyond its file contents is .bss,
            // and is a range rather than a block of zeros. See `zeroed`.
            if memsz > filesz as u64 {
                zeroed.push((paddr + filesz as u64, memsz - filesz as u64));
            }
        }

        if regions.is_empty() {
            return Err(Error::VM(
                "Multiboot kernel is an ELF32 with no PT_LOAD segments, so there is nothing \
                 to execute"
                    .into(),
            ));
        }

        Ok(Some(KernelPlacement {
            regions,
            zeroed,
            entry: u64::from(entry),
        }))
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

        // Kernel, wherever the image itself says it belongs. Its `.bss` is
        // materialised here, because this function's contract is every byte the
        // guest should see; `LoadedBoot::zero_ranges` is how a caller asks for
        // the cheaper form.
        let placed = Self::place_kernel(&info.kernel_image, layout)?;
        regions.extend(placed.regions);
        for (addr, len) in placed.zeroed {
            regions.push((addr, vec![0u8; len as usize]));
        }

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

        Self::reject_overlaps(&regions)?;

        Ok(regions)
    }

    /// Refuse a set of regions in which any two overlap.
    ///
    /// Two regions writing the same address is not a layout question, it is a
    /// kernel silently losing either its own text or the boot information it
    /// was handed -- and the symptom is a guest that starts and does something
    /// inexplicable, which is the most expensive kind of bug this protocol has.
    /// Now that a kernel says for itself where it loads, this is reachable from
    /// the kernel image rather than only from the layout constants, so it is
    /// worth an explicit check that names both regions.
    fn reject_overlaps(regions: &[(u64, Vec<u8>)]) -> Result<()> {
        let mut spans: Vec<(u64, u64)> = regions
            .iter()
            .filter(|(_, data)| !data.is_empty())
            .map(|(addr, data)| (*addr, addr + data.len() as u64))
            .collect();
        spans.sort_unstable();

        for pair in spans.windows(2) {
            let (first_start, first_end) = pair[0];
            let (second_start, second_end) = pair[1];
            if second_start < first_end {
                return Err(Error::VM(format!(
                    "Multiboot regions overlap: {first_start:#x}..{first_end:#x} and \
                     {second_start:#x}..{second_end:#x}. The kernel's own load addresses \
                     collide with the boot environment or with another image."
                )));
            }
        }

        Ok(())
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

    /// The load addresses the header carries, present only when flags bit 16
    /// is set. When present they are authoritative and the image's own format
    /// is not consulted.
    pub addresses: Option<MultibootAddresses>,
}

/// The five address fields a Multiboot header carries when flags bit 16 is set.
///
/// All are physical addresses in the guest, except that `header_addr` is the
/// address the header itself is loaded at -- which is what ties the file's
/// bytes to the addresses the other four describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultibootAddresses {
    /// Address the Multiboot header itself is loaded at.
    pub header_addr: u32,
    /// Address the image's text begins at.
    pub load_addr: u32,
    /// One past the last byte loaded from the file, or 0 for "to the end".
    pub load_end_addr: u32,
    /// One past the last byte of `.bss`, which is zeroed rather than loaded.
    pub bss_end_addr: u32,
    /// Address of the first instruction.
    pub entry_addr: u32,
}

/// Where a kernel image's bytes go and where execution begins.
///
/// Separate from the boot environment around it because the two answer
/// different questions: the layout says where *this loader* puts the things it
/// builds, and this says where *the image* asked to be put.
#[derive(Debug, Clone)]
pub struct KernelPlacement {
    /// `(guest physical address, bytes)` for each part of the loaded image.
    pub regions: Vec<(u64, Vec<u8>)>,
    /// Guest physical address of the first instruction.
    /// `(guest physical address, length)` for each range that must read as
    /// zero — a `.bss`, in other words.
    ///
    /// A range rather than a block of zeros, because the two are not the same
    /// cost. A guest whose memory is already zero needs nothing written here,
    /// and writing it anyway makes every page resident on the host, per guest,
    /// whether or not the guest ever touches it. At a thousand agents that is
    /// the difference between a heap costing nothing and costing its full size
    /// a thousand times.
    pub zeroed: Vec<(u64, u64)>,
    pub entry: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Where an image says it belongs ──────────────────────────────────
    //
    // Only the flat case used to exist: every image was written to 1 MB and
    // entered at its first byte. An ELF's first byte is `\x7fELF`, which
    // decodes as `jns +0x45`, so a compiled kernel jumped 0x45 bytes into its
    // own header and died quietly. These say which of the three shapes is
    // being honoured, so that a regression names itself.

    /// A 12-byte Multiboot header with the given flags, plus any address
    /// fields.
    fn header_bytes(flags: u32, addresses: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        let checksum = 0u32
            .wrapping_sub(MULTIBOOT_HEADER_MAGIC)
            .wrapping_sub(flags);
        out.extend_from_slice(&MULTIBOOT_HEADER_MAGIC.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&checksum.to_le_bytes());
        for value in addresses {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    /// An ELF32 with one `PT_LOAD` segment: `filesz` bytes of `0xCC` at
    /// `paddr`, growing to `memsz` in memory, entered at `entry`.
    fn elf32(paddr: u32, entry: u32, filesz: usize, memsz: u32) -> Vec<u8> {
        const EHSIZE: u32 = 52;
        const PHENTSIZE: u32 = 32;
        let payload_offset = EHSIZE + PHENTSIZE;

        let mut image = Vec::new();
        image.extend_from_slice(&[0x7F, b'E', b'L', b'F']);
        image.extend_from_slice(&[1, 1, 1]); // ELFCLASS32, little-endian, v1
        image.extend_from_slice(&[0u8; 9]);
        image.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        image.extend_from_slice(&3u16.to_le_bytes()); // EM_386
        image.extend_from_slice(&1u32.to_le_bytes());
        image.extend_from_slice(&entry.to_le_bytes());
        image.extend_from_slice(&EHSIZE.to_le_bytes()); // e_phoff
        image.extend_from_slice(&0u32.to_le_bytes()); // e_shoff
        image.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        image.extend_from_slice(&(EHSIZE as u16).to_le_bytes());
        image.extend_from_slice(&(PHENTSIZE as u16).to_le_bytes());
        image.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
        image.extend_from_slice(&40u16.to_le_bytes());
        image.extend_from_slice(&0u16.to_le_bytes());
        image.extend_from_slice(&0u16.to_le_bytes());

        image.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        image.extend_from_slice(&payload_offset.to_le_bytes());
        image.extend_from_slice(&paddr.to_le_bytes()); // p_vaddr
        image.extend_from_slice(&paddr.to_le_bytes()); // p_paddr
        image.extend_from_slice(&(filesz as u32).to_le_bytes());
        image.extend_from_slice(&memsz.to_le_bytes());
        image.extend_from_slice(&5u32.to_le_bytes()); // R+X
        image.extend_from_slice(&0x1000u32.to_le_bytes());

        // The Multiboot header lives inside the loaded segment, as it must.
        let mut payload = header_bytes(0, &[]);
        payload.resize(filesz, 0xCC);
        image.extend_from_slice(&payload);
        image
    }

    #[test]
    fn a_flat_image_goes_where_the_layout_says() {
        let image = create_multiboot_kernel();
        let layout = MultibootLayout::default();

        let placed = MultibootProtocol::place_kernel(&image, &layout).expect("place");

        assert_eq!(placed.entry, layout.kernel_addr);
        assert_eq!(placed.regions.len(), 1);
        assert_eq!(placed.regions[0].0, layout.kernel_addr);
        assert_eq!(placed.regions[0].1, image);
    }

    #[test]
    fn address_fields_override_the_layout() {
        // The header is at file offset 16, and declares itself loaded at
        // 0x300010 — so the byte before it, at file offset 15, is 0x30000F.
        let mut image = vec![0xAAu8; 16];
        image.extend_from_slice(&header_bytes(
            MULTIBOOT_AOUT_KLUDGE,
            &[0x0030_0010, 0x0030_0000, 0, 0, 0x0030_0040],
        ));
        image.resize(96, 0xBB);

        let placed =
            MultibootProtocol::place_kernel(&image, &MultibootLayout::default()).expect("place");

        assert_eq!(
            placed.entry, 0x0030_0040,
            "entry_addr, not the load address"
        );
        assert_eq!(placed.regions.len(), 1);
        assert_eq!(placed.regions[0].0, 0x0030_0000);
        assert_eq!(
            placed.regions[0].1, image,
            "load_end_addr of 0 means the whole file"
        );
    }

    #[test]
    fn a_bss_beyond_the_file_is_zeroed() {
        let mut image = header_bytes(
            MULTIBOOT_AOUT_KLUDGE,
            &[
                0x0010_0000, // header_addr
                0x0010_0000, // load_addr
                0x0010_0020, // load_end_addr — 32 bytes of text
                0x0010_0100, // bss_end_addr  — 224 more of .bss
                0x0010_0000,
            ],
        );
        image.resize(64, 0xCC); // more file than the header says to load

        let placed =
            MultibootProtocol::place_kernel(&image, &MultibootLayout::default()).expect("place");

        assert_eq!(placed.regions.len(), 1, "only the text has bytes");
        assert_eq!(
            placed.regions[0].1.len(),
            32,
            "load_end_addr bounds the text"
        );

        // The .bss is a range, not a block of zeros — which is the whole point:
        // a guest whose memory is already zero pays nothing for it.
        assert_eq!(placed.zeroed, vec![(0x0010_0020, 0xE0)]);
    }

    #[test]
    fn an_elf_is_placed_by_its_program_headers() {
        let image = elf32(0x0030_0000, 0x0030_000C, 64, 64);

        let placed =
            MultibootProtocol::place_kernel(&image, &MultibootLayout::default()).expect("place");

        assert_eq!(placed.entry, 0x0030_000C, "e_entry, not the load address");
        assert_eq!(placed.regions.len(), 1);
        assert_eq!(placed.regions[0].0, 0x0030_0000, "p_paddr");
        assert_eq!(placed.regions[0].1.len(), 64, "p_filesz");
        assert_ne!(
            placed.regions[0].1[..4],
            [0x7F, b'E', b'L', b'F'],
            "the segment is loaded, not the file that contains it"
        );
    }

    #[test]
    fn an_elf_segment_larger_in_memory_than_in_the_file_gets_its_bss() {
        let image = elf32(0x0030_0000, 0x0030_0000, 64, 4096);

        let placed =
            MultibootProtocol::place_kernel(&image, &MultibootLayout::default()).expect("place");

        assert_eq!(placed.regions.len(), 1, "only the file contents have bytes");
        assert_eq!(placed.zeroed, vec![(0x0030_0040, 4096 - 64)]);
    }

    #[test]
    fn a_64_bit_elf_is_refused_rather_than_entered() {
        let mut image = elf32(0x0030_0000, 0x0030_0000, 64, 64);
        image[4] = 2; // ELFCLASS64

        let error = MultibootProtocol::place_kernel(&image, &MultibootLayout::default())
            .expect_err("an ELF64 cannot be entered in 32-bit protected mode");
        assert!(
            error.to_string().contains("64-bit"),
            "the error should say why: {error}"
        );
    }

    #[test]
    fn address_fields_pointing_outside_the_file_are_refused() {
        // header_addr below load_addr: the header would precede its own text.
        let image = header_bytes(
            MULTIBOOT_AOUT_KLUDGE,
            &[0x0010_0000, 0x0010_0010, 0, 0, 0x0010_0010],
        );
        assert!(MultibootProtocol::place_kernel(&image, &MultibootLayout::default()).is_err());

        // A load_addr so far below header_addr that the text starts before the
        // file does.
        let image = header_bytes(
            MULTIBOOT_AOUT_KLUDGE,
            &[0x0010_1000, 0x0010_0000, 0, 0, 0x0010_0000],
        );
        assert!(MultibootProtocol::place_kernel(&image, &MultibootLayout::default()).is_err());
    }

    #[test]
    fn an_image_shorter_than_a_header_is_refused_not_a_panic() {
        // The search used to compute `len - 12` on a `usize`, which wraps for a
        // short image and indexes far out of bounds.
        for len in 0..12usize {
            assert!(MultibootProtocol::find_header(&vec![0u8; len]).is_err());
        }
    }

    #[test]
    fn a_kernel_loaded_over_the_boot_information_is_refused() {
        // 0x9000 is where the multiboot_info structure goes. A kernel that asks
        // to load there would overwrite the very thing EBX points at.
        let mut image = header_bytes(
            MULTIBOOT_AOUT_KLUDGE,
            &[0x0000_9000, 0x0000_9000, 0, 0, 0x0000_9000],
        );
        image.resize(4096, 0);

        let info = MultibootInfo {
            kernel_image: image,
            ..MultibootInfo::default()
        };

        let error = MultibootProtocol::prepare_guest_memory(&info, &MultibootLayout::default())
            .expect_err("a kernel overlapping the boot information must be refused");
        assert!(
            error.to_string().contains("overlap"),
            "the error should name the collision: {error}"
        );
    }

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
