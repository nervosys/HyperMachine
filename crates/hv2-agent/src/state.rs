//! Agent State Management - Persistent state storage for AI agents
//!
//! This module provides state management primitives for AI agents:
//! - Key-value state storage
//! - State versioning and history
//! - State snapshots and checkpoints
//! - State synchronization across agents

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// Result type for state operations
pub type StateResult<T> = Result<T, StateError>;

/// State operation errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    /// Key not found
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    /// Version conflict
    #[error("Version conflict: expected {expected}, actual {actual}")]
    VersionConflict { expected: u64, actual: u64 },
    /// State is locked
    #[error("State locked: {0}")]
    StateLocked(String),
    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
    /// Capacity exceeded
    #[error("Capacity exceeded: {0}")]
    CapacityExceeded(String),
    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    /// Checkpoint not found
    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(String),
}

/// State value with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateValue {
    /// The serialized value data
    pub data: Vec<u8>,
    /// Content type hint
    pub content_type: String,
    /// Version number
    pub version: u64,
    /// Creation timestamp
    pub created_at: SystemTime,
    /// Last modified timestamp
    pub modified_at: SystemTime,
    /// Time-to-live (optional)
    pub ttl: Option<Duration>,
    /// Custom tags
    pub tags: HashMap<String, String>,
}

impl StateValue {
    /// Create a new state value from bytes
    pub fn from_bytes(data: Vec<u8>) -> Self {
        let now = SystemTime::now();
        Self {
            data,
            content_type: "application/octet-stream".to_string(),
            version: 1,
            created_at: now,
            modified_at: now,
            ttl: None,
            tags: HashMap::new(),
        }
    }

    /// Create a state value from a JSON-serializable type
    pub fn from_json<T: Serialize>(value: &T) -> StateResult<Self> {
        let data =
            serde_json::to_vec(value).map_err(|e| StateError::SerializationError(e.to_string()))?;
        let now = SystemTime::now();
        Ok(Self {
            data,
            content_type: "application/json".to_string(),
            version: 1,
            created_at: now,
            modified_at: now,
            ttl: None,
            tags: HashMap::new(),
        })
    }

    /// Create a state value from a string
    pub fn from_string(s: impl Into<String>) -> Self {
        let now = SystemTime::now();
        Self {
            data: s.into().into_bytes(),
            content_type: "text/plain".to_string(),
            version: 1,
            created_at: now,
            modified_at: now,
            ttl: None,
            tags: HashMap::new(),
        }
    }

    /// Deserialize as JSON
    pub fn as_json<T: for<'de> Deserialize<'de>>(&self) -> StateResult<T> {
        serde_json::from_slice(&self.data)
            .map_err(|e| StateError::SerializationError(e.to_string()))
    }

    /// Get as string
    pub fn as_string(&self) -> StateResult<String> {
        String::from_utf8(self.data.clone())
            .map_err(|e| StateError::SerializationError(e.to_string()))
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Get size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Check if expired
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            if let Ok(elapsed) = self.modified_at.elapsed() {
                return elapsed > ttl;
            }
        }
        false
    }

    /// Set TTL
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Update the value data
    fn update(&mut self, data: Vec<u8>) {
        self.data = data;
        self.version += 1;
        self.modified_at = SystemTime::now();
    }
}

/// A versioned state entry for history tracking
#[derive(Debug, Clone)]
struct StateEntry {
    value: StateValue,
    previous_version: Option<u64>,
}

/// State store with versioning and history
#[derive(Debug)]
pub struct StateStore {
    /// Current state values
    data: BTreeMap<String, StateValue>,
    /// Version history per key
    history: HashMap<String, VecDeque<StateEntry>>,
    /// Maximum history entries per key
    max_history: usize,
    /// Maximum total size in bytes
    max_size: usize,
    /// Current total size
    current_size: usize,
    /// Global version counter
    global_version: u64,
    /// Store name
    name: String,
    /// Creation time
    created_at: Instant,
}

impl StateStore {
    /// Create a new state store
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            data: BTreeMap::new(),
            history: HashMap::new(),
            max_history: 100,
            max_size: 100 * 1024 * 1024, // 100 MB default
            current_size: 0,
            global_version: 0,
            name: name.into(),
            created_at: Instant::now(),
        }
    }

    /// Set maximum history entries per key
    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    /// Set maximum total size
    pub fn with_max_size(mut self, max: usize) -> Self {
        self.max_size = max;
        self
    }

    /// Get the store name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get global version
    pub fn version(&self) -> u64 {
        self.global_version
    }

    /// Get current size in bytes
    pub fn size(&self) -> usize {
        self.current_size
    }

    /// Get number of keys
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get a value by key
    pub fn get(&self, key: &str) -> Option<&StateValue> {
        self.data.get(key).filter(|v| !v.is_expired())
    }

    /// Check if key exists
    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Set a value
    pub fn set(&mut self, key: impl Into<String>, value: StateValue) -> StateResult<()> {
        let key = key.into();
        let new_size = value.size();

        // Check capacity
        let old_size = self.data.get(&key).map(|v| v.size()).unwrap_or(0);
        let size_delta = new_size as i64 - old_size as i64;

        if size_delta > 0 && self.current_size + size_delta as usize > self.max_size {
            return Err(StateError::CapacityExceeded(format!(
                "Would exceed max size of {} bytes",
                self.max_size
            )));
        }

        // Store old value in history
        if let Some(old_value) = self.data.get(&key) {
            let entry = StateEntry {
                value: old_value.clone(),
                previous_version: Some(self.global_version),
            };

            let history = self.history.entry(key.clone()).or_default();
            history.push_back(entry);

            // Trim history if needed
            while history.len() > self.max_history {
                history.pop_front();
            }
        }

        // Update size tracking
        self.current_size = (self.current_size as i64 + size_delta) as usize;
        self.global_version += 1;

        self.data.insert(key, value);
        Ok(())
    }

    /// Set with optimistic locking (version check)
    pub fn set_if_version(
        &mut self,
        key: impl Into<String>,
        value: StateValue,
        expected_version: u64,
    ) -> StateResult<()> {
        let key = key.into();

        if let Some(current) = self.data.get(&key) {
            if current.version != expected_version {
                return Err(StateError::VersionConflict {
                    expected: expected_version,
                    actual: current.version,
                });
            }
        } else if expected_version != 0 {
            return Err(StateError::VersionConflict {
                expected: expected_version,
                actual: 0,
            });
        }

        self.set(key, value)
    }

    /// Delete a key
    pub fn delete(&mut self, key: &str) -> StateResult<StateValue> {
        let value = self
            .data
            .remove(key)
            .ok_or_else(|| StateError::KeyNotFound(key.to_string()))?;

        self.current_size -= value.size();
        self.global_version += 1;

        // Keep history of deleted value
        let entry = StateEntry {
            value: value.clone(),
            previous_version: Some(self.global_version - 1),
        };

        let history = self.history.entry(key.to_string()).or_default();
        history.push_back(entry);

        while history.len() > self.max_history {
            history.pop_front();
        }

        Ok(value)
    }

    /// Get history for a key
    pub fn get_history(&self, key: &str) -> Vec<&StateValue> {
        self.history
            .get(key)
            .map(|h| h.iter().map(|e| &e.value).collect())
            .unwrap_or_default()
    }

    /// List all keys
    pub fn keys(&self) -> Vec<&str> {
        self.data.keys().map(|k| k.as_str()).collect()
    }

    /// List keys matching a prefix
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<&str> {
        self.data
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .filter(|(_, v)| !v.is_expired())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Clear all expired entries
    pub fn cleanup_expired(&mut self) -> usize {
        let expired_keys: Vec<String> = self
            .data
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        let count = expired_keys.len();

        for key in expired_keys {
            if let Some(value) = self.data.remove(&key) {
                self.current_size -= value.size();
            }
        }

        count
    }

    /// Clear all data
    pub fn clear(&mut self) {
        self.data.clear();
        self.history.clear();
        self.current_size = 0;
        self.global_version += 1;
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// A checkpoint of state store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCheckpoint {
    /// Checkpoint ID
    pub id: String,
    /// Checkpoint name
    pub name: String,
    /// Store name
    pub store_name: String,
    /// Global version at checkpoint time
    pub version: u64,
    /// Serialized state data
    pub data: Vec<(String, StateValue)>,
    /// Creation timestamp
    pub created_at: SystemTime,
    /// Description
    pub description: String,
}

impl StateCheckpoint {
    /// Get size in bytes
    pub fn size(&self) -> usize {
        self.data.iter().map(|(k, v)| k.len() + v.size()).sum()
    }

    /// Get number of keys
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Checkpoint manager for state stores
#[derive(Debug, Default)]
pub struct CheckpointManager {
    /// Stored checkpoints
    checkpoints: HashMap<String, StateCheckpoint>,
    /// Maximum checkpoints to keep
    max_checkpoints: usize,
}

impl CheckpointManager {
    /// Create a new checkpoint manager
    pub fn new() -> Self {
        Self {
            checkpoints: HashMap::new(),
            max_checkpoints: 100,
        }
    }

    /// Set maximum checkpoints
    pub fn with_max_checkpoints(mut self, max: usize) -> Self {
        self.max_checkpoints = max;
        self
    }

    /// Create a checkpoint from a state store
    pub fn create_checkpoint(
        &mut self,
        store: &StateStore,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> StateResult<String> {
        if self.checkpoints.len() >= self.max_checkpoints {
            return Err(StateError::CapacityExceeded(format!(
                "Maximum {} checkpoints reached",
                self.max_checkpoints
            )));
        }

        let id = format!("cp-{}", store.version());
        let checkpoint = StateCheckpoint {
            id: id.clone(),
            name: name.into(),
            store_name: store.name().to_string(),
            version: store.version(),
            data: store
                .data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            created_at: SystemTime::now(),
            description: description.into(),
        };

        self.checkpoints.insert(id.clone(), checkpoint);
        Ok(id)
    }

    /// Get a checkpoint by ID
    pub fn get_checkpoint(&self, id: &str) -> Option<&StateCheckpoint> {
        self.checkpoints.get(id)
    }

    /// Delete a checkpoint
    pub fn delete_checkpoint(&mut self, id: &str) -> Option<StateCheckpoint> {
        self.checkpoints.remove(id)
    }

    /// List all checkpoints
    pub fn list_checkpoints(&self) -> Vec<&StateCheckpoint> {
        self.checkpoints.values().collect()
    }

    /// Restore a checkpoint to a state store
    pub fn restore_checkpoint(&self, id: &str, store: &mut StateStore) -> StateResult<()> {
        let checkpoint = self
            .checkpoints
            .get(id)
            .ok_or_else(|| StateError::CheckpointNotFound(id.to_string()))?;

        // Clear current data
        store.clear();

        // Restore from checkpoint
        for (key, value) in &checkpoint.data {
            store.set(key.clone(), value.clone())?;
        }

        Ok(())
    }

    /// Get checkpoint count
    pub fn len(&self) -> usize {
        self.checkpoints.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }
}

/// Thread-safe state store wrapper
#[derive(Debug, Clone)]
pub struct SharedStateStore {
    inner: Arc<RwLock<StateStore>>,
}

impl SharedStateStore {
    /// Create a new shared state store
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StateStore::new(name))),
        }
    }

    /// Get a value
    pub fn get(&self, key: &str) -> Option<StateValue> {
        self.inner.read().ok()?.get(key).cloned()
    }

    /// Set a value
    pub fn set(&self, key: impl Into<String>, value: StateValue) -> StateResult<()> {
        self.inner
            .write()
            .map_err(|_| StateError::StateLocked("Failed to acquire write lock".into()))?
            .set(key, value)
    }

    /// Delete a value
    pub fn delete(&self, key: &str) -> StateResult<StateValue> {
        self.inner
            .write()
            .map_err(|_| StateError::StateLocked("Failed to acquire write lock".into()))?
            .delete(key)
    }

    /// Check if key exists
    pub fn contains(&self, key: &str) -> bool {
        self.inner
            .read()
            .ok()
            .map(|s| s.contains(key))
            .unwrap_or(false)
    }

    /// Get number of keys
    pub fn len(&self) -> usize {
        self.inner.read().ok().map(|s| s.len()).unwrap_or(0)
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get global version
    pub fn version(&self) -> u64 {
        self.inner.read().ok().map(|s| s.version()).unwrap_or(0)
    }

    /// List keys
    pub fn keys(&self) -> Vec<String> {
        self.inner
            .read()
            .ok()
            .map(|s| s.keys().iter().map(|k| k.to_string()).collect())
            .unwrap_or_default()
    }
}

/// State synchronization direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// Push local changes to remote
    Push,
    /// Pull remote changes to local
    Pull,
    /// Bidirectional sync
    Bidirectional,
}

/// State synchronization result
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Number of keys pushed
    pub pushed: usize,
    /// Number of keys pulled
    pub pulled: usize,
    /// Number of conflicts
    pub conflicts: usize,
    /// Conflict keys
    pub conflict_keys: Vec<String>,
    /// Duration of sync
    pub duration: Duration,
}

impl SyncResult {
    /// Create a new sync result
    pub fn new() -> Self {
        Self {
            pushed: 0,
            pulled: 0,
            conflicts: 0,
            conflict_keys: Vec::new(),
            duration: Duration::ZERO,
        }
    }

    /// Check if sync was successful (no conflicts)
    pub fn is_success(&self) -> bool {
        self.conflicts == 0
    }

    /// Get total changes
    pub fn total_changes(&self) -> usize {
        self.pushed + self.pulled
    }
}

impl Default for SyncResult {
    fn default() -> Self {
        Self::new()
    }
}

/// State synchronizer between stores
#[derive(Debug)]
pub struct StateSynchronizer {
    /// Conflict resolution strategy
    conflict_strategy: ConflictStrategy,
    /// Sync statistics
    total_syncs: u64,
    /// Last sync time
    last_sync: Option<Instant>,
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Local version wins
    LocalWins,
    /// Remote version wins
    RemoteWins,
    /// Newer version wins (by modified_at)
    NewerWins,
    /// Higher version number wins
    HigherVersionWins,
}

impl Default for StateSynchronizer {
    fn default() -> Self {
        Self::new()
    }
}

impl StateSynchronizer {
    /// Create a new synchronizer
    pub fn new() -> Self {
        Self {
            conflict_strategy: ConflictStrategy::NewerWins,
            total_syncs: 0,
            last_sync: None,
        }
    }

    /// Set conflict resolution strategy
    pub fn with_conflict_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.conflict_strategy = strategy;
        self
    }

    /// Synchronize two state stores
    pub fn sync(
        &mut self,
        local: &mut StateStore,
        remote: &mut StateStore,
        direction: SyncDirection,
    ) -> SyncResult {
        let start = Instant::now();
        let mut result = SyncResult::new();

        match direction {
            SyncDirection::Push => {
                self.push_changes(local, remote, &mut result);
            }
            SyncDirection::Pull => {
                self.pull_changes(local, remote, &mut result);
            }
            SyncDirection::Bidirectional => {
                self.push_changes(local, remote, &mut result);
                self.pull_changes(local, remote, &mut result);
            }
        }

        result.duration = start.elapsed();
        self.total_syncs += 1;
        self.last_sync = Some(Instant::now());

        result
    }

    /// Push local changes to remote
    fn push_changes(&self, local: &StateStore, remote: &mut StateStore, result: &mut SyncResult) {
        for key in local.keys() {
            if let Some(local_value) = local.get(key) {
                if let Some(remote_value) = remote.get(key) {
                    // Conflict - resolve based on strategy
                    if self.should_overwrite(local_value, remote_value) {
                        if remote.set(key.to_string(), local_value.clone()).is_ok() {
                            result.pushed += 1;
                        }
                    } else {
                        result.conflicts += 1;
                        result.conflict_keys.push(key.to_string());
                    }
                } else {
                    // No conflict - push new value
                    if remote.set(key.to_string(), local_value.clone()).is_ok() {
                        result.pushed += 1;
                    }
                }
            }
        }
    }

    /// Pull remote changes to local
    fn pull_changes(&self, local: &mut StateStore, remote: &StateStore, result: &mut SyncResult) {
        for key in remote.keys() {
            if let Some(remote_value) = remote.get(key) {
                if let Some(local_value) = local.get(key) {
                    // Conflict - resolve based on strategy
                    if self.should_overwrite(remote_value, local_value) {
                        if local.set(key.to_string(), remote_value.clone()).is_ok() {
                            result.pulled += 1;
                        }
                    } else {
                        // Don't double-count conflicts from push
                        if !result.conflict_keys.contains(&key.to_string()) {
                            result.conflicts += 1;
                            result.conflict_keys.push(key.to_string());
                        }
                    }
                } else {
                    // No conflict - pull new value
                    if local.set(key.to_string(), remote_value.clone()).is_ok() {
                        result.pulled += 1;
                    }
                }
            }
        }
    }

    /// Determine if source should overwrite target based on conflict strategy
    fn should_overwrite(&self, source: &StateValue, target: &StateValue) -> bool {
        match self.conflict_strategy {
            ConflictStrategy::LocalWins => true,
            ConflictStrategy::RemoteWins => false,
            ConflictStrategy::NewerWins => source.modified_at > target.modified_at,
            ConflictStrategy::HigherVersionWins => source.version > target.version,
        }
    }

    /// Get total sync count
    pub fn total_syncs(&self) -> u64 {
        self.total_syncs
    }

    /// Get time since last sync
    pub fn time_since_last_sync(&self) -> Option<Duration> {
        self.last_sync.map(|t| t.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_error_display() {
        let err = StateError::KeyNotFound("test-key".into());
        assert!(err.to_string().contains("Key not found"));

        let err = StateError::VersionConflict {
            expected: 5,
            actual: 3,
        };
        assert!(err.to_string().contains("Version conflict"));
    }

    #[test]
    fn test_state_value_from_bytes() {
        let value = StateValue::from_bytes(vec![1, 2, 3, 4]);
        assert_eq!(value.as_bytes(), &[1, 2, 3, 4]);
        assert_eq!(value.size(), 4);
        assert_eq!(value.version, 1);
    }

    #[test]
    fn test_state_value_from_json() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestData {
            value: i32,
        }

        let data = TestData { value: 42 };
        let value = StateValue::from_json(&data).unwrap();
        assert_eq!(value.content_type, "application/json");

        let decoded: TestData = value.as_json().unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_state_value_from_string() {
        let value = StateValue::from_string("hello world");
        assert_eq!(value.as_string().unwrap(), "hello world");
        assert_eq!(value.content_type, "text/plain");
    }

    #[test]
    fn test_state_value_ttl() {
        let value = StateValue::from_string("test").with_ttl(Duration::from_millis(1));

        assert!(!value.is_expired());
        std::thread::sleep(Duration::from_millis(10));
        assert!(value.is_expired());
    }

    #[test]
    fn test_state_value_tags() {
        let value = StateValue::from_string("test")
            .with_tag("env", "production")
            .with_tag("region", "us-east-1");

        assert_eq!(value.tags.get("env"), Some(&"production".to_string()));
        assert_eq!(value.tags.get("region"), Some(&"us-east-1".to_string()));
    }

    #[test]
    fn test_state_store_basic_operations() {
        let mut store = StateStore::new("test-store");

        assert!(store.is_empty());

        store
            .set("key1", StateValue::from_string("value1"))
            .unwrap();
        store
            .set("key2", StateValue::from_string("value2"))
            .unwrap();

        assert_eq!(store.len(), 2);
        assert!(store.contains("key1"));
        assert!(!store.contains("key3"));

        let value = store.get("key1").unwrap();
        assert_eq!(value.as_string().unwrap(), "value1");
    }

    #[test]
    fn test_state_store_versioning() {
        let mut store = StateStore::new("test-store");

        store.set("key1", StateValue::from_string("v1")).unwrap();
        assert_eq!(store.version(), 1);

        store.set("key1", StateValue::from_string("v2")).unwrap();
        assert_eq!(store.version(), 2);

        let value = store.get("key1").unwrap();
        assert_eq!(value.version, 1); // Value's own version
    }

    #[test]
    fn test_state_store_history() {
        let mut store = StateStore::new("test-store").with_max_history(10);

        store.set("key1", StateValue::from_string("v1")).unwrap();
        store.set("key1", StateValue::from_string("v2")).unwrap();
        store.set("key1", StateValue::from_string("v3")).unwrap();

        let history = store.get_history("key1");
        assert_eq!(history.len(), 2); // v1 and v2 in history

        assert_eq!(history[0].as_string().unwrap(), "v1");
        assert_eq!(history[1].as_string().unwrap(), "v2");
    }

    #[test]
    fn test_state_store_optimistic_locking() {
        let mut store = StateStore::new("test-store");

        // Set initial value (version will be 1)
        store.set("key1", StateValue::from_string("v1")).unwrap();
        let initial_version = store.get("key1").unwrap().version;

        // Update with correct version
        let result = store.set_if_version("key1", StateValue::from_string("v2"), initial_version);
        assert!(result.is_ok());

        // The stored value still has version 1 (value's own version, not store's global)
        // Update with wrong version should fail
        let current_version = store.get("key1").unwrap().version;
        let result =
            store.set_if_version("key1", StateValue::from_string("v3"), current_version + 100);
        assert!(matches!(result, Err(StateError::VersionConflict { .. })));
    }

    #[test]
    fn test_state_store_delete() {
        let mut store = StateStore::new("test-store");

        store
            .set("key1", StateValue::from_string("value1"))
            .unwrap();
        assert!(store.contains("key1"));

        let deleted = store.delete("key1").unwrap();
        assert_eq!(deleted.as_string().unwrap(), "value1");
        assert!(!store.contains("key1"));

        // Delete non-existent key should fail
        let result = store.delete("key1");
        assert!(matches!(result, Err(StateError::KeyNotFound(_))));
    }

    #[test]
    fn test_state_store_prefix_search() {
        let mut store = StateStore::new("test-store");

        store
            .set("users/alice", StateValue::from_string("Alice"))
            .unwrap();
        store
            .set("users/bob", StateValue::from_string("Bob"))
            .unwrap();
        store
            .set("items/item1", StateValue::from_string("Item 1"))
            .unwrap();

        let user_keys = store.keys_with_prefix("users/");
        assert_eq!(user_keys.len(), 2);
        assert!(user_keys.contains(&"users/alice"));
        assert!(user_keys.contains(&"users/bob"));
    }

    #[test]
    fn test_state_store_capacity() {
        let mut store = StateStore::new("test-store").with_max_size(100);

        // This should succeed
        store
            .set("small", StateValue::from_bytes(vec![0; 50]))
            .unwrap();

        // This should fail (would exceed capacity)
        let result = store.set("large", StateValue::from_bytes(vec![0; 100]));
        assert!(matches!(result, Err(StateError::CapacityExceeded(_))));
    }

    #[test]
    fn test_state_store_cleanup_expired() {
        let mut store = StateStore::new("test-store");

        store
            .set(
                "expires",
                StateValue::from_string("temp").with_ttl(Duration::from_millis(1)),
            )
            .unwrap();
        store
            .set("stays", StateValue::from_string("permanent"))
            .unwrap();

        std::thread::sleep(Duration::from_millis(10));

        let cleaned = store.cleanup_expired();
        assert_eq!(cleaned, 1);
        assert!(!store.contains("expires"));
        assert!(store.contains("stays"));
    }

    #[test]
    fn test_checkpoint_create_restore() {
        let mut store = StateStore::new("test-store");
        let mut manager = CheckpointManager::new();

        store
            .set("key1", StateValue::from_string("value1"))
            .unwrap();
        store
            .set("key2", StateValue::from_string("value2"))
            .unwrap();

        let cp_id = manager
            .create_checkpoint(&store, "checkpoint-1", "Test checkpoint")
            .unwrap();

        // Modify store
        store
            .set("key1", StateValue::from_string("modified"))
            .unwrap();
        store.delete("key2").unwrap();

        // Restore from checkpoint
        manager.restore_checkpoint(&cp_id, &mut store).unwrap();

        assert_eq!(store.get("key1").unwrap().as_string().unwrap(), "value1");
        assert!(store.contains("key2"));
    }

    #[test]
    fn test_checkpoint_metadata() {
        let store = StateStore::new("test-store");
        let mut manager = CheckpointManager::new();

        let cp_id = manager
            .create_checkpoint(&store, "my-checkpoint", "Description here")
            .unwrap();

        let checkpoint = manager.get_checkpoint(&cp_id).unwrap();
        assert_eq!(checkpoint.name, "my-checkpoint");
        assert_eq!(checkpoint.description, "Description here");
        assert_eq!(checkpoint.store_name, "test-store");
    }

    #[test]
    fn test_checkpoint_not_found() {
        let mut store = StateStore::new("test-store");
        let manager = CheckpointManager::new();

        let result = manager.restore_checkpoint("non-existent", &mut store);
        assert!(matches!(result, Err(StateError::CheckpointNotFound(_))));
    }

    #[test]
    fn test_shared_state_store() {
        let store = SharedStateStore::new("shared-store");

        store
            .set("key1", StateValue::from_string("value1"))
            .unwrap();
        assert!(store.contains("key1"));

        let value = store.get("key1").unwrap();
        assert_eq!(value.as_string().unwrap(), "value1");

        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());

        store.delete("key1").unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn test_shared_state_store_version() {
        let store = SharedStateStore::new("shared-store");

        assert_eq!(store.version(), 0);

        store.set("key1", StateValue::from_string("v1")).unwrap();
        assert_eq!(store.version(), 1);

        store.set("key1", StateValue::from_string("v2")).unwrap();
        assert_eq!(store.version(), 2);
    }

    #[test]
    fn test_sync_result() {
        let mut result = SyncResult::new();
        result.pushed = 5;
        result.pulled = 3;

        assert!(result.is_success());
        assert_eq!(result.total_changes(), 8);

        result.conflicts = 1;
        result.conflict_keys.push("conflict-key".to_string());
        assert!(!result.is_success());
    }

    #[test]
    fn test_state_synchronizer_push() {
        let mut local = StateStore::new("local");
        let mut remote = StateStore::new("remote");
        let mut syncer = StateSynchronizer::new();

        local
            .set("key1", StateValue::from_string("local-value"))
            .unwrap();

        let result = syncer.sync(&mut local, &mut remote, SyncDirection::Push);

        assert_eq!(result.pushed, 1);
        assert_eq!(result.pulled, 0);
        assert!(remote.contains("key1"));
    }

    #[test]
    fn test_state_synchronizer_pull() {
        let mut local = StateStore::new("local");
        let mut remote = StateStore::new("remote");
        let mut syncer = StateSynchronizer::new();

        remote
            .set("key1", StateValue::from_string("remote-value"))
            .unwrap();

        let result = syncer.sync(&mut local, &mut remote, SyncDirection::Pull);

        assert_eq!(result.pushed, 0);
        assert_eq!(result.pulled, 1);
        assert!(local.contains("key1"));
    }

    #[test]
    fn test_state_synchronizer_bidirectional() {
        let mut local = StateStore::new("local");
        let mut remote = StateStore::new("remote");
        let mut syncer = StateSynchronizer::new();

        local
            .set("local-key", StateValue::from_string("local-value"))
            .unwrap();
        remote
            .set("remote-key", StateValue::from_string("remote-value"))
            .unwrap();

        let result = syncer.sync(&mut local, &mut remote, SyncDirection::Bidirectional);

        assert_eq!(result.pushed, 1);
        assert_eq!(result.pulled, 1);
        assert!(local.contains("remote-key"));
        assert!(remote.contains("local-key"));
    }

    #[test]
    fn test_state_synchronizer_conflict_resolution() {
        let mut local = StateStore::new("local");
        let mut remote = StateStore::new("remote");

        local
            .set("key1", StateValue::from_string("local-value"))
            .unwrap();
        std::thread::sleep(Duration::from_millis(10));
        remote
            .set("key1", StateValue::from_string("remote-value"))
            .unwrap();

        // NewerWins strategy - remote is newer
        let mut syncer =
            StateSynchronizer::new().with_conflict_strategy(ConflictStrategy::NewerWins);
        let result = syncer.sync(&mut local, &mut remote, SyncDirection::Push);

        // Local trying to push older value should conflict
        assert_eq!(result.conflicts, 1);

        // LocalWins strategy - local always wins
        let mut syncer =
            StateSynchronizer::new().with_conflict_strategy(ConflictStrategy::LocalWins);
        let result = syncer.sync(&mut local, &mut remote, SyncDirection::Push);

        assert_eq!(result.pushed, 1);
        assert_eq!(result.conflicts, 0);
    }

    #[test]
    fn test_state_synchronizer_stats() {
        let mut local = StateStore::new("local");
        let mut remote = StateStore::new("remote");
        let mut syncer = StateSynchronizer::new();

        assert_eq!(syncer.total_syncs(), 0);
        assert!(syncer.time_since_last_sync().is_none());

        syncer.sync(&mut local, &mut remote, SyncDirection::Push);

        assert_eq!(syncer.total_syncs(), 1);
        assert!(syncer.time_since_last_sync().is_some());
    }
}
