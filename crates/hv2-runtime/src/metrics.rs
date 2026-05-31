//! Runtime Metrics & Observability
//!
//! Collects structured metrics from all runtime subsystems and exposes
//! them in Prometheus text exposition format. The metrics collector is
//! read-only — it snapshots subsystem state without locking them out.
//!
//! # Metric Types
//!
//! | Type      | Description                          | Example                     |
//! |-----------|--------------------------------------|-----------------------------|
//! | Gauge     | Current value (can go up/down)       | `pool_total_vms`            |
//! | Counter   | Monotonically increasing count       | `sessions_created_total`    |
//! | Histogram | Distribution with configurable bins  | `workload_latency_seconds`  |
//!
//! # Prometheus Endpoint
//!
//! The `/metrics` API endpoint calls `MetricsCollector::collect()` to
//! produce a `RuntimeMetrics` snapshot, then renders it via
//! `to_prometheus()` into text exposition format for scraping.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

// ============================================================================
// Metric Primitives
// ============================================================================

/// A monotonically increasing counter
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Create a new counter starting at 0
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Increment the counter by 1
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the counter by a given amount
    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Get the current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// A value that can go up or down
#[derive(Debug)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    /// Create a new gauge at 0
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Set the gauge to a specific value
    pub fn set(&self, val: u64) {
        self.value.store(val, Ordering::Relaxed);
    }

    /// Get the current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Increment by 1
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement by 1 (saturating)
    pub fn dec(&self) {
        // Saturating decrement via CAS loop
        loop {
            let current = self.value.load(Ordering::Relaxed);
            if current == 0 {
                break;
            }
            if self
                .value
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

/// A histogram that tracks value distribution across configurable buckets
#[derive(Debug)]
pub struct Histogram {
    /// Bucket upper bounds (sorted ascending)
    buckets: Vec<f64>,
    /// Count of observations in each bucket (cumulative)
    counts: Vec<AtomicU64>,
    /// Total number of observations
    count: AtomicU64,
    /// Sum of all observed values (stored as bits for atomic access)
    sum_bits: AtomicU64,
}

impl Histogram {
    /// Create a histogram with the given bucket boundaries
    pub fn new(buckets: Vec<f64>) -> Self {
        let counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            buckets,
            counts,
            count: AtomicU64::new(0),
            sum_bits: AtomicU64::new(f64::to_bits(0.0)),
        }
    }

    /// Create a histogram with default latency buckets (in seconds)
    pub fn with_latency_buckets() -> Self {
        Self::new(vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ])
    }

    /// Create a histogram with default size buckets
    pub fn with_size_buckets() -> Self {
        Self::new(vec![
            1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0,
        ])
    }

    /// Record an observation
    pub fn observe(&self, value: f64) {
        self.count.fetch_add(1, Ordering::Relaxed);

        // Add to sum via CAS loop
        loop {
            let old_bits = self.sum_bits.load(Ordering::Relaxed);
            let old = f64::from_bits(old_bits);
            let new = old + value;
            if self
                .sum_bits
                .compare_exchange_weak(
                    old_bits,
                    f64::to_bits(new),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Increment all buckets where value <= bound
        for (i, bound) in self.buckets.iter().enumerate() {
            if value <= *bound {
                self.counts[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Get a snapshot of the histogram
    pub fn snapshot(&self) -> HistogramSnapshot {
        HistogramSnapshot {
            buckets: self
                .buckets
                .iter()
                .zip(self.counts.iter())
                .map(|(bound, count)| (*bound, count.load(Ordering::Relaxed)))
                .collect(),
            count: self.count.load(Ordering::Relaxed),
            sum: f64::from_bits(self.sum_bits.load(Ordering::Relaxed)),
        }
    }
}

/// Immutable snapshot of histogram state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramSnapshot {
    /// (upper_bound, cumulative_count) pairs
    pub buckets: Vec<(f64, u64)>,
    /// Total observations
    pub count: u64,
    /// Sum of all observations
    pub sum: f64,
}

impl HistogramSnapshot {
    /// Mean of observed values (returns 0 if no observations)
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }
}

// ============================================================================
// Runtime Metrics Snapshot
// ============================================================================

/// Complete metrics snapshot from all runtime subsystems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    // ── Pool Gauges ──────────────────────────────────────────────
    /// Total VMs in pool
    pub pool_total: u64,
    /// Warm (idle) VMs
    pub pool_warm: u64,
    /// Assigned VMs
    pub pool_assigned: u64,
    /// Draining VMs
    pub pool_draining: u64,
    /// Recycling VMs
    pub pool_recycling: u64,
    /// Failed VMs
    pub pool_failed: u64,
    /// Provisioning VMs
    pub pool_provisioning: u64,

    // ── Pool Counters ────────────────────────────────────────────
    /// Total assignments ever
    pub pool_total_assignments: u64,
    /// Total recycles ever
    pub pool_total_recycles: u64,

    // ── Scheduler Gauges ─────────────────────────────────────────
    /// Pending workloads in queue
    pub scheduler_pending: u64,

    // ── Workflow Gauges ──────────────────────────────────────────
    /// Active workflow executions
    pub workflows_active: u64,

    // ── Gateway Gauges ───────────────────────────────────────────
    /// Active gateway routes (sessions)
    pub gateway_routes: u64,

    // ── Health Gauges ────────────────────────────────────────────
    /// Healthy VMs
    pub health_healthy: u64,
    /// Degraded VMs
    pub health_degraded: u64,
    /// Unhealthy VMs
    pub health_unhealthy: u64,
    /// Unknown health VMs
    pub health_unknown: u64,

    // ── Billing Gauges ───────────────────────────────────────────
    /// Active billing sessions
    pub billing_sessions: u64,

    // ── Store Gauges ─────────────────────────────────────────────
    /// Entries in durable store
    pub store_entries: u64,

    // ── Autoscale ────────────────────────────────────────────────
    /// Whether autoscaler is enabled
    pub autoscale_enabled: bool,
    /// Current autoscale policy
    pub autoscale_policy: String,
    /// Total scale-up events in history
    pub autoscale_ups: u64,
    /// Total scale-down events in history
    pub autoscale_downs: u64,

    // ── Lifecycle Counters ───────────────────────────────────────
    /// Total sessions created (tracked by metrics collector)
    pub sessions_created_total: u64,
    /// Total sessions destroyed (tracked by metrics collector)
    pub sessions_destroyed_total: u64,
    /// Total maintenance ticks run
    pub maintenance_ticks_total: u64,

    // ── Timing ───────────────────────────────────────────────────
    /// Runtime uptime in seconds
    pub uptime_seconds: u64,
    /// When this snapshot was taken
    pub collected_at: SystemTime,

    /// Runtime instance ID
    pub instance_id: String,
}

impl RuntimeMetrics {
    /// Render metrics in Prometheus text exposition format
    pub fn to_prometheus(&self) -> String {
        let mut out = String::with_capacity(4096);

        // Helper macro
        macro_rules! gauge {
            ($name:expr, $help:expr, $val:expr) => {
                out.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} gauge\n{} {}\n",
                    $name, $help, $name, $name, $val
                ));
            };
        }

        macro_rules! counter {
            ($name:expr, $help:expr, $val:expr) => {
                out.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} counter\n{} {}\n",
                    $name, $help, $name, $name, $val
                ));
            };
        }

        // Pool gauges
        gauge!(
            "hm_pool_total_vms",
            "Total VMs in the pool",
            self.pool_total
        );
        gauge!("hm_pool_warm_vms", "Warm (idle) VMs", self.pool_warm);
        gauge!(
            "hm_pool_assigned_vms",
            "Assigned VMs doing work",
            self.pool_assigned
        );
        gauge!(
            "hm_pool_draining_vms",
            "VMs draining before recycle",
            self.pool_draining
        );
        gauge!(
            "hm_pool_recycling_vms",
            "VMs being recycled",
            self.pool_recycling
        );
        gauge!("hm_pool_failed_vms", "Failed VMs", self.pool_failed);
        gauge!(
            "hm_pool_provisioning_vms",
            "VMs being provisioned",
            self.pool_provisioning
        );

        // Pool counters
        counter!(
            "hm_pool_assignments_total",
            "Total VM assignments ever",
            self.pool_total_assignments
        );
        counter!(
            "hm_pool_recycles_total",
            "Total VM recycles ever",
            self.pool_total_recycles
        );

        // Scheduler
        gauge!(
            "hm_scheduler_pending_workloads",
            "Pending workloads in scheduler queue",
            self.scheduler_pending
        );

        // Workflows
        gauge!(
            "hm_workflows_active",
            "Active workflow executions",
            self.workflows_active
        );

        // Gateway
        gauge!(
            "hm_gateway_active_routes",
            "Active gateway routes (sessions)",
            self.gateway_routes
        );

        // Health
        gauge!(
            "hm_health_healthy_vms",
            "VMs in healthy state",
            self.health_healthy
        );
        gauge!(
            "hm_health_degraded_vms",
            "VMs in degraded state",
            self.health_degraded
        );
        gauge!(
            "hm_health_unhealthy_vms",
            "VMs in unhealthy state",
            self.health_unhealthy
        );
        gauge!(
            "hm_health_unknown_vms",
            "VMs with unknown health",
            self.health_unknown
        );

        // Billing
        gauge!(
            "hm_billing_active_sessions",
            "Active billing sessions",
            self.billing_sessions
        );

        // Store
        gauge!(
            "hm_store_entries",
            "Entries in durable store",
            self.store_entries
        );

        // Autoscale
        gauge!(
            "hm_autoscale_enabled",
            "Whether autoscaler is enabled",
            if self.autoscale_enabled { 1 } else { 0 }
        );
        counter!(
            "hm_autoscale_ups_total",
            "Total scale-up events",
            self.autoscale_ups
        );
        counter!(
            "hm_autoscale_downs_total",
            "Total scale-down events",
            self.autoscale_downs
        );

        // Lifecycle counters
        counter!(
            "hm_sessions_created_total",
            "Total sessions created",
            self.sessions_created_total
        );
        counter!(
            "hm_sessions_destroyed_total",
            "Total sessions destroyed",
            self.sessions_destroyed_total
        );
        counter!(
            "hm_maintenance_ticks_total",
            "Total maintenance ticks run",
            self.maintenance_ticks_total
        );

        // Uptime
        gauge!(
            "hm_uptime_seconds",
            "Runtime uptime in seconds",
            self.uptime_seconds
        );

        out
    }
}

impl fmt::Display for RuntimeMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== HyperMachine Runtime Metrics ===")?;
        writeln!(f)?;
        writeln!(
            f,
            "Pool:      total={} warm={} assigned={} failed={}",
            self.pool_total, self.pool_warm, self.pool_assigned, self.pool_failed
        )?;
        writeln!(f, "Scheduler: pending={}", self.scheduler_pending)?;
        writeln!(f, "Workflows: active={}", self.workflows_active)?;
        writeln!(f, "Gateway:   routes={}", self.gateway_routes)?;
        writeln!(
            f,
            "Health:    healthy={} degraded={} unhealthy={} unknown={}",
            self.health_healthy, self.health_degraded, self.health_unhealthy, self.health_unknown
        )?;
        writeln!(f, "Billing:   sessions={}", self.billing_sessions)?;
        writeln!(f, "Store:     entries={}", self.store_entries)?;
        writeln!(
            f,
            "Autoscale: enabled={} policy={} ups={} downs={}",
            self.autoscale_enabled, self.autoscale_policy, self.autoscale_ups, self.autoscale_downs
        )?;
        writeln!(f, "Uptime:    {}s", self.uptime_seconds)?;
        Ok(())
    }
}

// ============================================================================
// Metrics Collector
// ============================================================================

/// Collects metrics from a Runtime instance
///
/// The collector maintains its own lifecycle counters (sessions created,
/// maintenance ticks) and snapshots subsystem state on demand.
pub struct MetricsCollector {
    /// Monotonic counters for lifecycle events
    sessions_created: Counter,
    sessions_destroyed: Counter,
    maintenance_ticks: Counter,
    /// When the collector was created (== runtime start time)
    started_at: Instant,
    /// Runtime instance ID
    instance_id: String,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new(instance_id: impl Into<String>) -> Self {
        Self {
            sessions_created: Counter::new(),
            sessions_destroyed: Counter::new(),
            maintenance_ticks: Counter::new(),
            started_at: Instant::now(),
            instance_id: instance_id.into(),
        }
    }

    /// Record a session creation event
    pub fn on_session_created(&self) {
        self.sessions_created.inc();
    }

    /// Record a session destruction event
    pub fn on_session_destroyed(&self) {
        self.sessions_destroyed.inc();
    }

    /// Record a maintenance tick
    pub fn on_maintenance_tick(&self) {
        self.maintenance_ticks.inc();
    }

    /// Get the runtime uptime
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Get total sessions created
    pub fn sessions_created(&self) -> u64 {
        self.sessions_created.get()
    }

    /// Get total sessions destroyed
    pub fn sessions_destroyed(&self) -> u64 {
        self.sessions_destroyed.get()
    }

    /// Get total maintenance ticks
    pub fn maintenance_ticks(&self) -> u64 {
        self.maintenance_ticks.get()
    }

    /// Collect a full metrics snapshot from a Runtime
    ///
    /// This reads current state from all subsystems and combines it
    /// with the collector's own lifecycle counters.
    pub fn collect(&self, runtime: &crate::Runtime) -> RuntimeMetrics {
        // Pool stats
        let pool_stats = runtime.pool().stats();

        // Health summary
        let health_summary = runtime.health_monitor().summary();

        // Autoscale history
        let scale_history = runtime.autoscaler().history(1000);
        let autoscale_ups = scale_history
            .iter()
            .filter(|e| e.decision.direction == crate::ScaleDirection::Up)
            .count() as u64;
        let autoscale_downs = scale_history
            .iter()
            .filter(|e| e.decision.direction == crate::ScaleDirection::Down)
            .count() as u64;

        RuntimeMetrics {
            // Pool gauges
            pool_total: pool_stats.total as u64,
            pool_warm: pool_stats.warm as u64,
            pool_assigned: pool_stats.assigned as u64,
            pool_draining: pool_stats.draining as u64,
            pool_recycling: pool_stats.recycling as u64,
            pool_failed: pool_stats.failed as u64,
            pool_provisioning: pool_stats.provisioning as u64,

            // Pool counters
            pool_total_assignments: pool_stats.total_assignments,
            pool_total_recycles: pool_stats.total_recycles,

            // Scheduler
            scheduler_pending: runtime.scheduler().pending_count() as u64,

            // Workflows
            workflows_active: runtime.workflow_engine().active_count() as u64,

            // Gateway
            gateway_routes: runtime.gateway().route_count() as u64,

            // Health
            health_healthy: *health_summary
                .get(&crate::HealthStatus::Healthy)
                .unwrap_or(&0) as u64,
            health_degraded: *health_summary
                .get(&crate::HealthStatus::Degraded)
                .unwrap_or(&0) as u64,
            health_unhealthy: *health_summary
                .get(&crate::HealthStatus::Unhealthy)
                .unwrap_or(&0) as u64,
            health_unknown: *health_summary
                .get(&crate::HealthStatus::Unknown)
                .unwrap_or(&0) as u64,

            // Billing
            billing_sessions: runtime.billing_engine().session_count() as u64,

            // Store
            store_entries: runtime.store().len() as u64,

            // Autoscale
            autoscale_enabled: runtime.autoscaler().config().enabled,
            autoscale_policy: format!("{:?}", runtime.autoscaler().config().policy),
            autoscale_ups,
            autoscale_downs,

            // Lifecycle counters
            sessions_created_total: self.sessions_created.get(),
            sessions_destroyed_total: self.sessions_destroyed.get(),
            maintenance_ticks_total: self.maintenance_ticks.get(),

            // Timing
            uptime_seconds: self.started_at.elapsed().as_secs(),
            collected_at: SystemTime::now(),

            instance_id: self.instance_id.clone(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Counter Tests ────────────────────────────────────────────

    #[test]
    fn test_counter_new() {
        let c = Counter::new();
        assert_eq!(c.get(), 0);
    }

    #[test]
    fn test_counter_inc() {
        let c = Counter::new();
        c.inc();
        c.inc();
        c.inc();
        assert_eq!(c.get(), 3);
    }

    #[test]
    fn test_counter_inc_by() {
        let c = Counter::new();
        c.inc_by(10);
        c.inc_by(5);
        assert_eq!(c.get(), 15);
    }

    // ── Gauge Tests ──────────────────────────────────────────────

    #[test]
    fn test_gauge_new() {
        let g = Gauge::new();
        assert_eq!(g.get(), 0);
    }

    #[test]
    fn test_gauge_set() {
        let g = Gauge::new();
        g.set(42);
        assert_eq!(g.get(), 42);
    }

    #[test]
    fn test_gauge_inc_dec() {
        let g = Gauge::new();
        g.inc();
        g.inc();
        g.inc();
        assert_eq!(g.get(), 3);
        g.dec();
        assert_eq!(g.get(), 2);
    }

    #[test]
    fn test_gauge_dec_saturates() {
        let g = Gauge::new();
        g.dec(); // Should not underflow
        assert_eq!(g.get(), 0);
    }

    // ── Histogram Tests ──────────────────────────────────────────

    #[test]
    fn test_histogram_empty() {
        let h = Histogram::with_latency_buckets();
        let snap = h.snapshot();
        assert_eq!(snap.count, 0);
        assert_eq!(snap.sum, 0.0);
        assert_eq!(snap.mean(), 0.0);
    }

    #[test]
    fn test_histogram_observe() {
        let h = Histogram::new(vec![1.0, 5.0, 10.0]);
        h.observe(0.5);
        h.observe(3.0);
        h.observe(7.0);

        let snap = h.snapshot();
        assert_eq!(snap.count, 3);
        assert!((snap.sum - 10.5).abs() < f64::EPSILON);

        // Bucket counts: 0.5 <= 1.0 (1), 3.0 <= 5.0 (2), 7.0 <= 10.0 (3)
        assert_eq!(snap.buckets[0], (1.0, 1)); // <= 1.0
        assert_eq!(snap.buckets[1], (5.0, 2)); // <= 5.0
        assert_eq!(snap.buckets[2], (10.0, 3)); // <= 10.0
    }

    #[test]
    fn test_histogram_mean() {
        let h = Histogram::new(vec![10.0]);
        h.observe(2.0);
        h.observe(4.0);
        h.observe(6.0);

        let snap = h.snapshot();
        assert!((snap.mean() - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_histogram_latency_buckets() {
        let h = Histogram::with_latency_buckets();
        let snap = h.snapshot();
        assert_eq!(snap.buckets.len(), 12);
        assert_eq!(snap.buckets[0].0, 0.001);
        assert_eq!(snap.buckets[11].0, 10.0);
    }

    #[test]
    fn test_histogram_size_buckets() {
        let h = Histogram::with_size_buckets();
        let snap = h.snapshot();
        assert_eq!(snap.buckets.len(), 9);
    }

    // ── HistogramSnapshot Tests ──────────────────────────────────

    #[test]
    fn test_snapshot_mean_empty() {
        let snap = HistogramSnapshot {
            buckets: vec![],
            count: 0,
            sum: 0.0,
        };
        assert_eq!(snap.mean(), 0.0);
    }

    // ── MetricsCollector Tests ───────────────────────────────────

    #[test]
    fn test_collector_new() {
        let mc = MetricsCollector::new("test-instance");
        assert_eq!(mc.sessions_created(), 0);
        assert_eq!(mc.sessions_destroyed(), 0);
        assert_eq!(mc.maintenance_ticks(), 0);
    }

    #[test]
    fn test_collector_lifecycle_counters() {
        let mc = MetricsCollector::new("test-instance");
        mc.on_session_created();
        mc.on_session_created();
        mc.on_session_created();
        mc.on_session_destroyed();
        mc.on_maintenance_tick();
        mc.on_maintenance_tick();

        assert_eq!(mc.sessions_created(), 3);
        assert_eq!(mc.sessions_destroyed(), 1);
        assert_eq!(mc.maintenance_ticks(), 2);
    }

    #[test]
    fn test_collector_uptime() {
        let mc = MetricsCollector::new("test-instance");
        std::thread::sleep(Duration::from_millis(10));
        assert!(mc.uptime().as_millis() >= 10);
    }

    #[test]
    fn test_collector_collect_from_runtime() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new(runtime.instance_id());

        // Create some state
        for _ in 0..3 {
            let _ = runtime.pool().provision();
        }

        collector.on_session_created();
        collector.on_maintenance_tick();

        let metrics = collector.collect(&runtime);

        assert_eq!(metrics.pool_total, 3);
        assert_eq!(metrics.sessions_created_total, 1);
        assert_eq!(metrics.maintenance_ticks_total, 1);
        assert!(metrics.autoscale_enabled);
        assert_eq!(metrics.autoscale_policy, "TargetUtilization");
        assert!(!metrics.instance_id.is_empty());
    }

    #[test]
    fn test_collect_pool_states() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        // Provision and assign
        let vm_id = runtime.pool().provision().unwrap();
        runtime.pool().mark_warm(&vm_id).unwrap();
        runtime.pool().acquire("session-1").unwrap();

        let metrics = collector.collect(&runtime);
        assert_eq!(metrics.pool_total, 1);
        assert_eq!(metrics.pool_assigned, 1);
        assert_eq!(metrics.pool_warm, 0);
        assert_eq!(metrics.pool_total_assignments, 1);
    }

    #[test]
    fn test_collect_health_states() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        // Register some VMs for health monitoring
        runtime.health_monitor().register("vm-1");
        runtime.health_monitor().register("vm-2");

        let metrics = collector.collect(&runtime);
        let total_health = metrics.health_healthy
            + metrics.health_degraded
            + metrics.health_unhealthy
            + metrics.health_unknown;
        assert_eq!(total_health, 2);
    }

    #[test]
    fn test_collect_gateway_routes() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        // Add some routes
        let _ = runtime
            .gateway()
            .route("session-1", &["vm-1".to_string(), "vm-2".to_string()]);

        let metrics = collector.collect(&runtime);
        assert_eq!(metrics.gateway_routes, 1);
    }

    #[test]
    fn test_collect_billing_sessions() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        runtime
            .billing_engine()
            .register_session("s-1", crate::BillingTier::Standard);
        runtime
            .billing_engine()
            .register_session("s-2", crate::BillingTier::Enterprise);

        let metrics = collector.collect(&runtime);
        assert_eq!(metrics.billing_sessions, 2);
    }

    #[test]
    fn test_collect_store_entries() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        let _ = runtime.store().put("key-1", b"value-1".to_vec());
        let _ = runtime.store().put("key-2", b"value-2".to_vec());
        let _ = runtime.store().put("key-3", b"value-3".to_vec());

        let metrics = collector.collect(&runtime);
        assert_eq!(metrics.store_entries, 3);
    }

    // ── Prometheus Output Tests ──────────────────────────────────

    #[test]
    fn test_prometheus_format() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        let metrics = collector.collect(&runtime);
        let prom = metrics.to_prometheus();

        // Should contain HELP and TYPE annotations
        assert!(prom.contains("# HELP hm_pool_total_vms"));
        assert!(prom.contains("# TYPE hm_pool_total_vms gauge"));
        assert!(prom.contains("# HELP hm_pool_assignments_total"));
        assert!(prom.contains("# TYPE hm_pool_assignments_total counter"));
        assert!(prom.contains("# HELP hm_uptime_seconds"));
        assert!(prom.contains("hm_autoscale_enabled 1"));
    }

    #[test]
    fn test_prometheus_format_values() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        // Create some VMs
        for _ in 0..5 {
            let _ = runtime.pool().provision();
        }

        let metrics = collector.collect(&runtime);
        let prom = metrics.to_prometheus();

        assert!(prom.contains("hm_pool_total_vms 5"));
    }

    // ── Display Tests ────────────────────────────────────────────

    #[test]
    fn test_display_format() {
        let config = crate::RuntimeConfig::default();
        let runtime = crate::Runtime::new(config);
        let collector = MetricsCollector::new("test");

        let metrics = collector.collect(&runtime);
        let display = format!("{}", metrics);

        assert!(display.contains("HyperMachine Runtime Metrics"));
        assert!(display.contains("Pool:"));
        assert!(display.contains("Scheduler:"));
        assert!(display.contains("Health:"));
        assert!(display.contains("Autoscale:"));
    }
}
