//! Multi-Agent Orchestration Layer
//!
//! This module provides coordination primitives for multi-agent VM management.
//! It enables multiple AI agents to collaborate on VM operations with proper
//! isolation, resource locking, and conflict resolution.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                    AgentOrchestrator                           │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐         │
//! │  │  Agent A     │  │  Agent B     │  │  Agent C     │         │
//! │  │  (Operator)  │  │  (Monitor)   │  │  (Security)  │         │
//! │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘         │
//! │         │                 │                 │                  │
//! │  ┌──────┴─────────────────┴─────────────────┴───────┐         │
//! │  │              Message Bus (Pub/Sub)                │         │
//! │  └──────┬─────────────────┬─────────────────┬───────┘         │
//! │         │                 │                 │                  │
//! │  ┌──────┴───────┐  ┌──────┴───────┐  ┌──────┴───────┐         │
//! │  │ Resource     │  │ Lock         │  │ Event        │         │
//! │  │ Manager      │  │ Manager      │  │ Dispatcher   │         │
//! │  └──────────────┘  └──────────────┘  └──────────────┘         │
//! └────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use hv2_agent::orchestration::{AgentOrchestrator, AgentRole};
//!
//! // Create orchestrator
//! let orchestrator = AgentOrchestrator::new();
//!
//! // Register agents with different roles
//! orchestrator.register_agent("agent-1", AgentRole::Operator).await?;
//! orchestrator.register_agent("agent-2", AgentRole::Monitor).await?;
//!
//! // Agent 1 claims a VM for exclusive access
//! orchestrator.claim_vm("agent-1", "vm-1").await?;
//!
//! // Agent 2 can still read VM status (observer access)
//! let status = orchestrator.get_vm_status("agent-2", "vm-1").await?;
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// Agent orchestrator - coordinates multiple agents
pub struct AgentOrchestrator {
    /// Registered agents
    agents: RwLock<HashMap<String, AgentInfo>>,
    /// VM claim locks
    vm_claims: RwLock<HashMap<String, VmClaim>>,
    /// Message channels (pub/sub)
    channels: RwLock<HashMap<String, Channel>>,
    /// Pending messages per agent
    agent_messages: RwLock<HashMap<String, VecDeque<AgentMessage>>>,
    /// Event subscribers
    event_subscribers: RwLock<HashMap<EventType, Vec<String>>>,
    /// Configuration
    config: OrchestratorConfig,
}

/// Orchestrator configuration
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Maximum agents
    pub max_agents: usize,
    /// Default claim duration
    pub default_claim_duration: Duration,
    /// Message queue size per agent
    pub message_queue_size: usize,
    /// Enable conflict detection
    pub conflict_detection: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_agents: 100,
            default_claim_duration: Duration::from_secs(300),
            message_queue_size: 1000,
            conflict_detection: true,
        }
    }
}

/// Agent role in the orchestration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    /// Full access operator - can create/delete/modify VMs
    Operator,
    /// Read-only monitor - can observe but not modify
    Monitor,
    /// Security agent - can audit and enforce policies
    Security,
    /// Backup agent - can create/restore snapshots
    Backup,
    /// Network agent - can manage networking
    Network,
    /// Scaling agent - can resize and migrate VMs
    Scaler,
    /// Custom role with explicit permissions
    Custom,
}

/// Registered agent information
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// Agent identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Agent role
    pub role: AgentRole,
    /// Registration time
    pub registered_at: Instant,
    /// Last heartbeat
    pub last_heartbeat: Instant,
    /// Agent metadata
    pub metadata: HashMap<String, JsonValue>,
    /// Currently claimed VMs
    pub claimed_vms: HashSet<String>,
    /// Agent state
    pub state: AgentState,
}

/// Agent state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    /// Agent is active and responsive
    Active,
    /// Agent is idle (no recent activity)
    Idle,
    /// Agent is busy with a long-running task
    Busy,
    /// Agent is disconnected
    Disconnected,
}

/// VM claim (exclusive lock)
#[derive(Debug, Clone)]
pub struct VmClaim {
    /// VM name
    pub vm_name: String,
    /// Claiming agent
    pub agent_id: String,
    /// Claim start time
    pub claimed_at: Instant,
    /// Claim expiry
    pub expires_at: Instant,
    /// Claim reason
    pub reason: Option<String>,
    /// Whether the claim is exclusive (blocks writes from others)
    pub exclusive: bool,
}

/// Message channel for pub/sub
#[derive(Debug, Clone)]
pub struct Channel {
    /// Channel name
    pub name: String,
    /// Subscribed agents
    pub subscribers: HashSet<String>,
    /// Message history
    pub history: VecDeque<AgentMessage>,
    /// Max history size
    pub max_history: usize,
}

/// Message between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Message ID
    pub id: String,
    /// Sender agent
    pub sender: String,
    /// Target (agent ID, channel name, or "broadcast")
    pub target: String,
    /// Message type
    pub message_type: MessageType,
    /// Priority
    pub priority: MessagePriority,
    /// Payload
    pub payload: JsonValue,
    /// Timestamp
    pub timestamp: SystemTime,
    /// Whether the message requires acknowledgment
    pub requires_ack: bool,
}

/// Message type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Informational message
    Info,
    /// Request expecting a response
    Request,
    /// Response to a request
    Response,
    /// Task assignment
    Task,
    /// Task completion notification
    TaskComplete,
    /// Alert/warning
    Alert,
    /// Coordination message
    Coordination,
    /// Heartbeat
    Heartbeat,
}

/// Message priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MessagePriority {
    Low,
    Normal,
    High,
    Critical,
}

/// Event types for pub/sub
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    /// VM created
    VmCreated,
    /// VM started
    VmStarted,
    /// VM stopped
    VmStopped,
    /// VM deleted
    VmDeleted,
    /// VM resource changed
    VmResourceChanged,
    /// Snapshot created
    SnapshotCreated,
    /// Agent connected
    AgentConnected,
    /// Agent disconnected
    AgentDisconnected,
    /// Security alert
    SecurityAlert,
    /// Resource threshold exceeded
    ResourceAlert,
}

/// Conflict detected between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Conflict ID
    pub id: String,
    /// Agents involved
    pub agents: Vec<String>,
    /// Resource being contested
    pub resource: String,
    /// Conflict type
    pub conflict_type: ConflictType,
    /// Suggested resolution
    pub resolution: Option<ConflictResolution>,
    /// Timestamp
    pub timestamp: SystemTime,
}

/// Type of conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictType {
    /// Multiple agents trying to modify same VM
    ConcurrentModification,
    /// Agent trying to access claimed resource
    ClaimViolation,
    /// Agent exceeded its role permissions
    RoleViolation,
    /// Conflicting operations scheduled
    OperationConflict,
}

/// Resolution for a conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// First-come-first-served
    FirstWins,
    /// Higher priority agent wins
    PriorityWins,
    /// All operations rejected
    RejectAll,
    /// Queue operations
    Queue,
    /// Manual resolution required
    Manual,
}

/// Orchestration error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrchestrationError {
    /// Agent not found
    AgentNotFound(String),
    /// VM not found
    VmNotFound(String),
    /// Claim already exists
    ClaimExists { vm: String, owner: String },
    /// No claim exists
    NoClaim(String),
    /// Permission denied
    PermissionDenied { agent: String, action: String },
    /// Conflict detected
    Conflict(Conflict),
    /// Rate limit exceeded
    RateLimitExceeded(String),
    /// Internal error
    Internal(String),
}

impl std::fmt::Display for OrchestrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentNotFound(id) => write!(f, "Agent not found: {}", id),
            Self::VmNotFound(vm) => write!(f, "VM not found: {}", vm),
            Self::ClaimExists { vm, owner } => write!(f, "VM '{}' already claimed by '{}'", vm, owner),
            Self::NoClaim(vm) => write!(f, "No claim on VM: {}", vm),
            Self::PermissionDenied { agent, action } => {
                write!(f, "Permission denied for agent '{}' to perform '{}'", agent, action)
            }
            Self::Conflict(c) => write!(f, "Conflict: {:?}", c.conflict_type),
            Self::RateLimitExceeded(agent) => write!(f, "Rate limit exceeded for agent: {}", agent),
            Self::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for OrchestrationError {}

impl AgentOrchestrator {
    /// Create a new orchestrator
    pub fn new() -> Self {
        Self::with_config(OrchestratorConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: OrchestratorConfig) -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            vm_claims: RwLock::new(HashMap::new()),
            channels: RwLock::new(HashMap::new()),
            agent_messages: RwLock::new(HashMap::new()),
            event_subscribers: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Register a new agent
    pub fn register_agent(
        &self,
        id: &str,
        name: &str,
        role: AgentRole,
    ) -> Result<AgentInfo, OrchestrationError> {
        let mut agents = self.agents.write().unwrap();

        if agents.len() >= self.config.max_agents {
            return Err(OrchestrationError::Internal("Maximum agents reached".to_string()));
        }

        let info = AgentInfo {
            id: id.to_string(),
            name: name.to_string(),
            role,
            registered_at: Instant::now(),
            last_heartbeat: Instant::now(),
            metadata: HashMap::new(),
            claimed_vms: HashSet::new(),
            state: AgentState::Active,
        };

        agents.insert(id.to_string(), info.clone());

        // Initialize message queue
        self.agent_messages
            .write()
            .unwrap()
            .insert(id.to_string(), VecDeque::new());

        // Emit event
        self.emit_event(EventType::AgentConnected, json!({ "agent_id": id }));

        Ok(info)
    }

    /// Unregister an agent
    pub fn unregister_agent(&self, agent_id: &str) -> Result<(), OrchestrationError> {
        let mut agents = self.agents.write().unwrap();

        if agents.remove(agent_id).is_none() {
            return Err(OrchestrationError::AgentNotFound(agent_id.to_string()));
        }

        // Release all claims
        let mut claims = self.vm_claims.write().unwrap();
        claims.retain(|_, claim| claim.agent_id != agent_id);

        // Remove from channels
        let mut channels = self.channels.write().unwrap();
        for channel in channels.values_mut() {
            channel.subscribers.remove(agent_id);
        }

        // Clear message queue
        self.agent_messages.write().unwrap().remove(agent_id);

        // Emit event
        self.emit_event(EventType::AgentDisconnected, json!({ "agent_id": agent_id }));

        Ok(())
    }

    /// Update agent heartbeat
    pub fn heartbeat(&self, agent_id: &str) -> Result<(), OrchestrationError> {
        let mut agents = self.agents.write().unwrap();
        let agent = agents
            .get_mut(agent_id)
            .ok_or_else(|| OrchestrationError::AgentNotFound(agent_id.to_string()))?;

        agent.last_heartbeat = Instant::now();
        if agent.state == AgentState::Disconnected {
            agent.state = AgentState::Active;
        }

        Ok(())
    }

    /// Claim exclusive access to a VM
    pub fn claim_vm(
        &self,
        agent_id: &str,
        vm_name: &str,
        reason: Option<&str>,
        duration: Option<Duration>,
    ) -> Result<VmClaim, OrchestrationError> {
        // Verify agent exists and has permission
        {
            let agents = self.agents.read().unwrap();
            let agent = agents
                .get(agent_id)
                .ok_or_else(|| OrchestrationError::AgentNotFound(agent_id.to_string()))?;

            // Check role permissions
            if !self.role_can_claim(agent.role) {
                return Err(OrchestrationError::PermissionDenied {
                    agent: agent_id.to_string(),
                    action: "claim_vm".to_string(),
                });
            }
        }

        // Check for existing claim
        let mut claims = self.vm_claims.write().unwrap();
        if let Some(existing) = claims.get(vm_name) {
            if existing.agent_id != agent_id && existing.expires_at > Instant::now() {
                return Err(OrchestrationError::ClaimExists {
                    vm: vm_name.to_string(),
                    owner: existing.agent_id.clone(),
                });
            }
        }

        let duration = duration.unwrap_or(self.config.default_claim_duration);
        let claim = VmClaim {
            vm_name: vm_name.to_string(),
            agent_id: agent_id.to_string(),
            claimed_at: Instant::now(),
            expires_at: Instant::now() + duration,
            reason: reason.map(|s| s.to_string()),
            exclusive: true,
        };

        claims.insert(vm_name.to_string(), claim.clone());

        // Update agent's claimed VMs
        let mut agents = self.agents.write().unwrap();
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.claimed_vms.insert(vm_name.to_string());
        }

        Ok(claim)
    }

    /// Release a VM claim
    pub fn release_vm(&self, agent_id: &str, vm_name: &str) -> Result<(), OrchestrationError> {
        let mut claims = self.vm_claims.write().unwrap();
        let claim = claims
            .get(vm_name)
            .ok_or_else(|| OrchestrationError::NoClaim(vm_name.to_string()))?;

        if claim.agent_id != agent_id {
            return Err(OrchestrationError::PermissionDenied {
                agent: agent_id.to_string(),
                action: format!("release claim on '{}' owned by '{}'", vm_name, claim.agent_id),
            });
        }

        claims.remove(vm_name);

        // Update agent's claimed VMs
        let mut agents = self.agents.write().unwrap();
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.claimed_vms.remove(vm_name);
        }

        Ok(())
    }

    /// Check if an agent can perform an action on a VM
    pub fn can_access_vm(
        &self,
        agent_id: &str,
        vm_name: &str,
        write: bool,
    ) -> Result<bool, OrchestrationError> {
        // Verify agent exists
        let agents = self.agents.read().unwrap();
        let agent = agents
            .get(agent_id)
            .ok_or_else(|| OrchestrationError::AgentNotFound(agent_id.to_string()))?;

        // Read access is always allowed for Monitor+ roles
        if !write && self.role_can_read(agent.role) {
            return Ok(true);
        }

        // Check role write permission
        if write && !self.role_can_write(agent.role) {
            return Ok(false);
        }

        // Check claims
        let claims = self.vm_claims.read().unwrap();
        if let Some(claim) = claims.get(vm_name) {
            if claim.exclusive && claim.agent_id != agent_id && claim.expires_at > Instant::now() {
                // VM is claimed by another agent
                if write {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Send a message to a specific agent
    pub fn send_message(
        &self,
        from: &str,
        to: &str,
        message_type: MessageType,
        payload: JsonValue,
    ) -> Result<String, OrchestrationError> {
        // Verify sender exists
        {
            let agents = self.agents.read().unwrap();
            if !agents.contains_key(from) {
                return Err(OrchestrationError::AgentNotFound(from.to_string()));
            }
            if !agents.contains_key(to) {
                return Err(OrchestrationError::AgentNotFound(to.to_string()));
            }
        }

        let msg_id = generate_id();
        let message = AgentMessage {
            id: msg_id.clone(),
            sender: from.to_string(),
            target: to.to_string(),
            message_type,
            priority: MessagePriority::Normal,
            payload,
            timestamp: SystemTime::now(),
            requires_ack: message_type == MessageType::Request,
        };

        let mut queues = self.agent_messages.write().unwrap();
        if let Some(queue) = queues.get_mut(to) {
            // Check queue size
            if queue.len() >= self.config.message_queue_size {
                queue.pop_front();
            }
            queue.push_back(message);
        }

        Ok(msg_id)
    }

    /// Broadcast to a channel
    pub fn broadcast(
        &self,
        from: &str,
        channel: &str,
        payload: JsonValue,
    ) -> Result<usize, OrchestrationError> {
        // Verify sender exists
        {
            let agents = self.agents.read().unwrap();
            if !agents.contains_key(from) {
                return Err(OrchestrationError::AgentNotFound(from.to_string()));
            }
        }

        let channels = self.channels.read().unwrap();
        let channel_info = match channels.get(channel) {
            Some(c) => c.clone(),
            None => return Ok(0),
        };
        drop(channels);

        let message = AgentMessage {
            id: generate_id(),
            sender: from.to_string(),
            target: channel.to_string(),
            message_type: MessageType::Info,
            priority: MessagePriority::Normal,
            payload,
            timestamp: SystemTime::now(),
            requires_ack: false,
        };

        let mut queues = self.agent_messages.write().unwrap();
        let mut delivered = 0;

        for subscriber in &channel_info.subscribers {
            if subscriber != from {
                if let Some(queue) = queues.get_mut(subscriber) {
                    if queue.len() >= self.config.message_queue_size {
                        queue.pop_front();
                    }
                    queue.push_back(message.clone());
                    delivered += 1;
                }
            }
        }

        // Store in channel history
        let mut channels = self.channels.write().unwrap();
        if let Some(ch) = channels.get_mut(channel) {
            if ch.history.len() >= ch.max_history {
                ch.history.pop_front();
            }
            ch.history.push_back(message);
        }

        Ok(delivered)
    }

    /// Subscribe to a channel
    pub fn subscribe(&self, agent_id: &str, channel: &str) -> Result<(), OrchestrationError> {
        // Verify agent exists
        {
            let agents = self.agents.read().unwrap();
            if !agents.contains_key(agent_id) {
                return Err(OrchestrationError::AgentNotFound(agent_id.to_string()));
            }
        }

        let mut channels = self.channels.write().unwrap();
        let channel_info = channels.entry(channel.to_string()).or_insert_with(|| Channel {
            name: channel.to_string(),
            subscribers: HashSet::new(),
            history: VecDeque::new(),
            max_history: 100,
        });

        channel_info.subscribers.insert(agent_id.to_string());
        Ok(())
    }

    /// Receive pending messages for an agent
    pub fn receive_messages(&self, agent_id: &str, limit: usize) -> Vec<AgentMessage> {
        let mut queues = self.agent_messages.write().unwrap();
        match queues.get_mut(agent_id) {
            Some(queue) => {
                let count = std::cmp::min(limit, queue.len());
                queue.drain(..count).collect()
            }
            None => Vec::new(),
        }
    }

    /// Subscribe to event type
    pub fn subscribe_event(&self, agent_id: &str, event_type: EventType) {
        let mut subs = self.event_subscribers.write().unwrap();
        subs.entry(event_type)
            .or_default()
            .push(agent_id.to_string());
    }

    /// Emit an event
    pub fn emit_event(&self, event_type: EventType, data: JsonValue) {
        let subs = self.event_subscribers.read().unwrap();
        if let Some(subscribers) = subs.get(&event_type) {
            let message = AgentMessage {
                id: generate_id(),
                sender: "system".to_string(),
                target: "event".to_string(),
                message_type: MessageType::Info,
                priority: MessagePriority::Normal,
                payload: json!({
                    "event": format!("{:?}", event_type),
                    "data": data
                }),
                timestamp: SystemTime::now(),
                requires_ack: false,
            };

            let mut queues = self.agent_messages.write().unwrap();
            for agent_id in subscribers {
                if let Some(queue) = queues.get_mut(agent_id) {
                    if queue.len() < self.config.message_queue_size {
                        queue.push_back(message.clone());
                    }
                }
            }
        }
    }

    /// List all registered agents
    pub fn list_agents(&self) -> Vec<AgentInfo> {
        let agents = self.agents.read().unwrap();
        agents.values().cloned().collect()
    }

    /// List all VM claims
    pub fn list_claims(&self) -> Vec<VmClaim> {
        let claims = self.vm_claims.read().unwrap();
        claims.values().cloned().collect()
    }

    /// Detect and report conflicts
    pub fn detect_conflicts(&self) -> Vec<Conflict> {
        if !self.config.conflict_detection {
            return Vec::new();
        }

        let mut conflicts = Vec::new();

        // Check for expired claims
        let claims = self.vm_claims.read().unwrap();
        for claim in claims.values() {
            if claim.expires_at <= Instant::now() {
                conflicts.push(Conflict {
                    id: generate_id(),
                    agents: vec![claim.agent_id.clone()],
                    resource: claim.vm_name.clone(),
                    conflict_type: ConflictType::ClaimViolation,
                    resolution: Some(ConflictResolution::FirstWins),
                    timestamp: SystemTime::now(),
                });
            }
        }

        conflicts
    }

    /// Clean up expired claims and disconnected agents
    pub fn cleanup(&self) {
        // Clean expired claims
        let mut claims = self.vm_claims.write().unwrap();
        claims.retain(|_, claim| claim.expires_at > Instant::now());

        // Mark agents as disconnected if no heartbeat
        let mut agents = self.agents.write().unwrap();
        let timeout = Duration::from_secs(60);
        for agent in agents.values_mut() {
            if agent.last_heartbeat.elapsed() > timeout {
                agent.state = AgentState::Disconnected;
            }
        }
    }

    // Helper: check if role can claim VMs
    fn role_can_claim(&self, role: AgentRole) -> bool {
        matches!(
            role,
            AgentRole::Operator | AgentRole::Scaler | AgentRole::Custom
        )
    }

    // Helper: check if role can read
    fn role_can_read(&self, role: AgentRole) -> bool {
        matches!(
            role,
            AgentRole::Operator
                | AgentRole::Monitor
                | AgentRole::Security
                | AgentRole::Backup
                | AgentRole::Network
                | AgentRole::Scaler
                | AgentRole::Custom
        )
    }

    // Helper: check if role can write
    fn role_can_write(&self, role: AgentRole) -> bool {
        matches!(
            role,
            AgentRole::Operator | AgentRole::Scaler | AgentRole::Custom
        )
    }
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique ID
fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", timestamp)
}

/// Task for multi-agent workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTask {
    /// Task ID
    pub id: String,
    /// Task name
    pub name: String,
    /// Task description
    pub description: String,
    /// Required role to execute
    pub required_role: AgentRole,
    /// Dependencies (task IDs that must complete first)
    pub dependencies: Vec<String>,
    /// Input parameters
    pub input: JsonValue,
    /// Task state
    pub state: TaskState,
    /// Assigned agent (if any)
    pub assigned_to: Option<String>,
    /// Result (if completed)
    pub result: Option<JsonValue>,
}

/// Task state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    Ready,
    Assigned,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Workflow for coordinating multiple agents
#[derive(Debug, Clone)]
pub struct Workflow {
    /// Workflow ID
    pub id: String,
    /// Workflow name
    pub name: String,
    /// Tasks in the workflow
    pub tasks: Vec<WorkflowTask>,
    /// Workflow state
    pub state: WorkflowState,
    /// Created timestamp
    pub created_at: SystemTime,
}

/// Workflow state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    Created,
    Running,
    Paused,
    Completed,
    Failed,
}

impl Workflow {
    /// Create a new workflow
    pub fn new(name: &str) -> Self {
        Self {
            id: generate_id(),
            name: name.to_string(),
            tasks: Vec::new(),
            state: WorkflowState::Created,
            created_at: SystemTime::now(),
        }
    }

    /// Add a task to the workflow
    pub fn add_task(&mut self, task: WorkflowTask) {
        self.tasks.push(task);
    }

    /// Get ready tasks (dependencies satisfied)
    pub fn get_ready_tasks(&self) -> Vec<&WorkflowTask> {
        self.tasks
            .iter()
            .filter(|t| {
                t.state == TaskState::Ready
                    && t.dependencies.iter().all(|dep| {
                        self.tasks
                            .iter()
                            .any(|d| &d.id == dep && d.state == TaskState::Completed)
                    })
            })
            .collect()
    }

    /// Update task state
    pub fn update_task_state(&mut self, task_id: &str, state: TaskState) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.state = state;
        }
    }

    /// Check if workflow is complete
    pub fn is_complete(&self) -> bool {
        self.tasks.iter().all(|t| {
            t.state == TaskState::Completed || t.state == TaskState::Cancelled
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_registration() {
        let orch = AgentOrchestrator::new();
        let agent = orch
            .register_agent("agent-1", "Test Agent", AgentRole::Operator)
            .unwrap();

        assert_eq!(agent.id, "agent-1");
        assert_eq!(agent.role, AgentRole::Operator);
    }

    #[test]
    fn test_vm_claim() {
        let orch = AgentOrchestrator::new();
        orch.register_agent("agent-1", "Agent 1", AgentRole::Operator)
            .unwrap();

        let claim = orch
            .claim_vm("agent-1", "vm-1", Some("testing"), None)
            .unwrap();

        assert_eq!(claim.vm_name, "vm-1");
        assert_eq!(claim.agent_id, "agent-1");
    }

    #[test]
    fn test_claim_conflict() {
        let orch = AgentOrchestrator::new();
        orch.register_agent("agent-1", "Agent 1", AgentRole::Operator)
            .unwrap();
        orch.register_agent("agent-2", "Agent 2", AgentRole::Operator)
            .unwrap();

        // Agent 1 claims VM
        orch.claim_vm("agent-1", "vm-1", None, None).unwrap();

        // Agent 2 tries to claim same VM
        let result = orch.claim_vm("agent-2", "vm-1", None, None);
        assert!(matches!(result, Err(OrchestrationError::ClaimExists { .. })));
    }

    #[test]
    fn test_monitor_read_access() {
        let orch = AgentOrchestrator::new();
        orch.register_agent("operator", "Operator", AgentRole::Operator)
            .unwrap();
        orch.register_agent("monitor", "Monitor", AgentRole::Monitor)
            .unwrap();

        // Operator claims VM
        orch.claim_vm("operator", "vm-1", None, None).unwrap();

        // Monitor can still read
        assert!(orch.can_access_vm("monitor", "vm-1", false).unwrap());

        // Monitor cannot write
        assert!(!orch.can_access_vm("monitor", "vm-1", true).unwrap());
    }

    #[test]
    fn test_messaging() {
        let orch = AgentOrchestrator::new();
        orch.register_agent("agent-1", "Agent 1", AgentRole::Operator)
            .unwrap();
        orch.register_agent("agent-2", "Agent 2", AgentRole::Operator)
            .unwrap();

        // Send message
        orch.send_message(
            "agent-1",
            "agent-2",
            MessageType::Request,
            json!({"action": "status"}),
        )
        .unwrap();

        // Receive message
        let messages = orch.receive_messages("agent-2", 10);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender, "agent-1");
    }
}
