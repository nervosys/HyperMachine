//! ACPI Table Generation
//!
//! This module generates ACPI tables for the virtual machine, enabling
//! modern operating systems to discover hardware configuration.
//!
//! Tables generated:
//! - RSDP (Root System Description Pointer) - Entry point for ACPI
//! - RSDT (Root System Description Table) - 32-bit table pointers
//! - XSDT (Extended System Description Table) - 64-bit table pointers
//! - FADT (Fixed ACPI Description Table) - Power management info
//! - MADT (Multiple APIC Description Table) - Interrupt controller info
//! - DSDT (Differentiated System Description Table) - AML bytecode
//!
//! Memory Layout:
//! - RSDP: 0xF0000 (in BIOS ROM area, 16-byte aligned)
//! - Tables: 0xE0000-0xEFFFF (64KB reserved for ACPI)

use std::mem;

/// ACPI memory region start address
pub const ACPI_TABLE_BASE: u64 = 0xE0000;

/// RSDP location (must be 16-byte aligned, in BIOS area)
pub const RSDP_ADDRESS: u64 = 0xF0000;

/// ACPI table signatures
pub const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
pub const RSDT_SIGNATURE: &[u8; 4] = b"RSDT";
pub const XSDT_SIGNATURE: &[u8; 4] = b"XSDT";
pub const FADT_SIGNATURE: &[u8; 4] = b"FACP";
pub const MADT_SIGNATURE: &[u8; 4] = b"APIC";
pub const DSDT_SIGNATURE: &[u8; 4] = b"DSDT";

/// ACPI revision (2.0+)
pub const ACPI_REVISION: u8 = 2;

/// OEM ID for AetherVM
pub const OEM_ID: &[u8; 6] = b"AETHER";

/// OEM Table ID
pub const OEM_TABLE_ID: &[u8; 8] = b"AETHERVM";

/// RSDP (Root System Description Pointer)
///
/// The entry point for ACPI table discovery. BIOS places this in
/// the first 1MB of memory, typically in the EBDA or 0xE0000-0xFFFFF.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    /// Signature: "RSD PTR "
    pub signature: [u8; 8],
    /// Checksum for bytes 0-19
    pub checksum: u8,
    /// OEM ID
    pub oem_id: [u8; 6],
    /// Revision (0=1.0, 2=2.0+)
    pub revision: u8,
    /// Physical address of RSDT (32-bit)
    pub rsdt_address: u32,
    // ACPI 2.0+ fields
    /// Length of table including extended fields
    pub length: u32,
    /// Physical address of XSDT (64-bit)
    pub xsdt_address: u64,
    /// Extended checksum
    pub extended_checksum: u8,
    /// Reserved
    pub reserved: [u8; 3],
}

impl Rsdp {
    /// Create a new RSDP pointing to the given RSDT/XSDT addresses
    pub fn new(rsdt_address: u32, xsdt_address: u64) -> Self {
        let mut rsdp = Self {
            signature: *RSDP_SIGNATURE,
            checksum: 0,
            oem_id: *OEM_ID,
            revision: ACPI_REVISION,
            rsdt_address,
            length: mem::size_of::<Rsdp>() as u32,
            xsdt_address,
            extended_checksum: 0,
            reserved: [0; 3],
        };

        // Calculate checksums
        rsdp.checksum = rsdp.calculate_checksum(20);
        rsdp.extended_checksum = rsdp.calculate_extended_checksum();

        rsdp
    }

    /// Calculate checksum for first 20 bytes (ACPI 1.0 portion)
    fn calculate_checksum(&self, len: usize) -> u8 {
        // SAFETY: `Rsdp` is `#[repr(C, packed)]` with only integer fields.
        // `self` is a valid reference, so reading `len` bytes (≤ size_of::<Rsdp>())
        // from it as a byte slice is safe.
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, len) };
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        (!sum).wrapping_add(1)
    }

    /// Calculate extended checksum for full table
    fn calculate_extended_checksum(&self) -> u8 {
        let len = mem::size_of::<Rsdp>();
        // SAFETY: `Rsdp` is `#[repr(C, packed)]` with only integer fields.
        // `self` is a valid reference and `len` equals `size_of::<Rsdp>()`.
        let bytes = unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, len) };
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        (!sum).wrapping_add(1)
    }

    /// Convert to bytes for memory placement
    pub fn to_bytes(&self) -> Vec<u8> {
        let size = mem::size_of::<Rsdp>();
        let mut bytes = vec![0u8; size];
        // SAFETY: `Rsdp` is `#[repr(C, packed)]` and composed entirely of integer
        // fields. Reading `size` bytes from `self` into a freshly allocated buffer
        // of the same size is safe.
        unsafe {
            std::ptr::copy_nonoverlapping(self as *const _ as *const u8, bytes.as_mut_ptr(), size);
        }
        bytes
    }
}

/// ACPI Table Header (common to all tables except RSDP)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AcpiTableHeader {
    /// Table signature (4 bytes)
    pub signature: [u8; 4],
    /// Length of table including header
    pub length: u32,
    /// ACPI specification revision
    pub revision: u8,
    /// Checksum (sum of all bytes must be 0)
    pub checksum: u8,
    /// OEM ID
    pub oem_id: [u8; 6],
    /// OEM Table ID
    pub oem_table_id: [u8; 8],
    /// OEM Revision
    pub oem_revision: u32,
    /// Creator ID
    pub creator_id: [u8; 4],
    /// Creator Revision
    pub creator_revision: u32,
}

impl AcpiTableHeader {
    /// Serialize header to a byte slice.
    ///
    /// # Safety guarantee
    ///
    /// `AcpiTableHeader` is `#[repr(C, packed)]` with only integer and byte-array
    /// fields, so viewing it as a contiguous byte slice is well-defined.
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: `AcpiTableHeader` is `#[repr(C, packed)]` with only integer/byte-array
        // fields and no padding, so viewing it as a byte slice is well-defined.
        unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, mem::size_of::<Self>()) }
    }

    /// Create a new ACPI table header
    pub fn new(signature: &[u8; 4], length: u32) -> Self {
        Self {
            signature: *signature,
            length,
            revision: ACPI_REVISION,
            checksum: 0,
            oem_id: *OEM_ID,
            oem_table_id: *OEM_TABLE_ID,
            oem_revision: 1,
            creator_id: *b"AVMH", // AetherVM Hypervisor
            creator_revision: 1,
        }
    }
}

/// RSDT (Root System Description Table)
///
/// Contains 32-bit pointers to other ACPI tables.
#[derive(Debug)]
pub struct Rsdt {
    header: AcpiTableHeader,
    entries: Vec<u32>,
}

impl Rsdt {
    /// Create a new RSDT with the given table addresses
    pub fn new(table_addresses: &[u32]) -> Self {
        let length = mem::size_of::<AcpiTableHeader>() as u32 + (table_addresses.len() * 4) as u32;

        Self {
            header: AcpiTableHeader::new(RSDT_SIGNATURE, length),
            entries: table_addresses.to_vec(),
        }
    }

    /// Convert to bytes with calculated checksum
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.header.length as usize);

        bytes.extend_from_slice(self.header.as_bytes());

        // Write entries
        for addr in &self.entries {
            bytes.extend_from_slice(&addr.to_le_bytes());
        }

        // Calculate and set checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        let checksum_offset = 9; // Offset of checksum in header
        bytes[checksum_offset] = (!sum).wrapping_add(1);

        bytes
    }
}

/// XSDT (Extended System Description Table)
///
/// Contains 64-bit pointers to other ACPI tables (ACPI 2.0+).
#[derive(Debug)]
pub struct Xsdt {
    header: AcpiTableHeader,
    entries: Vec<u64>,
}

impl Xsdt {
    /// Create a new XSDT with the given table addresses
    pub fn new(table_addresses: &[u64]) -> Self {
        let length = mem::size_of::<AcpiTableHeader>() as u32 + (table_addresses.len() * 8) as u32;

        Self {
            header: AcpiTableHeader::new(XSDT_SIGNATURE, length),
            entries: table_addresses.to_vec(),
        }
    }

    /// Convert to bytes with calculated checksum
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.header.length as usize);

        bytes.extend_from_slice(self.header.as_bytes());

        // Write entries
        for addr in &self.entries {
            bytes.extend_from_slice(&addr.to_le_bytes());
        }

        // Calculate and set checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        let checksum_offset = 9;
        bytes[checksum_offset] = (!sum).wrapping_add(1);

        bytes
    }
}

/// MADT entry types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MadtEntryType {
    /// Local APIC
    LocalApic = 0,
    /// I/O APIC
    IoApic = 1,
    /// Interrupt Source Override
    InterruptSourceOverride = 2,
    /// NMI Source
    NmiSource = 3,
    /// Local APIC NMI
    LocalApicNmi = 4,
    /// Local APIC Address Override
    LocalApicAddressOverride = 5,
    /// Local x2APIC
    LocalX2Apic = 9,
}

/// MADT Local APIC Entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtLocalApic {
    /// Entry type (0)
    pub entry_type: u8,
    /// Entry length (8)
    pub length: u8,
    /// ACPI Processor UID
    pub processor_uid: u8,
    /// Local APIC ID
    pub apic_id: u8,
    /// Flags (bit 0: enabled)
    pub flags: u32,
}

impl MadtLocalApic {
    /// Create a new Local APIC entry
    pub fn new(processor_uid: u8, apic_id: u8, enabled: bool) -> Self {
        Self {
            entry_type: MadtEntryType::LocalApic as u8,
            length: 8,
            processor_uid,
            apic_id,
            flags: if enabled { 1 } else { 0 },
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; 8];
        bytes[0] = self.entry_type;
        bytes[1] = self.length;
        bytes[2] = self.processor_uid;
        bytes[3] = self.apic_id;
        bytes[4..8].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }
}

/// MADT I/O APIC Entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtIoApic {
    /// Entry type (1)
    pub entry_type: u8,
    /// Entry length (12)
    pub length: u8,
    /// I/O APIC ID
    pub io_apic_id: u8,
    /// Reserved
    pub reserved: u8,
    /// I/O APIC Address
    pub io_apic_address: u32,
    /// Global System Interrupt Base
    pub gsi_base: u32,
}

impl MadtIoApic {
    /// Create a new I/O APIC entry
    pub fn new(io_apic_id: u8, address: u32, gsi_base: u32) -> Self {
        Self {
            entry_type: MadtEntryType::IoApic as u8,
            length: 12,
            io_apic_id,
            reserved: 0,
            io_apic_address: address,
            gsi_base,
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; 12];
        bytes[0] = self.entry_type;
        bytes[1] = self.length;
        bytes[2] = self.io_apic_id;
        bytes[3] = self.reserved;
        bytes[4..8].copy_from_slice(&self.io_apic_address.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.gsi_base.to_le_bytes());
        bytes
    }
}

/// MADT Interrupt Source Override Entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtInterruptOverride {
    /// Entry type (2)
    pub entry_type: u8,
    /// Entry length (10)
    pub length: u8,
    /// Bus (0 = ISA)
    pub bus: u8,
    /// Source IRQ
    pub source: u8,
    /// Global System Interrupt
    pub gsi: u32,
    /// Flags (polarity, trigger mode)
    pub flags: u16,
}

impl MadtInterruptOverride {
    /// Create a new Interrupt Source Override entry
    pub fn new(source_irq: u8, gsi: u32, flags: u16) -> Self {
        Self {
            entry_type: MadtEntryType::InterruptSourceOverride as u8,
            length: 10,
            bus: 0, // ISA
            source: source_irq,
            gsi,
            flags,
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; 10];
        bytes[0] = self.entry_type;
        bytes[1] = self.length;
        bytes[2] = self.bus;
        bytes[3] = self.source;
        bytes[4..8].copy_from_slice(&self.gsi.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }
}

/// MADT Local APIC NMI Entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtLocalApicNmi {
    /// Entry type (4)
    pub entry_type: u8,
    /// Entry length (6)
    pub length: u8,
    /// ACPI Processor UID (0xFF = all)
    pub processor_uid: u8,
    /// Flags
    pub flags: u16,
    /// Local APIC LINT# (0 or 1)
    pub lint: u8,
}

impl MadtLocalApicNmi {
    /// Create a new Local APIC NMI entry
    pub fn new(processor_uid: u8, lint: u8, flags: u16) -> Self {
        Self {
            entry_type: MadtEntryType::LocalApicNmi as u8,
            length: 6,
            processor_uid,
            flags,
            lint,
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; 6];
        bytes[0] = self.entry_type;
        bytes[1] = self.length;
        bytes[2] = self.processor_uid;
        bytes[3..5].copy_from_slice(&self.flags.to_le_bytes());
        bytes[5] = self.lint;
        bytes
    }
}

/// MADT (Multiple APIC Description Table)
///
/// Describes the interrupt controller configuration.
#[derive(Debug)]
pub struct Madt {
    header: AcpiTableHeader,
    /// Local APIC Address
    local_apic_address: u32,
    /// Flags
    flags: u32,
    /// MADT entries
    entries: Vec<Vec<u8>>,
}

impl Madt {
    /// Create a new MADT
    pub fn new(local_apic_address: u32) -> Self {
        // Base length: header + local APIC address + flags
        let base_length = mem::size_of::<AcpiTableHeader>() as u32 + 8;

        Self {
            header: AcpiTableHeader::new(MADT_SIGNATURE, base_length),
            local_apic_address,
            flags: 1, // PC-AT dual 8259 compatible
            entries: Vec::new(),
        }
    }

    /// Add a Local APIC entry
    pub fn add_local_apic(&mut self, processor_uid: u8, apic_id: u8, enabled: bool) {
        let entry = MadtLocalApic::new(processor_uid, apic_id, enabled);
        self.entries.push(entry.to_bytes());
        self.header.length += 8;
    }

    /// Add an I/O APIC entry
    pub fn add_io_apic(&mut self, io_apic_id: u8, address: u32, gsi_base: u32) {
        let entry = MadtIoApic::new(io_apic_id, address, gsi_base);
        self.entries.push(entry.to_bytes());
        self.header.length += 12;
    }

    /// Add an Interrupt Source Override entry
    pub fn add_interrupt_override(&mut self, source_irq: u8, gsi: u32, flags: u16) {
        let entry = MadtInterruptOverride::new(source_irq, gsi, flags);
        self.entries.push(entry.to_bytes());
        self.header.length += 10;
    }

    /// Add a Local APIC NMI entry
    pub fn add_local_apic_nmi(&mut self, processor_uid: u8, lint: u8, flags: u16) {
        let entry = MadtLocalApicNmi::new(processor_uid, lint, flags);
        self.entries.push(entry.to_bytes());
        self.header.length += 6;
    }

    /// Convert to bytes with calculated checksum
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.header.length as usize);

        bytes.extend_from_slice(self.header.as_bytes());

        // Write Local APIC address and flags
        bytes.extend_from_slice(&self.local_apic_address.to_le_bytes());
        bytes.extend_from_slice(&self.flags.to_le_bytes());

        // Write entries
        for entry in &self.entries {
            bytes.extend_from_slice(entry);
        }

        // Calculate and set checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        let checksum_offset = 9;
        bytes[checksum_offset] = (!sum).wrapping_add(1);

        bytes
    }
}

/// FADT (Fixed ACPI Description Table)
///
/// Contains power management information and DSDT pointer.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    /// Header
    pub header: AcpiTableHeader,
    /// Physical address of FACS (32-bit)
    pub firmware_ctrl: u32,
    /// Physical address of DSDT (32-bit)
    pub dsdt: u32,
    /// Reserved (was INT_MODEL in ACPI 1.0)
    pub reserved1: u8,
    /// Preferred PM Profile
    pub preferred_pm_profile: u8,
    /// SCI Interrupt
    pub sci_int: u16,
    /// SMI Command Port
    pub smi_cmd: u32,
    /// ACPI Enable value
    pub acpi_enable: u8,
    /// ACPI Disable value
    pub acpi_disable: u8,
    /// S4BIOS_REQ value
    pub s4bios_req: u8,
    /// PSTATE_CNT
    pub pstate_cnt: u8,
    /// PM1a Event Block address
    pub pm1a_evt_blk: u32,
    /// PM1b Event Block address
    pub pm1b_evt_blk: u32,
    /// PM1a Control Block address
    pub pm1a_cnt_blk: u32,
    /// PM1b Control Block address
    pub pm1b_cnt_blk: u32,
    /// PM2 Control Block address
    pub pm2_cnt_blk: u32,
    /// PM Timer Block address
    pub pm_tmr_blk: u32,
    /// GPE0 Block address
    pub gpe0_blk: u32,
    /// GPE1 Block address
    pub gpe1_blk: u32,
    /// PM1 Event Block length
    pub pm1_evt_len: u8,
    /// PM1 Control Block length
    pub pm1_cnt_len: u8,
    /// PM2 Control Block length
    pub pm2_cnt_len: u8,
    /// PM Timer Block length
    pub pm_tmr_len: u8,
    /// GPE0 Block length
    pub gpe0_blk_len: u8,
    /// GPE1 Block length
    pub gpe1_blk_len: u8,
    /// GPE1 Base
    pub gpe1_base: u8,
    /// CST_CNT
    pub cst_cnt: u8,
    /// P_LVL2_LAT
    pub p_lvl2_lat: u16,
    /// P_LVL3_LAT
    pub p_lvl3_lat: u16,
    /// FLUSH_SIZE
    pub flush_size: u16,
    /// FLUSH_STRIDE
    pub flush_stride: u16,
    /// DUTY_OFFSET
    pub duty_offset: u8,
    /// DUTY_WIDTH
    pub duty_width: u8,
    /// DAY_ALRM
    pub day_alrm: u8,
    /// MON_ALRM
    pub mon_alrm: u8,
    /// CENTURY
    pub century: u8,
    /// Boot Architecture Flags (ACPI 2.0+)
    pub boot_flags: u16,
    /// Reserved
    pub reserved2: u8,
    /// Flags
    pub flags: u32,
    // ACPI 2.0+ Generic Address Structures would follow
    // For simplicity, we'll use a minimal FADT
}

impl Fadt {
    /// Create a new FADT with minimal configuration
    pub fn new(dsdt_address: u32) -> Self {
        Self {
            header: AcpiTableHeader::new(FADT_SIGNATURE, mem::size_of::<Fadt>() as u32),
            firmware_ctrl: 0,
            dsdt: dsdt_address,
            reserved1: 0,
            preferred_pm_profile: 0, // Unspecified
            sci_int: 9,              // SCI on IRQ 9
            smi_cmd: 0,              // No SMI
            acpi_enable: 0,
            acpi_disable: 0,
            s4bios_req: 0,
            pstate_cnt: 0,
            pm1a_evt_blk: 0x400, // PM1a event block
            pm1b_evt_blk: 0,
            pm1a_cnt_blk: 0x404, // PM1a control block
            pm1b_cnt_blk: 0,
            pm2_cnt_blk: 0,
            pm_tmr_blk: 0x408, // PM timer block
            gpe0_blk: 0,
            gpe1_blk: 0,
            pm1_evt_len: 4,
            pm1_cnt_len: 2,
            pm2_cnt_len: 0,
            pm_tmr_len: 4,
            gpe0_blk_len: 0,
            gpe1_blk_len: 0,
            gpe1_base: 0,
            cst_cnt: 0,
            p_lvl2_lat: 0xFFFF, // C2 not supported
            p_lvl3_lat: 0xFFFF, // C3 not supported
            flush_size: 0,
            flush_stride: 0,
            duty_offset: 0,
            duty_width: 0,
            day_alrm: 0,
            mon_alrm: 0,
            century: 0x32,      // RTC century register
            boot_flags: 0x0003, // Legacy devices, 8042
            reserved2: 0,
            flags: 0x000004A5, // WBINVD, PROC_C1, SLP_BUTTON, RTC_S4
        }
    }

    /// Convert to bytes with calculated checksum
    pub fn to_bytes(&self) -> Vec<u8> {
        let size = mem::size_of::<Fadt>();
        let mut bytes = vec![0u8; size];
        // SAFETY: `Fadt` is `#[repr(C, packed)]` and composed entirely of integer
        // fields. Reading `size` bytes from `self` into a freshly allocated buffer
        // of the same size is safe.
        unsafe {
            std::ptr::copy_nonoverlapping(self as *const _ as *const u8, bytes.as_mut_ptr(), size);
        }

        // Calculate and set checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        let checksum_offset = 9;
        bytes[checksum_offset] = (!sum).wrapping_add(1);

        bytes
    }
}

/// Minimal DSDT (Differentiated System Description Table)
///
/// Contains AML bytecode describing the system. For now, this is
/// a minimal placeholder that just satisfies ACPI requirements.
#[derive(Debug)]
pub struct Dsdt {
    header: AcpiTableHeader,
    aml: Vec<u8>,
}

impl Dsdt {
    /// Create a minimal DSDT with basic device definitions
    pub fn new() -> Self {
        // Minimal AML bytecode: just a definition block
        let aml = Self::generate_minimal_aml();
        let length = mem::size_of::<AcpiTableHeader>() as u32 + aml.len() as u32;

        Self {
            header: AcpiTableHeader::new(DSDT_SIGNATURE, length),
            aml,
        }
    }

    /// Generate minimal AML bytecode
    ///
    /// Produces a valid AML stream containing:
    /// - `\_SB_` scope with PCI host bridge (`PCI0`, _HID PNP0A03), power
    ///   button (`PWRB`, _HID PNP0C0C), COM1 serial port (`UAR1`, _HID
    ///   PNP0501 with _CRS I/O 0x3F8-0x3FF IRQ 4), real-time clock (`RTC0`,
    ///   _HID PNP0B00 with _CRS I/O 0x70-0x71 IRQ 8), and high precision
    ///   event timer (`HPET`, _HID PNP0103)
    /// - `\_SB_.CPU0` processor device
    /// - `\_PIC` method (interrupt model selector, 1 argument)
    /// - `\_S5_` sleep package for power-off
    fn generate_minimal_aml() -> Vec<u8> {
        // AML opcode constants
        const SCOPE_OP: u8 = 0x10;
        const NAME_OP: u8 = 0x08;
        const METHOD_OP: u8 = 0x14;
        const RETURN_OP: u8 = 0xA4;
        const STRING_PREFIX: u8 = 0x0D;
        const ARG0_OP: u8 = 0x68;
        const EXT_OP_PREFIX: u8 = 0x5B;
        const DEVICE_OP: u8 = 0x82;
        const ZERO_OP: u8 = 0x00;
        const PACKAGE_OP: u8 = 0x12;
        const BYTE_PREFIX: u8 = 0x0A;
        const WORD_PREFIX: u8 = 0x0B;
        const BUFFER_OP: u8 = 0x11;

        /// Build a _CRS resource template for I/O port range + IRQ
        fn build_io_irq_crs(io_base: u16, io_len: u8, irq: u8, out: &mut Vec<u8>) {
            // Build the resource descriptor bytes first
            let mut res = Vec::new();

            // I/O port descriptor (small resource type 0x47)
            res.push(0x47); // I/O port descriptor tag
            res.push(0x01); // decode 16-bit
            res.extend_from_slice(&io_base.to_le_bytes()); // min base
            res.extend_from_slice(&io_base.to_le_bytes()); // max base
            res.push(0x01); // alignment
            res.push(io_len); // range length

            // IRQ descriptor (small resource type 0x22)
            res.push(0x22); // IRQ descriptor tag
            let irq_mask: u16 = 1 << irq;
            res.extend_from_slice(&irq_mask.to_le_bytes());

            // End tag
            res.push(0x79); // end tag
            res.push(0x00); // checksum

            // Name(_CRS, Buffer() { ... })
            out.push(NAME_OP);
            out.extend_from_slice(b"_CRS");
            out.push(BUFFER_OP);
            let buf_len = res.len() + 1; // +1 for the byte-count byte
            Dsdt::encode_pkg_length(out, buf_len);
            out.push(res.len() as u8); // buffer size as byte
            out.extend_from_slice(&res);
        }

        /// Build a Device(name) { Name(_HID, pnp_id) } AML block
        fn build_device(name: &[u8; 4], pnp_id: &[u8], out: &mut Vec<u8>) {
            let mut body = Vec::new();
            body.push(NAME_OP);
            body.extend_from_slice(b"_HID");
            body.push(STRING_PREFIX);
            body.extend_from_slice(pnp_id);
            body.push(0x00); // null terminator

            out.push(EXT_OP_PREFIX);
            out.push(DEVICE_OP);
            let dev_len = 4 + body.len();
            Dsdt::encode_pkg_length(out, dev_len);
            out.extend_from_slice(name);
            out.extend_from_slice(&body);
        }

        /// Build a Device with _HID and _CRS (I/O + IRQ)
        fn build_device_with_crs(
            name: &[u8; 4],
            pnp_id: &[u8],
            io_base: u16,
            io_len: u8,
            irq: u8,
            out: &mut Vec<u8>,
        ) {
            let mut body = Vec::new();
            body.push(NAME_OP);
            body.extend_from_slice(b"_HID");
            body.push(STRING_PREFIX);
            body.extend_from_slice(pnp_id);
            body.push(0x00);

            build_io_irq_crs(io_base, io_len, irq, &mut body);

            out.push(EXT_OP_PREFIX);
            out.push(DEVICE_OP);
            let dev_len = 4 + body.len();
            Dsdt::encode_pkg_length(out, dev_len);
            out.extend_from_slice(name);
            out.extend_from_slice(&body);
        }

        let mut aml = Vec::with_capacity(512);

        // --- Scope(\_SB_) ---
        let sb_body = {
            let mut body = Vec::new();

            // PCI host bridge
            build_device(b"PCI0", b"PNP0A03", &mut body);
            // Power button
            build_device(b"PWRB", b"PNP0C0C", &mut body);
            // COM1 serial port with _CRS: I/O 0x3F8-0x3FF, IRQ 4
            build_device_with_crs(b"UAR1", b"PNP0501", 0x3F8, 8, 4, &mut body);
            // Real-time clock with _CRS: I/O 0x70-0x71, IRQ 8
            build_device_with_crs(b"RTC0", b"PNP0B00", 0x70, 2, 8, &mut body);
            // High precision event timer
            build_device(b"HPET", b"PNP0103", &mut body);

            // CPU0 processor device
            {
                let mut cpu_body = Vec::new();
                cpu_body.push(NAME_OP);
                cpu_body.extend_from_slice(b"_HID");
                cpu_body.push(STRING_PREFIX);
                cpu_body.extend_from_slice(b"ACPI0007");
                cpu_body.push(0x00);
                // _UID = 0
                cpu_body.push(NAME_OP);
                cpu_body.extend_from_slice(b"_UID");
                cpu_body.push(ZERO_OP);

                body.push(EXT_OP_PREFIX);
                body.push(DEVICE_OP);
                let dev_len = 4 + cpu_body.len();
                Dsdt::encode_pkg_length(&mut body, dev_len);
                body.extend_from_slice(b"CPU0");
                body.extend_from_slice(&cpu_body);
            }

            body
        };

        // Scope(\_SB_)
        aml.push(SCOPE_OP);
        let scope_len = 4 + sb_body.len(); // nameseg `_SB_` + body
        Self::encode_pkg_length(&mut aml, scope_len);
        aml.extend_from_slice(b"_SB_");
        aml.extend_from_slice(&sb_body);

        // --- Method(\_PIC, 1) { Return(Arg0) } ---
        let pic_body = [RETURN_OP, ARG0_OP]; // Return(Arg0)
        aml.push(METHOD_OP);
        let method_len = 4 + 1 + pic_body.len(); // nameseg + flags + body
        Self::encode_pkg_length(&mut aml, method_len);
        aml.extend_from_slice(b"_PIC");
        aml.push(0x01); // flags: ArgCount=1, NotSerialized
        aml.extend_from_slice(&pic_body);

        // --- Name(\_S5_, Package(4) { 0x05, 0x00, 0x00, 0x00 }) ---
        // Sleep state S5 (soft off): PM1a_CNT.SLP_TYP = 5
        aml.push(NAME_OP);
        aml.extend_from_slice(b"_S5_");
        aml.push(PACKAGE_OP);
        let pkg_body_len = 1 + 4 * 2; // element count + 4 × (BYTE_PREFIX + value)
        Self::encode_pkg_length(&mut aml, pkg_body_len);
        aml.push(0x04); // 4 elements
        aml.push(BYTE_PREFIX);
        aml.push(0x05); // SLP_TYP for PM1a
        aml.push(BYTE_PREFIX);
        aml.push(0x05); // SLP_TYP for PM1b
        aml.push(ZERO_OP); // reserved
        aml.push(ZERO_OP); // reserved

        // --- Name(\_OS_, "Microsoft Windows NT") ---
        aml.push(NAME_OP);
        aml.extend_from_slice(b"_OS_");
        aml.push(STRING_PREFIX);
        aml.extend_from_slice(b"Microsoft Windows NT\0");

        aml
    }

    /// Encode AML PkgLength into `out`.
    /// Handles lengths up to 0x0FFF_FFFF (4-byte encoding).
    fn encode_pkg_length(out: &mut Vec<u8>, raw_len: usize) {
        // PkgLength includes the length-field bytes themselves.
        if raw_len < 0x3F {
            out.push((raw_len + 1) as u8);
        } else if raw_len + 2 <= 0x0FFF {
            let total = raw_len + 2;
            out.push(0x40 | (total & 0x0F) as u8);
            out.push((total >> 4) as u8);
        } else if raw_len + 3 <= 0x0F_FFFF {
            let total = raw_len + 3;
            out.push(0x80 | (total & 0x0F) as u8);
            out.push((total >> 4) as u8);
            out.push((total >> 12) as u8);
        } else {
            let total = raw_len + 4;
            out.push(0xC0 | (total & 0x0F) as u8);
            out.push((total >> 4) as u8);
            out.push((total >> 12) as u8);
            out.push((total >> 20) as u8);
        }
    }

    /// Convert to bytes with calculated checksum
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.header.length as usize);

        bytes.extend_from_slice(self.header.as_bytes());

        // Write AML
        bytes.extend_from_slice(&self.aml);

        // Calculate and set checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        let checksum_offset = 9;
        bytes[checksum_offset] = (!sum).wrapping_add(1);

        bytes
    }
}

impl Default for Dsdt {
    fn default() -> Self {
        Self::new()
    }
}

/// ACPI Table Builder
///
/// Generates a complete set of ACPI tables for the VM.
#[derive(Debug)]
pub struct AcpiTableBuilder {
    /// Number of CPUs
    num_cpus: u32,
    /// Local APIC address
    local_apic_address: u32,
    /// I/O APIC address
    io_apic_address: u32,
}

impl AcpiTableBuilder {
    /// Create a new ACPI table builder
    pub fn new() -> Self {
        Self {
            num_cpus: 1,
            local_apic_address: 0xFEE0_0000,
            io_apic_address: 0xFEC0_0000,
        }
    }

    /// Set the number of CPUs
    pub fn with_cpus(mut self, num_cpus: u32) -> Self {
        self.num_cpus = num_cpus;
        self
    }

    /// Set the Local APIC address
    pub fn with_local_apic_address(mut self, address: u32) -> Self {
        self.local_apic_address = address;
        self
    }

    /// Set the I/O APIC address
    pub fn with_io_apic_address(mut self, address: u32) -> Self {
        self.io_apic_address = address;
        self
    }

    /// Build all ACPI tables
    ///
    /// Returns a vector of (address, data) pairs for each table.
    pub fn build(&self) -> AcpiTables {
        let mut current_addr = ACPI_TABLE_BASE as u32;
        let mut tables = Vec::new();

        // Build DSDT first (referenced by FADT)
        let dsdt = Dsdt::new();
        let dsdt_bytes = dsdt.to_bytes();
        let dsdt_addr = current_addr;
        tables.push((dsdt_addr as u64, dsdt_bytes.clone()));
        current_addr += dsdt_bytes.len() as u32;
        current_addr = (current_addr + 7) & !7; // Align to 8 bytes

        // Build FADT (references DSDT)
        let fadt = Fadt::new(dsdt_addr);
        let fadt_bytes = fadt.to_bytes();
        let fadt_addr = current_addr;
        tables.push((fadt_addr as u64, fadt_bytes.clone()));
        current_addr += fadt_bytes.len() as u32;
        current_addr = (current_addr + 7) & !7;

        // Build MADT
        let mut madt = Madt::new(self.local_apic_address);

        // Add Local APIC entries for each CPU
        for i in 0..self.num_cpus {
            madt.add_local_apic(i as u8, i as u8, true);
        }

        // Add I/O APIC
        madt.add_io_apic(0, self.io_apic_address, 0);

        // Add standard interrupt overrides
        // IRQ 0 -> GSI 2 (timer)
        madt.add_interrupt_override(0, 2, 0);
        // IRQ 9 -> GSI 9 (SCI, level-triggered, active-low)
        madt.add_interrupt_override(9, 9, 0x000D);

        // Add Local APIC NMI (all processors, LINT1)
        madt.add_local_apic_nmi(0xFF, 1, 0);

        let madt_bytes = madt.to_bytes();
        let madt_addr = current_addr;
        tables.push((madt_addr as u64, madt_bytes.clone()));
        current_addr += madt_bytes.len() as u32;
        current_addr = (current_addr + 7) & !7;

        // Build RSDT (32-bit pointers)
        let rsdt = Rsdt::new(&[fadt_addr, madt_addr]);
        let rsdt_bytes = rsdt.to_bytes();
        let rsdt_addr = current_addr;
        tables.push((rsdt_addr as u64, rsdt_bytes.clone()));
        current_addr += rsdt_bytes.len() as u32;
        current_addr = (current_addr + 7) & !7;

        // Build XSDT (64-bit pointers)
        let xsdt = Xsdt::new(&[fadt_addr as u64, madt_addr as u64]);
        let xsdt_bytes = xsdt.to_bytes();
        let xsdt_addr = current_addr;
        tables.push((xsdt_addr as u64, xsdt_bytes));

        // Build RSDP (entry point)
        let rsdp = Rsdp::new(rsdt_addr, xsdt_addr as u64);
        let rsdp_bytes = rsdp.to_bytes();
        tables.push((RSDP_ADDRESS, rsdp_bytes));

        AcpiTables {
            tables,
            rsdp_address: RSDP_ADDRESS,
        }
    }
}

impl Default for AcpiTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Generated ACPI tables ready for memory placement
#[derive(Debug)]
pub struct AcpiTables {
    /// Vector of (address, data) pairs for each table
    pub tables: Vec<(u64, Vec<u8>)>,
    /// Address where RSDP should be placed
    pub rsdp_address: u64,
}

impl AcpiTables {
    /// Get total size of all tables
    pub fn total_size(&self) -> usize {
        self.tables.iter().map(|(_, data)| data.len()).sum()
    }

    /// Iterate over tables
    pub fn iter(&self) -> impl Iterator<Item = &(u64, Vec<u8>)> {
        self.tables.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsdp_creation() {
        let rsdp = Rsdp::new(0xE0000, 0xE1000);
        assert_eq!(&rsdp.signature, RSDP_SIGNATURE);
        // Copy packed fields to avoid unaligned access
        let revision = rsdp.revision;
        let rsdt_address = rsdp.rsdt_address;
        let xsdt_address = rsdp.xsdt_address;
        assert_eq!(revision, ACPI_REVISION);
        assert_eq!(rsdt_address, 0xE0000);
        assert_eq!(xsdt_address, 0xE1000);
    }

    #[test]
    fn test_rsdp_checksum() {
        let rsdp = Rsdp::new(0xE0000, 0xE1000);
        let bytes = rsdp.to_bytes();

        // Sum of first 20 bytes should be 0
        let sum: u8 = bytes[..20].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "RSDP checksum failed");
    }

    #[test]
    fn test_rsdt_creation() {
        let rsdt = Rsdt::new(&[0x1000, 0x2000]);
        let bytes = rsdt.to_bytes();

        // Check signature
        assert_eq!(&bytes[0..4], RSDT_SIGNATURE);

        // Verify checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "RSDT checksum failed");
    }

    #[test]
    fn test_xsdt_creation() {
        let xsdt = Xsdt::new(&[0x1000, 0x2000]);
        let bytes = xsdt.to_bytes();

        // Check signature
        assert_eq!(&bytes[0..4], XSDT_SIGNATURE);

        // Verify checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "XSDT checksum failed");
    }

    #[test]
    fn test_madt_creation() {
        let mut madt = Madt::new(0xFEE0_0000);
        madt.add_local_apic(0, 0, true);
        madt.add_io_apic(0, 0xFEC0_0000, 0);

        let bytes = madt.to_bytes();

        // Check signature
        assert_eq!(&bytes[0..4], MADT_SIGNATURE);

        // Verify checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "MADT checksum failed");
    }

    #[test]
    fn test_madt_local_apic_entry() {
        let entry = MadtLocalApic::new(0, 1, true);
        let bytes = entry.to_bytes();

        assert_eq!(bytes[0], MadtEntryType::LocalApic as u8);
        assert_eq!(bytes[1], 8); // Length
        assert_eq!(bytes[2], 0); // Processor UID
        assert_eq!(bytes[3], 1); // APIC ID
        assert_eq!(bytes[4], 1); // Flags (enabled)
    }

    #[test]
    fn test_madt_io_apic_entry() {
        let entry = MadtIoApic::new(0, 0xFEC0_0000, 0);
        let bytes = entry.to_bytes();

        assert_eq!(bytes[0], MadtEntryType::IoApic as u8);
        assert_eq!(bytes[1], 12); // Length
        assert_eq!(bytes[2], 0); // I/O APIC ID
    }

    #[test]
    fn test_madt_interrupt_override() {
        let entry = MadtInterruptOverride::new(0, 2, 0);
        let bytes = entry.to_bytes();

        assert_eq!(bytes[0], MadtEntryType::InterruptSourceOverride as u8);
        assert_eq!(bytes[1], 10); // Length
        assert_eq!(bytes[2], 0); // Bus (ISA)
        assert_eq!(bytes[3], 0); // Source IRQ
    }

    #[test]
    fn test_fadt_creation() {
        let fadt = Fadt::new(0xE0000);
        let bytes = fadt.to_bytes();

        // Check signature
        assert_eq!(&bytes[0..4], FADT_SIGNATURE);

        // Verify checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "FADT checksum failed");
    }

    #[test]
    fn test_dsdt_creation() {
        let dsdt = Dsdt::new();
        let bytes = dsdt.to_bytes();

        // Check signature
        assert_eq!(&bytes[0..4], DSDT_SIGNATURE);

        // Verify checksum
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "DSDT checksum failed");
    }

    #[test]
    fn test_dsdt_aml_not_empty() {
        let aml = Dsdt::generate_minimal_aml();
        assert!(
            !aml.is_empty(),
            "DSDT AML should contain device definitions"
        );
        // Should contain _SB_ scope name
        assert!(
            aml.windows(4).any(|w| w == b"_SB_"),
            "AML should contain _SB_ scope"
        );
        // Should contain PCI0 device
        assert!(
            aml.windows(4).any(|w| w == b"PCI0"),
            "AML should contain PCI0 device"
        );
        // Should contain PWRB power button
        assert!(
            aml.windows(4).any(|w| w == b"PWRB"),
            "AML should contain PWRB device"
        );
        // Should contain _PIC method
        assert!(
            aml.windows(4).any(|w| w == b"_PIC"),
            "AML should contain _PIC method"
        );
        // Should contain PNP IDs
        assert!(
            aml.windows(7).any(|w| w == b"PNP0A03"),
            "AML should contain PCI host bridge PNP ID"
        );
        assert!(
            aml.windows(7).any(|w| w == b"PNP0C0C"),
            "AML should contain power button PNP ID"
        );
        // COM1 serial port
        assert!(
            aml.windows(4).any(|w| w == b"UAR1"),
            "AML should contain UAR1 serial port device"
        );
        assert!(
            aml.windows(7).any(|w| w == b"PNP0501"),
            "AML should contain COM1 PNP ID"
        );
        // Real-time clock
        assert!(
            aml.windows(4).any(|w| w == b"RTC0"),
            "AML should contain RTC0 device"
        );
        assert!(
            aml.windows(7).any(|w| w == b"PNP0B00"),
            "AML should contain RTC PNP ID"
        );
        // HPET
        assert!(
            aml.windows(4).any(|w| w == b"HPET"),
            "AML should contain HPET device"
        );
        assert!(
            aml.windows(7).any(|w| w == b"PNP0103"),
            "AML should contain HPET PNP ID"
        );
        // OS name should not be empty
        assert!(
            aml.windows(20).any(|w| w == b"Microsoft Windows NT"),
            "AML should contain OS name string"
        );
    }

    #[test]
    fn test_dsdt_aml_checksum_with_content() {
        // Ensure the DSDT with real AML still passes checksum
        let dsdt = Dsdt::new();
        let bytes = dsdt.to_bytes();
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "DSDT with AML content checksum failed");
        // Length field should account for header + AML
        let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert!(length > mem::size_of::<AcpiTableHeader>() as u32);
    }

    #[test]
    fn test_acpi_table_builder_default() {
        let builder = AcpiTableBuilder::new();
        let tables = builder.build();

        // Should have DSDT, FADT, MADT, RSDT, XSDT, RSDP
        assert_eq!(tables.tables.len(), 6);

        // RSDP should be at the expected address
        assert_eq!(tables.rsdp_address, RSDP_ADDRESS);
    }

    #[test]
    fn test_acpi_table_builder_multi_cpu() {
        let builder = AcpiTableBuilder::new().with_cpus(4);
        let tables = builder.build();

        // Find MADT and verify it has 4 Local APIC entries
        let madt_table = tables
            .tables
            .iter()
            .find(|(_, data)| &data[0..4] == MADT_SIGNATURE)
            .expect("MADT not found");

        // MADT should be larger with 4 CPUs
        assert!(madt_table.1.len() > 60);
    }

    #[test]
    fn test_acpi_all_checksums_valid() {
        let builder = AcpiTableBuilder::new();
        let tables = builder.build();

        for (addr, data) in &tables.tables {
            let sum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
            assert_eq!(sum, 0, "Checksum failed for table at {:#x}", addr);
        }
    }

    #[test]
    fn test_acpi_table_addresses_aligned() {
        let builder = AcpiTableBuilder::new();
        let tables = builder.build();

        for (addr, _) in &tables.tables {
            // All tables should be at least 4-byte aligned
            assert_eq!(addr % 4, 0, "Table at {:#x} not aligned", addr);
        }
    }

    #[test]
    fn test_acpi_tables_total_size() {
        let builder = AcpiTableBuilder::new();
        let tables = builder.build();

        let total = tables.total_size();
        // Should be reasonable size (< 4KB for basic config)
        assert!(total < 4096);
        assert!(total > 0);
    }
}
