//! Hardware-reduced ACPI tables for a Linux microVM guest.
//!
//! A guest that finds only an MP table assumes a PC: it initialises the
//! 8259 PIC, then reads and masks all 24 I/O APIC redirection entries one
//! register at a time, and registers the CMOS RTC device -- about 500 VM
//! exits per boot for hardware the guest never uses. An FADT that declares
//! the platform hardware-reduced (ACPI 5.0+) tells Linux there is no legacy
//! PIC and no fixed timer to set up, so it skips all of that and programs only
//! the redirection entries its drivers ask for. Firecracker boots its guests
//! the same way.
//!
//! The set is the minimum Linux needs: RSDP -> XSDT -> FADT (revision 6,
//! `HW_REDUCED_ACPI`) and MADT, and a DSDT naming the UART and every
//! virtio-mmio window. The DSDT is not optional: without a legacy PIC, Linux
//! no longer maps ISA IRQs 0-15 to I/O APIC pins on its own, so a device given
//! as `virtio_mmio.device=...:5` on the command line gets an IRQ number nothing
//! routes, and the guest boots without its devices. Described here, each one
//! carries an `Interrupt` resource Linux maps through the MADT. Written at
//! [`ACPI_ADDR`], which the `e820` map leaves out of RAM, and found there by
//! Linux's scan of `0xE0000..0x100000` for the RSDP.

/// Where the tables go: the RSDP first, the rest after it.
pub const ACPI_ADDR: u64 = 0xE_0000;

/// The most bytes the tables may take: up to the BIOS area at `0xF0000`.
pub const ACPI_MAX_LEN: usize = 0x1_0000;

/// A virtio-mmio window for the DSDT: where it is and which I/O APIC pin it
/// raises, edge-triggered and active-high as the command-line form was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioDevice {
    /// Guest physical base of the register window.
    pub base: u64,
    /// Window size in bytes.
    pub size: u32,
    /// I/O APIC input (GSI) the device interrupts on.
    pub gsi: u32,
}

/// Where every local APIC is, and the I/O APIC (as in the MP table).
const LAPIC_ADDR: u32 = 0xFEE0_0000;
const IOAPIC_ADDR: u32 = 0xFEC0_0000;

/// The one UART, as the legacy PC set attaches it.
const COM1_PORT: u16 = 0x3F8;
const COM1_IRQ: u8 = 4;

const HEADER_LEN: usize = 36;
const RSDP_LEN: usize = 36;
const FADT_LEN: usize = 276;

/// FADT `Flags`: no fixed power or sleep button, and hardware-reduced.
const FADT_PWR_BUTTON: u32 = 1 << 4;
const FADT_SLP_BUTTON: u32 = 1 << 5;
const FADT_HW_REDUCED_ACPI: u32 = 1 << 20;
/// FADT `IAPC_BOOT_ARCH`: an 8042 is present. Without it Linux marks the
/// keyboard controller absent and stops probing it, which is a separate
/// behaviour change from this one.
const IAPC_BOOT_ARCH_8042: u16 = 1 << 1;

/// The tables for `cpus` processors and these virtio-mmio `devices`, to be
/// written at [`ACPI_ADDR`]. Processor `i` has local APIC ID `i`; the I/O
/// APIC's ID is `cpus`, as in the MP table.
///
/// # Panics
///
/// If `cpus` is 0 or more than 255, if there are more than 1,000 devices, or
/// if a device lies above 4 GiB (the DSDT describes it with a 32-bit window).
#[must_use]
pub fn build(cpus: u32, devices: &[MmioDevice]) -> Vec<u8> {
    assert!(
        (1..=255).contains(&cpus),
        "ACPI tables for {cpus} CPUs: 1 to 255"
    );
    assert!(
        devices.len() <= 1000,
        "{} virtio-mmio devices: at most 1,000",
        devices.len()
    );
    let base = ACPI_ADDR;
    let xsdt_at = align16(RSDP_LEN);
    let xsdt_len = HEADER_LEN + 2 * 8;
    let fadt_at = align16(xsdt_at + xsdt_len);
    let madt_at = align16(fadt_at + FADT_LEN);
    let madt_len = HEADER_LEN + 8 + 8 * cpus as usize + 12;
    let dsdt_at = align16(madt_at + madt_len);
    let aml = dsdt_body(devices);
    let mut out = vec![0u8; dsdt_at + HEADER_LEN + aml.len()];
    assert!(
        out.len() <= ACPI_MAX_LEN,
        "ACPI tables of {} bytes",
        out.len()
    );

    // RSDP, revision 2: points at the XSDT only.
    let rsdp = &mut out[..RSDP_LEN];
    rsdp[0..8].copy_from_slice(b"RSD PTR ");
    rsdp[9..15].copy_from_slice(b"HYPERM");
    rsdp[15] = 2;
    put32(rsdp, 20, RSDP_LEN as u32);
    put64(rsdp, 24, base + xsdt_at as u64);
    rsdp[8] = checksum(&rsdp[..20]);
    rsdp[32] = checksum(&rsdp[..RSDP_LEN]);

    let xsdt = &mut out[xsdt_at..xsdt_at + xsdt_len];
    header(xsdt, b"XSDT", 1);
    put64(xsdt, HEADER_LEN, base + fadt_at as u64);
    put64(xsdt, HEADER_LEN + 8, base + madt_at as u64);
    seal(xsdt);

    let fadt = &mut out[fadt_at..fadt_at + FADT_LEN];
    header(fadt, b"FACP", 6);
    put16(fadt, 109, IAPC_BOOT_ARCH_8042);
    put32(
        fadt,
        112,
        FADT_PWR_BUTTON | FADT_SLP_BUTTON | FADT_HW_REDUCED_ACPI,
    );
    fadt[131] = 5; // FADT minor version: ACPI 6.5
    put64(fadt, 140, base + dsdt_at as u64); // X_DSDT
    seal(fadt);

    let madt = &mut out[madt_at..madt_at + madt_len];
    header(madt, b"APIC", 5);
    put32(madt, HEADER_LEN, LAPIC_ADDR);
    put32(madt, HEADER_LEN + 4, 0); // no PC-AT dual 8259 setup
    let mut at = HEADER_LEN + 8;
    for id in 0..cpus {
        let id = id as u8;
        madt[at..at + 4].copy_from_slice(&[0, 8, id, id]); // processor local APIC
        put32(madt, at + 4, 1); // enabled
        at += 8;
    }
    madt[at..at + 4].copy_from_slice(&[1, 12, cpus as u8, 0]); // I/O APIC
    put32(madt, at + 4, IOAPIC_ADDR);
    put32(madt, at + 8, 0); // GSI base
    seal(madt);

    let dsdt = &mut out[dsdt_at..];
    header(dsdt, b"DSDT", 2);
    dsdt[HEADER_LEN..].copy_from_slice(&aml);
    seal(dsdt);

    out
}

/// A PCI function's interrupt pin A, for the tables of a guest with a PCI
/// bus: which slot of bus 0 it sits in and which I/O APIC input it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciIntx {
    /// The device number on bus 0.
    pub slot: u8,
    /// The I/O APIC input (GSI), level-triggered.
    pub gsi: u8,
}

/// The fixed-hardware power registers a PC's tables name, and the value a
/// guest writes to turn itself off. The ports are where QEMU's `pc` machine
/// has them, since that is where a guest's fallbacks look.
pub const PM1A_EVT_PORT: u16 = 0x600;
/// PM1a control: `SLP_TYP` in bits 10-12 and `SLP_EN` in bit 13.
pub const PM1A_CNT_PORT: u16 = 0x604;
/// The interrupt the tables name for ACPI events. None is ever raised.
pub const SCI_IRQ: u8 = 9;
/// `SLP_TYP` for S5, soft off, as `\_S5` gives it.
pub const S5_SLEEP_TYPE: u16 = 5;
/// `SLP_EN`: the write that enters the sleep state.
pub const SLP_EN: u16 = 1 << 13;

/// Whether a write of `value` to [`PM1A_CNT_PORT`] turns the machine off.
#[must_use]
pub fn is_power_off(value: u16) -> bool {
    value & SLP_EN != 0 && (value >> 10) & 0x7 == S5_SLEEP_TYPE
}

/// Where the PCI host bridge's memory window is: the part of the hole below
/// 4 GiB that every virtio-pci BAR this VMM assigns falls in.
const PCI_WINDOW_BASE: u32 = 0xD000_0000;
const PCI_WINDOW_LEN: u32 = 0x1000_0000;

const FACS_LEN: usize = 64;
/// MADT `Flags`: the machine also has a pair of 8259s.
const MADT_PCAT_COMPAT: u32 = 1;
/// An interrupt source override's flags: level-triggered and active-high,
/// which is how KVM's I/O APIC takes a line raised through `KVM_IRQ_LINE`.
const LEVEL_ACTIVE_HIGH: u16 = 0x000D;

/// The tables for a PC with a PCI bus: `cpus` processors, the legacy devices,
/// and these PCI functions. To be written at [`ACPI_ADDR`].
///
/// For a guest started by firmware from a disk image, whose kernel is a
/// distribution's and not this project's. It differs from [`build`] in what
/// it has to describe:
///
/// - **A PCI host bridge**, with the window its devices' BARs are in and a
///   routing table from each device's pin to an I/O APIC input. Without the
///   routing table a kernel that has switched to the I/O APIC has no interrupt
///   for a PCI device.
/// - **Fixed power hardware**, not the hardware-reduced kind. A guest booted
///   through EFI on a hardware-reduced platform turns itself off and restarts
///   by asking the firmware, and what a given firmware then does is not this
///   VMM's to know. With PM1 registers and `\_S5` named here, the guest writes
///   a port this VMM watches ([`is_power_off`]).
/// - **A pair of 8259s as well as the I/O APIC**, since both are there.
///
/// Processor `i` has local APIC ID `i`; the I/O APIC's ID is `cpus`.
///
/// # Panics
///
/// If `cpus` is 0 or more than 255, or if there are more than 32 PCI
/// functions.
#[must_use]
pub fn build_pc(cpus: u32, pci: &[PciIntx]) -> Vec<u8> {
    assert!(
        (1..=255).contains(&cpus),
        "ACPI tables for {cpus} CPUs: 1 to 255"
    );
    assert!(pci.len() <= 32, "{} PCI functions: at most 32", pci.len());
    // Each input PCI raises, once: an override per input, not per device.
    let mut level: Vec<u8> = pci.iter().map(|d| d.gsi).collect();
    level.push(SCI_IRQ);
    level.sort_unstable();
    level.dedup();

    let base = ACPI_ADDR;
    let xsdt_at = align16(RSDP_LEN);
    let xsdt_len = HEADER_LEN + 2 * 8;
    let facs_at = align64(xsdt_at + xsdt_len);
    let fadt_at = align16(facs_at + FACS_LEN);
    let madt_at = align16(fadt_at + FADT_LEN);
    // Local APICs, the I/O APIC, the timer's override, one per level input,
    // and the NMI line every processor has on LINT1.
    let madt_len = HEADER_LEN + 8 + 8 * cpus as usize + 12 + 10 + 10 * level.len() + 6;
    let dsdt_at = align16(madt_at + madt_len);
    let aml = pc_dsdt_body(pci);
    let mut out = vec![0u8; dsdt_at + HEADER_LEN + aml.len()];
    assert!(
        out.len() <= ACPI_MAX_LEN,
        "ACPI tables of {} bytes",
        out.len()
    );

    let rsdp = &mut out[..RSDP_LEN];
    rsdp[0..8].copy_from_slice(b"RSD PTR ");
    rsdp[9..15].copy_from_slice(b"HYPERM");
    rsdp[15] = 2;
    put32(rsdp, 20, RSDP_LEN as u32);
    put64(rsdp, 24, base + xsdt_at as u64);
    rsdp[8] = checksum(&rsdp[..20]);
    rsdp[32] = checksum(&rsdp[..RSDP_LEN]);

    let xsdt = &mut out[xsdt_at..xsdt_at + xsdt_len];
    header(xsdt, b"XSDT", 1);
    put64(xsdt, HEADER_LEN, base + fadt_at as u64);
    put64(xsdt, HEADER_LEN + 8, base + madt_at as u64);
    seal(xsdt);

    // The FACS has no checksum and nothing in it a guest that never sleeps
    // to memory uses; a fixed-hardware FADT is expected to name one.
    let facs = &mut out[facs_at..facs_at + FACS_LEN];
    facs[0..4].copy_from_slice(b"FACS");
    put32(facs, 4, FACS_LEN as u32);
    facs[32] = 2; // version

    let fadt = &mut out[fadt_at..fadt_at + FADT_LEN];
    header(fadt, b"FACP", 6);
    put32(fadt, 36, (base + facs_at as u64) as u32); // FIRMWARE_CTRL
    put32(fadt, 40, (base + dsdt_at as u64) as u32); // DSDT
    put16(fadt, 46, u16::from(SCI_IRQ)); // SCI_INT
                                         // SMI_CMD stays zero: there is no SMM to ask, and a guest reads that as
                                         // "already in ACPI mode".
    put32(fadt, 56, u32::from(PM1A_EVT_PORT)); // PM1a_EVT_BLK
    put32(fadt, 64, u32::from(PM1A_CNT_PORT)); // PM1a_CNT_BLK
    fadt[88] = 4; // PM1_EVT_LEN: status and enable, two bytes each
    fadt[89] = 2; // PM1_CNT_LEN
    put16(fadt, 96, 0x0FFF); // P_LVL2_LAT: no C2
    put16(fadt, 98, 0x0FFF); // P_LVL3_LAT: no C3
    put16(fadt, 109, IAPC_BOOT_ARCH_8042);
    // No fixed power or sleep button: nothing here presses one.
    put32(fadt, 112, FADT_PWR_BUTTON | FADT_SLP_BUTTON);
    fadt[131] = 5; // FADT minor version: ACPI 6.5
    put64(fadt, 132, base + facs_at as u64); // X_FIRMWARE_CTRL
    put64(fadt, 140, base + dsdt_at as u64); // X_DSDT
    gas_io(fadt, 148, 32, PM1A_EVT_PORT); // X_PM1a_EVT_BLK
    gas_io(fadt, 172, 16, PM1A_CNT_PORT); // X_PM1a_CNT_BLK
    seal(fadt);

    let madt = &mut out[madt_at..madt_at + madt_len];
    header(madt, b"APIC", 5);
    put32(madt, HEADER_LEN, LAPIC_ADDR);
    put32(madt, HEADER_LEN + 4, MADT_PCAT_COMPAT);
    let mut at = HEADER_LEN + 8;
    for id in 0..cpus {
        let id = id as u8;
        madt[at..at + 4].copy_from_slice(&[0, 8, id, id]); // processor local APIC
        put32(madt, at + 4, 1); // enabled
        at += 8;
    }
    madt[at..at + 4].copy_from_slice(&[1, 12, cpus as u8, 0]); // I/O APIC
    put32(madt, at + 4, IOAPIC_ADDR);
    put32(madt, at + 8, 0); // GSI base
    at += 12;
    // The timer is ISA IRQ 0 and arrives on input 2, as on every PC.
    madt[at..at + 4].copy_from_slice(&[2, 10, 0, 0]);
    put32(madt, at + 4, 2);
    put16(madt, at + 8, 0); // as the bus says: edge, active-high
    at += 10;
    for gsi in &level {
        madt[at..at + 4].copy_from_slice(&[2, 10, 0, *gsi]);
        put32(madt, at + 4, u32::from(*gsi));
        put16(madt, at + 8, LEVEL_ACTIVE_HIGH);
        at += 10;
    }
    // NMI on LINT1 of every processor.
    madt[at..at + 6].copy_from_slice(&[4, 6, 0xFF, 0, 0, 1]);
    seal(madt);

    let dsdt = &mut out[dsdt_at..];
    header(dsdt, b"DSDT", 2);
    dsdt[HEADER_LEN..].copy_from_slice(&aml);
    seal(dsdt);

    out
}

/// A Generic Address Structure for an I/O port register `bits` wide.
fn gas_io(table: &mut [u8], at: usize, bits: u8, port: u16) {
    table[at] = 1; // system I/O space
    table[at + 1] = bits;
    table[at + 2] = 0; // bit offset
    table[at + 3] = if bits == 16 { 2 } else { 3 }; // access size: word, dword
    put64(table, at + 4, u64::from(port));
}

/// `Name (\_S5, Package () {5, 5, 0, 0})`, then
/// `Scope (\_SB) { Device (COM1) {...} Device (PCI0) {...} }`, in AML.
fn pc_dsdt_body(pci: &[PciIntx]) -> Vec<u8> {
    let s5 = integer(u32::from(S5_SLEEP_TYPE));
    let mut out = name(b"_S5_", &package(&[s5.clone(), s5, integer(0), integer(0)]));

    let mut bridge = Vec::new();
    bridge.extend(name(b"_HID", &eisa_id(*b"PNP", 0x0A03)));
    bridge.extend(name(b"_UID", &integer(0)));
    bridge.extend(name(b"_ADR", &integer(0)));
    bridge.extend(name(b"_BBN", &integer(0)));
    // Bus 0 alone: nothing here is behind a bridge.
    let mut crs = vec![0x88, 0x0D, 0x00, 0x02, 0x0C, 0x00];
    for word in [0u16, 0, 0, 0, 1] {
        // granularity, minimum, maximum, translation, length
        crs.extend(word.to_le_bytes());
    }
    // The configuration mechanism's own ports.
    crs.extend([0x47, 0x01, 0xF8, 0x0C, 0xF8, 0x0C, 0x01, 0x08]);
    // The window the BARs are in: read-write, not cacheable, both ends fixed.
    crs.extend([0x87, 0x17, 0x00, 0x00, 0x0C, 0x01]);
    for dword in [
        0,
        PCI_WINDOW_BASE,
        PCI_WINDOW_BASE + (PCI_WINDOW_LEN - 1),
        0,
        PCI_WINDOW_LEN,
    ] {
        crs.extend(dword.to_le_bytes());
    }
    bridge.extend(name(b"_CRS", &resource_buffer(crs)));
    // Pin A of each slot to its input. A source of zero says the fourth
    // element is the input itself and not an index into a link device.
    let routes: Vec<Vec<u8>> = pci
        .iter()
        .map(|d| {
            package(&[
                dword((u32::from(d.slot) << 16) | 0xFFFF),
                integer(0),
                integer(0),
                integer(u32::from(d.gsi)),
            ])
        })
        .collect();
    bridge.extend(name(b"_PRT", &package(&routes)));

    let mut scope = com1();
    scope.extend(device(*b"PCI0", bridge));
    out.push(0x10); // ScopeOp
    let mut inner = vec![b'\\', b'_', b'S', b'B', b'_'];
    inner.extend(scope);
    out.extend(pkg_length(inner.len()));
    out.extend(inner);
    out
}

/// `Package () { elements... }`.
fn package(elements: &[Vec<u8>]) -> Vec<u8> {
    assert!(elements.len() < 256, "a package of {}", elements.len());
    let mut inner = vec![elements.len() as u8];
    for element in elements {
        inner.extend(element);
    }
    let mut out = vec![0x12]; // PackageOp
    out.extend(pkg_length(inner.len()));
    out.extend(inner);
    out
}

/// An integer in its shortest AML form.
fn integer(v: u32) -> Vec<u8> {
    match v {
        0 => vec![0x00],                 // ZeroOp
        1 => vec![0x01],                 // OneOp
        2..=0xFF => vec![0x0A, v as u8], // ByteConst
        0x100..=0xFFFF => {
            let mut out = vec![0x0B]; // WordConst
            out.extend((v as u16).to_le_bytes());
            out
        }
        _ => dword(v),
    }
}

/// `Device (COM1)`: the legacy UART, ISA IRQ 4.
fn com1() -> Vec<u8> {
    let mut com1 = Vec::new();
    com1.extend(name(b"_HID", &eisa_id(*b"PNP", 0x0501)));
    com1.extend(name(b"_UID", &[0x00])); // ZeroOp
    let mut crs = vec![0x47, 0x01]; // I/O port descriptor, 16-bit decode
    crs.extend(COM1_PORT.to_le_bytes()); // minimum
    crs.extend(COM1_PORT.to_le_bytes()); // maximum
    crs.extend([0x01, 0x08]); // alignment, length
    crs.extend([0x22]); // IRQ descriptor, no flags: edge, active-high
    crs.extend((1u16 << COM1_IRQ).to_le_bytes());
    com1.extend(name(b"_CRS", &resource_buffer(crs)));
    device(*b"COM1", com1)
}

/// `Scope (\_SB) { Device (COM1) {...} Device (V000) {...} ... }` in AML.
fn dsdt_body(devices: &[MmioDevice]) -> Vec<u8> {
    let mut scope = com1();

    for (i, d) in devices.iter().enumerate() {
        let base = u32::try_from(d.base).expect("virtio-mmio window below 4 GiB");
        let mut body = Vec::new();
        // The ID Linux's virtio_mmio driver matches on ACPI systems.
        let mut hid = vec![0x0D]; // StringPrefix
        hid.extend(b"LNRO0005\0");
        body.extend(name(b"_HID", &hid));
        body.extend(name(b"_UID", &dword(i as u32)));
        let mut crs = vec![0x86, 0x09, 0x00, 0x01]; // Memory32Fixed, read/write
        crs.extend(base.to_le_bytes());
        crs.extend(d.size.to_le_bytes());
        // Extended interrupt: consumer, edge, active-high, exclusive; one GSI.
        crs.extend([0x89, 0x06, 0x00, 0x03, 0x01]);
        crs.extend(d.gsi.to_le_bytes());
        body.extend(name(b"_CRS", &resource_buffer(crs)));
        let seg = format!("V{i:03}");
        scope.extend(device(seg.as_bytes().try_into().expect("four bytes"), body));
    }

    let mut out = vec![0x10]; // ScopeOp
    let mut inner = vec![b'\\', b'_', b'S', b'B', b'_'];
    inner.extend(scope);
    out.extend(pkg_length(inner.len()));
    out.extend(inner);
    out
}

/// `Name (seg, value)`.
fn name(seg: &[u8; 4], value: &[u8]) -> Vec<u8> {
    let mut out = vec![0x08];
    out.extend(seg);
    out.extend(value);
    out
}

/// `Device (seg) { body }`.
fn device(seg: [u8; 4], body: Vec<u8>) -> Vec<u8> {
    let mut inner = seg.to_vec();
    inner.extend(body);
    let mut out = vec![0x5B, 0x82]; // ExtOpPrefix, DeviceOp
    out.extend(pkg_length(inner.len()));
    out.extend(inner);
    out
}

/// A resource template: `Buffer` holding `descriptors` and an end tag.
fn resource_buffer(mut descriptors: Vec<u8>) -> Vec<u8> {
    descriptors.extend([0x79, 0x00]); // end tag, checksum 0 ("ignore")
    let size = descriptors.len();
    assert!(size < 256, "resource template of {size} bytes");
    let mut inner = vec![0x0A, size as u8]; // BufferSize as ByteConst
    inner.extend(descriptors);
    let mut out = vec![0x11]; // BufferOp
    out.extend(pkg_length(inner.len()));
    out.extend(inner);
    out
}

/// A `DWordConst`.
fn dword(v: u32) -> Vec<u8> {
    let mut out = vec![0x0C];
    out.extend(v.to_le_bytes());
    out
}

/// `EisaId ("PNPxxxx")`: three letters packed five bits each, then the
/// product number, stored big-endian in a little-endian `DWordConst`.
fn eisa_id(vendor: [u8; 3], product: u16) -> Vec<u8> {
    let packed = vendor
        .iter()
        .fold(0u16, |acc, c| (acc << 5) | u16::from(c - b'A' + 1));
    let [v0, v1] = packed.to_be_bytes();
    let [p0, p1] = product.to_be_bytes();
    dword(u32::from_le_bytes([v0, v1, p0, p1]))
}

/// An AML `PkgLength` for `content` bytes that follow it; the encoded length
/// counts its own bytes.
fn pkg_length(content: usize) -> Vec<u8> {
    if content + 1 < 0x40 {
        return vec![(content + 1) as u8];
    }
    let (extra, total) = if content + 2 < 0x1000 {
        (1, content + 2)
    } else if content + 3 < 0x10_0000 {
        (2, content + 3)
    } else {
        (3, content + 4)
    };
    let mut out = vec![((extra as u8) << 6) | (total & 0xF) as u8];
    for i in 0..extra {
        out.push((total >> (4 + 8 * i)) as u8);
    }
    out
}

fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// The FACS is to be 64-byte aligned.
fn align64(n: usize) -> usize {
    (n + 63) & !63
}

/// A table header with this table's own length; the checksum is set by [`seal`].
fn header(table: &mut [u8], signature: &[u8; 4], revision: u8) {
    table[0..4].copy_from_slice(signature);
    put32(table, 4, table.len() as u32);
    table[8] = revision;
    table[10..16].copy_from_slice(b"HYPERM");
    table[16..24].copy_from_slice(b"HMMICROV");
    put32(table, 24, 1); // OEM revision
    table[28..32].copy_from_slice(b"HYPM");
    put32(table, 32, 1); // creator revision
}

fn seal(table: &mut [u8]) {
    table[9] = 0;
    table[9] = checksum(table);
}

/// The byte that makes `bytes` sum to zero.
fn checksum(bytes: &[u8]) -> u8 {
    0u8.wrapping_sub(bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)))
}

fn put16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn u64_at(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    /// The table a guest physical address points at, as a slice of `blob`.
    fn table(blob: &[u8], addr: u64) -> &[u8] {
        let at = (addr - ACPI_ADDR) as usize;
        let len = u32_at(blob, at + 4) as usize;
        &blob[at..at + len]
    }

    fn sums_to_zero(bytes: &[u8]) -> bool {
        bytes.iter().fold(0u8, |s, b| s.wrapping_add(*b)) == 0
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    const VSOCK: MmioDevice = MmioDevice {
        base: 0xD000_0000,
        size: 0x1000,
        gsi: 5,
    };
    const NET: MmioDevice = MmioDevice {
        base: 0xD000_1000,
        size: 0x1000,
        gsi: 6,
    };

    #[test]
    fn tables_chain_from_the_rsdp_with_valid_checksums() {
        for (cpus, devices) in [(1, &[][..]), (2, &[VSOCK][..]), (32, &[VSOCK, NET][..])] {
            let blob = build(cpus, devices);
            assert!(blob.len() <= ACPI_MAX_LEN);
            assert_eq!(&blob[0..8], b"RSD PTR ");
            assert!(sums_to_zero(&blob[..20]) && sums_to_zero(&blob[..RSDP_LEN]));

            let xsdt = table(&blob, u64_at(&blob, 24));
            assert_eq!(&xsdt[0..4], b"XSDT");
            assert!(sums_to_zero(xsdt));
            let fadt = table(&blob, u64_at(xsdt, HEADER_LEN));
            let madt = table(&blob, u64_at(xsdt, HEADER_LEN + 8));

            assert_eq!(
                (&fadt[0..4], fadt.len(), fadt[8]),
                (&b"FACP"[..], FADT_LEN, 6)
            );
            assert!(sums_to_zero(fadt));
            assert_ne!(u32_at(fadt, 112) & FADT_HW_REDUCED_ACPI, 0);
            let dsdt = table(&blob, u64_at(fadt, 140));
            assert_eq!(&dsdt[0..4], b"DSDT");
            assert!(sums_to_zero(dsdt));

            assert_eq!(&madt[0..4], b"APIC");
            assert!(sums_to_zero(madt));
            assert_eq!(u32_at(madt, HEADER_LEN), LAPIC_ADDR);
            assert_eq!(u32_at(madt, HEADER_LEN + 4), 0, "no PCAT_COMPAT");
            let entries = &madt[HEADER_LEN + 8..];
            for id in 0..cpus as usize {
                assert_eq!(&entries[id * 8..id * 8 + 4], &[0, 8, id as u8, id as u8]);
            }
            let io = &entries[cpus as usize * 8..];
            assert_eq!(&io[0..3], &[1, 12, cpus as u8]);
            assert_eq!(u32_at(io, 4), IOAPIC_ADDR);
        }
    }

    #[test]
    fn dsdt_names_the_uart_and_each_window_with_its_interrupt() {
        let blob = build(1, &[VSOCK, NET]);
        let xsdt = table(&blob, u64_at(&blob, 24));
        let fadt = table(&blob, u64_at(xsdt, HEADER_LEN));
        let aml = &table(&blob, u64_at(fadt, 140))[HEADER_LEN..];

        // Scope (\_SB) spanning the whole body.
        assert_eq!(aml[0], 0x10);
        let (len, width) = decode_pkg_length(&aml[1..]);
        assert_eq!(1 + len, aml.len(), "scope length covers the body");
        assert_eq!(&aml[1 + width..1 + width + 5], b"\\_SB_");

        // COM1: EisaId("PNP0501") is the well-known 0x0105D041; port 0x3F8, IRQ 4.
        assert!(contains(aml, b"COM1"));
        assert!(contains(aml, &[0x0C, 0x41, 0xD0, 0x05, 0x01]));
        assert!(contains(
            aml,
            &[0x47, 0x01, 0xF8, 0x03, 0xF8, 0x03, 0x01, 0x08, 0x22, 0x10, 0x00]
        ));

        for (i, d) in [VSOCK, NET].iter().enumerate() {
            assert!(contains(aml, format!("V{i:03}").as_bytes()));
            let mut mem = vec![0x86, 0x09, 0x00, 0x01];
            mem.extend((d.base as u32).to_le_bytes());
            mem.extend(d.size.to_le_bytes());
            mem.extend([0x89, 0x06, 0x00, 0x03, 0x01]);
            mem.extend(d.gsi.to_le_bytes());
            mem.extend([0x79, 0x00]);
            assert!(contains(aml, &mem), "window {i}");
        }
        assert_eq!(aml.windows(8).filter(|w| *w == b"LNRO0005").count(), 2);
    }

    fn decode_pkg_length(b: &[u8]) -> (usize, usize) {
        let extra = (b[0] >> 6) as usize;
        if extra == 0 {
            return ((b[0] & 0x3F) as usize, 1);
        }
        let mut len = (b[0] & 0x0F) as usize;
        for i in 0..extra {
            len |= (b[1 + i] as usize) << (4 + 8 * i);
        }
        (len, 1 + extra)
    }

    const DISK: PciIntx = PciIntx { slot: 4, gsi: 10 };
    const NIC: PciIntx = PciIntx { slot: 5, gsi: 11 };
    const AGENT: PciIntx = PciIntx { slot: 3, gsi: 11 };

    /// The MADT's entries after its fixed part, each as its bytes.
    fn madt_entries(madt: &[u8]) -> Vec<&[u8]> {
        let mut entries = Vec::new();
        let mut at = HEADER_LEN + 8;
        while at < madt.len() {
            let len = madt[at + 1] as usize;
            entries.push(&madt[at..at + len]);
            at += len;
        }
        entries
    }

    #[test]
    fn a_pc_s_tables_chain_from_the_rsdp_and_name_its_power_registers() {
        for (cpus, pci) in [
            (1, &[][..]),
            (2, &[DISK][..]),
            (32, &[DISK, NIC, AGENT][..]),
        ] {
            let blob = build_pc(cpus, pci);
            assert!(blob.len() <= ACPI_MAX_LEN);
            assert_eq!(&blob[0..8], b"RSD PTR ");
            assert!(sums_to_zero(&blob[..20]) && sums_to_zero(&blob[..RSDP_LEN]));
            let xsdt = table(&blob, u64_at(&blob, 24));
            assert!(sums_to_zero(xsdt));
            let fadt = table(&blob, u64_at(xsdt, HEADER_LEN));
            let madt = table(&blob, u64_at(xsdt, HEADER_LEN + 8));
            assert!(sums_to_zero(fadt) && sums_to_zero(madt));

            // Fixed hardware, not hardware-reduced, with both forms of each
            // register agreeing and no SMI command to wait on.
            assert_eq!((&fadt[0..4], fadt.len()), (&b"FACP"[..], FADT_LEN));
            assert_eq!(u32_at(fadt, 112) & FADT_HW_REDUCED_ACPI, 0);
            assert_eq!(u32_at(fadt, 48), 0, "SMI_CMD");
            assert_eq!(u16::from_le_bytes([fadt[46], fadt[47]]), 9);
            assert_eq!(u32_at(fadt, 56), 0x600);
            assert_eq!(u32_at(fadt, 64), 0x604);
            assert_eq!((fadt[88], fadt[89]), (4, 2));
            assert_eq!((fadt[148], fadt[149], u64_at(fadt, 152)), (1, 32, 0x600));
            assert_eq!((fadt[172], fadt[173], u64_at(fadt, 176)), (1, 16, 0x604));
            // The 32-bit and 64-bit pointers name the same tables.
            assert_eq!(u64::from(u32_at(fadt, 40)), u64_at(fadt, 140));
            assert_eq!(u64::from(u32_at(fadt, 36)), u64_at(fadt, 132));
            let facs_at = (u64_at(fadt, 132) - ACPI_ADDR) as usize;
            assert_eq!(&blob[facs_at..facs_at + 4], b"FACS");
            assert_eq!(u64_at(fadt, 132) % 64, 0, "the FACS is 64-byte aligned");
            let dsdt = table(&blob, u64_at(fadt, 140));
            assert_eq!(&dsdt[0..4], b"DSDT");
            assert!(sums_to_zero(dsdt));

            // One local APIC per processor, the I/O APIC, the 8259s.
            assert_eq!(u32_at(madt, HEADER_LEN + 4), MADT_PCAT_COMPAT);
            let entries = madt_entries(madt);
            let lapics: Vec<u8> = entries.iter().filter(|e| e[0] == 0).map(|e| e[3]).collect();
            assert_eq!(lapics, (0..cpus as u8).collect::<Vec<_>>());
            let ioapics: Vec<&&[u8]> = entries.iter().filter(|e| e[0] == 1).collect();
            assert_eq!(ioapics.len(), 1);
            assert_eq!(ioapics[0][2], cpus as u8);
            // The timer on input 2; then each PCI input and the SCI, once
            // each, level-triggered and active-high.
            let overrides: Vec<(u8, u32, u16)> = entries
                .iter()
                .filter(|e| e[0] == 2)
                .map(|e| (e[3], u32_at(e, 4), u16::from_le_bytes([e[8], e[9]])))
                .collect();
            assert_eq!(overrides[0], (0, 2, 0));
            let mut wanted: Vec<u8> = pci.iter().map(|d| d.gsi).chain([9]).collect();
            wanted.sort_unstable();
            wanted.dedup();
            let level: Vec<(u8, u32, u16)> =
                wanted.iter().map(|g| (*g, u32::from(*g), 0x000D)).collect();
            assert_eq!(&overrides[1..], &level[..]);
            assert_eq!(entries.last().unwrap(), &&[4u8, 6, 0xFF, 0, 0, 1][..]);
        }
    }

    #[test]
    fn a_pc_s_dsdt_names_the_host_bridge_its_window_and_each_route() {
        let blob = build_pc(2, &[DISK, NIC]);
        let xsdt = table(&blob, u64_at(&blob, 24));
        let fadt = table(&blob, u64_at(xsdt, HEADER_LEN));
        let aml = &table(&blob, u64_at(fadt, 140))[HEADER_LEN..];

        // \_S5 first, at the root: Package (4) {5, 5, 0, 0}.
        assert_eq!(
            &aml[..13],
            &[0x08, b'_', b'S', b'5', b'_', 0x12, 0x08, 0x04, 0x0A, 0x05, 0x0A, 0x05, 0x00]
        );
        // Then Scope (\_SB) to the end.
        let scope = &aml[14..];
        assert_eq!(scope[0], 0x10);
        let (len, width) = decode_pkg_length(&scope[1..]);
        assert_eq!(1 + len, scope.len(), "scope length covers the rest");
        assert_eq!(&scope[1 + width..1 + width + 5], b"\\_SB_");

        assert!(contains(aml, b"COM1"));
        assert!(contains(aml, b"PCI0"));
        // EisaId("PNP0A03") is the well-known 0x030AD041.
        assert!(contains(aml, &[0x0C, 0x41, 0xD0, 0x0A, 0x03]));
        // Bus 0 only; the configuration ports; the BAR window.
        assert!(contains(
            aml,
            &[0x88, 0x0D, 0x00, 0x02, 0x0C, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]
        ));
        assert!(contains(
            aml,
            &[0x47, 0x01, 0xF8, 0x0C, 0xF8, 0x0C, 0x01, 0x08]
        ));
        let mut window = vec![0x87, 0x17, 0x00, 0x00, 0x0C, 0x01];
        for dword in [0u32, 0xD000_0000, 0xDFFF_FFFF, 0, 0x1000_0000] {
            window.extend(dword.to_le_bytes());
        }
        assert!(contains(aml, &window));
        // _PRT: a package of two, each {slot, pin A, no link, input}.
        let prt = aml.windows(4).position(|w| w == b"_PRT").expect("_PRT") + 4;
        assert_eq!((aml[prt], aml[prt + 2]), (0x12, 2));
        for d in [DISK, NIC] {
            let mut route = vec![0x12, 0x0B, 0x04, 0x0C];
            route.extend(((u32::from(d.slot) << 16) | 0xFFFF).to_le_bytes());
            route.extend([0x00, 0x00, 0x0A, d.gsi]);
            assert!(contains(aml, &route), "slot {}", d.slot);
        }
    }

    #[test]
    fn only_the_write_that_enters_s5_turns_the_machine_off() {
        assert!(is_power_off((5 << 10) | (1 << 13)));
        // With the SCI enable bit a guest leaves set.
        assert!(is_power_off((5 << 10) | (1 << 13) | 1));
        // The sleep type alone, written first by some guests: not yet.
        assert!(!is_power_off(5 << 10));
        // Another sleep state, entered.
        assert!(!is_power_off((1 << 10) | (1 << 13)));
        assert!(!is_power_off(0));
        assert!(!is_power_off(1));
    }

    #[test]
    fn integers_take_their_shortest_form() {
        assert_eq!(integer(0), [0x00]);
        assert_eq!(integer(1), [0x01]);
        assert_eq!(integer(10), [0x0A, 10]);
        assert_eq!(integer(0x1234), [0x0B, 0x34, 0x12]);
        assert_eq!(integer(0x0004_FFFF), [0x0C, 0xFF, 0xFF, 0x04, 0x00]);
    }

    #[test]
    fn pkg_length_counts_its_own_bytes() {
        for content in [0, 10, 62, 63, 64, 300, 4093, 4094, 70_000] {
            let enc = pkg_length(content);
            let (len, width) = decode_pkg_length(&enc);
            assert_eq!(width, enc.len());
            assert_eq!(len, content + width, "content {content}");
        }
    }
}
