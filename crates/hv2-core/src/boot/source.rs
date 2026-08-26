//! Declarative boot sources for a VM.
//!
//! [`BootSource`] is the *configuration* half of booting: a serializable
//! description of what a VM should boot, suitable for a TOML file, a REST
//! request body, or an agent tool call. [`LoadedBoot`] is the *resolved* half:
//! the same description with every image read off disk and validated, ready to
//! hand to a hypervisor backend.
//!
//! Splitting the two keeps file I/O at the edge — a backend receives bytes it
//! can write straight into guest memory and never touches the filesystem, so
//! the boot path is testable without a kernel image on disk.
//!
//! # Example
//!
//! ```no_run
//! use hv2_core::boot::source::BootSource;
//!
//! # fn example() -> hv2_core::Result<()> {
//! let source = BootSource::linux("/boot/vmlinuz")
//!     .with_initrd("/boot/initrd.img")
//!     .with_cmdline("console=ttyS0 root=/dev/vda");
//!
//! // Read the images and validate the kernel header.
//! let loaded = source.load()?;
//! println!("boot entry point: {:#x}", loaded.entry_point());
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::boot::linux::{LinuxBootParams, LinuxBootProtocol};
use crate::boot::multiboot::{MultibootInfo, MultibootLayout, MultibootModule, MultibootProtocol};
use crate::{Error, Result};

/// Default guest physical address a Linux protected-mode kernel is loaded at.
pub const DEFAULT_KERNEL_ADDR: u64 = 0x100000;

/// Default guest physical address of the Linux `boot_params` structure.
pub const DEFAULT_SETUP_ADDR: u64 = 0x90000;

/// Guest physical address a legacy boot sector is loaded at and entered from.
pub const BOOT_SECTOR_ADDR: u64 = 0x7C00;

/// What a VM should boot.
///
/// Each variant names images by path; nothing is read until [`BootSource::load`]
/// is called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BootSource {
    /// A Linux kernel in bzImage format, booted via the Linux boot protocol.
    Linux {
        /// Path to the bzImage kernel.
        kernel: PathBuf,
        /// Optional initial ramdisk.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initrd: Option<PathBuf>,
        /// Kernel command line.
        #[serde(default)]
        cmdline: String,
        /// Guest physical address to load the protected-mode kernel at.
        #[serde(default = "default_kernel_addr")]
        kernel_addr: u64,
        /// Guest physical address of the `boot_params` structure.
        #[serde(default = "default_setup_addr")]
        setup_addr: u64,
    },

    /// A Multiboot 1.0 compliant kernel.
    Multiboot {
        /// Path to the kernel image.
        kernel: PathBuf,
        /// Additional modules, loaded in order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        modules: Vec<PathBuf>,
        /// Kernel command line.
        #[serde(default)]
        cmdline: String,
    },

    /// A raw image copied verbatim into guest memory.
    ///
    /// Used for boot sectors, unikernels, and hand-written guest code: the
    /// image is written at `load_addr` and the vCPU starts at `entry`.
    Raw {
        /// Path to the image.
        image: PathBuf,
        /// Guest physical address to load the image at.
        #[serde(default = "default_boot_sector_addr")]
        load_addr: u64,
        /// Guest physical address to begin execution at.
        #[serde(default = "default_boot_sector_addr")]
        entry: u64,
    },
}

fn default_kernel_addr() -> u64 {
    DEFAULT_KERNEL_ADDR
}

fn default_setup_addr() -> u64 {
    DEFAULT_SETUP_ADDR
}

fn default_boot_sector_addr() -> u64 {
    BOOT_SECTOR_ADDR
}

impl BootSource {
    /// A Linux boot source with default load addresses and no initrd.
    pub fn linux(kernel: impl Into<PathBuf>) -> Self {
        Self::Linux {
            kernel: kernel.into(),
            initrd: None,
            cmdline: String::new(),
            kernel_addr: DEFAULT_KERNEL_ADDR,
            setup_addr: DEFAULT_SETUP_ADDR,
        }
    }

    /// A Multiboot boot source with no modules.
    pub fn multiboot(kernel: impl Into<PathBuf>) -> Self {
        Self::Multiboot {
            kernel: kernel.into(),
            modules: Vec::new(),
            cmdline: String::new(),
        }
    }

    /// A raw image loaded at, and entered from, the legacy boot sector address.
    pub fn raw(image: impl Into<PathBuf>) -> Self {
        Self::Raw {
            image: image.into(),
            load_addr: BOOT_SECTOR_ADDR,
            entry: BOOT_SECTOR_ADDR,
        }
    }

    /// Attach an initial ramdisk (Linux only; ignored by other variants).
    #[must_use]
    pub fn with_initrd(mut self, path: impl Into<PathBuf>) -> Self {
        if let Self::Linux { initrd, .. } = &mut self {
            *initrd = Some(path.into());
        }
        self
    }

    /// Set the kernel command line (Linux and Multiboot; ignored by `Raw`).
    #[must_use]
    pub fn with_cmdline(mut self, line: impl Into<String>) -> Self {
        match &mut self {
            Self::Linux { cmdline, .. } | Self::Multiboot { cmdline, .. } => *cmdline = line.into(),
            Self::Raw { .. } => {}
        }
        self
    }

    /// Add a Multiboot module (Multiboot only; ignored by other variants).
    #[must_use]
    pub fn with_module(mut self, path: impl Into<PathBuf>) -> Self {
        if let Self::Multiboot { modules, .. } = &mut self {
            modules.push(path.into());
        }
        self
    }

    /// Load and place a raw image at an explicit address, entering at `entry`.
    #[must_use]
    pub fn at(mut self, load_addr: u64, entry: u64) -> Self {
        if let Self::Raw {
            load_addr: la,
            entry: e,
            ..
        } = &mut self
        {
            *la = load_addr;
            *e = entry;
        }
        self
    }

    /// Path of the primary image this source boots.
    pub fn image_path(&self) -> &Path {
        match self {
            Self::Linux { kernel, .. } | Self::Multiboot { kernel, .. } => kernel,
            Self::Raw { image, .. } => image,
        }
    }

    /// Short protocol name, for logs and API responses.
    pub fn protocol(&self) -> &'static str {
        match self {
            Self::Linux { .. } => "linux",
            Self::Multiboot { .. } => "multiboot",
            Self::Raw { .. } => "raw",
        }
    }

    /// Read every image off disk and validate it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if an image cannot be read, and [`Error::VM`] if a
    /// Linux kernel fails header validation or an image is empty.
    pub fn load(&self) -> Result<LoadedBoot> {
        match self {
            Self::Linux {
                kernel,
                initrd,
                cmdline,
                kernel_addr,
                setup_addr,
            } => {
                let params = LinuxBootParams {
                    kernel_image: read_image(kernel)?,
                    initrd: initrd.as_deref().map(read_image).transpose()?,
                    cmdline: cmdline.clone(),
                    setup_addr: *setup_addr,
                    kernel_addr: *kernel_addr,
                    // Filled in by `set_memory_size` once a VM exists.
                    memory_size: 0,
                };
                // Fail here — with the path in hand — rather than deep inside a
                // backend where the error has lost its context.
                LinuxBootProtocol::validate_params(&params).map_err(|e| {
                    Error::VM(format!("invalid Linux kernel {}: {e}", kernel.display()))
                })?;
                Ok(LoadedBoot::Linux(Box::new(params)))
            }

            Self::Multiboot {
                kernel,
                modules,
                cmdline,
            } => {
                let mut loaded_modules = Vec::with_capacity(modules.len());
                for path in modules {
                    loaded_modules.push(MultibootModule {
                        data: read_image(path)?,
                        cmdline: path.display().to_string(),
                    });
                }
                Ok(LoadedBoot::Multiboot(Box::new(MultibootInfo {
                    kernel_image: read_image(kernel)?,
                    modules: loaded_modules,
                    cmdline: cmdline.clone(),
                    ..MultibootInfo::default()
                })))
            }

            Self::Raw {
                image,
                load_addr,
                entry,
            } => Ok(LoadedBoot::Raw {
                data: read_image(image)?,
                load_addr: *load_addr,
                entry: *entry,
            }),
        }
    }
}

/// Read an image file, rejecting an empty one.
fn read_image(path: &Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path)
        .map_err(|e| Error::VM(format!("failed to read {}: {e}", path.display())))?;
    if data.is_empty() {
        return Err(Error::VM(format!("boot image {} is empty", path.display())));
    }
    Ok(data)
}

/// A [`BootSource`] with every image read and validated.
///
/// This is what a hypervisor backend consumes: bytes plus the addresses they
/// belong at. Backends never read files.
#[derive(Debug, Clone)]
pub enum LoadedBoot {
    /// Linux boot protocol parameters, kernel image included.
    Linux(Box<LinuxBootParams>),
    /// Multiboot information, kernel and modules included.
    Multiboot(Box<MultibootInfo>),
    /// A raw image and where it goes.
    Raw {
        /// Image bytes.
        data: Vec<u8>,
        /// Guest physical address to load at.
        load_addr: u64,
        /// Guest physical address to begin execution at.
        entry: u64,
    },
}

impl LoadedBoot {
    /// Guest physical address the vCPU begins executing at.
    pub fn entry_point(&self) -> u64 {
        match self {
            Self::Linux(params) => params.kernel_addr,
            // Multiboot kernels are conventionally linked to run from 1 MB;
            // the backend refines this from the ELF header when present.
            Self::Multiboot(_) => DEFAULT_KERNEL_ADDR,
            Self::Raw { entry, .. } => *entry,
        }
    }

    /// Tell a Linux boot how much RAM the guest has.
    ///
    /// Separate from [`BootSource::load`] because that reads and validates
    /// images, which callers do before a VM exists — an API server checking an
    /// image is admissible has no memory size to give. The kernel's `e820` map
    /// is built from this and it has no other source of one, so a VM must set
    /// it before asking for the memory regions.
    ///
    /// Does nothing for protocols that do not carry a memory map.
    pub fn set_memory_size(&mut self, bytes: u64) {
        if let Self::Linux(params) = self {
            params.memory_size = bytes;
        }
    }

    /// Short protocol name, matching [`BootSource::protocol`].
    pub fn protocol(&self) -> &'static str {
        match self {
            Self::Linux(_) => "linux",
            Self::Multiboot(_) => "multiboot",
            Self::Raw { .. } => "raw",
        }
    }

    /// The primary image's bytes — the kernel, or the raw image itself.
    ///
    /// This is what identifies a boot for admission control. Initrds and
    /// Multiboot modules are deliberately excluded: they are separate artifacts
    /// that a registry tracks under their own entries, so folding them in would
    /// make the digest depend on which modules happened to accompany the kernel.
    pub fn primary_image(&self) -> &[u8] {
        match self {
            Self::Linux(params) => &params.kernel_image,
            Self::Multiboot(info) => &info.kernel_image,
            Self::Raw { data, .. } => data,
        }
    }

    /// Lower-case hex SHA-256 of [`Self::primary_image`].
    ///
    /// Requires the `ring` feature. Without it this returns an error rather
    /// than a placeholder, so an enforcement point fails closed instead of
    /// admitting an image it could not identify.
    pub fn primary_image_digest(&self) -> Result<String> {
        #[cfg(feature = "ring")]
        {
            let hash = ring::digest::digest(&ring::digest::SHA256, self.primary_image());
            Ok(hash.as_ref().iter().map(|b| format!("{b:02x}")).collect())
        }

        #[cfg(not(feature = "ring"))]
        {
            Err(Error::NotSupported(
                "computing a boot image digest requires the `ring` feature; \
                 image admission control cannot run without it"
                    .to_string(),
            ))
        }
    }

    /// Total bytes that will be written into guest memory.
    pub fn image_bytes(&self) -> usize {
        match self {
            Self::Linux(params) => {
                params.kernel_image.len() + params.initrd.as_ref().map_or(0, Vec::len)
            }
            Self::Multiboot(info) => {
                info.kernel_image.len() + info.modules.iter().map(|m| m.data.len()).sum::<usize>()
            }
            Self::Raw { data, .. } => data.len(),
        }
    }

    /// The `(guest_physical_address, bytes)` regions to write into guest memory.
    ///
    /// Backends that only have a "write these bytes there" primitive — and no
    /// protocol-specific boot helper — can load a guest with this alone, then
    /// set the vCPU to [`Self::entry_point`].
    pub fn memory_regions(&self) -> Result<Vec<(u64, Vec<u8>)>> {
        match self {
            Self::Linux(params) => LinuxBootProtocol::prepare_guest_memory(params),
            Self::Multiboot(info) => {
                MultibootProtocol::prepare_guest_memory(info, &MultibootLayout::default())
            }
            Self::Raw {
                data, load_addr, ..
            } => Ok(vec![(*load_addr, data.clone())]),
        }
    }

    /// The largest guest physical address any region touches.
    ///
    /// A VM whose memory is smaller than this cannot hold the boot images.
    pub fn highest_address(&self) -> Result<u64> {
        Ok(self
            .memory_regions()?
            .iter()
            .map(|(addr, data)| addr + data.len() as u64)
            .max()
            .unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bzImage header the Linux protocol accepts: boot flag, "HdrS"
    /// signature, protocol 2.12, and 4 setup sectors.
    fn valid_bzimage() -> Vec<u8> {
        let mut image = vec![0u8; 8192];
        image[0x1F1] = 4;
        image[0x1FE] = 0x55;
        image[0x1FF] = 0xAA;
        image[0x202..0x206].copy_from_slice(b"HdrS");
        image[0x206] = 0x0C;
        image[0x207] = 0x02;
        image
    }

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hv2-boot-{name}"));
        std::fs::write(&path, bytes).expect("write temp boot image");
        path
    }

    #[test]
    fn linux_builder_sets_defaults() {
        let source = BootSource::linux("/boot/vmlinuz");
        let BootSource::Linux {
            kernel_addr,
            setup_addr,
            initrd,
            ..
        } = &source
        else {
            panic!("expected a Linux source");
        };
        assert_eq!(*kernel_addr, DEFAULT_KERNEL_ADDR);
        assert_eq!(*setup_addr, DEFAULT_SETUP_ADDR);
        assert!(initrd.is_none());
        assert_eq!(source.protocol(), "linux");
    }

    #[test]
    fn builders_are_variant_scoped() {
        // with_initrd applies to Linux and is a no-op elsewhere, so a config
        // built by a generic caller can't silently produce a wrong boot.
        let raw = BootSource::raw("/boot/disk.img").with_initrd("/boot/initrd");
        assert_eq!(raw, BootSource::raw("/boot/disk.img"));

        let multi = BootSource::multiboot("/boot/kernel.elf").with_module("/boot/mod");
        let BootSource::Multiboot { modules, .. } = &multi else {
            panic!("expected a Multiboot source");
        };
        assert_eq!(modules.len(), 1);
    }

    #[test]
    fn raw_at_overrides_addresses() {
        let source = BootSource::raw("/boot/code.bin").at(0x2000, 0x2010);
        let BootSource::Raw {
            load_addr, entry, ..
        } = &source
        else {
            panic!("expected a Raw source");
        };
        assert_eq!(*load_addr, 0x2000);
        assert_eq!(*entry, 0x2010);
    }

    #[test]
    fn load_reads_and_validates_a_linux_kernel() {
        let kernel = temp_file("kernel-ok.bin", &valid_bzimage());
        let loaded = BootSource::linux(&kernel)
            .with_cmdline("console=ttyS0")
            .load()
            .expect("valid bzImage should load");

        assert_eq!(loaded.protocol(), "linux");
        assert_eq!(loaded.entry_point(), DEFAULT_KERNEL_ADDR);
        assert_eq!(loaded.image_bytes(), 8192);

        let _ = std::fs::remove_file(kernel);
    }

    #[test]
    fn load_rejects_a_malformed_kernel_with_its_path() {
        let kernel = temp_file("kernel-bad.bin", &[0u8; 8192]);
        let err = BootSource::linux(&kernel)
            .load()
            .expect_err("a kernel with no HdrS signature must be rejected");

        // The path matters: the operator needs to know *which* image is bad.
        assert!(
            err.to_string().contains("kernel-bad.bin"),
            "error should name the image: {err}"
        );

        let _ = std::fs::remove_file(kernel);
    }

    #[test]
    fn load_rejects_an_empty_image() {
        let image = temp_file("empty.bin", &[]);
        let err = BootSource::raw(&image)
            .load()
            .expect_err("an empty image must be rejected");
        assert!(err.to_string().contains("empty"), "got: {err}");

        let _ = std::fs::remove_file(image);
    }

    #[test]
    fn load_reports_a_missing_image() {
        let err = BootSource::raw("/nonexistent/boot/image.bin")
            .load()
            .expect_err("a missing image must be rejected");
        assert!(err.to_string().contains("image.bin"), "got: {err}");
    }

    #[test]
    fn raw_regions_place_the_image_at_its_load_address() {
        let image = temp_file("raw.bin", &[0xF4, 0xEB, 0xFD]);
        let loaded = BootSource::raw(&image).at(0x7C00, 0x7C00).load().unwrap();

        let regions = loaded.memory_regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].0, 0x7C00);
        assert_eq!(regions[0].1, vec![0xF4, 0xEB, 0xFD]);
        assert_eq!(loaded.highest_address().unwrap(), 0x7C03);

        let _ = std::fs::remove_file(image);
    }

    #[test]
    fn linux_regions_cover_kernel_boot_params_and_cmdline() {
        let kernel = temp_file("kernel-regions.bin", &valid_bzimage());
        let mut loaded = BootSource::linux(&kernel)
            .with_cmdline("quiet")
            .load()
            .unwrap();

        // Loading reads and validates the images; the memory map needs a size
        // that only a VM has, and asking for the regions without one is
        // refused rather than answered with an empty map.
        assert!(
            loaded.memory_regions().is_err(),
            "regions without a memory size would carry an empty e820 map"
        );
        loaded.set_memory_size(256 * 1024 * 1024);

        let regions = loaded.memory_regions().unwrap();
        let addrs: Vec<u64> = regions.iter().map(|(a, _)| *a).collect();
        assert!(addrs.contains(&DEFAULT_SETUP_ADDR), "boot_params region");
        assert!(
            addrs.contains(&(DEFAULT_SETUP_ADDR + 0x1000)),
            "cmdline region"
        );
        assert!(addrs.contains(&DEFAULT_KERNEL_ADDR), "kernel region");

        let _ = std::fs::remove_file(kernel);
    }

    #[test]
    fn boot_source_round_trips_through_json() {
        let source = BootSource::linux("/boot/vmlinuz")
            .with_initrd("/boot/initrd.img")
            .with_cmdline("root=/dev/vda");

        let json = serde_json::to_string(&source).unwrap();
        assert!(json.contains("\"type\":\"linux\""), "tagged: {json}");

        let back: BootSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }

    #[test]
    fn minimal_json_fills_in_default_addresses() {
        // An API caller should only have to name a kernel.
        let source: BootSource =
            serde_json::from_str(r#"{"type":"linux","kernel":"/boot/vmlinuz"}"#).unwrap();
        assert_eq!(source, BootSource::linux("/boot/vmlinuz"));
    }
}
