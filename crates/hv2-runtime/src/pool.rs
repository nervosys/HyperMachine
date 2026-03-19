//! VM Pool Management
//!
//! Manages a pool of virtual machines with warm standby, cold start,
//! and recycling. VMs are pre-created and held warm so agent workloads
//! can be assigned instantly without cold-start latency.
//!
//! # Pool Lifecycle
//!
//! ```text
//! ┌──────────┐    acquire()    ┌──────────┐    release()    ┌──────────┐
//! │   Warm   │ ──────────────> │ Assigned │ ──────────────> │ Draining │
//! │ (idle)   │                 │ (in use) │                 │(cooldown)│
//! └──────────┘                 └──────────┘                 └─────┬────┘
//!      ▲                                                         │
//!      └──────────────── recycle() ──────────────────────────────┘
//! ```

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Pool operation result
pub type PoolResult<T> = Result<T, PoolError>;

/// Pool operation errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PoolError {
    /// Pool is at capacity
    #[error("Pool at capacity: {current}/{max}")]
    AtCapacity { current: usize, max: usize },

    /// No warm VMs available
    #[error("No warm VMs available (warm: {warm}, total: {total})")]
    NoWarmVms { warm: usize, total: usize },

    /// VM not found in pool
    #[error("VM not found: {0}")]
    VmNotFound(String),

    /// Invalid state transition
    #[error("Invalid state transition: {from:?} -> {to:?}")]
    InvalidTransition { from: VmSlotState, to: VmSlotState },

    /// VM creation failed
    #[error("VM creation failed: {0}")]
    CreationFailed(String),

    /// Pool is shut down
    #[error("Pool is shut down")]
    Shutdown,
}

/// State of a VM slot in the pool
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VmSlotState {
    /// VM is booting up
    Provisioning,
    /// VM is warm and ready for assignment
    Warm,
    /// VM is assigned to an agent session
    Assigned,
    /// VM is draining (finishing work before recycle)
    Draining,
    /// VM is being recycled (state wiped, preparing for re-use)
    Recycling,
    /// VM has failed and needs replacement
    Failed,
    /// VM is being terminated
    Terminating,
}

impl VmSlotState {
    /// Check if the VM can accept new work
    pub fn is_assignable(&self) -> bool {
        matches!(self, Self::Warm)
    }

    /// Check if the VM is in a terminal or error state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Failed | Self::Terminating)
    }

    /// Check if the VM is actively doing work
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Assigned | Self::Draining)
    }
}

/// Pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum number of warm (idle) VMs to maintain
    pub min_warm: usize,
    /// Maximum total VMs in the pool
    pub max_size: usize,
    /// Default VM configuration for pool members
    pub default_vcpus: u32,
    /// Default memory per VM in bytes
    pub default_memory: u64,
    /// Enable GPU for pool VMs
    pub default_gpu: bool,
    /// Maximum time a VM stays warm before recycling
    pub max_idle_time: Duration,
    /// Maximum lifetime of any VM before forced recycling
    pub max_lifetime: Duration,
    /// Cooldown period before recycled VM is re-warmed
    pub recycle_cooldown: Duration,
    /// Maximum assignments before a VM must be recycled (0 = unlimited)
    pub max_assignments: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_warm: 2,
            max_size: 64,
            default_vcpus: 2,
            default_memory: 2 * 1024 * 1024 * 1024, // 2 GB
            default_gpu: false,
            max_idle_time: Duration::from_secs(600), // 10 minutes
            max_lifetime: Duration::from_secs(3600 * 24), // 24 hours
            recycle_cooldown: Duration::from_secs(5),
            max_assignments: 0,
        }
    }
}

/// A VM slot in the pool
#[derive(Debug, Clone)]
pub struct VmSlot {
    /// Unique slot ID
    pub id: String,
    /// Current state
    pub state: VmSlotState,
    /// When this slot was created
    pub created_at: Instant,
    /// When the state last changed
    pub state_changed_at: Instant,
    /// Number of times this VM has been assigned
    pub assignment_count: u64,
    /// Current assignee (agent session ID)
    pub assignee: Option<String>,
    /// VM configuration override (if any)
    pub vcpus: u32,
    /// Memory in bytes
    pub memory: u64,
    /// GPU enabled
    pub gpu: bool,
}

impl VmSlot {
    /// Create a new provisioning slot
    fn new(config: &PoolConfig) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4().to_string(),
            state: VmSlotState::Provisioning,
            created_at: now,
            state_changed_at: now,
            assignment_count: 0,
            assignee: None,
            vcpus: config.default_vcpus,
            memory: config.default_memory,
            gpu: config.default_gpu,
        }
    }

    /// Transition to a new state
    fn transition(&mut self, new_state: VmSlotState) -> PoolResult<()> {
        let valid = matches!(
            (self.state, new_state),
            (VmSlotState::Provisioning, VmSlotState::Warm)
                | (VmSlotState::Provisioning, VmSlotState::Failed)
                | (VmSlotState::Warm, VmSlotState::Assigned)
                | (VmSlotState::Warm, VmSlotState::Recycling)
                | (VmSlotState::Warm, VmSlotState::Terminating)
                | (VmSlotState::Assigned, VmSlotState::Draining)
                | (VmSlotState::Assigned, VmSlotState::Failed)
                | (VmSlotState::Draining, VmSlotState::Recycling)
                | (VmSlotState::Draining, VmSlotState::Terminating)
                | (VmSlotState::Draining, VmSlotState::Failed)
                | (VmSlotState::Recycling, VmSlotState::Warm)
                | (VmSlotState::Recycling, VmSlotState::Failed)
                | (VmSlotState::Recycling, VmSlotState::Terminating)
                | (VmSlotState::Failed, VmSlotState::Terminating)
        );

        if !valid {
            return Err(PoolError::InvalidTransition {
                from: self.state,
                to: new_state,
            });
        }

        self.state = new_state;
        self.state_changed_at = Instant::now();
        Ok(())
    }

    /// How long the VM has been in its current state
    pub fn time_in_state(&self) -> Duration {
        self.state_changed_at.elapsed()
    }

    /// Total lifetime of this VM slot
    pub fn lifetime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Check if idle time exceeded
    pub fn is_idle_expired(&self, max_idle: Duration) -> bool {
        self.state == VmSlotState::Warm && self.time_in_state() > max_idle
    }

    /// Check if lifetime exceeded
    pub fn is_lifetime_expired(&self, max_lifetime: Duration) -> bool {
        self.lifetime() > max_lifetime
    }
}

/// A borrowed view of a VM entry for external consumers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmEntry {
    /// Slot ID
    pub id: String,
    /// Current state
    pub state: VmSlotState,
    /// Assignment count
    pub assignment_count: u64,
    /// Current assignee
    pub assignee: Option<String>,
    /// vCPU count
    pub vcpus: u32,
    /// Memory in bytes
    pub memory: u64,
    /// GPU enabled
    pub gpu: bool,
}

impl From<&VmSlot> for VmEntry {
    fn from(slot: &VmSlot) -> Self {
        Self {
            id: slot.id.clone(),
            state: slot.state,
            assignment_count: slot.assignment_count,
            assignee: slot.assignee.clone(),
            vcpus: slot.vcpus,
            memory: slot.memory,
            gpu: slot.gpu,
        }
    }
}

/// Pool statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolStats {
    /// Total VMs in the pool
    pub total: usize,
    /// VMs in warm (idle) state
    pub warm: usize,
    /// VMs currently assigned
    pub assigned: usize,
    /// VMs draining
    pub draining: usize,
    /// VMs being recycled
    pub recycling: usize,
    /// VMs in failed state
    pub failed: usize,
    /// VMs being provisioned
    pub provisioning: usize,
    /// Total assignments ever made
    pub total_assignments: u64,
    /// Total recycles ever done
    pub total_recycles: u64,
}

/// Pool events for observability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEvent {
    /// Event type
    pub kind: PoolEventKind,
    /// VM slot ID
    pub vm_id: String,
    /// Timestamp
    pub timestamp: SystemTime,
    /// Additional context
    pub detail: String,
}

/// Pool event kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolEventKind {
    /// VM provisioned and warming up
    VmProvisioned,
    /// VM became warm
    VmWarmed,
    /// VM assigned to agent
    VmAssigned,
    /// VM released by agent
    VmReleased,
    /// VM recycled
    VmRecycled,
    /// VM failed
    VmFailed,
    /// VM terminated
    VmTerminated,
}

/// VM pool manager
///
/// Manages a fleet of VMs with warm standby, assignment, and recycling.
/// Thread-safe — all operations are internally synchronized.
pub struct VmPool {
    /// Pool configuration
    config: PoolConfig,
    /// VM slots indexed by ID
    slots: RwLock<HashMap<String, VmSlot>>,
    /// Queue of warm VM IDs (FIFO for fair rotation)
    warm_queue: RwLock<VecDeque<String>>,
    /// Event log (bounded ring buffer)
    events: RwLock<VecDeque<PoolEvent>>,
    /// Cumulative stats
    total_assignments: RwLock<u64>,
    total_recycles: RwLock<u64>,
}

impl VmPool {
    /// Create a new VM pool with the given configuration
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            slots: RwLock::new(HashMap::new()),
            warm_queue: RwLock::new(VecDeque::new()),
            events: RwLock::new(VecDeque::new()),
            total_assignments: RwLock::new(0),
            total_recycles: RwLock::new(0),
        }
    }

    /// Provision a new VM slot and add it to the pool
    pub fn provision(&self) -> PoolResult<String> {
        let slots = self.slots.read();
        if slots.len() >= self.config.max_size {
            return Err(PoolError::AtCapacity {
                current: slots.len(),
                max: self.config.max_size,
            });
        }
        drop(slots);

        let slot = VmSlot::new(&self.config);
        let id = slot.id.clone();

        self.slots.write().insert(id.clone(), slot);
        self.emit_event(PoolEventKind::VmProvisioned, &id, "Provisioning new VM");
        Ok(id)
    }

    /// Mark a provisioning VM as warm (ready for use)
    pub fn mark_warm(&self, vm_id: &str) -> PoolResult<()> {
        let mut slots = self.slots.write();
        let slot = slots
            .get_mut(vm_id)
            .ok_or_else(|| PoolError::VmNotFound(vm_id.to_string()))?;
        slot.transition(VmSlotState::Warm)?;
        drop(slots);

        self.warm_queue.write().push_back(vm_id.to_string());
        self.emit_event(PoolEventKind::VmWarmed, vm_id, "VM is warm and ready");
        Ok(())
    }

    /// Acquire a warm VM for an agent session
    ///
    /// Returns the VM slot ID. The caller is responsible for creating
    /// the `AgentVM` session using the returned slot.
    pub fn acquire(&self, session_id: &str) -> PoolResult<String> {
        // Pop from warm queue
        let vm_id = self.warm_queue.write().pop_front().ok_or_else(|| {
            let slots = self.slots.read();
            let warm = slots
                .values()
                .filter(|s| s.state == VmSlotState::Warm)
                .count();
            PoolError::NoWarmVms {
                warm,
                total: slots.len(),
            }
        })?;

        // Transition to assigned
        let mut slots = self.slots.write();
        if let Some(slot) = slots.get_mut(&vm_id) {
            slot.transition(VmSlotState::Assigned)?;
            slot.assignee = Some(session_id.to_string());
            slot.assignment_count += 1;
        }
        drop(slots);

        *self.total_assignments.write() += 1;
        self.emit_event(
            PoolEventKind::VmAssigned,
            &vm_id,
            &format!("Assigned to session {session_id}"),
        );
        Ok(vm_id)
    }

    /// Release a VM back to the pool after use
    ///
    /// The VM enters draining state, then can be recycled.
    pub fn release(&self, vm_id: &str) -> PoolResult<()> {
        let mut slots = self.slots.write();
        let slot = slots
            .get_mut(vm_id)
            .ok_or_else(|| PoolError::VmNotFound(vm_id.to_string()))?;
        let session = slot.assignee.take();
        slot.transition(VmSlotState::Draining)?;
        drop(slots);

        self.emit_event(
            PoolEventKind::VmReleased,
            vm_id,
            &format!("Released by session {}", session.unwrap_or_default()),
        );
        Ok(())
    }

    /// Recycle a draining VM — wipe state and return to warm pool
    pub fn recycle(&self, vm_id: &str) -> PoolResult<()> {
        let mut slots = self.slots.write();
        let slot = slots
            .get_mut(vm_id)
            .ok_or_else(|| PoolError::VmNotFound(vm_id.to_string()))?;

        // Check max assignments
        if self.config.max_assignments > 0 && slot.assignment_count >= self.config.max_assignments {
            slot.transition(VmSlotState::Terminating)?;
            drop(slots);
            self.emit_event(
                PoolEventKind::VmTerminated,
                vm_id,
                "Max assignments reached, terminating",
            );
            return Ok(());
        }

        slot.transition(VmSlotState::Recycling)?;
        drop(slots);

        *self.total_recycles.write() += 1;

        // After recycling, transition back to warm
        let mut slots = self.slots.write();
        if let Some(slot) = slots.get_mut(vm_id) {
            slot.state = VmSlotState::Warm;
            slot.state_changed_at = Instant::now();
            slot.assignee = None;
        }
        drop(slots);

        self.warm_queue.write().push_back(vm_id.to_string());
        self.emit_event(
            PoolEventKind::VmRecycled,
            vm_id,
            "VM recycled and re-warmed",
        );
        Ok(())
    }

    /// Mark a VM as failed
    pub fn mark_failed(&self, vm_id: &str, reason: &str) -> PoolResult<()> {
        let mut slots = self.slots.write();
        let slot = slots
            .get_mut(vm_id)
            .ok_or_else(|| PoolError::VmNotFound(vm_id.to_string()))?;
        slot.transition(VmSlotState::Failed)?;
        slot.assignee = None;
        drop(slots);

        // Remove from warm queue if present
        self.warm_queue.write().retain(|id| id != vm_id);
        self.emit_event(PoolEventKind::VmFailed, vm_id, reason);
        Ok(())
    }

    /// Terminate and remove a VM from the pool
    pub fn terminate(&self, vm_id: &str) -> PoolResult<()> {
        let mut slots = self.slots.write();
        let slot = slots
            .get_mut(vm_id)
            .ok_or_else(|| PoolError::VmNotFound(vm_id.to_string()))?;

        // Allow termination from any non-assigned state
        if slot.state != VmSlotState::Assigned {
            slot.state = VmSlotState::Terminating;
            slot.state_changed_at = Instant::now();
        } else {
            return Err(PoolError::InvalidTransition {
                from: VmSlotState::Assigned,
                to: VmSlotState::Terminating,
            });
        }
        drop(slots);

        // Remove from warm queue
        self.warm_queue.write().retain(|id| id != vm_id);

        // Remove from pool
        self.slots.write().remove(vm_id);
        self.emit_event(
            PoolEventKind::VmTerminated,
            vm_id,
            "VM terminated and removed",
        );
        Ok(())
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let slots = self.slots.read();
        let mut stats = PoolStats {
            total: slots.len(),
            total_assignments: *self.total_assignments.read(),
            total_recycles: *self.total_recycles.read(),
            ..Default::default()
        };

        for slot in slots.values() {
            match slot.state {
                VmSlotState::Provisioning => stats.provisioning += 1,
                VmSlotState::Warm => stats.warm += 1,
                VmSlotState::Assigned => stats.assigned += 1,
                VmSlotState::Draining => stats.draining += 1,
                VmSlotState::Recycling => stats.recycling += 1,
                VmSlotState::Failed => stats.failed += 1,
                VmSlotState::Terminating => {}
            }
        }

        stats
    }

    /// List all VM entries
    pub fn list(&self) -> Vec<VmEntry> {
        self.slots.read().values().map(VmEntry::from).collect()
    }

    /// Get a specific VM entry
    pub fn get(&self, vm_id: &str) -> Option<VmEntry> {
        self.slots.read().get(vm_id).map(VmEntry::from)
    }

    /// Get the pool configuration
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Number of warm VMs ready for assignment
    pub fn warm_count(&self) -> usize {
        self.warm_queue.read().len()
    }

    /// Get recent events
    pub fn recent_events(&self, count: usize) -> Vec<PoolEvent> {
        let events = self.events.read();
        events.iter().rev().take(count).cloned().collect()
    }

    /// Identify VMs that need maintenance (expired idle, expired lifetime, failed)
    pub fn maintenance_candidates(&self) -> Vec<String> {
        let slots = self.slots.read();
        let mut candidates = Vec::new();
        for slot in slots.values() {
            if slot.is_idle_expired(self.config.max_idle_time)
                || slot.is_lifetime_expired(self.config.max_lifetime)
                || slot.state == VmSlotState::Failed
            {
                candidates.push(slot.id.clone());
            }
        }
        candidates
    }

    /// Get the number of additional VMs needed to meet min_warm target
    pub fn warm_deficit(&self) -> usize {
        let warm = self.warm_count();
        self.config.min_warm.saturating_sub(warm)
    }

    fn emit_event(&self, kind: PoolEventKind, vm_id: &str, detail: &str) {
        let event = PoolEvent {
            kind,
            vm_id: vm_id.to_string(),
            timestamp: SystemTime::now(),
            detail: detail.to_string(),
        };

        let mut events = self.events.write();
        events.push_back(event);
        // Keep bounded
        while events.len() > 10_000 {
            events.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool() -> VmPool {
        VmPool::new(PoolConfig {
            min_warm: 2,
            max_size: 4,
            max_assignments: 3,
            ..Default::default()
        })
    }

    #[test]
    fn test_provision_and_warm() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        assert_eq!(pool.stats().provisioning, 1);

        pool.mark_warm(&id).unwrap();
        assert_eq!(pool.stats().warm, 1);
        assert_eq!(pool.warm_count(), 1);
    }

    #[test]
    fn test_acquire_and_release() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();

        let acquired = pool.acquire("session-1").unwrap();
        assert_eq!(acquired, id);
        assert_eq!(pool.stats().assigned, 1);
        assert_eq!(pool.warm_count(), 0);

        pool.release(&acquired).unwrap();
        assert_eq!(pool.stats().draining, 1);
    }

    #[test]
    fn test_recycle() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();
        pool.acquire("s1").unwrap();
        pool.release(&id).unwrap();
        pool.recycle(&id).unwrap();

        assert_eq!(pool.stats().warm, 1);
        assert_eq!(pool.warm_count(), 1);
        assert_eq!(*pool.total_recycles.read(), 1);
    }

    #[test]
    fn test_capacity_limit() {
        let pool = test_pool(); // max_size = 4
        for _ in 0..4 {
            pool.provision().unwrap();
        }
        let err = pool.provision().unwrap_err();
        assert!(matches!(err, PoolError::AtCapacity { .. }));
    }

    #[test]
    fn test_no_warm_vms() {
        let pool = test_pool();
        let err = pool.acquire("s1").unwrap_err();
        assert!(matches!(err, PoolError::NoWarmVms { .. }));
    }

    #[test]
    fn test_max_assignments_termination() {
        let pool = test_pool(); // max_assignments = 3
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();

        // Use it 3 times
        for i in 0..3 {
            pool.acquire(&format!("s{i}")).unwrap();
            pool.release(&id).unwrap();
            if i < 2 {
                pool.recycle(&id).unwrap();
            }
        }

        // Third recycle should terminate (assignment_count >= 3)
        pool.recycle(&id).unwrap();
        // VM should be terminated (state = Terminating)
        let entry = pool.get(&id);
        assert!(entry.is_none() || entry.unwrap().state == VmSlotState::Terminating);
    }

    #[test]
    fn test_mark_failed() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();
        pool.acquire("s1").unwrap();
        pool.mark_failed(&id, "OOM killed").unwrap();

        assert_eq!(pool.stats().failed, 1);
        assert_eq!(pool.warm_count(), 0);
    }

    #[test]
    fn test_terminate() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();
        pool.terminate(&id).unwrap();

        assert_eq!(pool.stats().total, 0);
    }

    #[test]
    fn test_pool_stats() {
        let pool = test_pool();
        let id1 = pool.provision().unwrap();
        let id2 = pool.provision().unwrap();
        pool.mark_warm(&id1).unwrap();
        pool.mark_warm(&id2).unwrap();
        pool.acquire("s1").unwrap();

        let stats = pool.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.warm, 1);
        assert_eq!(stats.assigned, 1);
        assert_eq!(stats.total_assignments, 1);
    }

    #[test]
    fn test_list_and_get() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();

        let list = pool.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, VmSlotState::Warm);

        let entry = pool.get(&id).unwrap();
        assert_eq!(entry.id, id);
    }

    #[test]
    fn test_warm_deficit() {
        let pool = test_pool(); // min_warm = 2
        assert_eq!(pool.warm_deficit(), 2);

        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();
        assert_eq!(pool.warm_deficit(), 1);

        let id2 = pool.provision().unwrap();
        pool.mark_warm(&id2).unwrap();
        assert_eq!(pool.warm_deficit(), 0);
    }

    #[test]
    fn test_events() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        pool.mark_warm(&id).unwrap();

        let events = pool.recent_events(10);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, PoolEventKind::VmWarmed);
        assert_eq!(events[1].kind, PoolEventKind::VmProvisioned);
    }

    #[test]
    fn test_invalid_transition() {
        let pool = test_pool();
        let id = pool.provision().unwrap();
        // Cannot go directly from Provisioning to Assigned
        let err = {
            let mut slots = pool.slots.write();
            slots
                .get_mut(&id)
                .unwrap()
                .transition(VmSlotState::Assigned)
        };
        assert!(err.is_err());
    }
}
