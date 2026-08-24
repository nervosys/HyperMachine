//! AI Agent interface for HyperMachine
//!
//! This crate provides a safe, scriptable interface for AI agents to control
//! virtual machines. It includes sandboxed execution, resource limits, and
//! comprehensive observability.
//!
//! # Agent Interface Design
//!
//! HyperMachine is designed for agentic AI systems, providing:
//!
//! - **MCP-Compatible Tools**: Function calling interface compatible with OpenAI
//!   and Anthropic tool-use patterns via the [`mcp`] module
//! - **Multi-Agent Orchestration**: Coordination primitives for multiple agents
//!   to collaborate on VM management via the [`orchestration`] module
//! - **Capability-Based Security**: Fine-grained permissions for agent actions
//! - **Audit Logging**: Complete traceability of all agent operations
//!
//! # Example
//!
//! ```rust,ignore
//! use hv2_agent::{McpServer, AgentCapabilities, AgentOrchestrator};
//!
//! // Create MCP server for tool-use interface
//! let mcp = McpServer::new();
//!
//! // Register an AI agent with operator capabilities
//! let session = mcp.create_session("ai-assistant", AgentCapabilities::operator())?;
//!
//! // Agent can discover available tools
//! let tools = session.list_tools(&mcp);
//!
//! // For multi-agent scenarios, use the orchestrator
//! let orchestrator = AgentOrchestrator::new();
//! orchestrator.register_agent("agent-1", "Operator", AgentRole::Operator)?;
//! orchestrator.register_agent("agent-2", "Monitor", AgentRole::Monitor)?;
//! ```

#![allow(dead_code)]

pub mod actions;
pub mod agent_vm;
pub mod capabilities;
pub mod communication;
pub mod events;
pub mod gpu_host;
pub mod image_host;
pub mod learning;
pub mod limits;
pub mod mcp;
pub mod memory;
pub mod orchestration;
pub mod perception;
pub mod permissions;
pub mod planning;
pub mod policies;
pub mod reasoning;
pub mod runtime;
pub mod sandbox;
pub mod script;
pub mod state;
pub mod tasks;
pub mod telemetry;
pub mod tools;
pub mod vm_host;

pub use actions::{
    ActionCategory, ActionError, ActionExecutor, ActionQueue, ActionRequest, ActionResponse,
    ActionResult, ActionValidator, AgentAction, DebugAction, Direction, FirewallAction,
    FirewallRule, NetworkAction, PortRange, PowerAction, Protocol, ResourceAction, SnapshotAction,
    StorageAction,
};
pub use agent_vm::{AgentVM, AgentVMBuilder};
pub use capabilities::{Capability, CapabilitySet};
pub use communication::{
    AgentInfo, Channel, CommError, CommResult, Message, MessagePayload, MessagePriority,
    MessageQueue, MessageRouter, MessageType, SharedRouter,
};
pub use events::{
    EventAggregator, EventBus, EventCategory, EventError, EventFilter, EventProcessor,
    EventReceiver, EventResult, EventSeverity, VmEvent,
};
pub use gpu_host::{GpuDescriptor, GpuHost, InMemoryGpuHost};
pub use image_host::{AdmissionVerdict, ImageDescriptor, ImageHost, RegistryImageHost};
pub use learning::{
    ActionValue, Experience, ExperienceBuffer, LearningConfig, LearningError, LearningRateSchedule,
    LearningResult, LearningState, LearningStats, LearningSystem, Reward, SharedLearning, Skill,
    SkillLevel,
};
pub use limits::{
    ConcurrencyGuard, ConcurrencyLimiter, LimitError, LimitResult, RateLimiter, ResourceEnforcer,
    ResourceLimits, ResourceSummary, ResourceUsage, TokenBucket,
};
pub use memory::{
    Episode, EpisodicMemory, Importance, MemoryError, MemoryId, MemoryResult, MemoryStats,
    MemorySystem, MemoryType, RetrievalConfig, RetrievalResult, SemanticFact, SemanticMemory,
    SharedMemory, WorkingItem, WorkingMemory,
};
pub use perception::{
    Observable, Observation, ObservationQuality, ObservationValue, PerceptionError,
    PerceptionFilter, PerceptionResult, PerceptionSystem, Sensor, SensorConfig, SensorDefinition,
    SensorType, SharedPerception, WorldModel,
};
pub use planning::{
    Condition, ConditionOperator, Effect, EffectOperation, Goal as PlanGoal, GoalManager,
    GoalStatus as PlanGoalStatus, Plan, PlanAction, PlanStatus, PlanStep, Planner as ActionPlanner,
    PlanningAlgorithm, PlanningConfig, PlanningError, PlanningResult, PlanningSystem, Priority,
    SharedPlanning, StepStatus, WorldState,
};
pub use policies::{
    AgentPolicy, PolicyAction, PolicyCondition, PolicyContext, PolicyEffect, PolicyEngine,
    PolicyError, PolicyResult, PolicyRule, PolicySet, QuotaSpec, RateLimitSpec, ResourceId,
    TimeWindow,
};
pub use reasoning::{
    Action, ActionEffect, BdiAgent, Belief, DecisionNode, DecisionNodeType, DecisionTree, Desire,
    Fact, FactPattern, FactSource, Goal, GoalStatus, InferenceEngine, Intention, IntentionStatus,
    KnowledgeBase, PatternPosition, Planner, ReasoningError, ReasoningResult, Rule, SharedReasoner,
    TruthValue,
};
pub use runtime::{AgentHandle, AgentRuntime};
pub use sandbox::{Sandbox, SandboxConfig};
pub use script::{ScriptEngine, ScriptResult};
pub use state::{
    CheckpointManager, ConflictStrategy, SharedStateStore, StateCheckpoint, StateError,
    StateResult, StateStore, StateSynchronizer, StateValue, SyncDirection, SyncResult,
};
pub use tasks::{
    ExecutionStats, RetryPolicy, Task, TaskError, TaskOutput, TaskPriority, TaskQueue, TaskResult,
    TaskScheduler, TaskStatus, Workflow, WorkflowExecutor, WorkflowStatus,
};
pub use telemetry::{
    AgentEvent, Counter, EventLevel, Gauge, Histogram, MetricInfo, MetricPoint, MetricType,
    MetricUnit, Span, SpanStatus, TelemetryCollector, TelemetryConfig, TelemetryError,
};
pub use tools::{
    ArgSource, ParameterType, RegisteredTool, SharedToolRegistry, ToolCall, ToolCallResult,
    ToolCategory, ToolChain, ToolChainStep, ToolDefinition, ToolError, ToolHandler, ToolParameter,
    ToolRegistry, ToolResult,
};
pub use vm_host::{LocalVmHost, VmConsole, VmDescriptor, VmHost, VmMetrics, VmSpec};

// MCP interface for AI agent tool-use
pub use mcp::{
    AgentCapabilities, AgentCapability, AgentSession, AuditEntry, McpConfig, McpServer, McpTool,
    ToolCallRequest, ToolCallResponse, ToolCategory as McpToolCategory,
};

// Multi-agent orchestration
pub use orchestration::{
    AgentInfo as OrchAgentInfo, AgentMessage, AgentOrchestrator, AgentRole, AgentState,
    Channel as OrchChannel, Conflict, ConflictResolution, ConflictType, EventType,
    MessagePriority as OrchMessagePriority, MessageType as OrchMessageType, OrchestrationError,
    OrchestratorConfig, TaskState, VmClaim, Workflow as OrchWorkflow, WorkflowState, WorkflowTask,
};

// Distributed graph-based hierarchical permissions
pub use permissions::{
    AuditLog, Delegation, DelegationChain, DelegationConstraint, EffectivePermissions, GrantEdge,
    GrantId, Permission, PermissionAuditEntry, PermissionChange, PermissionError, PermissionGraph,
    PermissionSet, Principal, PrincipalId, PrincipalKind, ResolutionEngine, ResourceScope,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Script error: {0}")]
    Script(String),

    #[error("Security violation: {0}")]
    Security(String),

    #[error("Resource limit exceeded: {0}")]
    ResourceLimit(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("VM error: {0}")]
    VM(#[from] hv2_core::Error),

    #[error("Timeout: {0}")]
    Timeout(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;
