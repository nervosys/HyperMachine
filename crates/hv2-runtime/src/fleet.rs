//! Fleet Lifecycle Management
//!
//! Coordinates driver, CUDA stack, and VM image rollouts across a
//! heterogeneous fleet. Supports rolling updates, canary deployments,
//! and targeted upgrades that respect host maintenance windows.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Fleet operation result
pub type FleetResult<T> = Result<T, FleetError>;

/// Fleet lifecycle errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FleetError {
    /// Rollout already in progress
    #[error("Rollout already in progress: {0}")]
    RolloutInProgress(String),

    /// Host not found
    #[error("Host not found: {0}")]
    HostNotFound(String),

    /// Version not found in catalog
    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),

    /// Rollout failed on a host
    #[error("Rollout failed on host {host}: {reason}")]
    RolloutFailed { host: String, reason: String },

    /// Invalid rollout configuration
    #[error("Invalid rollout config: {0}")]
    InvalidConfig(String),

    /// Rollout not found
    #[error("Rollout not found: {0}")]
    RolloutNotFound(String),
}

/// Type of fleet artifact being managed
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtifactKind {
    /// GPU driver package (e.g., NVIDIA 550.90.07)
    GpuDriver,
    /// CUDA toolkit version
    CudaToolkit,
    /// VM base image / rootfs
    VmImage,
    /// Container runtime
    ContainerRuntime,
    /// Firmware update
    Firmware,
    /// Custom component
    Custom(String),
}

/// Version descriptor for a fleet artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactVersion {
    /// Artifact kind
    pub kind: ArtifactKind,
    /// Version string (semver or vendor-specific)
    pub version: String,
    /// SHA-256 digest of the artifact
    pub sha256: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// When this version was published
    pub published_at: SystemTime,
    /// Whether this version is marked as the fleet default
    pub is_default: bool,
    /// Free-text release notes
    pub release_notes: String,
}

impl ArtifactVersion {
    /// Create a new artifact version
    pub fn new(kind: ArtifactKind, version: impl Into<String>, sha256: impl Into<String>) -> Self {
        Self {
            kind,
            version: version.into(),
            sha256: sha256.into(),
            size_bytes: 0,
            published_at: SystemTime::now(),
            is_default: false,
            release_notes: String::new(),
        }
    }
}

/// Rollout strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RolloutStrategy {
    /// Update all hosts simultaneously
    Immediate,
    /// Update hosts in sequential batches
    Rolling,
    /// Update a small canary group first, then proceed if healthy
    Canary,
    /// Update hosts in maintenance windows only
    Scheduled,
}

/// Rollout state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RolloutPhase {
    /// Rollout created, not started
    Pending,
    /// Canary hosts are being updated
    CanaryInProgress,
    /// Waiting for canary health validation
    CanaryValidation,
    /// Main rollout in progress
    RollingOut,
    /// Rollout complete
    Completed,
    /// Rollout paused by operator
    Paused,
    /// Rollout failed and stopped
    Failed,
    /// Rollout rolled back
    RolledBack,
}

/// Configuration for a fleet rollout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolloutConfig {
    /// Rollout strategy
    pub strategy: RolloutStrategy,
    /// Max hosts updated concurrently
    pub max_concurrent: usize,
    /// Canary group size (for Canary strategy)
    pub canary_count: usize,
    /// Time to wait for health check after each batch
    pub health_check_delay: Duration,
    /// Maximum tolerated failure percentage before halting (0–100)
    pub max_failure_pct: u32,
    /// Automatically rollback on failure threshold
    pub auto_rollback: bool,
}

impl Default for RolloutConfig {
    fn default() -> Self {
        Self {
            strategy: RolloutStrategy::Rolling,
            max_concurrent: 5,
            canary_count: 1,
            health_check_delay: Duration::from_secs(60),
            max_failure_pct: 10,
            auto_rollback: true,
        }
    }
}

/// Status of a rollout on a single host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostRolloutStatus {
    /// Host ID
    pub host_id: String,
    /// Current phase on this host
    pub phase: HostUpdatePhase,
    /// Previous artifact version (for rollback)
    pub previous_version: String,
    /// Target artifact version
    pub target_version: String,
    /// When the update started on this host
    pub started_at: Option<SystemTime>,
    /// When the update completed on this host
    pub completed_at: Option<SystemTime>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Update phase for a single host
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostUpdatePhase {
    /// Waiting to be updated
    Pending,
    /// Draining workloads from this host
    Draining,
    /// Applying the update
    Updating,
    /// Verifying health after update
    Verifying,
    /// Update succeeded
    Succeeded,
    /// Update failed
    Failed,
    /// Rolled back to previous version
    RolledBack,
}

/// A fleet rollout operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rollout {
    /// Unique rollout ID
    pub id: String,
    /// Artifact being rolled out
    pub artifact: ArtifactKind,
    /// Target version
    pub target_version: String,
    /// Rollout configuration
    pub config: RolloutConfig,
    /// Current phase
    pub phase: RolloutPhase,
    /// Per-host status
    pub hosts: Vec<HostRolloutStatus>,
    /// When the rollout was created
    pub created_at: SystemTime,
    /// When the rollout completed (if done)
    pub completed_at: Option<SystemTime>,
}

impl Rollout {
    /// Count hosts in a given phase
    pub fn count_in_phase(&self, phase: HostUpdatePhase) -> usize {
        self.hosts.iter().filter(|h| h.phase == phase).count()
    }

    /// Whether the rollout is terminal (completed, failed, or rolled back)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            RolloutPhase::Completed | RolloutPhase::Failed | RolloutPhase::RolledBack
        )
    }

    /// Failure percentage across all hosts
    pub fn failure_pct(&self) -> u32 {
        if self.hosts.is_empty() {
            return 0;
        }
        let failed = self.count_in_phase(HostUpdatePhase::Failed);
        ((failed as f64 / self.hosts.len() as f64) * 100.0) as u32
    }
}

/// Fleet host inventory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetHost {
    /// Host identifier
    pub id: String,
    /// Current artifact versions installed on this host
    pub installed_versions: HashMap<ArtifactKind, String>,
    /// Whether the host is in a maintenance window
    pub in_maintenance_window: bool,
    /// Whether the host is healthy
    pub healthy: bool,
    /// Number of active VMs on this host
    pub active_vm_count: u32,
    /// Tags for targeting (e.g., "region=us-east-1", "gpu=a100")
    pub tags: HashMap<String, String>,
}

impl FleetHost {
    /// Create a new fleet host entry
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            installed_versions: HashMap::new(),
            in_maintenance_window: false,
            healthy: true,
            active_vm_count: 0,
            tags: HashMap::new(),
        }
    }

    /// Set a tag
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Set installed version for an artifact
    pub fn installed(mut self, kind: ArtifactKind, version: impl Into<String>) -> Self {
        self.installed_versions.insert(kind, version.into());
        self
    }
}

/// Fleet lifecycle manager
///
/// Tracks host inventory, artifact catalog, and coordinates rollouts.
pub struct FleetManager {
    /// Host inventory
    hosts: RwLock<HashMap<String, FleetHost>>,
    /// Artifact catalog: (kind, version) -> ArtifactVersion
    catalog: RwLock<HashMap<(ArtifactKind, String), ArtifactVersion>>,
    /// Active and historical rollouts
    rollouts: RwLock<HashMap<String, Rollout>>,
    /// Rollout counter for ID generation
    rollout_counter: std::sync::atomic::AtomicU64,
}

impl FleetManager {
    /// Create a new fleet manager
    pub fn new() -> Self {
        Self {
            hosts: RwLock::new(HashMap::new()),
            catalog: RwLock::new(HashMap::new()),
            rollouts: RwLock::new(HashMap::new()),
            rollout_counter: std::sync::atomic::AtomicU64::new(1),
        }
    }

    // ── Host Inventory ────────────────────────────────────────────────

    /// Register a host in the fleet
    pub fn register_host(&self, host: FleetHost) {
        self.hosts.write().insert(host.id.clone(), host);
    }

    /// Unregister a host
    pub fn unregister_host(&self, host_id: &str) {
        self.hosts.write().remove(host_id);
    }

    /// Get a host entry
    pub fn get_host(&self, host_id: &str) -> Option<FleetHost> {
        self.hosts.read().get(host_id).cloned()
    }

    /// List all hosts
    pub fn list_hosts(&self) -> Vec<FleetHost> {
        self.hosts.read().values().cloned().collect()
    }

    /// Total host count
    pub fn host_count(&self) -> usize {
        self.hosts.read().len()
    }

    /// Filter hosts by tag
    pub fn hosts_with_tag(&self, key: &str, value: &str) -> Vec<FleetHost> {
        self.hosts
            .read()
            .values()
            .filter(|h| h.tags.get(key).is_some_and(|v| v == value))
            .cloned()
            .collect()
    }

    // ── Artifact Catalog ──────────────────────────────────────────────

    /// Publish a new artifact version to the catalog
    pub fn publish_artifact(&self, artifact: ArtifactVersion) {
        self.catalog
            .write()
            .insert((artifact.kind.clone(), artifact.version.clone()), artifact);
    }

    /// Get an artifact version
    pub fn get_artifact(&self, kind: &ArtifactKind, version: &str) -> Option<ArtifactVersion> {
        self.catalog
            .read()
            .get(&(kind.clone(), version.to_string()))
            .cloned()
    }

    /// List all versions for an artifact kind
    pub fn list_versions(&self, kind: &ArtifactKind) -> Vec<ArtifactVersion> {
        self.catalog
            .read()
            .values()
            .filter(|a| a.kind == *kind)
            .cloned()
            .collect()
    }

    // ── Rollouts ──────────────────────────────────────────────────────

    /// Create a new rollout targeting specific hosts (or all hosts)
    pub fn create_rollout(
        &self,
        artifact_kind: ArtifactKind,
        target_version: &str,
        target_hosts: Option<Vec<String>>,
        config: RolloutConfig,
    ) -> FleetResult<String> {
        // Validate artifact exists
        if self.get_artifact(&artifact_kind, target_version).is_none() {
            return Err(FleetError::ArtifactNotFound(format!(
                "{artifact_kind:?} v{target_version}"
            )));
        }

        // Check no conflicting rollout
        let rollouts = self.rollouts.read();
        for r in rollouts.values() {
            if r.artifact == artifact_kind && !r.is_terminal() {
                return Err(FleetError::RolloutInProgress(r.id.clone()));
            }
        }
        drop(rollouts);

        // Determine target hosts
        let hosts = self.hosts.read();
        let host_ids: Vec<String> = match target_hosts {
            Some(ids) => {
                // Validate all exist
                for id in &ids {
                    if !hosts.contains_key(id) {
                        return Err(FleetError::HostNotFound(id.clone()));
                    }
                }
                ids
            }
            None => hosts.keys().cloned().collect(),
        };

        if host_ids.is_empty() {
            return Err(FleetError::InvalidConfig("No target hosts".into()));
        }

        // Build per-host status
        let host_statuses: Vec<HostRolloutStatus> = host_ids
            .iter()
            .map(|hid| {
                let prev = hosts
                    .get(hid)
                    .and_then(|h| h.installed_versions.get(&artifact_kind))
                    .cloned()
                    .unwrap_or_default();
                HostRolloutStatus {
                    host_id: hid.clone(),
                    phase: HostUpdatePhase::Pending,
                    previous_version: prev,
                    target_version: target_version.to_string(),
                    started_at: None,
                    completed_at: None,
                    error: None,
                }
            })
            .collect();
        drop(hosts);

        let id = format!(
            "rollout-{}",
            self.rollout_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );

        let rollout = Rollout {
            id: id.clone(),
            artifact: artifact_kind,
            target_version: target_version.to_string(),
            config,
            phase: RolloutPhase::Pending,
            hosts: host_statuses,
            created_at: SystemTime::now(),
            completed_at: None,
        };

        self.rollouts.write().insert(id.clone(), rollout);
        Ok(id)
    }

    /// Advance a rollout by one step.
    ///
    /// Returns the updated rollout phase. Call this in a loop or on a
    /// timer to drive the rollout forward.
    pub fn advance_rollout(&self, rollout_id: &str) -> FleetResult<RolloutPhase> {
        let mut rollouts = self.rollouts.write();
        let rollout = rollouts
            .get_mut(rollout_id)
            .ok_or_else(|| FleetError::RolloutNotFound(rollout_id.to_string()))?;

        if rollout.is_terminal() {
            return Ok(rollout.phase);
        }

        match rollout.phase {
            RolloutPhase::Pending => {
                if rollout.config.strategy == RolloutStrategy::Canary {
                    // Move canary hosts to Updating
                    let canary = rollout.config.canary_count.min(rollout.hosts.len());
                    for host in rollout.hosts.iter_mut().take(canary) {
                        host.phase = HostUpdatePhase::Draining;
                        host.started_at = Some(SystemTime::now());
                    }
                    rollout.phase = RolloutPhase::CanaryInProgress;
                } else {
                    // Move first batch to Updating
                    let batch = rollout.config.max_concurrent.min(rollout.hosts.len());
                    for host in rollout.hosts.iter_mut().take(batch) {
                        host.phase = HostUpdatePhase::Draining;
                        host.started_at = Some(SystemTime::now());
                    }
                    rollout.phase = RolloutPhase::RollingOut;
                }
            }
            RolloutPhase::CanaryInProgress => {
                // Simulate canary hosts finishing
                let mut all_done = true;
                for host in &mut rollout.hosts {
                    match host.phase {
                        HostUpdatePhase::Draining => {
                            host.phase = HostUpdatePhase::Updating;
                            all_done = false;
                        }
                        HostUpdatePhase::Updating => {
                            host.phase = HostUpdatePhase::Verifying;
                            all_done = false;
                        }
                        HostUpdatePhase::Verifying => {
                            host.phase = HostUpdatePhase::Succeeded;
                            host.completed_at = Some(SystemTime::now());
                        }
                        HostUpdatePhase::Pending => all_done = false,
                        _ => {}
                    }
                }
                if all_done
                    || rollout.count_in_phase(HostUpdatePhase::Succeeded)
                        >= rollout.config.canary_count
                {
                    rollout.phase = RolloutPhase::CanaryValidation;
                }
            }
            RolloutPhase::CanaryValidation => {
                // Check canary health — if OK, proceed to full rollout
                let failed = rollout.count_in_phase(HostUpdatePhase::Failed);
                if failed > 0 && rollout.config.auto_rollback {
                    rollout.phase = RolloutPhase::RolledBack;
                } else {
                    // Move remaining pending hosts to the rolling phase
                    let batch = rollout.config.max_concurrent;
                    let mut started = 0;
                    for host in &mut rollout.hosts {
                        if host.phase == HostUpdatePhase::Pending && started < batch {
                            host.phase = HostUpdatePhase::Draining;
                            host.started_at = Some(SystemTime::now());
                            started += 1;
                        }
                    }
                    rollout.phase = RolloutPhase::RollingOut;
                }
            }
            RolloutPhase::RollingOut => {
                // Advance in-progress hosts
                for host in &mut rollout.hosts {
                    match host.phase {
                        HostUpdatePhase::Draining => host.phase = HostUpdatePhase::Updating,
                        HostUpdatePhase::Updating => host.phase = HostUpdatePhase::Verifying,
                        HostUpdatePhase::Verifying => {
                            host.phase = HostUpdatePhase::Succeeded;
                            host.completed_at = Some(SystemTime::now());
                        }
                        _ => {}
                    }
                }

                // Check failure threshold
                if rollout.failure_pct() > rollout.config.max_failure_pct {
                    if rollout.config.auto_rollback {
                        rollout.phase = RolloutPhase::RolledBack;
                        rollout.completed_at = Some(SystemTime::now());
                        return Ok(rollout.phase);
                    } else {
                        rollout.phase = RolloutPhase::Failed;
                        rollout.completed_at = Some(SystemTime::now());
                        return Ok(rollout.phase);
                    }
                }

                // Start next batch of pending hosts
                let active = rollout
                    .hosts
                    .iter()
                    .filter(|h| {
                        matches!(
                            h.phase,
                            HostUpdatePhase::Draining
                                | HostUpdatePhase::Updating
                                | HostUpdatePhase::Verifying
                        )
                    })
                    .count();
                let can_start = rollout.config.max_concurrent.saturating_sub(active);
                let mut started = 0;
                for host in &mut rollout.hosts {
                    if host.phase == HostUpdatePhase::Pending && started < can_start {
                        host.phase = HostUpdatePhase::Draining;
                        host.started_at = Some(SystemTime::now());
                        started += 1;
                    }
                }

                // Check completion
                let pending = rollout.count_in_phase(HostUpdatePhase::Pending);
                let in_progress = active + started;
                if pending == 0 && in_progress == 0 {
                    rollout.phase = RolloutPhase::Completed;
                    rollout.completed_at = Some(SystemTime::now());

                    // Collect data we need before releasing the rollouts lock
                    let artifact = rollout.artifact.clone();
                    let target_version = rollout.target_version.clone();
                    let succeeded: HashSet<String> = rollout
                        .hosts
                        .iter()
                        .filter(|h| h.phase == HostUpdatePhase::Succeeded)
                        .map(|h| h.host_id.clone())
                        .collect();
                    drop(rollouts);

                    // Update installed versions on succeeded hosts
                    let mut hosts = self.hosts.write();
                    for hid in &succeeded {
                        if let Some(host) = hosts.get_mut(hid) {
                            host.installed_versions
                                .insert(artifact.clone(), target_version.clone());
                        }
                    }

                    return Ok(RolloutPhase::Completed);
                }
            }
            _ => {}
        }

        Ok(rollout.phase)
    }

    /// Pause a running rollout
    pub fn pause_rollout(&self, rollout_id: &str) -> FleetResult<()> {
        let mut rollouts = self.rollouts.write();
        let rollout = rollouts
            .get_mut(rollout_id)
            .ok_or_else(|| FleetError::RolloutNotFound(rollout_id.to_string()))?;

        if rollout.is_terminal() {
            return Err(FleetError::InvalidConfig("Rollout is terminal".into()));
        }
        rollout.phase = RolloutPhase::Paused;
        Ok(())
    }

    /// Resume a paused rollout
    pub fn resume_rollout(&self, rollout_id: &str) -> FleetResult<()> {
        let mut rollouts = self.rollouts.write();
        let rollout = rollouts
            .get_mut(rollout_id)
            .ok_or_else(|| FleetError::RolloutNotFound(rollout_id.to_string()))?;

        if rollout.phase != RolloutPhase::Paused {
            return Err(FleetError::InvalidConfig("Rollout is not paused".into()));
        }
        // Resume to rolling phase
        rollout.phase = RolloutPhase::RollingOut;
        Ok(())
    }

    /// Get rollout status
    pub fn get_rollout(&self, rollout_id: &str) -> Option<Rollout> {
        self.rollouts.read().get(rollout_id).cloned()
    }

    /// List all rollouts (active and historical)
    pub fn list_rollouts(&self) -> Vec<Rollout> {
        self.rollouts.read().values().cloned().collect()
    }

    /// Get active rollout count
    pub fn active_rollout_count(&self) -> usize {
        self.rollouts
            .read()
            .values()
            .filter(|r| !r.is_terminal())
            .count()
    }
}

impl Default for FleetManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_fleet() -> FleetManager {
        let fm = FleetManager::new();

        // Register 5 hosts
        for i in 0..5 {
            fm.register_host(
                FleetHost::new(format!("host-{i}"))
                    .tag("region", "us-east-1")
                    .installed(ArtifactKind::GpuDriver, "545.23.08"),
            );
        }

        // Publish target artifact
        fm.publish_artifact(ArtifactVersion::new(
            ArtifactKind::GpuDriver,
            "550.90.07",
            "abcdef1234567890",
        ));
        fm.publish_artifact(ArtifactVersion::new(
            ArtifactKind::GpuDriver,
            "545.23.08",
            "oldsha256",
        ));

        fm
    }

    #[test]
    fn test_host_registration() {
        let fm = setup_fleet();
        assert_eq!(fm.host_count(), 5);
        assert!(fm.get_host("host-0").is_some());
        assert!(fm.get_host("nonexistent").is_none());
    }

    #[test]
    fn test_host_unregister() {
        let fm = setup_fleet();
        fm.unregister_host("host-0");
        assert_eq!(fm.host_count(), 4);
    }

    #[test]
    fn test_artifact_catalog() {
        let fm = setup_fleet();
        let versions = fm.list_versions(&ArtifactKind::GpuDriver);
        assert_eq!(versions.len(), 2);
        assert!(fm
            .get_artifact(&ArtifactKind::GpuDriver, "550.90.07")
            .is_some());
    }

    #[test]
    fn test_create_rollout() {
        let fm = setup_fleet();
        let id = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                None,
                RolloutConfig::default(),
            )
            .unwrap();

        let rollout = fm.get_rollout(&id).unwrap();
        assert_eq!(rollout.hosts.len(), 5);
        assert_eq!(rollout.phase, RolloutPhase::Pending);
        assert_eq!(rollout.target_version, "550.90.07");
    }

    #[test]
    fn test_duplicate_rollout_rejected() {
        let fm = setup_fleet();
        fm.create_rollout(
            ArtifactKind::GpuDriver,
            "550.90.07",
            None,
            RolloutConfig::default(),
        )
        .unwrap();

        let err = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                None,
                RolloutConfig::default(),
            )
            .unwrap_err();
        assert!(matches!(err, FleetError::RolloutInProgress(_)));
    }

    #[test]
    fn test_missing_artifact() {
        let fm = setup_fleet();
        let err = fm
            .create_rollout(
                ArtifactKind::CudaToolkit,
                "12.4",
                None,
                RolloutConfig::default(),
            )
            .unwrap_err();
        assert!(matches!(err, FleetError::ArtifactNotFound(_)));
    }

    #[test]
    fn test_rolling_rollout_to_completion() {
        let fm = setup_fleet();
        let id = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                None,
                RolloutConfig {
                    strategy: RolloutStrategy::Rolling,
                    max_concurrent: 2,
                    ..Default::default()
                },
            )
            .unwrap();

        // Drive rollout until complete
        let mut phases = Vec::new();
        for _ in 0..50 {
            let phase = fm.advance_rollout(&id).unwrap();
            phases.push(phase);
            if phase == RolloutPhase::Completed {
                break;
            }
        }

        assert!(phases.contains(&RolloutPhase::RollingOut));
        assert_eq!(*phases.last().unwrap(), RolloutPhase::Completed);

        let rollout = fm.get_rollout(&id).unwrap();
        let succeeded = rollout.count_in_phase(HostUpdatePhase::Succeeded);
        assert_eq!(succeeded, 5);
    }

    #[test]
    fn test_canary_rollout() {
        let fm = setup_fleet();
        let id = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                None,
                RolloutConfig {
                    strategy: RolloutStrategy::Canary,
                    canary_count: 1,
                    max_concurrent: 5,
                    ..Default::default()
                },
            )
            .unwrap();

        // First advance: Pending -> CanaryInProgress
        let phase = fm.advance_rollout(&id).unwrap();
        assert_eq!(phase, RolloutPhase::CanaryInProgress);

        // Drive to completion
        for _ in 0..50 {
            let phase = fm.advance_rollout(&id).unwrap();
            if phase == RolloutPhase::Completed {
                break;
            }
        }

        let rollout = fm.get_rollout(&id).unwrap();
        assert_eq!(rollout.phase, RolloutPhase::Completed);
    }

    #[test]
    fn test_pause_resume() {
        let fm = setup_fleet();
        let id = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                None,
                RolloutConfig::default(),
            )
            .unwrap();

        fm.advance_rollout(&id).unwrap(); // Start it
        fm.pause_rollout(&id).unwrap();

        let rollout = fm.get_rollout(&id).unwrap();
        assert_eq!(rollout.phase, RolloutPhase::Paused);

        fm.resume_rollout(&id).unwrap();
        let rollout = fm.get_rollout(&id).unwrap();
        assert_eq!(rollout.phase, RolloutPhase::RollingOut);
    }

    #[test]
    fn test_targeted_rollout() {
        let fm = setup_fleet();
        let id = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                Some(vec!["host-0".into(), "host-1".into()]),
                RolloutConfig::default(),
            )
            .unwrap();

        let rollout = fm.get_rollout(&id).unwrap();
        assert_eq!(rollout.hosts.len(), 2);
    }

    #[test]
    fn test_unknown_host_rejected() {
        let fm = setup_fleet();
        let err = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                Some(vec!["nonexistent".into()]),
                RolloutConfig::default(),
            )
            .unwrap_err();
        assert!(matches!(err, FleetError::HostNotFound(_)));
    }

    #[test]
    fn test_host_tag_filter() {
        let fm = setup_fleet();
        let east = fm.hosts_with_tag("region", "us-east-1");
        assert_eq!(east.len(), 5);
        let west = fm.hosts_with_tag("region", "us-west-2");
        assert!(west.is_empty());
    }

    #[test]
    fn test_previous_version_tracking() {
        let fm = setup_fleet();
        let id = fm
            .create_rollout(
                ArtifactKind::GpuDriver,
                "550.90.07",
                None,
                RolloutConfig::default(),
            )
            .unwrap();

        let rollout = fm.get_rollout(&id).unwrap();
        for host in &rollout.hosts {
            assert_eq!(host.previous_version, "545.23.08");
        }
    }

    #[test]
    fn test_active_rollout_count() {
        let fm = setup_fleet();
        assert_eq!(fm.active_rollout_count(), 0);

        fm.create_rollout(
            ArtifactKind::GpuDriver,
            "550.90.07",
            None,
            RolloutConfig::default(),
        )
        .unwrap();
        assert_eq!(fm.active_rollout_count(), 1);
    }
}
