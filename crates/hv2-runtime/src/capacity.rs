//! Capacity Reservations and SLA-Aligned VM Classes
//!
//! Provides reserved capacity blocks, committed-use contracts, and
//! SLA tiers that map to scheduling guarantees. Operators can define
//! VM classes (e.g., "secure-gpu-premium") with guaranteed resource
//! allocation, priority scheduling, and uptime SLAs.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Capacity operation result
pub type CapacityResult<T> = Result<T, CapacityError>;

/// Capacity errors
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CapacityError {
    /// Reservation not found
    #[error("Reservation not found: {0}")]
    ReservationNotFound(String),

    /// Insufficient capacity to fulfil reservation
    #[error("Insufficient capacity: need {needed}, available {available} ({resource})")]
    InsufficientCapacity {
        resource: String,
        needed: u64,
        available: u64,
    },

    /// VM class not found
    #[error("VM class not found: {0}")]
    VmClassNotFound(String),

    /// Reservation expired
    #[error("Reservation expired: {0}")]
    Expired(String),

    /// Duplicate name
    #[error("Duplicate: {0}")]
    Duplicate(String),

    /// Invalid configuration
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
}

/// SLA tier with uptime guarantees
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlaTier {
    /// Best-effort, no guarantees (preemptible)
    BestEffort,
    /// Standard SLA (99.9% availability)
    Standard,
    /// Premium SLA (99.95% availability, priority scheduling)
    Premium,
    /// Dedicated SLA (99.99% availability, dedicated hosts)
    Dedicated,
}

impl SlaTier {
    /// Target availability percentage
    pub fn target_availability(&self) -> f64 {
        match self {
            Self::BestEffort => 0.0,
            Self::Standard => 99.9,
            Self::Premium => 99.95,
            Self::Dedicated => 99.99,
        }
    }

    /// Scheduling priority boost (added to workload priority)
    pub fn priority_boost(&self) -> u32 {
        match self {
            Self::BestEffort => 0,
            Self::Standard => 10,
            Self::Premium => 50,
            Self::Dedicated => 100,
        }
    }

    /// Whether workloads in this tier can be preempted
    pub fn preemptible(&self) -> bool {
        matches!(self, Self::BestEffort)
    }
}

/// VM class definition
///
/// Represents a named, pre-configured VM shape with guaranteed
/// resource allocation and SLA properties. Operators define these
/// as product SKUs for tenants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmClass {
    /// Class name (e.g., "gpu-a100-4x-premium")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Guaranteed vCPUs
    pub vcpus: u32,
    /// Guaranteed memory in bytes
    pub memory_bytes: u64,
    /// Number of GPUs
    pub gpu_count: u32,
    /// GPU model requirement (empty = any)
    pub gpu_model: String,
    /// SLA tier
    pub sla_tier: SlaTier,
    /// Whether this class requires dedicated (non-shared) hosts
    pub dedicated_host: bool,
    /// Rate per hour in USD
    pub rate_per_hour: f64,
    /// Maximum instances of this class across the fleet
    pub max_instances: u32,
    /// Currently active instances
    pub active_instances: u32,
}

impl VmClass {
    /// Create a new VM class
    pub fn new(name: impl Into<String>, sla_tier: SlaTier) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            vcpus: 2,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_model: String::new(),
            sla_tier,
            dedicated_host: false,
            rate_per_hour: 0.0,
            max_instances: u32::MAX,
            active_instances: 0,
        }
    }

    /// Builder: set description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: set vCPUs
    pub fn vcpus(mut self, n: u32) -> Self {
        self.vcpus = n;
        self
    }

    /// Builder: set memory
    pub fn memory(mut self, bytes: u64) -> Self {
        self.memory_bytes = bytes;
        self
    }

    /// Builder: set GPU count and model
    pub fn gpus(mut self, count: u32, model: impl Into<String>) -> Self {
        self.gpu_count = count;
        self.gpu_model = model.into();
        self
    }

    /// Builder: set hourly rate
    pub fn rate(mut self, usd_per_hour: f64) -> Self {
        self.rate_per_hour = usd_per_hour;
        self
    }

    /// Builder: set max instances
    pub fn max(mut self, n: u32) -> Self {
        self.max_instances = n;
        self
    }

    /// Builder: require dedicated hosts
    pub fn dedicated(mut self) -> Self {
        self.dedicated_host = true;
        self
    }

    /// Check if another instance can be launched
    pub fn has_capacity(&self) -> bool {
        self.active_instances < self.max_instances
    }
}

/// Reservation state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationState {
    /// Reservation is active
    Active,
    /// Reservation is fully consumed (all slots in use)
    FullyUtilized,
    /// Reservation has expired
    Expired,
    /// Reservation was cancelled
    Cancelled,
}

/// A capacity reservation
///
/// Guarantees a block of resources for a tenant over a time window.
/// Resources reserved are not available to best-effort workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    /// Unique reservation ID
    pub id: String,
    /// Tenant or account that owns the reservation
    pub tenant_id: String,
    /// VM class reserved
    pub vm_class: String,
    /// Number of instances reserved
    pub instance_count: u32,
    /// Instances currently in use
    pub instances_used: u32,
    /// Reservation start time
    pub starts_at: SystemTime,
    /// Reservation end time
    pub expires_at: SystemTime,
    /// Current state
    pub state: ReservationState,
    /// Created timestamp
    pub created_at: SystemTime,
}

impl Reservation {
    /// Available (unused) instance count
    pub fn available(&self) -> u32 {
        self.instance_count.saturating_sub(self.instances_used)
    }

    /// Whether the reservation has unused capacity
    pub fn has_available(&self) -> bool {
        self.available() > 0 && self.state == ReservationState::Active
    }

    /// Check if the reservation covers the current time
    pub fn is_current(&self) -> bool {
        let now = SystemTime::now();
        now >= self.starts_at && now < self.expires_at
    }
}

/// Capacity manager
///
/// Manages VM class definitions and capacity reservations. The scheduler
/// queries this to honor reservations and enforce SLA guarantees.
pub struct CapacityManager {
    /// VM class catalog
    classes: RwLock<HashMap<String, VmClass>>,
    /// Active reservations (reservation_id -> Reservation)
    reservations: RwLock<HashMap<String, Reservation>>,
    /// Reservation counter
    counter: std::sync::atomic::AtomicU64,
}

impl CapacityManager {
    /// Create a new capacity manager
    pub fn new() -> Self {
        Self {
            classes: RwLock::new(HashMap::new()),
            reservations: RwLock::new(HashMap::new()),
            counter: std::sync::atomic::AtomicU64::new(1),
        }
    }

    // ── VM Classes ────────────────────────────────────────────────────

    /// Register a VM class
    pub fn register_class(&self, class: VmClass) -> CapacityResult<()> {
        let mut classes = self.classes.write();
        if classes.contains_key(&class.name) {
            return Err(CapacityError::Duplicate(class.name.clone()));
        }
        classes.insert(class.name.clone(), class);
        Ok(())
    }

    /// Get a VM class by name
    pub fn get_class(&self, name: &str) -> Option<VmClass> {
        self.classes.read().get(name).cloned()
    }

    /// List all VM classes
    pub fn list_classes(&self) -> Vec<VmClass> {
        self.classes.read().values().cloned().collect()
    }

    /// Update active instance count for a class
    pub fn increment_class_usage(&self, name: &str) -> CapacityResult<()> {
        let mut classes = self.classes.write();
        let class = classes
            .get_mut(name)
            .ok_or_else(|| CapacityError::VmClassNotFound(name.to_string()))?;
        if !class.has_capacity() {
            return Err(CapacityError::InsufficientCapacity {
                resource: format!("vm_class:{name}"),
                needed: 1,
                available: 0,
            });
        }
        class.active_instances += 1;
        Ok(())
    }

    /// Decrement active instance count for a class
    pub fn decrement_class_usage(&self, name: &str) -> CapacityResult<()> {
        let mut classes = self.classes.write();
        let class = classes
            .get_mut(name)
            .ok_or_else(|| CapacityError::VmClassNotFound(name.to_string()))?;
        class.active_instances = class.active_instances.saturating_sub(1);
        Ok(())
    }

    // ── Reservations ──────────────────────────────────────────────────

    /// Create a capacity reservation
    pub fn create_reservation(
        &self,
        tenant_id: &str,
        vm_class: &str,
        instance_count: u32,
        duration: Duration,
    ) -> CapacityResult<String> {
        // Validate class exists
        if self.get_class(vm_class).is_none() {
            return Err(CapacityError::VmClassNotFound(vm_class.to_string()));
        }

        if instance_count == 0 {
            return Err(CapacityError::InvalidConfig(
                "instance_count must be > 0".into(),
            ));
        }

        let now = SystemTime::now();
        let id = format!(
            "rsv-{}",
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );

        let reservation = Reservation {
            id: id.clone(),
            tenant_id: tenant_id.to_string(),
            vm_class: vm_class.to_string(),
            instance_count,
            instances_used: 0,
            starts_at: now,
            expires_at: now + duration,
            state: ReservationState::Active,
            created_at: now,
        };

        self.reservations.write().insert(id.clone(), reservation);
        Ok(id)
    }

    /// Consume one slot from a reservation
    pub fn consume_reservation(&self, reservation_id: &str) -> CapacityResult<()> {
        let mut reservations = self.reservations.write();
        let reservation = reservations
            .get_mut(reservation_id)
            .ok_or_else(|| CapacityError::ReservationNotFound(reservation_id.to_string()))?;

        if !reservation.is_current() {
            reservation.state = ReservationState::Expired;
            return Err(CapacityError::Expired(reservation_id.to_string()));
        }

        if !reservation.has_available() {
            return Err(CapacityError::InsufficientCapacity {
                resource: format!("reservation:{reservation_id}"),
                needed: 1,
                available: 0,
            });
        }

        reservation.instances_used += 1;
        if reservation.instances_used >= reservation.instance_count {
            reservation.state = ReservationState::FullyUtilized;
        }
        Ok(())
    }

    /// Release one slot back to a reservation
    pub fn release_reservation(&self, reservation_id: &str) -> CapacityResult<()> {
        let mut reservations = self.reservations.write();
        let reservation = reservations
            .get_mut(reservation_id)
            .ok_or_else(|| CapacityError::ReservationNotFound(reservation_id.to_string()))?;

        reservation.instances_used = reservation.instances_used.saturating_sub(1);
        if reservation.state == ReservationState::FullyUtilized
            && reservation.instances_used < reservation.instance_count
        {
            reservation.state = ReservationState::Active;
        }
        Ok(())
    }

    /// Cancel a reservation
    pub fn cancel_reservation(&self, reservation_id: &str) -> CapacityResult<()> {
        let mut reservations = self.reservations.write();
        let reservation = reservations
            .get_mut(reservation_id)
            .ok_or_else(|| CapacityError::ReservationNotFound(reservation_id.to_string()))?;
        reservation.state = ReservationState::Cancelled;
        Ok(())
    }

    /// Get a reservation
    pub fn get_reservation(&self, id: &str) -> Option<Reservation> {
        self.reservations.read().get(id).cloned()
    }

    /// List reservations for a tenant
    pub fn tenant_reservations(&self, tenant_id: &str) -> Vec<Reservation> {
        self.reservations
            .read()
            .values()
            .filter(|r| r.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    /// List all active reservations
    pub fn active_reservations(&self) -> Vec<Reservation> {
        self.reservations
            .read()
            .values()
            .filter(|r| r.state == ReservationState::Active || r.state == ReservationState::FullyUtilized)
            .cloned()
            .collect()
    }

    /// Expire stale reservations
    pub fn expire_stale(&self) -> usize {
        let mut reservations = self.reservations.write();
        let mut expired = 0;
        for reservation in reservations.values_mut() {
            if reservation.state == ReservationState::Active && !reservation.is_current() {
                reservation.state = ReservationState::Expired;
                expired += 1;
            }
        }
        expired
    }

    /// Total reserved instance count across all active reservations (optionally filtered by class)
    pub fn total_reserved(&self, vm_class: Option<&str>) -> u32 {
        self.reservations
            .read()
            .values()
            .filter(|r| r.state == ReservationState::Active || r.state == ReservationState::FullyUtilized)
            .filter(|r| vm_class.is_none_or(|c| r.vm_class == c))
            .map(|r| r.instance_count)
            .sum()
    }
}

impl Default for CapacityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> CapacityManager {
        let cm = CapacityManager::new();
        cm.register_class(
            VmClass::new("gpu-a100-premium", SlaTier::Premium)
                .description("4x A100 80GB, Premium SLA")
                .vcpus(32)
                .memory(256 * 1024 * 1024 * 1024)
                .gpus(4, "A100-80GB")
                .rate(12.0)
                .max(100),
        )
        .unwrap();
        cm.register_class(
            VmClass::new("cpu-standard", SlaTier::Standard)
                .description("General purpose compute")
                .vcpus(8)
                .memory(32 * 1024 * 1024 * 1024)
                .rate(0.50),
        )
        .unwrap();
        cm
    }

    #[test]
    fn test_register_class() {
        let cm = setup();
        assert_eq!(cm.list_classes().len(), 2);
        let cls = cm.get_class("gpu-a100-premium").unwrap();
        assert_eq!(cls.gpu_count, 4);
        assert_eq!(cls.sla_tier, SlaTier::Premium);
    }

    #[test]
    fn test_duplicate_class() {
        let cm = setup();
        let err = cm
            .register_class(VmClass::new("gpu-a100-premium", SlaTier::Standard))
            .unwrap_err();
        assert!(matches!(err, CapacityError::Duplicate(_)));
    }

    #[test]
    fn test_class_usage() {
        let cm = setup();
        cm.increment_class_usage("gpu-a100-premium").unwrap();
        let cls = cm.get_class("gpu-a100-premium").unwrap();
        assert_eq!(cls.active_instances, 1);

        cm.decrement_class_usage("gpu-a100-premium").unwrap();
        let cls = cm.get_class("gpu-a100-premium").unwrap();
        assert_eq!(cls.active_instances, 0);
    }

    #[test]
    fn test_class_capacity_limit() {
        let cm = CapacityManager::new();
        cm.register_class(
            VmClass::new("tiny", SlaTier::BestEffort).max(1),
        )
        .unwrap();

        cm.increment_class_usage("tiny").unwrap();
        let err = cm.increment_class_usage("tiny").unwrap_err();
        assert!(matches!(err, CapacityError::InsufficientCapacity { .. }));
    }

    #[test]
    fn test_create_reservation() {
        let cm = setup();
        let id = cm
            .create_reservation("tenant-1", "gpu-a100-premium", 10, Duration::from_secs(3600))
            .unwrap();

        let rsv = cm.get_reservation(&id).unwrap();
        assert_eq!(rsv.tenant_id, "tenant-1");
        assert_eq!(rsv.instance_count, 10);
        assert_eq!(rsv.state, ReservationState::Active);
    }

    #[test]
    fn test_reservation_unknown_class() {
        let cm = setup();
        let err = cm
            .create_reservation("t1", "nonexistent", 1, Duration::from_secs(60))
            .unwrap_err();
        assert!(matches!(err, CapacityError::VmClassNotFound(_)));
    }

    #[test]
    fn test_consume_reservation() {
        let cm = setup();
        let id = cm
            .create_reservation("t1", "gpu-a100-premium", 2, Duration::from_secs(3600))
            .unwrap();

        cm.consume_reservation(&id).unwrap();
        let rsv = cm.get_reservation(&id).unwrap();
        assert_eq!(rsv.instances_used, 1);
        assert_eq!(rsv.state, ReservationState::Active);

        cm.consume_reservation(&id).unwrap();
        let rsv = cm.get_reservation(&id).unwrap();
        assert_eq!(rsv.instances_used, 2);
        assert_eq!(rsv.state, ReservationState::FullyUtilized);

        // Third consume should fail
        let err = cm.consume_reservation(&id).unwrap_err();
        assert!(matches!(err, CapacityError::InsufficientCapacity { .. }));
    }

    #[test]
    fn test_release_reservation() {
        let cm = setup();
        let id = cm
            .create_reservation("t1", "cpu-standard", 1, Duration::from_secs(3600))
            .unwrap();

        cm.consume_reservation(&id).unwrap();
        assert_eq!(
            cm.get_reservation(&id).unwrap().state,
            ReservationState::FullyUtilized
        );

        cm.release_reservation(&id).unwrap();
        let rsv = cm.get_reservation(&id).unwrap();
        assert_eq!(rsv.instances_used, 0);
        assert_eq!(rsv.state, ReservationState::Active);
    }

    #[test]
    fn test_cancel_reservation() {
        let cm = setup();
        let id = cm
            .create_reservation("t1", "cpu-standard", 5, Duration::from_secs(3600))
            .unwrap();

        cm.cancel_reservation(&id).unwrap();
        let rsv = cm.get_reservation(&id).unwrap();
        assert_eq!(rsv.state, ReservationState::Cancelled);
    }

    #[test]
    fn test_tenant_reservations() {
        let cm = setup();
        cm.create_reservation("t1", "gpu-a100-premium", 2, Duration::from_secs(3600))
            .unwrap();
        cm.create_reservation("t1", "cpu-standard", 5, Duration::from_secs(3600))
            .unwrap();
        cm.create_reservation("t2", "cpu-standard", 1, Duration::from_secs(3600))
            .unwrap();

        assert_eq!(cm.tenant_reservations("t1").len(), 2);
        assert_eq!(cm.tenant_reservations("t2").len(), 1);
    }

    #[test]
    fn test_total_reserved() {
        let cm = setup();
        cm.create_reservation("t1", "gpu-a100-premium", 10, Duration::from_secs(3600))
            .unwrap();
        cm.create_reservation("t2", "gpu-a100-premium", 5, Duration::from_secs(3600))
            .unwrap();

        assert_eq!(cm.total_reserved(Some("gpu-a100-premium")), 15);
        assert_eq!(cm.total_reserved(Some("cpu-standard")), 0);
        assert_eq!(cm.total_reserved(None), 15);
    }

    #[test]
    fn test_sla_tier_properties() {
        assert!(SlaTier::BestEffort.preemptible());
        assert!(!SlaTier::Premium.preemptible());
        assert!(SlaTier::Dedicated.priority_boost() > SlaTier::Standard.priority_boost());
        assert!(SlaTier::Standard.target_availability() > 99.0);
        assert!(SlaTier::Dedicated.target_availability() > SlaTier::Premium.target_availability());
    }

    #[test]
    fn test_not_found_errors() {
        let cm = setup();
        assert!(matches!(
            cm.increment_class_usage("nope").unwrap_err(),
            CapacityError::VmClassNotFound(_)
        ));
        assert!(matches!(
            cm.consume_reservation("nope").unwrap_err(),
            CapacityError::ReservationNotFound(_)
        ));
        assert!(matches!(
            cm.cancel_reservation("nope").unwrap_err(),
            CapacityError::ReservationNotFound(_)
        ));
    }

    #[test]
    fn test_zero_instance_reservation() {
        let cm = setup();
        let err = cm
            .create_reservation("t1", "cpu-standard", 0, Duration::from_secs(3600))
            .unwrap_err();
        assert!(matches!(err, CapacityError::InvalidConfig(_)));
    }
}
