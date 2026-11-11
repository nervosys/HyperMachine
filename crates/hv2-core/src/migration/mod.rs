//! Live migration infrastructure
//!
//! Building blocks for live migration: dirty page tracking, state serialization,
//! and a protocol model. There is no transport, so live migration itself is not
//! implemented and the protocol controller refuses to start.

pub mod dirty_tracking;
pub mod protocol;
pub mod state;

pub use dirty_tracking::{
    shared_dirty_tracker, DirtyBitmap, DirtyPageIterator, DirtyStats, DirtyTracker,
    SharedDirtyTracker, PAGE_SIZE,
};

pub use state::{
    crc32, CpuState, DescriptorTable, DeviceState, MemoryRegionState, Migratable, SectionHeader,
    SectionType, SegmentRegister, SerializeError, SerializeResult, StateDeserializer,
    StateSerializer, VmState, FORMAT_VERSION, STATE_MAGIC,
};

pub use protocol::{
    MigrationConfig, MigrationController, MigrationMessage, MigrationRole, MigrationStage,
    MigrationStats, MigrationStream, PageData, PreCopyMigration, ProgressCallback,
};
