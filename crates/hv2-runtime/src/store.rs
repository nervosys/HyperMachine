//! Durable State Store
//!
//! Provides persistent key-value storage that survives VM restarts
//! and failures. Workflow checkpoints, agent state, and session data
//! are stored here so they can be recovered on a different VM.
//!
//! # Backends
//!
//! - **Memory**: In-process `HashMap` (testing and single-process mode)
//! - **File**: Local filesystem with JSON serialization
//! - **External**: Placeholder for etcd, Postgres, S3, etc.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Store operation result
pub type StoreResult<T> = Result<T, StoreError>;

/// Store errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// Key not found
    #[error("Key not found: {0}")]
    NotFound(String),

    /// Key already exists (conditional put failed)
    #[error("Key already exists: {0}")]
    AlreadyExists(String),

    /// Version conflict (CAS failed)
    #[error("Version conflict on key '{key}': expected {expected}, actual {actual}")]
    VersionConflict {
        key: String,
        expected: u64,
        actual: u64,
    },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Backend error
    #[error("Backend error: {0}")]
    Backend(String),

    /// Store is closed
    #[error("Store is closed")]
    Closed,
}

/// Store backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StoreBackend {
    /// In-memory (non-persistent, for testing)
    #[default]
    Memory,
    /// Local filesystem
    File,
    /// External KV store (etcd, Consul, etc.)
    External,
}

/// Store configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    /// Backend type
    pub backend: StoreBackend,
    /// File path for File backend
    pub file_path: Option<String>,
    /// Connection URL for External backend (e.g. "etcd://localhost:2379")
    pub external_url: Option<String>,
    /// Maximum entries
    pub max_entries: usize,
    /// Default TTL for entries (None = no expiry)
    pub default_ttl: Option<Duration>,
    /// Enable versioning (compare-and-swap)
    pub enable_versioning: bool,
    /// Enable watch notifications
    pub enable_watch: bool,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            backend: StoreBackend::Memory,
            file_path: None,
            external_url: None,
            max_entries: 100_000,
            default_ttl: None,
            enable_versioning: true,
            enable_watch: true,
        }
    }
}

/// A stored entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreEntry {
    /// The key
    pub key: String,
    /// The value (JSON bytes)
    pub value: Vec<u8>,
    /// Version number (incremented on each write)
    pub version: u64,
    /// Creation timestamp
    pub created_at: SystemTime,
    /// Last modified timestamp
    pub modified_at: SystemTime,
    /// Time-to-live (absolute expiry time)
    pub expires_at: Option<SystemTime>,
    /// Content type hint
    pub content_type: String,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl StoreEntry {
    /// Create a new entry
    fn new(key: String, value: Vec<u8>, ttl: Option<Duration>) -> Self {
        let now = SystemTime::now();
        Self {
            key,
            value,
            version: 1,
            created_at: now,
            modified_at: now,
            expires_at: ttl.map(|d| now + d),
            content_type: "application/json".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Check if this entry has expired
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| SystemTime::now() > exp)
    }

    /// Get the value as a string
    pub fn as_string(&self) -> StoreResult<String> {
        String::from_utf8(self.value.clone()).map_err(|e| StoreError::Serialization(e.to_string()))
    }

    /// Deserialize value as JSON
    pub fn as_json<T: for<'de> Deserialize<'de>>(&self) -> StoreResult<T> {
        serde_json::from_slice(&self.value).map_err(|e| StoreError::Serialization(e.to_string()))
    }
}

/// Watch event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchEventType {
    /// Entry created
    Created,
    /// Entry updated
    Updated,
    /// Entry deleted
    Deleted,
    /// Entry expired
    Expired,
}

/// A watch event notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEvent {
    /// Event type
    pub event_type: WatchEventType,
    /// Key affected
    pub key: String,
    /// New version (if applicable)
    pub version: Option<u64>,
    /// Timestamp
    pub timestamp: SystemTime,
}

/// Durable state store
///
/// Thread-safe key-value store with versioning, TTL, and watch support.
/// Supports Memory, File, and External backends.
///
/// - **Memory**: Fastest, non-persistent (testing / single-process).
/// - **File**: JSON-serialized to `config.file_path`. Data is loaded on
///   construction and flushed after every mutation.
/// - **External**: Placeholder for etcd/Consul/Postgres backends.
pub struct DurableStore {
    /// Configuration
    config: StoreConfig,
    /// In-memory data (used for all backends — File syncs to disk)
    data: RwLock<BTreeMap<String, StoreEntry>>,
    /// Watch event log
    watch_log: RwLock<Vec<WatchEvent>>,
}

impl DurableStore {
    /// Create a new durable store
    ///
    /// For the `File` backend, `config.file_path` must be `Some`. If the file
    /// already exists it is loaded; otherwise an empty store is created.
    pub fn new(config: StoreConfig) -> Self {
        let data = if config.backend == StoreBackend::File {
            if let Some(ref path) = config.file_path {
                Self::load_from_file(path).unwrap_or_default()
            } else {
                BTreeMap::new()
            }
        } else if config.backend == StoreBackend::External {
            if let Some(ref url) = config.external_url {
                tracing::info!("External store backend configured with URL: {url}");
                tracing::warn!("External backend operates in memory-cache mode; writes are not persisted to an external service yet");
            } else {
                tracing::warn!("External backend selected without external_url; operating as in-memory store");
            }
            BTreeMap::new()
        } else {
            BTreeMap::new()
        };

        Self {
            config,
            data: RwLock::new(data),
            watch_log: RwLock::new(Vec::new()),
        }
    }

    /// Load data from a JSON file on disk
    fn load_from_file(path: &str) -> StoreResult<BTreeMap<String, StoreEntry>> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| StoreError::Backend(format!("Failed to read {path}: {e}")))?;
        serde_json::from_str(&contents)
            .map_err(|e| StoreError::Serialization(format!("Failed to parse {path}: {e}")))
    }

    /// Flush the current in-memory data to the configured file path.
    ///
    /// For non-File backends this is a no-op.
    fn flush_to_file(&self) {
        if self.config.backend != StoreBackend::File {
            return;
        }
        if let Some(ref path) = self.config.file_path {
            let data = self.data.read();
            // Write to a temporary file first, then rename for atomicity
            let tmp_path = format!("{path}.tmp");
            let serialized = match serde_json::to_string_pretty(&*data) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to serialize store data: {e}");
                    return;
                }
            };
            if let Err(e) = std::fs::write(&tmp_path, serialized) {
                tracing::error!("Failed to write store file {tmp_path}: {e}");
                return;
            }
            if let Err(e) = std::fs::rename(&tmp_path, path) {
                tracing::error!("Failed to rename {tmp_path} -> {path}: {e}");
            }
        }
    }

    /// Get an entry by key
    pub fn get(&self, key: &str) -> StoreResult<StoreEntry> {
        let data = self.data.read();
        let entry = data
            .get(key)
            .ok_or_else(|| StoreError::NotFound(key.to_string()))?;

        if entry.is_expired() {
            drop(data);
            self.delete(key)?;
            return Err(StoreError::NotFound(key.to_string()));
        }

        Ok(entry.clone())
    }

    /// Put a value (create or update)
    pub fn put(&self, key: &str, value: Vec<u8>) -> StoreResult<u64> {
        self.put_with_ttl(key, value, self.config.default_ttl)
    }

    /// Put a value with explicit TTL
    pub fn put_with_ttl(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> StoreResult<u64> {
        let mut data = self.data.write();

        if data.len() >= self.config.max_entries && !data.contains_key(key) {
            return Err(StoreError::Backend(format!(
                "Store full: {}/{}",
                data.len(),
                self.config.max_entries
            )));
        }

        let version = if let Some(existing) = data.get(key) {
            let new_version = existing.version + 1;
            let mut entry = StoreEntry::new(key.to_string(), value, ttl);
            entry.version = new_version;
            entry.created_at = existing.created_at;
            data.insert(key.to_string(), entry);
            self.emit_watch(WatchEventType::Updated, key, Some(new_version));
            new_version
        } else {
            let entry = StoreEntry::new(key.to_string(), value, ttl);
            data.insert(key.to_string(), entry);
            self.emit_watch(WatchEventType::Created, key, Some(1));
            1
        };

        drop(data);
        self.flush_to_file();
        Ok(version)
    }

    /// Put a JSON-serializable value
    pub fn put_json<T: Serialize>(&self, key: &str, value: &T) -> StoreResult<u64> {
        let bytes =
            serde_json::to_vec(value).map_err(|e| StoreError::Serialization(e.to_string()))?;
        self.put(key, bytes)
    }

    /// Compare-and-swap: update only if version matches
    pub fn cas(&self, key: &str, value: Vec<u8>, expected_version: u64) -> StoreResult<u64> {
        let mut data = self.data.write();
        let existing = data
            .get(key)
            .ok_or_else(|| StoreError::NotFound(key.to_string()))?;

        if existing.version != expected_version {
            return Err(StoreError::VersionConflict {
                key: key.to_string(),
                expected: expected_version,
                actual: existing.version,
            });
        }

        let new_version = existing.version + 1;
        let mut entry = StoreEntry::new(key.to_string(), value, self.config.default_ttl);
        entry.version = new_version;
        entry.created_at = existing.created_at;
        data.insert(key.to_string(), entry);

        self.emit_watch(WatchEventType::Updated, key, Some(new_version));
        drop(data);
        self.flush_to_file();
        Ok(new_version)
    }

    /// Create a key only if it doesn't exist
    pub fn create(&self, key: &str, value: Vec<u8>) -> StoreResult<()> {
        let mut data = self.data.write();
        if data.contains_key(key) {
            return Err(StoreError::AlreadyExists(key.to_string()));
        }

        if data.len() >= self.config.max_entries {
            return Err(StoreError::Backend(format!(
                "Store full: {}/{}",
                data.len(),
                self.config.max_entries
            )));
        }

        let entry = StoreEntry::new(key.to_string(), value, self.config.default_ttl);
        data.insert(key.to_string(), entry);
        self.emit_watch(WatchEventType::Created, key, Some(1));
        drop(data);
        self.flush_to_file();
        Ok(())
    }

    /// Delete a key
    pub fn delete(&self, key: &str) -> StoreResult<()> {
        let mut data = self.data.write();
        if data.remove(key).is_none() {
            return Err(StoreError::NotFound(key.to_string()));
        }
        self.emit_watch(WatchEventType::Deleted, key, None);
        drop(data);
        self.flush_to_file();
        Ok(())
    }

    /// Check if a key exists
    pub fn exists(&self, key: &str) -> bool {
        let data = self.data.read();
        data.get(key).is_some_and(|e| !e.is_expired())
    }

    /// List keys matching a prefix
    pub fn list_prefix(&self, prefix: &str) -> Vec<String> {
        let data = self.data.read();
        data.range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .filter(|(_, v)| !v.is_expired())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Count entries
    pub fn len(&self) -> usize {
        self.data.read().len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.data.read().is_empty()
    }

    /// Get recent watch events
    pub fn recent_events(&self, count: usize) -> Vec<WatchEvent> {
        let log = self.watch_log.read();
        log.iter().rev().take(count).cloned().collect()
    }

    /// Clear all entries
    pub fn clear(&self) {
        self.data.write().clear();
        self.flush_to_file();
    }

    /// Get store configuration
    pub fn config(&self) -> &StoreConfig {
        &self.config
    }

    /// Remove expired entries
    pub fn gc(&self) -> usize {
        let mut data = self.data.write();
        let before = data.len();
        let expired_keys: Vec<String> = data
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for key in &expired_keys {
            data.remove(key);
        }
        drop(data);

        for key in &expired_keys {
            self.emit_watch(WatchEventType::Expired, key, None);
        }

        let removed = before - self.data.read().len();
        if removed > 0 {
            self.flush_to_file();
        }
        removed
    }

    fn emit_watch(&self, event_type: WatchEventType, key: &str, version: Option<u64>) {
        if !self.config.enable_watch {
            return;
        }
        let event = WatchEvent {
            event_type,
            key: key.to_string(),
            version,
            timestamp: SystemTime::now(),
        };
        let mut log = self.watch_log.write();
        log.push(event);
        // Keep bounded
        while log.len() > 10_000 {
            log.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> DurableStore {
        DurableStore::new(StoreConfig::default())
    }

    #[test]
    fn test_put_and_get() {
        let store = test_store();
        store.put("key1", b"value1".to_vec()).unwrap();

        let entry = store.get("key1").unwrap();
        assert_eq!(entry.value, b"value1");
        assert_eq!(entry.version, 1);
    }

    #[test]
    fn test_put_json() {
        let store = test_store();
        store
            .put_json("config", &serde_json::json!({"cpu": 4}))
            .unwrap();

        let entry = store.get("config").unwrap();
        let val: serde_json::Value = entry.as_json().unwrap();
        assert_eq!(val["cpu"], 4);
    }

    #[test]
    fn test_version_increment() {
        let store = test_store();
        let v1 = store.put("k", b"v1".to_vec()).unwrap();
        assert_eq!(v1, 1);
        let v2 = store.put("k", b"v2".to_vec()).unwrap();
        assert_eq!(v2, 2);
    }

    #[test]
    fn test_cas_success() {
        let store = test_store();
        store.put("k", b"v1".to_vec()).unwrap();
        let v2 = store.cas("k", b"v2".to_vec(), 1).unwrap();
        assert_eq!(v2, 2);
    }

    #[test]
    fn test_cas_conflict() {
        let store = test_store();
        store.put("k", b"v1".to_vec()).unwrap();
        let err = store.cas("k", b"v2".to_vec(), 99).unwrap_err();
        assert!(matches!(err, StoreError::VersionConflict { .. }));
    }

    #[test]
    fn test_create_idempotent() {
        let store = test_store();
        store.create("k", b"v".to_vec()).unwrap();
        let err = store.create("k", b"v2".to_vec()).unwrap_err();
        assert!(matches!(err, StoreError::AlreadyExists(_)));
    }

    #[test]
    fn test_delete() {
        let store = test_store();
        store.put("k", b"v".to_vec()).unwrap();
        store.delete("k").unwrap();
        assert!(!store.exists("k"));
    }

    #[test]
    fn test_delete_not_found() {
        let store = test_store();
        let err = store.delete("missing").unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn test_list_prefix() {
        let store = test_store();
        store.put("wf/1/step/a", b"v".to_vec()).unwrap();
        store.put("wf/1/step/b", b"v".to_vec()).unwrap();
        store.put("wf/2/step/a", b"v".to_vec()).unwrap();
        store.put("other/key", b"v".to_vec()).unwrap();

        let keys = store.list_prefix("wf/1/");
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_ttl_expiry() {
        let store = DurableStore::new(StoreConfig {
            default_ttl: Some(Duration::from_millis(1)),
            ..Default::default()
        });
        store.put("ephemeral", b"v".to_vec()).unwrap();

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(10));

        let err = store.get("ephemeral").unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn test_gc() {
        let store = DurableStore::new(StoreConfig::default());
        // Manually insert an expired entry
        {
            let mut data = store.data.write();
            let mut entry = StoreEntry::new("expired".to_string(), b"v".to_vec(), None);
            entry.expires_at = Some(SystemTime::now() - Duration::from_secs(1));
            data.insert("expired".to_string(), entry);
        }
        let removed = store.gc();
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_capacity_limit() {
        let store = DurableStore::new(StoreConfig {
            max_entries: 2,
            ..Default::default()
        });
        store.put("k1", b"v".to_vec()).unwrap();
        store.put("k2", b"v".to_vec()).unwrap();
        let err = store.put("k3", b"v".to_vec()).unwrap_err();
        assert!(matches!(err, StoreError::Backend(_)));
    }

    #[test]
    fn test_watch_events() {
        let store = test_store();
        store.put("k", b"v1".to_vec()).unwrap();
        store.put("k", b"v2".to_vec()).unwrap();
        store.delete("k").unwrap();

        let events = store.recent_events(10);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, WatchEventType::Deleted);
        assert_eq!(events[1].event_type, WatchEventType::Updated);
        assert_eq!(events[2].event_type, WatchEventType::Created);
    }

    #[test]
    fn test_clear() {
        let store = test_store();
        store.put("k1", b"v".to_vec()).unwrap();
        store.put("k2", b"v".to_vec()).unwrap();
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_exists() {
        let store = test_store();
        assert!(!store.exists("k"));
        store.put("k", b"v".to_vec()).unwrap();
        assert!(store.exists("k"));
    }
}
