//! The PVH boot protocol: how a firmware image is entered directly.
//!
//! PVH is Xen's direct-boot ABI for x86 HVM guests, and it is what firmware
//! built for hypervisors without a BIOS speaks: Rust Hypervisor Firmware and
//! the edk2 `CloudHv` target are both entered this way. The image is an ELF
//! whose note names a 32-bit entry point. The guest starts there in protected
//! mode with paging off and `EBX` pointing at a `hvm_start_info` structure,
//! which names the memory map and the ACPI RSDP. Nothing else is prepared: no
//! `boot_params`, no command line the image needs, no real-mode reset vector.
//!
//! This module places the image and builds that structure. Entering it is the
//! backend's part.

use std::ops::Range;

use crate::{Error, Result};

/// Where `hvm_start_info` goes. Below the boot stack and clear of the GDT
/// (`BootSetup::allocate_standard_tables`).
pub const START_INFO_ADDR: u64 = 0x6000;

/// Where the memory map `hvm_start_info` points at goes.
pub const MEMMAP_ADDR: u64 = 0x9000;

/// `hvm_start_info.magic`: "xEn3" with the high bit of the `E` set.
const START_INFO_MAGIC: u32 = 0x336e_c578;

/// The structure's version that carries a memory map.
const START_INFO_VERSION: u32 = 1;

/// `sizeof(struct hvm_start_info)` at version 1.
const START_INFO_SIZE: usize = 56;

/// `sizeof(struct hvm_memmap_table_entry)`.
const MEMMAP_ENTRY_SIZE: usize = 24;

/// The memory map's types are E820's.
const E820_RAM: u32 = 1;
const E820_RESERVED: u32 = 2;

/// Conventional memory ends here; the EBDA and the legacy ROM area follow.
const EBDA_START: u64 = 0x9_FC00;
const HIGH_MEMORY_START: u64 = 0x10_0000;

/// The note that names the entry point: owner `Xen`, this type.
const XEN_ELFNOTE_PHYS32_ENTRY: u32 = 18;

const PT_LOAD: u32 = 1;
const PT_NOTE: u32 = 4;

/// A firmware image to enter by PVH, and what the guest is told.
#[derive(Debug, Clone)]
pub struct PvhBoot {
    /// The ELF image.
    pub image: Vec<u8>,
    /// Guest RAM, which the memory map is built from. Zero until a VM exists.
    pub memory_size: u64,
    /// Guest physical address of the ACPI RSDP, or zero for none.
    pub rsdp_addr: u64,
}

/// Where an image's bytes go and where it is entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PvhPlacement {
    /// Guest physical address and the range of the image to copy there.
    pub regions: Vec<(u64, Range<usize>)>,
    /// Ranges that must read as zero: each segment's `.bss`.
    pub zeroed: Vec<(u64, u64)>,
    /// The 32-bit entry point from the image's PVH note.
    pub entry: u64,
}

fn invalid(what: &str) -> Error {
    Error::VM(format!("not a PVH image: {what}"))
}

fn bytes<const N: usize>(image: &[u8], at: usize) -> Result<[u8; N]> {
    image
        .get(
            at..at
                .checked_add(N)
                .ok_or_else(|| invalid("offset overflow"))?,
        )
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| invalid("truncated"))
}

fn u16_at(image: &[u8], at: usize) -> Result<u16> {
    bytes(image, at).map(u16::from_le_bytes)
}

fn u32_at(image: &[u8], at: usize) -> Result<u32> {
    bytes(image, at).map(u32::from_le_bytes)
}

fn u64_at(image: &[u8], at: usize) -> Result<u64> {
    bytes(image, at).map(u64::from_le_bytes)
}

/// One program header, whichever ELF class it came from.
struct Segment {
    kind: u32,
    offset: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
}

impl PvhBoot {
    /// A firmware image with nothing yet known about its VM.
    #[must_use]
    pub fn new(image: Vec<u8>) -> Self {
        Self {
            image,
            memory_size: 0,
            rsdp_addr: 0,
        }
    }

    fn segments(image: &[u8]) -> Result<Vec<Segment>> {
        if image.get(..4) != Some(b"\x7fELF") {
            return Err(invalid("no ELF header"));
        }
        let wide = match image.get(4) {
            Some(1) => false,
            Some(2) => true,
            _ => return Err(invalid("unknown ELF class")),
        };
        if image.get(5) != Some(&1) {
            return Err(invalid("not little-endian"));
        }
        let (phoff, phentsize, phnum) = if wide {
            (
                u64_at(image, 0x20)?,
                u16_at(image, 0x36)?,
                u16_at(image, 0x38)?,
            )
        } else {
            (
                u64::from(u32_at(image, 0x1C)?),
                u16_at(image, 0x2A)?,
                u16_at(image, 0x2C)?,
            )
        };
        let mut segments = Vec::with_capacity(usize::from(phnum));
        for index in 0..u64::from(phnum) {
            let at = phoff
                .checked_add(index * u64::from(phentsize))
                .and_then(|at| usize::try_from(at).ok())
                .ok_or_else(|| invalid("program header offset overflow"))?;
            segments.push(if wide {
                Segment {
                    kind: u32_at(image, at)?,
                    offset: u64_at(image, at + 8)?,
                    paddr: u64_at(image, at + 24)?,
                    filesz: u64_at(image, at + 32)?,
                    memsz: u64_at(image, at + 40)?,
                }
            } else {
                Segment {
                    kind: u32_at(image, at)?,
                    offset: u64::from(u32_at(image, at + 4)?),
                    paddr: u64::from(u32_at(image, at + 12)?),
                    filesz: u64::from(u32_at(image, at + 16)?),
                    memsz: u64::from(u32_at(image, at + 20)?),
                }
            });
        }
        Ok(segments)
    }

    /// The entry point a note segment names, if it holds the PVH note.
    fn entry_in_notes(notes: &[u8]) -> Result<Option<u64>> {
        let mut at = 0usize;
        while at + 12 <= notes.len() {
            let namesz = u32_at(notes, at)? as usize;
            let descsz = u32_at(notes, at + 4)? as usize;
            let kind = u32_at(notes, at + 8)?;
            let name_at = at + 12;
            let desc_at = name_at
                .checked_add(namesz.next_multiple_of(4))
                .ok_or_else(|| invalid("note overflow"))?;
            let next = desc_at
                .checked_add(descsz.next_multiple_of(4))
                .ok_or_else(|| invalid("note overflow"))?;
            let name = notes
                .get(name_at..name_at + namesz)
                .ok_or_else(|| invalid("truncated note"))?;
            if kind == XEN_ELFNOTE_PHYS32_ENTRY && name == b"Xen\0" {
                return match descsz {
                    4 => Ok(Some(u64::from(u32_at(notes, desc_at)?))),
                    8 => Ok(Some(u64_at(notes, desc_at)?)),
                    _ => Err(invalid("entry note of an unknown size")),
                };
            }
            at = next;
        }
        Ok(None)
    }

    /// Where the image's loadable segments go, and where it is entered.
    ///
    /// # Errors
    ///
    /// [`Error::VM`] for an image that is not an ELF, has no PVH entry note,
    /// has a segment outside the file, or is entered above 4 GiB.
    pub fn place(image: &[u8]) -> Result<PvhPlacement> {
        let mut placement = PvhPlacement {
            regions: Vec::new(),
            zeroed: Vec::new(),
            entry: 0,
        };
        let mut entry = None;
        for segment in Self::segments(image)? {
            let start =
                usize::try_from(segment.offset).map_err(|_| invalid("segment offset overflow"))?;
            let filesz =
                usize::try_from(segment.filesz).map_err(|_| invalid("segment size overflow"))?;
            let end = start
                .checked_add(filesz)
                .filter(|end| *end <= image.len())
                .ok_or_else(|| invalid("a segment lies outside the file"))?;
            match segment.kind {
                PT_LOAD => {
                    if segment.memsz < segment.filesz {
                        return Err(invalid("a segment is smaller in memory than in the file"));
                    }
                    segment
                        .paddr
                        .checked_add(segment.memsz)
                        .ok_or_else(|| invalid("segment address overflow"))?;
                    if filesz > 0 {
                        placement.regions.push((segment.paddr, start..end));
                    }
                    if segment.memsz > segment.filesz {
                        placement.zeroed.push((
                            segment.paddr + segment.filesz,
                            segment.memsz - segment.filesz,
                        ));
                    }
                }
                PT_NOTE if entry.is_none() => entry = Self::entry_in_notes(&image[start..end])?,
                _ => {}
            }
        }
        if placement.regions.is_empty() {
            return Err(invalid("nothing to load"));
        }
        placement.entry = entry.ok_or_else(|| invalid("no PVH entry note"))?;
        if placement.entry > u64::from(u32::MAX) {
            return Err(invalid("entered above 4 GiB"));
        }
        Ok(placement)
    }

    /// The memory map the guest is given: conventional memory, the legacy
    /// area as reserved, and RAM from 1 MiB up around the device hole.
    fn memory_map(&self) -> Vec<(u64, u64, u32)> {
        let mut entries = vec![
            (0, EBDA_START, E820_RAM),
            (EBDA_START, HIGH_MEMORY_START - EBDA_START, E820_RESERVED),
        ];
        for (start, len) in crate::memory::ram_ranges(self.memory_size) {
            let end = start + len;
            let start = start.max(HIGH_MEMORY_START);
            if end > start {
                entries.push((start, end - start, E820_RAM));
            }
        }
        entries
    }

    /// `hvm_start_info` and its memory map, each with its address.
    #[must_use]
    pub fn start_info(&self) -> Vec<(u64, Vec<u8>)> {
        let map = self.memory_map();
        let mut table = Vec::with_capacity(map.len() * MEMMAP_ENTRY_SIZE);
        for (addr, size, kind) in &map {
            table.extend_from_slice(&addr.to_le_bytes());
            table.extend_from_slice(&size.to_le_bytes());
            table.extend_from_slice(&kind.to_le_bytes());
            table.extend_from_slice(&0u32.to_le_bytes());
        }
        let mut info = Vec::with_capacity(START_INFO_SIZE);
        info.extend_from_slice(&START_INFO_MAGIC.to_le_bytes());
        info.extend_from_slice(&START_INFO_VERSION.to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes()); // flags
        info.extend_from_slice(&0u32.to_le_bytes()); // nr_modules
        info.extend_from_slice(&0u64.to_le_bytes()); // modlist_paddr
        info.extend_from_slice(&0u64.to_le_bytes()); // cmdline_paddr
        info.extend_from_slice(&self.rsdp_addr.to_le_bytes());
        info.extend_from_slice(&MEMMAP_ADDR.to_le_bytes());
        info.extend_from_slice(&(map.len() as u32).to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes()); // reserved
        debug_assert_eq!(info.len(), START_INFO_SIZE);
        vec![(START_INFO_ADDR, info), (MEMMAP_ADDR, table)]
    }

    /// Check the image can be entered in a guest of this size.
    ///
    /// # Errors
    ///
    /// As [`Self::place`], and when a segment does not fit in guest RAM above
    /// the structures this module writes.
    pub fn validate(&self) -> Result<()> {
        let placement = Self::place(&self.image)?;
        let lowest = placement
            .regions
            .iter()
            .map(|(addr, _)| *addr)
            .chain(placement.zeroed.iter().map(|(addr, _)| *addr))
            .min()
            .unwrap_or(0);
        if lowest < HIGH_MEMORY_START {
            return Err(invalid("a segment is loaded below 1 MiB"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ELF64 with one loadable segment and a PVH note.
    fn firmware(entry: u32, note_owner: &[u8; 4]) -> Vec<u8> {
        let mut image = vec![0u8; 0x200];
        image[..4].copy_from_slice(b"\x7fELF");
        image[4] = 2; // 64-bit
        image[5] = 1; // little-endian
        image[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes()); // e_phoff
        image[0x36..0x38].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        image[0x38..0x3A].copy_from_slice(&2u16.to_le_bytes()); // e_phnum
        let mut header =
            |index: usize, kind: u32, offset: u64, paddr: u64, filesz: u64, memsz: u64| {
                let at = 0x40 + index * 56;
                image[at..at + 4].copy_from_slice(&kind.to_le_bytes());
                image[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
                image[at + 24..at + 32].copy_from_slice(&paddr.to_le_bytes());
                image[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
                image[at + 40..at + 48].copy_from_slice(&memsz.to_le_bytes());
            };
        header(0, PT_LOAD, 0x100, 0x10_0000, 0x40, 0x1000);
        header(1, PT_NOTE, 0x180, 0, 20, 20);
        image[0x100..0x140].fill(0x90);
        let note = &mut image[0x180..0x180 + 20];
        note[..4].copy_from_slice(&4u32.to_le_bytes());
        note[4..8].copy_from_slice(&4u32.to_le_bytes());
        note[8..12].copy_from_slice(&XEN_ELFNOTE_PHYS32_ENTRY.to_le_bytes());
        note[12..16].copy_from_slice(note_owner);
        note[16..20].copy_from_slice(&entry.to_le_bytes());
        image
    }

    #[test]
    fn an_image_is_placed_by_its_segments_and_entered_at_its_note() {
        let image = firmware(0x10_0020, b"Xen\0");
        let placement = PvhBoot::place(&image).unwrap();
        assert_eq!(placement.regions, vec![(0x10_0000, 0x100..0x140)]);
        assert_eq!(placement.zeroed, vec![(0x10_0040, 0x1000 - 0x40)]);
        assert_eq!(placement.entry, 0x10_0020);
        assert!(PvhBoot::new(image).validate().is_ok());
    }

    #[test]
    fn an_image_that_is_not_pvh_is_refused_with_the_reason() {
        let no_note = firmware(0x10_0020, b"GNU\0");
        assert!(PvhBoot::place(&no_note)
            .unwrap_err()
            .to_string()
            .contains("no PVH entry note"));
        assert!(PvhBoot::place(b"MZ not an elf")
            .unwrap_err()
            .to_string()
            .contains("no ELF header"));
        let mut outside = firmware(0x10_0020, b"Xen\0");
        outside[0x40 + 32..0x40 + 40].copy_from_slice(&0x10_0000u64.to_le_bytes());
        assert!(PvhBoot::place(&outside)
            .unwrap_err()
            .to_string()
            .contains("outside the file"));
        let mut truncated = firmware(0x10_0020, b"Xen\0");
        truncated.truncate(0x60);
        assert!(PvhBoot::place(&truncated).is_err());
    }

    /// What a backend writes for a PVH boot: the image's bytes at its
    /// segment's address and the two structures, with the `.bss` reported
    /// apart and counted in how much memory the boot needs.
    #[test]
    fn a_loaded_boot_carries_the_image_and_what_it_is_told() {
        use crate::boot::source::LoadedBoot;
        let mut boot = LoadedBoot::Pvh(Box::new(PvhBoot::new(firmware(0x10_0020, b"Xen "))));
        boot.set_memory_size(64 << 20);
        boot.set_rsdp(0xE_0000);
        assert_eq!(boot.protocol(), "pvh");
        assert_eq!(boot.entry_point().unwrap(), 0x10_0020);
        assert_eq!(boot.cmdline(), None);
        boot.append_cmdline("ignored");

        let regions = boot.data_regions_borrowed().unwrap();
        let addresses: Vec<u64> = regions.iter().map(|(address, _)| *address).collect();
        assert_eq!(addresses, vec![0x10_0000, START_INFO_ADDR, MEMMAP_ADDR]);
        assert_eq!(regions[0].1.as_ref(), &[0x90u8; 0x40][..]);
        assert_eq!(u64_at(&regions[1].1, 32).unwrap(), 0xE_0000);
        assert_eq!(boot.data_regions().unwrap().len(), 3);
        assert_eq!(
            boot.zero_ranges().unwrap(),
            vec![(0x10_0040, 0x1000 - 0x40)]
        );
        assert_eq!(boot.highest_address().unwrap(), 0x10_1000);
    }

    #[test]
    fn the_start_info_names_the_memory_map_and_the_rsdp() {
        let mut boot = PvhBoot::new(firmware(0x10_0020, b"Xen\0"));
        boot.memory_size = 512 << 20;
        boot.rsdp_addr = 0xE_0000;
        let regions = boot.start_info();
        let (info_addr, info) = &regions[0];
        let (map_addr, map) = &regions[1];
        assert_eq!((*info_addr, *map_addr), (START_INFO_ADDR, MEMMAP_ADDR));
        assert_eq!(info.len(), START_INFO_SIZE);
        assert_eq!(u32_at(info, 0).unwrap(), START_INFO_MAGIC);
        assert_eq!(u32_at(info, 4).unwrap(), 1);
        assert_eq!(u64_at(info, 32).unwrap(), 0xE_0000);
        assert_eq!(u64_at(info, 40).unwrap(), MEMMAP_ADDR);
        assert_eq!(u32_at(info, 48).unwrap(), 3);
        assert_eq!(map.len(), 3 * MEMMAP_ENTRY_SIZE);
        let entry = |index: usize| {
            let at = index * MEMMAP_ENTRY_SIZE;
            (
                u64_at(map, at).unwrap(),
                u64_at(map, at + 8).unwrap(),
                u32_at(map, at + 16).unwrap(),
            )
        };
        assert_eq!(entry(0), (0, EBDA_START, E820_RAM));
        assert_eq!(
            entry(1),
            (EBDA_START, HIGH_MEMORY_START - EBDA_START, E820_RESERVED)
        );
        assert_eq!(
            entry(2),
            (HIGH_MEMORY_START, (512 << 20) - HIGH_MEMORY_START, E820_RAM)
        );
    }
}
