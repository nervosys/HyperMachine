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
}

impl Default for LinuxBootParams {
    fn default() -> Self {
        Self {
            kernel_image: Vec::new(),
            initrd: None,
            cmdline: String::new(),
            setup_addr: 0x90000,
            kernel_addr: 0x100000,
        }
    }
}

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

        // Copy setup header from kernel image (if available)
        if params.kernel_image.len() >= 0x250 {
            boot_params[0x1F1..0x250].copy_from_slice(&params.kernel_image[0x1F1..0x250]);
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

        boot_params
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

        // 2. Parse header to determine setup vs kernel split
        let header = Self::parse_header(&params.kernel_image)?;

        let mut regions: Vec<(u64, Vec<u8>)> = Vec::new();

        // 3. Initrd (optional) — place at 32 MB (above kernel)
        let (initrd_addr, initrd_size) = if let Some(ref initrd) = params.initrd {
            let addr: u64 = 0x200_0000; // 32 MB
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
        };

        let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();

        // Should have: initrd, boot_params, cmdline, kernel
        assert!(regions.len() >= 3);

        // Initrd at 32 MB
        let (addr, data) = &regions[0];
        assert_eq!(*addr, 0x200_0000);
        assert_eq!(data, &initrd_data);

        // Boot params should reference the initrd
        let (_, boot_params) = &regions[1];
        let initrd_addr = u32::from_le_bytes([
            boot_params[0x218],
            boot_params[0x219],
            boot_params[0x21A],
            boot_params[0x21B],
        ]);
        assert_eq!(initrd_addr, 0x200_0000);
    }

    #[test]
    fn test_prepare_guest_memory_invalid_kernel() {
        let params = LinuxBootParams {
            kernel_image: Vec::new(),
            ..Default::default()
        };
        assert!(LinuxBootProtocol::prepare_guest_memory(&params).is_err());
    }
}
