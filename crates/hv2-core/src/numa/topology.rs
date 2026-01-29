//! NUMA topology management
//!
//! This module provides structures and functions for discovering and managing
//! NUMA topology including nodes, CPU assignments, and memory regions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::types::{
    AllocationPolicy, CpuAffinity, DistanceMatrix, InterleavingMode, MemoryAffinity, MemoryRange,
    NodeId, NodeStats, NUMA_DISTANCE_LOCAL, NUMA_DISTANCE_REMOTE,
};

/// A single NUMA node with its resources
#[derive(Debug, Clone)]
pub struct NumaNode {
    /// Node identifier
    pub id: NodeId,
    /// CPUs on this node (by APIC ID)
    pub cpus: Vec<u32>,
    /// Memory ranges on this node
    pub memory_ranges: Vec<MemoryRange>,
    /// Total memory in bytes
    pub total_memory: u64,
    /// Whether this node is online
    pub online: bool,
}

impl NumaNode {
    /// Create a new NUMA node
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            cpus: Vec::new(),
            memory_ranges: Vec::new(),
            total_memory: 0,
            online: true,
        }
    }

    /// Add a CPU to this node
    pub fn add_cpu(&mut self, apic_id: u32) {
        if !self.cpus.contains(&apic_id) {
            self.cpus.push(apic_id);
        }
    }

    /// Remove a CPU from this node
    pub fn remove_cpu(&mut self, apic_id: u32) {
        self.cpus.retain(|&id| id != apic_id);
    }

    /// Add a memory range to this node
    pub fn add_memory(&mut self, range: MemoryRange) {
        self.total_memory += range.length;
        self.memory_ranges.push(range);
    }

    /// Get the number of CPUs on this node
    pub fn cpu_count(&self) -> usize {
        self.cpus.len()
    }

    /// Get the total memory in bytes
    pub fn memory_bytes(&self) -> u64 {
        self.total_memory
    }

    /// Get the total memory in megabytes
    pub fn memory_mb(&self) -> u64 {
        self.total_memory >> 20
    }

    /// Get the total memory in gigabytes
    pub fn memory_gb(&self) -> u64 {
        self.total_memory >> 30
    }

    /// Check if this node contains a given address
    pub fn contains_address(&self, addr: u64) -> bool {
        self.memory_ranges.iter().any(|r| r.contains(addr))
    }

    /// Check if this node contains a given CPU
    pub fn contains_cpu(&self, apic_id: u32) -> bool {
        self.cpus.contains(&apic_id)
    }

    /// Set the node online/offline status
    pub fn set_online(&mut self, online: bool) {
        self.online = online;
    }
}

/// NUMA topology information
#[derive(Debug)]
pub struct NumaTopology {
    /// All NUMA nodes
    nodes: HashMap<NodeId, NumaNode>,
    /// CPU to node mapping
    cpu_to_node: HashMap<u32, NodeId>,
    /// Distance matrix between nodes
    distance_matrix: DistanceMatrix,
    /// Node statistics
    stats: HashMap<NodeId, NodeStats>,
    /// Current node for round-robin allocation
    current_node: AtomicU64,
    /// Number of nodes
    node_count: usize,
}

impl NumaTopology {
    /// Create a new NUMA topology with the given number of nodes
    pub fn new(node_count: usize) -> Self {
        let mut nodes = HashMap::new();
        let mut stats = HashMap::new();

        for i in 0..node_count {
            let id = NodeId::new(i as u32);
            nodes.insert(id, NumaNode::new(id));
            stats.insert(id, NodeStats::new());
        }

        Self {
            nodes,
            cpu_to_node: HashMap::new(),
            distance_matrix: DistanceMatrix::new(node_count),
            stats,
            current_node: AtomicU64::new(0),
            node_count,
        }
    }

    /// Create a simple 2-node NUMA topology
    pub fn two_node(cpus_per_node: u32, memory_per_node: u64) -> Self {
        let mut topology = Self::new(2);

        // Add CPUs to nodes
        for i in 0..cpus_per_node {
            topology.add_cpu(i, NodeId::new(0));
            topology.add_cpu(cpus_per_node + i, NodeId::new(1));
        }

        // Add memory to nodes
        topology.add_memory(NodeId::new(0), MemoryRange::new(0, memory_per_node));
        topology.add_memory(
            NodeId::new(1),
            MemoryRange::new(memory_per_node, memory_per_node),
        );

        topology
    }

    /// Create a 4-node NUMA topology (typical 2-socket Xeon)
    pub fn four_node(cpus_per_node: u32, memory_per_node: u64) -> Self {
        let mut topology = Self::new(4);

        // Add CPUs and memory to each node
        for node in 0..4u32 {
            let node_id = NodeId::new(node);
            for i in 0..cpus_per_node {
                topology.add_cpu(node * cpus_per_node + i, node_id);
            }
            topology.add_memory(
                node_id,
                MemoryRange::new(node as u64 * memory_per_node, memory_per_node),
            );
        }

        // Set up typical 4-node distances
        // Nodes 0,1 are on socket 0, nodes 2,3 are on socket 1
        topology.set_distance(NodeId::new(0), NodeId::new(1), 20);
        topology.set_distance(NodeId::new(2), NodeId::new(3), 20);
        topology.set_distance(NodeId::new(0), NodeId::new(2), 30);
        topology.set_distance(NodeId::new(0), NodeId::new(3), 30);
        topology.set_distance(NodeId::new(1), NodeId::new(2), 30);
        topology.set_distance(NodeId::new(1), NodeId::new(3), 30);

        topology
    }

    /// Add a CPU to a node
    pub fn add_cpu(&mut self, apic_id: u32, node: NodeId) {
        if let Some(numa_node) = self.nodes.get_mut(&node) {
            numa_node.add_cpu(apic_id);
            self.cpu_to_node.insert(apic_id, node);
        }
    }

    /// Remove a CPU from its node
    pub fn remove_cpu(&mut self, apic_id: u32) {
        if let Some(node) = self.cpu_to_node.remove(&apic_id) {
            if let Some(numa_node) = self.nodes.get_mut(&node) {
                numa_node.remove_cpu(apic_id);
            }
        }
    }

    /// Add memory to a node
    pub fn add_memory(&mut self, node: NodeId, range: MemoryRange) {
        if let Some(numa_node) = self.nodes.get_mut(&node) {
            let length = range.length;
            numa_node.add_memory(range);

            // Update stats
            if let Some(stats) = self.stats.get_mut(&node) {
                stats.total_memory += length;
                stats.free_memory += length;
                stats.cpu_count = numa_node.cpu_count() as u32;
            }
        }
    }

    /// Set the distance between two nodes (symmetric)
    pub fn set_distance(&mut self, node1: NodeId, node2: NodeId, distance: u8) {
        self.distance_matrix.set_symmetric(node1, node2, distance);
    }

    /// Get the distance between two nodes
    pub fn get_distance(&self, from: NodeId, to: NodeId) -> u8 {
        self.distance_matrix.get(from, to)
    }

    /// Get the node for a CPU
    pub fn cpu_node(&self, apic_id: u32) -> Option<NodeId> {
        self.cpu_to_node.get(&apic_id).copied()
    }

    /// Get the node for an address
    pub fn address_node(&self, addr: u64) -> Option<NodeId> {
        for (id, node) in &self.nodes {
            if node.contains_address(addr) {
                return Some(*id);
            }
        }
        None
    }

    /// Get a node by ID
    pub fn node(&self, id: NodeId) -> Option<&NumaNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable reference to a node
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut NumaNode> {
        self.nodes.get_mut(&id)
    }

    /// Get the number of nodes
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Get all node IDs
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().copied().collect()
    }

    /// Get all online node IDs
    pub fn online_node_ids(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.online)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get the total number of CPUs
    pub fn total_cpus(&self) -> usize {
        self.cpu_to_node.len()
    }

    /// Get the total memory across all nodes
    pub fn total_memory(&self) -> u64 {
        self.nodes.values().map(|n| n.total_memory).sum()
    }

    /// Get the distance matrix
    pub fn distance_matrix(&self) -> &DistanceMatrix {
        &self.distance_matrix
    }

    /// Get statistics for a node
    pub fn node_stats(&self, node: NodeId) -> Option<&NodeStats> {
        self.stats.get(&node)
    }

    /// Get mutable statistics for a node
    pub fn node_stats_mut(&mut self, node: NodeId) -> Option<&mut NodeStats> {
        self.stats.get_mut(&node)
    }

    /// Find the nearest node with available memory
    pub fn nearest_available_node(&self, from: NodeId, min_free: u64) -> Option<NodeId> {
        let mut best = None;
        let mut best_distance = u8::MAX;

        for (id, node) in &self.nodes {
            if !node.online {
                continue;
            }

            if let Some(stats) = self.stats.get(id) {
                if stats.free_memory >= min_free {
                    let distance = self.distance_matrix.get(from, *id);
                    if distance < best_distance {
                        best_distance = distance;
                        best = Some(*id);
                    }
                }
            }
        }

        best
    }

    /// Select a node based on allocation policy
    pub fn select_node(
        &self,
        policy: &AllocationPolicy,
        local_node: NodeId,
        size: u64,
    ) -> Option<NodeId> {
        match policy {
            AllocationPolicy::Local => {
                if self.node_has_memory(local_node, size) {
                    Some(local_node)
                } else {
                    None
                }
            }
            AllocationPolicy::Preferred(preferred) => {
                if self.node_has_memory(*preferred, size) {
                    Some(*preferred)
                } else {
                    self.nearest_available_node(*preferred, size)
                }
            }
            AllocationPolicy::NearestAvailable => self.nearest_available_node(local_node, size),
            AllocationPolicy::Interleaved(mode) => self.select_interleaved_node(mode, size),
            AllocationPolicy::Bind(node) => {
                if self.node_has_memory(*node, size) {
                    Some(*node)
                } else {
                    None
                }
            }
        }
    }

    /// Check if a node has sufficient free memory
    fn node_has_memory(&self, node: NodeId, size: u64) -> bool {
        self.stats
            .get(&node)
            .map(|s| s.free_memory >= size)
            .unwrap_or(false)
    }

    /// Select a node using interleaved allocation
    fn select_interleaved_node(&self, mode: &InterleavingMode, size: u64) -> Option<NodeId> {
        let online_nodes: Vec<NodeId> = self.online_node_ids();
        if online_nodes.is_empty() {
            return None;
        }

        let filtered_nodes: Vec<NodeId> = match mode {
            InterleavingMode::None => return None,
            InterleavingMode::RoundRobin => online_nodes,
            InterleavingMode::Weighted => online_nodes, // Simplified: same as round-robin
            InterleavingMode::NodeSet(_) => online_nodes
                .into_iter()
                .filter(|n| mode.includes_node(*n))
                .collect(),
        };

        if filtered_nodes.is_empty() {
            return None;
        }

        // Round-robin through available nodes
        let count = filtered_nodes.len();
        let mut attempts = 0;

        loop {
            let current = self.current_node.fetch_add(1, Ordering::Relaxed);
            let index = (current as usize) % count;
            let node = filtered_nodes[index];

            if self.node_has_memory(node, size) {
                return Some(node);
            }

            attempts += 1;
            if attempts >= count {
                return None; // All nodes checked, none have memory
            }
        }
    }

    /// Generate CPU affinity entries for ACPI SRAT
    pub fn cpu_affinities(&self) -> Vec<CpuAffinity> {
        let mut affinities = Vec::new();

        for (apic_id, node) in &self.cpu_to_node {
            affinities.push(CpuAffinity::new(*apic_id, *node));
        }

        // Sort by APIC ID for deterministic output
        affinities.sort_by_key(|a| a.apic_id);
        affinities
    }

    /// Generate memory affinity entries for ACPI SRAT
    pub fn memory_affinities(&self) -> Vec<MemoryAffinity> {
        let mut affinities = Vec::new();

        for (id, node) in &self.nodes {
            for range in &node.memory_ranges {
                affinities.push(MemoryAffinity::new(*range, *id));
            }
        }

        // Sort by base address for deterministic output
        affinities.sort_by_key(|a| a.range.base);
        affinities
    }

    /// Record a memory access for statistics
    pub fn record_access(&mut self, addr: u64, accessing_cpu: u32) {
        let addr_node = self.address_node(addr);
        let cpu_node = self.cpu_node(accessing_cpu);

        if let (Some(addr_node), Some(cpu_node)) = (addr_node, cpu_node) {
            if let Some(stats) = self.stats.get_mut(&addr_node) {
                if addr_node == cpu_node {
                    stats.record_local_access();
                } else {
                    stats.record_remote_access();
                }
            }
        }
    }
}

impl Default for NumaTopology {
    fn default() -> Self {
        Self::new(1)
    }
}

/// NUMA topology builder for easier construction
#[derive(Debug, Default)]
pub struct NumaTopologyBuilder {
    nodes: Vec<NumaNodeConfig>,
    distances: Vec<(u32, u32, u8)>,
}

/// Configuration for a single NUMA node
#[derive(Debug, Clone)]
pub struct NumaNodeConfig {
    /// CPUs on this node (APIC IDs)
    pub cpus: Vec<u32>,
    /// Memory ranges on this node
    pub memory: Vec<MemoryRange>,
}

impl NumaNodeConfig {
    /// Create a new node configuration
    pub fn new() -> Self {
        Self {
            cpus: Vec::new(),
            memory: Vec::new(),
        }
    }

    /// Add CPUs to this node
    pub fn with_cpus(mut self, cpus: impl IntoIterator<Item = u32>) -> Self {
        self.cpus.extend(cpus);
        self
    }

    /// Add a memory range to this node
    pub fn with_memory(mut self, base: u64, length: u64) -> Self {
        self.memory.push(MemoryRange::new(base, length));
        self
    }

    /// Add a hotpluggable memory range
    pub fn with_hotplug_memory(mut self, base: u64, length: u64) -> Self {
        self.memory.push(MemoryRange::hotpluggable(base, length));
        self
    }
}

impl Default for NumaNodeConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl NumaTopologyBuilder {
    /// Create a new topology builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node configuration
    pub fn add_node(mut self, config: NumaNodeConfig) -> Self {
        self.nodes.push(config);
        self
    }

    /// Set the distance between two nodes
    pub fn set_distance(mut self, from: u32, to: u32, distance: u8) -> Self {
        self.distances.push((from, to, distance));
        self
    }

    /// Build the NUMA topology
    pub fn build(self) -> NumaTopology {
        let mut topology = NumaTopology::new(self.nodes.len());

        // Add node configurations
        for (idx, config) in self.nodes.into_iter().enumerate() {
            let node_id = NodeId::new(idx as u32);

            for cpu in config.cpus {
                topology.add_cpu(cpu, node_id);
            }

            for range in config.memory {
                topology.add_memory(node_id, range);
            }
        }

        // Set distances
        for (from, to, distance) in self.distances {
            topology.set_distance(NodeId::new(from), NodeId::new(to), distance);
        }

        topology
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numa_node_creation() {
        let node = NumaNode::new(NodeId::new(0));
        assert_eq!(node.id, NodeId::new(0));
        assert!(node.cpus.is_empty());
        assert!(node.memory_ranges.is_empty());
        assert!(node.online);
    }

    #[test]
    fn test_numa_node_add_cpu() {
        let mut node = NumaNode::new(NodeId::new(0));
        node.add_cpu(0);
        node.add_cpu(1);
        node.add_cpu(0); // Duplicate, should be ignored

        assert_eq!(node.cpu_count(), 2);
        assert!(node.contains_cpu(0));
        assert!(node.contains_cpu(1));
        assert!(!node.contains_cpu(2));
    }

    #[test]
    fn test_numa_node_add_memory() {
        let mut node = NumaNode::new(NodeId::new(0));
        node.add_memory(MemoryRange::new(0, 0x4000_0000)); // 1GB
        node.add_memory(MemoryRange::new(0x4000_0000, 0x4000_0000)); // 1GB

        assert_eq!(node.memory_bytes(), 0x8000_0000);
        assert_eq!(node.memory_gb(), 2);
        assert!(node.contains_address(0x1000_0000));
        assert!(node.contains_address(0x5000_0000));
        assert!(!node.contains_address(0x9000_0000));
    }

    #[test]
    fn test_numa_topology_creation() {
        let topology = NumaTopology::new(4);
        assert_eq!(topology.node_count(), 4);
        assert!(topology.node(NodeId::new(0)).is_some());
        assert!(topology.node(NodeId::new(3)).is_some());
        assert!(topology.node(NodeId::new(4)).is_none());
    }

    #[test]
    fn test_numa_topology_two_node() {
        let topology = NumaTopology::two_node(4, 0x1_0000_0000); // 4 CPUs, 4GB per node

        assert_eq!(topology.node_count(), 2);
        assert_eq!(topology.total_cpus(), 8);
        assert_eq!(topology.total_memory(), 0x2_0000_0000); // 8GB

        // Check CPU assignments
        assert_eq!(topology.cpu_node(0), Some(NodeId::new(0)));
        assert_eq!(topology.cpu_node(3), Some(NodeId::new(0)));
        assert_eq!(topology.cpu_node(4), Some(NodeId::new(1)));
        assert_eq!(topology.cpu_node(7), Some(NodeId::new(1)));
    }

    #[test]
    fn test_numa_topology_four_node() {
        let topology = NumaTopology::four_node(2, 0x8000_0000); // 2 CPUs, 2GB per node

        assert_eq!(topology.node_count(), 4);
        assert_eq!(topology.total_cpus(), 8);
        assert_eq!(topology.total_memory(), 0x2_0000_0000); // 8GB

        // Check distances
        assert_eq!(
            topology.get_distance(NodeId::new(0), NodeId::new(0)),
            NUMA_DISTANCE_LOCAL
        );
        assert_eq!(topology.get_distance(NodeId::new(0), NodeId::new(1)), 20);
        assert_eq!(topology.get_distance(NodeId::new(0), NodeId::new(2)), 30);
    }

    #[test]
    fn test_numa_topology_address_node() {
        let topology = NumaTopology::two_node(2, 0x1_0000_0000);

        assert_eq!(topology.address_node(0x1000), Some(NodeId::new(0)));
        assert_eq!(topology.address_node(0x1_0000_1000), Some(NodeId::new(1)));
        assert_eq!(topology.address_node(0x3_0000_0000), None);
    }

    #[test]
    fn test_numa_topology_cpu_affinities() {
        let topology = NumaTopology::two_node(2, 0x1_0000_0000);
        let affinities = topology.cpu_affinities();

        assert_eq!(affinities.len(), 4);
        assert_eq!(affinities[0].apic_id, 0);
        assert_eq!(affinities[0].node, NodeId::new(0));
        assert_eq!(affinities[2].apic_id, 2);
        assert_eq!(affinities[2].node, NodeId::new(1));
    }

    #[test]
    fn test_numa_topology_memory_affinities() {
        let topology = NumaTopology::two_node(2, 0x1_0000_0000);
        let affinities = topology.memory_affinities();

        assert_eq!(affinities.len(), 2);
        assert_eq!(affinities[0].range.base, 0);
        assert_eq!(affinities[0].node, NodeId::new(0));
        assert_eq!(affinities[1].range.base, 0x1_0000_0000);
        assert_eq!(affinities[1].node, NodeId::new(1));
    }

    #[test]
    fn test_numa_topology_select_node_local() {
        let mut topology = NumaTopology::two_node(2, 0x1_0000_0000);
        let policy = AllocationPolicy::Local;

        // With enough memory
        let selected = topology.select_node(&policy, NodeId::new(0), 0x1000);
        assert_eq!(selected, Some(NodeId::new(0)));

        // Drain memory from node 0
        if let Some(stats) = topology.node_stats_mut(NodeId::new(0)) {
            stats.free_memory = 0;
        }

        let selected = topology.select_node(&policy, NodeId::new(0), 0x1000);
        assert_eq!(selected, None);
    }

    #[test]
    fn test_numa_topology_select_node_preferred() {
        let mut topology = NumaTopology::two_node(2, 0x1_0000_0000);
        let policy = AllocationPolicy::Preferred(NodeId::new(0));

        // Preferred node has memory
        let selected = topology.select_node(&policy, NodeId::new(1), 0x1000);
        assert_eq!(selected, Some(NodeId::new(0)));

        // Drain preferred node
        if let Some(stats) = topology.node_stats_mut(NodeId::new(0)) {
            stats.free_memory = 0;
        }

        // Should fall back to another node
        let selected = topology.select_node(&policy, NodeId::new(1), 0x1000);
        assert_eq!(selected, Some(NodeId::new(1)));
    }

    #[test]
    fn test_numa_topology_select_node_bound() {
        let mut topology = NumaTopology::two_node(2, 0x1_0000_0000);
        let policy = AllocationPolicy::Bind(NodeId::new(0));

        let selected = topology.select_node(&policy, NodeId::new(1), 0x1000);
        assert_eq!(selected, Some(NodeId::new(0)));

        // Drain bound node - should fail
        if let Some(stats) = topology.node_stats_mut(NodeId::new(0)) {
            stats.free_memory = 0;
        }

        let selected = topology.select_node(&policy, NodeId::new(1), 0x1000);
        assert_eq!(selected, None);
    }

    #[test]
    fn test_numa_topology_record_access() {
        let mut topology = NumaTopology::two_node(2, 0x1_0000_0000);

        // Local access
        topology.record_access(0x1000, 0);
        assert_eq!(topology.node_stats(NodeId::new(0)).unwrap().local_hits, 1);

        // Remote access
        topology.record_access(0x1_0000_1000, 0);
        assert_eq!(
            topology.node_stats(NodeId::new(1)).unwrap().remote_accesses,
            1
        );
    }

    #[test]
    fn test_numa_node_config() {
        let config = NumaNodeConfig::new()
            .with_cpus(0..4)
            .with_memory(0, 0x1_0000_0000)
            .with_hotplug_memory(0x1_0000_0000, 0x8000_0000);

        assert_eq!(config.cpus.len(), 4);
        assert_eq!(config.memory.len(), 2);
        assert!(config.memory[1].hotpluggable);
    }

    #[test]
    fn test_numa_topology_builder() {
        let topology = NumaTopologyBuilder::new()
            .add_node(
                NumaNodeConfig::new()
                    .with_cpus(0..4)
                    .with_memory(0, 0x1_0000_0000),
            )
            .add_node(
                NumaNodeConfig::new()
                    .with_cpus(4..8)
                    .with_memory(0x1_0000_0000, 0x1_0000_0000),
            )
            .set_distance(0, 1, 20)
            .build();

        assert_eq!(topology.node_count(), 2);
        assert_eq!(topology.total_cpus(), 8);
        assert_eq!(topology.get_distance(NodeId::new(0), NodeId::new(1)), 20);
    }

    #[test]
    fn test_numa_topology_online_nodes() {
        let mut topology = NumaTopology::two_node(2, 0x1_0000_0000);

        assert_eq!(topology.online_node_ids().len(), 2);

        // Take node 1 offline
        if let Some(node) = topology.node_mut(NodeId::new(1)) {
            node.set_online(false);
        }

        assert_eq!(topology.online_node_ids().len(), 1);
        assert_eq!(topology.online_node_ids()[0], NodeId::new(0));
    }

    #[test]
    fn test_numa_topology_nearest_available() {
        let mut topology = NumaTopology::four_node(2, 0x1_0000_0000);

        // Node 0 looking for memory - node 1 is closest (distance 20)
        let nearest = topology.nearest_available_node(NodeId::new(0), 0x1000);
        assert_eq!(nearest, Some(NodeId::new(0))); // Self is closest

        // Drain node 0
        if let Some(stats) = topology.node_stats_mut(NodeId::new(0)) {
            stats.free_memory = 0;
        }

        // Now node 1 should be closest
        let nearest = topology.nearest_available_node(NodeId::new(0), 0x1000);
        assert_eq!(nearest, Some(NodeId::new(1)));
    }
}
