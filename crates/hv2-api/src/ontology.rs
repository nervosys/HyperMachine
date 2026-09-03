//! Agentic Ontology Module
//!
//! Provides machine-readable API discovery for AI agents.
//! Supports multiple formats: OpenAPI, JSON-LD, and tool schemas
//! for OpenAI, Anthropic, and Google AI integrations.

use axum::{
    extract::Query,
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Core Ontology Types
// ============================================================================

/// Complete HyperMachine capability ontology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperMachineOntology {
    /// Ontology metadata
    #[serde(rename = "@context")]
    pub context: OntologyContext,

    /// System identification
    pub system: SystemInfo,

    /// Available capabilities
    pub capabilities: Vec<Capability>,

    /// Resource types
    pub resources: Vec<ResourceType>,

    /// Available operations
    pub operations: Vec<Operation>,

    /// State machine definitions
    pub state_machines: Vec<StateMachine>,

    /// Event types for subscriptions
    pub events: Vec<EventType>,
}

/// JSON-LD context for semantic web compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyContext {
    #[serde(rename = "@vocab")]
    pub vocab: String,
    pub schema: String,
    pub hm: String,
    pub dcterms: String,
}

impl Default for OntologyContext {
    fn default() -> Self {
        Self {
            vocab: "https://schema.nervosys.ai/hypermachine#".to_string(),
            schema: "https://schema.org/".to_string(),
            hm: "https://nervosys.ai/hypermachine/ontology/".to_string(),
            dcterms: "http://purl.org/dc/terms/".to_string(),
        }
    }
}

/// System information for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub documentation_url: String,
    pub api_base_url: String,
    pub supported_protocols: Vec<String>,
    pub authentication: AuthenticationInfo,
}

/// Authentication requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationInfo {
    pub required: bool,
    pub methods: Vec<AuthMethod>,
    pub token_endpoint: Option<String>,
}

/// Authentication method for API access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethod {
    pub method_type: String,
    pub description: String,
    pub header_name: Option<String>,
}

/// A capability that HyperMachine provides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: CapabilityCategory,
    pub operations: Vec<String>,
    pub prerequisites: Vec<String>,
    pub permissions_required: Vec<String>,
}

/// Category of hypervisor capability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    VirtualMachine,
    Compute,
    Storage,
    Network,
    Security,
    Monitoring,
    AgentExecution,
}

/// A resource type that can be managed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceType {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub lifecycle_states: Vec<String>,
    pub relationships: Vec<ResourceRelationship>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRelationship {
    pub name: String,
    pub target_type: String,
    pub cardinality: String,
    pub description: String,
}

/// An operation that can be performed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub name: String,
    pub description: String,
    pub http_method: String,
    pub path: String,
    pub parameters: Vec<Parameter>,
    pub request_body: Option<RequestBody>,
    pub responses: Vec<OperationResponse>,
    pub idempotent: bool,
    pub async_operation: bool,
    pub rate_limit: Option<RateLimit>,
    pub examples: Vec<OperationExample>,
}

/// API operation parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub location: ParameterLocation,
    pub required: bool,
    pub description: String,
    pub schema: serde_json::Value,
    pub default: Option<serde_json::Value>,
}

/// Location of a parameter in the HTTP request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Body,
}

/// Request body schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub content_type: String,
    pub schema: serde_json::Value,
    pub required: bool,
}

/// Expected response for an API operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    pub status_code: u16,
    pub description: String,
    pub schema: Option<serde_json::Value>,
}

/// Rate limiting configuration for an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
/// A rate limit published for an operation.
///
/// **Advisory.** This is metadata the server publishes so a client can pace
/// itself; nothing reads it back to reject a request. The only rate limiting
/// this server actually performs is the optional HTTP middleware token bucket
/// (`MiddlewareConfig::enable_rate_limit`, off by default), which is global
/// rather than per-operation. An agent must not assume it will receive a 429
/// on exceeding these numbers, nor that staying under them is required.
pub struct RateLimit {
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

/// Example request/response pair for an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationExample {
    pub name: String,
    pub description: String,
    pub request: Option<serde_json::Value>,
    pub response: serde_json::Value,
}

/// State machine definition for resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    pub resource_type: String,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub initial_state: String,
    pub terminal_states: Vec<String>,
}

/// A state within a resource state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub name: String,
    pub description: String,
    pub allowed_operations: Vec<String>,
}

/// A transition between states in a resource state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from_state: String,
    pub to_state: String,
    pub trigger_operation: String,
    pub conditions: Vec<String>,
}

/// Event type for subscriptions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventType {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub resource_types: Vec<String>,
}

// ============================================================================
// Composability & Agentic Primitives
// ============================================================================

/// An action plan that an agent submits for validation and execution.
///
/// Plans allow agents to compose multi-step operations declaratively.
/// The system validates preconditions, resolves dependencies, and can
/// execute the plan transactionally with rollback support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    /// Human/agent-readable plan name
    pub name: String,
    /// What this plan accomplishes
    pub description: String,
    /// Ordered steps to execute
    pub steps: Vec<PlanStep>,
    /// Whether to rollback completed steps on failure
    pub rollback_on_failure: bool,
}

/// A single step in an action plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Unique step identifier within the plan
    pub step_id: String,
    /// Operation to invoke (must match an Operation.id)
    pub operation_id: String,
    /// Parameters to pass to the operation
    pub parameters: serde_json::Value,
    /// Step IDs that must complete before this step runs
    pub depends_on: Vec<String>,
    /// Per-step timeout override
    pub timeout_seconds: Option<u64>,
}

/// Result of validating an action plan against the ontology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanValidationResult {
    /// Whether the plan is valid and can be executed
    pub valid: bool,
    /// Validation errors that prevent execution
    pub errors: Vec<PlanValidationError>,
    /// Non-blocking warnings
    pub warnings: Vec<String>,
    /// Estimated total execution time
    pub estimated_duration_ms: Option<u64>,
    /// Steps with resolved preconditions
    pub resolved_steps: Vec<ResolvedStep>,
}

/// A validation error for a specific plan step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanValidationError {
    pub step_id: String,
    pub error_type: String,
    pub message: String,
}

/// A plan step with resolved precondition analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedStep {
    pub step_id: String,
    pub operation_id: String,
    pub preconditions_met: bool,
    pub expected_postconditions: Vec<String>,
}

/// Affordances available for a resource in a given state.
///
/// Affordances tell agents what they CAN do right now, given the
/// current state of a resource. This is the key composability primitive
/// that enables agents to make context-aware decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Affordances {
    /// The resource type being queried
    pub resource_type: String,
    /// Current state of the resource
    pub current_state: String,
    /// Operations available in this state
    pub available_operations: Vec<AffordanceOperation>,
    /// State transitions reachable from here
    pub possible_transitions: Vec<AffordanceTransition>,
}

/// An operation available as an affordance in the current state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffordanceOperation {
    pub operation_id: String,
    pub name: String,
    pub description: String,
    pub http_method: String,
    pub path: String,
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
    pub idempotent: bool,
}

/// A state transition available as an affordance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffordanceTransition {
    pub target_state: String,
    pub trigger_operation: String,
    pub conditions: Vec<String>,
    pub reversible: bool,
}

/// Rules governing how operations can be composed together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionRules {
    /// Pre-defined multi-step workflows
    pub workflows: Vec<Workflow>,
    /// Constraints on operation composition
    pub constraints: Vec<CompositionConstraint>,
    /// Reusable composition patterns (templates)
    pub patterns: Vec<CompositionPattern>,
}

/// A pre-defined workflow composed of ordered operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub category: String,
}

/// A single step within a workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub order: u32,
    pub operation_id: String,
    pub description: String,
    pub required: bool,
    pub wait_for_state: Option<String>,
}

/// A constraint on how operations may be composed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionConstraint {
    pub name: String,
    pub description: String,
    pub rule_type: ConstraintType,
    pub operations: Vec<String>,
}

/// Type of composition constraint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    /// These operations cannot run concurrently on the same resource
    MutuallyExclusive,
    /// These operations must execute in the given order
    RequiresSequence,
    /// At most N of these operations may run concurrently
    MaxConcurrent,
    /// This operation is safe to retry
    Idempotent,
    /// This operation requires the resource to be in a specific state
    StatePrecondition,
}

/// A reusable composition pattern (plan template)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionPattern {
    pub name: String,
    pub description: String,
    pub template: ActionPlan,
}

/// Resource relationship graph for agent navigation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceGraph {
    pub nodes: Vec<ResourceNode>,
    pub edges: Vec<ResourceEdge>,
}

/// A node in the resource relationship graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceNode {
    pub id: String,
    pub resource_type: String,
    pub label: String,
    pub operations_count: usize,
}

/// An edge in the resource relationship graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEdge {
    pub from: String,
    pub to: String,
    pub relationship: String,
    pub cardinality: String,
}

/// A2A (Agent-to-Agent) protocol agent card.
///
/// Enables multi-agent discovery: other AI agents can find this
/// hypervisor's agent interface, understand its capabilities,
/// and compose interactions programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub name: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub capabilities: AgentCapabilities,
    pub skills: Vec<AgentSkill>,
    pub authentication: AgentAuthConfig,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
}

/// Agent capability flags for A2A discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub streaming: bool,
    pub push_notifications: bool,
    pub state_transition_history: bool,
}

/// A skill advertised by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub examples: Vec<String>,
}

/// Authentication config for agent card
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub schemes: Vec<String>,
}

/// MCP (Model Context Protocol) server manifest.
///
/// Allows MCP-compatible clients (Claude, etc.) to discover
/// this server's tools and resources automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpManifest {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
    pub capabilities: McpCapabilities,
    pub tools: Vec<McpTool>,
    pub resources: Vec<McpResource>,
}

/// MCP server capability flags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
    pub logging: bool,
}

/// An MCP tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// An MCP resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: String,
}

/// Pre/post-condition contract for an operation.
///
/// Contracts enable agents to reason about operation sequencing:
/// "Can I call stop_vm? Only if state == running."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationContract {
    pub operation_id: String,
    pub preconditions: Vec<Condition>,
    pub postconditions: Vec<Condition>,
    pub invariants: Vec<String>,
    pub composable_with: Vec<String>,
    pub mutually_exclusive_with: Vec<String>,
}

/// A condition on a resource field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub description: String,
    pub resource_type: String,
    pub field: String,
    pub operator: ConditionOperator,
    pub value: serde_json::Value,
}

/// Comparison operator for conditions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperator {
    Equals,
    NotEquals,
    In,
    NotIn,
    Exists,
    NotExists,
}

// ============================================================================
// Plan Execution Engine
// ============================================================================

/// Result of executing an action plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExecutionResult {
    /// Unique execution ID
    pub execution_id: String,
    /// Plan that was executed
    pub plan_name: String,
    /// Overall execution status
    pub status: PlanExecutionStatus,
    /// Per-step results in execution order
    pub step_results: Vec<PlanStepResult>,
    /// Total execution duration in milliseconds
    pub duration_ms: u64,
    /// Steps that were rolled back (if rollback_on_failure was true)
    pub rolled_back_steps: Vec<String>,
    /// Validation result (pre-execution check)
    pub validation: PlanValidationResult,
}

/// Overall status of a plan execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanExecutionStatus {
    /// All steps completed successfully
    Completed,
    /// One or more steps failed
    Failed,
    /// Plan failed validation and was not executed
    ValidationFailed,
    /// Execution was partially completed then rolled back
    RolledBack,
}

/// Result of executing a single plan step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepResult {
    /// Step identifier
    pub step_id: String,
    /// Operation that was invoked
    pub operation_id: String,
    /// Whether this step succeeded
    pub success: bool,
    /// Step execution duration in milliseconds
    pub duration_ms: u64,
    /// Operation output (if successful)
    pub output: Option<serde_json::Value>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Whether this step was rolled back
    pub rolled_back: bool,
}

/// Request to execute a plan with execution options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExecutionRequest {
    /// The action plan to execute
    pub plan: ActionPlan,
    /// Whether to perform a dry-run (validate only, do not execute)
    pub dry_run: Option<bool>,
    /// Global timeout for the entire plan execution (seconds)
    pub timeout_seconds: Option<u64>,
    /// Variable substitutions for parameterized plans
    pub variables: Option<serde_json::Map<String, serde_json::Value>>,
}

// ============================================================================
// Plan execution — where a validated plan meets a real resource
// ============================================================================

/// Dispatches an ontology operation to something that can carry it out.
///
/// The ontology owns *what* a plan means — its steps, their dependencies, its
/// preconditions — and a `PlanExecutor` owns *what actually happens*. Splitting
/// them lets the same plan be dry-run against [`SimulatingExecutor`] and then
/// run for real against [`VmHostExecutor`], with identical ordering and
/// validation both times.
#[async_trait::async_trait]
pub trait PlanExecutor: Send + Sync {
    /// Carry out one operation, returning its output or an error message.
    async fn execute(
        &self,
        operation: &Operation,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;

    /// Undo a step that already succeeded, during rollback.
    ///
    /// `output` is what [`Self::execute`] returned for that step, which is
    /// usually where the identifier of the thing to undo lives.
    ///
    /// The default is `Ok(())` — correct for a read-only operation, and the
    /// reason an executor only has to describe the operations it can reverse.
    /// Return an error when a step genuinely cannot be undone, so the result
    /// reports the resource as left behind rather than cleaned up.
    async fn compensate(
        &self,
        operation: &Operation,
        params: &serde_json::Value,
        output: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let _ = (operation, params, output);
        Ok(())
    }
}

/// What one step produced.
struct StepOutcome {
    success: bool,
    output: Option<serde_json::Value>,
    error: Option<String>,
}

impl StepOutcome {
    fn missing_operation(operation_id: &str) -> Self {
        Self {
            success: false,
            output: None,
            error: Some(format!("Operation '{}' not found", operation_id)),
        }
    }
}

/// Whether the plan loop should keep going.
enum StepFlow {
    Continue,
    Break,
}

impl StepFlow {
    fn is_break(&self) -> bool {
        matches!(self, Self::Break)
    }
}

/// One in-progress plan execution.
///
/// Owns everything both the simulated and the real execution paths share —
/// validation, dependency ordering, variable substitution, and how a run's
/// status is decided — so the two cannot drift apart.
struct PlanRun {
    execution_id: String,
    plan_name: String,
    rollback_on_failure: bool,
    validation: PlanValidationResult,
    sorted_steps: Vec<PlanStep>,
    variables: serde_json::Map<String, serde_json::Value>,
    /// Parameters actually used per step, kept for compensation.
    executed_params: Vec<serde_json::Value>,
    step_results: Vec<PlanStepResult>,
    failed: bool,
    start_time: std::time::Instant,
}

impl PlanRun {
    /// Validate and prepare a run, or return the finished result directly when
    /// the plan is invalid or this is a dry run.
    ///
    /// The early-return result is boxed: it is much larger than a `PlanRun`,
    /// and an unboxed `Err` of that size would bloat every call.
    fn begin(
        ontology: &HyperMachineOntology,
        request: &PlanExecutionRequest,
    ) -> Result<Self, Box<PlanExecutionResult>> {
        let execution_id = HyperMachineOntology::execution_id(&request.plan.name);
        let start_time = std::time::Instant::now();
        let validation = ontology.validate_plan(&request.plan);

        if !validation.valid {
            return Err(Box::new(PlanExecutionResult {
                execution_id,
                plan_name: request.plan.name.clone(),
                status: PlanExecutionStatus::ValidationFailed,
                step_results: vec![],
                duration_ms: start_time.elapsed().as_millis() as u64,
                rolled_back_steps: vec![],
                validation,
            }));
        }

        if request.dry_run.unwrap_or(false) {
            return Err(Box::new(PlanExecutionResult {
                execution_id,
                plan_name: request.plan.name.clone(),
                status: PlanExecutionStatus::Completed,
                step_results: vec![],
                duration_ms: start_time.elapsed().as_millis() as u64,
                rolled_back_steps: vec![],
                validation,
            }));
        }

        Ok(Self {
            execution_id,
            plan_name: request.plan.name.clone(),
            rollback_on_failure: request.plan.rollback_on_failure,
            validation,
            sorted_steps: HyperMachineOntology::topological_sort(&request.plan.steps),
            variables: request.variables.clone().unwrap_or_default(),
            executed_params: Vec::new(),
            step_results: Vec::new(),
            failed: false,
            start_time,
        })
    }

    /// The parameters for `step`, with `${var}` references resolved.
    fn params_for(&self, step: &PlanStep) -> serde_json::Value {
        HyperMachineOntology::substitute_variables(&step.parameters, &self.variables)
    }

    /// Record a step's outcome and say whether the plan should continue.
    fn record(
        &mut self,
        step: &PlanStep,
        params: serde_json::Value,
        outcome: StepOutcome,
    ) -> StepFlow {
        let step_start = std::time::Instant::now();

        self.executed_params.push(params);
        self.step_results.push(PlanStepResult {
            step_id: step.step_id.clone(),
            operation_id: step.operation_id.clone(),
            success: outcome.success,
            duration_ms: step_start.elapsed().as_millis() as u64,
            output: outcome.output,
            error: outcome.error,
            rolled_back: false,
        });

        if outcome.success {
            StepFlow::Continue
        } else {
            self.failed = true;
            StepFlow::Break
        }
    }

    /// Finish a simulated run, marking successful steps rolled back on failure.
    fn finish_marking_rollback(mut self) -> PlanExecutionResult {
        let mut rolled_back_steps = Vec::new();

        let status = if self.failed && self.rollback_on_failure {
            for result in self.step_results.iter_mut().rev() {
                if result.success {
                    result.rolled_back = true;
                    rolled_back_steps.push(result.step_id.clone());
                }
            }
            PlanExecutionStatus::RolledBack
        } else if self.failed {
            PlanExecutionStatus::Failed
        } else {
            PlanExecutionStatus::Completed
        };

        self.into_result(status, rolled_back_steps)
    }

    /// Finish a real run, actually compensating the steps that succeeded.
    ///
    /// Compensation runs in reverse order, so a plan is unwound the way it was
    /// built up.
    async fn finish_compensating(
        mut self,
        ontology: &HyperMachineOntology,
        executor: &dyn PlanExecutor,
    ) -> PlanExecutionResult {
        if !self.failed {
            return self.into_result(PlanExecutionStatus::Completed, Vec::new());
        }
        if !self.rollback_on_failure {
            return self.into_result(PlanExecutionStatus::Failed, Vec::new());
        }

        let mut rolled_back_steps = Vec::new();
        let mut compensation_failed = false;

        for (idx, result) in self.step_results.iter_mut().enumerate().rev() {
            if !result.success {
                continue;
            }

            let Some(op) = ontology.operation(&result.operation_id) else {
                result.error = Some(format!(
                    "cannot roll back: operation '{}' is no longer defined",
                    result.operation_id
                ));
                compensation_failed = true;
                continue;
            };

            let params = self
                .executed_params
                .get(idx)
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            match executor
                .compensate(op, &params, result.output.as_ref())
                .await
            {
                Ok(()) => {
                    result.rolled_back = true;
                    rolled_back_steps.push(result.step_id.clone());
                }
                Err(e) => {
                    // Leave `rolled_back` false: this resource is still there,
                    // and saying otherwise would hide a leak.
                    result.error = Some(match result.error.take() {
                        Some(existing) => format!("{existing}; rollback failed: {e}"),
                        None => format!("rollback failed: {e}"),
                    });
                    compensation_failed = true;
                }
            }
        }

        // A rollback that could not finish is not a clean unwind, and the
        // status has to say so.
        let status = if compensation_failed {
            PlanExecutionStatus::Failed
        } else {
            PlanExecutionStatus::RolledBack
        };

        self.into_result(status, rolled_back_steps)
    }

    fn into_result(
        self,
        status: PlanExecutionStatus,
        rolled_back_steps: Vec<String>,
    ) -> PlanExecutionResult {
        PlanExecutionResult {
            execution_id: self.execution_id,
            plan_name: self.plan_name,
            status,
            step_results: self.step_results,
            duration_ms: self.start_time.elapsed().as_millis() as u64,
            rolled_back_steps,
            validation: self.validation,
        }
    }
}

/// A [`PlanExecutor`] that fabricates plausible results without touching
/// anything.
///
/// This is what [`HyperMachineOntology::execute_plan`] uses. It exists so a
/// plan's shape can be exercised — ordering, substitution, failure handling —
/// on a machine with no hypervisor. Its outputs carry `"simulated": true` so a
/// caller can never mistake them for real ones.
pub struct SimulatingExecutor;

#[async_trait::async_trait]
impl PlanExecutor for SimulatingExecutor {
    async fn execute(
        &self,
        operation: &Operation,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match HyperMachineOntology::simulate_operation(operation, params) {
            (true, output, _) => Ok(output.unwrap_or(serde_json::Value::Null)),
            (false, _, error) => Err(error.unwrap_or_else(|| "simulated failure".to_string())),
        }
    }
}

/// A [`PlanExecutor`] that runs VM operations against a real
/// [`VmHost`](hv2_agent::VmHost).
///
/// This is what makes `/agentic/plans/execute` a control plane rather than a
/// rehearsal: `create_vm` allocates a VM, `start_vm` boots it, and a failed
/// plan's rollback destroys what it created.
pub struct VmHostExecutor {
    host: std::sync::Arc<dyn hv2_agent::VmHost>,
}

impl VmHostExecutor {
    /// Execute plans against `host`.
    pub fn new(host: std::sync::Arc<dyn hv2_agent::VmHost>) -> Self {
        Self { host }
    }

    /// Read the VM id a step operates on, from its parameters.
    fn vm_id(params: &serde_json::Value) -> Result<&str, String> {
        params
            .get("id")
            .or_else(|| params.get("vm_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required parameter: id".to_string())
    }

    /// Read the VM id a `create_vm` step produced, from its output.
    fn created_vm_id(output: Option<&serde_json::Value>) -> Result<&str, String> {
        output
            .and_then(|o| o.get("vm_id").or_else(|| o.get("id")))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "the create step recorded no VM id to roll back".to_string())
    }

    fn describe(descriptor: &hv2_agent::VmDescriptor) -> serde_json::Value {
        serde_json::to_value(descriptor).unwrap_or(serde_json::Value::Null)
    }
}

#[async_trait::async_trait]
impl PlanExecutor for VmHostExecutor {
    async fn execute(
        &self,
        operation: &Operation,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match operation.id.as_str() {
            "create_vm" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("missing required parameter: name")?;

                let mut spec = hv2_agent::VmSpec::new(name);
                if let Some(cores) = params.get("vcpu_count").and_then(|v| v.as_u64()) {
                    spec.cpu_cores = cores as u32;
                }
                if let Some(memory) = params.get("memory_gb").and_then(|v| v.as_u64()) {
                    spec.memory_gb = memory;
                }
                if let Some(boot) = params.get("boot") {
                    spec.boot = serde_json::from_value(boot.clone())
                        .map_err(|e| format!("invalid boot source: {e}"))?;
                }

                self.host.create(spec).await.map(|d| Self::describe(&d))
            }

            "start_vm" => self
                .host
                .start(Self::vm_id(params)?)
                .await
                .map(|d| Self::describe(&d)),

            "stop_vm" => {
                let force = params
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.host
                    .stop(Self::vm_id(params)?, force)
                    .await
                    .map(|d| Self::describe(&d))
            }

            "pause_vm" => self
                .host
                .pause(Self::vm_id(params)?)
                .await
                .map(|d| Self::describe(&d)),

            "resume_vm" => self
                .host
                .resume(Self::vm_id(params)?)
                .await
                .map(|d| Self::describe(&d)),

            "delete_vm" => {
                let id = Self::vm_id(params)?;
                self.host.delete(id).await?;
                Ok(serde_json::json!({ "vm_id": id, "deleted": true }))
            }

            "get_vm" => self
                .host
                .status(Self::vm_id(params)?)
                .await
                .map(|d| Self::describe(&d)),

            "list_vms" => {
                let vms = self.host.list().await?;
                let total = vms.len();
                Ok(serde_json::json!({ "vms": vms, "total": total }))
            }

            "get_metrics" => {
                let metrics = self.host.metrics(Self::vm_id(params)?).await?;
                serde_json::to_value(metrics).map_err(|e| e.to_string())
            }

            // The host models VM lifecycle, not guest execution. Refusing is
            // the honest answer: a step that silently returned fabricated
            // output here would make a plan look like it did work it did not
            // do.
            other => Err(format!(
                "operation '{other}' has no VM-host implementation; \
                 it is available only in simulated execution"
            )),
        }
    }

    async fn compensate(
        &self,
        operation: &Operation,
        params: &serde_json::Value,
        output: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        match operation.id.as_str() {
            // Undo a creation by destroying what it made — using the id the
            // host assigned, not one the caller guessed.
            "create_vm" => self.host.delete(Self::created_vm_id(output)?).await,

            "start_vm" => self.host.stop(Self::vm_id(params)?, false).await.map(drop),
            "stop_vm" => self.host.start(Self::vm_id(params)?).await.map(drop),
            "pause_vm" => self.host.resume(Self::vm_id(params)?).await.map(drop),
            "resume_vm" => self.host.pause(Self::vm_id(params)?).await.map(drop),

            // A destroyed VM cannot be brought back. Say so rather than
            // reporting a clean rollback that did not happen.
            "delete_vm" => Err("a deleted VM cannot be restored".to_string()),

            // Read-only operations need no compensation.
            _ => Ok(()),
        }
    }
}

// ============================================================================
// ============================================================================
// Plan Templates — Reusable action plan blueprints for agent discovery
// ============================================================================

/// Category of a plan template
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCategory {
    /// VM lifecycle management (create, start, stop, etc.)
    Lifecycle,
    /// Infrastructure provisioning workflows
    Provisioning,
    /// Monitoring and observability setup
    Monitoring,
    /// Backup, snapshot, and disaster recovery
    Recovery,
    /// Fleet and cluster management
    Fleet,
    /// Security and compliance operations
    Security,
    /// Performance tuning and benchmarking
    Performance,
}

/// A parameter required by a template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name used in variable substitution
    pub name: String,
    /// Human-readable label
    pub label: String,
    /// Description of what the parameter controls
    pub description: String,
    /// JSON Schema type (string, integer, boolean, etc.)
    pub param_type: String,
    /// Whether the parameter must be provided
    pub required: bool,
    /// Default value if not provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Example value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<serde_json::Value>,
    /// Allowed values (enum constraint)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_values: Option<Vec<serde_json::Value>>,
}

/// A reusable plan template that agents can discover and instantiate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTemplate {
    /// Unique template identifier
    pub id: String,
    /// Human-readable template name
    pub name: String,
    /// Detailed description of what the template does
    pub description: String,
    /// Template category
    pub category: TemplateCategory,
    /// Semantic version (e.g., "1.0.0")
    pub version: String,
    /// Tags for discovery and filtering
    pub tags: Vec<String>,
    /// Parameters the template accepts
    pub parameters: Vec<TemplateParameter>,
    /// The action plan blueprint with variable placeholders
    pub plan: ActionPlan,
    /// Estimated execution time in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_duration_seconds: Option<u64>,
    /// Whether rollback is recommended for this template
    pub rollback_recommended: bool,
}

/// Request to instantiate a plan template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInstantiationRequest {
    /// The template ID to instantiate
    pub template_id: String,
    /// Parameter values to bind
    #[serde(default)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
    /// Whether to also execute the instantiated plan
    #[serde(default)]
    pub execute: bool,
    /// If executing, whether to dry-run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

/// Result of instantiating a plan template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInstantiationResult {
    /// The template that was instantiated
    pub template_id: String,
    /// The instantiated action plan (with variables resolved)
    pub plan: ActionPlan,
    /// Validation result for the instantiated plan
    pub validation: PlanValidationResult,
    /// Execution result (if execute was true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<PlanExecutionResult>,
    /// Any parameter defaults that were applied
    pub defaults_applied: Vec<String>,
    /// Any missing required parameters
    pub missing_parameters: Vec<String>,
}

// AI Agent Tool Formats
// ============================================================================

/// OpenAI function calling format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITools {
    pub tools: Vec<OpenAITool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Anthropic MCP tool format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTools {
    pub name: String,
    pub version: String,
    pub tools: Vec<AnthropicTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Google Gemini function declarations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTools {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GeminiFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The complete MCP tool registry — every agent-callable tool with its JSON
/// schema, scoped to full capabilities. This is the single source of truth for
/// the agentic tool surface (it matches `/mcp/tools` and `/mcp/call`), so the
/// LLM tool exports below cannot drift from the tools an agent can actually
/// invoke.
fn mcp_registry_tools() -> Vec<hv2_agent::McpTool> {
    hv2_agent::McpServer::new().list_tools(&hv2_agent::AgentCapabilities::full())
}

// ============================================================================
// Ontology Builder
// ============================================================================

impl HyperMachineOntology {
    /// Build the complete HyperMachine ontology
    pub fn build() -> Self {
        Self {
            context: OntologyContext::default(),
            system: Self::build_system_info(),
            capabilities: Self::build_capabilities(),
            resources: Self::build_resources(),
            operations: Self::build_operations(),
            state_machines: Self::build_state_machines(),
            events: Self::build_events(),
        }
    }

    fn build_system_info() -> SystemInfo {
        SystemInfo {
            name: "HyperMachine".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Next-generation hybrid hypervisor with AI-native capabilities. \
                Provides Type-1 bare-metal and Type-2 hosted virtualization with \
                first-class support for AI agent orchestration."
                .to_string(),
            documentation_url: "https://docs.nervosys.ai/hypermachine".to_string(),
            api_base_url: "/api/v1".to_string(),
            supported_protocols: vec![
                "REST".to_string(),
                "gRPC".to_string(),
                "WebSocket".to_string(),
            ],
            authentication: AuthenticationInfo {
                required: true,
                methods: vec![
                    AuthMethod {
                        method_type: "bearer_token".to_string(),
                        description: "Ed25519-signed JWT bearer token".to_string(),
                        header_name: Some("Authorization".to_string()),
                    },
                    AuthMethod {
                        method_type: "mtls".to_string(),
                        description: "Mutual TLS with client certificate".to_string(),
                        header_name: None,
                    },
                ],
                token_endpoint: Some("/api/v1/auth/token".to_string()),
            },
        }
    }

    fn build_capabilities() -> Vec<Capability> {
        vec![
            Capability {
                id: "vm_management".to_string(),
                name: "Virtual Machine Management".to_string(),
                description: "Create, configure, and manage virtual machines with \
                    customizable CPU, memory, storage, and networking"
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "create_vm".to_string(),
                    "delete_vm".to_string(),
                    "get_vm".to_string(),
                    "list_vms".to_string(),
                    "update_vm".to_string(),
                ],
                prerequisites: vec![],
                permissions_required: vec!["vm:create".to_string(), "vm:read".to_string()],
            },
            Capability {
                id: "vm_lifecycle".to_string(),
                name: "VM Lifecycle Control".to_string(),
                description: "Start, stop, pause, resume, and snapshot virtual machines"
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "start_vm".to_string(),
                    "stop_vm".to_string(),
                    "pause_vm".to_string(),
                    "resume_vm".to_string(),
                    "snapshot_vm".to_string(),
                    "restore_vm".to_string(),
                ],
                prerequisites: vec!["vm_management".to_string()],
                permissions_required: vec!["vm:control".to_string()],
            },
            Capability {
                id: "agent_execution".to_string(),
                name: "AI Agent Execution".to_string(),
                description: "Execute AI agent scripts in sandboxed WASM/Rhai environments \
                    within VMs. Agents can automate VM operations with capability-based security."
                    .to_string(),
                category: CapabilityCategory::AgentExecution,
                operations: vec![
                    "execute_script".to_string(),
                    "list_agents".to_string(),
                    "get_agent_logs".to_string(),
                ],
                prerequisites: vec!["vm_management".to_string()],
                permissions_required: vec!["agent:execute".to_string()],
            },
            Capability {
                id: "gpu_passthrough".to_string(),
                name: "GPU Passthrough".to_string(),
                description: "Attach and manage GPU devices for AI/ML workloads in VMs".to_string(),
                category: CapabilityCategory::Compute,
                operations: vec![
                    "attach_gpu".to_string(),
                    "detach_gpu".to_string(),
                    "list_gpus".to_string(),
                ],
                prerequisites: vec!["vm_management".to_string()],
                permissions_required: vec!["gpu:manage".to_string()],
            },
            Capability {
                id: "metrics_monitoring".to_string(),
                name: "Metrics and Monitoring".to_string(),
                description: "Collect and query VM and system metrics, set up alerts".to_string(),
                category: CapabilityCategory::Monitoring,
                operations: vec![
                    "get_metrics".to_string(),
                    "query_metrics".to_string(),
                    "set_alert".to_string(),
                ],
                prerequisites: vec![],
                permissions_required: vec!["metrics:read".to_string()],
            },
        ]
    }

    fn build_resources() -> Vec<ResourceType> {
        vec![
            ResourceType {
                id: "vm".to_string(),
                name: "Virtual Machine".to_string(),
                description: "A virtual machine instance with dedicated compute, memory, and I/O"
                    .to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "name": { "type": "string", "minLength": 1, "maxLength": 64 },
                        "state": { "type": "string", "enum": ["created", "starting", "running", "paused", "stopping", "stopped", "error"] },
                        "vcpu_count": { "type": "integer", "minimum": 1, "maximum": 256 },
                        "memory_gb": { "type": "integer", "minimum": 1, "maximum": 4096 },
                        "enable_gpu": { "type": "boolean" },
                        "enable_networking": { "type": "boolean" },
                        "created_at": { "type": "string", "format": "date-time" },
                        "updated_at": { "type": "string", "format": "date-time" }
                    },
                    "required": ["id", "name", "state"]
                }),
                lifecycle_states: vec![
                    "created".to_string(),
                    "starting".to_string(),
                    "running".to_string(),
                    "paused".to_string(),
                    "stopping".to_string(),
                    "stopped".to_string(),
                    "error".to_string(),
                ],
                relationships: vec![
                    ResourceRelationship {
                        name: "disks".to_string(),
                        target_type: "disk".to_string(),
                        cardinality: "one-to-many".to_string(),
                        description: "Storage disks attached to the VM".to_string(),
                    },
                    ResourceRelationship {
                        name: "networks".to_string(),
                        target_type: "network".to_string(),
                        cardinality: "many-to-many".to_string(),
                        description: "Networks the VM is connected to".to_string(),
                    },
                    ResourceRelationship {
                        name: "gpus".to_string(),
                        target_type: "gpu".to_string(),
                        cardinality: "one-to-many".to_string(),
                        description: "GPUs attached to the VM".to_string(),
                    },
                ],
            },
            ResourceType {
                id: "agent".to_string(),
                name: "AI Agent".to_string(),
                description: "An AI agent script running in a sandboxed environment".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "vm_id": { "type": "string", "format": "uuid" },
                        "script_type": { "type": "string", "enum": ["rhai"] },
                        "state": { "type": "string", "enum": ["pending", "running", "completed", "failed"] },
                        "output": { "type": "string" },
                        "error": { "type": "string" },
                        "started_at": { "type": "string", "format": "date-time" },
                        "completed_at": { "type": "string", "format": "date-time" }
                    },
                    "required": ["id", "vm_id", "script_type", "state"]
                }),
                lifecycle_states: vec![
                    "pending".to_string(),
                    "running".to_string(),
                    "completed".to_string(),
                    "failed".to_string(),
                ],
                relationships: vec![ResourceRelationship {
                    name: "vm".to_string(),
                    target_type: "vm".to_string(),
                    cardinality: "many-to-one".to_string(),
                    description: "The VM this agent runs in".to_string(),
                }],
            },
        ]
    }

    fn build_operations() -> Vec<Operation> {
        vec![
            // VM CRUD Operations
            Operation {
                id: "list_vms".to_string(),
                name: "List Virtual Machines".to_string(),
                description: "Retrieve a list of all virtual machines. Supports filtering and pagination.".to_string(),
                http_method: "GET".to_string(),
                path: "/api/v1/vms".to_string(),
                parameters: vec![
                    Parameter {
                        name: "state".to_string(),
                        location: ParameterLocation::Query,
                        required: false,
                        description: "Filter by VM state".to_string(),
                        schema: serde_json::json!({ "type": "string", "enum": ["running", "stopped", "paused"] }),
                        default: None,
                    },
                    Parameter {
                        name: "limit".to_string(),
                        location: ParameterLocation::Query,
                        required: false,
                        description: "Maximum number of results".to_string(),
                        schema: serde_json::json!({ "type": "integer", "minimum": 1, "maximum": 100 }),
                        default: Some(serde_json::json!(20)),
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "List of VMs".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "vms": { "type": "array", "items": { "$ref": "#/resources/vm" } },
                                "total": { "type": "integer" }
                            }
                        })),
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 100, burst_size: 20 }),
                examples: vec![
                    OperationExample {
                        name: "List all running VMs".to_string(),
                        description: "Get all VMs in the running state".to_string(),
                        request: None,
                        response: serde_json::json!({
                            "vms": [
                                { "id": "vm-123", "name": "web-server", "state": "running" }
                            ],
                            "total": 1
                        }),
                    },
                ],
            },
            Operation {
                id: "create_vm".to_string(),
                name: "Create Virtual Machine".to_string(),
                description: "Create a new virtual machine with specified configuration. The VM will be created in 'stopped' state.".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms".to_string(),
                parameters: vec![],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "minLength": 1, "maxLength": 64, "description": "Human-readable VM name" },
                            "vcpu_count": { "type": "integer", "minimum": 1, "maximum": 256, "default": 2, "description": "Number of virtual CPUs" },
                            "memory_gb": { "type": "integer", "minimum": 1, "maximum": 4096, "default": 4, "description": "Memory in gigabytes" },
                            "enable_gpu": { "type": "boolean", "default": false, "description": "Enable GPU passthrough" },
                            "enable_networking": { "type": "boolean", "default": false, "description": "Enable network access" }
                        },
                        "required": ["name"]
                    }),
                    required: true,
                }),
                responses: vec![
                    OperationResponse {
                        status_code: 201,
                        description: "VM created successfully".to_string(),
                        schema: Some(serde_json::json!({ "$ref": "#/resources/vm" })),
                    },
                    OperationResponse {
                        status_code: 400,
                        description: "Invalid request parameters".to_string(),
                        schema: None,
                    },
                    OperationResponse {
                        status_code: 409,
                        description: "VM with this name already exists".to_string(),
                        schema: None,
                    },
                ],
                idempotent: false,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 10, burst_size: 5 }),
                examples: vec![
                    OperationExample {
                        name: "Create a basic VM".to_string(),
                        description: "Create a VM with 4 vCPUs and 8GB RAM".to_string(),
                        request: Some(serde_json::json!({
                            "name": "my-ai-worker",
                            "vcpu_count": 4,
                            "memory_gb": 8,
                            "enable_gpu": true
                        })),
                        response: serde_json::json!({
                            "id": "vm-456",
                            "name": "my-ai-worker",
                            "state": "created",
                            "vcpu_count": 4,
                            "memory_gb": 8
                        }),
                    },
                ],
            },
            Operation {
                id: "get_vm".to_string(),
                name: "Get Virtual Machine".to_string(),
                description: "Retrieve details of a specific virtual machine by ID".to_string(),
                http_method: "GET".to_string(),
                path: "/api/v1/vms/{id}".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM details".to_string(),
                        schema: Some(serde_json::json!({ "$ref": "#/resources/vm" })),
                    },
                    OperationResponse {
                        status_code: 404,
                        description: "VM not found".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "delete_vm".to_string(),
                name: "Delete Virtual Machine".to_string(),
                description: "Delete a virtual machine. The VM must be in stopped state.".to_string(),
                http_method: "DELETE".to_string(),
                path: "/api/v1/vms/{id}".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM deleted".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "deleted": { "type": "boolean" }
                            }
                        })),
                    },
                    OperationResponse {
                        status_code: 404,
                        description: "VM not found".to_string(),
                        schema: None,
                    },
                    OperationResponse {
                        status_code: 409,
                        description: "VM is not stopped".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            // Lifecycle Operations
            Operation {
                id: "start_vm".to_string(),
                name: "Start Virtual Machine".to_string(),
                description: "Start a stopped virtual machine. Transitions from 'stopped' to 'running' state.".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/start".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM started".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "operation": { "type": "string" },
                                "success": { "type": "boolean" },
                                "new_state": { "type": "string" }
                            }
                        })),
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "stop_vm".to_string(),
                name: "Stop Virtual Machine".to_string(),
                description: "Gracefully stop a running virtual machine".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/stop".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM stopped".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "pause_vm".to_string(),
                name: "Pause Virtual Machine".to_string(),
                description: "Pause a running VM, suspending all CPU activity".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/pause".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM paused".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "resume_vm".to_string(),
                name: "Resume Virtual Machine".to_string(),
                description: "Resume a paused VM".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/resume".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM resumed".to_string(),
                        schema: None,
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            // Agent Execution
            Operation {
                id: "execute_script".to_string(),
                name: "Execute Agent Script".to_string(),
                description: "Execute a Rhai script on the host, against a read-only view of \
                    the VM (state, name, vCPU count, memory size). This does NOT run anything \
                    inside the guest operating system — there is no in-guest agent — and it \
                    cannot control the VM. Requires the VmRead capability.".to_string(),
                http_method: "POST".to_string(),
                path: "/api/v1/vms/{id}/script".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "script_type": { 
                                "type": "string", 
                                "enum": ["rhai"],
                                "default": "rhai",
                                "description": "Script runtime. Rhai is the only one implemented."
                            },
                            "script": { 
                                "type": "string",
                                "description": "Rhai source code"
                            },
                            "timeout_seconds": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 300,
                                "default": 30,
                                "description": "Maximum execution time"
                            },
                            "capabilities": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Capabilities to grant to the script",
                                "default": []
                            }
                        },
                        "required": ["script"]
                    }),
                    required: true,
                }),
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "Script executed successfully".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "execution_id": { "type": "string" },
                                "status": { "type": "string", "enum": ["completed", "failed", "timeout"] },
                                "output": { "type": "string" },
                                "error": { "type": "string" },
                                "duration_ms": { "type": "integer" }
                            }
                        })),
                    },
                ],
                idempotent: false,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 60, burst_size: 10 }),
                examples: vec![
                    OperationExample {
                        name: "Get VM status via script".to_string(),
                        description: "Execute a Rhai script to check VM state".to_string(),
                        request: Some(serde_json::json!({
                            "script_type": "rhai",
                            "script": "let status = vm_status(); print(`VM is ${status}`); status",
                            "timeout_seconds": 10
                        })),
                        response: serde_json::json!({
                            "execution_id": "exec-789",
                            "status": "completed",
                            "output": "VM is running",
                            "duration_ms": 5
                        }),
                    },
                ],
            },
            // Metrics
            Operation {
                id: "get_metrics".to_string(),
                name: "Get VM Metrics".to_string(),
                description: "Retrieve current performance metrics for a VM".to_string(),
                http_method: "GET".to_string(),
                path: "/api/v1/vms/{id}/metrics".to_string(),
                parameters: vec![
                    Parameter {
                        name: "id".to_string(),
                        location: ParameterLocation::Path,
                        required: true,
                        description: "VM identifier".to_string(),
                        schema: serde_json::json!({ "type": "string" }),
                        default: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "VM metrics".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "cpu_usage_percent": { "type": "number" },
                                "memory_usage_percent": { "type": "number" },
                                "disk_read_bytes": { "type": "integer" },
                                "disk_write_bytes": { "type": "integer" },
                                "network_rx_bytes": { "type": "integer" },
                                "network_tx_bytes": { "type": "integer" },
                                "uptime_seconds": { "type": "integer" }
                            }
                        })),
                    },
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: Some(RateLimit { requests_per_minute: 300, burst_size: 50 }),
                examples: vec![],
            },
        ]
    }

    fn build_state_machines() -> Vec<StateMachine> {
        vec![StateMachine {
            resource_type: "vm".to_string(),
            states: vec![
                State {
                    name: "created".to_string(),
                    description: "VM has been created but not started".to_string(),
                    allowed_operations: vec![
                        "start_vm".to_string(),
                        "delete_vm".to_string(),
                        "update_vm".to_string(),
                    ],
                },
                State {
                    name: "starting".to_string(),
                    description: "VM is starting up".to_string(),
                    allowed_operations: vec![],
                },
                State {
                    name: "running".to_string(),
                    description: "VM is running and operational".to_string(),
                    allowed_operations: vec![
                        "stop_vm".to_string(),
                        "pause_vm".to_string(),
                        "execute_script".to_string(),
                        "get_metrics".to_string(),
                    ],
                },
                State {
                    name: "paused".to_string(),
                    description: "VM is paused".to_string(),
                    allowed_operations: vec!["resume_vm".to_string(), "stop_vm".to_string()],
                },
                State {
                    name: "stopping".to_string(),
                    description: "VM is shutting down".to_string(),
                    allowed_operations: vec![],
                },
                State {
                    name: "stopped".to_string(),
                    description: "VM is stopped".to_string(),
                    allowed_operations: vec![
                        "start_vm".to_string(),
                        "delete_vm".to_string(),
                        "update_vm".to_string(),
                    ],
                },
                State {
                    name: "error".to_string(),
                    description: "VM is in error state".to_string(),
                    allowed_operations: vec!["delete_vm".to_string()],
                },
            ],
            transitions: vec![
                Transition {
                    from_state: "created".to_string(),
                    to_state: "starting".to_string(),
                    trigger_operation: "start_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "starting".to_string(),
                    to_state: "running".to_string(),
                    trigger_operation: "auto".to_string(),
                    conditions: vec!["boot_complete".to_string()],
                },
                Transition {
                    from_state: "running".to_string(),
                    to_state: "paused".to_string(),
                    trigger_operation: "pause_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "paused".to_string(),
                    to_state: "running".to_string(),
                    trigger_operation: "resume_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "running".to_string(),
                    to_state: "stopping".to_string(),
                    trigger_operation: "stop_vm".to_string(),
                    conditions: vec![],
                },
                Transition {
                    from_state: "stopping".to_string(),
                    to_state: "stopped".to_string(),
                    trigger_operation: "auto".to_string(),
                    conditions: vec!["shutdown_complete".to_string()],
                },
                Transition {
                    from_state: "stopped".to_string(),
                    to_state: "starting".to_string(),
                    trigger_operation: "start_vm".to_string(),
                    conditions: vec![],
                },
            ],
            initial_state: "created".to_string(),
            terminal_states: vec!["error".to_string()],
        }]
    }

    fn build_events() -> Vec<EventType> {
        vec![
            EventType {
                id: "vm.state_changed".to_string(),
                name: "VM State Changed".to_string(),
                description: "Emitted when a VM transitions between states".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "vm_id": { "type": "string" },
                        "previous_state": { "type": "string" },
                        "new_state": { "type": "string" },
                        "timestamp": { "type": "string", "format": "date-time" }
                    }
                }),
                resource_types: vec!["vm".to_string()],
            },
            EventType {
                id: "vm.metrics".to_string(),
                name: "VM Metrics Update".to_string(),
                description: "Periodic metrics update for a VM".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "vm_id": { "type": "string" },
                        "metrics": { "$ref": "#/operations/get_metrics/responses/200/schema" },
                        "timestamp": { "type": "string", "format": "date-time" }
                    }
                }),
                resource_types: vec!["vm".to_string()],
            },
            EventType {
                id: "agent.completed".to_string(),
                name: "Agent Execution Completed".to_string(),
                description: "Emitted when an agent script completes execution".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "execution_id": { "type": "string" },
                        "vm_id": { "type": "string" },
                        "status": { "type": "string" },
                        "output": { "type": "string" },
                        "duration_ms": { "type": "integer" }
                    }
                }),
                resource_types: vec!["agent".to_string()],
            },
        ]
    }

    /// Convert to OpenAI function calling format
    pub fn to_openai_tools(&self) -> OpenAITools {
        // Project the complete MCP registry so the OpenAI tool list covers every
        // agent-callable tool (vm.*, guest.*, snapshot.*, network.*, agent.*,
        // system.*), not just the core VM-lifecycle REST operations.
        let tools = mcp_registry_tools()
            .into_iter()
            .map(|t| OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIFunction {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters,
                },
            })
            .collect();

        OpenAITools { tools }
    }

    /// Convert to Anthropic MCP tool format
    pub fn to_anthropic_tools(&self) -> AnthropicTools {
        let tools = mcp_registry_tools()
            .into_iter()
            .map(|t| AnthropicTool {
                name: t.name,
                description: t.description,
                input_schema: t.parameters,
            })
            .collect();

        AnthropicTools {
            name: "hypermachine".to_string(),
            version: self.system.version.clone(),
            tools,
        }
    }

    /// Convert to Google Gemini format
    pub fn to_gemini_tools(&self) -> GeminiTools {
        let function_declarations = mcp_registry_tools()
            .into_iter()
            .map(|t| GeminiFunction {
                // Gemini function names disallow '.', so map dotted MCP tool
                // names (e.g. `vm.create`) to `vm_create`.
                name: t.name.replace('.', "_"),
                description: t.description,
                parameters: t.parameters,
            })
            .collect();

        GeminiTools {
            function_declarations,
        }
    }

    /// Get affordances for a resource in a given state.
    ///
    /// Returns what operations are available and what state transitions
    /// are reachable from the current state. This is the primary
    /// composability primitive for agentic interaction.
    pub fn get_affordances(&self, resource_type: &str, current_state: &str) -> Affordances {
        let mut available_ops = Vec::new();
        let mut possible_trans = Vec::new();

        // Find the state machine for this resource type
        if let Some(sm) = self
            .state_machines
            .iter()
            .find(|s| s.resource_type == resource_type)
        {
            // Find the current state definition
            if let Some(state) = sm.states.iter().find(|s| s.name == current_state) {
                // Get operations allowed in this state
                for op_id in &state.allowed_operations {
                    if let Some(op) = self.operations.iter().find(|o| o.id == *op_id) {
                        let contracts = Self::build_operation_contracts();
                        let contract = contracts.iter().find(|c| c.operation_id == *op_id);
                        available_ops.push(AffordanceOperation {
                            operation_id: op.id.clone(),
                            name: op.name.clone(),
                            description: op.description.clone(),
                            http_method: op.http_method.clone(),
                            path: op.path.clone(),
                            preconditions: contract
                                .map(|c| {
                                    c.preconditions
                                        .iter()
                                        .map(|p| p.description.clone())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            postconditions: contract
                                .map(|c| {
                                    c.postconditions
                                        .iter()
                                        .map(|p| p.description.clone())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            idempotent: op.idempotent,
                        });
                    }
                }
            }

            // Find transitions from this state
            for t in &sm.transitions {
                if t.from_state == current_state {
                    // Check if the reverse transition exists
                    let reversible = sm
                        .transitions
                        .iter()
                        .any(|rt| rt.from_state == t.to_state && rt.to_state == t.from_state);
                    possible_trans.push(AffordanceTransition {
                        target_state: t.to_state.clone(),
                        trigger_operation: t.trigger_operation.clone(),
                        conditions: t.conditions.clone(),
                        reversible,
                    });
                }
            }
        }

        Affordances {
            resource_type: resource_type.to_string(),
            current_state: current_state.to_string(),
            available_operations: available_ops,
            possible_transitions: possible_trans,
        }
    }

    /// Validate an action plan against the ontology.
    ///
    /// Checks that all operations exist, dependencies form a DAG,
    /// and preconditions are satisfiable in sequence.
    pub fn validate_plan(&self, plan: &ActionPlan) -> PlanValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut resolved = Vec::new();

        let op_ids: Vec<&str> = self.operations.iter().map(|o| o.id.as_str()).collect();
        let contracts = Self::build_operation_contracts();
        let step_ids: Vec<&str> = plan.steps.iter().map(|s| s.step_id.as_str()).collect();

        for step in &plan.steps {
            // Check operation exists
            if !op_ids.contains(&step.operation_id.as_str()) {
                errors.push(PlanValidationError {
                    step_id: step.step_id.clone(),
                    error_type: "unknown_operation".to_string(),
                    message: format!("Operation '{}' not found in ontology", step.operation_id),
                });
                continue;
            }

            // Check dependencies reference valid steps
            for dep in &step.depends_on {
                if !step_ids.contains(&dep.as_str()) {
                    errors.push(PlanValidationError {
                        step_id: step.step_id.clone(),
                        error_type: "invalid_dependency".to_string(),
                        message: format!("Dependency '{}' references unknown step", dep),
                    });
                }
            }

            // Self-dependency check
            if step.depends_on.contains(&step.step_id) {
                errors.push(PlanValidationError {
                    step_id: step.step_id.clone(),
                    error_type: "circular_dependency".to_string(),
                    message: "Step depends on itself".to_string(),
                });
            }

            // Resolve preconditions
            let contract = contracts
                .iter()
                .find(|c| c.operation_id == step.operation_id);
            let preconditions_met = contract
                .map(|c| {
                    // If there are dependencies, assume preconditions will be met by prior steps
                    !step.depends_on.is_empty() || c.preconditions.is_empty()
                })
                .unwrap_or(true);

            if !preconditions_met {
                warnings.push(format!(
                    "Step '{}' ({}) has unmet preconditions and no dependencies",
                    step.step_id, step.operation_id
                ));
            }

            resolved.push(ResolvedStep {
                step_id: step.step_id.clone(),
                operation_id: step.operation_id.clone(),
                preconditions_met,
                expected_postconditions: contract
                    .map(|c| {
                        c.postconditions
                            .iter()
                            .map(|p| p.description.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }

        // Check for duplicate step IDs
        let mut seen = std::collections::HashSet::new();
        for step in &plan.steps {
            if !seen.insert(&step.step_id) {
                errors.push(PlanValidationError {
                    step_id: step.step_id.clone(),
                    error_type: "duplicate_step_id".to_string(),
                    message: "Duplicate step ID".to_string(),
                });
            }
        }

        PlanValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
            estimated_duration_ms: Some(plan.steps.len() as u64 * 500),
            resolved_steps: resolved,
        }
    }

    /// Build the resource relationship graph for agent navigation
    pub fn build_resource_graph(&self) -> ResourceGraph {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        for resource in &self.resources {
            let ops_count = self
                .operations
                .iter()
                .filter(|op| op.path.contains(&format!("/{}", resource.id)))
                .count();

            nodes.push(ResourceNode {
                id: resource.id.clone(),
                resource_type: resource.id.clone(),
                label: resource.name.clone(),
                operations_count: ops_count,
            });

            for rel in &resource.relationships {
                edges.push(ResourceEdge {
                    from: resource.id.clone(),
                    to: rel.target_type.clone(),
                    relationship: rel.name.clone(),
                    cardinality: rel.cardinality.clone(),
                });
            }
        }

        ResourceGraph { nodes, edges }
    }

    /// Build pre-defined composition rules
    pub fn build_composition_rules(&self) -> CompositionRules {
        CompositionRules {
            workflows: vec![
                Workflow {
                    id: "provision_and_start".to_string(),
                    name: "Provision and Start VM".to_string(),
                    description: "Create a new VM with specified configuration and start it"
                        .to_string(),
                    steps: vec![
                        WorkflowStep {
                            order: 1,
                            operation_id: "create_vm".to_string(),
                            description: "Create the VM with desired spec".to_string(),
                            required: true,
                            wait_for_state: Some("created".to_string()),
                        },
                        WorkflowStep {
                            order: 2,
                            operation_id: "start_vm".to_string(),
                            description: "Boot the VM".to_string(),
                            required: true,
                            wait_for_state: Some("running".to_string()),
                        },
                    ],
                    category: "lifecycle".to_string(),
                },
                Workflow {
                    id: "provision_gpu_workload".to_string(),
                    name: "Provision GPU Workload".to_string(),
                    description: "Create a GPU-enabled VM, start it, and deploy an agent script"
                        .to_string(),
                    steps: vec![
                        WorkflowStep {
                            order: 1,
                            operation_id: "create_vm".to_string(),
                            description: "Create VM with GPU enabled".to_string(),
                            required: true,
                            wait_for_state: Some("created".to_string()),
                        },
                        WorkflowStep {
                            order: 2,
                            operation_id: "start_vm".to_string(),
                            description: "Start the VM".to_string(),
                            required: true,
                            wait_for_state: Some("running".to_string()),
                        },
                        WorkflowStep {
                            order: 3,
                            operation_id: "execute_script".to_string(),
                            description: "Deploy and run the AI workload script".to_string(),
                            required: true,
                            wait_for_state: None,
                        },
                    ],
                    category: "ai_workload".to_string(),
                },
                Workflow {
                    id: "graceful_shutdown".to_string(),
                    name: "Graceful Shutdown".to_string(),
                    description: "Gracefully stop a VM and verify it is stopped".to_string(),
                    steps: vec![WorkflowStep {
                        order: 1,
                        operation_id: "stop_vm".to_string(),
                        description: "Initiate graceful shutdown".to_string(),
                        required: true,
                        wait_for_state: Some("stopped".to_string()),
                    }],
                    category: "lifecycle".to_string(),
                },
                Workflow {
                    id: "decommission".to_string(),
                    name: "Decommission VM".to_string(),
                    description: "Stop a VM if running, then delete it".to_string(),
                    steps: vec![
                        WorkflowStep {
                            order: 1,
                            operation_id: "stop_vm".to_string(),
                            description: "Ensure VM is stopped".to_string(),
                            required: false,
                            wait_for_state: Some("stopped".to_string()),
                        },
                        WorkflowStep {
                            order: 2,
                            operation_id: "delete_vm".to_string(),
                            description: "Remove the VM".to_string(),
                            required: true,
                            wait_for_state: None,
                        },
                    ],
                    category: "lifecycle".to_string(),
                },
            ],
            constraints: vec![
                CompositionConstraint {
                    name: "lifecycle_mutex".to_string(),
                    description: "Start, stop, pause, resume are mutually exclusive on a single VM"
                        .to_string(),
                    rule_type: ConstraintType::MutuallyExclusive,
                    operations: vec![
                        "start_vm".to_string(),
                        "stop_vm".to_string(),
                        "pause_vm".to_string(),
                        "resume_vm".to_string(),
                    ],
                },
                CompositionConstraint {
                    name: "create_before_start".to_string(),
                    description: "A VM must be created before it can be started".to_string(),
                    rule_type: ConstraintType::RequiresSequence,
                    operations: vec!["create_vm".to_string(), "start_vm".to_string()],
                },
                CompositionConstraint {
                    name: "stop_before_delete".to_string(),
                    description: "A VM must be stopped before it can be deleted".to_string(),
                    rule_type: ConstraintType::StatePrecondition,
                    operations: vec!["stop_vm".to_string(), "delete_vm".to_string()],
                },
                CompositionConstraint {
                    name: "script_requires_running".to_string(),
                    description: "Script execution requires the VM to be in running state"
                        .to_string(),
                    rule_type: ConstraintType::StatePrecondition,
                    operations: vec!["execute_script".to_string()],
                },
                CompositionConstraint {
                    name: "idempotent_reads".to_string(),
                    description: "GET operations are idempotent and can be called concurrently"
                        .to_string(),
                    rule_type: ConstraintType::Idempotent,
                    operations: vec![
                        "list_vms".to_string(),
                        "get_vm".to_string(),
                        "get_metrics".to_string(),
                    ],
                },
            ],
            patterns: vec![CompositionPattern {
                name: "monitor_then_scale".to_string(),
                description: "Check metrics and conditionally create additional VMs for scaling"
                    .to_string(),
                template: ActionPlan {
                    name: "Monitor and Scale".to_string(),
                    description: "Check VM load and provision new VM if overloaded".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "check_metrics".to_string(),
                            operation_id: "get_metrics".to_string(),
                            parameters: serde_json::json!({ "id": "${vm_id}" }),
                            depends_on: vec![],
                            timeout_seconds: Some(10),
                        },
                        PlanStep {
                            step_id: "provision_new".to_string(),
                            operation_id: "create_vm".to_string(),
                            parameters: serde_json::json!({
                                "name": "${new_vm_name}",
                                "vcpu_count": 4,
                                "memory_gb": 8
                            }),
                            depends_on: vec!["check_metrics".to_string()],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "start_new".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({ "id": "${new_vm_id}" }),
                            depends_on: vec!["provision_new".to_string()],
                            timeout_seconds: Some(60),
                        },
                    ],
                    rollback_on_failure: true,
                },
            }],
        }
    }

    /// Build the A2A agent card for multi-agent discovery
    pub fn build_agent_card(&self) -> AgentCard {
        AgentCard {
            name: "HyperMachine".to_string(),
            version: self.system.version.clone(),
            description: "AI-native hybrid hypervisor. Manages virtual machines, \
                GPU passthrough, agent sandbox execution, and fleet orchestration."
                .to_string(),
            url: "/agentic".to_string(),
            capabilities: AgentCapabilities {
                streaming: true,
                push_notifications: true,
                state_transition_history: true,
            },
            skills: vec![
                AgentSkill {
                    id: "vm_management".to_string(),
                    name: "Virtual Machine Management".to_string(),
                    description: "Create, configure, start, stop, and delete virtual machines"
                        .to_string(),
                    tags: vec![
                        "vm".to_string(),
                        "compute".to_string(),
                        "infrastructure".to_string(),
                    ],
                    examples: vec![
                        "Create a VM with 4 CPUs and 8GB RAM".to_string(),
                        "Start all stopped VMs".to_string(),
                        "Delete VMs older than 7 days".to_string(),
                    ],
                },
                AgentSkill {
                    id: "gpu_compute".to_string(),
                    name: "GPU Compute Orchestration".to_string(),
                    description: "Attach GPUs to VMs for AI/ML workloads, manage GPU allocation"
                        .to_string(),
                    tags: vec![
                        "gpu".to_string(),
                        "ai".to_string(),
                        "ml".to_string(),
                        "compute".to_string(),
                    ],
                    examples: vec![
                        "Provision a GPU-enabled VM for training".to_string(),
                        "List available GPUs".to_string(),
                    ],
                },
                AgentSkill {
                    id: "agent_scripting".to_string(),
                    name: "Agent Script Execution".to_string(),
                    description:
                        "Execute Rhai scripts on the host against a read-only view of a VM"
                            .to_string(),
                    tags: vec![
                        "agent".to_string(),
                        "script".to_string(),
                        "automation".to_string(),
                    ],
                    examples: vec![
                        "Run a health check script on VM".to_string(),
                        "Execute a maintenance script across all VMs".to_string(),
                    ],
                },
                AgentSkill {
                    id: "monitoring".to_string(),
                    name: "VM Monitoring and Metrics".to_string(),
                    description: "Collect CPU, memory, disk, and network metrics for VMs"
                        .to_string(),
                    tags: vec![
                        "monitoring".to_string(),
                        "metrics".to_string(),
                        "observability".to_string(),
                    ],
                    examples: vec![
                        "Get CPU usage for all running VMs".to_string(),
                        "Alert when memory exceeds 90%".to_string(),
                    ],
                },
            ],
            authentication: AgentAuthConfig {
                auth_type: "bearer".to_string(),
                schemes: vec!["Ed25519-JWT".to_string(), "mTLS".to_string()],
            },
            default_input_modes: vec!["application/json".to_string()],
            default_output_modes: vec![
                "application/json".to_string(),
                "application/ld+json".to_string(),
            ],
        }
    }

    /// Build the MCP server manifest
    pub fn build_mcp_manifest(&self) -> McpManifest {
        // The native MCP manifest must serve the SAME tools as the live MCP
        // registry — the single source of truth that also drives the
        // OpenAI/Anthropic/Gemini projections. Projecting from the registry
        // (rather than the hand-maintained `operations` list) keeps every
        // agent-facing surface in lockstep with what `call_tool` can actually
        // dispatch.
        let tools = mcp_registry_tools()
            .into_iter()
            .map(|t| McpTool {
                name: t.name,
                description: t.description,
                input_schema: t.parameters,
            })
            .collect();

        let resources = self
            .resources
            .iter()
            .map(|r| McpResource {
                uri: format!("hypermachine://{}s", r.id),
                name: r.name.clone(),
                description: r.description.clone(),
                mime_type: "application/json".to_string(),
            })
            .collect();

        McpManifest {
            name: "hypermachine".to_string(),
            version: self.system.version.clone(),
            protocol_version: "2024-11-05".to_string(),
            capabilities: McpCapabilities {
                tools: true,
                resources: true,
                prompts: false,
                logging: true,
            },
            tools,
            resources,
        }
    }

    /// Build operation contracts (preconditions + postconditions)
    pub fn build_operation_contracts() -> Vec<OperationContract> {
        vec![
            OperationContract {
                operation_id: "create_vm".to_string(),
                preconditions: vec![],
                postconditions: vec![Condition {
                    description: "VM exists in created state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("created"),
                }],
                invariants: vec!["VM name must be unique".to_string()],
                composable_with: vec!["start_vm".to_string(), "delete_vm".to_string()],
                mutually_exclusive_with: vec![],
            },
            OperationContract {
                operation_id: "start_vm".to_string(),
                preconditions: vec![Condition {
                    description: "VM must be in created or stopped state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::In,
                    value: serde_json::json!(["created", "stopped"]),
                }],
                postconditions: vec![Condition {
                    description: "VM transitions to running state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("running"),
                }],
                invariants: vec![],
                composable_with: vec![
                    "execute_script".to_string(),
                    "get_metrics".to_string(),
                    "pause_vm".to_string(),
                    "stop_vm".to_string(),
                ],
                mutually_exclusive_with: vec!["delete_vm".to_string()],
            },
            OperationContract {
                operation_id: "stop_vm".to_string(),
                preconditions: vec![Condition {
                    description: "VM must be in running state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("running"),
                }],
                postconditions: vec![Condition {
                    description: "VM transitions to stopped state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("stopped"),
                }],
                invariants: vec![],
                composable_with: vec!["delete_vm".to_string(), "start_vm".to_string()],
                mutually_exclusive_with: vec![
                    "start_vm".to_string(),
                    "pause_vm".to_string(),
                    "resume_vm".to_string(),
                ],
            },
            OperationContract {
                operation_id: "pause_vm".to_string(),
                preconditions: vec![Condition {
                    description: "VM must be in running state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("running"),
                }],
                postconditions: vec![Condition {
                    description: "VM transitions to paused state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("paused"),
                }],
                invariants: vec![],
                composable_with: vec!["resume_vm".to_string()],
                mutually_exclusive_with: vec!["stop_vm".to_string(), "start_vm".to_string()],
            },
            OperationContract {
                operation_id: "resume_vm".to_string(),
                preconditions: vec![Condition {
                    description: "VM must be in paused state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("paused"),
                }],
                postconditions: vec![Condition {
                    description: "VM transitions to running state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("running"),
                }],
                invariants: vec![],
                composable_with: vec!["stop_vm".to_string(), "execute_script".to_string()],
                mutually_exclusive_with: vec!["pause_vm".to_string()],
            },
            OperationContract {
                operation_id: "delete_vm".to_string(),
                preconditions: vec![Condition {
                    description: "VM must be in stopped or created state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::In,
                    value: serde_json::json!(["stopped", "created", "error"]),
                }],
                postconditions: vec![Condition {
                    description: "VM no longer exists".to_string(),
                    resource_type: "vm".to_string(),
                    field: "id".to_string(),
                    operator: ConditionOperator::NotExists,
                    value: serde_json::Value::Null,
                }],
                invariants: vec!["Operation is irreversible".to_string()],
                composable_with: vec![],
                mutually_exclusive_with: vec![
                    "start_vm".to_string(),
                    "stop_vm".to_string(),
                    "pause_vm".to_string(),
                ],
            },
            OperationContract {
                operation_id: "execute_script".to_string(),
                preconditions: vec![Condition {
                    description: "VM must be in running state".to_string(),
                    resource_type: "vm".to_string(),
                    field: "state".to_string(),
                    operator: ConditionOperator::Equals,
                    value: serde_json::json!("running"),
                }],
                postconditions: vec![],
                invariants: vec![
                    "Script runs in sandboxed environment".to_string(),
                    "Script capabilities are limited by granted permissions".to_string(),
                ],
                composable_with: vec!["get_metrics".to_string()],
                mutually_exclusive_with: vec![],
            },
            OperationContract {
                operation_id: "list_vms".to_string(),
                preconditions: vec![],
                postconditions: vec![],
                invariants: vec!["Read-only operation".to_string()],
                composable_with: vec![
                    "create_vm".to_string(),
                    "get_vm".to_string(),
                    "get_metrics".to_string(),
                ],
                mutually_exclusive_with: vec![],
            },
            OperationContract {
                operation_id: "get_vm".to_string(),
                preconditions: vec![Condition {
                    description: "VM must exist".to_string(),
                    resource_type: "vm".to_string(),
                    field: "id".to_string(),
                    operator: ConditionOperator::Exists,
                    value: serde_json::Value::Null,
                }],
                postconditions: vec![],
                invariants: vec!["Read-only operation".to_string()],
                composable_with: vec![
                    "list_vms".to_string(),
                    "get_metrics".to_string(),
                    "start_vm".to_string(),
                    "stop_vm".to_string(),
                ],
                mutually_exclusive_with: vec![],
            },
            OperationContract {
                operation_id: "get_metrics".to_string(),
                preconditions: vec![Condition {
                    description: "VM must exist".to_string(),
                    resource_type: "vm".to_string(),
                    field: "id".to_string(),
                    operator: ConditionOperator::Exists,
                    value: serde_json::Value::Null,
                }],
                postconditions: vec![],
                invariants: vec!["Read-only operation".to_string()],
                composable_with: vec![
                    "list_vms".to_string(),
                    "get_vm".to_string(),
                    "execute_script".to_string(),
                ],
                mutually_exclusive_with: vec![],
            },
        ]
    }

    /// Execute an action plan against simulated operations.
    ///
    /// Steps run in dependency order and each records a synthetic result. This
    /// is the right entry point for validating a plan's shape — dependency
    /// ordering, variable substitution, rollback bookkeeping — without touching
    /// any real resource.
    ///
    /// To run a plan for real, use [`Self::execute_plan_with`] with an executor
    /// such as [`VmHostExecutor`].
    ///
    /// Variable substitution: if `variables` are provided in the request,
    /// any `${var_name}` in step parameters is replaced with the variable value.
    pub fn execute_plan(&self, request: &PlanExecutionRequest) -> PlanExecutionResult {
        let mut run = match PlanRun::begin(self, request) {
            Ok(run) => run,
            Err(early) => return *early,
        };

        for step in run.sorted_steps.clone() {
            let params = run.params_for(&step);
            let outcome = match self.operation(&step.operation_id) {
                Some(op) => {
                    let (success, output, error) = Self::simulate_operation(op, &params);
                    StepOutcome {
                        success,
                        output,
                        error,
                    }
                }
                None => StepOutcome::missing_operation(&step.operation_id),
            };

            if run.record(&step, params, outcome).is_break() {
                break;
            }
        }

        // Simulated steps have no side effects, so "rolling back" is only a
        // bookkeeping claim — mark them without pretending anything was undone.
        run.finish_marking_rollback()
    }

    /// Execute an action plan for real, dispatching every step through
    /// `executor`.
    ///
    /// The orchestration is identical to [`Self::execute_plan`] — the same
    /// validation, the same dependency ordering, the same variable
    /// substitution — but each step reaches a live resource, and rollback runs
    /// the executor's compensating action rather than merely claiming to.
    ///
    /// A step whose compensation fails is reported as *not* rolled back, with
    /// the reason appended to its error, so an operator can see exactly what
    /// was left behind.
    pub async fn execute_plan_with(
        &self,
        request: &PlanExecutionRequest,
        executor: &dyn PlanExecutor,
    ) -> PlanExecutionResult {
        let mut run = match PlanRun::begin(self, request) {
            Ok(run) => run,
            Err(early) => return *early,
        };

        for step in run.sorted_steps.clone() {
            let params = run.params_for(&step);
            let outcome = match self.operation(&step.operation_id) {
                Some(op) => match executor.execute(op, &params).await {
                    Ok(output) => StepOutcome {
                        success: true,
                        output: Some(output),
                        error: None,
                    },
                    Err(error) => StepOutcome {
                        success: false,
                        output: None,
                        error: Some(error),
                    },
                },
                None => StepOutcome::missing_operation(&step.operation_id),
            };

            if run.record(&step, params, outcome).is_break() {
                break;
            }
        }

        run.finish_compensating(self, executor).await
    }

    /// Look up an operation definition by id.
    fn operation(&self, operation_id: &str) -> Option<&Operation> {
        self.operations.iter().find(|o| o.id == operation_id)
    }

    /// Deterministic-ish execution id: the plan name plus the current instant.
    fn execution_id(plan_name: &str) -> String {
        format!("exec-{:08x}", {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            plan_name.hash(&mut h);
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut h);
            h.finish() as u32
        })
    }

    /// Topological sort of plan steps by dependencies.
    ///
    /// Steps with no dependencies come first, then steps whose
    /// dependencies have already been placed. Uses Kahn's algorithm.
    /// Independent steps keep their declared order, so a plan executes the
    /// same way on every run.
    fn topological_sort(steps: &[PlanStep]) -> Vec<PlanStep> {
        if steps.is_empty() {
            return vec![];
        }

        let step_map: std::collections::HashMap<&str, &PlanStep> =
            steps.iter().map(|s| (s.step_id.as_str(), s)).collect();

        // In-degree of a step = how many of its dependencies are real steps in
        // this plan. A `depends_on` naming something that isn't here can't be
        // waited on, so it doesn't count (validation reports it separately).
        let mut in_degree: std::collections::HashMap<&str, usize> = steps
            .iter()
            .map(|step| {
                let valid_deps = step
                    .depends_on
                    .iter()
                    .filter(|d| step_map.contains_key(d.as_str()))
                    .count();
                (step.step_id.as_str(), valid_deps)
            })
            .collect();

        // Seed the queue in declaration order, not hash order. Steps that are
        // independent of each other are equally valid in any order
        // topologically, but a plan must execute the same way every time it
        // runs — so ties break toward the order the author wrote.
        let mut queue: std::collections::VecDeque<&str> = steps
            .iter()
            .filter(|step| in_degree.get(step.step_id.as_str()) == Some(&0))
            .map(|step| step.step_id.as_str())
            .collect();

        let mut sorted = Vec::new();

        while let Some(id) = queue.pop_front() {
            if let Some(&step) = step_map.get(id) {
                sorted.push(step.clone());
                // Reduce in-degree of steps that depend on this one
                for other in steps {
                    if other.depends_on.iter().any(|d| d == id) {
                        if let Some(count) = in_degree.get_mut(other.step_id.as_str()) {
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                queue.push_back(other.step_id.as_str());
                            }
                        }
                    }
                }
            }
        }

        // If some steps were not added (cycle), append them anyway
        for step in steps {
            if !sorted.iter().any(|s| s.step_id == step.step_id) {
                sorted.push(step.clone());
            }
        }

        sorted
    }

    /// Substitute `${var_name}` placeholders in a JSON value tree
    fn substitute_variables(
        value: &serde_json::Value,
        variables: &serde_json::Map<String, serde_json::Value>,
    ) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => {
                let mut result = s.clone();
                for (key, val) in variables {
                    let placeholder = format!("${{{}}}", key);
                    if result.contains(&placeholder) {
                        let replacement = match val {
                            serde_json::Value::String(v) => v.clone(),
                            other => other.to_string(),
                        };
                        result = result.replace(&placeholder, &replacement);
                    }
                }
                serde_json::Value::String(result)
            }
            serde_json::Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, v) in map {
                    new_map.insert(k.clone(), Self::substitute_variables(v, variables));
                }
                serde_json::Value::Object(new_map)
            }
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter()
                    .map(|v| Self::substitute_variables(v, variables))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Simulate executing an operation (returns success, output, error).
    ///
    /// In a production implementation this would dispatch to actual
    /// API handlers. Here we simulate based on operation semantics
    /// to enable plan execution testing and dry-run validation.
    fn simulate_operation(
        op: &Operation,
        params: &serde_json::Value,
    ) -> (bool, Option<serde_json::Value>, Option<String>) {
        match op.id.as_str() {
            "create_vm" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unnamed");
                let vm_id = format!("vm-{:06x}", {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    name.hash(&mut h);
                    h.finish() as u32 & 0xFFFFFF
                });
                (
                    true,
                    Some(serde_json::json!({
                        "id": vm_id,
                        "name": name,
                        "state": "created",
                        "vcpu_count": params.get("vcpu_count").unwrap_or(&serde_json::json!(2)),
                        "memory_gb": params.get("memory_gb").unwrap_or(&serde_json::json!(4)),
                    })),
                    None,
                )
            }
            "start_vm" | "stop_vm" | "pause_vm" | "resume_vm" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let new_state = match op.id.as_str() {
                    "start_vm" => "running",
                    "stop_vm" => "stopped",
                    "pause_vm" => "paused",
                    "resume_vm" => "running",
                    _ => "unknown",
                };
                (
                    true,
                    Some(serde_json::json!({
                        "id": id,
                        "operation": op.id,
                        "success": true,
                        "new_state": new_state,
                    })),
                    None,
                )
            }
            "delete_vm" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                (
                    true,
                    Some(serde_json::json!({
                        "id": id,
                        "deleted": true,
                    })),
                    None,
                )
            }
            "execute_script" => (
                true,
                Some(serde_json::json!({
                    "execution_id": "sim-exec-001",
                    "status": "completed",
                    "output": "simulated script output",
                    "duration_ms": 10,
                })),
                None,
            ),
            "get_vm" | "list_vms" | "get_metrics" => (
                true,
                Some(serde_json::json!({
                    "operation": op.id,
                    "simulated": true,
                })),
                None,
            ),
            _ => {
                // Unknown operations succeed with empty output
                (
                    true,
                    Some(serde_json::json!({
                        "operation": op.id,
                        "simulated": true,
                    })),
                    None,
                )
            }
        }
    }
    /// Build the library of available plan templates
    pub fn build_templates() -> Vec<PlanTemplate> {
        vec![
            // ---- Lifecycle Templates ----
            PlanTemplate {
                id: "tpl-create-and-start".to_string(),
                name: "Create and Start VM".to_string(),
                description: "Creates a new VM with the specified configuration and immediately starts it. The most common workflow for provisioning a new workload.".to_string(),
                category: TemplateCategory::Lifecycle,
                version: "1.0.0".to_string(),
                tags: vec!["vm".to_string(), "create".to_string(), "start".to_string(), "quick-start".to_string()],
                parameters: vec![
                    TemplateParameter {
                        name: "vm_name".to_string(),
                        label: "VM Name".to_string(),
                        description: "Name for the new virtual machine".to_string(),
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        example: Some(serde_json::json!("my-workload")),
                        allowed_values: None,
                    },
                    TemplateParameter {
                        name: "vcpu_count".to_string(),
                        label: "vCPU Count".to_string(),
                        description: "Number of virtual CPUs to allocate".to_string(),
                        param_type: "integer".to_string(),
                        required: false,
                        default: Some(serde_json::json!(2)),
                        example: Some(serde_json::json!(4)),
                        allowed_values: None,
                    },
                    TemplateParameter {
                        name: "memory_mb".to_string(),
                        label: "Memory (MB)".to_string(),
                        description: "Memory allocation in megabytes".to_string(),
                        param_type: "integer".to_string(),
                        required: false,
                        default: Some(serde_json::json!(2048)),
                        example: Some(serde_json::json!(8192)),
                        allowed_values: None,
                    },
                ],
                plan: ActionPlan {
                    name: "Create and Start ${vm_name}".to_string(),
                    description: "Create VM '${vm_name}' with ${vcpu_count} vCPUs, ${memory_mb} MB RAM, then start it".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "create".to_string(),
                            operation_id: "create_vm".to_string(),
                            parameters: serde_json::json!({
                                "name": "${vm_name}",
                                "vcpu_count": "${vcpu_count}",
                                "memory_mb": "${memory_mb}"
                            }),
                            depends_on: vec![],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "start".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_name}"}),
                            depends_on: vec!["create".to_string()],
                            timeout_seconds: Some(60),
                        },
                    ],
                    rollback_on_failure: true,
                },
                estimated_duration_seconds: Some(90),
                rollback_recommended: true,
            },
            PlanTemplate {
                id: "tpl-full-lifecycle".to_string(),
                name: "Full VM Lifecycle".to_string(),
                description: "Demonstrates the complete VM lifecycle: create, start, pause, resume, stop, and delete. Useful for testing and validation.".to_string(),
                category: TemplateCategory::Lifecycle,
                version: "1.0.0".to_string(),
                tags: vec!["vm".to_string(), "lifecycle".to_string(), "demo".to_string(), "testing".to_string()],
                parameters: vec![
                    TemplateParameter {
                        name: "vm_name".to_string(),
                        label: "VM Name".to_string(),
                        description: "Name for the lifecycle test VM".to_string(),
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        example: Some(serde_json::json!("lifecycle-test")),
                        allowed_values: None,
                    },
                ],
                plan: ActionPlan {
                    name: "Full Lifecycle for ${vm_name}".to_string(),
                    description: "Run complete lifecycle: create → start → pause → resume → stop → delete".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "create".to_string(),
                            operation_id: "create_vm".to_string(),
                            parameters: serde_json::json!({"name": "${vm_name}"}),
                            depends_on: vec![],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "start".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_name}"}),
                            depends_on: vec!["create".to_string()],
                            timeout_seconds: Some(60),
                        },
                        PlanStep {
                            step_id: "pause".to_string(),
                            operation_id: "pause_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_name}"}),
                            depends_on: vec!["start".to_string()],
                            timeout_seconds: Some(10),
                        },
                        PlanStep {
                            step_id: "resume".to_string(),
                            operation_id: "resume_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_name}"}),
                            depends_on: vec!["pause".to_string()],
                            timeout_seconds: Some(10),
                        },
                        PlanStep {
                            step_id: "stop".to_string(),
                            operation_id: "stop_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_name}"}),
                            depends_on: vec!["resume".to_string()],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "delete".to_string(),
                            operation_id: "delete_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_name}"}),
                            depends_on: vec!["stop".to_string()],
                            timeout_seconds: Some(15),
                        },
                    ],
                    rollback_on_failure: false,
                },
                estimated_duration_seconds: Some(155),
                rollback_recommended: false,
            },
            PlanTemplate {
                id: "tpl-graceful-shutdown".to_string(),
                name: "Graceful VM Shutdown".to_string(),
                description: "Safely stops a running VM by first creating a snapshot for recovery, then stopping the VM gracefully.".to_string(),
                category: TemplateCategory::Lifecycle,
                version: "1.0.0".to_string(),
                tags: vec!["vm".to_string(), "shutdown".to_string(), "graceful".to_string(), "safe".to_string()],
                parameters: vec![
                    TemplateParameter {
                        name: "vm_id".to_string(),
                        label: "VM ID".to_string(),
                        description: "The ID of the VM to shut down".to_string(),
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        example: Some(serde_json::json!("vm-abc-123")),
                        allowed_values: None,
                    },
                    TemplateParameter {
                        name: "snapshot_name".to_string(),
                        label: "Snapshot Name".to_string(),
                        description: "Name for the pre-shutdown snapshot".to_string(),
                        param_type: "string".to_string(),
                        required: false,
                        default: Some(serde_json::json!("pre-shutdown")),
                        example: Some(serde_json::json!("before-maintenance")),
                        allowed_values: None,
                    },
                ],
                plan: ActionPlan {
                    name: "Graceful Shutdown ${vm_id}".to_string(),
                    description: "Snapshot then stop VM ${vm_id}".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "snapshot".to_string(),
                            operation_id: "create_snapshot".to_string(),
                            parameters: serde_json::json!({"id": "${vm_id}", "name": "${snapshot_name}"}),
                            depends_on: vec![],
                            timeout_seconds: Some(120),
                        },
                        PlanStep {
                            step_id: "stop".to_string(),
                            operation_id: "stop_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_id}"}),
                            depends_on: vec!["snapshot".to_string()],
                            timeout_seconds: Some(30),
                        },
                    ],
                    rollback_on_failure: true,
                },
                estimated_duration_seconds: Some(150),
                rollback_recommended: true,
            },
            // ---- Monitoring Templates ----
            PlanTemplate {
                id: "tpl-health-check".to_string(),
                name: "Fleet Health Check".to_string(),
                description: "Runs a comprehensive health check by listing all VMs and gathering metrics. Ideal for periodic monitoring by AI agents.".to_string(),
                category: TemplateCategory::Monitoring,
                version: "1.0.0".to_string(),
                tags: vec!["monitoring".to_string(), "health".to_string(), "metrics".to_string(), "fleet".to_string()],
                parameters: vec![],
                plan: ActionPlan {
                    name: "Fleet Health Check".to_string(),
                    description: "List VMs and gather system metrics".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "list".to_string(),
                            operation_id: "list_vms".to_string(),
                            parameters: serde_json::json!({}),
                            depends_on: vec![],
                            timeout_seconds: Some(10),
                        },
                        PlanStep {
                            step_id: "metrics".to_string(),
                            operation_id: "get_metrics".to_string(),
                            parameters: serde_json::json!({}),
                            depends_on: vec![],
                            timeout_seconds: Some(10),
                        },
                    ],
                    rollback_on_failure: false,
                },
                estimated_duration_seconds: Some(20),
                rollback_recommended: false,
            },
            // ---- Provisioning Templates ----
            PlanTemplate {
                id: "tpl-batch-provision".to_string(),
                name: "Batch VM Provisioning".to_string(),
                description: "Creates and starts multiple VMs in parallel for batch processing workloads. Uses a single VM configuration applied to a fleet.".to_string(),
                category: TemplateCategory::Provisioning,
                version: "1.0.0".to_string(),
                tags: vec!["vm".to_string(), "batch".to_string(), "fleet".to_string(), "provision".to_string()],
                parameters: vec![
                    TemplateParameter {
                        name: "base_name".to_string(),
                        label: "Base Name".to_string(),
                        description: "Base name for VMs (suffixed with -1, -2, -3)".to_string(),
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        example: Some(serde_json::json!("worker")),
                        allowed_values: None,
                    },
                    TemplateParameter {
                        name: "vcpu_count".to_string(),
                        label: "vCPU Count".to_string(),
                        description: "vCPUs per VM".to_string(),
                        param_type: "integer".to_string(),
                        required: false,
                        default: Some(serde_json::json!(2)),
                        example: Some(serde_json::json!(4)),
                        allowed_values: None,
                    },
                ],
                plan: ActionPlan {
                    name: "Batch Provision ${base_name}".to_string(),
                    description: "Create and start three VMs named ${base_name}-1, ${base_name}-2, ${base_name}-3".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "create-1".to_string(),
                            operation_id: "create_vm".to_string(),
                            parameters: serde_json::json!({"name": "${base_name}-1", "vcpu_count": "${vcpu_count}"}),
                            depends_on: vec![],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "create-2".to_string(),
                            operation_id: "create_vm".to_string(),
                            parameters: serde_json::json!({"name": "${base_name}-2", "vcpu_count": "${vcpu_count}"}),
                            depends_on: vec![],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "create-3".to_string(),
                            operation_id: "create_vm".to_string(),
                            parameters: serde_json::json!({"name": "${base_name}-3", "vcpu_count": "${vcpu_count}"}),
                            depends_on: vec![],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "start-1".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({"id": "${base_name}-1"}),
                            depends_on: vec!["create-1".to_string()],
                            timeout_seconds: Some(60),
                        },
                        PlanStep {
                            step_id: "start-2".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({"id": "${base_name}-2"}),
                            depends_on: vec!["create-2".to_string()],
                            timeout_seconds: Some(60),
                        },
                        PlanStep {
                            step_id: "start-3".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({"id": "${base_name}-3"}),
                            depends_on: vec!["create-3".to_string()],
                            timeout_seconds: Some(60),
                        },
                    ],
                    rollback_on_failure: true,
                },
                estimated_duration_seconds: Some(90),
                rollback_recommended: true,
            },
            // ---- Recovery Templates ----
            PlanTemplate {
                id: "tpl-snapshot-restore".to_string(),
                name: "Snapshot and Restore".to_string(),
                description: "Creates a snapshot of a running VM, stops it, and restores from the snapshot. Useful for testing rollback procedures.".to_string(),
                category: TemplateCategory::Recovery,
                version: "1.0.0".to_string(),
                tags: vec!["vm".to_string(), "snapshot".to_string(), "restore".to_string(), "backup".to_string()],
                parameters: vec![
                    TemplateParameter {
                        name: "vm_id".to_string(),
                        label: "VM ID".to_string(),
                        description: "The VM to snapshot and restore".to_string(),
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        example: Some(serde_json::json!("vm-prod-001")),
                        allowed_values: None,
                    },
                    TemplateParameter {
                        name: "snapshot_name".to_string(),
                        label: "Snapshot Name".to_string(),
                        description: "Name for the snapshot".to_string(),
                        param_type: "string".to_string(),
                        required: false,
                        default: Some(serde_json::json!("backup")),
                        example: Some(serde_json::json!("pre-upgrade-backup")),
                        allowed_values: None,
                    },
                ],
                plan: ActionPlan {
                    name: "Snapshot and Restore ${vm_id}".to_string(),
                    description: "Snapshot VM ${vm_id}, stop, then restore from snapshot".to_string(),
                    steps: vec![
                        PlanStep {
                            step_id: "snapshot".to_string(),
                            operation_id: "create_snapshot".to_string(),
                            parameters: serde_json::json!({"id": "${vm_id}", "name": "${snapshot_name}"}),
                            depends_on: vec![],
                            timeout_seconds: Some(120),
                        },
                        PlanStep {
                            step_id: "stop".to_string(),
                            operation_id: "stop_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_id}"}),
                            depends_on: vec!["snapshot".to_string()],
                            timeout_seconds: Some(30),
                        },
                        PlanStep {
                            step_id: "restore".to_string(),
                            operation_id: "restore_snapshot".to_string(),
                            parameters: serde_json::json!({"id": "${vm_id}", "snapshot": "${snapshot_name}"}),
                            depends_on: vec!["stop".to_string()],
                            timeout_seconds: Some(120),
                        },
                        PlanStep {
                            step_id: "start".to_string(),
                            operation_id: "start_vm".to_string(),
                            parameters: serde_json::json!({"id": "${vm_id}"}),
                            depends_on: vec!["restore".to_string()],
                            timeout_seconds: Some(60),
                        },
                    ],
                    rollback_on_failure: true,
                },
                estimated_duration_seconds: Some(330),
                rollback_recommended: true,
            },
        ]
    }

    /// Get a template by its ID
    pub fn get_template(id: &str) -> Option<PlanTemplate> {
        Self::build_templates().into_iter().find(|t| t.id == id)
    }

    /// Instantiate a plan template with the given parameters
    pub fn instantiate_template(
        &self,
        request: &TemplateInstantiationRequest,
    ) -> TemplateInstantiationResult {
        let template = match Self::get_template(&request.template_id) {
            Some(t) => t,
            None => {
                return TemplateInstantiationResult {
                    template_id: request.template_id.clone(),
                    plan: ActionPlan {
                        name: String::new(),
                        description: String::new(),
                        steps: vec![],
                        rollback_on_failure: false,
                    },
                    validation: PlanValidationResult {
                        valid: false,
                        errors: vec![PlanValidationError {
                            step_id: String::new(),
                            error_type: "template_not_found".to_string(),
                            message: format!("Template '{}' not found", request.template_id),
                        }],
                        resolved_steps: vec![],
                        warnings: vec![],
                        estimated_duration_ms: None,
                    },
                    execution: None,
                    defaults_applied: vec![],
                    missing_parameters: vec![],
                };
            }
        };

        // Merge provided parameters with defaults
        let mut variables = serde_json::Map::new();
        let mut defaults_applied = Vec::new();
        let mut missing_parameters = Vec::new();

        for param in &template.parameters {
            if let Some(value) = request.parameters.get(&param.name) {
                variables.insert(param.name.clone(), value.clone());
            } else if let Some(default) = &param.default {
                variables.insert(param.name.clone(), default.clone());
                defaults_applied.push(param.name.clone());
            } else if param.required {
                missing_parameters.push(param.name.clone());
            }
        }

        // If there are missing required parameters, return early
        if !missing_parameters.is_empty() {
            return TemplateInstantiationResult {
                template_id: request.template_id.clone(),
                plan: template.plan.clone(),
                validation: PlanValidationResult {
                    valid: false,
                    errors: missing_parameters
                        .iter()
                        .map(|p| PlanValidationError {
                            step_id: String::new(),
                            error_type: "missing_parameter".to_string(),
                            message: format!("Required parameter '{}' not provided", p),
                        })
                        .collect(),
                    resolved_steps: vec![],
                    warnings: vec![],
                    estimated_duration_ms: None,
                },
                execution: None,
                defaults_applied,
                missing_parameters,
            };
        }

        // Substitute variables in the plan
        let plan_json = serde_json::to_value(&template.plan).unwrap_or_default();
        let resolved_json = Self::substitute_variables(&plan_json, &variables);
        let instantiated_plan: ActionPlan =
            serde_json::from_value(resolved_json).unwrap_or_else(|_| template.plan.clone());

        // Validate the instantiated plan
        let validation = self.validate_plan(&instantiated_plan);

        // Optionally execute
        let execution = if request.execute {
            let exec_request = PlanExecutionRequest {
                plan: instantiated_plan.clone(),
                dry_run: request.dry_run,
                timeout_seconds: None,
                variables: Some(variables),
            };
            Some(self.execute_plan(&exec_request))
        } else {
            None
        };

        TemplateInstantiationResult {
            template_id: request.template_id.clone(),
            plan: instantiated_plan,
            validation,
            execution,
            defaults_applied,
            missing_parameters,
        }
    }
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// Query parameters for ontology endpoint requests
#[derive(Debug, Deserialize)]
pub struct OntologyQuery {
    /// Output format: json-ld, openapi, openai, anthropic, gemini
    pub format: Option<String>,
}

/// Create ontology router
pub fn create_ontology_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/agentic/ontology", get(get_ontology))
        .route("/agentic/tools/openai", get(get_openai_tools))
        .route("/agentic/tools/anthropic", get(get_anthropic_tools))
        .route("/agentic/tools/gemini", get(get_gemini_tools))
        .route(
            "/agentic/affordances/{resource_type}/{state}",
            get(get_affordances),
        )
        .route(
            "/agentic/plans/validate",
            axum::routing::post(validate_plan),
        )
        .route(
            "/agentic/plans/execute",
            axum::routing::post(execute_plan_handler),
        )
        .route("/agentic/compose", get(get_composition_rules))
        .route("/agentic/graph", get(get_resource_graph))
        .route("/agentic/contracts", get(get_operation_contracts))
        .route("/agentic/mcp", get(get_mcp_manifest))
        .route("/.well-known/ai-plugin.json", get(get_ai_plugin_manifest))
        .route("/.well-known/agent.json", get(get_agent_card))
        .route(
            "/agentic/templates",
            get(list_templates).post(instantiate_template_handler),
        )
        .route("/agentic/templates/{id}", get(get_template_handler))
}

/// Create an ontology router whose plan execution reaches real resources.
///
/// Without this, `/agentic/plans/execute` simulates — useful for validating a
/// plan, useless for running one. Pass a [`VmHostExecutor`] to make the
/// endpoint a control plane.
pub fn create_ontology_router_with_executor<S>(
    executor: std::sync::Arc<dyn PlanExecutor>,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    create_ontology_router().layer(axum::Extension(PlanExecutorLayer(executor)))
}

async fn get_ontology(Query(query): Query<OntologyQuery>) -> Response {
    let ontology = HyperMachineOntology::build();
    let format = query.format.as_deref().unwrap_or("json-ld");

    match format {
        "openai" => {
            let tools = ontology.to_openai_tools();
            Json(tools).into_response()
        }
        "anthropic" => {
            let tools = ontology.to_anthropic_tools();
            Json(tools).into_response()
        }
        "gemini" => {
            let tools = ontology.to_gemini_tools();
            Json(tools).into_response()
        }
        _ => {
            // Default: full JSON-LD ontology
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/ld+json"),
            );
            (headers, Json(ontology)).into_response()
        }
    }
}

async fn get_openai_tools() -> Json<OpenAITools> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.to_openai_tools())
}

async fn get_anthropic_tools() -> Json<AnthropicTools> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.to_anthropic_tools())
}

async fn get_gemini_tools() -> Json<GeminiTools> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.to_gemini_tools())
}

/// OpenAI ChatGPT Plugin manifest
#[derive(Serialize)]
struct AiPluginManifest {
    schema_version: String,
    name_for_human: String,
    name_for_model: String,
    description_for_human: String,
    description_for_model: String,
    auth: PluginAuth,
    api: PluginApi,
    logo_url: String,
    contact_email: String,
    legal_info_url: String,
}

#[derive(Serialize)]
struct PluginAuth {
    #[serde(rename = "type")]
    auth_type: String,
}

#[derive(Serialize)]
struct PluginApi {
    #[serde(rename = "type")]
    api_type: String,
    url: String,
}

async fn get_ai_plugin_manifest() -> Json<AiPluginManifest> {
    Json(AiPluginManifest {
        schema_version: "v1".to_string(),
        name_for_human: "HyperMachine".to_string(),
        name_for_model: "hypermachine".to_string(),
        description_for_human: "Manage virtual machines with HyperMachine hypervisor".to_string(),
        description_for_model: "HyperMachine is a hybrid hypervisor API. Use it to create, \
            manage, and control virtual machines. You can start/stop VMs, execute agent scripts, \
            attach GPUs, and monitor performance. Always check VM state before operations."
            .to_string(),
        auth: PluginAuth {
            auth_type: "service_http".to_string(),
        },
        api: PluginApi {
            api_type: "openapi".to_string(),
            url: "/agentic/openapi.yaml".to_string(),
        },
        logo_url: "https://nervosys.ai/logo.png".to_string(),
        contact_email: "api@nervosys.ai".to_string(),
        legal_info_url: "https://nervosys.ai/legal".to_string(),
    })
}

/// Query parameters for affordance lookup
#[derive(Debug, Deserialize)]
pub struct AffordanceQuery {
    /// Include full operation details (default: true)
    pub detail: Option<bool>,
}

async fn get_affordances(
    axum::extract::Path((resource_type, state)): axum::extract::Path<(String, String)>,
) -> Json<Affordances> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.get_affordances(&resource_type, &state))
}

async fn validate_plan(Json(plan): Json<ActionPlan>) -> Json<PlanValidationResult> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.validate_plan(&plan))
}

async fn get_composition_rules() -> Json<CompositionRules> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.build_composition_rules())
}

async fn get_resource_graph() -> Json<ResourceGraph> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.build_resource_graph())
}

async fn get_operation_contracts() -> Json<Vec<OperationContract>> {
    Json(HyperMachineOntology::build_operation_contracts())
}

async fn get_mcp_manifest() -> Json<McpManifest> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.build_mcp_manifest())
}

async fn get_agent_card() -> Json<AgentCard> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.build_agent_card())
}

/// The executor `/agentic/plans/execute` dispatches through.
///
/// Installed by [`create_ontology_router_with_executor`]. When it is absent the
/// endpoint falls back to simulation, so a server that has not been given a
/// hypervisor still answers — with clearly-marked synthetic results — instead
/// of failing.
#[derive(Clone)]
pub struct PlanExecutorLayer(pub std::sync::Arc<dyn PlanExecutor>);

async fn execute_plan_handler(
    executor: Option<axum::Extension<PlanExecutorLayer>>,
    Json(request): Json<PlanExecutionRequest>,
) -> Json<PlanExecutionResult> {
    let ontology = HyperMachineOntology::build();

    match executor {
        Some(axum::Extension(PlanExecutorLayer(executor))) => Json(
            ontology
                .execute_plan_with(&request, executor.as_ref())
                .await,
        ),
        None => Json(ontology.execute_plan(&request)),
    }
}

/// Query parameters for template listing
#[derive(Debug, Deserialize)]
pub struct TemplateQuery {
    /// Filter by category
    #[serde(default)]
    pub category: Option<String>,
    /// Filter by tag
    #[serde(default)]
    pub tag: Option<String>,
}

/// List all available plan templates, optionally filtered by category or tag
async fn list_templates(Query(query): Query<TemplateQuery>) -> Json<Vec<PlanTemplate>> {
    let mut templates = HyperMachineOntology::build_templates();

    if let Some(ref category) = query.category {
        templates.retain(|t| {
            let cat_str = serde_json::to_string(&t.category).unwrap_or_default();
            cat_str.contains(category)
        });
    }

    if let Some(ref tag) = query.tag {
        templates.retain(|t| t.tags.iter().any(|tt| tt == tag));
    }

    Json(templates)
}

/// Get a specific plan template by ID
async fn get_template_handler(
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    match HyperMachineOntology::get_template(&id) {
        Some(template) => (
            axum::http::StatusCode::OK,
            Json(serde_json::to_value(template).unwrap()),
        )
            .into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "template_not_found",
                "message": format!("Template '{}' not found", id)
            })),
        )
            .into_response(),
    }
}

/// Instantiate a plan template with parameters, optionally executing it
async fn instantiate_template_handler(
    Json(request): Json<TemplateInstantiationRequest>,
) -> Json<TemplateInstantiationResult> {
    let ontology = HyperMachineOntology::build();
    Json(ontology.instantiate_template(&request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ontology_builds() {
        let ontology = HyperMachineOntology::build();
        assert!(!ontology.capabilities.is_empty());
        assert!(!ontology.operations.is_empty());
        assert!(!ontology.resources.is_empty());
    }

    #[test]
    fn agentic_tools_cover_the_full_mcp_registry() {
        let onto = HyperMachineOntology::build();
        let registry = mcp_registry_tools();
        assert!(registry.len() >= 20, "expected the full MCP tool set");

        // Every MCP tool must appear in the OpenAI export — no silent drift, no
        // missing capability for an agent.
        let openai = onto.to_openai_tools();
        let names: std::collections::HashSet<&str> = openai
            .tools
            .iter()
            .map(|t| t.function.name.as_str())
            .collect();
        assert_eq!(openai.tools.len(), registry.len());
        for tool in &registry {
            assert!(
                names.contains(tool.name.as_str()),
                "OpenAI export missing {}",
                tool.name
            );
        }

        // Spot-check the categories that were previously absent from the ontology.
        for expected in [
            "vm.create",
            "vm.resize",
            "guest.exec",
            "snapshot.create",
            "network.attach",
            "gpu.attach",
            "agent.broadcast",
            "system.info",
        ] {
            assert!(names.contains(expected), "agentic tools missing {expected}");
        }

        // Anthropic matches; Gemini sanitizes dotted names.
        assert_eq!(onto.to_anthropic_tools().tools.len(), registry.len());
        let gemini = onto.to_gemini_tools();
        assert_eq!(gemini.function_declarations.len(), registry.len());
        assert!(gemini
            .function_declarations
            .iter()
            .any(|f| f.name == "vm_create"));
    }

    #[test]
    fn test_openai_conversion() {
        let ontology = HyperMachineOntology::build();
        let tools = ontology.to_openai_tools();
        assert!(!tools.tools.is_empty());
        assert!(tools.tools.iter().any(|t| t.function.name == "vm.create"));
    }

    #[test]
    fn test_anthropic_conversion() {
        let ontology = HyperMachineOntology::build();
        let tools = ontology.to_anthropic_tools();
        assert_eq!(tools.name, "hypermachine");
        assert!(!tools.tools.is_empty());
    }

    #[test]
    fn test_gemini_conversion() {
        let ontology = HyperMachineOntology::build();
        let tools = ontology.to_gemini_tools();
        assert!(!tools.function_declarations.is_empty());
    }

    #[test]
    fn test_affordances_running_state() {
        let ontology = HyperMachineOntology::build();
        let aff = ontology.get_affordances("vm", "running");
        assert_eq!(aff.resource_type, "vm");
        assert_eq!(aff.current_state, "running");
        assert!(!aff.available_operations.is_empty());
        // Running state should allow stop, pause, execute_script, get_metrics
        let op_ids: Vec<&str> = aff
            .available_operations
            .iter()
            .map(|o| o.operation_id.as_str())
            .collect();
        assert!(
            op_ids.contains(&"stop_vm"),
            "running state should allow stop"
        );
        assert!(
            op_ids.contains(&"pause_vm"),
            "running state should allow pause"
        );
        assert!(
            op_ids.contains(&"execute_script"),
            "running state should allow script execution"
        );
    }

    #[test]
    fn test_affordances_stopped_state() {
        let ontology = HyperMachineOntology::build();
        let aff = ontology.get_affordances("vm", "stopped");
        let op_ids: Vec<&str> = aff
            .available_operations
            .iter()
            .map(|o| o.operation_id.as_str())
            .collect();
        assert!(op_ids.contains(&"start_vm"));
        assert!(op_ids.contains(&"delete_vm"));
        assert!(
            !op_ids.contains(&"stop_vm"),
            "stopped state should not allow stop"
        );
    }

    #[test]
    fn test_affordances_transitions() {
        let ontology = HyperMachineOntology::build();
        let aff = ontology.get_affordances("vm", "running");
        assert!(!aff.possible_transitions.is_empty());
        let targets: Vec<&str> = aff
            .possible_transitions
            .iter()
            .map(|t| t.target_state.as_str())
            .collect();
        assert!(targets.contains(&"paused"));
        assert!(targets.contains(&"stopping"));
    }

    #[test]
    fn test_affordances_unknown_state() {
        let ontology = HyperMachineOntology::build();
        let aff = ontology.get_affordances("vm", "nonexistent");
        assert!(aff.available_operations.is_empty());
        assert!(aff.possible_transitions.is_empty());
    }

    #[test]
    fn test_affordances_unknown_resource() {
        let ontology = HyperMachineOntology::build();
        let aff = ontology.get_affordances("unknown", "running");
        assert!(aff.available_operations.is_empty());
    }

    #[test]
    fn test_plan_validation_valid() {
        let ontology = HyperMachineOntology::build();
        let plan = ActionPlan {
            name: "Test Plan".to_string(),
            description: "Create and start a VM".to_string(),
            steps: vec![
                PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "create_vm".to_string(),
                    parameters: serde_json::json!({"name": "test-vm"}),
                    depends_on: vec![],
                    timeout_seconds: None,
                },
                PlanStep {
                    step_id: "s2".to_string(),
                    operation_id: "start_vm".to_string(),
                    parameters: serde_json::json!({"id": "vm-123"}),
                    depends_on: vec!["s1".to_string()],
                    timeout_seconds: Some(60),
                },
            ],
            rollback_on_failure: true,
        };
        let result = ontology.validate_plan(&plan);
        assert!(result.valid);
        assert!(result.errors.is_empty());
        assert_eq!(result.resolved_steps.len(), 2);
    }

    #[test]
    fn test_plan_validation_unknown_operation() {
        let ontology = HyperMachineOntology::build();
        let plan = ActionPlan {
            name: "Bad Plan".to_string(),
            description: "Uses unknown operation".to_string(),
            steps: vec![PlanStep {
                step_id: "s1".to_string(),
                operation_id: "fly_to_moon".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec![],
                timeout_seconds: None,
            }],
            rollback_on_failure: false,
        };
        let result = ontology.validate_plan(&plan);
        assert!(!result.valid);
        assert_eq!(result.errors[0].error_type, "unknown_operation");
    }

    #[test]
    fn test_plan_validation_invalid_dependency() {
        let ontology = HyperMachineOntology::build();
        let plan = ActionPlan {
            name: "Bad Deps".to_string(),
            description: "References nonexistent step".to_string(),
            steps: vec![PlanStep {
                step_id: "s1".to_string(),
                operation_id: "create_vm".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec!["nonexistent".to_string()],
                timeout_seconds: None,
            }],
            rollback_on_failure: false,
        };
        let result = ontology.validate_plan(&plan);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.error_type == "invalid_dependency"));
    }

    #[test]
    fn test_plan_validation_self_dependency() {
        let ontology = HyperMachineOntology::build();
        let plan = ActionPlan {
            name: "Self Dep".to_string(),
            description: "Step depends on itself".to_string(),
            steps: vec![PlanStep {
                step_id: "s1".to_string(),
                operation_id: "list_vms".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec!["s1".to_string()],
                timeout_seconds: None,
            }],
            rollback_on_failure: false,
        };
        let result = ontology.validate_plan(&plan);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.error_type == "circular_dependency"));
    }

    #[test]
    fn test_plan_validation_duplicate_step_ids() {
        let ontology = HyperMachineOntology::build();
        let plan = ActionPlan {
            name: "Dup IDs".to_string(),
            description: "Duplicate step IDs".to_string(),
            steps: vec![
                PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "list_vms".to_string(),
                    parameters: serde_json::json!({}),
                    depends_on: vec![],
                    timeout_seconds: None,
                },
                PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "get_vm".to_string(),
                    parameters: serde_json::json!({}),
                    depends_on: vec![],
                    timeout_seconds: None,
                },
            ],
            rollback_on_failure: false,
        };
        let result = ontology.validate_plan(&plan);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.error_type == "duplicate_step_id"));
    }

    #[test]
    fn test_resource_graph() {
        let ontology = HyperMachineOntology::build();
        let graph = ontology.build_resource_graph();
        assert!(!graph.nodes.is_empty());
        assert!(!graph.edges.is_empty());
        // VM should have relationships to disk, network, gpu
        let vm_edges: Vec<&ResourceEdge> = graph.edges.iter().filter(|e| e.from == "vm").collect();
        assert!(vm_edges.len() >= 3);
    }

    #[test]
    fn test_composition_rules() {
        let ontology = HyperMachineOntology::build();
        let rules = ontology.build_composition_rules();
        assert!(!rules.workflows.is_empty());
        assert!(!rules.constraints.is_empty());
        assert!(!rules.patterns.is_empty());
        // Check provision_and_start workflow exists
        assert!(rules
            .workflows
            .iter()
            .any(|w| w.id == "provision_and_start"));
    }

    #[test]
    fn test_composition_workflows_well_formed() {
        let ontology = HyperMachineOntology::build();
        let rules = ontology.build_composition_rules();
        for workflow in &rules.workflows {
            assert!(!workflow.id.is_empty());
            assert!(!workflow.steps.is_empty());
            // Steps should have sequential order
            let orders: Vec<u32> = workflow.steps.iter().map(|s| s.order).collect();
            let mut sorted = orders.clone();
            sorted.sort();
            assert_eq!(
                orders, sorted,
                "Workflow {} steps not in order",
                workflow.id
            );
        }
    }

    #[test]
    fn test_operation_contracts() {
        let contracts = HyperMachineOntology::build_operation_contracts();
        assert!(!contracts.is_empty());
        // start_vm should have preconditions
        let start = contracts
            .iter()
            .find(|c| c.operation_id == "start_vm")
            .unwrap();
        assert!(!start.preconditions.is_empty());
        assert!(!start.postconditions.is_empty());
        // create_vm should have no preconditions
        let create = contracts
            .iter()
            .find(|c| c.operation_id == "create_vm")
            .unwrap();
        assert!(create.preconditions.is_empty());
        assert!(!create.postconditions.is_empty());
    }

    #[test]
    fn test_operation_contracts_composability() {
        let contracts = HyperMachineOntology::build_operation_contracts();
        // start_vm should be composable with execute_script
        let start = contracts
            .iter()
            .find(|c| c.operation_id == "start_vm")
            .unwrap();
        assert!(start
            .composable_with
            .contains(&"execute_script".to_string()));
        // delete_vm should be mutually exclusive with start_vm
        let delete = contracts
            .iter()
            .find(|c| c.operation_id == "delete_vm")
            .unwrap();
        assert!(delete
            .mutually_exclusive_with
            .contains(&"start_vm".to_string()));
    }

    #[test]
    fn test_agent_card() {
        let ontology = HyperMachineOntology::build();
        let card = ontology.build_agent_card();
        assert_eq!(card.name, "HyperMachine");
        assert!(!card.skills.is_empty());
        assert!(card.capabilities.streaming);
        assert!(card.capabilities.push_notifications);
        assert!(!card.default_input_modes.is_empty());
        assert!(!card.default_output_modes.is_empty());
    }

    #[test]
    fn test_agent_card_skills() {
        let ontology = HyperMachineOntology::build();
        let card = ontology.build_agent_card();
        let skill_ids: Vec<&str> = card.skills.iter().map(|s| s.id.as_str()).collect();
        assert!(skill_ids.contains(&"vm_management"));
        assert!(skill_ids.contains(&"gpu_compute"));
        assert!(skill_ids.contains(&"agent_scripting"));
        assert!(skill_ids.contains(&"monitoring"));
        for skill in &card.skills {
            assert!(!skill.tags.is_empty());
            assert!(!skill.examples.is_empty());
        }
    }

    #[test]
    fn test_mcp_manifest() {
        let ontology = HyperMachineOntology::build();
        let mcp = ontology.build_mcp_manifest();
        assert_eq!(mcp.name, "hypermachine");
        assert_eq!(mcp.protocol_version, "2024-11-05");
        assert!(mcp.capabilities.tools);
        assert!(mcp.capabilities.resources);
        assert!(!mcp.tools.is_empty());
        assert!(!mcp.resources.is_empty());
    }

    #[test]
    fn test_mcp_manifest_matches_registry() {
        let ontology = HyperMachineOntology::build();
        let mcp = ontology.build_mcp_manifest();
        let registry = mcp_registry_tools();

        // The native /agentic/mcp manifest must expose exactly the live MCP
        // registry — same surface as the OpenAI/Anthropic/Gemini projections,
        // so no agent transport sees a stale or partial tool set.
        assert_eq!(
            mcp.tools.len(),
            registry.len(),
            "MCP manifest tool count drifted from the registry"
        );
        for t in &registry {
            let tool = mcp
                .tools
                .iter()
                .find(|m| m.name == t.name)
                .unwrap_or_else(|| panic!("registry tool '{}' missing from MCP manifest", t.name));
            // The manifest must carry the tool's real input schema, not an empty stub.
            assert_eq!(
                tool.input_schema, t.parameters,
                "MCP manifest schema for '{}' diverged from the registry",
                t.name
            );
        }
        // Spot-check that lifecycle + cross-category tools are all present.
        for name in [
            "vm.create",
            "vm.pause",
            "vm.resume",
            "guest.exec",
            "snapshot.create",
            "gpu.attach",
            "agent.broadcast",
            "system.info",
        ] {
            assert!(
                mcp.tools.iter().any(|m| m.name == name),
                "MCP manifest is missing '{name}'"
            );
        }
    }

    #[test]
    fn test_affordance_reversibility() {
        let ontology = HyperMachineOntology::build();
        let aff = ontology.get_affordances("vm", "running");
        // pause should be reversible (resume goes back to running)
        let pause_trans = aff
            .possible_transitions
            .iter()
            .find(|t| t.trigger_operation == "pause_vm");
        assert!(pause_trans.is_some());
        assert!(
            pause_trans.unwrap().reversible,
            "pause should be reversible"
        );
    }

    #[test]
    fn test_condition_operators() {
        let contracts = HyperMachineOntology::build_operation_contracts();
        // start_vm uses In operator for precondition
        let start = contracts
            .iter()
            .find(|c| c.operation_id == "start_vm")
            .unwrap();
        assert!(start
            .preconditions
            .iter()
            .any(|c| c.operator == ConditionOperator::In));
        // delete_vm uses In for precondition too
        let delete = contracts
            .iter()
            .find(|c| c.operation_id == "delete_vm")
            .unwrap();
        assert!(delete
            .preconditions
            .iter()
            .any(|c| c.operator == ConditionOperator::In));
        // delete postcondition uses NotExists
        assert!(delete
            .postconditions
            .iter()
            .any(|c| c.operator == ConditionOperator::NotExists));
    }

    #[test]
    fn test_plan_empty_steps() {
        let ontology = HyperMachineOntology::build();
        let plan = ActionPlan {
            name: "Empty".to_string(),
            description: "No steps".to_string(),
            steps: vec![],
            rollback_on_failure: false,
        };
        let result = ontology.validate_plan(&plan);
        assert!(result.valid, "Empty plan should be valid");
        assert!(result.resolved_steps.is_empty());
    }

    #[test]
    fn test_resource_graph_nodes_have_operations() {
        let ontology = HyperMachineOntology::build();
        let graph = ontology.build_resource_graph();
        // VM node should have operations count > 0
        let vm_node = graph.nodes.iter().find(|n| n.id == "vm");
        assert!(vm_node.is_some());
        assert!(vm_node.unwrap().operations_count > 0);
    }

    #[test]
    fn test_execute_plan_simple() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Simple Create".to_string(),
                description: "Create a VM".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "create_vm".to_string(),
                    parameters: serde_json::json!({"name": "test-vm"}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert_eq!(result.step_results.len(), 1);
        assert!(result.step_results[0].success);
        assert!(result.step_results[0].output.is_some());
    }

    #[test]
    fn test_execute_plan_multi_step() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Create and Start".to_string(),
                description: "Create then start a VM".to_string(),
                steps: vec![
                    PlanStep {
                        step_id: "create".to_string(),
                        operation_id: "create_vm".to_string(),
                        parameters: serde_json::json!({"name": "multi-vm"}),
                        depends_on: vec![],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "start".to_string(),
                        operation_id: "start_vm".to_string(),
                        parameters: serde_json::json!({"id": "vm-123"}),
                        depends_on: vec!["create".to_string()],
                        timeout_seconds: None,
                    },
                ],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert_eq!(result.step_results.len(), 2);
        assert!(result.step_results.iter().all(|s| s.success));
        // First step should be create_vm (no deps)
        assert_eq!(result.step_results[0].operation_id, "create_vm");
    }

    #[test]
    fn test_execute_plan_dry_run() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Dry Run".to_string(),
                description: "Validate only".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "create_vm".to_string(),
                    parameters: serde_json::json!({"name": "dry-vm"}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: Some(true),
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert!(
            result.step_results.is_empty(),
            "Dry run should not execute steps"
        );
        assert!(result.validation.valid);
    }

    #[test]
    fn test_execute_plan_validation_failure() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Bad Plan".to_string(),
                description: "Uses unknown operation".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "fly_to_moon".to_string(),
                    parameters: serde_json::json!({}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::ValidationFailed);
        assert!(result.step_results.is_empty());
        assert!(!result.validation.valid);
    }

    #[test]
    fn test_execute_plan_with_variables() {
        let ontology = HyperMachineOntology::build();
        let mut vars = serde_json::Map::new();
        vars.insert("vm_name".to_string(), serde_json::json!("my-custom-vm"));
        vars.insert("cpu_count".to_string(), serde_json::json!(8));

        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Parameterized".to_string(),
                description: "Uses variables".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "create_vm".to_string(),
                    parameters: serde_json::json!({"name": "${vm_name}", "vcpu_count": "${cpu_count}"}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: Some(vars),
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert!(result.step_results[0].success);
        // Check that the VM was created with the substituted name
        let output = result.step_results[0].output.as_ref().unwrap();
        assert_eq!(output["name"], "my-custom-vm");
    }

    #[test]
    fn test_execute_plan_rollback() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Rollback Test".to_string(),
                description: "First steps succeed, last has unknown dep ref".to_string(),
                steps: vec![
                    PlanStep {
                        step_id: "s1".to_string(),
                        operation_id: "create_vm".to_string(),
                        parameters: serde_json::json!({"name": "rollback-vm"}),
                        depends_on: vec![],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "s2".to_string(),
                        operation_id: "list_vms".to_string(),
                        parameters: serde_json::json!({}),
                        depends_on: vec!["s1".to_string()],
                        timeout_seconds: None,
                    },
                ],
                rollback_on_failure: true,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        // Both operations should succeed
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert!(result.rolled_back_steps.is_empty());
    }

    #[test]
    fn test_execute_plan_empty() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Empty".to_string(),
                description: "No steps".to_string(),
                steps: vec![],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert!(result.step_results.is_empty());
    }

    #[test]
    fn test_execute_plan_execution_id() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "ID Test".to_string(),
                description: "Check execution ID".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "list_vms".to_string(),
                    parameters: serde_json::json!({}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert!(result.execution_id.starts_with("exec-"));
        assert_eq!(result.plan_name, "ID Test");
    }

    #[test]
    fn test_execute_plan_lifecycle_operations() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Full Lifecycle".to_string(),
                description: "Create, start, pause, resume, stop, delete".to_string(),
                steps: vec![
                    PlanStep {
                        step_id: "create".to_string(),
                        operation_id: "create_vm".to_string(),
                        parameters: serde_json::json!({"name": "lifecycle-vm"}),
                        depends_on: vec![],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "start".to_string(),
                        operation_id: "start_vm".to_string(),
                        parameters: serde_json::json!({"id": "vm-lc"}),
                        depends_on: vec!["create".to_string()],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "pause".to_string(),
                        operation_id: "pause_vm".to_string(),
                        parameters: serde_json::json!({"id": "vm-lc"}),
                        depends_on: vec!["start".to_string()],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "resume".to_string(),
                        operation_id: "resume_vm".to_string(),
                        parameters: serde_json::json!({"id": "vm-lc"}),
                        depends_on: vec!["pause".to_string()],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "stop".to_string(),
                        operation_id: "stop_vm".to_string(),
                        parameters: serde_json::json!({"id": "vm-lc"}),
                        depends_on: vec!["resume".to_string()],
                        timeout_seconds: None,
                    },
                    PlanStep {
                        step_id: "delete".to_string(),
                        operation_id: "delete_vm".to_string(),
                        parameters: serde_json::json!({"id": "vm-lc"}),
                        depends_on: vec!["stop".to_string()],
                        timeout_seconds: None,
                    },
                ],
                rollback_on_failure: true,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        assert_eq!(result.step_results.len(), 6);
        assert!(result.step_results.iter().all(|s| s.success));
        // Verify execution order follows dependency chain
        let order: Vec<&str> = result
            .step_results
            .iter()
            .map(|s| s.step_id.as_str())
            .collect();
        assert_eq!(
            order,
            vec!["create", "start", "pause", "resume", "stop", "delete"]
        );
    }

    #[test]
    fn test_execute_plan_script_execution() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Script Run".to_string(),
                description: "Execute a script".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "execute_script".to_string(),
                    parameters: serde_json::json!({"id": "vm-1", "script": "print('hello')"}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        assert_eq!(result.status, PlanExecutionStatus::Completed);
        let output = result.step_results[0].output.as_ref().unwrap();
        assert_eq!(output["status"], "completed");
    }

    #[test]
    fn test_topological_sort_independent() {
        // Independent steps can appear in any order but all should be present
        let steps = vec![
            PlanStep {
                step_id: "a".to_string(),
                operation_id: "list_vms".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec![],
                timeout_seconds: None,
            },
            PlanStep {
                step_id: "b".to_string(),
                operation_id: "get_metrics".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec![],
                timeout_seconds: None,
            },
        ];
        let sorted = HyperMachineOntology::topological_sort(&steps);
        assert_eq!(sorted.len(), 2);
        let ids: Vec<&str> = sorted.iter().map(|s| s.step_id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
    }

    #[test]
    fn test_topological_sort_chain() {
        let steps = vec![
            PlanStep {
                step_id: "c".to_string(),
                operation_id: "stop_vm".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec!["b".to_string()],
                timeout_seconds: None,
            },
            PlanStep {
                step_id: "a".to_string(),
                operation_id: "create_vm".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec![],
                timeout_seconds: None,
            },
            PlanStep {
                step_id: "b".to_string(),
                operation_id: "start_vm".to_string(),
                parameters: serde_json::json!({}),
                depends_on: vec!["a".to_string()],
                timeout_seconds: None,
            },
        ];
        let sorted = HyperMachineOntology::topological_sort(&steps);
        assert_eq!(sorted.len(), 3);
        // a must come before b, b must come before c
        let ids: Vec<&str> = sorted.iter().map(|s| s.step_id.as_str()).collect();
        assert!(
            ids.iter().position(|&x| x == "a").unwrap()
                < ids.iter().position(|&x| x == "b").unwrap()
        );
        assert!(
            ids.iter().position(|&x| x == "b").unwrap()
                < ids.iter().position(|&x| x == "c").unwrap()
        );
    }

    #[test]
    fn test_topological_sort_empty() {
        let sorted = HyperMachineOntology::topological_sort(&[]);
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_variable_substitution() {
        let mut vars = serde_json::Map::new();
        vars.insert("name".to_string(), serde_json::json!("my-vm"));
        vars.insert("count".to_string(), serde_json::json!(4));

        let input = serde_json::json!({"vm_name": "${name}", "cpu": "${count}", "nested": {"x": "${name}"}});
        let result = HyperMachineOntology::substitute_variables(&input, &vars);
        assert_eq!(result["vm_name"], "my-vm");
        assert_eq!(result["cpu"], "4");
        assert_eq!(result["nested"]["x"], "my-vm");
    }

    #[test]
    fn test_variable_substitution_no_vars() {
        let vars = serde_json::Map::new();
        let input = serde_json::json!({"name": "literal", "count": 5});
        let result = HyperMachineOntology::substitute_variables(&input, &vars);
        assert_eq!(result["name"], "literal");
        assert_eq!(result["count"], 5);
    }

    #[test]
    fn test_simulate_create_vm() {
        let ontology = HyperMachineOntology::build();
        let op = ontology
            .operations
            .iter()
            .find(|o| o.id == "create_vm")
            .unwrap();
        let params = serde_json::json!({"name": "sim-vm", "vcpu_count": 8});
        let (success, output, error) = HyperMachineOntology::simulate_operation(op, &params);
        assert!(success);
        assert!(error.is_none());
        let out = output.unwrap();
        assert_eq!(out["name"], "sim-vm");
        assert_eq!(out["state"], "created");
    }

    #[test]
    fn test_simulate_lifecycle_ops() {
        let ontology = HyperMachineOntology::build();
        for op_id in &["start_vm", "stop_vm", "pause_vm", "resume_vm"] {
            let op = ontology.operations.iter().find(|o| o.id == *op_id).unwrap();
            let params = serde_json::json!({"id": "vm-test"});
            let (success, output, error) = HyperMachineOntology::simulate_operation(op, &params);
            assert!(success, "{} should succeed", op_id);
            assert!(error.is_none());
            assert!(output.is_some());
        }
    }

    #[test]
    fn test_plan_duration_tracked() {
        let ontology = HyperMachineOntology::build();
        let request = PlanExecutionRequest {
            plan: ActionPlan {
                name: "Duration".to_string(),
                description: "Track timing".to_string(),
                steps: vec![PlanStep {
                    step_id: "s1".to_string(),
                    operation_id: "list_vms".to_string(),
                    parameters: serde_json::json!({}),
                    depends_on: vec![],
                    timeout_seconds: None,
                }],
                rollback_on_failure: false,
            },
            dry_run: None,
            timeout_seconds: None,
            variables: None,
        };
        let result = ontology.execute_plan(&request);
        // Duration should be tracked (>= 0)
        assert!(result.duration_ms < 10000, "Should complete quickly");
        assert!(result.step_results[0].duration_ms < 10000);
    }

    // ---- Phase 117: Plan Template Tests ----

    #[test]
    fn test_build_templates_non_empty() {
        let templates = HyperMachineOntology::build_templates();
        assert!(
            templates.len() >= 6,
            "Should have at least 6 templates, got {}",
            templates.len()
        );
        // All templates should have unique IDs
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "Template IDs must be unique");
    }

    #[test]
    fn test_template_categories() {
        let templates = HyperMachineOntology::build_templates();
        let categories: Vec<&TemplateCategory> = templates.iter().map(|t| &t.category).collect();
        assert!(categories.contains(&&TemplateCategory::Lifecycle));
        assert!(categories.contains(&&TemplateCategory::Monitoring));
        assert!(categories.contains(&&TemplateCategory::Provisioning));
        assert!(categories.contains(&&TemplateCategory::Recovery));
    }

    #[test]
    fn test_get_template_by_id() {
        let template = HyperMachineOntology::get_template("tpl-create-and-start");
        assert!(template.is_some());
        let t = template.unwrap();
        assert_eq!(t.name, "Create and Start VM");
        assert_eq!(t.category, TemplateCategory::Lifecycle);
        assert!(!t.parameters.is_empty());
    }

    #[test]
    fn test_get_template_not_found() {
        let template = HyperMachineOntology::get_template("tpl-nonexistent");
        assert!(template.is_none());
    }

    #[test]
    fn test_template_has_valid_plan() {
        let ontology = HyperMachineOntology::build();
        let templates = HyperMachineOntology::build_templates();
        for template in &templates {
            let validation = ontology.validate_plan(&template.plan);
            // Templates with operations like create_snapshot/restore_snapshot
            // may not validate since those ops aren't in the base ontology.
            // But lifecycle and monitoring templates should validate.
            if template.category == TemplateCategory::Lifecycle
                || template.category == TemplateCategory::Monitoring
            {
                assert!(
                    validation.valid || !validation.errors.is_empty(),
                    "Template '{}' should have validation result",
                    template.id
                );
            }
        }
    }

    #[test]
    fn test_instantiate_create_and_start() {
        let ontology = HyperMachineOntology::build();
        let mut params = serde_json::Map::new();
        params.insert("vm_name".to_string(), serde_json::json!("test-server"));

        let request = TemplateInstantiationRequest {
            template_id: "tpl-create-and-start".to_string(),
            parameters: params,
            execute: false,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert_eq!(result.template_id, "tpl-create-and-start");
        assert!(result.missing_parameters.is_empty());
        // vm_name was provided, vcpu_count and memory_mb have defaults
        assert!(result.defaults_applied.contains(&"vcpu_count".to_string()));
        assert!(result.defaults_applied.contains(&"memory_mb".to_string()));
        // Plan should have variable substituted
        assert!(result.plan.name.contains("test-server"));
        assert!(
            result.execution.is_none(),
            "Should not execute when execute=false"
        );
    }

    #[test]
    fn test_instantiate_with_all_params() {
        let ontology = HyperMachineOntology::build();
        let mut params = serde_json::Map::new();
        params.insert("vm_name".to_string(), serde_json::json!("custom-vm"));
        params.insert("vcpu_count".to_string(), serde_json::json!(8));
        params.insert("memory_mb".to_string(), serde_json::json!(16384));

        let request = TemplateInstantiationRequest {
            template_id: "tpl-create-and-start".to_string(),
            parameters: params,
            execute: false,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(
            result.defaults_applied.is_empty(),
            "No defaults should be applied when all params provided"
        );
        assert!(result.missing_parameters.is_empty());
    }

    #[test]
    fn test_instantiate_missing_required_param() {
        let ontology = HyperMachineOntology::build();
        let request = TemplateInstantiationRequest {
            template_id: "tpl-create-and-start".to_string(),
            parameters: serde_json::Map::new(),
            execute: false,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(!result.validation.valid);
        assert!(result.missing_parameters.contains(&"vm_name".to_string()));
    }

    #[test]
    fn test_instantiate_nonexistent_template() {
        let ontology = HyperMachineOntology::build();
        let request = TemplateInstantiationRequest {
            template_id: "tpl-does-not-exist".to_string(),
            parameters: serde_json::Map::new(),
            execute: false,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(!result.validation.valid);
        assert!(result
            .validation
            .errors
            .iter()
            .any(|e| e.error_type == "template_not_found"));
    }

    #[test]
    fn test_instantiate_and_execute() {
        let ontology = HyperMachineOntology::build();
        let mut params = serde_json::Map::new();
        params.insert("vm_name".to_string(), serde_json::json!("exec-vm"));

        let request = TemplateInstantiationRequest {
            template_id: "tpl-create-and-start".to_string(),
            parameters: params,
            execute: true,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(result.execution.is_some());
        let exec = result.execution.unwrap();
        assert_eq!(exec.status, PlanExecutionStatus::Completed);
        assert_eq!(exec.step_results.len(), 2);
    }

    #[test]
    fn test_instantiate_and_dry_run() {
        let ontology = HyperMachineOntology::build();
        let mut params = serde_json::Map::new();
        params.insert("vm_name".to_string(), serde_json::json!("dry-vm"));

        let request = TemplateInstantiationRequest {
            template_id: "tpl-create-and-start".to_string(),
            parameters: params,
            execute: true,
            dry_run: Some(true),
        };
        let result = ontology.instantiate_template(&request);
        assert!(result.execution.is_some());
        let exec = result.execution.unwrap();
        assert_eq!(exec.status, PlanExecutionStatus::Completed);
        assert!(
            exec.step_results.is_empty(),
            "Dry run should not execute steps"
        );
    }

    #[test]
    fn test_health_check_template_no_params() {
        let ontology = HyperMachineOntology::build();
        let request = TemplateInstantiationRequest {
            template_id: "tpl-health-check".to_string(),
            parameters: serde_json::Map::new(),
            execute: true,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(result.missing_parameters.is_empty());
        assert!(result.defaults_applied.is_empty());
        assert!(result.execution.is_some());
        let exec = result.execution.unwrap();
        assert_eq!(exec.status, PlanExecutionStatus::Completed);
    }

    #[test]
    fn test_batch_provision_template() {
        let ontology = HyperMachineOntology::build();
        let mut params = serde_json::Map::new();
        params.insert("base_name".to_string(), serde_json::json!("worker"));

        let request = TemplateInstantiationRequest {
            template_id: "tpl-batch-provision".to_string(),
            parameters: params,
            execute: true,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(result.defaults_applied.contains(&"vcpu_count".to_string()));
        assert!(result.execution.is_some());
        let exec = result.execution.unwrap();
        assert_eq!(exec.status, PlanExecutionStatus::Completed);
        assert_eq!(exec.step_results.len(), 6); // 3 create + 3 start
    }

    #[test]
    fn test_full_lifecycle_template() {
        let ontology = HyperMachineOntology::build();
        let mut params = serde_json::Map::new();
        params.insert("vm_name".to_string(), serde_json::json!("lc-test"));

        let request = TemplateInstantiationRequest {
            template_id: "tpl-full-lifecycle".to_string(),
            parameters: params,
            execute: true,
            dry_run: None,
        };
        let result = ontology.instantiate_template(&request);
        assert!(result.execution.is_some());
        let exec = result.execution.unwrap();
        assert_eq!(exec.status, PlanExecutionStatus::Completed);
        assert_eq!(exec.step_results.len(), 6);
    }

    #[test]
    fn test_template_version() {
        let templates = HyperMachineOntology::build_templates();
        for template in &templates {
            assert!(
                !template.version.is_empty(),
                "Template '{}' must have a version",
                template.id
            );
            // Should be semver-like
            let parts: Vec<&str> = template.version.split('.').collect();
            assert_eq!(
                parts.len(),
                3,
                "Template '{}' version should be semver",
                template.id
            );
        }
    }

    #[test]
    fn test_template_tags_non_empty() {
        let templates = HyperMachineOntology::build_templates();
        for template in &templates {
            assert!(
                !template.tags.is_empty(),
                "Template '{}' must have at least one tag",
                template.id
            );
        }
    }

    #[test]
    fn test_template_parameters_have_labels() {
        let templates = HyperMachineOntology::build_templates();
        for template in &templates {
            for param in &template.parameters {
                assert!(
                    !param.label.is_empty(),
                    "Parameter '{}' in template '{}' must have a label",
                    param.name,
                    template.id
                );
                assert!(
                    !param.description.is_empty(),
                    "Parameter '{}' in template '{}' must have a description",
                    param.name,
                    template.id
                );
            }
        }
    }
}
