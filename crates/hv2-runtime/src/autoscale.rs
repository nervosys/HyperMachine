//! Autoscaler
//!
//! Monitors pool utilization and workload queue depth to make scaling
//! decisions. Produces `AutoscaleDecision` values that the caller
//! applies to the VM pool — the autoscaler itself does not create
//! or destroy VMs directly.
//!
//! # Policies
//!
//! - **TargetUtilization**: Scale to keep pool utilization at a target %
//! - **QueueDepth**: Scale based on pending workload queue length
//! - **StepFunction**: Fixed thresholds for scaling up/down

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Autoscale operation result
pub type AutoscaleResult<T> = Result<T, AutoscaleError>;

/// Autoscale errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AutoscaleError {
    /// Invalid configuration
    #[error("Invalid config: {0}")]
    InvalidConfig(String),

    /// Cooldown in effect
    #[error("Cooldown: {remaining:?} remaining")]
    Cooldown { remaining: Duration },
}

/// Scaling direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleDirection {
    /// Add VMs
    Up,
    /// Remove VMs
    Down,
    /// No change
    None,
}

/// Why a scaling decision was made
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScaleReason {
    /// High utilization
    HighUtilization { current: f64, target: f64 },
    /// Low utilization
    LowUtilization { current: f64, target: f64 },
    /// Queue depth exceeded threshold
    QueueDepth { current: usize, threshold: usize },
    /// Below minimum VMs
    BelowMinimum { current: usize, minimum: usize },
    /// Above maximum VMs
    AboveMaximum { current: usize, maximum: usize },
    /// Warm pool deficit
    WarmDeficit { warm: usize, min_warm: usize },
    /// Manual override
    Manual,
}

/// Autoscale policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AutoscalePolicy {
    /// Scale to maintain target utilization percentage
    #[default]
    TargetUtilization,
    /// Scale based on work queue depth
    QueueDepth,
    /// Fixed step thresholds
    StepFunction,
}

/// Autoscale configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoscaleConfig {
    /// Scaling policy
    pub policy: AutoscalePolicy,
    /// Minimum VMs (never scale below this)
    pub min_vms: usize,
    /// Maximum VMs (never scale above this)
    pub max_vms: usize,
    /// Target utilization (0.0 - 1.0) for TargetUtilization policy
    pub target_utilization: f64,
    /// Queue depth threshold for QueueDepth policy
    pub queue_depth_threshold: usize,
    /// VMs to add per scale-up event
    pub scale_up_increment: usize,
    /// VMs to remove per scale-down event
    pub scale_down_increment: usize,
    /// Cooldown after scaling up
    pub scale_up_cooldown: Duration,
    /// Cooldown after scaling down
    pub scale_down_cooldown: Duration,
    /// Evaluation interval
    pub evaluation_interval: Duration,
    /// Enable autoscaling
    pub enabled: bool,
}

impl Default for AutoscaleConfig {
    fn default() -> Self {
        Self {
            policy: AutoscalePolicy::TargetUtilization,
            min_vms: 2,
            max_vms: 64,
            target_utilization: 0.7,
            queue_depth_threshold: 10,
            scale_up_increment: 2,
            scale_down_increment: 1,
            scale_up_cooldown: Duration::from_secs(60),
            scale_down_cooldown: Duration::from_secs(300),
            evaluation_interval: Duration::from_secs(15),
            enabled: true,
        }
    }
}

/// Metrics snapshot for autoscale evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoscaleMetrics {
    /// Total VMs in pool
    pub total_vms: usize,
    /// VMs currently assigned (doing work)
    pub assigned_vms: usize,
    /// Warm (idle) VMs
    pub warm_vms: usize,
    /// Pending workloads in queue
    pub pending_workloads: usize,
    /// Utilization ratio (assigned / total)
    pub utilization: f64,
    /// Timestamp
    pub timestamp: SystemTime,
}

impl AutoscaleMetrics {
    /// Create metrics from pool stats
    pub fn from_pool(total: usize, assigned: usize, warm: usize, pending: usize) -> Self {
        let utilization = if total > 0 {
            assigned as f64 / total as f64
        } else {
            0.0
        };
        Self {
            total_vms: total,
            assigned_vms: assigned,
            warm_vms: warm,
            pending_workloads: pending,
            utilization,
            timestamp: SystemTime::now(),
        }
    }
}

/// A scaling decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoscaleDecision {
    /// Direction of scaling
    pub direction: ScaleDirection,
    /// Number of VMs to add or remove
    pub count: usize,
    /// Reason for the decision
    pub reason: ScaleReason,
    /// When the decision was made
    pub decided_at: SystemTime,
}

/// Scale event for history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleEvent {
    /// Decision that was made
    pub decision: AutoscaleDecision,
    /// Metrics at the time of decision
    pub metrics: AutoscaleMetrics,
    /// Whether the decision was applied
    pub applied: bool,
}

/// Autoscaler
///
/// Evaluates pool metrics against the configured policy and produces
/// scaling decisions. Thread-safe.
pub struct Autoscaler {
    /// Configuration
    config: AutoscaleConfig,
    /// Last scale-up time
    last_scale_up: RwLock<Option<Instant>>,
    /// Last scale-down time
    last_scale_down: RwLock<Option<Instant>>,
    /// Scaling history
    history: RwLock<VecDeque<ScaleEvent>>,
}

impl Autoscaler {
    /// Create a new autoscaler
    pub fn new(config: AutoscaleConfig) -> Self {
        Self {
            config,
            last_scale_up: RwLock::new(None),
            last_scale_down: RwLock::new(None),
            history: RwLock::new(VecDeque::new()),
        }
    }

    /// Evaluate current metrics and produce a scaling decision
    pub fn evaluate(&self, metrics: &AutoscaleMetrics) -> AutoscaleDecision {
        if !self.config.enabled {
            return AutoscaleDecision {
                direction: ScaleDirection::None,
                count: 0,
                reason: ScaleReason::Manual,
                decided_at: SystemTime::now(),
            };
        }

        // Check minimum
        if metrics.total_vms < self.config.min_vms {
            let deficit = self.config.min_vms - metrics.total_vms;
            return AutoscaleDecision {
                direction: ScaleDirection::Up,
                count: deficit,
                reason: ScaleReason::BelowMinimum {
                    current: metrics.total_vms,
                    minimum: self.config.min_vms,
                },
                decided_at: SystemTime::now(),
            };
        }

        // Check maximum
        if metrics.total_vms > self.config.max_vms {
            let excess = metrics.total_vms - self.config.max_vms;
            return AutoscaleDecision {
                direction: ScaleDirection::Down,
                count: excess,
                reason: ScaleReason::AboveMaximum {
                    current: metrics.total_vms,
                    maximum: self.config.max_vms,
                },
                decided_at: SystemTime::now(),
            };
        }

        // Policy-specific evaluation
        match self.config.policy {
            AutoscalePolicy::TargetUtilization => self.evaluate_utilization(metrics),
            AutoscalePolicy::QueueDepth => self.evaluate_queue_depth(metrics),
            AutoscalePolicy::StepFunction => self.evaluate_step_function(metrics),
        }
    }

    /// Record that a decision was applied
    pub fn record(&self, decision: AutoscaleDecision, metrics: AutoscaleMetrics) {
        match decision.direction {
            ScaleDirection::Up => *self.last_scale_up.write() = Some(Instant::now()),
            ScaleDirection::Down => *self.last_scale_down.write() = Some(Instant::now()),
            ScaleDirection::None => {}
        }

        let event = ScaleEvent {
            decision,
            metrics,
            applied: true,
        };
        let mut history = self.history.write();
        history.push_back(event);
        while history.len() > 1000 {
            history.pop_front();
        }
    }

    /// Check if scale-up is in cooldown
    pub fn is_scale_up_cooldown(&self) -> bool {
        self.last_scale_up
            .read()
            .is_some_and(|t| t.elapsed() < self.config.scale_up_cooldown)
    }

    /// Check if scale-down is in cooldown
    pub fn is_scale_down_cooldown(&self) -> bool {
        self.last_scale_down
            .read()
            .is_some_and(|t| t.elapsed() < self.config.scale_down_cooldown)
    }

    /// Get scaling history
    pub fn history(&self, count: usize) -> Vec<ScaleEvent> {
        let history = self.history.read();
        history.iter().rev().take(count).cloned().collect()
    }

    /// Get the autoscale configuration
    pub fn config(&self) -> &AutoscaleConfig {
        &self.config
    }

    fn evaluate_utilization(&self, metrics: &AutoscaleMetrics) -> AutoscaleDecision {
        let target = self.config.target_utilization;

        if metrics.utilization > target + 0.1 && !self.is_scale_up_cooldown() {
            AutoscaleDecision {
                direction: ScaleDirection::Up,
                count: self.config.scale_up_increment,
                reason: ScaleReason::HighUtilization {
                    current: metrics.utilization,
                    target,
                },
                decided_at: SystemTime::now(),
            }
        } else if metrics.utilization < target - 0.2
            && metrics.total_vms > self.config.min_vms
            && !self.is_scale_down_cooldown()
        {
            let count = self
                .config
                .scale_down_increment
                .min(metrics.total_vms - self.config.min_vms);
            AutoscaleDecision {
                direction: ScaleDirection::Down,
                count,
                reason: ScaleReason::LowUtilization {
                    current: metrics.utilization,
                    target,
                },
                decided_at: SystemTime::now(),
            }
        } else {
            AutoscaleDecision {
                direction: ScaleDirection::None,
                count: 0,
                reason: ScaleReason::HighUtilization {
                    current: metrics.utilization,
                    target,
                },
                decided_at: SystemTime::now(),
            }
        }
    }

    fn evaluate_queue_depth(&self, metrics: &AutoscaleMetrics) -> AutoscaleDecision {
        if metrics.pending_workloads > self.config.queue_depth_threshold
            && !self.is_scale_up_cooldown()
        {
            AutoscaleDecision {
                direction: ScaleDirection::Up,
                count: self.config.scale_up_increment,
                reason: ScaleReason::QueueDepth {
                    current: metrics.pending_workloads,
                    threshold: self.config.queue_depth_threshold,
                },
                decided_at: SystemTime::now(),
            }
        } else if metrics.pending_workloads == 0
            && metrics.warm_vms > 2
            && metrics.total_vms > self.config.min_vms
            && !self.is_scale_down_cooldown()
        {
            let count = self
                .config
                .scale_down_increment
                .min(metrics.total_vms - self.config.min_vms);
            AutoscaleDecision {
                direction: ScaleDirection::Down,
                count,
                reason: ScaleReason::QueueDepth {
                    current: 0,
                    threshold: self.config.queue_depth_threshold,
                },
                decided_at: SystemTime::now(),
            }
        } else {
            AutoscaleDecision {
                direction: ScaleDirection::None,
                count: 0,
                reason: ScaleReason::QueueDepth {
                    current: metrics.pending_workloads,
                    threshold: self.config.queue_depth_threshold,
                },
                decided_at: SystemTime::now(),
            }
        }
    }

    fn evaluate_step_function(&self, metrics: &AutoscaleMetrics) -> AutoscaleDecision {
        // Simple step function: if utilization > 80%, scale up; if < 30%, scale down
        if metrics.utilization > 0.8 && !self.is_scale_up_cooldown() {
            AutoscaleDecision {
                direction: ScaleDirection::Up,
                count: self.config.scale_up_increment,
                reason: ScaleReason::HighUtilization {
                    current: metrics.utilization,
                    target: 0.8,
                },
                decided_at: SystemTime::now(),
            }
        } else if metrics.utilization < 0.3
            && metrics.total_vms > self.config.min_vms
            && !self.is_scale_down_cooldown()
        {
            let count = self
                .config
                .scale_down_increment
                .min(metrics.total_vms - self.config.min_vms);
            AutoscaleDecision {
                direction: ScaleDirection::Down,
                count,
                reason: ScaleReason::LowUtilization {
                    current: metrics.utilization,
                    target: 0.3,
                },
                decided_at: SystemTime::now(),
            }
        } else {
            AutoscaleDecision {
                direction: ScaleDirection::None,
                count: 0,
                reason: ScaleReason::HighUtilization {
                    current: metrics.utilization,
                    target: 0.5,
                },
                decided_at: SystemTime::now(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_autoscaler() -> Autoscaler {
        Autoscaler::new(AutoscaleConfig {
            min_vms: 2,
            max_vms: 10,
            target_utilization: 0.7,
            scale_up_increment: 2,
            scale_down_increment: 1,
            scale_up_cooldown: Duration::from_millis(1),
            scale_down_cooldown: Duration::from_millis(1),
            ..Default::default()
        })
    }

    #[test]
    fn test_below_minimum() {
        let autoscaler = test_autoscaler();
        let metrics = AutoscaleMetrics::from_pool(1, 0, 1, 0);
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::Up);
        assert_eq!(decision.count, 1); // Need 1 more to reach min=2
    }

    #[test]
    fn test_above_maximum() {
        let autoscaler = test_autoscaler();
        let metrics = AutoscaleMetrics::from_pool(12, 10, 2, 0);
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::Down);
        assert_eq!(decision.count, 2); // 12 - 10 = 2 excess
    }

    #[test]
    fn test_high_utilization_scale_up() {
        let autoscaler = test_autoscaler();
        // Wait for cooldown to expire
        std::thread::sleep(Duration::from_millis(5));

        let metrics = AutoscaleMetrics::from_pool(4, 4, 0, 0); // 100% util
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::Up);
        assert_eq!(decision.count, 2);
    }

    #[test]
    fn test_low_utilization_scale_down() {
        let autoscaler = test_autoscaler();
        std::thread::sleep(Duration::from_millis(5));

        let metrics = AutoscaleMetrics::from_pool(8, 2, 6, 0); // 25% util
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::Down);
        assert_eq!(decision.count, 1);
    }

    #[test]
    fn test_stable_no_change() {
        let autoscaler = test_autoscaler();
        let metrics = AutoscaleMetrics::from_pool(4, 3, 1, 0); // 75% util ~ target
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::None);
    }

    #[test]
    fn test_queue_depth_policy() {
        let autoscaler = Autoscaler::new(AutoscaleConfig {
            policy: AutoscalePolicy::QueueDepth,
            queue_depth_threshold: 5,
            scale_up_cooldown: Duration::from_millis(1),
            scale_down_cooldown: Duration::from_millis(1),
            ..Default::default()
        });
        std::thread::sleep(Duration::from_millis(5));

        let metrics = AutoscaleMetrics::from_pool(4, 4, 0, 10); // 10 pending > thresh 5
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::Up);
    }

    #[test]
    fn test_disabled() {
        let autoscaler = Autoscaler::new(AutoscaleConfig {
            enabled: false,
            ..Default::default()
        });
        let metrics = AutoscaleMetrics::from_pool(1, 1, 0, 100);
        let decision = autoscaler.evaluate(&metrics);

        assert_eq!(decision.direction, ScaleDirection::None);
    }

    #[test]
    fn test_record_history() {
        let autoscaler = test_autoscaler();
        let metrics = AutoscaleMetrics::from_pool(4, 4, 0, 0);
        let decision = autoscaler.evaluate(&metrics);
        autoscaler.record(decision, metrics);

        let history = autoscaler.history(10);
        assert_eq!(history.len(), 1);
        assert!(history[0].applied);
    }

    #[test]
    fn test_scale_down_respects_minimum() {
        let autoscaler = test_autoscaler(); // min=2
        std::thread::sleep(Duration::from_millis(5));

        let metrics = AutoscaleMetrics::from_pool(2, 0, 2, 0); // At minimum already
        let decision = autoscaler.evaluate(&metrics);

        // Should not scale down below minimum
        assert_ne!(decision.direction, ScaleDirection::Down);
    }

    #[test]
    fn test_metrics_from_pool() {
        let m = AutoscaleMetrics::from_pool(10, 7, 3, 5);
        assert_eq!(m.total_vms, 10);
        assert_eq!(m.assigned_vms, 7);
        assert!((m.utilization - 0.7).abs() < f64::EPSILON);
    }
}
