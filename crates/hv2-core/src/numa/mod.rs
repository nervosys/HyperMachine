//! NUMA (Non-Uniform Memory Access) support
//!
//! This module provides comprehensive NUMA topology management including:
//! - Node identification and memory ranges
//! - CPU-to-node affinity mapping
//! - Distance matrix for inter-node latency
//! - ACPI SRAT/SLIT table generation
//! - NUMA-aware memory allocation

pub mod acpi;
pub mod allocator;
pub mod topology;
pub mod types;

// Re-export commonly used types
pub use acpi::{
    AcpiHeader, MemoryAffinityFlags, MemoryAffinityStructure, NumaAcpiTables,
    ProcessorAffinityFlags, ProcessorLocalApicAffinity, ProcessorX2ApicAffinity, SlitBuilder,
    SratBuilder, SratSubtableType, SLIT_REVISION, SLIT_SIGNATURE, SRAT_REVISION, SRAT_SIGNATURE,
};

pub use allocator::{
    AllocError, AllocResult, Allocation, FreeRegion, NodeMemoryPool, NodePoolStats, NumaAllocator,
};

pub use topology::{NumaNode, NumaNodeConfig, NumaTopology, NumaTopologyBuilder};

pub use types::{
    AllocationPolicy, CpuAffinity, DistanceMatrix, InterleavingMode, MemoryAffinity, MemoryRange,
    NodeId, NodeStats, NumaDistance, NUMA_DISTANCE_FAR, NUMA_DISTANCE_LOCAL, NUMA_DISTANCE_REMOTE,
    NUMA_DISTANCE_UNREACHABLE, NUMA_DISTANCE_VERY_FAR,
};
