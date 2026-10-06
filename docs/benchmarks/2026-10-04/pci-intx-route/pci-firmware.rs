//! Conventional MP-table PCI INTx routing for the reserved PC firmware page.
//! Entry layouts follow the Intel MP specification (Linux mpspec_def.h).
use crate::{memory::GuestMemory, Error, Result};

const START: u64 = 0x9fc00;
const END: u64 = 0xa0000;
const HEADER: usize = 44;
fn bad(message: &str) -> Error {
    Error::Config(format!("PCI MP routing: {message}"))
}
fn sum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |n, b| n.wrapping_add(*b))
}

fn add_route(table: &[u8], slot: u8, pin: u8, irq: u8) -> Result<Vec<u8>> {
    if slot >= 32 || pin >= 4 || irq >= 24 {
        return Err(bad("route out of range"));
    }
    if table.len() < HEADER
        || &table[..4] != b"PCMP"
        || sum(table) != 0
        || u16::from_le_bytes([table[4], table[5]]) as usize != table.len()
        || !matches!(table[6], 1 | 4)
        || table[40..44] != [0, 0, 0, 0]
    {
        return Err(bad("invalid or extended configuration table"));
    }
    let mut entries = Vec::new();
    let mut at = HEADER;
    while at < table.len() {
        let len = match table[at] {
            0 => 20,
            1..=4 => 8,
            _ => return Err(bad("unknown entry type")),
        };
        if at + len > table.len() {
            return Err(bad("truncated entry"));
        }
        entries.push(&table[at..at + len]);
        at += len;
    }
    if entries.len() != u16::from_le_bytes([table[34], table[35]]) as usize {
        return Err(bad("entry count mismatch"));
    }
    let ioapic = entries
        .iter()
        .find(|e| e[0] == 2 && e[3] & 1 != 0)
        .ok_or_else(|| bad("no enabled IOAPIC"))?[1];
    let buses: Vec<_> = entries.iter().filter(|e| e[0] == 1).collect();
    let existing_pci = buses.iter().find(|e| &e[2..8] == b"PCI   ").map(|e| e[1]);
    let bus = match existing_pci {
        Some(bus) => bus,
        None => (0..=255)
            .find(|id| !buses.iter().any(|e| e[1] == *id))
            .ok_or_else(|| bad("no unused bus identifier"))?,
    };
    let source = slot * 4 + pin;
    let route = [3, 0, 15, 0, bus, source, ioapic, irq];
    for entry in &entries {
        if entry[0] == 3 && entry[1] == 0 && entry[4] == bus && entry[5] == source {
            if *entry == route {
                return Ok(table.to_vec());
            }
            return Err(bad("existing PCI route conflicts"));
        }
    }
    let mut result = table.to_vec();
    let mut added = 1;
    if existing_pci.is_none() {
        result.extend_from_slice(&[1, bus, b'P', b'C', b'I', b' ', b' ', b' ']);
        added += 1;
    }
    result.extend_from_slice(&route);
    let length = u16::try_from(result.len()).map_err(|_| bad("table too long"))?;
    let count = u16::try_from(entries.len() + added).map_err(|_| bad("too many entries"))?;
    result[4..6].copy_from_slice(&length.to_le_bytes());
    result[34..36].copy_from_slice(&count.to_le_bytes());
    result[7] = 0;
    result[7] = 0u8.wrapping_sub(sum(&result));
    Ok(result)
}

/// Append a level-triggered, active-low PCI route before the guest starts.
/// Refuse unsupported/corrupt layouts or occupied padding rather than damage it.
pub(crate) fn route_intx(memory: &GuestMemory, slot: u8, pin: u8, irq: u8) -> Result<()> {
    let page = memory.read_bytes(START, (END - START) as usize)?;
    let offset = (0..page.len() - 15)
        .step_by(16)
        .find(|at| {
            &page[*at..*at + 4] == b"_MP_" && page[*at + 8] == 1 && sum(&page[*at..*at + 16]) == 0
        })
        .ok_or_else(|| bad("no valid floating pointer in reserved firmware page"))?;
    let pointer = &page[offset..offset + 16];
    if pointer[11] != 0 {
        return Err(bad("default MP configuration unsupported"));
    }
    let address = u32::from_le_bytes(pointer[4..8].try_into().unwrap()) as u64;
    if address < START || address + HEADER as u64 > END {
        return Err(bad("table outside firmware page"));
    }
    let header = memory.read_bytes(address, HEADER)?;
    let length = u16::from_le_bytes([header[4], header[5]]) as usize;
    if length < HEADER || address + length as u64 > END {
        return Err(bad("table bounds invalid"));
    }
    let old = memory.read_bytes(address, length)?;
    let new = add_route(&old, slot, pin, irq)?;
    let new_end = address + new.len() as u64;
    if new_end > END {
        return Err(bad("no reserved table capacity"));
    }
    let float_start = START + offset as u64;
    if address < float_start + 16 && new_end > float_start {
        return Err(bad("table overlaps floating pointer"));
    }
    if new.len() > old.len()
        && memory
            .read_bytes(address + old.len() as u64, new.len() - old.len())?
            .iter()
            .any(|b| *b != 0)
    {
        return Err(bad("table expansion would overwrite occupied firmware"));
    }
    memory.write_bytes(address, &new)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table() -> Vec<u8> {
        let mut v = vec![0; HEADER];
        v[..4].copy_from_slice(b"PCMP");
        v[6] = 4;
        v.extend_from_slice(&[1, 0, b'I', b'S', b'A', b' ', b' ', b' ']);
        v.extend_from_slice(&[2, 2, 0x20, 1, 0, 0, 0xc0, 0xfe]);
        v.extend_from_slice(&[3, 0, 0, 0, 0, 4, 2, 4]);
        let length = v.len() as u16;
        v[4..6].copy_from_slice(&length.to_le_bytes());
        v[34] = 3;
        v[7] = 0u8.wrapping_sub(sum(&v));
        v
    }
    #[test]
    fn pci_route_preserves_legacy_entries_and_checksums_and_is_idempotent() {
        let old = table();
        let new = add_route(&old, 3, 0, 11).unwrap();
        assert_eq!(&new[HEADER..old.len()], &old[HEADER..]);
        assert_eq!(sum(&new), 0);
        assert_eq!(new[34], 5);
        assert_eq!(&new[new.len() - 8..], &[3, 0, 15, 0, 1, 12, 2, 11]);
        assert_eq!(add_route(&new, 3, 0, 11).unwrap(), new);
        assert!(add_route(&new, 3, 0, 10).is_err());
    }
    #[test]
    fn corrupt_table_and_invalid_routes_are_refused() {
        let mut v = table();
        v[7] ^= 1;
        assert!(add_route(&v, 3, 0, 11).is_err());
        for (slot, pin, irq) in [(32, 0, 11), (3, 4, 11), (3, 0, 24)] {
            assert!(add_route(&table(), slot, pin, irq).is_err());
        }
    }
    #[test]
    fn firmware_expansion_refuses_occupied_bytes_without_any_write() {
        let memory = GuestMemory::new(1024 * 1024).unwrap();
        memory.allocate_region(1024 * 1024, false).unwrap();
        let address = START + 16;
        let v = table();
        let mut f = [0u8; 16];
        f[..4].copy_from_slice(b"_MP_");
        f[4..8].copy_from_slice(&(address as u32).to_le_bytes());
        f[8] = 1;
        f[9] = 4;
        f[10] = 0u8.wrapping_sub(sum(&f));
        memory.write_bytes(START, &f).unwrap();
        memory.write_bytes(address, &v).unwrap();
        memory
            .write_bytes(address + v.len() as u64, &[0x55])
            .unwrap();
        let before = memory.read_bytes(START, (END - START) as usize).unwrap();
        assert!(route_intx(&memory, 3, 0, 11).is_err());
        assert_eq!(
            memory.read_bytes(START, (END - START) as usize).unwrap(),
            before
        );
        memory.write_bytes(address + v.len() as u64, &[0]).unwrap();
        route_intx(&memory, 3, 0, 11).unwrap();
        assert_eq!(memory.read_bytes(START, 16).unwrap(), f);
        assert_eq!(
            memory.read_bytes(address, v.len() + 16).unwrap(),
            add_route(&v, 3, 0, 11).unwrap()
        );
    }
}
