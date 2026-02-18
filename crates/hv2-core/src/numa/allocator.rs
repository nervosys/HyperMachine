//! NUMA-aware memory allocator
//!
//! This module provides a memory allocator that is aware of NUMA topology
//! and can allocate memory from specific nodes or with interleaving policies.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::topology::NumaTopology;
use super::types::{AllocationPolicy, InterleavingMode, MemoryRange, NodeId};

/// Error type for allocation failures
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AllocError {
    /// No memory available on any node
    #[error("Out of memory")]
    OutOfMemory,
    /// No memory available on the requested node
    #[error("Out of memory on {0}")]
    NodeOutOfMemory(NodeId),
    /// Invalid node specified
    #[error("Invalid node: {0}")]
    InvalidNode(NodeId),
    /// Allocation size is too large
    #[error("Allocation too large")]
    AllocationTooLarge,
    /// Alignment is invalid
    #[error("Invalid alignment")]
    InvalidAlignment,
}

/// Result type for allocation operations
pub type AllocResult<T> = Result<T, AllocError>;

/// A memory allocation record
#[derive(Debug, Clone)]
pub struct Allocation {
    /// Starting address
    pub address: u64,
    /// Size in bytes
    pub size: u64,
    /// Node the allocation is on
    pub node: NodeId,
    /// Allocation policy used
    pub policy: AllocationPolicy,
}

impl Allocation {
    /// Create a new allocation record
    pub fn new(address: u64, size: u64, node: NodeId, policy: AllocationPolicy) -> Self {
        Self {
            address,
            size,
            node,
            policy,
        }
    }

    /// Get the end address (exclusive)
    pub fn end(&self) -> u64 {
        self.address.saturating_add(self.size)
    }
}

/// Free memory region on a node
#[derive(Debug, Clone)]
pub struct FreeRegion {
    /// Starting address
    pub address: u64,
    /// Size in bytes
    pub size: u64,
}

impl FreeRegion {
    /// Create a new free region
    pub fn new(address: u64, size: u64) -> Self {
        Self { address, size }
    }

    /// Get the end address (exclusive)
    pub fn end(&self) -> u64 {
        self.address.saturating_add(self.size)
    }

    /// Check if this region can satisfy an allocation
    pub fn can_allocate(&self, size: u64, alignment: u64) -> bool {
        let aligned_addr = self.aligned_address(alignment);
        if aligned_addr < self.address {
            return false;
        }
        let waste = aligned_addr - self.address;
        self.size >= waste + size
    }

    /// Get the aligned starting address
    pub fn aligned_address(&self, alignment: u64) -> u64 {
        if alignment == 0 {
            return self.address;
        }
        (self.address + alignment - 1) & !(alignment - 1)
    }
}

/// Per-node memory pool
#[derive(Debug)]
pub struct NodeMemoryPool {
    /// Node ID
    pub node: NodeId,
    /// Free regions (sorted by address)
    free_regions: Vec<FreeRegion>,
    /// Total memory in bytes
    total_memory: u64,
    /// Free memory in bytes
    free_memory: u64,
    /// Allocation count
    allocation_count: u64,
}

impl NodeMemoryPool {
    /// Create a new node memory pool
    pub fn new(node: NodeId) -> Self {
        Self {
            node,
            free_regions: Vec::new(),
            total_memory: 0,
            free_memory: 0,
            allocation_count: 0,
        }
    }

    /// Add a memory range to this pool
    pub fn add_range(&mut self, range: &MemoryRange) {
        self.total_memory += range.length;
        self.free_memory += range.length;
        self.free_regions
            .push(FreeRegion::new(range.base, range.length));
        self.coalesce_free_regions();
    }

    /// Get total memory
    pub fn total_memory(&self) -> u64 {
        self.total_memory
    }

    /// Get free memory
    pub fn free_memory(&self) -> u64 {
        self.free_memory
    }

    /// Get allocation count
    pub fn allocation_count(&self) -> u64 {
        self.allocation_count
    }

    /// Allocate memory from this pool
    pub fn allocate(&mut self, size: u64, alignment: u64) -> AllocResult<u64> {
        if size == 0 {
            return Err(AllocError::InvalidAlignment);
        }

        let alignment = if alignment == 0 { 1 } else { alignment };

        // Find a suitable free region (first-fit)
        let mut best_idx = None;
        for (idx, region) in self.free_regions.iter().enumerate() {
            if region.can_allocate(size, alignment) {
                best_idx = Some(idx);
                break;
            }
        }

        let idx = best_idx.ok_or(AllocError::NodeOutOfMemory(self.node))?;

        // Perform the allocation
        let region = &self.free_regions[idx];
        let alloc_addr = region.aligned_address(alignment);
        let waste_before = alloc_addr - region.address;
        let remaining_after = region.size - waste_before - size;

        // Split the region
        let mut new_regions = Vec::new();

        if waste_before > 0 {
            new_regions.push(FreeRegion::new(region.address, waste_before));
        }
        if remaining_after > 0 {
            new_regions.push(FreeRegion::new(alloc_addr + size, remaining_after));
        }

        self.free_regions.remove(idx);
        for (i, r) in new_regions.into_iter().enumerate() {
            self.free_regions.insert(idx + i, r);
        }

        self.free_memory -= size;
        self.allocation_count += 1;

        Ok(alloc_addr)
    }

    /// Free memory back to this pool
    pub fn free(&mut self, address: u64, size: u64) {
        // Insert the freed region
        let new_region = FreeRegion::new(address, size);

        // Find insertion point (keep sorted by address)
        let insert_idx = self
            .free_regions
            .iter()
            .position(|r| r.address > address)
            .unwrap_or(self.free_regions.len());

        self.free_regions.insert(insert_idx, new_region);
        self.free_memory += size;

        // Coalesce adjacent regions
        self.coalesce_free_regions();
    }

    /// Coalesce adjacent free regions
    fn coalesce_free_regions(&mut self) {
        if self.free_regions.len() < 2 {
            return;
        }

        // Sort by address
        self.free_regions.sort_by_key(|r| r.address);

        let mut i = 0;
        while i < self.free_regions.len() - 1 {
            let current_end = self.free_regions[i].end();
            let next_start = self.free_regions[i + 1].address;

            if current_end == next_start {
                // Merge regions
                self.free_regions[i].size += self.free_regions[i + 1].size;
                self.free_regions.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }
}

/// NUMA-aware memory allocator
#[derive(Debug)]
pub struct NumaAllocator {
    /// Per-node memory pools
    pools: HashMap<NodeId, NodeMemoryPool>,
    /// Current allocation for round-robin
    current_node: AtomicU64,
    /// Node count
    node_count: usize,
    /// Active allocations
    allocations: HashMap<u64, Allocation>,
    /// Default allocation policy
    default_policy: AllocationPolicy,
}

impl NumaAllocator {
    /// Create a new NUMA allocator from topology
    pub fn from_topology(topology: &NumaTopology) -> Self {
        let mut pools = HashMap::new();

        for node_id in topology.node_ids() {
            let mut pool = NodeMemoryPool::new(node_id);

            if let Some(node) = topology.node(node_id) {
                for range in &node.memory_ranges {
                    pool.add_range(range);
                }
            }

            pools.insert(node_id, pool);
        }

        Self {
            pools,
            current_node: AtomicU64::new(0),
            node_count: topology.node_count(),
            allocations: HashMap::new(),
            default_policy: AllocationPolicy::Local,
        }
    }

    /// Create a simple allocator with the given nodes and memory
    pub fn simple(node_count: usize, memory_per_node: u64) -> Self {
        let mut pools = HashMap::new();

        for i in 0..node_count {
            let node_id = NodeId::new(i as u32);
            let mut pool = NodeMemoryPool::new(node_id);
            let range = MemoryRange::new(i as u64 * memory_per_node, memory_per_node);
            pool.add_range(&range);
            pools.insert(node_id, pool);
        }

        Self {
            pools,
            current_node: AtomicU64::new(0),
            node_count,
            allocations: HashMap::new(),
            default_policy: AllocationPolicy::Local,
        }
    }

    /// Set the default allocation policy
    pub fn set_default_policy(&mut self, policy: AllocationPolicy) {
        self.default_policy = policy;
    }

    /// Get the default allocation policy
    pub fn default_policy(&self) -> AllocationPolicy {
        self.default_policy
    }

    /// Allocate memory using the default policy
    pub fn allocate(&mut self, size: u64, local_node: NodeId) -> AllocResult<Allocation> {
        self.allocate_with_policy(size, 1, local_node, self.default_policy)
    }

    /// Allocate aligned memory using the default policy
    pub fn allocate_aligned(
        &mut self,
        size: u64,
        alignment: u64,
        local_node: NodeId,
    ) -> AllocResult<Allocation> {
        self.allocate_with_policy(size, alignment, local_node, self.default_policy)
    }

    /// Allocate memory with a specific policy
    pub fn allocate_with_policy(
        &mut self,
        size: u64,
        alignment: u64,
        local_node: NodeId,
        policy: AllocationPolicy,
    ) -> AllocResult<Allocation> {
        let target_node = self.select_node(&policy, local_node, size)?;
        self.allocate_on_node(size, alignment, target_node, policy)
    }

    /// Allocate memory on a specific node
    pub fn allocate_on_node(
        &mut self,
        size: u64,
        alignment: u64,
        node: NodeId,
        policy: AllocationPolicy,
    ) -> AllocResult<Allocation> {
        let pool = self
            .pools
            .get_mut(&node)
            .ok_or(AllocError::InvalidNode(node))?;

        let address = pool.allocate(size, alignment)?;

        let allocation = Allocation::new(address, size, node, policy);
        self.allocations.insert(address, allocation.clone());

        Ok(allocation)
    }

    /// Free an allocation
    pub fn free(&mut self, address: u64) -> Option<Allocation> {
        let allocation = self.allocations.remove(&address)?;

        if let Some(pool) = self.pools.get_mut(&allocation.node) {
            pool.free(allocation.address, allocation.size);
        }

        Some(allocation)
    }

    /// Free an allocation by reference
    pub fn free_allocation(&mut self, allocation: &Allocation) {
        self.free(allocation.address);
    }

    /// Select a node based on allocation policy
    fn select_node(
        &mut self,
        policy: &AllocationPolicy,
        local_node: NodeId,
        size: u64,
    ) -> AllocResult<NodeId> {
        match policy {
            AllocationPolicy::Local => {
                if self.node_has_memory(local_node, size) {
                    Ok(local_node)
                } else {
                    Err(AllocError::NodeOutOfMemory(local_node))
                }
            }
            AllocationPolicy::Preferred(preferred) => {
                if self.node_has_memory(*preferred, size) {
                    Ok(*preferred)
                } else {
                    self.find_any_node_with_memory(size)
                }
            }
            AllocationPolicy::NearestAvailable => self.find_any_node_with_memory(size),
            AllocationPolicy::Interleaved(mode) => self.select_interleaved_node(mode, size),
            AllocationPolicy::Bind(node) => {
                if self.node_has_memory(*node, size) {
                    Ok(*node)
                } else {
                    Err(AllocError::NodeOutOfMemory(*node))
                }
            }
        }
    }

    /// Check if a node has sufficient free memory
    fn node_has_memory(&self, node: NodeId, size: u64) -> bool {
        self.pools
            .get(&node)
            .map(|p| p.free_memory() >= size)
            .unwrap_or(false)
    }

    /// Find any node with sufficient memory
    fn find_any_node_with_memory(&self, size: u64) -> AllocResult<NodeId> {
        for (id, pool) in &self.pools {
            if pool.free_memory() >= size {
                return Ok(*id);
            }
        }
        Err(AllocError::OutOfMemory)
    }

    /// Select a node using interleaved allocation
    fn select_interleaved_node(
        &mut self,
        mode: &InterleavingMode,
        size: u64,
    ) -> AllocResult<NodeId> {
        let node_ids: Vec<NodeId> = match mode {
            InterleavingMode::None => return Err(AllocError::OutOfMemory),
            InterleavingMode::RoundRobin | InterleavingMode::Weighted => {
                self.pools.keys().copied().collect()
            }
            InterleavingMode::NodeSet(_) => self
                .pools
                .keys()
                .copied()
                .filter(|n| mode.includes_node(*n))
                .collect(),
        };

        if node_ids.is_empty() {
            return Err(AllocError::OutOfMemory);
        }

        // Round-robin through nodes
        for _ in 0..node_ids.len() {
            let current = self.current_node.fetch_add(1, Ordering::Relaxed);
            let idx = (current as usize) % node_ids.len();
            let node = node_ids[idx];

            if self.node_has_memory(node, size) {
                return Ok(node);
            }
        }

        Err(AllocError::OutOfMemory)
    }

    /// Get statistics for a node
    pub fn node_stats(&self, node: NodeId) -> Option<NodePoolStats> {
        self.pools.get(&node).map(|pool| NodePoolStats {
            node,
            total_memory: pool.total_memory(),
            free_memory: pool.free_memory(),
            allocation_count: pool.allocation_count(),
        })
    }

    /// Get total free memory across all nodes
    pub fn total_free_memory(&self) -> u64 {
        self.pools.values().map(|p| p.free_memory()).sum()
    }

    /// Get total memory across all nodes
    pub fn total_memory(&self) -> u64 {
        self.pools.values().map(|p| p.total_memory()).sum()
    }

    /// Get the number of active allocations
    pub fn allocation_count(&self) -> usize {
        self.allocations.len()
    }

    /// Get an allocation by address
    pub fn get_allocation(&self, address: u64) -> Option<&Allocation> {
        self.allocations.get(&address)
    }
}

/// Statistics for a node's memory pool
#[derive(Debug, Clone)]
pub struct NodePoolStats {
    /// Node ID
    pub node: NodeId,
    /// Total memory in bytes
    pub total_memory: u64,
    /// Free memory in bytes
    pub free_memory: u64,
    /// Number of allocations
    pub allocation_count: u64,
}

impl NodePoolStats {
    /// Get memory utilization as a percentage
    pub fn utilization(&self) -> f64 {
        if self.total_memory == 0 {
            0.0
        } else {
            let used = self.total_memory - self.free_memory;
            (used as f64 / self.total_memory as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_error_display() {
        assert_eq!(format!("{}", AllocError::OutOfMemory), "Out of memory");
        assert_eq!(
            format!("{}", AllocError::NodeOutOfMemory(NodeId::new(1))),
            "Out of memory on Node1"
        );
    }

    #[test]
    fn test_allocation_creation() {
        let alloc = Allocation::new(0x1000, 0x100, NodeId::new(0), AllocationPolicy::Local);
        assert_eq!(alloc.address, 0x1000);
        assert_eq!(alloc.size, 0x100);
        assert_eq!(alloc.end(), 0x1100);
    }

    #[test]
    fn test_free_region() {
        let region = FreeRegion::new(0x1000, 0x2000);
        assert_eq!(region.end(), 0x3000);
        assert!(region.can_allocate(0x1000, 1));
        assert!(region.can_allocate(0x2000, 1));
        assert!(!region.can_allocate(0x3000, 1));
    }

    #[test]
    fn test_free_region_alignment() {
        let region = FreeRegion::new(0x1001, 0x2000);

        // Aligned address should be 0x1010 for 16-byte alignment
        assert_eq!(region.aligned_address(16), 0x1010);

        // Should be able to allocate with alignment
        assert!(region.can_allocate(0x1000, 16));
    }

    #[test]
    fn test_node_memory_pool_basic() {
        let mut pool = NodeMemoryPool::new(NodeId::new(0));
        pool.add_range(&MemoryRange::new(0, 0x1000_0000));

        assert_eq!(pool.total_memory(), 0x1000_0000);
        assert_eq!(pool.free_memory(), 0x1000_0000);
    }

    #[test]
    fn test_node_memory_pool_allocate() {
        let mut pool = NodeMemoryPool::new(NodeId::new(0));
        pool.add_range(&MemoryRange::new(0, 0x1000_0000));

        let addr1 = pool.allocate(0x1000, 1).unwrap();
        assert_eq!(addr1, 0);
        assert_eq!(pool.free_memory(), 0x1000_0000 - 0x1000);

        let addr2 = pool.allocate(0x1000, 1).unwrap();
        assert_eq!(addr2, 0x1000);
    }

    #[test]
    fn test_node_memory_pool_free() {
        let mut pool = NodeMemoryPool::new(NodeId::new(0));
        pool.add_range(&MemoryRange::new(0, 0x1000_0000));

        let addr = pool.allocate(0x1000, 1).unwrap();
        assert_eq!(pool.free_memory(), 0x1000_0000 - 0x1000);

        pool.free(addr, 0x1000);
        assert_eq!(pool.free_memory(), 0x1000_0000);
    }

    #[test]
    fn test_node_memory_pool_coalesce() {
        let mut pool = NodeMemoryPool::new(NodeId::new(0));
        pool.add_range(&MemoryRange::new(0, 0x3000));

        // Allocate three blocks
        let addr1 = pool.allocate(0x1000, 1).unwrap();
        let addr2 = pool.allocate(0x1000, 1).unwrap();
        let addr3 = pool.allocate(0x1000, 1).unwrap();

        assert_eq!(pool.free_memory(), 0);

        // Free middle block first
        pool.free(addr2, 0x1000);
        assert_eq!(pool.free_regions.len(), 1);

        // Free first block - should coalesce with middle
        pool.free(addr1, 0x1000);
        assert_eq!(pool.free_regions.len(), 1);

        // Free last block - should coalesce all
        pool.free(addr3, 0x1000);
        assert_eq!(pool.free_regions.len(), 1);
        assert_eq!(pool.free_memory(), 0x3000);
    }

    #[test]
    fn test_numa_allocator_simple() {
        let allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        assert_eq!(allocator.total_memory(), 0x2_0000_0000);
        assert_eq!(allocator.total_free_memory(), 0x2_0000_0000);
    }

    #[test]
    fn test_numa_allocator_allocate_local() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let alloc = allocator.allocate(0x1000, NodeId::new(0)).unwrap();
        assert_eq!(alloc.node, NodeId::new(0));
        assert_eq!(alloc.size, 0x1000);
        assert_eq!(allocator.allocation_count(), 1);
    }

    #[test]
    fn test_numa_allocator_allocate_on_node() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let alloc = allocator
            .allocate_on_node(0x1000, 1, NodeId::new(1), AllocationPolicy::Local)
            .unwrap();
        assert_eq!(alloc.node, NodeId::new(1));
    }

    #[test]
    fn test_numa_allocator_free() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let alloc = allocator.allocate(0x1000, NodeId::new(0)).unwrap();
        let addr = alloc.address;

        let freed = allocator.free(addr);
        assert!(freed.is_some());
        assert_eq!(allocator.allocation_count(), 0);
    }

    #[test]
    fn test_numa_allocator_policy_preferred() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);
        allocator.set_default_policy(AllocationPolicy::Preferred(NodeId::new(1)));

        let alloc = allocator.allocate(0x1000, NodeId::new(0)).unwrap();
        assert_eq!(alloc.node, NodeId::new(1)); // Should use preferred node
    }

    #[test]
    fn test_numa_allocator_policy_bind() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let alloc = allocator
            .allocate_with_policy(
                0x1000,
                1,
                NodeId::new(0),
                AllocationPolicy::Bind(NodeId::new(1)),
            )
            .unwrap();
        assert_eq!(alloc.node, NodeId::new(1));
    }

    #[test]
    fn test_numa_allocator_policy_interleaved() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let policy = AllocationPolicy::Interleaved(InterleavingMode::RoundRobin);

        let alloc1 = allocator
            .allocate_with_policy(0x1000, 1, NodeId::new(0), policy)
            .unwrap();
        let alloc2 = allocator
            .allocate_with_policy(0x1000, 1, NodeId::new(0), policy)
            .unwrap();

        // Allocations should be on different nodes (round-robin)
        assert_ne!(alloc1.node, alloc2.node);
    }

    #[test]
    fn test_numa_allocator_out_of_memory() {
        let mut allocator = NumaAllocator::simple(1, 0x1000);

        // This should succeed
        let _ = allocator.allocate(0x800, NodeId::new(0)).unwrap();

        // This should fail
        let result = allocator.allocate(0x1000, NodeId::new(0));
        assert!(matches!(result, Err(AllocError::NodeOutOfMemory(_))));
    }

    #[test]
    fn test_numa_allocator_invalid_node() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let result = allocator.allocate_on_node(
            0x1000,
            1,
            NodeId::new(5), // Invalid node
            AllocationPolicy::Local,
        );
        assert!(matches!(result, Err(AllocError::InvalidNode(_))));
    }

    #[test]
    fn test_numa_allocator_node_stats() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        allocator
            .allocate_on_node(0x1000, 1, NodeId::new(0), AllocationPolicy::Local)
            .unwrap();

        let stats = allocator.node_stats(NodeId::new(0)).unwrap();
        assert_eq!(stats.total_memory, 0x1_0000_0000);
        assert_eq!(stats.free_memory, 0x1_0000_0000 - 0x1000);
        assert_eq!(stats.allocation_count, 1);
        assert!(stats.utilization() > 0.0);
    }

    #[test]
    fn test_numa_allocator_get_allocation() {
        let mut allocator = NumaAllocator::simple(2, 0x1_0000_0000);

        let alloc = allocator.allocate(0x1000, NodeId::new(0)).unwrap();
        let addr = alloc.address;

        let found = allocator.get_allocation(addr);
        assert!(found.is_some());
        assert_eq!(found.unwrap().size, 0x1000);
    }

    #[test]
    fn test_numa_allocator_aligned() {
        let mut allocator = NumaAllocator::simple(1, 0x1_0000_0000);

        let alloc = allocator
            .allocate_aligned(0x1000, 0x1000, NodeId::new(0))
            .unwrap();
        assert_eq!(alloc.address & 0xFFF, 0); // Should be page-aligned
    }

    #[test]
    fn test_numa_allocator_from_topology() {
        let topology = NumaTopology::two_node(4, 0x1_0000_0000);
        let mut allocator = NumaAllocator::from_topology(&topology);

        assert_eq!(allocator.total_memory(), 0x2_0000_0000);

        let alloc = allocator.allocate(0x1000, NodeId::new(0)).unwrap();
        assert!(alloc.address < 0x1_0000_0000); // Should be in node 0's range
    }
}
