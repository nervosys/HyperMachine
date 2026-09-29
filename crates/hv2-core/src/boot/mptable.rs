//! An Intel MultiProcessor Specification (1.4) table: how a Linux guest
//! booted without a BIOS or ACPI learns it has more than one CPU.
//!
//! Without one, Linux finds neither an ACPI MADT nor an MP table, says
//! "SMP disabled", and runs on the boot vCPU alone however many the VM has --
//! the others sit in KVM waiting for a startup IPI that never comes. With
//! one, it starts each application processor with INIT and SIPI, which the
//! in-kernel local APICs deliver, and brings the I/O APIC up in place of the
//! PIC's virtual wire.
//!
//! Written where Linux looks first: the floating pointer at the start of the
//! last KiB of base memory (`0x9FC00`), which the `e820` map leaves out of
//! RAM, and the configuration table right after it. One KiB holds the table
//! for [`MAX_CPUS`] processors.

/// Where the floating pointer goes: the last KiB below 640 KiB.
pub const MPTABLE_ADDR: u64 = 0x9_FC00;

/// The most processors the table is written for: what fits in the KiB.
pub const MAX_CPUS: u32 = 32;

/// Where every local APIC is, and the I/O APIC.
const LAPIC_ADDR: u32 = 0xFEE0_0000;
const IOAPIC_ADDR: u32 = 0xFEC0_0000;

/// ISA interrupts, each routed to the I/O APIC pin of its own number.
const IRQS: u8 = 24;

const FLOATING_LEN: usize = 16;
const HEADER_LEN: usize = 44;
const PROCESSOR_LEN: usize = 20;
const ENTRY_LEN: usize = 8;

/// The table for `cpus` processors, to be written at [`MPTABLE_ADDR`]: the
/// floating pointer, then the configuration table it points at. Processor
/// `i` has local APIC ID `i`, which is what KVM gives vCPU `i`; processor 0
/// boots.
///
/// # Panics
///
/// If `cpus` is 0 or more than [`MAX_CPUS`].
#[must_use]
pub fn build(cpus: u32) -> Vec<u8> {
    assert!(
        (1..=MAX_CPUS).contains(&cpus),
        "an MP table for {cpus} CPUs: 1 to {MAX_CPUS}"
    );
    let ioapic_id = u8::try_from(cpus).unwrap_or(u8::MAX);
    let table_addr = u32::try_from(MPTABLE_ADDR).unwrap_or(0) + FLOATING_LEN as u32;

    let mut entries = Vec::new();
    let mut count: u16 = 0;
    for id in 0..cpus {
        let id = u8::try_from(id).unwrap_or(u8::MAX);
        entries.push(0); // processor
        entries.push(id); // local APIC ID
        entries.push(0x14); // local APIC version
        entries.push(if id == 0 { 0b11 } else { 0b01 }); // enabled; BSP
        entries.extend_from_slice(&0x0600_u32.to_le_bytes()); // family 6
        entries.extend_from_slice(&0x0201_u32.to_le_bytes()); // FPU, APIC
        entries.extend_from_slice(&[0; 8]);
        count += 1;
    }
    entries.extend_from_slice(&[1, 0]); // bus 0
    entries.extend_from_slice(b"ISA   ");
    count += 1;
    entries.extend_from_slice(&[2, ioapic_id, 0x11, 1]); // I/O APIC, enabled
    entries.extend_from_slice(&IOAPIC_ADDR.to_le_bytes());
    count += 1;
    for irq in 0..IRQS {
        // An interrupt, conforming polarity and trigger, from ISA bus 0's
        // `irq` to the I/O APIC's pin `irq`.
        entries.extend_from_slice(&[3, 0, 0, 0, 0, irq, ioapic_id, irq]);
        count += 1;
    }
    // The local interrupts: ExtINT on LINT0, NMI on LINT1, of every APIC.
    entries.extend_from_slice(&[4, 3, 0, 0, 0, 0, 0xFF, 0]);
    entries.extend_from_slice(&[4, 1, 0, 0, 0, 0, 0xFF, 1]);
    count += 2;

    let mut table = Vec::with_capacity(HEADER_LEN + entries.len());
    table.extend_from_slice(b"PCMP");
    let length = u16::try_from(HEADER_LEN + entries.len()).unwrap_or(u16::MAX);
    table.extend_from_slice(&length.to_le_bytes());
    table.push(4); // spec 1.4
    table.push(0); // checksum, below
    table.extend_from_slice(b"HV2     ");
    table.extend_from_slice(b"HYPERMACHINE");
    table.extend_from_slice(&0_u32.to_le_bytes()); // no OEM table
    table.extend_from_slice(&0_u16.to_le_bytes());
    table.extend_from_slice(&count.to_le_bytes());
    table.extend_from_slice(&LAPIC_ADDR.to_le_bytes());
    table.extend_from_slice(&0_u16.to_le_bytes()); // no extended table
    table.extend_from_slice(&[0, 0]);
    debug_assert_eq!(table.len(), HEADER_LEN);
    table.extend_from_slice(&entries);
    table[7] = checksum(&table);

    let mut floating = Vec::with_capacity(FLOATING_LEN);
    floating.extend_from_slice(b"_MP_");
    floating.extend_from_slice(&table_addr.to_le_bytes());
    floating.push(1); // length, in 16-byte units
    floating.push(4); // spec 1.4
    floating.push(0); // checksum, below
    floating.extend_from_slice(&[0; 5]); // a configuration table, not a default
    floating[10] = checksum(&floating);

    floating.extend_from_slice(&table);
    floating
}

/// The byte that makes `bytes` sum to zero, as the specification checks.
fn checksum(bytes: &[u8]) -> u8 {
    0_u8.wrapping_sub(bytes.iter().fold(0_u8, |sum, b| sum.wrapping_add(*b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sums_to_zero(bytes: &[u8]) -> bool {
        bytes.iter().fold(0_u8, |sum, b| sum.wrapping_add(*b)) == 0
    }

    #[test]
    fn the_table_is_what_linux_parses() {
        for cpus in [1, 2, 4, MAX_CPUS] {
            let table = build(cpus);
            assert!(
                table.len() <= 1024,
                "{cpus} CPUs fit in the KiB: {}",
                table.len()
            );

            let floating = &table[..FLOATING_LEN];
            assert_eq!(&floating[..4], b"_MP_");
            assert!(sums_to_zero(floating), "the floating pointer's checksum");
            let points_at = u32::from_le_bytes(floating[4..8].try_into().unwrap());
            assert_eq!(u64::from(points_at), MPTABLE_ADDR + FLOATING_LEN as u64);

            let config = &table[FLOATING_LEN..];
            assert_eq!(&config[..4], b"PCMP");
            let length = u16::from_le_bytes(config[4..6].try_into().unwrap());
            assert_eq!(usize::from(length), config.len());
            assert!(sums_to_zero(config), "the configuration table's checksum");

            let count = u16::from_le_bytes(config[34..36].try_into().unwrap());
            assert_eq!(u32::from(count), cpus + 2 + u32::from(IRQS) + 2);

            // Each processor, its APIC ID its index, the first the BSP.
            for id in 0..cpus as usize {
                let at = HEADER_LEN + id * PROCESSOR_LEN;
                assert_eq!(config[at], 0, "a processor entry");
                assert_eq!(usize::from(config[at + 1]), id);
                assert_eq!(config[at + 3] & 0b10 != 0, id == 0, "BSP only on 0");
                assert!(config[at + 3] & 1 != 0, "enabled");
            }
        }
    }

    #[test]
    #[should_panic(expected = "an MP table for 0 CPUs")]
    fn no_table_for_no_cpus() {
        let _ = build(0);
    }
}
