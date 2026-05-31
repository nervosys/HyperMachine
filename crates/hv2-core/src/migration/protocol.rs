//! Live migration protocol implementation
//!
//! This module implements the pre-copy live migration algorithm for
//! transferring VM state between hosts with minimal downtime.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use super::dirty_tracking::{DirtyTracker, PAGE_SIZE};
use super::state::{CpuState, DeviceState, SerializeError, SerializeResult};

/// Migration stage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStage {
    /// Not started
    Idle,
    /// Setting up migration
    Setup,
    /// Pre-copy: iteratively transferring dirty pages while VM runs
    PreCopy,
    /// Stop-and-copy: VM paused, final state transfer
    StopAndCopy,
    /// Post-copy: VM running on destination, pages faulted on demand
    PostCopy,
    /// Migration completed successfully
    Completed,
    /// Migration failed
    Failed,
    /// Migration cancelled
    Cancelled,
}

/// Migration direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationRole {
    /// Source VM (sending state)
    Source,
    /// Destination VM (receiving state)
    Destination,
}

/// Migration configuration
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Maximum number of pre-copy iterations
    pub max_precopy_iterations: u32,
    /// Dirty page threshold to enter stop-and-copy (pages)
    pub dirty_threshold: u64,
    /// Maximum downtime allowed (milliseconds)
    pub max_downtime_ms: u64,
    /// Bandwidth limit (bytes per second, 0 = unlimited)
    pub bandwidth_limit: u64,
    /// Enable compression
    pub compression_enabled: bool,
    /// Enable RDMA (if available)
    pub rdma_enabled: bool,
    /// Post-copy enabled
    pub postcopy_enabled: bool,
    /// Auto-converge (slow down vCPU if dirty rate too high)
    pub auto_converge: bool,
    /// Multifd channels (parallel transfer)
    pub multifd_channels: u32,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            max_precopy_iterations: 30,
            dirty_threshold: 256, // 1MB with 4KB pages
            max_downtime_ms: 300,
            bandwidth_limit: 0,
            compression_enabled: false,
            rdma_enabled: false,
            postcopy_enabled: false,
            auto_converge: true,
            multifd_channels: 2,
        }
    }
}

/// Migration statistics
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    /// Total bytes transferred
    pub total_bytes: u64,
    /// Total pages transferred
    pub total_pages: u64,
    /// Duplicate pages skipped
    pub duplicate_pages: u64,
    /// Pages transferred in current iteration
    pub iteration_pages: u64,
    /// Current iteration number
    pub iteration: u32,
    /// Transfer rate (bytes/sec)
    pub transfer_rate: f64,
    /// Dirty rate (pages/sec)
    pub dirty_rate: f64,
    /// Expected downtime (ms)
    pub expected_downtime_ms: u64,
    /// Actual downtime (ms)
    pub actual_downtime_ms: u64,
    /// Time in pre-copy (ms)
    pub precopy_time_ms: u64,
    /// Compression ratio (if enabled)
    pub compression_ratio: f64,
}

/// Memory page for transfer
#[derive(Debug, Clone)]
pub struct PageData {
    /// Guest physical address
    pub gpa: u64,
    /// Page data
    pub data: Vec<u8>,
    /// Page is zero-filled
    pub is_zero: bool,
}

impl PageData {
    /// Create a new page
    pub fn new(gpa: u64, data: Vec<u8>) -> Self {
        let is_zero = data.iter().all(|&b| b == 0);
        Self { gpa, data, is_zero }
    }

    /// Create a zero page
    pub fn zero(gpa: u64) -> Self {
        Self {
            gpa,
            data: Vec::new(),
            is_zero: true,
        }
    }

    /// Get page size
    pub fn size(&self) -> usize {
        if self.is_zero {
            0
        } else {
            self.data.len()
        }
    }
}

/// Migration message types
#[derive(Debug, Clone)]
pub enum MigrationMessage {
    /// Migration setup
    Setup {
        version: u32,
        config: MigrationConfig,
    },
    /// Setup acknowledgment
    SetupAck {
        accepted: bool,
        error: Option<String>,
    },
    /// Memory pages
    Pages {
        iteration: u32,
        pages: Vec<PageData>,
    },
    /// CPU state
    CpuState { cpu_id: u32, state: CpuState },
    /// Device state
    DeviceState { device: DeviceState },
    /// End of iteration
    IterationEnd { iteration: u32, dirty_pages: u64 },
    /// Request to pause VM
    PauseRequest,
    /// VM paused acknowledgment
    PauseAck,
    /// Migration complete
    Complete,
    /// Migration failed
    Failed { error: String },
    /// Cancel migration
    Cancel,
    /// Post-copy page fault request
    PageFault { gpa: u64 },
    /// Post-copy page response
    PageResponse { page: PageData },
}

/// Migration progress callback
pub type ProgressCallback = Box<dyn Fn(&MigrationStats) + Send + Sync>;

/// Migration state machine
#[derive(Debug)]
pub struct MigrationController {
    /// Current stage
    stage: RwLock<MigrationStage>,
    /// Role (source or destination)
    role: MigrationRole,
    /// Configuration
    config: MigrationConfig,
    /// Statistics
    stats: RwLock<MigrationStats>,
    /// Cancel flag
    cancelled: AtomicBool,
    /// Start time
    start_time: RwLock<Option<Instant>>,
    /// Precopy start time
    precopy_start: RwLock<Option<Instant>>,
    /// Pages pending transfer
    pending_pages: RwLock<VecDeque<PageData>>,
    /// Bytes transferred
    bytes_transferred: AtomicU64,
    /// Pages transferred
    pages_transferred: AtomicU64,
}

impl MigrationController {
    /// Create a new migration controller
    pub fn new(role: MigrationRole, config: MigrationConfig) -> Self {
        Self {
            stage: RwLock::new(MigrationStage::Idle),
            role,
            config,
            stats: RwLock::new(MigrationStats::default()),
            cancelled: AtomicBool::new(false),
            start_time: RwLock::new(None),
            precopy_start: RwLock::new(None),
            pending_pages: RwLock::new(VecDeque::new()),
            bytes_transferred: AtomicU64::new(0),
            pages_transferred: AtomicU64::new(0),
        }
    }

    /// Get current stage
    pub fn stage(&self) -> MigrationStage {
        *self.stage.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Get role
    pub fn role(&self) -> MigrationRole {
        self.role
    }

    /// Get configuration
    pub fn config(&self) -> &MigrationConfig {
        &self.config
    }

    /// Get statistics
    pub fn stats(&self) -> MigrationStats {
        self.stats.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Start migration
    pub fn start(&self) -> SerializeResult<()> {
        let mut stage = self.stage.write().unwrap_or_else(|e| e.into_inner());
        if *stage != MigrationStage::Idle {
            return Err(SerializeError::InvalidFormat(
                "Migration already in progress".into(),
            ));
        }

        *stage = MigrationStage::Setup;
        *self.start_time.write().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());

        Ok(())
    }

    /// Transition to pre-copy stage
    pub fn begin_precopy(&self) -> SerializeResult<()> {
        let mut stage = self.stage.write().unwrap_or_else(|e| e.into_inner());
        if *stage != MigrationStage::Setup {
            return Err(SerializeError::InvalidFormat(
                "Invalid stage for precopy".into(),
            ));
        }

        *stage = MigrationStage::PreCopy;
        *self
            .precopy_start
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());

        Ok(())
    }

    /// Transition to stop-and-copy stage
    pub fn begin_stop_and_copy(&self) -> SerializeResult<()> {
        let mut stage = self.stage.write().unwrap_or_else(|e| e.into_inner());
        if *stage != MigrationStage::PreCopy {
            return Err(SerializeError::InvalidFormat(
                "Invalid stage for stop-and-copy".into(),
            ));
        }

        *stage = MigrationStage::StopAndCopy;

        // Record precopy time
        if let Some(start) = *self.precopy_start.read().unwrap_or_else(|e| e.into_inner()) {
            let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
            stats.precopy_time_ms = start.elapsed().as_millis() as u64;
        }

        Ok(())
    }

    /// Complete migration
    pub fn complete(&self) -> SerializeResult<()> {
        let mut stage = self.stage.write().unwrap_or_else(|e| e.into_inner());
        if *stage != MigrationStage::StopAndCopy && *stage != MigrationStage::PostCopy {
            return Err(SerializeError::InvalidFormat(
                "Invalid stage for completion".into(),
            ));
        }

        *stage = MigrationStage::Completed;

        Ok(())
    }

    /// Fail migration
    pub fn fail(&self, _error: &str) {
        *self.stage.write().unwrap_or_else(|e| e.into_inner()) = MigrationStage::Failed;
    }

    /// Cancel migration
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        *self.stage.write().unwrap_or_else(|e| e.into_inner()) = MigrationStage::Cancelled;
    }

    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Add pages to transfer queue
    pub fn queue_pages(&self, pages: Vec<PageData>) {
        let mut pending = self
            .pending_pages
            .write()
            .unwrap_or_else(|e| e.into_inner());
        for page in pages {
            pending.push_back(page);
        }
    }

    /// Get next batch of pages to transfer
    pub fn get_pages_batch(&self, max_pages: usize) -> Vec<PageData> {
        let mut pending = self
            .pending_pages
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let mut batch = Vec::with_capacity(max_pages);

        for _ in 0..max_pages {
            if let Some(page) = pending.pop_front() {
                batch.push(page);
            } else {
                break;
            }
        }

        batch
    }

    /// Record transferred bytes
    pub fn record_transfer(&self, bytes: u64, pages: u64) {
        self.bytes_transferred.fetch_add(bytes, Ordering::Relaxed);
        self.pages_transferred.fetch_add(pages, Ordering::Relaxed);

        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.total_bytes += bytes;
        stats.total_pages += pages;
        stats.iteration_pages += pages;
    }

    /// Start new iteration
    pub fn start_iteration(&self) {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.iteration += 1;
        stats.iteration_pages = 0;
    }

    /// Check if should enter stop-and-copy
    pub fn should_stop_and_copy(&self, dirty_pages: u64, dirty_rate: f64) -> bool {
        // Enter stop-and-copy if:
        // 1. Dirty page count below threshold
        // 2. Max iterations reached
        // 3. Expected downtime acceptable

        let stats = self.stats.read().unwrap_or_else(|e| e.into_inner());

        if dirty_pages <= self.config.dirty_threshold {
            return true;
        }

        if stats.iteration >= self.config.max_precopy_iterations {
            return true;
        }

        // Calculate expected downtime
        let expected_downtime_ms = if stats.transfer_rate > 0.0 {
            (dirty_pages as f64 * PAGE_SIZE as f64 / stats.transfer_rate * 1000.0) as u64
        } else {
            u64::MAX
        };

        expected_downtime_ms <= self.config.max_downtime_ms
            && dirty_rate < stats.transfer_rate / PAGE_SIZE as f64
    }

    /// Update transfer rate
    pub fn update_transfer_rate(&self, elapsed_secs: f64) {
        let bytes = self.bytes_transferred.swap(0, Ordering::AcqRel);
        let rate = bytes as f64 / elapsed_secs;

        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.transfer_rate = rate;
    }

    /// Update dirty rate
    pub fn update_dirty_rate(&self, dirty_rate: f64) {
        let mut stats = self.stats.write().unwrap_or_else(|e| e.into_inner());
        stats.dirty_rate = dirty_rate;

        // Update expected downtime
        if stats.transfer_rate > 0.0 {
            let pages_per_sec = stats.transfer_rate / PAGE_SIZE as f64;
            let convergence_rate = pages_per_sec - dirty_rate;
            if convergence_rate > 0.0 {
                // Rough estimate
                stats.expected_downtime_ms =
                    (stats.dirty_rate * PAGE_SIZE as f64 / stats.transfer_rate * 1000.0) as u64;
            }
        }
    }

    /// Get pending page count
    pub fn pending_page_count(&self) -> usize {
        self.pending_pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

/// Pre-copy migration algorithm
pub struct PreCopyMigration {
    /// Controller
    controller: Arc<MigrationController>,
    /// Dirty tracker
    dirty_tracker: Arc<RwLock<DirtyTracker>>,
    /// Memory reader function
    memory_reader: Option<Box<dyn Fn(u64, u64) -> Vec<u8> + Send + Sync>>,
}

impl PreCopyMigration {
    /// Create new pre-copy migration
    pub fn new(
        controller: Arc<MigrationController>,
        dirty_tracker: Arc<RwLock<DirtyTracker>>,
    ) -> Self {
        Self {
            controller,
            dirty_tracker,
            memory_reader: None,
        }
    }

    /// Set memory reader
    pub fn set_memory_reader<F>(&mut self, reader: F)
    where
        F: Fn(u64, u64) -> Vec<u8> + Send + Sync + 'static,
    {
        self.memory_reader = Some(Box::new(reader));
    }

    /// Run one pre-copy iteration
    pub fn run_iteration(&self) -> SerializeResult<(Vec<PageData>, u64)> {
        if self.controller.is_cancelled() {
            return Err(SerializeError::InvalidFormat("Migration cancelled".into()));
        }

        self.controller.start_iteration();

        // Get dirty pages
        let dirty_pages = {
            let tracker = self.dirty_tracker.read().unwrap_or_else(|e| e.into_inner());
            tracker.dirty_pages()
        };

        let dirty_count = dirty_pages.len() as u64;

        // Read page data
        let mut pages = Vec::with_capacity(dirty_pages.len());
        if let Some(ref reader) = self.memory_reader {
            for gpa in dirty_pages {
                let data = reader(gpa, PAGE_SIZE);
                pages.push(PageData::new(gpa, data));
            }
        }

        // Clear dirty bits
        {
            let tracker = self.dirty_tracker.read().unwrap_or_else(|e| e.into_inner());
            tracker.clear_all();
        }

        Ok((pages, dirty_count))
    }

    /// Check convergence
    pub fn is_converging(&self) -> bool {
        let stats = self.controller.stats();
        stats.transfer_rate > 0.0 && stats.dirty_rate < stats.transfer_rate / PAGE_SIZE as f64
    }
}

/// Migration stream for sending/receiving data
pub struct MigrationStream {
    /// Outgoing message queue
    outgoing: RwLock<VecDeque<MigrationMessage>>,
    /// Incoming message queue
    incoming: RwLock<VecDeque<MigrationMessage>>,
}

impl MigrationStream {
    /// Create new stream
    pub fn new() -> Self {
        Self {
            outgoing: RwLock::new(VecDeque::new()),
            incoming: RwLock::new(VecDeque::new()),
        }
    }

    /// Send a message
    pub fn send(&self, msg: MigrationMessage) {
        self.outgoing
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(msg);
    }

    /// Receive a message
    pub fn receive(&self) -> Option<MigrationMessage> {
        self.incoming
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    /// Push incoming message (from network)
    pub fn push_incoming(&self, msg: MigrationMessage) {
        self.incoming
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(msg);
    }

    /// Pop outgoing message (for network)
    pub fn pop_outgoing(&self) -> Option<MigrationMessage> {
        self.outgoing
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    /// Check if has outgoing messages
    pub fn has_outgoing(&self) -> bool {
        !self
            .outgoing
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    /// Check if has incoming messages
    pub fn has_incoming(&self) -> bool {
        !self
            .incoming
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }
}

impl Default for MigrationStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_stage() {
        assert_ne!(MigrationStage::Idle, MigrationStage::PreCopy);
        assert_eq!(MigrationStage::Idle, MigrationStage::Idle);
    }

    #[test]
    fn test_migration_config_default() {
        let config = MigrationConfig::default();
        assert_eq!(config.max_precopy_iterations, 30);
        assert_eq!(config.dirty_threshold, 256);
        assert_eq!(config.max_downtime_ms, 300);
    }

    #[test]
    fn test_page_data_new() {
        let page = PageData::new(0x1000, vec![0; 4096]);
        assert_eq!(page.gpa, 0x1000);
        assert!(page.is_zero);
        assert_eq!(page.size(), 0); // Zero pages have no data

        let page = PageData::new(0x2000, vec![1; 4096]);
        assert!(!page.is_zero);
        assert_eq!(page.size(), 4096);
    }

    #[test]
    fn test_page_data_zero() {
        let page = PageData::zero(0x3000);
        assert!(page.is_zero);
        assert!(page.data.is_empty());
        assert_eq!(page.size(), 0);
    }

    #[test]
    fn test_migration_controller_creation() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());
        assert_eq!(controller.stage(), MigrationStage::Idle);
        assert_eq!(controller.role(), MigrationRole::Source);
    }

    #[test]
    fn test_migration_controller_transitions() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        assert!(controller.start().is_ok());
        assert_eq!(controller.stage(), MigrationStage::Setup);

        assert!(controller.begin_precopy().is_ok());
        assert_eq!(controller.stage(), MigrationStage::PreCopy);

        assert!(controller.begin_stop_and_copy().is_ok());
        assert_eq!(controller.stage(), MigrationStage::StopAndCopy);

        assert!(controller.complete().is_ok());
        assert_eq!(controller.stage(), MigrationStage::Completed);
    }

    #[test]
    fn test_migration_controller_invalid_transition() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        // Can't begin precopy from Idle
        assert!(controller.begin_precopy().is_err());

        // Start first
        controller.start().unwrap();

        // Can't start again
        assert!(controller.start().is_err());
    }

    #[test]
    fn test_migration_controller_cancel() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        assert!(!controller.is_cancelled());
        controller.cancel();
        assert!(controller.is_cancelled());
        assert_eq!(controller.stage(), MigrationStage::Cancelled);
    }

    #[test]
    fn test_migration_controller_queue_pages() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        let pages = vec![
            PageData::new(0x1000, vec![1; 4096]),
            PageData::new(0x2000, vec![2; 4096]),
        ];

        controller.queue_pages(pages);
        assert_eq!(controller.pending_page_count(), 2);

        let batch = controller.get_pages_batch(1);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].gpa, 0x1000);

        assert_eq!(controller.pending_page_count(), 1);
    }

    #[test]
    fn test_migration_controller_record_transfer() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        controller.record_transfer(4096, 1);
        controller.record_transfer(8192, 2);

        let stats = controller.stats();
        assert_eq!(stats.total_bytes, 12288);
        assert_eq!(stats.total_pages, 3);
    }

    #[test]
    fn test_migration_controller_iteration() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        controller.start_iteration();
        assert_eq!(controller.stats().iteration, 1);

        controller.record_transfer(4096, 1);
        assert_eq!(controller.stats().iteration_pages, 1);

        controller.start_iteration();
        assert_eq!(controller.stats().iteration, 2);
        assert_eq!(controller.stats().iteration_pages, 0);
    }

    #[test]
    fn test_should_stop_and_copy() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        // Below threshold
        assert!(controller.should_stop_and_copy(100, 100.0));

        // Above threshold but max iterations reached
        {
            let mut stats = controller.stats.write().unwrap();
            stats.iteration = 30;
        }
        assert!(controller.should_stop_and_copy(1000, 100.0));
    }

    #[test]
    fn test_migration_stream() {
        let stream = MigrationStream::new();

        assert!(!stream.has_outgoing());
        assert!(!stream.has_incoming());

        stream.send(MigrationMessage::PauseRequest);
        assert!(stream.has_outgoing());

        let msg = stream.pop_outgoing().unwrap();
        assert!(matches!(msg, MigrationMessage::PauseRequest));

        stream.push_incoming(MigrationMessage::PauseAck);
        assert!(stream.has_incoming());

        let msg = stream.receive().unwrap();
        assert!(matches!(msg, MigrationMessage::PauseAck));
    }

    #[test]
    fn test_precopy_migration() {
        let controller = Arc::new(MigrationController::new(
            MigrationRole::Source,
            MigrationConfig::default(),
        ));
        let dirty_tracker = Arc::new(RwLock::new(DirtyTracker::new()));

        // Setup dirty tracker
        {
            let mut tracker = dirty_tracker.write().unwrap();
            tracker.add_region(0, 0x100000);
            tracker.enable();
            tracker.mark_dirty(0x1000);
            tracker.mark_dirty(0x2000);
        }

        let mut migration = PreCopyMigration::new(controller.clone(), dirty_tracker.clone());

        // Set dummy memory reader
        migration.set_memory_reader(|_gpa, size| vec![0xAA; size as usize]);

        controller.start().unwrap();
        controller.begin_precopy().unwrap();

        let (pages, dirty_count) = migration.run_iteration().unwrap();

        assert_eq!(dirty_count, 2);
        assert_eq!(pages.len(), 2);
    }

    #[test]
    fn test_precopy_cancelled() {
        let controller = Arc::new(MigrationController::new(
            MigrationRole::Source,
            MigrationConfig::default(),
        ));
        let dirty_tracker = Arc::new(RwLock::new(DirtyTracker::new()));

        let migration = PreCopyMigration::new(controller.clone(), dirty_tracker);

        controller.cancel();

        assert!(migration.run_iteration().is_err());
    }

    #[test]
    fn test_migration_message_variants() {
        let setup = MigrationMessage::Setup {
            version: 1,
            config: MigrationConfig::default(),
        };
        assert!(matches!(setup, MigrationMessage::Setup { .. }));

        let failed = MigrationMessage::Failed {
            error: "test error".to_string(),
        };
        assert!(matches!(failed, MigrationMessage::Failed { .. }));
    }

    #[test]
    fn test_migration_stats_default() {
        let stats = MigrationStats::default();
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.total_pages, 0);
        assert_eq!(stats.iteration, 0);
    }

    #[test]
    fn test_controller_fail() {
        let controller =
            MigrationController::new(MigrationRole::Source, MigrationConfig::default());

        controller.start().unwrap();
        controller.fail("test error");

        assert_eq!(controller.stage(), MigrationStage::Failed);
    }
}
