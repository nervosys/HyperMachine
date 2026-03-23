//! ARM64 Stage-2 address translation (guest physical → host physical)
//!
//! Stage-2 translation is configured via VTTBR_EL2 and VTCR_EL2 and
//! enforces memory isolation between VMs.  Each VM gets its own set of
//! stage-2 page tables.
//!
//! # Page sizes and levels
//!
//! With 4 KB granule and 40-bit IPA (T0SZ = 24):
//!
//! | Level | Entry covers | Index bits (IPA) |
//! |-------|-------------|------------------|
//! | 1     | 1 GB        | \[39:30\]          |
//! | 2     | 2 MB        | \[29:21\]          |
//! | 3     | 4 KB        | \[20:12\]          |
//!
//! # Descriptor format (stage-2)
//!
//! - Bits \[1:0\]: valid + table/block
//! - Bits \[47:12\]: output address (OA)
//! - Bits \[7:6\]: S2AP (stage-2 access permissions)
//! - Bits \[5:4\]: memory attributes (MemAttr)

use crate::{Error, Result};
use bitflags::bitflags;

/// 4 KB page size
pub const PAGE_SIZE: usize = 4096;
/// 2 MB large page (block) size
pub const LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024;
/// 1 GB huge page (block) size
pub const HUGE_PAGE_SIZE: usize = 1024 * 1024 * 1024;
/// Number of entries per page table (4 KB / 8 bytes)
pub const ENTRIES_PER_TABLE: usize = 512;

/// Bits \[47:12\] mask for output address
const OA_MASK: u64 = 0x0000_FFFF_FFFF_F000;

bitflags! {
    /// Stage-2 descriptor attribute bits
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Stage2Attrs: u64 {
        /// Descriptor is valid
        const VALID         = 1 << 0;
        /// Table descriptor (level 0-2) or Page descriptor (level 3)
        const TABLE_OR_PAGE = 1 << 1;
        /// Stage-2 access permission: read
        const S2AP_READ     = 1 << 6;
        /// Stage-2 access permission: write
        const S2AP_WRITE    = 1 << 7;
        /// Memory attribute: Device-nGnRnE (bits \[5:4\] = 0b00)
        const MEMATTR_DEVICE = 0 << 4;
        /// Memory attribute: Normal (bits \[5:4\] = 0b11 → outer/inner write-back)
        const MEMATTR_NORMAL = 0b11 << 4;
        /// Access flag
        const AF             = 1 << 10;
        /// Execute-never for EL1
        const XN             = 1 << 54;
    }
}

impl Stage2Attrs {
    /// Default attributes for a normal RAM mapping (RWX).
    pub fn normal_ram() -> Self {
        Self::VALID
            | Self::TABLE_OR_PAGE
            | Self::S2AP_READ
            | Self::S2AP_WRITE
            | Self::MEMATTR_NORMAL
            | Self::AF
    }

    /// Attributes for a device MMIO mapping (RW, no-exec, device memory).
    pub fn device_mmio() -> Self {
        Self::VALID
            | Self::TABLE_OR_PAGE
            | Self::S2AP_READ
            | Self::S2AP_WRITE
            | Self::MEMATTR_DEVICE
            | Self::AF
            | Self::XN
    }

    /// Read-only normal RAM mapping.
    pub fn normal_rom() -> Self {
        Self::VALID | Self::TABLE_OR_PAGE | Self::S2AP_READ | Self::MEMATTR_NORMAL | Self::AF
    }
}

/// A stage-2 page table descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor(u64);

impl Descriptor {
    /// Invalid (zero) descriptor.
    pub const INVALID: Self = Self(0);

    /// Create a table descriptor pointing at the next-level table.
    pub fn table(next_table_phys: u64) -> Self {
        Self((next_table_phys & OA_MASK) | 0b11)
    }

    /// Create a block descriptor (1 GB at level 1, 2 MB at level 2).
    pub fn block(output_addr: u64, attrs: Stage2Attrs) -> Self {
        // Block entries have bit[1] = 0 (not table/page), bit[0] = 1 (valid)
        Self((output_addr & OA_MASK) | (attrs.bits() & !0b10) | 0b01)
    }

    /// Create a level-3 page descriptor (4 KB).
    pub fn page(output_addr: u64, attrs: Stage2Attrs) -> Self {
        Self((output_addr & OA_MASK) | attrs.bits())
    }

    /// Check if this descriptor is valid.
    pub fn is_valid(&self) -> bool {
        self.0 & 1 != 0
    }

    /// Check if this is a table descriptor (not a block/page).
    pub fn is_table(&self) -> bool {
        self.0 & 0b11 == 0b11
    }

    /// Extract the output address.
    pub fn output_addr(&self) -> u64 {
        self.0 & OA_MASK
    }

    /// Raw 64-bit value.
    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// A guest physical address region mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stage2Mapping {
    /// Guest IPA (Intermediate Physical Address) start
    pub ipa: u64,
    /// Host physical address start
    pub hpa: u64,
    /// Region size in bytes
    pub size: u64,
    /// Mapping attributes
    pub attrs: Stage2Attrs,
}

/// Stage-2 page table manager for a single VM.
#[derive(Debug)]
pub struct Stage2PageTable {
    /// VMID for VTTBR_EL2
    vmid: u16,
    /// Recorded mappings (software tracking)
    mappings: alloc::vec::Vec<Stage2Mapping>,
}

impl Stage2PageTable {
    /// Create a new empty stage-2 page table for the given VMID.
    pub fn new(vmid: u16) -> Self {
        Self {
            vmid,
            mappings: alloc::vec::Vec::new(),
        }
    }

    /// Get the VMID.
    pub fn vmid(&self) -> u16 {
        self.vmid
    }

    /// Map a guest IPA region to a host physical address.
    ///
    /// Both `ipa` and `hpa` must be page-aligned. `size` must be a
    /// non-zero multiple of PAGE_SIZE.
    pub fn map_region(&mut self, mapping: Stage2Mapping) -> Result<()> {
        // Validate alignment
        if mapping.ipa & (PAGE_SIZE as u64 - 1) != 0 {
            return Err(Error::InvalidStage2Mapping);
        }
        if mapping.hpa & (PAGE_SIZE as u64 - 1) != 0 {
            return Err(Error::InvalidStage2Mapping);
        }
        if mapping.size == 0 || mapping.size & (PAGE_SIZE as u64 - 1) != 0 {
            return Err(Error::InvalidStage2Mapping);
        }

        // Check for overlapping IPA regions
        let new_end = mapping
            .ipa
            .checked_add(mapping.size)
            .ok_or(Error::InvalidStage2Mapping)?;

        for existing in &self.mappings {
            let existing_end = existing.ipa + existing.size;
            if mapping.ipa < existing_end && new_end > existing.ipa {
                return Err(Error::OverlappingMapping);
            }
        }

        self.mappings.push(mapping);
        Ok(())
    }

    /// Remove all mappings that overlap the given IPA range.
    pub fn unmap_region(&mut self, ipa: u64, size: u64) -> Result<()> {
        if size == 0 {
            return Err(Error::InvalidParameter);
        }
        let end = ipa.checked_add(size).ok_or(Error::InvalidParameter)?;
        self.mappings
            .retain(|m| m.ipa + m.size <= ipa || m.ipa >= end);
        Ok(())
    }

    /// Look up which mapping covers a given IPA.
    pub fn lookup(&self, ipa: u64) -> Option<&Stage2Mapping> {
        self.mappings
            .iter()
            .find(|m| ipa >= m.ipa && ipa < m.ipa + m.size)
    }

    /// Translate an IPA to a host physical address.
    pub fn translate(&self, ipa: u64) -> Option<u64> {
        self.lookup(ipa).map(|m| m.hpa + (ipa - m.ipa))
    }

    /// Number of recorded mappings.
    pub fn mapping_count(&self) -> usize {
        self.mappings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Descriptor tests ---

    #[test]
    fn descriptor_invalid() {
        let d = Descriptor::INVALID;
        assert!(!d.is_valid());
        assert_eq!(d.raw(), 0);
    }

    #[test]
    fn descriptor_table() {
        let d = Descriptor::table(0x1000);
        assert!(d.is_valid());
        assert!(d.is_table());
        assert_eq!(d.output_addr(), 0x1000);
    }

    #[test]
    fn descriptor_block() {
        let d = Descriptor::block(0x4000_0000, Stage2Attrs::normal_ram());
        assert!(d.is_valid());
        assert!(!d.is_table()); // Block, not table
    }

    #[test]
    fn descriptor_page() {
        let d = Descriptor::page(0x2000, Stage2Attrs::normal_ram());
        assert!(d.is_valid());
        assert_eq!(d.output_addr(), 0x2000);
    }

    // -- Stage2Attrs tests ---

    #[test]
    fn attrs_normal_ram_is_valid() {
        let attrs = Stage2Attrs::normal_ram();
        assert!(attrs.contains(Stage2Attrs::VALID));
        assert!(attrs.contains(Stage2Attrs::S2AP_READ));
        assert!(attrs.contains(Stage2Attrs::S2AP_WRITE));
        assert!(attrs.contains(Stage2Attrs::AF));
    }

    #[test]
    fn attrs_device_mmio_is_xn() {
        let attrs = Stage2Attrs::device_mmio();
        assert!(attrs.contains(Stage2Attrs::XN));
    }

    #[test]
    fn attrs_normal_rom_is_read_only() {
        let attrs = Stage2Attrs::normal_rom();
        assert!(attrs.contains(Stage2Attrs::S2AP_READ));
        assert!(!attrs.contains(Stage2Attrs::S2AP_WRITE));
    }

    // -- Stage2PageTable tests ---

    #[test]
    fn stage2_new() {
        let pt = Stage2PageTable::new(42);
        assert_eq!(pt.vmid(), 42);
        assert_eq!(pt.mapping_count(), 0);
    }

    #[test]
    fn stage2_map_and_lookup() {
        let mut pt = Stage2PageTable::new(1);
        let mapping = Stage2Mapping {
            ipa: 0x0000_0000,
            hpa: 0x8000_0000,
            size: 0x1000,
            attrs: Stage2Attrs::normal_ram(),
        };
        pt.map_region(mapping).unwrap();
        assert_eq!(pt.mapping_count(), 1);

        let found = pt.lookup(0).unwrap();
        assert_eq!(found.hpa, 0x8000_0000);
    }

    #[test]
    fn stage2_translate() {
        let mut pt = Stage2PageTable::new(1);
        pt.map_region(Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0x10_0000,
            attrs: Stage2Attrs::normal_ram(),
        })
        .unwrap();

        assert_eq!(pt.translate(0x0), Some(0x8000_0000));
        assert_eq!(pt.translate(0x1234), Some(0x8000_1234));
        assert_eq!(pt.translate(0x10_0000), None); // past end
    }

    #[test]
    fn stage2_reject_unaligned_ipa() {
        let mut pt = Stage2PageTable::new(1);
        let mapping = Stage2Mapping {
            ipa: 0x123, // not page-aligned
            hpa: 0x8000_0000,
            size: 0x1000,
            attrs: Stage2Attrs::normal_ram(),
        };
        assert_eq!(pt.map_region(mapping), Err(Error::InvalidStage2Mapping));
    }

    #[test]
    fn stage2_reject_unaligned_hpa() {
        let mut pt = Stage2PageTable::new(1);
        let mapping = Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0001, // not page-aligned
            size: 0x1000,
            attrs: Stage2Attrs::normal_ram(),
        };
        assert_eq!(pt.map_region(mapping), Err(Error::InvalidStage2Mapping));
    }

    #[test]
    fn stage2_reject_zero_size() {
        let mut pt = Stage2PageTable::new(1);
        let mapping = Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0,
            attrs: Stage2Attrs::normal_ram(),
        };
        assert_eq!(pt.map_region(mapping), Err(Error::InvalidStage2Mapping));
    }

    #[test]
    fn stage2_reject_misaligned_size() {
        let mut pt = Stage2PageTable::new(1);
        let mapping = Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0x1001, // not page-aligned
            attrs: Stage2Attrs::normal_ram(),
        };
        assert_eq!(pt.map_region(mapping), Err(Error::InvalidStage2Mapping));
    }

    #[test]
    fn stage2_reject_overlapping() {
        let mut pt = Stage2PageTable::new(1);
        pt.map_region(Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0x10_0000,
            attrs: Stage2Attrs::normal_ram(),
        })
        .unwrap();

        // Exact overlap
        assert_eq!(
            pt.map_region(Stage2Mapping {
                ipa: 0x0,
                hpa: 0x9000_0000,
                size: 0x10_0000,
                attrs: Stage2Attrs::normal_ram(),
            }),
            Err(Error::OverlappingMapping)
        );

        // Partial overlap at end
        assert_eq!(
            pt.map_region(Stage2Mapping {
                ipa: 0x8_0000,
                hpa: 0xA000_0000,
                size: 0x10_0000,
                attrs: Stage2Attrs::normal_ram(),
            }),
            Err(Error::OverlappingMapping)
        );
    }

    #[test]
    fn stage2_allow_adjacent() {
        let mut pt = Stage2PageTable::new(1);
        pt.map_region(Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0x10_0000,
            attrs: Stage2Attrs::normal_ram(),
        })
        .unwrap();

        // Immediately adjacent — should succeed
        assert!(pt
            .map_region(Stage2Mapping {
                ipa: 0x10_0000,
                hpa: 0x9000_0000,
                size: 0x10_0000,
                attrs: Stage2Attrs::normal_ram(),
            })
            .is_ok());
        assert_eq!(pt.mapping_count(), 2);
    }

    #[test]
    fn stage2_unmap_region() {
        let mut pt = Stage2PageTable::new(1);
        pt.map_region(Stage2Mapping {
            ipa: 0x0,
            hpa: 0x8000_0000,
            size: 0x1000,
            attrs: Stage2Attrs::normal_ram(),
        })
        .unwrap();
        pt.map_region(Stage2Mapping {
            ipa: 0x10_0000,
            hpa: 0x9000_0000,
            size: 0x1000,
            attrs: Stage2Attrs::normal_ram(),
        })
        .unwrap();
        assert_eq!(pt.mapping_count(), 2);

        pt.unmap_region(0x0, 0x2000).unwrap();
        assert_eq!(pt.mapping_count(), 1);
        assert!(pt.lookup(0).is_none());
        assert!(pt.lookup(0x10_0000).is_some());
    }

    #[test]
    fn stage2_unmap_zero_size_fails() {
        let mut pt = Stage2PageTable::new(1);
        assert_eq!(pt.unmap_region(0, 0), Err(Error::InvalidParameter));
    }

    #[test]
    fn stage2_lookup_miss() {
        let pt = Stage2PageTable::new(1);
        assert!(pt.lookup(0x1000).is_none());
    }

    #[test]
    fn stage2_multiple_mappings() {
        let mut pt = Stage2PageTable::new(1);
        for i in 0..10u64 {
            pt.map_region(Stage2Mapping {
                ipa: i * 0x10_0000,
                hpa: 0x8000_0000 + i * 0x10_0000,
                size: 0x10_0000,
                attrs: Stage2Attrs::normal_ram(),
            })
            .unwrap();
        }
        assert_eq!(pt.mapping_count(), 10);
        assert_eq!(pt.translate(0x5_0000 * 16 + 0x42), Some(0x8050_0042));
    }
}
