//! Boot and initialization for Type-1 hypervisor
//!
//! This module handles the bare-metal boot process:
//! - Bootloader handoff
//! - Early console initialization
//! - Memory map processing
//! - CPU initialization

use crate::{Error, Result};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Boot information passed from bootloader
#[derive(Debug)]
pub struct BootInfo {
    /// Physical memory map
    pub memory_map: MemoryMap,
    /// Framebuffer information (if available)
    pub framebuffer: Option<FramebufferInfo>,
    /// RSDP address for ACPI
    pub rsdp_addr: Option<u64>,
    /// Kernel physical address
    pub kernel_addr: u64,
    /// Kernel size in bytes
    pub kernel_size: u64,
}

/// Memory region types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Usable memory
    Usable,
    /// Reserved memory
    Reserved,
    /// ACPI reclaimable
    AcpiReclaimable,
    /// ACPI NVS
    AcpiNvs,
    /// Bad memory
    BadMemory,
    /// Bootloader reclaimable
    BootloaderReclaimable,
    /// Kernel and modules
    KernelAndModules,
    /// Framebuffer
    Framebuffer,
}

/// A memory region in the memory map
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    /// Start physical address
    pub start: u64,
    /// Size in bytes
    pub size: u64,
    /// Region type
    pub kind: MemoryRegionType,
}

/// Physical memory map
#[derive(Debug)]
pub struct MemoryMap {
    /// Memory regions
    regions: [Option<MemoryRegion>; 64],
    /// Number of valid regions
    count: usize,
}

impl MemoryMap {
    /// Create a new empty memory map
    pub const fn new() -> Self {
        Self {
            regions: [None; 64],
            count: 0,
        }
    }

    /// Add a memory region
    pub fn add_region(&mut self, region: MemoryRegion) -> Result<()> {
        if self.count >= 64 {
            return Err(Error::OutOfMemory);
        }
        self.regions[self.count] = Some(region);
        self.count += 1;
        Ok(())
    }

    /// Get iterator over memory regions
    pub fn iter(&self) -> impl Iterator<Item = &MemoryRegion> {
        self.regions[..self.count].iter().filter_map(|r| r.as_ref())
    }

    /// Get total usable memory
    pub fn total_usable_memory(&self) -> u64 {
        self.iter()
            .filter(|r| r.kind == MemoryRegionType::Usable)
            .map(|r| r.size)
            .sum()
    }

    /// Find a region containing the given address
    pub fn find_region(&self, addr: u64) -> Option<&MemoryRegion> {
        self.iter()
            .find(|r| addr >= r.start && addr < r.start + r.size)
    }
}

impl Default for MemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Framebuffer information
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// Physical address of framebuffer
    pub address: u64,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pitch (bytes per row)
    pub pitch: u32,
    /// Bits per pixel
    pub bpp: u8,
    /// Pixel format
    pub format: PixelFormat,
}

/// Pixel format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGB with 8 bits per channel
    Rgb,
    /// BGR with 8 bits per channel
    Bgr,
    /// Unknown format
    Unknown,
}

/// Early initialization before memory allocator is available
pub fn early_init() -> Result<()> {
    // Disable interrupts during early init
    crate::arch::cli();

    Ok(())
}

/// Late initialization after memory allocator is available.
///
/// Performs:
/// 1. Process the physical memory map and initialise the frame allocator
/// 2. Enable the local APIC on the BSP
/// 3. If an RSDP pointer is available, scan ACPI tables for AP (SMP) info
/// 4. Enable interrupts
pub fn late_init(boot_info: &BootInfo) -> Result<()> {
    // --- 1. Frame allocator from memory map ---
    // Find the largest usable region for the hypervisor's internal allocator.
    let mut best_start: u64 = 0;
    let mut best_size: u64 = 0;
    for region in boot_info.memory_map.iter() {
        if region.kind == MemoryRegionType::Usable && region.size > best_size {
            best_start = region.start;
            best_size = region.size;
        }
    }
    if best_size == 0 {
        return Err(Error::OutOfMemory);
    }

    // Store it in atomics so higher layers (including APs) can read safely.
    BOOT_FRAME_ALLOCATOR_START.store(best_start, Ordering::Release);
    BOOT_FRAME_ALLOCATOR_END.store(best_start + best_size, Ordering::Release);

    // --- 2. Local APIC ---
    crate::interrupt::initialize_apic()?;

    // --- 3. SMP / AP detection via ACPI MADT ---
    if let Some(rsdp) = boot_info.rsdp_addr {
        detect_smp_from_rsdp(rsdp);
    }

    // --- 4. Enable interrupts ---
    crate::arch::sti();

    Ok(())
}

/// Frame allocator region discovered during boot (BSP only).
///
/// These use atomics so that concurrent reads from APs are safe without
/// requiring `unsafe` access.  The BSP writes them exactly once in
/// `late_init`; all subsequent reads use `Acquire` ordering.
static BOOT_FRAME_ALLOCATOR_START: AtomicU64 = AtomicU64::new(0);
static BOOT_FRAME_ALLOCATOR_END: AtomicU64 = AtomicU64::new(0);

/// Get the boot-time frame-allocator region.
///
/// Returns `(start, end)` as set by [`late_init`].  Returns `(0, 0)` if
/// `late_init` has not been called yet.
pub fn boot_frame_allocator_region() -> (u64, u64) {
    (
        BOOT_FRAME_ALLOCATOR_START.load(Ordering::Acquire),
        BOOT_FRAME_ALLOCATOR_END.load(Ordering::Acquire),
    )
}

// ---------------------------------------------------------------------------
// SMP / AP enumeration
// ---------------------------------------------------------------------------

/// Maximum number of CPUs
pub const MAX_CPUS: usize = 256;

/// Detected Application Processor APIC IDs.
///
/// Written once by the BSP during `late_init`; read-only afterwards.
/// Access is guarded by `AP_COUNT` (atomic) so readers only see
/// fully-initialised entries.
static mut AP_APIC_IDS: [u8; MAX_CPUS] = [0; MAX_CPUS];
static AP_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Get the number of detected APs (not including BSP).
///
/// Safe to call from any core after `late_init` has returned on the BSP.
pub fn ap_count() -> usize {
    AP_COUNT.load(Ordering::Acquire)
}

/// Get the APIC ID of an AP by index.
///
/// Safe to call from any core after `late_init` has returned on the BSP.
pub fn ap_apic_id(index: usize) -> Option<u8> {
    let count = AP_COUNT.load(Ordering::Acquire);
    if index < count {
        // SAFETY: `AP_APIC_IDS[index]` was written before the BSP
        // performed the `Release` store to `AP_COUNT`, so the
        // `Acquire` load above synchronises the read.
        Some(unsafe { AP_APIC_IDS[index] })
    } else {
        None
    }
}

/// Parse the ACPI MADT (APIC table) starting from the RSDP address.
///
/// This is a minimal inline parser — we only need the Local APIC entries
/// to count APs.  We walk: RSDP → RSDT/XSDT → MADT.
fn detect_smp_from_rsdp(rsdp_addr: u64) {
    // SAFETY: The RSDP is in firmware-reserved memory and is read-only.
    // We perform volatile reads to avoid the compiler re-ordering or caching.
    unsafe {
        let rsdp = rsdp_addr as *const u8;

        // Validate RSDP signature "RSD PTR "
        let sig = core::slice::from_raw_parts(rsdp, 8);
        if sig != b"RSD PTR " {
            return;
        }

        // Validate RSDP checksum (bytes 0..20 must sum to 0 mod 256)
        let mut checksum: u8 = 0;
        for i in 0..20 {
            checksum = checksum.wrapping_add(core::ptr::read_volatile(rsdp.add(i)));
        }
        if checksum != 0 {
            return; // Corrupt RSDP — skip SMP detection
        }

        // Revision byte at offset 15: 0 = ACPI 1.0 (RSDT), >=2 = ACPI 2.0+ (XSDT)
        let revision = core::ptr::read_volatile(rsdp.add(15));

        // For ACPI 2.0+, validate extended checksum (bytes 0..36)
        if revision >= 2 {
            let mut ext_checksum: u8 = 0;
            for i in 0..36 {
                ext_checksum = ext_checksum.wrapping_add(core::ptr::read_volatile(rsdp.add(i)));
            }
            if ext_checksum != 0 {
                return; // Corrupt extended RSDP
            }
        }

        let sdt_addr: u64 = if revision >= 2 {
            // XSDT address at offset 24 (8 bytes)
            core::ptr::read_volatile(rsdp.add(24) as *const u64)
        } else {
            // RSDT address at offset 16 (4 bytes)
            core::ptr::read_volatile(rsdp.add(16) as *const u32) as u64
        };

        if sdt_addr == 0 {
            return;
        }

        // Read SDT header: signature (4), length (4 @ offset 4)
        let sdt = sdt_addr as *const u8;
        let sdt_len = core::ptr::read_volatile(sdt.add(4) as *const u32) as usize;

        // Validate SDT header checksum
        if !(36..=0x10_0000).contains(&sdt_len) {
            return; // Implausible length — reject
        }
        let mut sdt_checksum: u8 = 0;
        for i in 0..sdt_len {
            sdt_checksum = sdt_checksum.wrapping_add(core::ptr::read_volatile(sdt.add(i)));
        }
        if sdt_checksum != 0 {
            return; // Corrupt SDT
        }

        let entry_size: usize = if revision >= 2 { 8 } else { 4 };
        let header_size = 36usize; // Standard ACPI SDT header
        let entry_count = (sdt_len.saturating_sub(header_size)) / entry_size;

        // Walk entries looking for MADT (signature "APIC")
        for i in 0..entry_count {
            let entry_off = header_size + i * entry_size;
            let table_addr: u64 = if entry_size == 8 {
                core::ptr::read_volatile(sdt.add(entry_off) as *const u64)
            } else {
                core::ptr::read_volatile(sdt.add(entry_off) as *const u32) as u64
            };

            if table_addr == 0 {
                continue;
            }

            let tbl = table_addr as *const u8;
            let tbl_sig = core::slice::from_raw_parts(tbl, 4);
            if tbl_sig == b"APIC" {
                parse_madt(table_addr);
                return;
            }
        }
    }
}

/// Parse the MADT and extract Local APIC entries.
///
/// # Safety
/// `madt_addr` must point to a valid MADT in firmware memory.
unsafe fn parse_madt(madt_addr: u64) {
    let madt = madt_addr as *const u8;
    let madt_len = core::ptr::read_volatile(madt.add(4) as *const u32) as usize;

    // Validate MADT checksum
    if !(44..=0x10_0000).contains(&madt_len) {
        return; // Implausible length
    }
    let mut checksum: u8 = 0;
    for i in 0..madt_len {
        checksum = checksum.wrapping_add(core::ptr::read_volatile(madt.add(i)));
    }
    if checksum != 0 {
        return; // Corrupt MADT
    }

    // The BSP APIC ID (current CPU)
    let bsp_apic_id = {
        let apic = crate::interrupt::LocalApic::new();
        apic.id()
    };

    // Track count locally, then publish atomically at the end.
    let mut local_count: usize = 0;

    // MADT entries start at offset 44
    let mut offset = 44usize;
    while offset + 2 <= madt_len {
        let entry_type = core::ptr::read_volatile(madt.add(offset));
        let entry_len = core::ptr::read_volatile(madt.add(offset + 1)) as usize;
        if entry_len < 2 {
            break;
        }
        // Guard against walking past the table
        if offset + entry_len > madt_len {
            break;
        }

        // Type 0 = Processor Local APIC
        if entry_type == 0 && entry_len >= 8 {
            let apic_id = core::ptr::read_volatile(madt.add(offset + 3));
            let flags = core::ptr::read_volatile(madt.add(offset + 4) as *const u32);

            // Bit 0 = enabled, bit 1 = online-capable
            let usable = flags & 0x3 != 0;

            if usable && apic_id != bsp_apic_id && local_count < MAX_CPUS {
                AP_APIC_IDS[local_count] = apic_id;
                local_count += 1;
            }
        }

        offset += entry_len;
    }

    // Publish the count with Release so AP readers synchronise.
    AP_COUNT.store(local_count, Ordering::Release);
}

// ---------------------------------------------------------------------------
// SMP AP startup (INIT-SIPI-SIPI)
// ---------------------------------------------------------------------------

/// Wake an Application Processor using the standard INIT-SIPI-SIPI sequence.
///
/// `ap_apic_id` — target AP's local-APIC ID.
/// `trampoline_page` — the 4 KB–aligned physical page number (vector) whose
/// address `vector << 12` contains the AP boot trampoline code.  The value
/// must be < 256 (i.e. the trampoline is in the first 1 MB).
///
/// # Safety
/// The trampoline page must contain valid real-mode code that transitions
/// the AP into protected/long mode and calls into the hypervisor.
pub unsafe fn startup_ap(ap_apic_id: u8, trampoline_page: u8) {
    let apic = crate::interrupt::LocalApic::new();

    // 1. Send INIT IPI
    apic.write(
        crate::interrupt::apic_reg::ICR_HIGH,
        (ap_apic_id as u32) << 24,
    );
    apic.write(
        crate::interrupt::apic_reg::ICR_LOW,
        0x0000_4500, // INIT, level assert
    );

    // Wait ~10 ms (spin)
    spin_delay(10_000);

    // 2. Send SIPI #1
    apic.write(
        crate::interrupt::apic_reg::ICR_HIGH,
        (ap_apic_id as u32) << 24,
    );
    apic.write(
        crate::interrupt::apic_reg::ICR_LOW,
        0x0000_4600 | trampoline_page as u32, // SIPI, vector = trampoline page
    );

    spin_delay(200);

    // 3. Send SIPI #2
    apic.write(
        crate::interrupt::apic_reg::ICR_HIGH,
        (ap_apic_id as u32) << 24,
    );
    apic.write(
        crate::interrupt::apic_reg::ICR_LOW,
        0x0000_4600 | trampoline_page as u32,
    );

    spin_delay(200);
}

/// Rough spin-loop delay (microseconds). Not calibrated — just burns cycles.
fn spin_delay(us: u64) {
    // ~1000 iterations per microsecond on a modern CPU at ~3 GHz is a
    // conservative lower bound (actual loop throughput is faster).
    let iters = us.saturating_mul(1000);
    for _ in 0..iters {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- MemoryMap ---

    #[test]
    fn memory_map_empty() {
        let mm = MemoryMap::new();
        assert_eq!(mm.iter().count(), 0);
        assert_eq!(mm.total_usable_memory(), 0);
        assert!(mm.find_region(0x1000).is_none());
    }

    #[test]
    fn memory_map_add_and_iter() {
        let mut mm = MemoryMap::new();
        mm.add_region(MemoryRegion {
            start: 0x0,
            size: 0x10_0000,
            kind: MemoryRegionType::Usable,
        })
        .unwrap();
        mm.add_region(MemoryRegion {
            start: 0x10_0000,
            size: 0x1000,
            kind: MemoryRegionType::Reserved,
        })
        .unwrap();

        assert_eq!(mm.iter().count(), 2);
    }

    #[test]
    fn memory_map_total_usable() {
        let mut mm = MemoryMap::new();
        mm.add_region(MemoryRegion {
            start: 0x0,
            size: 0x10_0000, // 1 MiB
            kind: MemoryRegionType::Usable,
        })
        .unwrap();
        mm.add_region(MemoryRegion {
            start: 0x10_0000,
            size: 0x1000,
            kind: MemoryRegionType::Reserved,
        })
        .unwrap();
        mm.add_region(MemoryRegion {
            start: 0x20_0000,
            size: 0x20_0000, // 2 MiB
            kind: MemoryRegionType::Usable,
        })
        .unwrap();

        assert_eq!(mm.total_usable_memory(), 0x10_0000 + 0x20_0000);
    }

    #[test]
    fn memory_map_find_region() {
        let mut mm = MemoryMap::new();
        mm.add_region(MemoryRegion {
            start: 0x1000,
            size: 0x2000,
            kind: MemoryRegionType::Usable,
        })
        .unwrap();

        assert!(mm.find_region(0x1000).is_some());
        assert!(mm.find_region(0x2FFF).is_some());
        assert!(mm.find_region(0x3000).is_none());
        assert!(mm.find_region(0x0FFF).is_none());
    }

    #[test]
    fn memory_map_overflow() {
        let mut mm = MemoryMap::new();
        for i in 0..64 {
            mm.add_region(MemoryRegion {
                start: i * 0x1000,
                size: 0x1000,
                kind: MemoryRegionType::Usable,
            })
            .unwrap();
        }
        // 65th should fail
        assert!(mm
            .add_region(MemoryRegion {
                start: 0xFF_0000,
                size: 0x1000,
                kind: MemoryRegionType::Usable,
            })
            .is_err());
    }

    // --- MemoryRegionType ---

    #[test]
    fn memory_region_type_equality() {
        assert_eq!(MemoryRegionType::Usable, MemoryRegionType::Usable);
        assert_ne!(MemoryRegionType::Usable, MemoryRegionType::Reserved);
        assert_ne!(MemoryRegionType::AcpiReclaimable, MemoryRegionType::AcpiNvs);
    }

    // --- PixelFormat ---

    #[test]
    fn pixel_format_equality() {
        assert_eq!(PixelFormat::Rgb, PixelFormat::Rgb);
        assert_ne!(PixelFormat::Rgb, PixelFormat::Bgr);
        assert_ne!(PixelFormat::Bgr, PixelFormat::Unknown);
    }

    // --- BootInfo ---

    #[test]
    fn boot_info_construction() {
        let info = BootInfo {
            memory_map: MemoryMap::new(),
            framebuffer: None,
            rsdp_addr: Some(0xDEAD_0000),
            kernel_addr: 0,
            kernel_size: 0,
        };
        assert!(info.framebuffer.is_none());
        assert_eq!(info.rsdp_addr, Some(0xDEAD_0000));
    }

    // --- FramebufferInfo ---

    #[test]
    fn framebuffer_info_construction() {
        let fb = FramebufferInfo {
            address: 0xB800_0000,
            width: 1920,
            height: 1080,
            pitch: 1920 * 4,
            bpp: 32,
            format: PixelFormat::Bgr,
        };
        assert_eq!(fb.width, 1920);
        assert_eq!(fb.height, 1080);
        assert_eq!(fb.format, PixelFormat::Bgr);
    }
}

/// Kernel entry point (called from bootloader)
///
/// # Safety
/// This function must only be called once at boot time.
#[cfg(feature = "bootloader_api")]
pub unsafe fn kernel_main(boot_info: &'static bootloader_api::BootInfo) -> ! {
    // Convert bootloader_api boot info to our format
    let _info = convert_boot_info(boot_info);

    // Early initialization
    early_init().expect("Early init failed");

    // Initialize the hypervisor
    crate::initialize().expect("Hypervisor init failed");

    // Enter hypervisor main loop
    loop {
        crate::arch::hlt();
    }
}

/// Convert bootloader_api BootInfo to our format
#[cfg(feature = "bootloader_api")]
fn convert_boot_info(boot_info: &bootloader_api::BootInfo) -> BootInfo {
    let mut memory_map = MemoryMap::new();

    // Convert memory regions
    for region in boot_info.memory_regions.iter() {
        let kind = match region.kind {
            bootloader_api::info::MemoryRegionKind::Usable => MemoryRegionType::Usable,
            bootloader_api::info::MemoryRegionKind::Bootloader => {
                MemoryRegionType::BootloaderReclaimable
            }
            _ => MemoryRegionType::Reserved,
        };

        let _ = memory_map.add_region(MemoryRegion {
            start: region.start,
            size: region.end - region.start,
            kind,
        });
    }

    // Convert framebuffer info
    let framebuffer = boot_info.framebuffer.as_ref().map(|fb| FramebufferInfo {
        address: fb.buffer().as_ptr() as u64,
        width: fb.info().width as u32,
        height: fb.info().height as u32,
        pitch: fb.info().stride as u32 * fb.info().bytes_per_pixel as u32,
        bpp: (fb.info().bytes_per_pixel * 8) as u8,
        format: match fb.info().pixel_format {
            bootloader_api::info::PixelFormat::Rgb => PixelFormat::Rgb,
            bootloader_api::info::PixelFormat::Bgr => PixelFormat::Bgr,
            _ => PixelFormat::Unknown,
        },
    });

    BootInfo {
        memory_map,
        framebuffer,
        rsdp_addr: boot_info.rsdp_addr.map(|a| a.into()),
        kernel_addr: 0, // bootloader_api 0.11 does not expose kernel load address
        kernel_size: 0, // bootloader_api 0.11 does not expose kernel size
    }
}
