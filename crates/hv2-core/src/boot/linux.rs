//! Linux boot protocol implementation
//!
//! This module implements the Linux x86 boot protocol for loading and booting
//! Linux kernels in bzImage format.
//!
//! # Boot Protocol Overview
//!
//! The Linux boot protocol is documented in the kernel source at
//! `Documentation/x86/boot.rst`. Key aspects:
//!
//! - **Boot parameters**: Located at a fixed address (typically 0x90000)
//! - **Kernel image**: Loaded at 1MB (0x100000) for bzImage
//! - **Initial ramdisk**: Optional, loaded at high memory
//! - **Command line**: Null-terminated string with boot parameters
//!
//! # Boot Sequence
//!
//! 1. Setup boot parameters structure
//! 2. Load kernel image to memory
//! 3. Load initrd (if present)
//! 4. Setup GDT, IDT, and page tables
//! 5. Configure CPU for protected mode or long mode
//! 6. Set registers and jump to kernel entry point
//!
//! # References
//!
//! - Linux kernel: Documentation/x86/boot.rst
//! - Linux kernel: arch/x86/include/uapi/asm/bootparam.h

use crate::{Error, Result};

/// Linux boot parameters configuration
#[derive(Debug, Clone)]
pub struct LinuxBootParams {
    /// Kernel image bytes (bzImage format)
    pub kernel_image: Vec<u8>,

    /// Optional initial ramdisk
    pub initrd: Option<Vec<u8>>,

    /// Kernel command line
    pub cmdline: String,

    /// Address for boot parameters structure (typically 0x90000)
    pub setup_addr: u64,

    /// Address to load kernel (typically 0x100000)
    pub kernel_addr: u64,

    /// Size of the guest's RAM, in bytes.
    ///
    /// Not decoration: the kernel gets its memory map from `boot_params` and
    /// from nowhere else, because a guest booted this way never runs a BIOS to
    /// ask. Zero here means no map can be built, and
    /// [`LinuxBootProtocol::prepare_guest_memory`] refuses rather than handing
    /// the kernel an empty one.
    pub memory_size: u64,
}

impl Default for LinuxBootParams {
    fn default() -> Self {
        Self {
            kernel_image: Vec::new(),
            initrd: None,
            cmdline: String::new(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
            memory_size: 0,
        }
    }
}

/// First byte of the setup header inside a bzImage.
const SETUP_HEADER_START: usize = 0x1F1;

/// Where the setup header ended before the protocol started growing.
///
/// Used as a floor: an image whose self-reported end is below this is either
/// truncated or lying, and copying less than protocol 2.09 defined would leave
/// fields zero that every kernel reads.
const SETUP_HEADER_MIN_END: usize = 0x250;

/// The furthest the setup header can reach.
///
/// `boot_params` is one page and the `e820` table starts at 0x2D0, so a header
/// claiming to extend past this is not one this loader can honour.
const SETUP_HEADER_MAX_END: usize = 0x2D0;

/// `initrd_addr_max` in the setup header: the highest address the initrd may
/// occupy.
const INITRD_ADDR_MAX_OFFSET: usize = 0x22C;

/// What the boot protocol says to assume when the header predates the field.
const DEFAULT_INITRD_ADDR_MAX: u64 = 0x37FF_FFFF;

/// `init_size` in the setup header: how much room the kernel needs to unpack
/// itself into, starting from where it ends up running.
const INIT_SIZE_OFFSET: usize = 0x260;

/// `pref_address` in the setup header: where a relocatable kernel would rather
/// be placed.
const PREF_ADDRESS_OFFSET: usize = 0x258;

/// Initrd placement is page aligned, as every loader does it.
const INITRD_ALIGN: u64 = 4096;

/// Usable RAM, in the ACPI address range descriptor's numbering.
const E820_RAM: u32 = 1;

/// Number of `e820` entries, as a byte in `boot_params`.
const E820_ENTRIES_OFFSET: usize = 0x1E8;

/// Start of the `e820` table in `boot_params`.
const E820_TABLE_OFFSET: usize = 0x2D0;

/// Bytes per entry: a 64-bit address, a 64-bit size, and a 32-bit type.
const E820_ENTRY_SIZE: usize = 20;

/// Top of conventional memory, below the extended BIOS data area.
const EBDA_START: u64 = 0x9_FC00;

/// Where memory above the legacy hole resumes, and where the kernel loads.
const HIGH_MEMORY_START: u64 = 0x10_0000;

/// Linux boot protocol implementation
pub struct LinuxBootProtocol;

impl LinuxBootProtocol {
    /// Magic signature for Linux boot protocol
    const BOOT_SIGNATURE: u32 = 0x53726448; // "HdrS"

    /// Boot protocol version we support (2.10+)
    const MIN_PROTOCOL_VERSION: u16 = 0x020A;

    /// Parse Linux bzImage header
    ///
    /// The bzImage header contains important boot information including
    /// the protocol version, kernel size, and entry point offset.
    ///
    /// # Header Structure (simplified)
    ///
    /// ```text
    /// Offset  Size    Field
    /// 0x01F1  1       setup_sects (number of 512-byte setup sectors)
    /// 0x01FE  2       boot_flag (0xAA55)
    /// 0x0202  4       header (0x53726448 "HdrS")
    /// 0x0206  2       version (boot protocol version)
    /// 0x0210  4       kernel_alignment
    /// 0x0214  1       relocatable_kernel
    /// 0x0228  4       cmdline_size
    /// 0x0230  8       setup_data
    /// ```
    pub fn parse_header(kernel_image: &[u8]) -> Result<LinuxHeader> {
        if kernel_image.len() < 0x250 {
            return Err(Error::VM(
                "Kernel image too small to contain valid header".into(),
            ));
        }

        // Check boot flag (0xAA55 at offset 0x1FE)
        let boot_flag = u16::from_le_bytes([kernel_image[0x1FE], kernel_image[0x1FF]]);
        if boot_flag != 0xAA55 {
            return Err(Error::VM(format!(
                "Invalid boot flag: expected 0xAA55, got 0x{:04X}",
                boot_flag
            )));
        }

        // Check boot signature ("HdrS" at offset 0x202)
        let signature = u32::from_le_bytes([
            kernel_image[0x202],
            kernel_image[0x203],
            kernel_image[0x204],
            kernel_image[0x205],
        ]);
        if signature != Self::BOOT_SIGNATURE {
            return Err(Error::VM(format!(
                "Invalid boot signature: expected 0x{:08X}, got 0x{:08X}",
                Self::BOOT_SIGNATURE,
                signature
            )));
        }

        // Get protocol version
        let version = u16::from_le_bytes([kernel_image[0x206], kernel_image[0x207]]);
        if version < Self::MIN_PROTOCOL_VERSION {
            return Err(Error::VM(format!(
                "Boot protocol version too old: need >= 2.10, got {}.{:02}",
                version >> 8,
                version & 0xFF
            )));
        }

        // Get setup sectors
        let setup_sects = kernel_image[0x1F1] as usize;
        let setup_size = if setup_sects == 0 {
            4 * 512 // Default to 4 sectors
        } else {
            (setup_sects + 1) * 512 // +1 for boot sector
        };

        Ok(LinuxHeader {
            version,
            setup_size,
            kernel_size: kernel_image.len() - setup_size,
        })
    }

    /// Create boot parameters structure
    ///
    /// This creates the boot_params structure that Linux expects at the
    /// setup_addr location. The structure contains hardware configuration,
    /// memory layout, and command line information.
    pub fn create_boot_params(
        params: &LinuxBootParams,
        initrd_addr: Option<u64>,
        initrd_size: Option<usize>,
    ) -> Vec<u8> {
        // Allocate boot_params structure (4KB)
        let mut boot_params = vec![0u8; 4096];

        // Copy the setup header out of the image.
        //
        // The header does not have a fixed length: it has grown with the boot
        // protocol, and the image says where it ends -- `0x202 + the byte at
        // 0x201`, which is the second half of the `jump` instruction sitting
        // in front of it. Copying a fixed 0x1f1..0x250 was right for protocol
        // 2.09 and silently truncates every version since, which is not a
        // parse error but a set of zeroed fields.
        //
        // `init_size` at 0x260 is the one that bites: the kernel computes its
        // stack pointer from it, so zero puts `%rsp` somewhere unmapped and the
        // guest triple-faults on the first push -- with no console, no
        // exception, and nothing to read but a reset vCPU.
        let header_end = Self::setup_header_end(&params.kernel_image);
        // An image too short to hold a header at all copies nothing rather
        // than slicing backwards. `prepare_guest_memory` rejects those, but
        // this is also called directly.
        if header_end > SETUP_HEADER_START {
            boot_params[SETUP_HEADER_START..header_end]
                .copy_from_slice(&params.kernel_image[SETUP_HEADER_START..header_end]);
        }

        // Set command line pointer (offset 0x228, 4 bytes)
        let cmdline_addr = params.setup_addr + 0x1000; // Place after boot_params
        boot_params[0x228..0x22C].copy_from_slice(&(cmdline_addr as u32).to_le_bytes());

        // Set initrd address and size (if present)
        if let (Some(addr), Some(size)) = (initrd_addr, initrd_size) {
            // ramdisk_image at offset 0x218
            boot_params[0x218..0x21C].copy_from_slice(&(addr as u32).to_le_bytes());
            // ramdisk_size at offset 0x21C
            boot_params[0x21C..0x220].copy_from_slice(&(size as u32).to_le_bytes());
        }

        // Set type_of_loader (offset 0x210) - bootloader ID
        boot_params[0x210] = 0xFF; // Undefined

        // Set loadflags (offset 0x211)
        boot_params[0x211] = 0x81; // LOADED_HIGH | CAN_USE_HEAP

        // The memory map. A guest booted this way runs no BIOS, so there is no
        // INT 15h/E820 for the kernel to call: what is written here is the
        // only account of memory it will ever get. Without it the kernel finds
        // no usable RAM and never reaches the point of being able to say so.
        Self::write_e820_map(&mut boot_params, params.memory_size);

        boot_params
    }

    /// Read a little-endian `u32` out of the setup header.
    fn header_u32(kernel_image: &[u8], offset: usize) -> u64 {
        kernel_image
            .get(offset..offset + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map_or(0, |bytes: [u8; 4]| u64::from(u32::from_le_bytes(bytes)))
    }

    /// Where to put the initrd.
    ///
    /// As high in guest memory as it will go, which is what every real
    /// bootloader does and for a concrete reason: a compressed kernel unpacks
    /// itself into `init_size` bytes starting from where it will run, and that
    /// region is far larger than the compressed image. This used to place the
    /// initrd at a fixed 32 MB, which sits *inside* that region for any kernel
    /// of ordinary size -- decompression then wrote straight over the initrd,
    /// and the kernel reported "invalid magic at start of compressed archive"
    /// about bytes it had overwritten itself.
    ///
    /// The ceiling is the header's `initrd_addr_max` (some kernels cannot
    /// address an initrd anywhere in RAM), clamped to the memory the guest
    /// actually has.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VM`] if the initrd cannot be placed clear of the
    /// kernel's unpack region -- which is a guest that needs more memory, and
    /// is worth saying rather than discovering as a corrupted archive.
    fn initrd_address(params: &LinuxBootParams, initrd_len: u64) -> Result<u64> {
        let image = &params.kernel_image;

        let addr_max = match Self::header_u32(image, INITRD_ADDR_MAX_OFFSET) {
            0 => DEFAULT_INITRD_ADDR_MAX,
            max => max,
        };
        let ceiling = addr_max.min(params.memory_size.saturating_sub(1));

        let addr = (ceiling + 1).checked_sub(initrd_len).ok_or_else(|| {
            Error::VM(format!(
                "an initrd of {initrd_len:#x} bytes does not fit below {ceiling:#x}"
            ))
        })? & !(INITRD_ALIGN - 1);

        // The kernel unpacks itself into init_size bytes from where it runs.
        // Anything landing inside that is overwritten before it is read.
        let unpack_from = match Self::header_u32(image, PREF_ADDRESS_OFFSET) {
            0 => params.kernel_addr,
            pref => pref.min(params.kernel_addr),
        };
        let unpack_end = unpack_from + Self::header_u32(image, INIT_SIZE_OFFSET);

        if addr < unpack_end {
            return Err(Error::VM(format!(
                "an initrd of {initrd_len:#x} bytes has nowhere to go: the kernel unpacks \
                 itself through {unpack_end:#x} and the initrd must end by {ceiling:#x}. \
                 Give the guest more memory"
            )));
        }

        Ok(addr)
    }

    /// Where this image's setup header ends.
    ///
    /// Read from the image rather than assumed: the boot protocol puts the
    /// answer at 0x201, as the displacement of the `jump` that skips the
    /// header. Clamped at both ends, because this is a guest-supplied number
    /// deciding how much gets copied.
    fn setup_header_end(kernel_image: &[u8]) -> usize {
        let claimed = kernel_image
            .get(0x201)
            .map_or(0, |offset| 0x202 + usize::from(*offset));

        claimed
            .clamp(SETUP_HEADER_MIN_END, SETUP_HEADER_MAX_END)
            .min(kernel_image.len())
    }

    /// Write the `e820` memory map into `boot_params`.
    ///
    /// Two entries, which is what a machine with no devices below 4 GB needs:
    /// conventional memory below the EBDA, and everything from 1 MB up. The
    /// legacy hole between them is left out of the map, which is how a region
    /// is reported absent.
    fn write_e820_map(boot_params: &mut [u8], memory_size: u64) {
        let mut entries: Vec<(u64, u64, u32)> = vec![(0, EBDA_START, E820_RAM)];
        if memory_size > HIGH_MEMORY_START {
            entries.push((HIGH_MEMORY_START, memory_size - HIGH_MEMORY_START, E820_RAM));
        }

        for (index, (addr, size, kind)) in entries.iter().enumerate() {
            let at = E820_TABLE_OFFSET + index * E820_ENTRY_SIZE;
            boot_params[at..at + 8].copy_from_slice(&addr.to_le_bytes());
            boot_params[at + 8..at + 16].copy_from_slice(&size.to_le_bytes());
            boot_params[at + 16..at + 20].copy_from_slice(&kind.to_le_bytes());
        }
        boot_params[E820_ENTRIES_OFFSET] = entries.len() as u8;
    }

    /// Boot a Linux kernel
    ///
    /// Prepare guest memory layout for Linux boot.
    ///
    /// Returns a list of `(guest_physical_address, data)` pairs that the
    /// caller must write into guest memory (via `HypervisorVm::map_memory`,
    /// `KvmVm::map_memory`, etc.). This keeps the function backend-agnostic.
    ///
    /// # Memory Layout
    ///
    /// ```text
    /// setup_addr          → boot_params (4 KB)
    /// setup_addr + 0x1000 → command line (null-terminated)
    /// kernel_addr         → kernel (bzImage protected-mode code)
    /// initrd_addr         → initrd (optional, placed at 32 MB)
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the kernel image is invalid or parameters fail
    /// validation.
    pub fn prepare_guest_memory(params: &LinuxBootParams) -> Result<Vec<(u64, Vec<u8>)>> {
        // 1. Validate everything first
        Self::validate_params(params)?;

        // The memory map is written from this, and a kernel handed an empty
        // one does not report the problem: it finds no RAM and stops before it
        // has a console to say so on. `validate_params` cannot check it,
        // because a caller validating an image before a VM exists has no size
        // to give; by here one is required.
        if params.memory_size <= HIGH_MEMORY_START {
            return Err(Error::VM(format!(
                "guest memory size {:#x} leaves no RAM above 1 MB; the e820 map in boot_params would describe nowhere for the kernel to run",
                params.memory_size
            )));
        }

        // 2. Parse header to determine setup vs kernel split
        let header = Self::parse_header(&params.kernel_image)?;

        let mut regions: Vec<(u64, Vec<u8>)> = Vec::new();

        // 3. Initrd (optional) — place at 32 MB (above kernel)
        let (initrd_addr, initrd_size) = if let Some(ref initrd) = params.initrd {
            let addr = Self::initrd_address(params, initrd.len() as u64)?;
            regions.push((addr, initrd.clone()));
            (Some(addr), Some(initrd.len()))
        } else {
            (None, None)
        };

        // 4. Boot parameters structure at setup_addr
        let boot_params = Self::create_boot_params(params, initrd_addr, initrd_size);
        regions.push((params.setup_addr, boot_params));

        // 5. Command line at setup_addr + 0x1000
        let cmdline_addr = params.setup_addr + 0x1000;
        let mut cmdline_bytes = params.cmdline.as_bytes().to_vec();
        cmdline_bytes.push(0); // null-terminate
        regions.push((cmdline_addr, cmdline_bytes));

        // 6. Protected-mode kernel at kernel_addr (skip setup sectors)
        let kernel_offset = header.setup_size;
        if kernel_offset < params.kernel_image.len() {
            let kernel_data = params.kernel_image[kernel_offset..].to_vec();
            regions.push((params.kernel_addr, kernel_data));
        }

        Ok(regions)
    }

    /// Validate boot parameters.
    ///
    /// Checks that the kernel image is valid, addresses are sensible,
    /// and the command line is within limits.
    pub fn validate_params(params: &LinuxBootParams) -> Result<()> {
        // Validate kernel image
        if params.kernel_image.is_empty() {
            return Err(Error::VM("Kernel image is empty".into()));
        }

        // Parse header to validate
        let _header = Self::parse_header(&params.kernel_image)?;

        // Validate addresses
        crate::boot::BootSetup::validate_boot_addresses(
            params.kernel_addr,
            params.kernel_image.len(),
            Some(params.setup_addr),
        )?;

        // Validate command line length (max 4KB)
        if params.cmdline.len() > 4096 {
            return Err(Error::VM(
                "Command line exceeds maximum length of 4KB".into(),
            ));
        }

        Ok(())
    }

    /// Boot a Linux kernel using a memory mapper.
    ///
    /// Combines `prepare_guest_memory()` with a `MemoryMapper` to write all
    /// prepared regions directly into guest memory. This is a convenience
    /// method that avoids the caller needing to iterate over regions manually.
    ///
    /// # Arguments
    ///
    /// * `params` - Linux boot parameters (kernel image, cmdline, etc.)
    /// * `mapper` - Backend-specific memory mapper for writing to guest RAM
    pub fn boot_with_mapper(
        params: &LinuxBootParams,
        mapper: &dyn crate::hypervisor::MemoryMapper,
    ) -> Result<()> {
        let regions = Self::prepare_guest_memory(params)?;

        for (addr, data) in &regions {
            mapper.map_region(*addr, data)?;
        }

        Ok(())
    }
}

/// Parsed Linux kernel header information
#[derive(Debug, Clone)]
pub struct LinuxHeader {
    /// Boot protocol version (e.g., 0x020C for 2.12)
    pub version: u16,

    /// Size of setup code in bytes
    pub setup_size: usize,

    /// Size of compressed kernel in bytes
    pub kernel_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_minimal_bzimage() -> Vec<u8> {
        // Create image large enough for setup sectors + some kernel data
        // setup_sects=4 means 5 sectors total (4 + boot sector) = 2560 bytes
        let mut image = vec![0u8; 4096]; // 4KB total

        // Set boot flag
        image[0x1FE] = 0x55;
        image[0x1FF] = 0xAA;

        // Set boot signature "HdrS"
        image[0x202] = 0x48; // 'H'
        image[0x203] = 0x64; // 'd'
        image[0x204] = 0x72; // 'r'
        image[0x205] = 0x53; // 'S'

        // Set protocol version (2.12)
        image[0x206] = 0x0C;
        image[0x207] = 0x02;

        // Set setup_sects (4 sectors)
        image[0x1F1] = 4;

        image
    }

    #[test]
    fn test_parse_header_valid() {
        let image = create_minimal_bzimage();
        let header = LinuxBootProtocol::parse_header(&image).unwrap();

        assert_eq!(header.version, 0x020C);
        assert_eq!(header.setup_size, 5 * 512); // 4 + 1 for boot sector
        assert_eq!(header.kernel_size, image.len() - (5 * 512));
    }

    #[test]
    fn test_parse_header_too_small() {
        let image = vec![0u8; 100];
        assert!(LinuxBootProtocol::parse_header(&image).is_err());
    }

    #[test]
    fn test_parse_header_invalid_boot_flag() {
        let mut image = create_minimal_bzimage();
        image[0x1FE] = 0x00; // Invalid boot flag
        assert!(LinuxBootProtocol::parse_header(&image).is_err());
    }

    #[test]
    fn test_parse_header_invalid_signature() {
        let mut image = create_minimal_bzimage();
        image[0x202] = 0x00; // Invalid signature
        assert!(LinuxBootProtocol::parse_header(&image).is_err());
    }

    #[test]
    fn test_parse_header_old_protocol() {
        let mut image = create_minimal_bzimage();
        image[0x206] = 0x00; // Version 2.0 (too old)
        image[0x207] = 0x02;
        assert!(LinuxBootProtocol::parse_header(&image).is_err());
    }

    #[test]
    fn test_create_boot_params() {
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            initrd: None,
            cmdline: "console=ttyS0".to_string(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
            memory_size: 256 * 1024 * 1024,
        };

        let boot_params = LinuxBootProtocol::create_boot_params(&params, None, None);

        assert_eq!(boot_params.len(), 4096);

        // Check command line pointer
        let cmdline_addr = u32::from_le_bytes([
            boot_params[0x228],
            boot_params[0x229],
            boot_params[0x22A],
            boot_params[0x22B],
        ]);
        assert_eq!(cmdline_addr, 0x91000); // setup_addr + 0x1000
    }

    #[test]
    fn test_create_boot_params_with_initrd() {
        let params = LinuxBootParams::default();
        let initrd_addr = 0x2000000;
        let initrd_size = 1024 * 1024;

        let boot_params =
            LinuxBootProtocol::create_boot_params(&params, Some(initrd_addr), Some(initrd_size));

        // Check initrd address
        let addr = u32::from_le_bytes([
            boot_params[0x218],
            boot_params[0x219],
            boot_params[0x21A],
            boot_params[0x21B],
        ]);
        assert_eq!(addr as u64, initrd_addr);

        // Check initrd size
        let size = u32::from_le_bytes([
            boot_params[0x21C],
            boot_params[0x21D],
            boot_params[0x21E],
            boot_params[0x21F],
        ]);
        assert_eq!(size as usize, initrd_size);
    }

    #[test]
    fn test_validate_params() {
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            initrd: None,
            cmdline: "console=ttyS0".to_string(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
            memory_size: 256 * 1024 * 1024,
        };

        assert!(LinuxBootProtocol::validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_empty_kernel() {
        let params = LinuxBootParams {
            kernel_image: Vec::new(),
            ..Default::default()
        };

        assert!(LinuxBootProtocol::validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_cmdline_too_long() {
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            cmdline: "x".repeat(5000), // Exceeds 4KB
            ..Default::default()
        };

        assert!(LinuxBootProtocol::validate_params(&params).is_err());
    }

    #[test]
    fn test_prepare_guest_memory_basic() {
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            initrd: None,
            cmdline: "console=ttyS0".to_string(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
            memory_size: 256 * 1024 * 1024,
        };

        let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();

        // Should have: boot_params, cmdline, kernel
        assert!(regions.len() >= 2);

        // Boot params at setup_addr
        let (addr, data) = &regions[0];
        assert_eq!(*addr, 0x90000);
        assert_eq!(data.len(), 4096);

        // Command line at setup_addr + 0x1000
        let (addr, data) = &regions[1];
        assert_eq!(*addr, 0x91000);
        assert!(data.ends_with(&[0])); // null-terminated
        assert!(data.starts_with(b"console=ttyS0"));
    }

    #[test]
    fn test_prepare_guest_memory_with_initrd() {
        let initrd_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            initrd: Some(initrd_data.clone()),
            cmdline: String::new(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
            memory_size: 256 * 1024 * 1024,
        };

        let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();

        // Should have: initrd, boot_params, cmdline, kernel
        assert!(regions.len() >= 3);

        // High in guest memory, not at a fixed address. The address itself is
        // not the contract -- being clear of where the kernel unpacks itself
        // is, and a constant here was what hid the collision that overwrote
        // the initrd.
        let (addr, data) = &regions[0];
        assert_eq!(data, &initrd_data);
        assert!(
            *addr + initrd_data.len() as u64 <= params.memory_size,
            "initrd at {addr:#x} runs past the guest's memory"
        );
        assert_eq!(*addr % 4096, 0, "page aligned");

        // ...and boot_params points at wherever it actually went.
        let (_, boot_params) = &regions[1];
        let recorded = u32::from_le_bytes([
            boot_params[0x218],
            boot_params[0x219],
            boot_params[0x21A],
            boot_params[0x21B],
        ]);
        assert_eq!(
            u64::from(recorded),
            *addr,
            "the kernel looks for the initrd where this field says it is"
        );
    }

    #[test]
    fn test_prepare_guest_memory_invalid_kernel() {
        let params = LinuxBootParams {
            kernel_image: Vec::new(),
            ..Default::default()
        };
        assert!(LinuxBootProtocol::prepare_guest_memory(&params).is_err());
    }
    /// Read one `e820` entry out of `boot_params`.
    fn e820_entry(boot_params: &[u8], index: usize) -> (u64, u64, u32) {
        let at = E820_TABLE_OFFSET + index * E820_ENTRY_SIZE;
        let addr = u64::from_le_bytes(boot_params[at..at + 8].try_into().unwrap());
        let size = u64::from_le_bytes(boot_params[at + 8..at + 16].try_into().unwrap());
        let kind = u32::from_le_bytes(boot_params[at + 16..at + 20].try_into().unwrap());
        (addr, size, kind)
    }

    #[test]
    fn boot_params_carry_a_memory_map_the_kernel_can_read() {
        // The field this asserts on was zero for the life of the module. A
        // kernel handed a zero here finds no RAM and stops before it has a
        // console to say so on, so the symptom is a guest that produces
        // nothing -- indistinguishable from one that never started.
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            memory_size: 512 * 1024 * 1024,
            ..LinuxBootParams::default()
        };

        let boot_params = LinuxBootProtocol::create_boot_params(&params, None, None);

        assert_eq!(
            boot_params[E820_ENTRIES_OFFSET], 2,
            "the kernel reads the entry count from this byte and believes it"
        );
        assert_eq!(
            e820_entry(&boot_params, 0),
            (0, EBDA_START, E820_RAM),
            "conventional memory, ending below the EBDA"
        );
        assert_eq!(
            e820_entry(&boot_params, 1),
            (
                HIGH_MEMORY_START,
                512 * 1024 * 1024 - HIGH_MEMORY_START,
                E820_RAM
            ),
            "everything from 1 MB up, which is where the kernel is loaded"
        );
    }

    #[test]
    fn the_memory_map_stops_where_the_guest_memory_does() {
        // Reporting RAM a guest does not have is worse than reporting less
        // than it has: the kernel will use it.
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            memory_size: 64 * 1024 * 1024,
            ..LinuxBootParams::default()
        };
        let boot_params = LinuxBootProtocol::create_boot_params(&params, None, None);
        let (addr, size, _) = e820_entry(&boot_params, 1);
        assert_eq!(addr + size, 64 * 1024 * 1024);
    }

    #[test]
    fn a_guest_with_no_memory_above_1mb_is_refused() {
        // There is nowhere to load the kernel, and no e820 map that could say
        // otherwise. Refusing here beats handing over an empty map.
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(),
            memory_size: 512 * 1024,
            ..LinuxBootParams::default()
        };
        let err = LinuxBootProtocol::prepare_guest_memory(&params)
            .expect_err("512 KiB of guest RAM cannot hold a kernel");
        assert!(err.to_string().contains("e820"), "got: {err}");
    }
    #[test]
    fn the_whole_setup_header_is_copied_not_the_first_version_of_it() {
        // The header has grown with the protocol and the image says where it
        // ends. Copying a fixed 0x250 truncates every version since 2.09 --
        // not as a parse error, but as fields that read back zero. init_size
        // at 0x260 is the one that matters: the kernel computes its stack
        // pointer from it, and zero puts %rsp somewhere unmapped.
        let mut kernel = create_minimal_bzimage();
        kernel.resize(0x400, 0);
        kernel[0x201] = 0x6A; // header ends at 0x26c, as a modern kernel says
        kernel[0x260..0x264].copy_from_slice(&0x03BE_2000u32.to_le_bytes());

        let params = LinuxBootParams {
            kernel_image: kernel,
            memory_size: 256 * 1024 * 1024,
            ..LinuxBootParams::default()
        };
        let boot_params = LinuxBootProtocol::create_boot_params(&params, None, None);

        let init_size = u32::from_le_bytes(boot_params[0x260..0x264].try_into().unwrap());
        assert_eq!(
            init_size, 0x03BE_2000,
            "init_size reached the guest as {init_size:#x}"
        );
    }

    #[test]
    fn a_header_extent_the_image_invents_is_clamped() {
        // The byte at 0x201 is guest-supplied and decides how much gets
        // copied, so it is bounded at both ends rather than trusted.
        let mut kernel = create_minimal_bzimage();
        kernel.resize(0x400, 0);

        kernel[0x201] = 0xFF; // claims the header runs to 0x301
        assert_eq!(
            LinuxBootProtocol::setup_header_end(&kernel),
            SETUP_HEADER_MAX_END
        );

        kernel[0x201] = 0x00; // claims it ends before it starts
        assert_eq!(
            LinuxBootProtocol::setup_header_end(&kernel),
            SETUP_HEADER_MIN_END
        );
    }

    #[test]
    fn a_short_image_is_not_read_past_its_end() {
        let mut kernel = create_minimal_bzimage();
        kernel.truncate(0x220);
        kernel[0x201] = 0x6A;
        assert_eq!(LinuxBootProtocol::setup_header_end(&kernel), 0x220);
    }
    #[test]
    fn an_initrd_is_placed_clear_of_where_the_kernel_unpacks_itself() {
        // The defect this replaces: a fixed 32 MB address that sits inside the
        // unpack region of any ordinary kernel, so decompression overwrote the
        // initrd and the kernel reported invalid magic about bytes it had
        // destroyed itself.
        let mut kernel = create_minimal_bzimage();
        kernel.resize(0x400, 0);
        kernel[0x201] = 0x6A;
        kernel[PREF_ADDRESS_OFFSET..PREF_ADDRESS_OFFSET + 4]
            .copy_from_slice(&0x0100_0000u32.to_le_bytes()); // runs at 16 MB
        kernel[INIT_SIZE_OFFSET..INIT_SIZE_OFFSET + 4]
            .copy_from_slice(&0x03BE_2000u32.to_le_bytes()); // needs 62 MB

        let params = LinuxBootParams {
            kernel_image: kernel,
            initrd: Some(vec![0xAB; 4096]),
            memory_size: 512 * 1024 * 1024,
            ..LinuxBootParams::default()
        };

        let addr = LinuxBootProtocol::initrd_address(&params, 4096).unwrap();
        assert!(
            addr >= 0x0100_0000 + 0x03BE_2000,
            "{addr:#x} lands inside the unpack region"
        );
        assert_eq!(addr % INITRD_ALIGN, 0, "initrd placement is page aligned");
        assert!(
            addr + 4096 <= 512 * 1024 * 1024,
            "{addr:#x} runs past guest RAM"
        );
    }

    #[test]
    fn an_initrd_with_nowhere_to_go_is_refused_rather_than_overwritten() {
        // Silently placing it somewhere it will be destroyed is how this
        // failed before; the caller needs to hear "give the guest more
        // memory", not debug a corrupted archive.
        let mut kernel = create_minimal_bzimage();
        kernel.resize(0x400, 0);
        kernel[0x201] = 0x6A;
        kernel[INIT_SIZE_OFFSET..INIT_SIZE_OFFSET + 4]
            .copy_from_slice(&0x0400_0000u32.to_le_bytes()); // needs 64 MB

        let params = LinuxBootParams {
            kernel_image: kernel,
            initrd: Some(vec![0xAB; 1024]),
            memory_size: 32 * 1024 * 1024,
            ..LinuxBootParams::default()
        };

        let err = LinuxBootProtocol::initrd_address(&params, 1024).unwrap_err();
        assert!(err.to_string().contains("more memory"), "got: {err}");
    }

    #[test]
    fn the_initrd_ceiling_honours_what_the_kernel_can_address() {
        // A kernel that cannot address an initrd across all of RAM says so in
        // its header, and ignoring that puts the initrd where it will never be
        // found.
        let mut kernel = create_minimal_bzimage();
        kernel.resize(0x400, 0);
        kernel[0x201] = 0x6A;
        kernel[INITRD_ADDR_MAX_OFFSET..INITRD_ADDR_MAX_OFFSET + 4]
            .copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes()); // 256 MB ceiling

        let params = LinuxBootParams {
            kernel_image: kernel,
            initrd: Some(vec![0xAB; 4096]),
            memory_size: 2 * 1024 * 1024 * 1024,
            ..LinuxBootParams::default()
        };

        let addr = LinuxBootProtocol::initrd_address(&params, 4096).unwrap();
        assert!(addr + 4096 <= 0x1000_0000, "{addr:#x} is above the ceiling");
    }
}
