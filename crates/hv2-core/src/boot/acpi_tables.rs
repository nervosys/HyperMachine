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

/// `Scope (\_SB) { Device (COM1) {...} Device (V000) {...} ... }` in AML.
fn dsdt_body(devices: &[MmioDevice]) -> Vec<u8> {
    // COM1: legacy UART, ISA IRQ 4.
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
    let mut scope = device(*b"COM1", com1);

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
