//! NUMA core types and definitions
//!
//! This module provides fundamental types for Non-Uniform Memory Access (NUMA)
//! topology representation including node identifiers, memory ranges, and
//! distance matrices.

use std::fmt;

/// NUMA node identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Maximum number of NUMA nodes supported
    pub const MAX_NODES: u32 = 256;

    /// Invalid node ID constant
    pub const INVALID: NodeId = NodeId(u32::MAX);

    /// Create a new node ID
    pub const fn new(id: u32) -> Self {
        NodeId(id)
    }

    /// Check if this is a valid node ID
    pub fn is_valid(&self) -> bool {
        self.0 < Self::MAX_NODES
    }

    /// Get the raw node ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::INVALID {
            write!(f, "INVALID")
        } else {
            write!(f, "Node{}", self.0)
        }
    }
}

impl From<u32> for NodeId {
    fn from(id: u32) -> Self {
        NodeId(id)
    }
}

impl From<NodeId> for u32 {
    fn from(id: NodeId) -> Self {
        id.0
    }
}

/// Memory range within a NUMA node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRange {
    /// Starting physical address
    pub base: u64,
    /// Length in bytes
    pub length: u64,
    /// Whether this range is hotpluggable
    pub hotpluggable: bool,
    /// Whether this range is non-volatile (persistent memory)
    pub non_volatile: bool,
}

impl MemoryRange {
    /// Create a new memory range
    pub const fn new(base: u64, length: u64) -> Self {
        Self {
            base,
            length,
            hotpluggable: false,
            non_volatile: false,
        }
    }

    /// Create a hotpluggable memory range
    pub const fn hotpluggable(base: u64, length: u64) -> Self {
        Self {
            base,
            length,
            hotpluggable: true,
            non_volatile: false,
        }
    }

    /// Create a non-volatile (persistent) memory range
    pub const fn persistent(base: u64, length: u64) -> Self {
        Self {
            base,
            length,
            hotpluggable: false,
            non_volatile: true,
        }
    }

    /// Get the end address (exclusive)
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.length)
    }

    /// Check if an address falls within this range
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }

    /// Check if this range overlaps with another
    pub fn overlaps(&self, other: &MemoryRange) -> bool {
        self.base < other.end() && other.base < self.end()
    }

    /// Get the size in pages (4KB)
    pub fn pages(&self) -> u64 {
        (self.length + 0xFFF) >> 12
    }

    /// Get the size in megabytes
    pub fn size_mb(&self) -> u64 {
        self.length >> 20
    }

    /// Get the size in gigabytes
    pub fn size_gb(&self) -> u64 {
        self.length >> 30
    }
}

impl fmt::Display for MemoryRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flags = match (self.hotpluggable, self.non_volatile) {
            (true, true) => " [hotplug,nvdimm]",
            (true, false) => " [hotplug]",
            (false, true) => " [nvdimm]",
            (false, false) => "",
        };
        write!(
            f,
            "0x{:016x}-0x{:016x} ({}MB){}",
            self.base,
            self.end(),
            self.size_mb(),
            flags
        )
    }
}

/// NUMA distance value between nodes
///
/// Distance values follow the ACPI SLIT convention:
/// - 10 = local access (same node)
/// - 20 = typical remote access
/// - 255 = unreachable
pub type NumaDistance = u8;

/// Local node distance (same node)
pub const NUMA_DISTANCE_LOCAL: NumaDistance = 10;

/// Default remote node distance
pub const NUMA_DISTANCE_REMOTE: NumaDistance = 20;

/// Far remote node distance (e.g., across sockets)
pub const NUMA_DISTANCE_FAR: NumaDistance = 30;

/// Very far distance (e.g., across boards)
pub const NUMA_DISTANCE_VERY_FAR: NumaDistance = 40;

/// Unreachable distance
pub const NUMA_DISTANCE_UNREACHABLE: NumaDistance = 255;

/// NUMA distance matrix for inter-node latency
#[derive(Debug, Clone)]
pub struct DistanceMatrix {
    /// Number of nodes
    node_count: usize,
    /// Distance values in row-major order
    distances: Vec<NumaDistance>,
}

impl DistanceMatrix {
    /// Create a new distance matrix with default values
    pub fn new(node_count: usize) -> Self {
        let mut distances = vec![NUMA_DISTANCE_REMOTE; node_count * node_count];

        // Set diagonal to local distance
        for i in 0..node_count {
            distances[i * node_count + i] = NUMA_DISTANCE_LOCAL;
        }

        Self {
            node_count,
            distances,
        }
    }

    /// Create a symmetric distance matrix from node pairs
    pub fn symmetric(node_count: usize) -> Self {
        Self::new(node_count)
    }

    /// Get the distance between two nodes
    pub fn get(&self, from: NodeId, to: NodeId) -> NumaDistance {
        let from_idx = from.as_u32() as usize;
        let to_idx = to.as_u32() as usize;

        if from_idx >= self.node_count || to_idx >= self.node_count {
            return NUMA_DISTANCE_UNREACHABLE;
        }

        self.distances[from_idx * self.node_count + to_idx]
    }

    /// Set the distance between two nodes
    pub fn set(&mut self, from: NodeId, to: NodeId, distance: NumaDistance) {
        let from_idx = from.as_u32() as usize;
        let to_idx = to.as_u32() as usize;

        if from_idx < self.node_count && to_idx < self.node_count {
            self.distances[from_idx * self.node_count + to_idx] = distance;
        }
    }

    /// Set symmetric distance between two nodes
    pub fn set_symmetric(&mut self, node1: NodeId, node2: NodeId, distance: NumaDistance) {
        self.set(node1, node2, distance);
        self.set(node2, node1, distance);
    }

    /// Get the number of nodes
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Get the raw distance values for SLIT generation
    pub fn as_slice(&self) -> &[NumaDistance] {
        &self.distances
    }

    /// Check if the matrix is symmetric
    pub fn is_symmetric(&self) -> bool {
        for i in 0..self.node_count {
            for j in (i + 1)..self.node_count {
                let idx1 = i * self.node_count + j;
                let idx2 = j * self.node_count + i;
                if self.distances[idx1] != self.distances[idx2] {
                    return false;
                }
            }
        }
        true
    }

    /// Find the nearest node to the given node (excluding itself)
    pub fn nearest_node(&self, node: NodeId) -> Option<NodeId> {
        let node_idx = node.as_u32() as usize;
        if node_idx >= self.node_count {
            return None;
        }

        let mut nearest = None;
        let mut min_distance = NUMA_DISTANCE_UNREACHABLE;

        for i in 0..self.node_count {
            if i == node_idx {
                continue;
            }

            let distance = self.distances[node_idx * self.node_count + i];
            if distance < min_distance {
                min_distance = distance;
                nearest = Some(NodeId::new(i as u32));
            }
        }

        nearest
    }
}

/// CPU affinity information for NUMA
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuAffinity {
    /// APIC ID of the CPU
    pub apic_id: u32,
    /// NUMA node this CPU belongs to
    pub node: NodeId,
    /// Proximity domain (for ACPI SRAT)
    pub proximity_domain: u32,
    /// Whether this CPU is enabled
    pub enabled: bool,
}

impl CpuAffinity {
    /// Create a new CPU affinity entry
    pub fn new(apic_id: u32, node: NodeId) -> Self {
        Self {
            apic_id,
            node,
            proximity_domain: node.as_u32(),
            enabled: true,
        }
    }

    /// Create a disabled CPU affinity entry (for hotplug slots)
    pub fn disabled(apic_id: u32, node: NodeId) -> Self {
        Self {
            apic_id,
            node,
            proximity_domain: node.as_u32(),
            enabled: false,
        }
    }

    /// Set the proximity domain explicitly
    pub fn with_proximity_domain(mut self, domain: u32) -> Self {
        self.proximity_domain = domain;
        self
    }
}

/// Memory affinity information for NUMA
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAffinity {
    /// Memory range
    pub range: MemoryRange,
    /// NUMA node this memory belongs to
    pub node: NodeId,
    /// Proximity domain (for ACPI SRAT)
    pub proximity_domain: u32,
    /// Whether this memory is enabled
    pub enabled: bool,
}

impl MemoryAffinity {
    /// Create a new memory affinity entry
    pub fn new(range: MemoryRange, node: NodeId) -> Self {
        Self {
            range,
            node,
            proximity_domain: node.as_u32(),
            enabled: true,
        }
    }

    /// Create a disabled memory affinity entry (for hotplug regions)
    pub fn disabled(range: MemoryRange, node: NodeId) -> Self {
        Self {
            range,
            node,
            proximity_domain: node.as_u32(),
            enabled: false,
        }
    }

    /// Set the proximity domain explicitly
    pub fn with_proximity_domain(mut self, domain: u32) -> Self {
        self.proximity_domain = domain;
        self
    }
}

/// NUMA node statistics
#[derive(Debug, Clone, Default)]
pub struct NodeStats {
    /// Total memory in bytes
    pub total_memory: u64,
    /// Free memory in bytes
    pub free_memory: u64,
    /// Number of CPUs on this node
    pub cpu_count: u32,
    /// Memory allocations from this node
    pub allocations: u64,
    /// Memory allocations from remote nodes
    pub remote_allocations: u64,
    /// Local memory hits
    pub local_hits: u64,
    /// Remote memory accesses
    pub remote_accesses: u64,
}

impl NodeStats {
    /// Create new node statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the local hit rate as a percentage
    pub fn local_hit_rate(&self) -> f64 {
        let total = self.local_hits + self.remote_accesses;
        if total == 0 {
            100.0
        } else {
            (self.local_hits as f64 / total as f64) * 100.0
        }
    }

    /// Get the memory utilization as a percentage
    pub fn memory_utilization(&self) -> f64 {
        if self.total_memory == 0 {
            0.0
        } else {
            let used = self.total_memory.saturating_sub(self.free_memory);
            (used as f64 / self.total_memory as f64) * 100.0
        }
    }

    /// Record a local memory access
    pub fn record_local_access(&mut self) {
        self.local_hits += 1;
    }

    /// Record a remote memory access
    pub fn record_remote_access(&mut self) {
        self.remote_accesses += 1;
    }

    /// Record a memory allocation
    pub fn record_allocation(&mut self, size: u64, is_remote: bool) {
        self.allocations += 1;
        if is_remote {
            self.remote_allocations += 1;
        }
        self.free_memory = self.free_memory.saturating_sub(size);
    }

    /// Record a memory free
    pub fn record_free(&mut self, size: u64) {
        self.free_memory = self.free_memory.saturating_add(size);
        if self.free_memory > self.total_memory {
            self.free_memory = self.total_memory;
        }
    }
}

/// Memory interleaving mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterleavingMode {
    /// No interleaving, allocate from specific node
    None,
    /// Round-robin across all nodes
    RoundRobin,
    /// Weighted round-robin based on memory capacity
    Weighted,
    /// Interleave across specified nodes only
    NodeSet(u64), // Bitmask of nodes
}

impl InterleavingMode {
    /// Create a node set interleaving mode
    pub fn node_set(nodes: &[NodeId]) -> Self {
        let mut mask = 0u64;
        for node in nodes {
            if node.as_u32() < 64 {
                mask |= 1 << node.as_u32();
            }
        }
        InterleavingMode::NodeSet(mask)
    }

    /// Check if a node is included in this interleaving mode
    pub fn includes_node(&self, node: NodeId) -> bool {
        match self {
            InterleavingMode::None => false,
            InterleavingMode::RoundRobin => true,
            InterleavingMode::Weighted => true,
            InterleavingMode::NodeSet(mask) => {
                if node.as_u32() < 64 {
                    (mask >> node.as_u32()) & 1 != 0
                } else {
                    false
                }
            }
        }
    }
}

/// Memory allocation policy for NUMA-aware allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationPolicy {
    /// Allocate from the local node only
    Local,
    /// Allocate from the preferred node, fall back to others
    Preferred(NodeId),
    /// Allocate from the nearest node with available memory
    NearestAvailable,
    /// Interleave allocations across nodes
    Interleaved(InterleavingMode),
    /// Bind to a specific node (fail if unavailable)
    Bind(NodeId),
}

impl AllocationPolicy {
    /// Create a local-only allocation policy
    pub fn local() -> Self {
        AllocationPolicy::Local
    }

    /// Create a preferred node allocation policy
    pub fn preferred(node: NodeId) -> Self {
        AllocationPolicy::Preferred(node)
    }

    /// Create an interleaved allocation policy
    pub fn interleaved() -> Self {
        AllocationPolicy::Interleaved(InterleavingMode::RoundRobin)
    }

    /// Create a bound allocation policy
    pub fn bind(node: NodeId) -> Self {
        AllocationPolicy::Bind(node)
    }
}

impl Default for AllocationPolicy {
    fn default() -> Self {
        AllocationPolicy::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_creation() {
        let node = NodeId::new(5);
        assert_eq!(node.as_u32(), 5);
        assert!(node.is_valid());
        assert_eq!(format!("{}", node), "Node5");
    }

    #[test]
    fn test_node_id_invalid() {
        let invalid = NodeId::INVALID;
        assert!(!invalid.is_valid());
        assert_eq!(format!("{}", invalid), "INVALID");
    }

    #[test]
    fn test_node_id_conversion() {
        let node: NodeId = 10u32.into();
        assert_eq!(node.as_u32(), 10);

        let raw: u32 = node.into();
        assert_eq!(raw, 10);
    }

    #[test]
    fn test_memory_range_creation() {
        let range = MemoryRange::new(0x1000_0000, 0x4000_0000);
        assert_eq!(range.base, 0x1000_0000);
        assert_eq!(range.length, 0x4000_0000);
        assert!(!range.hotpluggable);
        assert!(!range.non_volatile);
    }

    #[test]
    fn test_memory_range_hotpluggable() {
        let range = MemoryRange::hotpluggable(0x2000_0000, 0x1000_0000);
        assert!(range.hotpluggable);
        assert!(!range.non_volatile);
    }

    #[test]
    fn test_memory_range_persistent() {
        let range = MemoryRange::persistent(0x3000_0000, 0x2000_0000);
        assert!(!range.hotpluggable);
        assert!(range.non_volatile);
    }

    #[test]
    fn test_memory_range_end() {
        let range = MemoryRange::new(0x1000, 0x2000);
        assert_eq!(range.end(), 0x3000);
    }

    #[test]
    fn test_memory_range_contains() {
        let range = MemoryRange::new(0x1000, 0x2000);
        assert!(range.contains(0x1000));
        assert!(range.contains(0x2000));
        assert!(range.contains(0x2FFF));
        assert!(!range.contains(0x0FFF));
        assert!(!range.contains(0x3000));
    }

    #[test]
    fn test_memory_range_overlaps() {
        let range1 = MemoryRange::new(0x1000, 0x2000);
        let range2 = MemoryRange::new(0x2000, 0x2000);
        let range3 = MemoryRange::new(0x3000, 0x1000);
        let range4 = MemoryRange::new(0x4000, 0x1000);

        assert!(range1.overlaps(&range2));
        assert!(range2.overlaps(&range3));
        assert!(!range1.overlaps(&range4));
        assert!(!range3.overlaps(&range4));
    }

    #[test]
    fn test_memory_range_size() {
        let range = MemoryRange::new(0, 0x4000_0000); // 1GB
        assert_eq!(range.pages(), 0x40000); // 1GB / 4KB = 262144 pages
        assert_eq!(range.size_mb(), 1024);
        assert_eq!(range.size_gb(), 1);
    }

    #[test]
    fn test_distance_matrix_creation() {
        let matrix = DistanceMatrix::new(4);
        assert_eq!(matrix.node_count(), 4);

        // Local distances should be 10
        assert_eq!(
            matrix.get(NodeId::new(0), NodeId::new(0)),
            NUMA_DISTANCE_LOCAL
        );
        assert_eq!(
            matrix.get(NodeId::new(3), NodeId::new(3)),
            NUMA_DISTANCE_LOCAL
        );

        // Remote distances should be 20 by default
        assert_eq!(
            matrix.get(NodeId::new(0), NodeId::new(1)),
            NUMA_DISTANCE_REMOTE
        );
    }

    #[test]
    fn test_distance_matrix_set() {
        let mut matrix = DistanceMatrix::new(3);
        matrix.set(NodeId::new(0), NodeId::new(2), NUMA_DISTANCE_FAR);
        assert_eq!(
            matrix.get(NodeId::new(0), NodeId::new(2)),
            NUMA_DISTANCE_FAR
        );
        assert_eq!(
            matrix.get(NodeId::new(2), NodeId::new(0)),
            NUMA_DISTANCE_REMOTE
        ); // Not symmetric
    }

    #[test]
    fn test_distance_matrix_symmetric() {
        let mut matrix = DistanceMatrix::symmetric(3);
        matrix.set_symmetric(NodeId::new(0), NodeId::new(2), NUMA_DISTANCE_FAR);
        assert_eq!(
            matrix.get(NodeId::new(0), NodeId::new(2)),
            NUMA_DISTANCE_FAR
        );
        assert_eq!(
            matrix.get(NodeId::new(2), NodeId::new(0)),
            NUMA_DISTANCE_FAR
        );
        assert!(matrix.is_symmetric());
    }

    #[test]
    fn test_distance_matrix_nearest_node() {
        let mut matrix = DistanceMatrix::new(4);
        matrix.set(NodeId::new(0), NodeId::new(1), 15);
        matrix.set(NodeId::new(0), NodeId::new(2), 25);
        matrix.set(NodeId::new(0), NodeId::new(3), 35);

        let nearest = matrix.nearest_node(NodeId::new(0)).unwrap();
        assert_eq!(nearest, NodeId::new(1));
    }

    #[test]
    fn test_distance_matrix_invalid_node() {
        let matrix = DistanceMatrix::new(2);
        assert_eq!(
            matrix.get(NodeId::new(5), NodeId::new(0)),
            NUMA_DISTANCE_UNREACHABLE
        );
    }

    #[test]
    fn test_cpu_affinity() {
        let cpu = CpuAffinity::new(0, NodeId::new(0));
        assert_eq!(cpu.apic_id, 0);
        assert_eq!(cpu.node, NodeId::new(0));
        assert_eq!(cpu.proximity_domain, 0);
        assert!(cpu.enabled);
    }

    #[test]
    fn test_cpu_affinity_disabled() {
        let cpu = CpuAffinity::disabled(4, NodeId::new(1));
        assert_eq!(cpu.apic_id, 4);
        assert!(!cpu.enabled);
    }

    #[test]
    fn test_cpu_affinity_custom_proximity() {
        let cpu = CpuAffinity::new(0, NodeId::new(0)).with_proximity_domain(100);
        assert_eq!(cpu.proximity_domain, 100);
    }

    #[test]
    fn test_memory_affinity() {
        let range = MemoryRange::new(0x1000_0000, 0x4000_0000);
        let mem = MemoryAffinity::new(range, NodeId::new(1));
        assert_eq!(mem.range.base, 0x1000_0000);
        assert_eq!(mem.node, NodeId::new(1));
        assert!(mem.enabled);
    }

    #[test]
    fn test_memory_affinity_disabled() {
        let range = MemoryRange::hotpluggable(0x8000_0000, 0x4000_0000);
        let mem = MemoryAffinity::disabled(range, NodeId::new(2));
        assert!(!mem.enabled);
        assert!(mem.range.hotpluggable);
    }

    #[test]
    fn test_node_stats_creation() {
        let mut stats = NodeStats::new();
        stats.total_memory = 0x1_0000_0000; // 4GB
        stats.free_memory = 0x8000_0000; // 2GB
        stats.cpu_count = 8;

        assert_eq!(stats.memory_utilization(), 50.0);
    }

    #[test]
    fn test_node_stats_hit_rate() {
        let mut stats = NodeStats::new();
        stats.local_hits = 90;
        stats.remote_accesses = 10;

        assert_eq!(stats.local_hit_rate(), 90.0);
    }

    #[test]
    fn test_node_stats_access_tracking() {
        let mut stats = NodeStats::new();
        stats.total_memory = 0x1000_0000;
        stats.free_memory = 0x1000_0000;

        stats.record_local_access();
        stats.record_remote_access();
        assert_eq!(stats.local_hits, 1);
        assert_eq!(stats.remote_accesses, 1);

        stats.record_allocation(0x1000, false);
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.remote_allocations, 0);
        assert_eq!(stats.free_memory, 0x1000_0000 - 0x1000);

        stats.record_allocation(0x2000, true);
        assert_eq!(stats.allocations, 2);
        assert_eq!(stats.remote_allocations, 1);
    }

    #[test]
    fn test_interleaving_mode_none() {
        let mode = InterleavingMode::None;
        assert!(!mode.includes_node(NodeId::new(0)));
        assert!(!mode.includes_node(NodeId::new(1)));
    }

    #[test]
    fn test_interleaving_mode_round_robin() {
        let mode = InterleavingMode::RoundRobin;
        assert!(mode.includes_node(NodeId::new(0)));
        assert!(mode.includes_node(NodeId::new(63)));
    }

    #[test]
    fn test_interleaving_mode_node_set() {
        let mode = InterleavingMode::node_set(&[NodeId::new(0), NodeId::new(2), NodeId::new(4)]);
        assert!(mode.includes_node(NodeId::new(0)));
        assert!(!mode.includes_node(NodeId::new(1)));
        assert!(mode.includes_node(NodeId::new(2)));
        assert!(!mode.includes_node(NodeId::new(3)));
        assert!(mode.includes_node(NodeId::new(4)));
    }

    #[test]
    fn test_allocation_policy_default() {
        let policy = AllocationPolicy::default();
        assert_eq!(policy, AllocationPolicy::Local);
    }

    #[test]
    fn test_allocation_policy_variants() {
        let local = AllocationPolicy::local();
        assert_eq!(local, AllocationPolicy::Local);

        let preferred = AllocationPolicy::preferred(NodeId::new(1));
        assert_eq!(preferred, AllocationPolicy::Preferred(NodeId::new(1)));

        let interleaved = AllocationPolicy::interleaved();
        assert_eq!(
            interleaved,
            AllocationPolicy::Interleaved(InterleavingMode::RoundRobin)
        );

        let bound = AllocationPolicy::bind(NodeId::new(2));
        assert_eq!(bound, AllocationPolicy::Bind(NodeId::new(2)));
    }
}
