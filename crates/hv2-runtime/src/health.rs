//! Health Monitoring
//!
//! Periodic health checks for VMs in the pool. Detects unresponsive,
//! degraded, or failed VMs and reports them for replacement.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Health check result type
pub type HealthCheckResult<T> = Result<T, HealthCheckError>;

/// Health check errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HealthCheckError {
    /// VM not registered for health monitoring
    #[error("VM not registered: {0}")]
    NotRegistered(String),

    /// Health check timed out
    #[error("Health check timeout: {vm_id} after {elapsed:?}")]
    Timeout { vm_id: String, elapsed: Duration },

    /// Probe failed
    #[error("Probe failed: {vm_id}: {reason}")]
    ProbeFailed { vm_id: String, reason: String },
}

/// Health status of a VM
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HealthStatus {
    /// VM is healthy and responsive
    Healthy,
    /// VM is responding but degraded (slow, high resource usage)
    Degraded,
    /// VM is not responding to health checks
    Unhealthy,
    /// Health status unknown (no check performed yet)
    Unknown,
}

impl HealthStatus {
    /// Check if the VM should be replaced
    pub fn needs_replacement(&self) -> bool {
        matches!(self, Self::Unhealthy)
    }
}

/// Type of health probe
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProbeType {
    /// Liveness: is the VM process running?
    Liveness,
    /// Readiness: can the VM accept new work?
    Readiness,
    /// Startup: has the VM finished booting?
    Startup,
}

/// Result of a single probe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Probe type
    pub probe_type: ProbeType,
    /// Whether the probe succeeded
    pub success: bool,
    /// Response time
    pub response_time: Duration,
    /// Optional detail message
    pub detail: String,
    /// When the probe was executed
    pub checked_at: SystemTime,
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Interval between health checks
    pub check_interval: Duration,
    /// Timeout for each check
    pub check_timeout: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    /// Number of consecutive successes to recover from unhealthy
    pub success_threshold: u32,
    /// Response time threshold for degraded status (ms)
    pub degraded_threshold_ms: u64,
    /// Enable health checking
    pub enabled: bool,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(10),
            check_timeout: Duration::from_secs(5),
            failure_threshold: 3,
            success_threshold: 2,
            degraded_threshold_ms: 1000,
            enabled: true,
        }
    }
}

/// Health state tracked per VM
#[derive(Debug, Clone)]
pub struct VmHealth {
    /// VM ID
    pub vm_id: String,
    /// Current health status
    pub status: HealthStatus,
    /// Consecutive failures
    pub consecutive_failures: u32,
    /// Consecutive successes
    pub consecutive_successes: u32,
    /// Last check time
    last_check: Option<Instant>,
    /// Last successful check
    last_success: Option<Instant>,
    /// History of probe results
    pub probe_history: Vec<ProbeResult>,
    /// Max history entries to keep
    pub max_history: usize,
}

impl VmHealth {
    fn new(vm_id: String) -> Self {
        Self {
            vm_id,
            status: HealthStatus::Unknown,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check: None,
            last_success: None,
            probe_history: Vec::new(),
            max_history: 100,
        }
    }

    /// Record a probe result and update health status
    fn record_probe(&mut self, result: ProbeResult, config: &HealthCheckConfig) {
        self.last_check = Some(Instant::now());

        if result.success {
            self.consecutive_failures = 0;
            self.consecutive_successes += 1;
            self.last_success = Some(Instant::now());

            if result.response_time.as_millis() as u64 > config.degraded_threshold_ms {
                self.status = HealthStatus::Degraded;
            } else if self.consecutive_successes >= config.success_threshold {
                self.status = HealthStatus::Healthy;
            }
        } else {
            self.consecutive_successes = 0;
            self.consecutive_failures += 1;

            if self.consecutive_failures >= config.failure_threshold {
                self.status = HealthStatus::Unhealthy;
            }
        }

        self.probe_history.push(result);
        while self.probe_history.len() > self.max_history {
            self.probe_history.remove(0);
        }
    }

    /// Check if this VM needs a health check now
    fn needs_check(&self, interval: Duration) -> bool {
        self.last_check.is_none_or(|t| t.elapsed() >= interval)
    }
}

/// A health check to perform
#[derive(Debug, Clone)]
pub struct HealthCheck {
    /// VM ID to check
    pub vm_id: String,
    /// Probe type
    pub probe_type: ProbeType,
}

/// Health monitor
///
/// Tracks health state for all VMs in the pool.
pub struct HealthMonitor {
    /// Configuration
    config: HealthCheckConfig,
    /// Per-VM health state
    vm_health: RwLock<HashMap<String, VmHealth>>,
}

impl HealthMonitor {
    /// Create a new health monitor
    pub fn new(config: HealthCheckConfig) -> Self {
        Self {
            config,
            vm_health: RwLock::new(HashMap::new()),
        }
    }

    /// Register a VM for health monitoring
    pub fn register(&self, vm_id: &str) {
        self.vm_health
            .write()
            .entry(vm_id.to_string())
            .or_insert_with(|| VmHealth::new(vm_id.to_string()));
    }

    /// Unregister a VM
    pub fn unregister(&self, vm_id: &str) {
        self.vm_health.write().remove(vm_id);
    }

    /// Get VMs that need health checks
    pub fn due_checks(&self) -> Vec<HealthCheck> {
        if !self.config.enabled {
            return Vec::new();
        }
        let health = self.vm_health.read();
        health
            .values()
            .filter(|h| h.needs_check(self.config.check_interval))
            .map(|h| HealthCheck {
                vm_id: h.vm_id.clone(),
                probe_type: ProbeType::Liveness,
            })
            .collect()
    }

    /// Record a probe result for a VM
    pub fn record_probe(&self, vm_id: &str, result: ProbeResult) -> HealthCheckResult<()> {
        let mut health = self.vm_health.write();
        let vm = health
            .get_mut(vm_id)
            .ok_or_else(|| HealthCheckError::NotRegistered(vm_id.to_string()))?;
        vm.record_probe(result, &self.config);
        Ok(())
    }

    /// Get the health status of a VM
    pub fn status(&self, vm_id: &str) -> Option<HealthStatus> {
        self.vm_health.read().get(vm_id).map(|h| h.status)
    }

    /// Get VMs that need replacement (unhealthy)
    pub fn unhealthy_vms(&self) -> Vec<String> {
        self.vm_health
            .read()
            .values()
            .filter(|h| h.status.needs_replacement())
            .map(|h| h.vm_id.clone())
            .collect()
    }

    /// Get VMs that are degraded
    pub fn degraded_vms(&self) -> Vec<String> {
        self.vm_health
            .read()
            .values()
            .filter(|h| h.status == HealthStatus::Degraded)
            .map(|h| h.vm_id.clone())
            .collect()
    }

    /// Get a summary of all health states
    pub fn summary(&self) -> HashMap<HealthStatus, usize> {
        let mut counts = HashMap::new();
        for h in self.vm_health.read().values() {
            *counts.entry(h.status).or_insert(0) += 1;
        }
        counts
    }

    /// Get detailed health for a VM
    pub fn get_health(&self, vm_id: &str) -> Option<VmHealth> {
        self.vm_health.read().get(vm_id).cloned()
    }

    /// Number of monitored VMs
    pub fn monitored_count(&self) -> usize {
        self.vm_health.read().len()
    }

    /// Get monitor configuration
    pub fn config(&self) -> &HealthCheckConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_monitor() -> HealthMonitor {
        HealthMonitor::new(HealthCheckConfig {
            failure_threshold: 3,
            success_threshold: 2,
            check_interval: Duration::from_millis(1),
            degraded_threshold_ms: 100,
            ..Default::default()
        })
    }

    fn success_probe() -> ProbeResult {
        ProbeResult {
            probe_type: ProbeType::Liveness,
            success: true,
            response_time: Duration::from_millis(5),
            detail: String::new(),
            checked_at: SystemTime::now(),
        }
    }

    fn failure_probe() -> ProbeResult {
        ProbeResult {
            probe_type: ProbeType::Liveness,
            success: false,
            response_time: Duration::from_secs(5),
            detail: "connection refused".to_string(),
            checked_at: SystemTime::now(),
        }
    }

    fn slow_probe() -> ProbeResult {
        ProbeResult {
            probe_type: ProbeType::Liveness,
            success: true,
            response_time: Duration::from_millis(500),
            detail: String::new(),
            checked_at: SystemTime::now(),
        }
    }

    #[test]
    fn test_register_and_status() {
        let monitor = test_monitor();
        monitor.register("vm-1");
        assert_eq!(monitor.status("vm-1"), Some(HealthStatus::Unknown));
        assert_eq!(monitor.monitored_count(), 1);
    }

    #[test]
    fn test_unregister() {
        let monitor = test_monitor();
        monitor.register("vm-1");
        monitor.unregister("vm-1");
        assert_eq!(monitor.monitored_count(), 0);
    }

    #[test]
    fn test_healthy_after_successes() {
        let monitor = test_monitor(); // success_threshold = 2
        monitor.register("vm-1");

        monitor.record_probe("vm-1", success_probe()).unwrap();
        assert_ne!(monitor.status("vm-1"), Some(HealthStatus::Healthy)); // Need 2

        monitor.record_probe("vm-1", success_probe()).unwrap();
        assert_eq!(monitor.status("vm-1"), Some(HealthStatus::Healthy));
    }

    #[test]
    fn test_unhealthy_after_failures() {
        let monitor = test_monitor(); // failure_threshold = 3
        monitor.register("vm-1");

        for _ in 0..2 {
            monitor.record_probe("vm-1", failure_probe()).unwrap();
        }
        assert_ne!(monitor.status("vm-1"), Some(HealthStatus::Unhealthy));

        monitor.record_probe("vm-1", failure_probe()).unwrap();
        assert_eq!(monitor.status("vm-1"), Some(HealthStatus::Unhealthy));
    }

    #[test]
    fn test_degraded_on_slow_response() {
        let monitor = test_monitor(); // degraded_threshold = 100ms
        monitor.register("vm-1");

        monitor.record_probe("vm-1", slow_probe()).unwrap(); // 500ms > 100ms
        assert_eq!(monitor.status("vm-1"), Some(HealthStatus::Degraded));
    }

    #[test]
    fn test_recovery_from_unhealthy() {
        let monitor = test_monitor();
        monitor.register("vm-1");

        // Make unhealthy
        for _ in 0..3 {
            monitor.record_probe("vm-1", failure_probe()).unwrap();
        }
        assert_eq!(monitor.status("vm-1"), Some(HealthStatus::Unhealthy));

        // Recover
        for _ in 0..2 {
            monitor.record_probe("vm-1", success_probe()).unwrap();
        }
        assert_eq!(monitor.status("vm-1"), Some(HealthStatus::Healthy));
    }

    #[test]
    fn test_unhealthy_vms() {
        let monitor = test_monitor();
        monitor.register("vm-1");
        monitor.register("vm-2");

        // Make vm-1 unhealthy
        for _ in 0..3 {
            monitor.record_probe("vm-1", failure_probe()).unwrap();
        }
        // Make vm-2 healthy
        for _ in 0..2 {
            monitor.record_probe("vm-2", success_probe()).unwrap();
        }

        let unhealthy = monitor.unhealthy_vms();
        assert_eq!(unhealthy, vec!["vm-1"]);
    }

    #[test]
    fn test_due_checks() {
        let monitor = test_monitor();
        monitor.register("vm-1");

        // Should need check immediately (never checked)
        std::thread::sleep(Duration::from_millis(5));
        let due = monitor.due_checks();
        assert_eq!(due.len(), 1);

        // After checking, should not need another immediately
        monitor.record_probe("vm-1", success_probe()).unwrap();
        let due = monitor.due_checks();
        assert_eq!(due.len(), 0);
    }

    #[test]
    fn test_summary() {
        let monitor = test_monitor();
        monitor.register("vm-1");
        monitor.register("vm-2");
        monitor.register("vm-3");

        // vm-1: healthy
        for _ in 0..2 {
            monitor.record_probe("vm-1", success_probe()).unwrap();
        }
        // vm-2: unhealthy
        for _ in 0..3 {
            monitor.record_probe("vm-2", failure_probe()).unwrap();
        }
        // vm-3: unknown (no probes)

        let summary = monitor.summary();
        assert_eq!(summary.get(&HealthStatus::Healthy), Some(&1));
        assert_eq!(summary.get(&HealthStatus::Unhealthy), Some(&1));
        assert_eq!(summary.get(&HealthStatus::Unknown), Some(&1));
    }

    #[test]
    fn test_not_registered_error() {
        let monitor = test_monitor();
        let err = monitor
            .record_probe("unknown", success_probe())
            .unwrap_err();
        assert!(matches!(err, HealthCheckError::NotRegistered(_)));
    }

    #[test]
    fn test_disabled_no_checks() {
        let monitor = HealthMonitor::new(HealthCheckConfig {
            enabled: false,
            ..Default::default()
        });
        monitor.register("vm-1");
        let due = monitor.due_checks();
        assert!(due.is_empty());
    }
}
