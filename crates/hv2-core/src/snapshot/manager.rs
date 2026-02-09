//! Snapshot manager for complete VM snapshots
//!
//! This module provides the main snapshot management interface
//! for creating, storing, and restoring VM snapshots.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use super::device::{DeviceResult, DeviceStateError, DeviceStateManager, Snapshottable};
use super::memory::{MemoryResult, MemorySnapshotConfig, MemorySnapshotManager};
use super::types::{
    CompressionType, CpuSnapshot, MemoryRegionSnapshot, SnapshotId, SnapshotInfo, SnapshotState,
    SnapshotType, VmSnapshot,
};

/// Snapshot manager result
pub type SnapshotResult<T> = Result<T, SnapshotError>;

/// Snapshot manager error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Snapshot not found
    NotFound(SnapshotId),
    /// Snapshot already exists
    AlreadyExists(SnapshotId),
    /// Invalid snapshot state
    InvalidState(SnapshotState),
    /// IO error
    IoError(String),
    /// Memory snapshot error
    MemoryError(String),
    /// Device state error
    DeviceError(String),
    /// Invalid parent snapshot
    InvalidParent(SnapshotId),
    /// Snapshot in use
    InUse(SnapshotId),
    /// Storage full
    StorageFull,
    /// Invalid configuration
    InvalidConfig(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::NotFound(id) => {
                write!(f, "Snapshot not found: {}", id)
            }
            SnapshotError::AlreadyExists(id) => {
                write!(f, "Snapshot already exists: {}", id)
            }
            SnapshotError::InvalidState(state) => {
                write!(f, "Invalid snapshot state: {:?}", state)
            }
            SnapshotError::IoError(msg) => {
                write!(f, "IO error: {}", msg)
            }
            SnapshotError::MemoryError(msg) => {
                write!(f, "Memory error: {}", msg)
            }
            SnapshotError::DeviceError(msg) => {
                write!(f, "Device error: {}", msg)
            }
            SnapshotError::InvalidParent(id) => {
                write!(f, "Invalid parent snapshot: {}", id)
            }
            SnapshotError::InUse(id) => {
                write!(f, "Snapshot in use: {}", id)
            }
            SnapshotError::StorageFull => write!(f, "Snapshot storage full"),
            SnapshotError::InvalidConfig(msg) => {
                write!(f, "Invalid configuration: {}", msg)
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Snapshot manager configuration
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Storage directory for snapshots
    pub storage_path: PathBuf,
    /// Maximum number of snapshots to keep
    pub max_snapshots: usize,
    /// Maximum total storage size
    pub max_storage_bytes: u64,
    /// Default compression type
    pub compression: CompressionType,
    /// Enable incremental snapshots
    pub incremental_enabled: bool,
    /// Memory snapshot configuration
    pub memory_config: MemorySnapshotConfig,
    /// Auto-delete oldest snapshots when full
    pub auto_cleanup: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("./snapshots"),
            max_snapshots: 100,
            max_storage_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            compression: CompressionType::Lz4,
            incremental_enabled: true,
            memory_config: MemorySnapshotConfig::fast(),
            auto_cleanup: true,
        }
    }
}

impl SnapshotConfig {
    /// Create a new configuration
    pub fn new(storage_path: impl Into<PathBuf>) -> Self {
        Self {
            storage_path: storage_path.into(),
            ..Default::default()
        }
    }

    /// Set maximum snapshots
    pub fn with_max_snapshots(mut self, max: usize) -> Self {
        self.max_snapshots = max;
        self
    }

    /// Set compression type
    pub fn with_compression(mut self, compression: CompressionType) -> Self {
        self.compression = compression;
        self
    }

    /// Enable or disable incremental snapshots
    pub fn with_incremental(mut self, enabled: bool) -> Self {
        self.incremental_enabled = enabled;
        self
    }
}

/// Snapshot creation options
#[derive(Debug, Clone, Default)]
pub struct CreateSnapshotOptions {
    /// Snapshot name
    pub name: Option<String>,
    /// Snapshot description
    pub description: Option<String>,
    /// Snapshot type
    pub snapshot_type: SnapshotType,
    /// Parent snapshot for incremental
    pub parent_id: Option<SnapshotId>,
    /// Tags
    pub tags: Vec<String>,
    /// Force full snapshot even if incremental available
    pub force_full: bool,
}

impl CreateSnapshotOptions {
    /// Create options for a full snapshot
    pub fn full() -> Self {
        Self {
            snapshot_type: SnapshotType::Full,
            ..Default::default()
        }
    }

    /// Create options for an incremental snapshot
    pub fn incremental(parent: SnapshotId) -> Self {
        Self {
            snapshot_type: SnapshotType::Incremental,
            parent_id: Some(parent),
            ..Default::default()
        }
    }

    /// Create options for a memory-only snapshot
    pub fn memory_only() -> Self {
        Self {
            snapshot_type: SnapshotType::MemoryOnly,
            ..Default::default()
        }
    }

    /// Create options for a checkpoint
    pub fn checkpoint() -> Self {
        Self {
            snapshot_type: SnapshotType::Checkpoint,
            ..Default::default()
        }
    }

    /// Set name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

/// Restore snapshot options
#[derive(Debug, Clone, Default)]
pub struct RestoreSnapshotOptions {
    /// Verify checksums during restore
    pub verify_checksums: bool,
    /// Skip device restoration
    pub skip_devices: bool,
    /// Skip memory restoration
    pub skip_memory: bool,
}

/// Snapshot manager
#[derive(Debug)]
pub struct SnapshotManager {
    /// Configuration
    config: SnapshotConfig,
    /// Snapshot catalog (metadata)
    catalog: HashMap<SnapshotId, SnapshotInfo>,
    /// Active snapshot being created/restored
    active_snapshot: Option<SnapshotId>,
    /// Memory manager
    memory_manager: MemorySnapshotManager,
    /// Device manager
    device_manager: DeviceStateManager,
    /// Statistics
    stats: SnapshotManagerStats,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(config: SnapshotConfig) -> Self {
        Self {
            memory_manager: MemorySnapshotManager::with_config(config.memory_config.clone()),
            config,
            catalog: HashMap::new(),
            active_snapshot: None,
            device_manager: DeviceStateManager::new(),
            stats: SnapshotManagerStats::default(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SnapshotConfig::default())
    }

    /// Begin creating a snapshot
    pub fn begin_snapshot(&mut self, options: CreateSnapshotOptions) -> SnapshotResult<SnapshotId> {
        // Check if another snapshot is in progress
        if self.active_snapshot.is_some() {
            return Err(SnapshotError::InvalidState(SnapshotState::Creating));
        }

        // Check storage limits
        if self.catalog.len() >= self.config.max_snapshots {
            if self.config.auto_cleanup {
                self.cleanup_oldest()?;
            } else {
                return Err(SnapshotError::StorageFull);
            }
        }

        // Validate parent for incremental
        if let Some(parent_id) = &options.parent_id {
            if !self.catalog.contains_key(parent_id) {
                return Err(SnapshotError::InvalidParent(*parent_id));
            }
        }

        // Create snapshot info
        let id = SnapshotId::generate();
        let name = options.name.unwrap_or_else(|| format!("Snapshot {}", id));
        let mut info = SnapshotInfo::new(id, &name)
            .with_type(options.snapshot_type)
            .with_description(options.description.unwrap_or_default());

        // Set parent if incremental
        if let Some(parent_id) = options.parent_id {
            info = info.with_parent(parent_id);
        }

        // Add tags
        for tag in options.tags {
            info = info.with_tag(&tag, "");
        }

        self.catalog.insert(id, info);
        self.active_snapshot = Some(id);
        self.stats.snapshots_started += 1;

        Ok(id)
    }

    /// Add CPU state to the current snapshot
    pub fn add_cpu_state(&mut self, cpu: CpuSnapshot) -> SnapshotResult<()> {
        let id = self
            .active_snapshot
            .ok_or(SnapshotError::InvalidState(SnapshotState::Invalid))?;

        if let Some(info) = self.catalog.get_mut(&id) {
            info.vcpu_count += 1;
        }

        Ok(())
    }

    /// Add memory region to the current snapshot
    pub fn add_memory_region(&mut self, region: MemoryRegionSnapshot) -> SnapshotResult<()> {
        let id = self
            .active_snapshot
            .ok_or(SnapshotError::InvalidState(SnapshotState::Invalid))?;

        if let Some(info) = self.catalog.get_mut(&id) {
            info.size_bytes += region.size;
        }

        Ok(())
    }

    /// Add device state to the current snapshot
    pub fn add_device(&mut self, device: &dyn Snapshottable) -> SnapshotResult<()> {
        let _id = self
            .active_snapshot
            .ok_or(SnapshotError::InvalidState(SnapshotState::Invalid))?;

        self.device_manager
            .capture_device(device)
            .map_err(|e| SnapshotError::DeviceError(e.to_string()))?;

        Ok(())
    }

    /// Complete the current snapshot
    pub fn complete_snapshot(&mut self) -> SnapshotResult<SnapshotId> {
        let id = self
            .active_snapshot
            .take()
            .ok_or(SnapshotError::InvalidState(SnapshotState::Invalid))?;

        if let Some(info) = self.catalog.get_mut(&id) {
            info.state = SnapshotState::Valid;
        }

        self.stats.snapshots_completed += 1;
        Ok(id)
    }

    /// Abort the current snapshot
    pub fn abort_snapshot(&mut self) -> SnapshotResult<()> {
        if let Some(id) = self.active_snapshot.take() {
            self.catalog.remove(&id);
            self.stats.snapshots_failed += 1;
        }
        Ok(())
    }

    /// Get snapshot info
    pub fn get_snapshot(&self, id: &SnapshotId) -> Option<&SnapshotInfo> {
        self.catalog.get(id)
    }

    /// List all snapshots
    pub fn list_snapshots(&self) -> Vec<&SnapshotInfo> {
        self.catalog.values().collect()
    }

    /// Delete a snapshot
    pub fn delete_snapshot(&mut self, id: &SnapshotId) -> SnapshotResult<()> {
        // Check if snapshot exists
        if !self.catalog.contains_key(id) {
            return Err(SnapshotError::NotFound(*id));
        }

        // Check if snapshot is in use (being restored)
        if self.active_snapshot.as_ref() == Some(id) {
            return Err(SnapshotError::InUse(*id));
        }

        // Check if snapshot is parent of other snapshots
        for info in self.catalog.values() {
            if info.parent_id.as_ref() == Some(id) {
                return Err(SnapshotError::InUse(*id));
            }
        }

        self.catalog.remove(id);
        self.stats.snapshots_deleted += 1;

        Ok(())
    }

    /// Begin restoring from a snapshot
    pub fn begin_restore(&mut self, id: &SnapshotId) -> SnapshotResult<&SnapshotInfo> {
        // Check if another operation is in progress
        if self.active_snapshot.is_some() {
            return Err(SnapshotError::InvalidState(SnapshotState::Restoring));
        }

        let info = self.catalog.get(id).ok_or(SnapshotError::NotFound(*id))?;

        // Check snapshot state
        if info.state != SnapshotState::Valid {
            return Err(SnapshotError::InvalidState(info.state));
        }

        self.active_snapshot = Some(*id);
        self.stats.restores_started += 1;

        Ok(info)
    }

    /// Complete the restore operation
    pub fn complete_restore(&mut self) -> SnapshotResult<()> {
        self.active_snapshot
            .take()
            .ok_or(SnapshotError::InvalidState(SnapshotState::Invalid))?;

        self.stats.restores_completed += 1;
        Ok(())
    }

    /// Abort the restore operation
    pub fn abort_restore(&mut self) -> SnapshotResult<()> {
        self.active_snapshot.take();
        self.stats.restores_failed += 1;
        Ok(())
    }

    /// Get total storage used
    pub fn storage_used(&self) -> u64 {
        self.catalog.values().map(|i| i.size_bytes).sum()
    }

    /// Get number of snapshots
    pub fn snapshot_count(&self) -> usize {
        self.catalog.len()
    }

    /// Get statistics
    pub fn stats(&self) -> &SnapshotManagerStats {
        &self.stats
    }

    /// Get configuration
    pub fn config(&self) -> &SnapshotConfig {
        &self.config
    }

    /// Cleanup oldest snapshots
    fn cleanup_oldest(&mut self) -> SnapshotResult<()> {
        // Find oldest snapshot without children
        let oldest = self
            .catalog
            .iter()
            .filter(|(id, info)| {
                info.state == SnapshotState::Valid
                    && !self
                        .catalog
                        .values()
                        .any(|i| i.parent_id.as_ref() == Some(*id))
            })
            .min_by_key(|(_, info)| info.created_at)
            .map(|(id, _)| *id);

        if let Some(id) = oldest {
            self.delete_snapshot(&id)?;
        }

        Ok(())
    }

    /// Find snapshots by tag
    pub fn find_by_tag(&self, tag: &str) -> Vec<&SnapshotInfo> {
        self.catalog
            .values()
            .filter(|info| info.tags.contains_key(tag))
            .collect()
    }

    /// Find snapshots by name pattern
    pub fn find_by_name(&self, pattern: &str) -> Vec<&SnapshotInfo> {
        self.catalog
            .values()
            .filter(|info| info.name.contains(pattern))
            .collect()
    }

    /// Get snapshot chain (for incremental)
    pub fn get_chain(&self, id: &SnapshotId) -> Vec<&SnapshotInfo> {
        let mut chain = Vec::new();
        let mut current_id = Some(*id);

        while let Some(id) = current_id {
            if let Some(info) = self.catalog.get(&id) {
                chain.push(info);
                current_id = info.parent_id;
            } else {
                break;
            }
        }

        chain.reverse();
        chain
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Snapshot manager statistics
#[derive(Debug, Clone, Default)]
pub struct SnapshotManagerStats {
    /// Snapshots started
    pub snapshots_started: u64,
    /// Snapshots completed
    pub snapshots_completed: u64,
    /// Snapshots failed
    pub snapshots_failed: u64,
    /// Snapshots deleted
    pub snapshots_deleted: u64,
    /// Restores started
    pub restores_started: u64,
    /// Restores completed
    pub restores_completed: u64,
    /// Restores failed
    pub restores_failed: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// Total bytes read
    pub bytes_read: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_error_display() {
        let id = SnapshotId::generate();
        let err = SnapshotError::NotFound(id);
        assert!(format!("{}", err).contains(&id.to_string()));
    }

    #[test]
    fn test_snapshot_config_default() {
        let config = SnapshotConfig::default();
        assert_eq!(config.max_snapshots, 100);
        assert!(config.incremental_enabled);
    }

    #[test]
    fn test_snapshot_config_builder() {
        let config = SnapshotConfig::new("/tmp/snapshots")
            .with_max_snapshots(50)
            .with_compression(CompressionType::Zstd)
            .with_incremental(false);

        assert_eq!(config.max_snapshots, 50);
        assert_eq!(config.compression, CompressionType::Zstd);
        assert!(!config.incremental_enabled);
    }

    #[test]
    fn test_create_snapshot_options() {
        let opts = CreateSnapshotOptions::full()
            .with_name("test")
            .with_description("test description")
            .with_tag("tag1");

        assert_eq!(opts.name, Some("test".to_string()));
        assert_eq!(opts.description, Some("test description".to_string()));
        assert_eq!(opts.tags, vec!["tag1"]);
    }

    #[test]
    fn test_snapshot_manager_begin_complete() {
        let mut manager = SnapshotManager::with_defaults();

        let id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();

        let completed_id = manager.complete_snapshot().unwrap();
        assert_eq!(id, completed_id);

        let info = manager.get_snapshot(&id).unwrap();
        assert_eq!(info.state, SnapshotState::Valid);
    }

    #[test]
    fn test_snapshot_manager_abort() {
        let mut manager = SnapshotManager::with_defaults();

        let id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();

        manager.abort_snapshot().unwrap();
        assert!(manager.get_snapshot(&id).is_none());
    }

    #[test]
    fn test_snapshot_manager_double_begin() {
        let mut manager = SnapshotManager::with_defaults();

        manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();

        let result = manager.begin_snapshot(CreateSnapshotOptions::full());
        assert!(matches!(result, Err(SnapshotError::InvalidState(_))));
    }

    #[test]
    fn test_snapshot_manager_delete() {
        let mut manager = SnapshotManager::with_defaults();

        let id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.complete_snapshot().unwrap();

        manager.delete_snapshot(&id).unwrap();
        assert!(manager.get_snapshot(&id).is_none());
    }

    #[test]
    fn test_snapshot_manager_delete_not_found() {
        let mut manager = SnapshotManager::with_defaults();

        let id = SnapshotId::generate();
        let result = manager.delete_snapshot(&id);
        assert!(matches!(result, Err(SnapshotError::NotFound(_))));
    }

    #[test]
    fn test_snapshot_manager_list() {
        let mut manager = SnapshotManager::with_defaults();

        let id1 = manager
            .begin_snapshot(CreateSnapshotOptions::full().with_name("snap1"))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let id2 = manager
            .begin_snapshot(CreateSnapshotOptions::full().with_name("snap2"))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let snapshots = manager.list_snapshots();
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn test_snapshot_manager_incremental() {
        let mut manager = SnapshotManager::with_defaults();

        let parent_id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.complete_snapshot().unwrap();

        let child_id = manager
            .begin_snapshot(CreateSnapshotOptions::incremental(parent_id))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let child_info = manager.get_snapshot(&child_id).unwrap();
        assert_eq!(child_info.parent_id, Some(parent_id));
    }

    #[test]
    fn test_snapshot_manager_invalid_parent() {
        let mut manager = SnapshotManager::with_defaults();

        let fake_parent = SnapshotId::generate();
        let result = manager.begin_snapshot(CreateSnapshotOptions::incremental(fake_parent));

        assert!(matches!(result, Err(SnapshotError::InvalidParent(_))));
    }

    #[test]
    fn test_snapshot_manager_restore() {
        let mut manager = SnapshotManager::with_defaults();

        let id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.complete_snapshot().unwrap();

        let info = manager.begin_restore(&id).unwrap();
        assert_eq!(info.id, id);

        manager.complete_restore().unwrap();
        assert_eq!(manager.stats().restores_completed, 1);
    }

    #[test]
    fn test_snapshot_manager_restore_not_found() {
        let mut manager = SnapshotManager::with_defaults();

        let id = SnapshotId::generate();
        let result = manager.begin_restore(&id);

        assert!(matches!(result, Err(SnapshotError::NotFound(_))));
    }

    #[test]
    fn test_snapshot_manager_find_by_tag() {
        let mut manager = SnapshotManager::with_defaults();

        manager
            .begin_snapshot(CreateSnapshotOptions::full().with_tag("important"))
            .unwrap();
        manager.complete_snapshot().unwrap();

        manager
            .begin_snapshot(CreateSnapshotOptions::full().with_tag("temp"))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let found = manager.find_by_tag("important");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_snapshot_manager_find_by_name() {
        let mut manager = SnapshotManager::with_defaults();

        manager
            .begin_snapshot(CreateSnapshotOptions::full().with_name("backup-daily"))
            .unwrap();
        manager.complete_snapshot().unwrap();

        manager
            .begin_snapshot(CreateSnapshotOptions::full().with_name("backup-weekly"))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let found = manager.find_by_name("backup");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn test_snapshot_manager_get_chain() {
        let mut manager = SnapshotManager::with_defaults();

        let base_id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.complete_snapshot().unwrap();

        let inc1_id = manager
            .begin_snapshot(CreateSnapshotOptions::incremental(base_id))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let inc2_id = manager
            .begin_snapshot(CreateSnapshotOptions::incremental(inc1_id))
            .unwrap();
        manager.complete_snapshot().unwrap();

        let chain = manager.get_chain(&inc2_id);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].id, base_id);
        assert_eq!(chain[1].id, inc1_id);
        assert_eq!(chain[2].id, inc2_id);
    }

    #[test]
    fn test_snapshot_manager_delete_with_children() {
        let mut manager = SnapshotManager::with_defaults();

        let parent_id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.complete_snapshot().unwrap();

        manager
            .begin_snapshot(CreateSnapshotOptions::incremental(parent_id))
            .unwrap();
        manager.complete_snapshot().unwrap();

        // Cannot delete parent with children
        let result = manager.delete_snapshot(&parent_id);
        assert!(matches!(result, Err(SnapshotError::InUse(_))));
    }

    #[test]
    fn test_snapshot_manager_storage_used() {
        let mut manager = SnapshotManager::with_defaults();

        let id = manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();

        manager
            .add_memory_region(MemoryRegionSnapshot {
                gpa_start: 0,
                size: 1000,
                file_offset: 0,
                compressed_size: Some(800),
                checksum: 0,
                is_dirty: false,
                compression: CompressionType::Lz4,
            })
            .unwrap();

        manager.complete_snapshot().unwrap();

        assert_eq!(manager.storage_used(), 1000);
    }

    #[test]
    fn test_snapshot_manager_stats() {
        let mut manager = SnapshotManager::with_defaults();

        manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.complete_snapshot().unwrap();

        manager
            .begin_snapshot(CreateSnapshotOptions::full())
            .unwrap();
        manager.abort_snapshot().unwrap();

        assert_eq!(manager.stats().snapshots_started, 2);
        assert_eq!(manager.stats().snapshots_completed, 1);
        assert_eq!(manager.stats().snapshots_failed, 1);
    }
}
