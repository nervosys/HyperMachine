//! HyperMachine Stateful Runtime Environment
//!
//! This crate provides fleet-level orchestration for agentic workflows,
//! sitting between the per-VM agent SDK ([`hv2_agent`]) and user-facing
//! frontends. It manages VM pools, schedules agent workloads, persists
//! durable workflow state, and scales from a single agent to thousands.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     hv2-runtime                             │
//! │  ┌──────────┐ ┌───────────┐ ┌──────────┐ ┌──────────────┐  │
//! │  │  Gateway  │ │ Scheduler │ │ Workflow │ │  Autoscaler  │  │
//! │  └────┬─────┘ └─────┬─────┘ └────┬─────┘ └──────┬───────┘  │
//! │       │             │            │               │          │
//! │  ┌────┴─────────────┴────────────┴───────────────┴───────┐  │
//! │  │                    VM Pool                            │  │
//! │  │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐        ┌─────────┐ │  │
//! │  │  │ VM₁ │ │ VM₂ │ │ VM₃ │ │ VM₄ │  ...   │ Health  │ │  │
//! │  │  └──┬──┘ └──┬──┘ └──┬──┘ └──┬──┘        └─────────┘ │  │
//! │  └─────┼───────┼───────┼───────┼────────────────────────┘  │
//! │        │       │       │       │                            │
//! │  ┌─────┴───────┴───────┴───────┴─────────────────────────┐  │
//! │  │           Store (Durable State Backend)               │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! │  ┌───────────────────────────────────────────────────────┐  │
//! │  │                 Billing / Metering                    │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Scaling Model
//!
//! | Scale | Mode | Description |
//! |-------|------|-------------|
//! | 1 agent | Embedded | `hv2-agent` directly, no runtime needed |
//! | ~10 agents | Single-host | Local VM pool, in-process scheduler |
//! | ~100 agents | Multi-host | Per-host runtime, shared state store |
//! | 1000+ agents | Distributed | Sharded runtime, consistent hashing, autoscaler |
//!
//! # Example
//!
//! ```rust,ignore
//! use hv2_runtime::{Runtime, RuntimeConfig, PoolConfig, WorkflowBuilder};
//!
//! // Create runtime with a warm pool of 4 VMs
//! let config = RuntimeConfig::builder()
//!     .pool(PoolConfig { min_warm: 4, max_size: 64, ..Default::default() })
//!     .build();
//! let runtime = Runtime::new(config).await?;
//!
//! // Submit a multi-step workflow
//! let workflow = WorkflowBuilder::new("data-pipeline")
//!     .step("ingest", |ctx| async move { ctx.exec("download data").await })
//!     .step("transform", |ctx| async move { ctx.exec("process data").await })
//!     .step("export", |ctx| async move { ctx.exec("upload results").await })
//!     .build();
//!
//! let result = runtime.submit_workflow(workflow).await?;
//! ```

#![allow(dead_code)]

pub mod autoscale;
pub mod billing;
pub mod capacity;
pub mod fleet;
pub mod gateway;
pub mod health;
pub mod metrics;
pub mod pool;
pub mod scheduler;
pub mod store;
pub mod topology;
pub mod workflow;

pub use autoscale::{
    AutoscaleConfig, AutoscaleDecision, AutoscaleMetrics, AutoscalePolicy, AutoscaleResult,
    Autoscaler, ScaleDirection, ScaleEvent, ScaleReason,
};
pub use billing::{
    BillingConfig, BillingError, BillingEvent, BillingResult, BillingTier, Invoice, LineItem,
    MeterReading, MeteringEngine, ResourceMeter, UsageSummary,
};
pub use capacity::{CapacityManager, Reservation, ReservationState, SlaTier, VmClass};
pub use fleet::{
    ArtifactKind, ArtifactVersion, FleetHost, FleetManager, HostUpdatePhase, RolloutConfig,
    RolloutPhase, RolloutStrategy,
};
pub use gateway::{
    Gateway, GatewayConfig, GatewayError, GatewayResult, Route, RoutePolicy, RoutingDecision,
    SessionAffinity,
};
pub use health::{
    HealthCheck, HealthCheckConfig, HealthCheckResult, HealthMonitor, HealthStatus, ProbeResult,
    ProbeType, VmHealth,
};
pub use metrics::{Counter, Gauge, Histogram, HistogramSnapshot, MetricsCollector, RuntimeMetrics};
pub use pool::{
    PoolConfig, PoolError, PoolEvent, PoolResult, PoolStats, VmEntry, VmPool, VmSlot, VmSlotState,
};
pub use scheduler::{
    Placement, PlacementConstraint, PlacementScore, PlacementStrategy, ScheduleError,
    ScheduleResult, Scheduler, SchedulerConfig, WorkloadDescriptor,
};
pub use store::{
    DurableStore, StoreBackend, StoreConfig, StoreEntry, StoreError, StoreResult, WatchEvent,
    WatchEventType,
};
pub use topology::{
    GpuDevice, GpuInterconnect, GpuPlacement, GpuRequirements, GpuTopologyMap, TopologyLink,
};
pub use workflow::{
    StepContext, StepError, StepOutcome, StepResult, StepSpec, WorkflowBuilder, WorkflowConfig,
    WorkflowEngine, WorkflowError, WorkflowExecution, WorkflowPhase, WorkflowResult, WorkflowSpec,
};

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use thiserror::Error;

/// Top-level runtime error
#[derive(Error, Debug)]
pub enum RuntimeError {
    /// VM pool error
    #[error("Pool error: {0}")]
    Pool(#[from] PoolError),

    /// Scheduling error
    #[error("Schedule error: {0}")]
    Schedule(#[from] ScheduleError),

    /// Workflow error
    #[error("Workflow error: {0}")]
    Workflow(#[from] WorkflowError),

    /// Store error
    #[error("Store error: {0}")]
    Store(#[from] StoreError),

    /// Gateway error
    #[error("Gateway error: {0}")]
    Gateway(#[from] GatewayError),

    /// Billing error
    #[error("Billing error: {0}")]
    Billing(#[from] BillingError),

    /// Agent error
    #[error("Agent error: {0}")]
    Agent(#[from] hv2_agent::AgentError),

    /// Core VM error
    #[error("VM error: {0}")]
    Vm(#[from] hv2_core::Error),

    /// Configuration error
    #[error("Config error: {0}")]
    Config(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Top-level runtime result
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// VM pool configuration
    pub pool: PoolConfig,
    /// Scheduler configuration
    pub scheduler: SchedulerConfig,
    /// Workflow engine configuration
    pub workflow: WorkflowConfig,
    /// Durable store configuration
    pub store: StoreConfig,
    /// Gateway configuration
    pub gateway: GatewayConfig,
    /// Autoscale configuration
    pub autoscale: AutoscaleConfig,
    /// Health monitoring configuration
    pub health: HealthCheckConfig,
    /// Billing configuration
    pub billing: BillingConfig,
    /// Enable GPU topology-aware scheduling
    pub gpu_topology_enabled: bool,
    /// Enable fleet lifecycle management
    pub fleet_management_enabled: bool,
    /// Enable capacity reservations
    pub capacity_reservations_enabled: bool,
    /// Runtime instance ID
    pub instance_id: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            pool: PoolConfig::default(),
            scheduler: SchedulerConfig::default(),
            workflow: WorkflowConfig::default(),
            store: StoreConfig::default(),
            gateway: GatewayConfig::default(),
            autoscale: AutoscaleConfig::default(),
            health: HealthCheckConfig::default(),
            billing: BillingConfig::default(),
            gpu_topology_enabled: false,
            fleet_management_enabled: false,
            capacity_reservations_enabled: false,
            instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl RuntimeConfig {
    /// Create a builder for runtime configuration
    pub fn builder() -> RuntimeConfigBuilder {
        RuntimeConfigBuilder::default()
    }
}

/// Builder for runtime configuration
#[derive(Debug, Default)]
pub struct RuntimeConfigBuilder {
    config: RuntimeConfig,
}

impl RuntimeConfigBuilder {
    /// Set VM pool configuration
    pub fn pool(mut self, pool: PoolConfig) -> Self {
        self.config.pool = pool;
        self
    }

    /// Set scheduler configuration
    pub fn scheduler(mut self, scheduler: SchedulerConfig) -> Self {
        self.config.scheduler = scheduler;
        self
    }

    /// Set workflow engine configuration
    pub fn workflow(mut self, workflow: WorkflowConfig) -> Self {
        self.config.workflow = workflow;
        self
    }

    /// Set durable store configuration
    pub fn store(mut self, store: StoreConfig) -> Self {
        self.config.store = store;
        self
    }

    /// Set gateway configuration
    pub fn gateway(mut self, gateway: GatewayConfig) -> Self {
        self.config.gateway = gateway;
        self
    }

    /// Set autoscale configuration
    pub fn autoscale(mut self, autoscale: AutoscaleConfig) -> Self {
        self.config.autoscale = autoscale;
        self
    }

    /// Set health check configuration
    pub fn health(mut self, health: HealthCheckConfig) -> Self {
        self.config.health = health;
        self
    }

    /// Set billing configuration
    pub fn billing(mut self, billing: BillingConfig) -> Self {
        self.config.billing = billing;
        self
    }

    /// Enable GPU topology-aware scheduling
    pub fn gpu_topology(mut self, enabled: bool) -> Self {
        self.config.gpu_topology_enabled = enabled;
        self
    }

    /// Enable fleet lifecycle management
    pub fn fleet_management(mut self, enabled: bool) -> Self {
        self.config.fleet_management_enabled = enabled;
        self
    }

    /// Enable capacity reservations
    pub fn capacity_reservations(mut self, enabled: bool) -> Self {
        self.config.capacity_reservations_enabled = enabled;
        self
    }

    /// Set instance ID
    pub fn instance_id(mut self, id: impl Into<String>) -> Self {
        self.config.instance_id = id.into();
        self
    }

    /// Build the configuration
    pub fn build(self) -> RuntimeConfig {
        self.config
    }
}

/// The stateful runtime environment
///
/// Manages a fleet of VMs, schedules agent workloads, executes workflows,
/// and provides durable state persistence across VM lifetimes.
///
/// The `Runtime` composes all eight subsystems and provides cross-cutting
/// orchestration methods that wire them together into coherent operations.
pub struct Runtime {
    /// Configuration
    config: RuntimeConfig,
    /// VM pool
    pool: VmPool,
    /// Workload scheduler
    scheduler: Scheduler,
    /// Workflow execution engine
    workflow_engine: WorkflowEngine,
    /// Durable state store
    store: DurableStore,
    /// Request gateway
    gateway: Gateway,
    /// Autoscaler
    autoscaler: Autoscaler,
    /// Health monitor
    health_monitor: HealthMonitor,
    /// Billing engine
    billing_engine: MeteringEngine,
    /// Metrics collector
    metrics: MetricsCollector,
    /// GPU topology map (active when gpu_topology_enabled)
    gpu_topology: Option<GpuTopologyMap>,
    /// Fleet lifecycle manager (active when fleet_management_enabled)
    fleet_manager: Option<FleetManager>,
    /// Capacity reservation manager (active when capacity_reservations_enabled)
    capacity_manager: Option<CapacityManager>,
}

/// Status snapshot of the runtime for dashboards and monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    /// Runtime instance ID
    pub instance_id: String,
    /// Pool statistics
    pub pool: PoolStats,
    /// Number of active gateway routes
    pub active_routes: usize,
    /// Number of pending workloads in scheduler queue
    pub pending_workloads: usize,
    /// Number of active workflow executions
    pub active_workflows: usize,
    /// Number of durable store entries
    pub store_entries: usize,
    /// Health summary (status → count)
    pub health_summary: std::collections::HashMap<HealthStatus, usize>,
    /// Number of active billing sessions
    pub billing_sessions: usize,
    /// Autoscale cooldown state
    pub scale_up_cooldown: bool,
    /// Autoscale cooldown state
    pub scale_down_cooldown: bool,
    /// Timestamp
    pub timestamp: SystemTime,
}

/// Result of a session creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID
    pub session_id: String,
    /// Assigned VM ID
    pub vm_id: String,
    /// Billing tier
    pub tier: BillingTier,
}

/// Result of a workload submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadResult {
    /// Workload ID
    pub workload_id: String,
    /// Placed on VM
    pub vm_id: Option<String>,
    /// Whether placement succeeded immediately
    pub placed: bool,
}

/// Result of a maintenance tick
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceReport {
    /// VMs marked unhealthy and removed
    pub unhealthy_removed: Vec<String>,
    /// VMs marked degraded
    pub degraded_detected: Vec<String>,
    /// Idle gateway sessions expired
    pub sessions_expired: Vec<String>,
    /// VMs needing maintenance (idle/lifetime expired)
    pub maintenance_candidates: Vec<String>,
    /// Autoscale decision (if any)
    pub scale_decision: Option<AutoscaleDecision>,
    /// VMs provisioned this tick
    pub vms_provisioned: usize,
    /// VMs terminated this tick
    pub vms_terminated: usize,
    /// Store entries garbage collected
    pub store_gc_count: usize,
}

impl Runtime {
    /// Create a new runtime with the given configuration
    pub fn new(config: RuntimeConfig) -> Self {
        let pool = VmPool::new(config.pool.clone());
        let scheduler = Scheduler::new(config.scheduler.clone());
        let workflow_engine = WorkflowEngine::new(config.workflow.clone());
        let store = DurableStore::new(config.store.clone());
        let gateway = Gateway::new(config.gateway.clone());
        let autoscaler = Autoscaler::new(config.autoscale.clone());
        let health_monitor = HealthMonitor::new(config.health.clone());
        let billing_engine = MeteringEngine::new(config.billing.clone());
        let metrics = MetricsCollector::new(config.instance_id.clone());

        let gpu_topology = if config.gpu_topology_enabled {
            Some(GpuTopologyMap::new())
        } else {
            None
        };
        let fleet_manager = if config.fleet_management_enabled {
            Some(FleetManager::new())
        } else {
            None
        };
        let capacity_manager = if config.capacity_reservations_enabled {
            Some(CapacityManager::new())
        } else {
            None
        };

        Self {
            config,
            pool,
            scheduler,
            workflow_engine,
            store,
            gateway,
            autoscaler,
            health_monitor,
            billing_engine,
            metrics,
            gpu_topology,
            fleet_manager,
            capacity_manager,
        }
    }

    // ── Identity ──────────────────────────────────────────────────────

    /// Get the runtime instance ID
    pub fn instance_id(&self) -> &str {
        &self.config.instance_id
    }

    // ── Subsystem Accessors ───────────────────────────────────────────

    /// Get a reference to the VM pool
    pub fn pool(&self) -> &VmPool {
        &self.pool
    }

    /// Get a reference to the scheduler
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Get a reference to the workflow engine
    pub fn workflow_engine(&self) -> &WorkflowEngine {
        &self.workflow_engine
    }

    /// Get a reference to the durable store
    pub fn store(&self) -> &DurableStore {
        &self.store
    }

    /// Get a reference to the gateway
    pub fn gateway(&self) -> &Gateway {
        &self.gateway
    }

    /// Get a reference to the autoscaler
    pub fn autoscaler(&self) -> &Autoscaler {
        &self.autoscaler
    }

    /// Get a reference to the health monitor
    pub fn health_monitor(&self) -> &HealthMonitor {
        &self.health_monitor
    }

    /// Get a reference to the billing engine
    pub fn billing_engine(&self) -> &MeteringEngine {
        &self.billing_engine
    }

    /// Get a reference to the metrics collector
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics
    }

    /// Collect a full metrics snapshot
    pub fn collect_metrics(&self) -> RuntimeMetrics {
        self.metrics.collect(self)
    }

    /// Get a reference to the GPU topology map (if enabled)
    pub fn gpu_topology(&self) -> Option<&GpuTopologyMap> {
        self.gpu_topology.as_ref()
    }

    /// Get a reference to the fleet manager (if enabled)
    pub fn fleet_manager(&self) -> Option<&FleetManager> {
        self.fleet_manager.as_ref()
    }

    /// Get a reference to the capacity manager (if enabled)
    pub fn capacity_manager(&self) -> Option<&CapacityManager> {
        self.capacity_manager.as_ref()
    }

    // ── Session Lifecycle ─────────────────────────────────────────────

    /// Create a new agent session: acquire a VM, register with billing
    /// and gateway, start health monitoring.
    ///
    /// This is the primary entry point for agents connecting to the runtime.
    pub fn create_session(&self, session_id: &str, tier: BillingTier) -> Result<SessionInfo> {
        // 1. Acquire a warm VM from the pool
        let vm_id = self.pool.acquire(session_id)?;

        // 2. Register the session with billing
        self.billing_engine.register_session(session_id, tier);

        // 3. Create a gateway route (session → VM)
        let available_vm_ids: Vec<String> = self.pool.list().iter().map(|v| v.id.clone()).collect();
        let _ = self.gateway.route(session_id, &available_vm_ids);

        // 4. Register the VM for health monitoring
        self.health_monitor.register(&vm_id);

        // 5. Persist session metadata to the durable store
        let meta = serde_json::json!({
            "session_id": session_id,
            "vm_id": vm_id,
            "tier": tier,
            "created_at": SystemTime::now(),
        });
        let _ = self.store.put(
            &format!("sessions/{session_id}"),
            meta.to_string().into_bytes(),
        );

        // 6. Record metric
        self.metrics.on_session_created();

        Ok(SessionInfo {
            session_id: session_id.to_string(),
            vm_id,
            tier,
        })
    }

    /// Destroy an agent session: release the VM, unregister from billing
    /// and gateway, stop health monitoring, and generate a final invoice.
    pub fn destroy_session(&self, session_id: &str) -> Result<Option<Invoice>> {
        // 1. Find the assigned VM
        let vm_id = self.gateway.get_route(session_id);

        // 2. Generate final invoice before unregistering
        let invoice = self.billing_engine.generate_invoice(session_id).ok();

        // 3. Remove the gateway route
        self.gateway.remove_route(session_id);

        // 4. Release and recycle the VM
        if let Some(ref vm_id) = vm_id {
            let _ = self.pool.release(vm_id);
            let _ = self.pool.recycle(vm_id);
            self.health_monitor.unregister(vm_id);
        }

        // 5. Unregister from billing
        self.billing_engine.unregister_session(session_id);

        // 6. Clean up session state from durable store
        let _ = self.store.delete(&format!("sessions/{session_id}"));

        // 7. Record metric
        self.metrics.on_session_destroyed();

        Ok(invoice)
    }

    // ── Workload Scheduling ───────────────────────────────────────────

    /// Submit a workload for scheduling onto the VM pool.
    ///
    /// The workload is added to the scheduler queue. If a suitable VM is
    /// available, placement happens immediately and the workload-to-VM
    /// mapping is stored durably.
    pub fn submit_workload(&self, workload: WorkloadDescriptor) -> Result<WorkloadResult> {
        let workload_id = workload.id.clone();

        // 1. Submit to scheduler queue
        self.scheduler.submit(workload)?;

        // 2. Attempt immediate placement
        let available_vms = self.pool.list();
        let placement = self.scheduler.schedule_next(&available_vms);

        match placement {
            Ok(Some(p)) => {
                // Persist placement
                let meta = serde_json::json!({
                    "workload_id": p.workload_id,
                    "vm_id": p.vm_id,
                    "score": p.score,
                    "placed_at": SystemTime::now(),
                });
                let _ = self.store.put(
                    &format!("placements/{}", p.workload_id),
                    meta.to_string().into_bytes(),
                );

                Ok(WorkloadResult {
                    workload_id: p.workload_id,
                    vm_id: Some(p.vm_id),
                    placed: true,
                })
            }
            Ok(None) => Ok(WorkloadResult {
                workload_id,
                vm_id: None,
                placed: false,
            }),
            Err(ScheduleError::NoPlacement(_)) => Ok(WorkloadResult {
                workload_id,
                vm_id: None,
                placed: false,
            }),
            Err(e) => Err(e.into()),
        }
    }

    /// Schedule all pending workloads in a batch. Returns placements made.
    pub fn schedule_pending(&self) -> Result<Vec<Placement>> {
        let available_vms = self.pool.list();
        let results = self.scheduler.schedule_batch(&available_vms);

        let mut placements = Vec::new();
        for p in results.into_iter().flatten() {
            let meta = serde_json::json!({
                "workload_id": p.workload_id,
                "vm_id": p.vm_id,
                "score": p.score,
                "placed_at": SystemTime::now(),
            });
            let _ = self.store.put(
                &format!("placements/{}", p.workload_id),
                meta.to_string().into_bytes(),
            );
            placements.push(p);
        }

        Ok(placements)
    }

    // ── Workflow Orchestration ─────────────────────────────────────────

    /// Submit and start a workflow, checkpointing initial state to the
    /// durable store.
    pub fn run_workflow(&self, spec: WorkflowSpec) -> Result<String> {
        // 1. Submit to workflow engine
        let workflow_id = self.workflow_engine.submit(spec)?;

        // 2. Start execution
        let ready_steps = self.workflow_engine.start(&workflow_id)?;

        // 3. Checkpoint initial state
        let meta = serde_json::json!({
            "workflow_id": workflow_id,
            "phase": "Running",
            "ready_steps": ready_steps,
            "started_at": SystemTime::now(),
        });
        let _ = self.store.put(
            &format!("workflows/{workflow_id}"),
            meta.to_string().into_bytes(),
        );

        Ok(workflow_id)
    }

    /// Advance a workflow step: start it, mark outcome, checkpoint, and
    /// return the next set of ready steps.
    pub fn advance_workflow_step(
        &self,
        workflow_id: &str,
        step_name: &str,
        outcome: StepOutcome,
    ) -> Result<Vec<String>> {
        // 1. Complete the step
        self.workflow_engine
            .complete_step(workflow_id, step_name, outcome)?;

        // 2. Get next ready steps (workflow may have been moved to completed)
        let ready = self
            .workflow_engine
            .ready_steps(workflow_id)
            .unwrap_or_default();

        // 3. Checkpoint progress
        let progress = self.workflow_engine.progress(workflow_id).unwrap_or(0.0);
        let meta = serde_json::json!({
            "workflow_id": workflow_id,
            "last_completed_step": step_name,
            "progress": progress,
            "ready_steps": ready,
            "updated_at": SystemTime::now(),
        });
        let _ = self.store.put(
            &format!("workflows/{workflow_id}"),
            meta.to_string().into_bytes(),
        );

        Ok(ready)
    }

    /// Cancel a running workflow and clean up its durable state.
    pub fn cancel_workflow(&self, workflow_id: &str) -> Result<()> {
        self.workflow_engine.cancel(workflow_id)?;
        let _ = self.store.delete(&format!("workflows/{workflow_id}"));
        Ok(())
    }

    // ── Maintenance Lifecycle ─────────────────────────────────────────

    /// Run one maintenance cycle across all subsystems.
    ///
    /// This should be called periodically (e.g., every 1-10 seconds) to:
    /// - Check VM health and replace unhealthy VMs
    /// - Expire idle gateway sessions
    /// - Evaluate autoscaling decisions
    /// - Provision or terminate VMs as needed
    /// - Garbage-collect expired store entries
    /// - Clean up maintenance candidates (expired idle/lifetime VMs)
    pub fn maintenance_tick(&self) -> MaintenanceReport {
        let mut report = MaintenanceReport {
            unhealthy_removed: Vec::new(),
            degraded_detected: Vec::new(),
            sessions_expired: Vec::new(),
            maintenance_candidates: Vec::new(),
            scale_decision: None,
            vms_provisioned: 0,
            vms_terminated: 0,
            store_gc_count: 0,
        };

        // 1. Health: identify unhealthy VMs
        let unhealthy = self.health_monitor.unhealthy_vms();
        for vm_id in &unhealthy {
            // Remove gateway routes for unhealthy VMs
            let affected_sessions = self.gateway.remove_vm_routes(vm_id);
            for session_id in &affected_sessions {
                let _ = self.store.delete(&format!("sessions/{session_id}"));
            }
            // Mark VM as failed in the pool
            let _ = self.pool.mark_failed(vm_id, "Health check failed");
            let _ = self.pool.terminate(vm_id);
            self.health_monitor.unregister(vm_id);
            report.vms_terminated += 1;
        }
        report.unhealthy_removed = unhealthy;

        // 2. Health: detect degraded VMs for alerting
        report.degraded_detected = self.health_monitor.degraded_vms();

        // 3. Gateway: expire idle sessions
        let expired_sessions = self.gateway.expire_idle();
        for session_id in &expired_sessions {
            // Clean up billing for expired sessions
            self.billing_engine.unregister_session(session_id);
            let _ = self.store.delete(&format!("sessions/{session_id}"));
        }
        report.sessions_expired = expired_sessions;

        // 4. Pool: collect maintenance candidates
        report.maintenance_candidates = self.pool.maintenance_candidates();
        for vm_id in &report.maintenance_candidates {
            let _ = self.pool.terminate(vm_id);
            self.health_monitor.unregister(vm_id);
            report.vms_terminated += 1;
        }

        // 5. Autoscale: evaluate current metrics
        let stats = self.pool.stats();
        let metrics = AutoscaleMetrics::from_pool(
            stats.total,
            stats.assigned,
            stats.warm,
            0, // pending workloads — scheduler doesn't expose count yet
        );
        let decision = self.autoscaler.evaluate(&metrics);
        self.autoscaler.record(decision.clone(), metrics);

        match decision.direction {
            ScaleDirection::Up => {
                for _ in 0..decision.count {
                    if let Ok(vm_id) = self.pool.provision() {
                        let _ = self.pool.mark_warm(&vm_id);
                        self.health_monitor.register(&vm_id);
                        report.vms_provisioned += 1;
                    }
                }
            }
            ScaleDirection::Down => {
                // Terminate excess warm VMs
                let warm_deficit = self
                    .pool
                    .warm_count()
                    .saturating_sub(self.pool.config().min_warm + decision.count);
                let vms = self.pool.list();
                let mut terminated = 0;
                for vm in &vms {
                    if terminated >= warm_deficit {
                        break;
                    }
                    if vm.state == VmSlotState::Warm {
                        let _ = self.pool.terminate(&vm.id);
                        self.health_monitor.unregister(&vm.id);
                        report.vms_terminated += 1;
                        terminated += 1;
                    }
                }
            }
            ScaleDirection::None => {}
        }
        report.scale_decision = Some(decision);

        // 6. Warm deficit: ensure min_warm target
        let deficit = self.pool.warm_deficit();
        for _ in 0..deficit {
            if let Ok(vm_id) = self.pool.provision() {
                let _ = self.pool.mark_warm(&vm_id);
                self.health_monitor.register(&vm_id);
                report.vms_provisioned += 1;
            }
        }

        // 7. Store: garbage collect expired entries
        report.store_gc_count = self.store.gc();

        // 8. Record maintenance tick metric
        self.metrics.on_maintenance_tick();

        report
    }

    // ── Observability ─────────────────────────────────────────────────

    /// Get a comprehensive status snapshot for dashboards / monitoring
    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            instance_id: self.config.instance_id.clone(),
            pool: self.pool.stats(),
            active_routes: self.gateway.route_count(),
            pending_workloads: 0, // scheduler doesn't expose pending count yet
            active_workflows: self.workflow_engine.active_count(),
            store_entries: self.store.len(),
            health_summary: self.health_monitor.summary(),
            billing_sessions: self.billing_engine.session_count(),
            scale_up_cooldown: self.autoscaler.is_scale_up_cooldown(),
            scale_down_cooldown: self.autoscaler.is_scale_down_cooldown(),
            timestamp: SystemTime::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a runtime with a warm pool
    fn runtime_with_warm_pool(warm_count: usize) -> Runtime {
        let config = RuntimeConfig::builder()
            .pool(PoolConfig {
                min_warm: warm_count,
                max_size: 64,
                ..Default::default()
            })
            .instance_id("test-runtime")
            .build();
        let rt = Runtime::new(config);

        // Pre-warm the pool
        for _ in 0..warm_count {
            let vm_id = rt.pool.provision().unwrap();
            rt.pool.mark_warm(&vm_id).unwrap();
            rt.health_monitor.register(&vm_id);
        }
        rt
    }

    #[test]
    fn test_runtime_default_config() {
        let config = RuntimeConfig::default();
        assert!(!config.instance_id.is_empty());
        assert_eq!(config.pool.max_size, 64);
        assert_eq!(config.scheduler.max_pending, 1000);
    }

    #[test]
    fn test_runtime_builder() {
        let config = RuntimeConfig::builder()
            .pool(PoolConfig {
                max_size: 128,
                ..Default::default()
            })
            .instance_id("test-runtime-1")
            .build();
        assert_eq!(config.pool.max_size, 128);
        assert_eq!(config.instance_id, "test-runtime-1");
    }

    #[test]
    fn test_runtime_creation() {
        let rt = Runtime::new(RuntimeConfig::default());
        assert!(!rt.instance_id().is_empty());
        let status = rt.status();
        assert_eq!(status.pool.total, 0);
    }

    #[test]
    fn test_runtime_accessors() {
        let rt = Runtime::new(RuntimeConfig::default());
        let _ = rt.pool();
        let _ = rt.scheduler();
        let _ = rt.workflow_engine();
        let _ = rt.store();
        let _ = rt.gateway();
        let _ = rt.autoscaler();
        let _ = rt.health_monitor();
        let _ = rt.billing_engine();
    }

    // ── Session lifecycle tests ───────────────────────────────────────

    #[test]
    fn test_create_session() {
        let rt = runtime_with_warm_pool(2);
        let session = rt
            .create_session("session-1", BillingTier::Standard)
            .unwrap();

        assert_eq!(session.session_id, "session-1");
        assert!(!session.vm_id.is_empty());
        assert_eq!(session.tier, BillingTier::Standard);

        // Verify side effects
        assert_eq!(rt.billing_engine.session_count(), 1);
        assert!(rt.store.exists("sessions/session-1"));
    }

    #[test]
    fn test_create_session_no_warm_vms() {
        let rt = Runtime::new(RuntimeConfig::default());
        let result = rt.create_session("session-1", BillingTier::Free);
        assert!(result.is_err());
    }

    #[test]
    fn test_destroy_session() {
        let rt = runtime_with_warm_pool(2);
        let session = rt
            .create_session("session-1", BillingTier::Standard)
            .unwrap();

        // Record some billing
        let _ = rt.billing_engine.record("session-1", "cpu_seconds", 100.0);

        let invoice = rt.destroy_session("session-1").unwrap();
        assert!(invoice.is_some());

        // Verify cleanup
        assert_eq!(rt.billing_engine.session_count(), 0);
        assert!(!rt.store.exists("sessions/session-1"));

        // VM should be recycled
        let vm = rt.pool.get(&session.vm_id);
        assert!(vm.is_some()); // Still in pool but recycled
    }

    #[test]
    fn test_destroy_nonexistent_session() {
        let rt = Runtime::new(RuntimeConfig::default());
        let result = rt.destroy_session("ghost");
        assert!(result.is_ok()); // Graceful no-op
    }

    #[test]
    fn test_multiple_sessions() {
        let rt = runtime_with_warm_pool(4);

        let s1 = rt.create_session("s1", BillingTier::Free).unwrap();
        let s2 = rt.create_session("s2", BillingTier::Standard).unwrap();
        let s3 = rt.create_session("s3", BillingTier::Premium).unwrap();

        assert_ne!(s1.vm_id, s2.vm_id);
        assert_ne!(s2.vm_id, s3.vm_id);
        assert_eq!(rt.billing_engine.session_count(), 3);

        rt.destroy_session("s2").unwrap();
        assert_eq!(rt.billing_engine.session_count(), 2);
    }

    // ── Workload scheduling tests ─────────────────────────────────────

    #[test]
    fn test_submit_workload_immediate_placement() {
        let rt = runtime_with_warm_pool(2);
        let workload = WorkloadDescriptor::new("w1", "s1");
        let result = rt.submit_workload(workload).unwrap();

        assert_eq!(result.workload_id, "w1");
        assert!(result.placed);
        assert!(result.vm_id.is_some());
        assert!(rt.store.exists("placements/w1"));
    }

    #[test]
    fn test_submit_workload_impossible_placement() {
        let rt = runtime_with_warm_pool(2);
        let workload = WorkloadDescriptor::new("w1", "s1").vcpus(9999);
        let result = rt.submit_workload(workload).unwrap();

        // Should not error — just returns placed=false
        assert!(!result.placed);
        assert!(result.vm_id.is_none());
    }

    #[test]
    fn test_schedule_pending_batch() {
        let rt = runtime_with_warm_pool(4);

        // Submit multiple workloads
        rt.scheduler
            .submit(WorkloadDescriptor::new("w1", "s1"))
            .unwrap();
        rt.scheduler
            .submit(WorkloadDescriptor::new("w2", "s2"))
            .unwrap();
        rt.scheduler
            .submit(WorkloadDescriptor::new("w3", "s3"))
            .unwrap();

        let placements = rt.schedule_pending().unwrap();
        assert!(!placements.is_empty());

        for p in &placements {
            assert!(rt.store.exists(&format!("placements/{}", p.workload_id)));
        }
    }

    // ── Workflow orchestration tests ──────────────────────────────────

    #[test]
    fn test_run_workflow() {
        let rt = runtime_with_warm_pool(2);

        let spec = WorkflowBuilder::new("pipeline")
            .step(StepSpec::new("step-1", "echo hello"))
            .step(StepSpec::new("step-2", "echo world").depends_on("step-1"))
            .build()
            .unwrap();

        let wf_id = rt.run_workflow(spec).unwrap();
        assert!(!wf_id.is_empty());
        assert!(rt.store.exists(&format!("workflows/{wf_id}")));

        // Workflow should be running
        let wf = rt.workflow_engine.get(&wf_id).unwrap();
        assert_eq!(wf.phase, WorkflowPhase::Running);
    }

    #[test]
    fn test_advance_workflow_step() {
        let rt = runtime_with_warm_pool(2);

        let spec = WorkflowBuilder::new("pipeline")
            .step(StepSpec::new("step-1", "echo hello"))
            .step(StepSpec::new("step-2", "echo world").depends_on("step-1"))
            .build()
            .unwrap();

        let wf_id = rt.run_workflow(spec).unwrap();

        // Start and complete step-1
        let _ = rt.workflow_engine.start_step(&wf_id, "step-1");
        let ready = rt
            .advance_workflow_step(
                &wf_id,
                "step-1",
                StepOutcome::Success {
                    output: Some("done".to_string()),
                },
            )
            .unwrap();

        // step-2 should now be ready
        assert!(ready.contains(&"step-2".to_string()));

        // Complete step-2
        let _ = rt.workflow_engine.start_step(&wf_id, "step-2");
        let ready = rt
            .advance_workflow_step(
                &wf_id,
                "step-2",
                StepOutcome::Success {
                    output: Some("done".to_string()),
                },
            )
            .unwrap();
        assert!(ready.is_empty());

        // Workflow should be completed
        let wf = rt.workflow_engine.get(&wf_id).unwrap();
        assert_eq!(wf.phase, WorkflowPhase::Completed);
    }

    #[test]
    fn test_cancel_workflow() {
        let rt = runtime_with_warm_pool(2);

        let spec = WorkflowBuilder::new("cancel-me")
            .step(StepSpec::new("step-1", "echo hello"))
            .build()
            .unwrap();

        let wf_id = rt.run_workflow(spec).unwrap();
        rt.cancel_workflow(&wf_id).unwrap();

        // Store entry should be removed
        assert!(!rt.store.exists(&format!("workflows/{wf_id}")));
    }

    // ── Maintenance lifecycle tests ───────────────────────────────────

    #[test]
    fn test_maintenance_tick_provisions_warm_deficit() {
        let config = RuntimeConfig::builder()
            .pool(PoolConfig {
                min_warm: 4,
                max_size: 64,
                ..Default::default()
            })
            .build();
        let rt = Runtime::new(config);

        // Pool starts empty — maintenance should provision min_warm VMs
        let report = rt.maintenance_tick();
        assert!(report.vms_provisioned >= 4);
        assert!(rt.pool.warm_count() >= 4);
    }

    #[test]
    fn test_maintenance_tick_gc() {
        let rt = runtime_with_warm_pool(2);

        // Add a store entry with zero TTL (immediately expired)
        let _ = rt.store.put_with_ttl(
            "ephemeral/key",
            b"value".to_vec(),
            Some(std::time::Duration::from_secs(0)),
        );

        // Wait a tiny bit to ensure expiry
        std::thread::sleep(std::time::Duration::from_millis(10));

        let report = rt.maintenance_tick();
        assert!(report.store_gc_count >= 1);
    }

    #[test]
    fn test_maintenance_tick_reports_autoscale() {
        let rt = runtime_with_warm_pool(2);
        let report = rt.maintenance_tick();
        assert!(report.scale_decision.is_some());
    }

    // ── Status / Observability tests ─────────────────────────────────

    #[test]
    fn test_runtime_status() {
        let rt = runtime_with_warm_pool(3);

        // Create a session to make things interesting
        rt.create_session("s1", BillingTier::Standard).unwrap();

        let status = rt.status();
        assert_eq!(status.instance_id, "test-runtime");
        assert_eq!(status.pool.total, 3);
        assert!(status.pool.assigned >= 1);
        assert_eq!(status.billing_sessions, 1);
        assert!(status.store_entries >= 1); // session metadata
    }

    #[test]
    fn test_runtime_status_after_maintenance() {
        let config = RuntimeConfig::builder()
            .pool(PoolConfig {
                min_warm: 2,
                max_size: 64,
                ..Default::default()
            })
            .build();
        let rt = Runtime::new(config);

        // Maintenance should provision warm VMs
        rt.maintenance_tick();
        let status = rt.status();
        assert!(status.pool.total >= 2);
    }

    // ── End-to-end integration test ──────────────────────────────────

    #[test]
    fn test_full_lifecycle() {
        let rt = runtime_with_warm_pool(4);

        // 1. Create two sessions
        let s1 = rt.create_session("agent-1", BillingTier::Standard).unwrap();
        let s2 = rt.create_session("agent-2", BillingTier::Premium).unwrap();
        assert_ne!(s1.vm_id, s2.vm_id);

        // 2. Record some billing
        rt.billing_engine
            .record("agent-1", "cpu_seconds", 500.0)
            .unwrap();
        rt.billing_engine
            .record("agent-2", "cpu_seconds", 1000.0)
            .unwrap();

        // 3. Submit a workflow
        let spec = WorkflowBuilder::new("e2e-pipeline")
            .step(StepSpec::new("ingest", "download data"))
            .step(StepSpec::new("transform", "process data").depends_on("ingest"))
            .step(StepSpec::new("export", "upload results").depends_on("transform"))
            .build()
            .unwrap();
        let wf_id = rt.run_workflow(spec).unwrap();

        // 4. Run workflow to completion
        let _ = rt.workflow_engine.start_step(&wf_id, "ingest");
        rt.advance_workflow_step(
            &wf_id,
            "ingest",
            StepOutcome::Success {
                output: Some("ok".to_string()),
            },
        )
        .unwrap();
        let _ = rt.workflow_engine.start_step(&wf_id, "transform");
        rt.advance_workflow_step(
            &wf_id,
            "transform",
            StepOutcome::Success {
                output: Some("ok".to_string()),
            },
        )
        .unwrap();
        let _ = rt.workflow_engine.start_step(&wf_id, "export");
        rt.advance_workflow_step(
            &wf_id,
            "export",
            StepOutcome::Success {
                output: Some("ok".to_string()),
            },
        )
        .unwrap();

        let wf = rt.workflow_engine.get(&wf_id).unwrap();
        assert_eq!(wf.phase, WorkflowPhase::Completed);

        // 5. Run maintenance
        let report = rt.maintenance_tick();
        assert!(report.scale_decision.is_some());

        // 6. Destroy sessions and check invoices
        let inv1 = rt.destroy_session("agent-1").unwrap().unwrap();
        assert!(inv1.total() > 0.0);

        let inv2 = rt.destroy_session("agent-2").unwrap().unwrap();
        assert!(inv2.total() > 0.0);
        assert!(inv2.total() >= inv1.total()); // Premium tier or more usage

        // 7. Check final status
        let status = rt.status();
        assert_eq!(status.billing_sessions, 0);
    }
}
