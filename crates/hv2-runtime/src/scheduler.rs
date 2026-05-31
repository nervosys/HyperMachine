//! Workload Scheduler
//!
//! Places agent workloads onto VM pool members using configurable
//! strategies: bin-packing (maximize utilization), spread (maximize
//! isolation), affinity (co-locate related workloads), or custom scoring.
//!
//! The scheduler is decoupled from the pool — it computes placement
//! decisions that the caller applies to the pool.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::pool::VmEntry;

/// Schedule operation result
pub type ScheduleResult<T> = Result<T, ScheduleError>;

/// Schedule errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// No suitable VM for placement
    #[error("No placement found: {0}")]
    NoPlacement(String),

    /// Constraint violation
    #[error("Constraint violated: {0}")]
    ConstraintViolated(String),

    /// Workload not found
    #[error("Workload not found: {0}")]
    WorkloadNotFound(String),

    /// Too many pending workloads
    #[error("Queue full: {current}/{max} pending")]
    QueueFull { current: usize, max: usize },

    /// Invalid configuration
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
}

/// Placement strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlacementStrategy {
    /// Pack workloads into fewest VMs (maximize utilization)
    BinPack,
    /// Spread workloads across VMs (maximize isolation)
    Spread,
    /// Place on VM with lowest latency / best fit
    #[default]
    BestFit,
    /// Random placement (for testing)
    Random,
}

/// Scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Default placement strategy
    pub strategy: PlacementStrategy,
    /// Maximum pending workloads in the queue
    pub max_pending: usize,
    /// Scheduling interval
    pub schedule_interval: Duration,
    /// Enable preemption (evict lower-priority work for higher-priority)
    pub enable_preemption: bool,
    /// Maximum retries for failed placements
    pub max_retries: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            strategy: PlacementStrategy::BestFit,
            max_pending: 1000,
            schedule_interval: Duration::from_millis(100),
            enable_preemption: false,
            max_retries: 3,
        }
    }
}

/// Describes a workload that needs placement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadDescriptor {
    /// Unique workload ID
    pub id: String,
    /// Agent session requesting placement
    pub session_id: String,
    /// Required vCPUs
    pub required_vcpus: u32,
    /// Required memory in bytes
    pub required_memory: u64,
    /// Requires GPU
    pub requires_gpu: bool,
    /// Priority (higher = more important)
    pub priority: u32,
    /// Constraints on placement
    pub constraints: Vec<PlacementConstraint>,
    /// When this workload was submitted
    pub submitted_at: SystemTime,
    /// Maximum time to wait for placement
    pub placement_timeout: Duration,
    /// Number of placement attempts so far
    pub attempts: u32,
}

impl WorkloadDescriptor {
    /// Create a new workload descriptor
    pub fn new(id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            session_id: session_id.into(),
            required_vcpus: 1,
            required_memory: 512 * 1024 * 1024, // 512 MB
            requires_gpu: false,
            priority: 50,
            constraints: Vec::new(),
            submitted_at: SystemTime::now(),
            placement_timeout: Duration::from_secs(30),
            attempts: 0,
        }
    }

    /// Set required vCPUs
    pub fn vcpus(mut self, count: u32) -> Self {
        self.required_vcpus = count;
        self
    }

    /// Set required memory
    pub fn memory(mut self, bytes: u64) -> Self {
        self.required_memory = bytes;
        self
    }

    /// Require GPU
    pub fn gpu(mut self, required: bool) -> Self {
        self.requires_gpu = required;
        self
    }

    /// Set priority
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Add a placement constraint
    pub fn constraint(mut self, constraint: PlacementConstraint) -> Self {
        self.constraints.push(constraint);
        self
    }
}

/// Placement constraint
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementConstraint {
    /// Must be placed on a specific VM
    RequireVm(String),
    /// Must NOT be placed on these VMs
    ExcludeVms(Vec<String>),
    /// Must be co-located with another workload's session
    AffinityWith(String),
    /// Must NOT be co-located with another workload's session
    AntiAffinityWith(String),
    /// Require a minimum number of vCPUs on the VM
    MinVcpus(u32),
    /// Require a minimum amount of memory on the VM
    MinMemory(u64),
}

/// Placement decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    /// Workload being placed
    pub workload_id: String,
    /// VM selected for placement
    pub vm_id: String,
    /// Score for this placement (0.0 - 1.0)
    pub score: f64,
    /// Strategy used
    pub strategy: PlacementStrategy,
    /// When this decision was made
    pub decided_at: SystemTime,
}

/// Scoring breakdown for a placement candidate
#[derive(Debug, Clone)]
pub struct PlacementScore {
    /// VM ID
    pub vm_id: String,
    /// Resource fit score (0.0 - 1.0)
    pub resource_fit: f64,
    /// Constraint satisfaction (0.0 or 1.0)
    pub constraint_score: f64,
    /// Strategy-specific score (0.0 - 1.0)
    pub strategy_score: f64,
    /// Combined score
    pub total: f64,
}

impl PlacementScore {
    fn compute(resource_fit: f64, constraint_score: f64, strategy_score: f64) -> Self {
        Self {
            vm_id: String::new(),
            resource_fit,
            constraint_score,
            // Hard constraints: if violated or resource doesn't fit, total = 0
            total: if constraint_score < 1.0 || resource_fit < f64::EPSILON {
                0.0
            } else {
                resource_fit * 0.4 + strategy_score * 0.6
            },
            strategy_score,
        }
    }
}

/// Workload scheduler
///
/// Computes placement decisions for workloads onto available VMs.
pub struct Scheduler {
    /// Configuration
    config: SchedulerConfig,
    /// Pending workloads awaiting placement
    pending: RwLock<Vec<WorkloadDescriptor>>,
    /// Active placements (workload_id -> Placement)
    active: RwLock<HashMap<String, Placement>>,
    /// Placement history for analytics
    history: RwLock<Vec<Placement>>,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            pending: RwLock::new(Vec::new()),
            active: RwLock::new(HashMap::new()),
            history: RwLock::new(Vec::new()),
        }
    }

    /// Submit a workload for scheduling
    pub fn submit(&self, workload: WorkloadDescriptor) -> ScheduleResult<()> {
        let mut pending = self.pending.write();
        if pending.len() >= self.config.max_pending {
            return Err(ScheduleError::QueueFull {
                current: pending.len(),
                max: self.config.max_pending,
            });
        }
        pending.push(workload);
        // Sort by priority (highest first)
        pending.sort_by_key(|b| std::cmp::Reverse(b.priority));
        Ok(())
    }

    /// Schedule the next pending workload against available VMs
    ///
    /// Returns a placement decision if a suitable VM is found.
    pub fn schedule_next(&self, available_vms: &[VmEntry]) -> ScheduleResult<Option<Placement>> {
        let mut pending = self.pending.write();
        if pending.is_empty() {
            return Ok(None);
        }

        // Try to place the highest-priority workload
        let workload = &pending[0];
        let placement = self.find_placement(workload, available_vms)?;

        // Remove from pending and add to active
        let workload = pending.remove(0);
        let mut active = self.active.write();
        active.insert(workload.id.clone(), placement.clone());
        self.history.write().push(placement.clone());

        Ok(Some(placement))
    }

    /// Schedule all pending workloads at once (batch scheduling)
    pub fn schedule_batch(&self, available_vms: &[VmEntry]) -> Vec<ScheduleResult<Placement>> {
        let mut pending = self.pending.write();
        let mut results = Vec::new();
        let mut used_vms: HashSet<String> = HashSet::new();

        // Collect VMs already assigned
        let active = self.active.read();
        for p in active.values() {
            used_vms.insert(p.vm_id.clone());
        }
        drop(active);

        let mut remaining = Vec::new();

        for workload in pending.drain(..) {
            // Filter out VMs already used in this batch (for Spread strategy)
            let candidate_vms: Vec<&VmEntry> = available_vms
                .iter()
                .filter(|vm| !used_vms.contains(&vm.id))
                .collect();

            match self.find_placement_from(&workload, &candidate_vms) {
                Ok(placement) => {
                    used_vms.insert(placement.vm_id.clone());
                    self.active
                        .write()
                        .insert(workload.id.clone(), placement.clone());
                    self.history.write().push(placement.clone());
                    results.push(Ok(placement));
                }
                Err(e) => {
                    remaining.push(workload);
                    results.push(Err(e));
                }
            }
        }

        // Put back any workloads that couldn't be placed
        *pending = remaining;
        results
    }

    /// Complete a workload (remove from active tracking)
    pub fn complete(&self, workload_id: &str) -> ScheduleResult<Placement> {
        self.active
            .write()
            .remove(workload_id)
            .ok_or_else(|| ScheduleError::WorkloadNotFound(workload_id.to_string()))
    }

    /// Get pending workload count
    pub fn pending_count(&self) -> usize {
        self.pending.read().len()
    }

    /// Get active placement count
    pub fn active_count(&self) -> usize {
        self.active.read().len()
    }

    /// Get the scheduler configuration
    pub fn config(&self) -> &SchedulerConfig {
        &self.config
    }

    /// Find the best placement for a workload
    fn find_placement(
        &self,
        workload: &WorkloadDescriptor,
        available_vms: &[VmEntry],
    ) -> ScheduleResult<Placement> {
        let refs: Vec<&VmEntry> = available_vms.iter().collect();
        self.find_placement_from(workload, &refs)
    }

    fn find_placement_from(
        &self,
        workload: &WorkloadDescriptor,
        available_vms: &[&VmEntry],
    ) -> ScheduleResult<Placement> {
        let mut best_score: Option<PlacementScore> = None;
        let mut best_vm_id: Option<String> = None;

        for vm in available_vms {
            if !vm.state.is_assignable() {
                continue;
            }

            let score = self.score_placement(workload, vm);
            if score.total > 0.0
                && (best_score.is_none() || score.total > best_score.as_ref().unwrap().total)
            {
                best_vm_id = Some(vm.id.clone());
                best_score = Some(score);
            }
        }

        match (best_vm_id, best_score) {
            (Some(vm_id), Some(score)) => Ok(Placement {
                workload_id: workload.id.clone(),
                vm_id,
                score: score.total,
                strategy: self.config.strategy,
                decided_at: SystemTime::now(),
            }),
            _ => Err(ScheduleError::NoPlacement(format!(
                "No VM satisfies workload {} (need {}vCPU/{}MB{})",
                workload.id,
                workload.required_vcpus,
                workload.required_memory / (1024 * 1024),
                if workload.requires_gpu { "/GPU" } else { "" },
            ))),
        }
    }

    fn score_placement(&self, workload: &WorkloadDescriptor, vm: &VmEntry) -> PlacementScore {
        // Resource fit: does the VM have what we need?
        let cpu_fit = if vm.vcpus >= workload.required_vcpus {
            1.0
        } else {
            0.0
        };
        let mem_fit = if vm.memory >= workload.required_memory {
            1.0
        } else {
            0.0
        };
        let gpu_fit = if workload.requires_gpu && !vm.gpu {
            0.0
        } else {
            1.0
        };
        let resource_fit = cpu_fit * mem_fit * gpu_fit;

        // Constraint check
        let constraint_score = self.check_constraints(workload, vm);

        // Strategy-specific scoring
        let strategy_score = match self.config.strategy {
            PlacementStrategy::BinPack => {
                // Prefer VMs with more assignments (pack tightly)
                let utilization = vm.assignment_count as f64 / 100.0;
                utilization.min(1.0)
            }
            PlacementStrategy::Spread => {
                // Prefer VMs with fewer assignments (spread out)
                let utilization = vm.assignment_count as f64 / 100.0;
                (1.0 - utilization).max(0.0)
            }
            PlacementStrategy::BestFit => {
                // Prefer VMs that are the closest match (least waste)
                let cpu_efficiency = workload.required_vcpus as f64 / vm.vcpus as f64;
                let mem_efficiency = workload.required_memory as f64 / vm.memory as f64;
                (cpu_efficiency + mem_efficiency) / 2.0
            }
            PlacementStrategy::Random => 0.5,
        };

        let mut score = PlacementScore::compute(resource_fit, constraint_score, strategy_score);
        score.vm_id = vm.id.clone();
        score
    }

    fn check_constraints(&self, workload: &WorkloadDescriptor, vm: &VmEntry) -> f64 {
        for constraint in &workload.constraints {
            match constraint {
                PlacementConstraint::RequireVm(required_id) => {
                    if vm.id != *required_id {
                        return 0.0;
                    }
                }
                PlacementConstraint::ExcludeVms(excluded) => {
                    if excluded.contains(&vm.id) {
                        return 0.0;
                    }
                }
                PlacementConstraint::AffinityWith(session) => {
                    // Check if the target session is on this VM
                    if vm.assignee.as_ref() != Some(session) {
                        return 0.0;
                    }
                }
                PlacementConstraint::AntiAffinityWith(session) => {
                    if vm.assignee.as_ref() == Some(session) {
                        return 0.0;
                    }
                }
                PlacementConstraint::MinVcpus(min) => {
                    if vm.vcpus < *min {
                        return 0.0;
                    }
                }
                PlacementConstraint::MinMemory(min) => {
                    if vm.memory < *min {
                        return 0.0;
                    }
                }
            }
        }
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::VmSlotState;

    fn test_vms() -> Vec<VmEntry> {
        vec![
            VmEntry {
                id: "vm-1".to_string(),
                state: VmSlotState::Warm,
                assignment_count: 0,
                assignee: None,
                vcpus: 4,
                memory: 4 * 1024 * 1024 * 1024,
                gpu: false,
            },
            VmEntry {
                id: "vm-2".to_string(),
                state: VmSlotState::Warm,
                assignment_count: 2,
                assignee: None,
                vcpus: 8,
                memory: 16 * 1024 * 1024 * 1024,
                gpu: true,
            },
            VmEntry {
                id: "vm-3".to_string(),
                state: VmSlotState::Assigned,
                assignment_count: 1,
                assignee: Some("existing-session".to_string()),
                vcpus: 2,
                memory: 2 * 1024 * 1024 * 1024,
                gpu: false,
            },
        ]
    }

    #[test]
    fn test_submit_and_schedule() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        let workload = WorkloadDescriptor::new("w1", "s1").vcpus(2);
        scheduler.submit(workload).unwrap();

        let vms = test_vms();
        let placement = scheduler.schedule_next(&vms).unwrap().unwrap();

        assert_eq!(placement.workload_id, "w1");
        // Should not place on vm-3 (Assigned state)
        assert_ne!(placement.vm_id, "vm-3");
        assert!(placement.score > 0.0);
    }

    #[test]
    fn test_gpu_constraint() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        let workload = WorkloadDescriptor::new("w1", "s1").gpu(true);
        scheduler.submit(workload).unwrap();

        let vms = test_vms();
        let placement = scheduler.schedule_next(&vms).unwrap().unwrap();

        assert_eq!(placement.vm_id, "vm-2"); // Only VM with GPU
    }

    #[test]
    fn test_exclude_constraint() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        let workload = WorkloadDescriptor::new("w1", "s1")
            .constraint(PlacementConstraint::ExcludeVms(vec!["vm-1".to_string()]));
        scheduler.submit(workload).unwrap();

        let vms = test_vms();
        let placement = scheduler.schedule_next(&vms).unwrap().unwrap();

        assert_eq!(placement.vm_id, "vm-2"); // vm-1 excluded, vm-3 assigned
    }

    #[test]
    fn test_require_vm_constraint() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        let workload = WorkloadDescriptor::new("w1", "s1")
            .constraint(PlacementConstraint::RequireVm("vm-1".to_string()));
        scheduler.submit(workload).unwrap();

        let vms = test_vms();
        let placement = scheduler.schedule_next(&vms).unwrap().unwrap();

        assert_eq!(placement.vm_id, "vm-1");
    }

    #[test]
    fn test_no_placement() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        let workload = WorkloadDescriptor::new("w1", "s1").vcpus(128); // Impossible
        scheduler.submit(workload).unwrap();

        let vms = test_vms();
        let err = scheduler.schedule_next(&vms).unwrap_err();
        assert!(matches!(err, ScheduleError::NoPlacement(_)));
    }

    #[test]
    fn test_queue_full() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_pending: 2,
            ..Default::default()
        });
        scheduler
            .submit(WorkloadDescriptor::new("w1", "s1"))
            .unwrap();
        scheduler
            .submit(WorkloadDescriptor::new("w2", "s2"))
            .unwrap();
        let err = scheduler
            .submit(WorkloadDescriptor::new("w3", "s3"))
            .unwrap_err();
        assert!(matches!(err, ScheduleError::QueueFull { .. }));
    }

    #[test]
    fn test_priority_ordering() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        scheduler
            .submit(WorkloadDescriptor::new("low", "s1").priority(10))
            .unwrap();
        scheduler
            .submit(WorkloadDescriptor::new("high", "s2").priority(90))
            .unwrap();

        let vms = test_vms();
        let placement = scheduler.schedule_next(&vms).unwrap().unwrap();

        // High priority should be scheduled first
        assert_eq!(placement.workload_id, "high");
    }

    #[test]
    fn test_batch_scheduling() {
        let scheduler = Scheduler::new(SchedulerConfig {
            strategy: PlacementStrategy::Spread,
            ..Default::default()
        });
        scheduler
            .submit(WorkloadDescriptor::new("w1", "s1"))
            .unwrap();
        scheduler
            .submit(WorkloadDescriptor::new("w2", "s2"))
            .unwrap();

        let vms = test_vms();
        let results = scheduler.schedule_batch(&vms);

        assert_eq!(results.len(), 2);
        let placed: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
        assert_eq!(placed.len(), 2);
        // With spread strategy, should use different VMs
        assert_ne!(placed[0].vm_id, placed[1].vm_id);
    }

    #[test]
    fn test_complete_workload() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        scheduler
            .submit(WorkloadDescriptor::new("w1", "s1"))
            .unwrap();

        let vms = test_vms();
        scheduler.schedule_next(&vms).unwrap();
        assert_eq!(scheduler.active_count(), 1);

        scheduler.complete("w1").unwrap();
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn test_complete_unknown() {
        let scheduler = Scheduler::new(SchedulerConfig::default());
        let err = scheduler.complete("unknown").unwrap_err();
        assert!(matches!(err, ScheduleError::WorkloadNotFound(_)));
    }
}
