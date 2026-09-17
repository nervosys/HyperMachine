//! VM Snapshot and Restore
//!
//! This module provides comprehensive snapshot functionality for virtual machines,
//! including full and incremental snapshots, memory management, device state
//! serialization, and external API integration.
//!
//! # Components
//!
//! - [`types`] - Core snapshot types and structures
//! - [`memory`] - Memory snapshot capture and restoration
//! - [`device`] - Device state serialization framework
//! - [`manager`] - Main snapshot lifecycle management
//!
//! # Example
//!
//! ```no_run
//! use hv2_core::snapshot::{CreateSnapshotOptions, SnapshotManager};
//! # fn example() -> hv2_core::snapshot::SnapshotResult<()> {
//! let mut manager = SnapshotManager::with_defaults();
//!
//! // Create a full snapshot
//! let id = manager.begin_snapshot(CreateSnapshotOptions::full())?;
//! // ... add CPU, memory, device state ...
//! manager.complete_snapshot()?;
//!
//! // Restore from snapshot
//! manager.begin_restore(&id)?;
//! // ... restore state ...
//! manager.complete_restore()?;
//! # Ok(())
//! # }
//! ```

pub mod device;
pub mod manager;
pub mod memory;
pub mod types;
pub mod vcpu;

// Re-export main types
pub use device::{
    DeviceResult, DeviceStateDeserializer, DeviceStateError, DeviceStateManager,
    DeviceStateSerializer, DeviceStateStats, Snapshottable, TestDevice,
};
pub use manager::{
    CreateSnapshotOptions, RestoreSnapshotOptions, SnapshotConfig, SnapshotError, SnapshotManager,
    SnapshotManagerStats, SnapshotResult,
};
pub use memory::{
    DirtyPageIterator, DirtyPageTracker, MemoryResult, MemorySnapshotConfig, MemorySnapshotError,
    MemorySnapshotManager, MemorySnapshotStats, PageState,
};
pub use types::{
    CompressionType, CpuSnapshot, DescriptorTableSnapshot, DeviceSnapshot, MemoryRegionSnapshot,
    SegmentSnapshot, SnapshotId, SnapshotInfo, SnapshotState, SnapshotStats, SnapshotType,
    VmSnapshot,
};
