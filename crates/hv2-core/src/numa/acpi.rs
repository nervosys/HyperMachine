//! ACPI SRAT and SLIT table generation for NUMA
//!
//! This module provides structures and functions for generating ACPI tables
//! that describe NUMA topology to the guest operating system.

use super::topology::NumaTopology;
use super::types::{CpuAffinity, DistanceMatrix, MemoryAffinity, MemoryRange, NodeId};

/// ACPI SRAT signature
pub const SRAT_SIGNATURE: [u8; 4] = *b"SRAT";

/// ACPI SLIT signature
pub const SLIT_SIGNATURE: [u8; 4] = *b"SLIT";

/// SRAT revision
pub const SRAT_REVISION: u8 = 3;

/// SLIT revision
pub const SLIT_REVISION: u8 = 1;

/// SRAT subtable types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SratSubtableType {
    /// Processor Local APIC/SAPIC Affinity
    ProcessorLocalApic = 0,
    /// Memory Affinity
    Memory = 1,
    /// Processor Local x2APIC Affinity
    ProcessorX2Apic = 2,
    /// GICC Affinity (ARM)
    GiccAffinity = 3,
    /// GIC ITS Affinity (ARM)
    GicItsAffinity = 4,
    /// Generic Initiator Affinity
    GenericInitiator = 5,
}

/// Flags for processor affinity entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorAffinityFlags(pub u32);

impl ProcessorAffinityFlags {
    /// Entry is enabled
    pub const ENABLED: ProcessorAffinityFlags = ProcessorAffinityFlags(1 << 0);

    /// Empty flags
    pub const fn empty() -> Self {
        ProcessorAffinityFlags(0)
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.0 & 1 != 0
    }
}

/// Flags for memory affinity entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryAffinityFlags(pub u32);

impl MemoryAffinityFlags {
    /// Memory region is enabled
    pub const ENABLED: MemoryAffinityFlags = MemoryAffinityFlags(1 << 0);
    /// Memory is hot-pluggable
    pub const HOTPLUGGABLE: MemoryAffinityFlags = MemoryAffinityFlags(1 << 1);
    /// Memory is non-volatile
    pub const NON_VOLATILE: MemoryAffinityFlags = MemoryAffinityFlags(1 << 2);

    /// Empty flags
    pub const fn empty() -> Self {
        MemoryAffinityFlags(0)
    }

    /// Create flags from memory range attributes
    pub fn from_range(range: &MemoryRange, enabled: bool) -> Self {
        let mut flags = 0u32;
        if enabled {
            flags |= Self::ENABLED.0;
        }
        if range.hotpluggable {
            flags |= Self::HOTPLUGGABLE.0;
        }
        if range.non_volatile {
            flags |= Self::NON_VOLATILE.0;
        }
        MemoryAffinityFlags(flags)
    }
}

/// Processor Local APIC Affinity Structure
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ProcessorLocalApicAffinity {
    /// Structure type (0)
    pub structure_type: u8,
    /// Structure length (16)
    pub length: u8,
    /// Proximity domain bits [7:0]
    pub proximity_domain_lo: u8,
    /// Local APIC ID
    pub apic_id: u8,
    /// Flags
    pub flags: u32,
    /// Local SAPIC EID
    pub sapic_eid: u8,
    /// Proximity domain bits [31:8]
    pub proximity_domain_hi: [u8; 3],
    /// Clock domain
    pub clock_domain: u32,
}

impl ProcessorLocalApicAffinity {
    /// Structure size in bytes
    pub const SIZE: usize = 16;

    /// Create a new processor affinity structure
    pub fn new(affinity: &CpuAffinity) -> Self {
        let domain = affinity.proximity_domain;
        let flags = if affinity.enabled {
            ProcessorAffinityFlags::ENABLED.0
        } else {
            0
        };

        Self {
            structure_type: SratSubtableType::ProcessorLocalApic as u8,
            length: Self::SIZE as u8,
            proximity_domain_lo: (domain & 0xFF) as u8,
            apic_id: affinity.apic_id as u8,
            flags,
            sapic_eid: 0,
            proximity_domain_hi: [
                ((domain >> 8) & 0xFF) as u8,
                ((domain >> 16) & 0xFF) as u8,
                ((domain >> 24) & 0xFF) as u8,
            ],
            clock_domain: 0,
        }
    }

    /// Encode to bytes
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0] = self.structure_type;
        bytes[1] = self.length;
        bytes[2] = self.proximity_domain_lo;
        bytes[3] = self.apic_id;
        bytes[4..8].copy_from_slice(&self.flags.to_le_bytes());
        bytes[8] = self.sapic_eid;
        bytes[9..12].copy_from_slice(&self.proximity_domain_hi);
        bytes[12..16].copy_from_slice(&self.clock_domain.to_le_bytes());
        bytes
    }
}

/// Processor Local x2APIC Affinity Structure
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ProcessorX2ApicAffinity {
    /// Structure type (2)
    pub structure_type: u8,
    /// Structure length (24)
    pub length: u8,
    /// Reserved
    pub reserved1: [u8; 2],
    /// Proximity domain
    pub proximity_domain: u32,
    /// x2APIC ID
    pub x2apic_id: u32,
    /// Flags
    pub flags: u32,
    /// Clock domain
    pub clock_domain: u32,
    /// Reserved
    pub reserved2: [u8; 4],
}

impl ProcessorX2ApicAffinity {
    /// Structure size in bytes
    pub const SIZE: usize = 24;

    /// Create a new x2APIC affinity structure
    pub fn new(affinity: &CpuAffinity) -> Self {
        let flags = if affinity.enabled {
            ProcessorAffinityFlags::ENABLED.0
        } else {
            0
        };

        Self {
            structure_type: SratSubtableType::ProcessorX2Apic as u8,
            length: Self::SIZE as u8,
            reserved1: [0; 2],
            proximity_domain: affinity.proximity_domain,
            x2apic_id: affinity.apic_id,
            flags,
            clock_domain: 0,
            reserved2: [0; 4],
        }
    }

    /// Encode to bytes
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0] = self.structure_type;
        bytes[1] = self.length;
        bytes[2..4].copy_from_slice(&self.reserved1);
        bytes[4..8].copy_from_slice(&self.proximity_domain.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.x2apic_id.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.flags.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.clock_domain.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.reserved2);
        bytes
    }
}

/// Memory Affinity Structure
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MemoryAffinityStructure {
    /// Structure type (1)
    pub structure_type: u8,
    /// Structure length (40)
    pub length: u8,
    /// Proximity domain
    pub proximity_domain: u32,
    /// Reserved
    pub reserved1: [u8; 2],
    /// Base address low
    pub base_addr_lo: u32,
    /// Base address high
    pub base_addr_hi: u32,
    /// Length low
    pub length_lo: u32,
    /// Length high
    pub length_hi: u32,
    /// Reserved
    pub reserved2: [u8; 4],
    /// Flags
    pub flags: u32,
    /// Reserved
    pub reserved3: [u8; 8],
}

impl MemoryAffinityStructure {
    /// Structure size in bytes
    pub const SIZE: usize = 40;

    /// Create a new memory affinity structure
    pub fn new(affinity: &MemoryAffinity) -> Self {
        let flags = MemoryAffinityFlags::from_range(&affinity.range, affinity.enabled);

        Self {
            structure_type: SratSubtableType::Memory as u8,
            length: Self::SIZE as u8,
            proximity_domain: affinity.proximity_domain,
            reserved1: [0; 2],
            base_addr_lo: (affinity.range.base & 0xFFFF_FFFF) as u32,
            base_addr_hi: (affinity.range.base >> 32) as u32,
            length_lo: (affinity.range.length & 0xFFFF_FFFF) as u32,
            length_hi: (affinity.range.length >> 32) as u32,
            reserved2: [0; 4],
            flags: flags.0,
            reserved3: [0; 8],
        }
    }

    /// Encode to bytes
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0] = self.structure_type;
        bytes[1] = self.length;
        bytes[2..6].copy_from_slice(&self.proximity_domain.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.reserved1);
        bytes[8..12].copy_from_slice(&self.base_addr_lo.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.base_addr_hi.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.length_lo.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.length_hi.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.reserved2);
        bytes[28..32].copy_from_slice(&self.flags.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.reserved3);
        bytes
    }
}

/// ACPI table header
#[derive(Debug, Clone)]
pub struct AcpiHeader {
    /// Signature
    pub signature: [u8; 4],
    /// Length of the entire table
    pub length: u32,
    /// Revision
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

impl AcpiHeader {
    /// Header size in bytes
    pub const SIZE: usize = 36;

    /// Create a new ACPI header
    pub fn new(signature: [u8; 4], length: u32, revision: u8) -> Self {
        Self {
            signature,
            length,
            revision,
            checksum: 0,
            oem_id: *b"AETHER",
            oem_table_id: *b"AETHERVM",
            oem_revision: 1,
            creator_id: *b"AETH",
            creator_revision: 1,
        }
    }

    /// Encode to bytes
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&self.signature);
        bytes[4..8].copy_from_slice(&self.length.to_le_bytes());
        bytes[8] = self.revision;
        bytes[9] = self.checksum;
        bytes[10..16].copy_from_slice(&self.oem_id);
        bytes[16..24].copy_from_slice(&self.oem_table_id);
        bytes[24..28].copy_from_slice(&self.oem_revision.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.creator_id);
        bytes[32..36].copy_from_slice(&self.creator_revision.to_le_bytes());
        bytes
    }
}

/// SRAT (System Resource Affinity Table) builder
#[derive(Debug)]
pub struct SratBuilder {
    /// CPU affinity entries
    cpu_affinities: Vec<CpuAffinity>,
    /// Memory affinity entries
    memory_affinities: Vec<MemoryAffinity>,
    /// Use x2APIC format for APIC IDs > 255
    use_x2apic: bool,
}

impl SratBuilder {
    /// Create a new SRAT builder
    pub fn new() -> Self {
        Self {
            cpu_affinities: Vec::new(),
            memory_affinities: Vec::new(),
            use_x2apic: false,
        }
    }

    /// Create from a NUMA topology
    pub fn from_topology(topology: &NumaTopology) -> Self {
        let mut builder = Self::new();
        builder.cpu_affinities = topology.cpu_affinities();
        builder.memory_affinities = topology.memory_affinities();

        // Check if we need x2APIC
        builder.use_x2apic = builder.cpu_affinities.iter().any(|a| a.apic_id > 255);

        builder
    }

    /// Add a CPU affinity entry
    pub fn add_cpu(&mut self, affinity: CpuAffinity) {
        if affinity.apic_id > 255 {
            self.use_x2apic = true;
        }
        self.cpu_affinities.push(affinity);
    }

    /// Add a memory affinity entry
    pub fn add_memory(&mut self, affinity: MemoryAffinity) {
        self.memory_affinities.push(affinity);
    }

    /// Calculate the total table length
    pub fn table_length(&self) -> usize {
        let header_len = AcpiHeader::SIZE + 12; // Header + table revision + reserved

        let cpu_len = if self.use_x2apic {
            self.cpu_affinities.len() * ProcessorX2ApicAffinity::SIZE
        } else {
            self.cpu_affinities.len() * ProcessorLocalApicAffinity::SIZE
        };

        let mem_len = self.memory_affinities.len() * MemoryAffinityStructure::SIZE;

        header_len + cpu_len + mem_len
    }

    /// Build the SRAT table
    pub fn build(&self) -> Vec<u8> {
        let length = self.table_length() as u32;
        let header = AcpiHeader::new(SRAT_SIGNATURE, length, SRAT_REVISION);

        let mut data = Vec::with_capacity(length as usize);

        // Header
        data.extend_from_slice(&header.encode());

        // Table revision (4 bytes)
        data.extend_from_slice(&1u32.to_le_bytes());

        // Reserved (8 bytes)
        data.extend_from_slice(&[0u8; 8]);

        // CPU affinity entries
        for affinity in &self.cpu_affinities {
            if self.use_x2apic {
                data.extend_from_slice(&ProcessorX2ApicAffinity::new(affinity).encode());
            } else {
                data.extend_from_slice(&ProcessorLocalApicAffinity::new(affinity).encode());
            }
        }

        // Memory affinity entries
        for affinity in &self.memory_affinities {
            data.extend_from_slice(&MemoryAffinityStructure::new(affinity).encode());
        }

        // Calculate and fix checksum
        let checksum = Self::calculate_checksum(&data);
        data[9] = checksum;

        data
    }

    /// Calculate ACPI checksum
    fn calculate_checksum(data: &[u8]) -> u8 {
        let sum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        (!sum).wrapping_add(1)
    }
}

impl Default for SratBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// SLIT (System Locality Information Table) builder
#[derive(Debug)]
pub struct SlitBuilder {
    /// Distance matrix
    distances: DistanceMatrix,
}

impl SlitBuilder {
    /// Create a new SLIT builder
    pub fn new(node_count: usize) -> Self {
        Self {
            distances: DistanceMatrix::new(node_count),
        }
    }

    /// Create from a NUMA topology
    pub fn from_topology(topology: &NumaTopology) -> Self {
        Self {
            distances: topology.distance_matrix().clone(),
        }
    }

    /// Create from a distance matrix
    pub fn from_matrix(matrix: DistanceMatrix) -> Self {
        Self { distances: matrix }
    }

    /// Set the distance between two nodes
    pub fn set_distance(&mut self, from: NodeId, to: NodeId, distance: u8) {
        self.distances.set(from, to, distance);
    }

    /// Set symmetric distance between two nodes
    pub fn set_symmetric_distance(&mut self, node1: NodeId, node2: NodeId, distance: u8) {
        self.distances.set_symmetric(node1, node2, distance);
    }

    /// Calculate the total table length
    pub fn table_length(&self) -> usize {
        let node_count = self.distances.node_count();
        AcpiHeader::SIZE + 8 + (node_count * node_count)
    }

    /// Build the SLIT table
    pub fn build(&self) -> Vec<u8> {
        let length = self.table_length() as u32;
        let node_count = self.distances.node_count() as u64;
        let header = AcpiHeader::new(SLIT_SIGNATURE, length, SLIT_REVISION);

        let mut data = Vec::with_capacity(length as usize);

        // Header
        data.extend_from_slice(&header.encode());

        // Number of system localities (8 bytes)
        data.extend_from_slice(&node_count.to_le_bytes());

        // Distance matrix
        data.extend_from_slice(self.distances.as_slice());

        // Calculate and fix checksum
        let checksum = Self::calculate_checksum(&data);
        data[9] = checksum;

        data
    }

    /// Calculate ACPI checksum
    fn calculate_checksum(data: &[u8]) -> u8 {
        let sum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        (!sum).wrapping_add(1)
    }
}

/// Combined NUMA ACPI table generator
pub struct NumaAcpiTables {
    /// SRAT data
    pub srat: Vec<u8>,
    /// SLIT data
    pub slit: Vec<u8>,
}

impl NumaAcpiTables {
    /// Generate SRAT and SLIT from a NUMA topology
    pub fn from_topology(topology: &NumaTopology) -> Self {
        let srat = SratBuilder::from_topology(topology).build();
        let slit = SlitBuilder::from_topology(topology).build();

        Self { srat, slit }
    }

    /// Get total size of all tables
    pub fn total_size(&self) -> usize {
        self.srat.len() + self.slit.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processor_affinity_flags() {
        let enabled = ProcessorAffinityFlags::ENABLED;
        assert!(enabled.is_enabled());

        let empty = ProcessorAffinityFlags::empty();
        assert!(!empty.is_enabled());
    }

    #[test]
    fn test_memory_affinity_flags() {
        let range = MemoryRange::new(0, 0x1000_0000);
        let flags = MemoryAffinityFlags::from_range(&range, true);
        assert_eq!(flags.0, MemoryAffinityFlags::ENABLED.0);

        let hotplug = MemoryRange::hotpluggable(0, 0x1000_0000);
        let flags = MemoryAffinityFlags::from_range(&hotplug, true);
        assert_eq!(
            flags.0,
            MemoryAffinityFlags::ENABLED.0 | MemoryAffinityFlags::HOTPLUGGABLE.0
        );

        let nvdimm = MemoryRange::persistent(0, 0x1000_0000);
        let flags = MemoryAffinityFlags::from_range(&nvdimm, true);
        assert_eq!(
            flags.0,
            MemoryAffinityFlags::ENABLED.0 | MemoryAffinityFlags::NON_VOLATILE.0
        );
    }

    #[test]
    fn test_processor_local_apic_affinity() {
        let affinity = CpuAffinity::new(5, NodeId::new(1));
        let structure = ProcessorLocalApicAffinity::new(&affinity);

        assert_eq!(
            structure.structure_type,
            SratSubtableType::ProcessorLocalApic as u8
        );
        assert_eq!(structure.length, 16);
        assert_eq!(structure.apic_id, 5);
        assert_eq!(structure.proximity_domain_lo, 1);

        // Copy packed field to avoid unaligned reference
        let flags = structure.flags;
        assert_eq!(flags, ProcessorAffinityFlags::ENABLED.0);

        let bytes = structure.encode();
        assert_eq!(bytes.len(), ProcessorLocalApicAffinity::SIZE);
        assert_eq!(bytes[0], 0); // Type
        assert_eq!(bytes[1], 16); // Length
    }

    #[test]
    fn test_processor_x2apic_affinity() {
        let affinity = CpuAffinity::new(300, NodeId::new(2));
        let structure = ProcessorX2ApicAffinity::new(&affinity);

        assert_eq!(
            structure.structure_type,
            SratSubtableType::ProcessorX2Apic as u8
        );
        assert_eq!(structure.length, 24);

        // Copy packed fields to avoid unaligned reference
        let x2apic_id = structure.x2apic_id;
        let proximity_domain = structure.proximity_domain;
        assert_eq!(x2apic_id, 300);
        assert_eq!(proximity_domain, 2);

        let bytes = structure.encode();
        assert_eq!(bytes.len(), ProcessorX2ApicAffinity::SIZE);
    }

    #[test]
    fn test_memory_affinity_structure() {
        let range = MemoryRange::new(0x1_0000_0000, 0x4000_0000);
        let affinity = MemoryAffinity::new(range, NodeId::new(1));
        let structure = MemoryAffinityStructure::new(&affinity);

        assert_eq!(structure.structure_type, SratSubtableType::Memory as u8);
        assert_eq!(structure.length, 40);

        // Copy packed fields to avoid unaligned reference
        let proximity_domain = structure.proximity_domain;
        let base_addr_lo = structure.base_addr_lo;
        let base_addr_hi = structure.base_addr_hi;
        let length_lo = structure.length_lo;
        let length_hi = structure.length_hi;

        assert_eq!(proximity_domain, 1);
        assert_eq!(base_addr_lo, 0);
        assert_eq!(base_addr_hi, 1);
        assert_eq!(length_lo, 0x4000_0000);
        assert_eq!(length_hi, 0);

        let bytes = structure.encode();
        assert_eq!(bytes.len(), MemoryAffinityStructure::SIZE);
    }

    #[test]
    fn test_acpi_header() {
        let header = AcpiHeader::new(SRAT_SIGNATURE, 100, SRAT_REVISION);

        assert_eq!(header.signature, *b"SRAT");
        assert_eq!(header.length, 100);
        assert_eq!(header.revision, SRAT_REVISION);
        assert_eq!(header.oem_id, *b"AETHER");
        assert_eq!(header.oem_table_id, *b"AETHERVM");

        let bytes = header.encode();
        assert_eq!(bytes.len(), AcpiHeader::SIZE);
        assert_eq!(&bytes[0..4], b"SRAT");
    }

    #[test]
    fn test_srat_builder_basic() {
        let mut builder = SratBuilder::new();

        builder.add_cpu(CpuAffinity::new(0, NodeId::new(0)));
        builder.add_cpu(CpuAffinity::new(1, NodeId::new(0)));
        builder.add_memory(MemoryAffinity::new(
            MemoryRange::new(0, 0x1_0000_0000),
            NodeId::new(0),
        ));

        let data = builder.build();

        // Check signature
        assert_eq!(&data[0..4], b"SRAT");

        // Check length
        let length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        assert_eq!(length as usize, data.len());

        // Verify checksum
        let checksum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(checksum, 0);
    }

    #[test]
    fn test_srat_builder_from_topology() {
        let topology = NumaTopology::two_node(4, 0x1_0000_0000);
        let builder = SratBuilder::from_topology(&topology);

        assert_eq!(builder.cpu_affinities.len(), 8);
        assert_eq!(builder.memory_affinities.len(), 2);

        let data = builder.build();
        assert!(!data.is_empty());

        // Verify checksum
        let checksum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(checksum, 0);
    }

    #[test]
    fn test_srat_builder_x2apic() {
        let mut builder = SratBuilder::new();

        // Add CPU with APIC ID > 255 to trigger x2APIC mode
        builder.add_cpu(CpuAffinity::new(300, NodeId::new(0)));
        assert!(builder.use_x2apic);

        let data = builder.build();

        // Check that the CPU entry is x2APIC format (type 2, length 24)
        let cpu_entry_start = AcpiHeader::SIZE + 12;
        assert_eq!(data[cpu_entry_start], 2); // Type
        assert_eq!(data[cpu_entry_start + 1], 24); // Length
    }

    #[test]
    fn test_slit_builder_basic() {
        let mut builder = SlitBuilder::new(2);
        builder.set_symmetric_distance(NodeId::new(0), NodeId::new(1), 20);

        let data = builder.build();

        // Check signature
        assert_eq!(&data[0..4], b"SLIT");

        // Check node count
        let node_count = u64::from_le_bytes([
            data[36], data[37], data[38], data[39], data[40], data[41], data[42], data[43],
        ]);
        assert_eq!(node_count, 2);

        // Check distances
        let distances_start = 44;
        assert_eq!(data[distances_start], 10); // Node 0 to Node 0
        assert_eq!(data[distances_start + 1], 20); // Node 0 to Node 1
        assert_eq!(data[distances_start + 2], 20); // Node 1 to Node 0
        assert_eq!(data[distances_start + 3], 10); // Node 1 to Node 1

        // Verify checksum
        let checksum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(checksum, 0);
    }

    #[test]
    fn test_slit_builder_from_topology() {
        let topology = NumaTopology::four_node(2, 0x1_0000_0000);
        let builder = SlitBuilder::from_topology(&topology);

        let data = builder.build();

        // Should have 4x4 = 16 distance entries
        let expected_len = AcpiHeader::SIZE + 8 + 16;
        assert_eq!(data.len(), expected_len);
    }

    #[test]
    fn test_numa_acpi_tables() {
        let topology = NumaTopology::two_node(4, 0x1_0000_0000);
        let tables = NumaAcpiTables::from_topology(&topology);

        // Check SRAT
        assert_eq!(&tables.srat[0..4], b"SRAT");
        let srat_checksum: u8 = tables.srat.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(srat_checksum, 0);

        // Check SLIT
        assert_eq!(&tables.slit[0..4], b"SLIT");
        let slit_checksum: u8 = tables.slit.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(slit_checksum, 0);

        // Total size
        assert_eq!(tables.total_size(), tables.srat.len() + tables.slit.len());
    }

    #[test]
    fn test_srat_disabled_entries() {
        let mut builder = SratBuilder::new();

        builder.add_cpu(CpuAffinity::disabled(4, NodeId::new(1)));
        builder.add_memory(MemoryAffinity::disabled(
            MemoryRange::hotpluggable(0x1_0000_0000, 0x4000_0000),
            NodeId::new(1),
        ));

        let data = builder.build();

        // Verify it builds without error
        let checksum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(checksum, 0);
    }
}
