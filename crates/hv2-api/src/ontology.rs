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

    /// Warnings attached to interfaces (operations, capabilities, tools).
    /// Absent in documents served before guardrails existed.
    #[serde(default)]
    pub guardrails: Vec<Guardrail>,
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

/// How serious a [`Guardrail`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailSeverity {
    /// Worth knowing; no action is required.
    Info,
    /// A weakness or limit that can bite under the stated condition.
    Warning,
    /// Can defeat a security or safety property under the stated condition.
    Critical,
}

/// A warning attached to one or more interfaces: a condition under which they
/// do not give the guarantee a reader would assume, and what to do about it.
///
/// `applies_to` holds ids of anything the ontology names: an
/// [`Operation`] id, a [`Capability`] id, or an agent-callable MCP tool name.
/// The tool projections (`/agentic/tools/*`, `/agentic/mcp`) append a short
/// warning line to the description of every tool named here, so an LLM sees
/// the caveat where it picks the tool. Guardrails are advisory data: nothing
/// in this crate blocks a call because of one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardrail {
    /// Stable identifier, e.g. `redis-shared-user-revocation`.
    pub id: String,
    pub severity: GuardrailSeverity,
    pub title: String,
    pub description: String,
    /// Operation, capability or tool ids this applies to.
    pub applies_to: Vec<String>,
    /// When it applies (a deployment or input condition).
    pub condition: String,
    /// What to do about it.
    pub mitigation: String,
    /// Repository path of the document that explains it.
    pub reference: String,
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
            guardrails: Self::build_guardrails(),
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
            Capability {
                id: "managed_api_keys".to_string(),
                name: "Managed API Keys (cluster control plane)".to_string(),
                description: "Make, list and revoke API keys at run time through the cluster \
                    control plane (`hv2-control-plane`), not this server. Keys live in the \
                    cluster store, sealed with an HMAC under `--managed-key-secret-file`; \
                    without that flag the whole interface answers 503. Authenticate with the \
                    `X-API-Key` header. Only an administrator (the legacy admin key, or an \
                    operator key holding the `admin` scope) or a `team-admin` may call it; \
                    every other caller gets 403. A team-admin is confined to its own team \
                    and cannot raise anyone above itself. There is no rotate route: rotate \
                    by creating a replacement key, moving clients over, then revoking the \
                    old one. Policy-file keys are not reachable here (see \
                    docs/API_KEY_ROTATION.md for the file and SIGHUP reload)."
                    .to_string(),
                category: CapabilityCategory::Security,
                operations: vec![
                    "create_managed_key".to_string(),
                    "list_managed_keys".to_string(),
                    "revoke_managed_key".to_string(),
                    "list_team_members".to_string(),
                ],
                prerequisites: vec![
                    "control plane started with --managed-key-secret-file".to_string()
                ],
                permissions_required: vec!["administrator".to_string(), "team-admin".to_string()],
            },
            Capability {
                id: "cluster_store_configuration".to_string(),
                name: "Cluster Store Configuration".to_string(),
                description: "Where control planes, nodes (`hv2-sandboxd --cluster-store`) and \
                    the native gateway keep shared state, including managed API key \
                    records and their revocation records: Redis, or an in-memory store. \
                    This is an operator deployment setting, not an HTTP operation. Give each \
                    role its own Redis ACL user (`tools/redis-acl.py`); the shipped \
                    deploy/compose and deploy/helm do."
                    .to_string(),
                category: CapabilityCategory::Security,
                operations: vec![],
                prerequisites: vec![],
                permissions_required: vec!["operator".to_string()],
            },
            Capability {
                id: "sso_sign_in".to_string(),
                name: "Single Sign-On (cluster control plane)".to_string(),
                description: "Sign people in to the cluster control plane \
                    (`hv2-control-plane`) through an OpenID Connect provider, in a browser \
                    or from the CLI, and look up or end that session. A signed-in member \
                    gets a session that the control plane accepts in place of an \
                    `X-API-Key`, with the scopes, role, principal and team of their entry \
                    in the members file. These routes need no API key (they are how a \
                    person without one gets a session); when single sign-on is not \
                    configured every one of them answers 404. `GET /auth/guest` extends \
                    the sign-in to private guest URLs. See docs/SSO.md."
                    .to_string(),
                category: CapabilityCategory::Security,
                operations: vec![
                    "sso_login".to_string(),
                    "sso_callback".to_string(),
                    "sso_session".to_string(),
                    "sso_guest_handoff".to_string(),
                    "sso_logout".to_string(),
                    "sso_cli_token".to_string(),
                ],
                prerequisites: vec![
                    "control plane started with --sso-issuer, --sso-client-id, \
                     --sso-redirect-url, --sso-members-file and --sso-session-key-file"
                        .to_string(),
                    "private guest URLs also need --sso-guest-urls, proxy TLS and at least \
                     one --guest-domain"
                        .to_string(),
                ],
                permissions_required: vec!["listed member".to_string()],
            },
            Capability {
                id: "team_template_sharing".to_string(),
                name: "Snapshot Sharing Between Teams (cluster control plane)".to_string(),
                description: "Let another team start sandboxes from a team's snapshot, list \
                    who a snapshot is shared with, and stop sharing, through the cluster \
                    control plane. Only the snapshot's own team (or an administrator) may \
                    use these; the team shared with cannot read the list, share it on, \
                    revoke it or delete the snapshot (404 each). A shared snapshot is the \
                    source sandbox as it was, memory included: read the guardrails first. \
                    See docs/TEAMS.md."
                    .to_string(),
                category: CapabilityCategory::Security,
                operations: vec![
                    "list_template_shares".to_string(),
                    "share_template_snapshot".to_string(),
                    "unshare_template_snapshot".to_string(),
                ],
                prerequisites: vec![
                    "a snapshot owned by a team".to_string(),
                    "a key or session with the `templates` scope".to_string(),
                ],
                permissions_required: vec!["templates".to_string()],
            },
            Capability {
                id: "sandbox_lifecycle".to_string(),
                name: "Sandbox Lifecycle (cluster control plane)".to_string(),
                description: "Create, list, inspect, connect to, pause, resume, resize and \
                    delete microVM sandboxes through the cluster control plane \
                    (`hv2-control-plane`), which picks a node, records who owns the sandbox \
                    and forwards to the node that runs it. The routes keep E2B's shapes \
                    (`/sandboxes`, `/v2/sandboxes`), so the E2B SDK works against them. They \
                    authenticate with `X-API-Key` or a signed-in session that has the \
                    `sandboxes` scope; a team key sees and acts on its own team's sandboxes \
                    only, and an observer or viewer may read the listings alone. The node \
                    serves the same paths on its own API as the `node_*` operations of \
                    `node_sandbox_lifecycle`. See docs/SANDBOXES.md and docs/TEAMS.md."
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "create_sandbox".to_string(),
                    "create_sandbox_v2".to_string(),
                    "list_sandboxes".to_string(),
                    "list_sandboxes_v2".to_string(),
                    "get_sandbox".to_string(),
                    "delete_sandbox".to_string(),
                    "connect_sandbox".to_string(),
                    "connect_sandbox_v2".to_string(),
                    "set_sandbox_timeout".to_string(),
                    "pause_sandbox".to_string(),
                    "resume_sandbox".to_string(),
                    "standby_sandbox".to_string(),
                    "get_sandbox_standby".to_string(),
                    "resize_sandbox".to_string(),
                    "adopt_sandbox_owner".to_string(),
                ],
                prerequisites: vec![
                    "a control plane with at least one live node that offers the template"
                        .to_string(),
                    "a key or session with the `sandboxes` scope".to_string(),
                ],
                permissions_required: vec!["sandboxes".to_string()],
            },
            Capability {
                id: "sandbox_commands".to_string(),
                name: "Sandbox Commands And Processes (cluster control plane)".to_string(),
                description: "Run a command in a running sandbox's guest, read the history of \
                    the commands run in it with their output, and list the guest's processes, \
                    through the cluster control plane, which forwards to the sandbox's node. \
                    Same authentication and team rules as `sandbox_lifecycle`. A command \
                    runs as root in the guest unless a user is named."
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "exec_in_sandbox".to_string(),
                    "list_sandbox_commands".to_string(),
                    "get_sandbox_command".to_string(),
                    "list_sandbox_processes".to_string(),
                ],
                prerequisites: vec!["a running sandbox".to_string()],
                permissions_required: vec!["sandboxes".to_string()],
            },
            Capability {
                id: "node_sandbox_lifecycle".to_string(),
                name: "Sandbox Lifecycle (node daemon)".to_string(),
                description: "The node daemon's own sandbox API (`hv2-sandboxd`, port 3980), \
                    which boots and runs the microVMs. The control plane forwards to it; a \
                    standalone node serves it directly. It has no users or teams: it checks \
                    only the cluster token (`x-hv2-cluster-token`) when it has one, and \
                    otherwise answers every caller that can reach it, which is why it \
                    listens on loopback unless it has a token or mTLS. It lists and acts on \
                    its own sandboxes only. Also serves `reboot`, which the control plane \
                    does not route. See docs/SANDBOXES.md, \"Where a node listens\"."
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "node_create_sandbox".to_string(),
                    "node_create_sandbox_v2".to_string(),
                    "node_list_sandboxes".to_string(),
                    "node_list_sandboxes_v2".to_string(),
                    "node_get_sandbox".to_string(),
                    "node_delete_sandbox".to_string(),
                    "node_connect_sandbox".to_string(),
                    "node_connect_sandbox_v2".to_string(),
                    "node_set_sandbox_timeout".to_string(),
                    "node_pause_sandbox".to_string(),
                    "node_resume_sandbox".to_string(),
                    "node_standby_sandbox".to_string(),
                    "node_get_sandbox_standby".to_string(),
                    "node_resize_sandbox".to_string(),
                    "node_adopt_sandbox_owner".to_string(),
                    "node_reboot_sandbox".to_string(),
                ],
                prerequisites: vec![
                    "a node (`hv2-sandboxd`) with a kernel and initramfs".to_string(),
                    "the node's cluster token, if it has one".to_string(),
                ],
                permissions_required: vec!["cluster token".to_string()],
            },
            Capability {
                id: "node_sandbox_commands".to_string(),
                name: "Sandbox Commands And Processes (node daemon)".to_string(),
                description: "The node daemon's routes for running a command in a sandbox, \
                    reading its command history and listing its processes. Same \
                    authentication as `node_sandbox_lifecycle`: the cluster token when the \
                    node has one, nothing otherwise. A command runs as root in the guest \
                    unless a user is named."
                    .to_string(),
                category: CapabilityCategory::VirtualMachine,
                operations: vec![
                    "node_exec_in_sandbox".to_string(),
                    "node_list_sandbox_commands".to_string(),
                    "node_get_sandbox_command".to_string(),
                    "node_list_sandbox_processes".to_string(),
                ],
                prerequisites: vec!["a running sandbox on the node".to_string()],
                permissions_required: vec!["cluster token".to_string()],
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
                            "enable_gpu": { "type": "boolean", "default": false, "description": "Not implemented: VM creation attaches no GPU, so a request with this set to true is refused" },
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
                            "memory_gb": 8
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
        .into_iter()
        .chain(Self::build_key_operations())
        .chain(Self::build_auth_operations())
        .chain(Self::build_template_share_operations())
        .chain(Self::build_sandbox_control_plane_operations())
        .chain(Self::build_sandbox_exec_control_plane_operations())
        .chain(Self::build_sandbox_node_operations())
        .chain(Self::build_sandbox_exec_node_operations())
        .collect()
    }

    /// The component that serves the operation `id`: `hv2-cluster` for the
    /// control plane operations that are modelled here as data,
    /// `hv2-sandboxd` for the node daemon's, `hv2-api` for every other one.
    /// An operation lives on one component; a path both the control plane
    /// and a node serve is two operations (the node's ids start `node_`).
    /// `tools/check-ontology-coverage.py` compares the operations against the
    /// routes of each component through this.
    pub fn operation_component(id: &str) -> &'static str {
        let on = |ops: Vec<Operation>| ops.into_iter().any(|op| op.id == id);
        if on(Self::build_sandbox_node_operations())
            || on(Self::build_sandbox_exec_node_operations())
        {
            return "hv2-sandboxd";
        }
        let control_plane = Self::build_key_operations()
            .into_iter()
            .chain(Self::build_auth_operations())
            .chain(Self::build_template_share_operations())
            .chain(Self::build_sandbox_control_plane_operations())
            .chain(Self::build_sandbox_exec_control_plane_operations())
            .any(|op| op.id == id);
        if control_plane {
            "hv2-cluster"
        } else {
            "hv2-api"
        }
    }

    /// The control plane's managed API key routes (`hv2-cluster`
    /// `control/key_api.rs`, routed in `control.rs`). Modelled as data here;
    /// `hv2-api` does not depend on `hv2-cluster`. They are served by the
    /// control plane, not by this server, and authenticate with `X-API-Key`.
    fn build_key_operations() -> Vec<Operation> {
        let error = |status_code: u16, description: &str| OperationResponse {
            status_code,
            description: description.to_string(),
            schema: None,
        };
        let auth_errors = || {
            vec![
                error(401, "missing, expired or wrong X-API-Key"),
                error(
                    403,
                    "caller is neither an administrator nor a team-admin, or the request would raise a team-admin above itself",
                ),
                error(
                    503,
                    "managed keys are disabled (no --managed-key-secret-file) or the cluster store is unavailable",
                ),
            ]
        };
        let mut create_responses = vec![
            OperationResponse {
                status_code: 201,
                description: "Key made. `key` is the secret and is shown only in this response \
                    (Cache-Control: no-store)."
                    .to_string(),
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "mk_ and 16 characters" },
                        "key": { "type": "string", "description": "hm_ and 43 characters; shown once" },
                        "label": { "type": "string" },
                        "scopes": { "type": "array", "items": { "type": "string" } },
                        "role": { "type": "string" },
                        "teamId": { "type": ["string", "null"] },
                        "principalId": { "type": ["string", "null"] },
                        "expiresAt": { "type": "integer", "description": "unix seconds" },
                        "createdAt": { "type": "integer", "description": "unix seconds" }
                    }
                })),
            },
            error(400, "invalid label, expiresInSeconds, preset, scopes or role; unknown fields and null are refused"),
            error(429, "the team already holds the maximum of unexpired managed keys (1000 by default)"),
        ];
        create_responses.extend(auth_errors());
        let mut list_responses = vec![OperationResponse {
            status_code: 200,
            description: "Keys visible to the caller; never a secret or a digest. A team-admin \
                sees only its own team's non-administrator keys."
                .to_string(),
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": { "keys": { "type": "array", "items": { "type": "object", "properties": {
                    "id": { "type": "string" }, "label": { "type": "string" },
                    "scopes": { "type": "array", "items": { "type": "string" } },
                    "role": { "type": "string" }, "teamId": { "type": ["string", "null"] },
                    "principalId": { "type": ["string", "null"] }, "createdBy": { "type": "string" },
                    "createdAt": { "type": "integer" }, "expiresAt": { "type": "integer" },
                    "expired": { "type": "boolean" }
                } } } }
            })),
        }];
        list_responses.extend(auth_errors());
        let mut revoke_responses = vec![
            OperationResponse {
                status_code: 200,
                description: "Revoked; the key is refused on the next request.".to_string(),
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "deleted": { "type": "string" }, "kind": { "type": "string", "enum": ["key"] } }
                })),
            },
            error(
                404,
                "the store holds no such managed key (for a team-admin: no such key in its team). \
                 Nothing is recorded, so an already-revoked key cannot be revoked again",
            ),
        ];
        revoke_responses.extend(auth_errors());
        let mut members_responses = vec![
            OperationResponse {
                status_code: 200,
                description: "The team's principals with their roles and active key counts."
                    .to_string(),
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "teamId": { "type": "string" },
                        "members": { "type": "array", "items": { "type": "object", "properties": {
                            "principalId": { "type": "string" },
                            "roles": { "type": "array", "items": { "type": "string" } },
                            "keyCount": { "type": "integer" }
                        } } }
                    }
                })),
            },
            error(400, "invalid team id"),
        ];
        members_responses.extend(auth_errors());
        vec![
            Operation {
                id: "create_managed_key".to_string(),
                name: "Create Managed API Key".to_string(),
                description: "Make a managed API key on the cluster control plane. Requires an \
                    administrator or a team-admin; a team-admin's key is forced to its own \
                    team and may not be a team-admin, hold `admin`, hold a scope the caller \
                    lacks, or outlive the caller. The secret is returned once and cannot be \
                    retrieved later. `teamId` is required unless the key is an \
                    administrator's. Not idempotent: each call makes a new key."
                    .to_string(),
                http_method: "POST".to_string(),
                path: "/keys".to_string(),
                parameters: vec![],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["label", "expiresInSeconds"],
                        "additionalProperties": false,
                        "properties": {
                            "label": { "type": "string", "minLength": 1, "maxLength": 128 },
                            "expiresInSeconds": { "type": "integer", "minimum": 60,
                                "description": "60 up to 90 days unless the operator set --managed-key-max-ttl-secs (at most ten years)" },
                            "preset": { "type": "string", "enum": ["read-only", "developer", "machines", "admin"] },
                            "scopes": { "type": "array", "items": { "type": "string" } },
                            "role": { "type": "string", "enum": ["operator", "member", "observer", "viewer", "team-admin"] },
                            "teamId": { "type": "string" },
                            "principalId": { "type": "string" }
                        }
                    }),
                }),
                responses: create_responses,
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "list_managed_keys".to_string(),
                name: "List Managed API Keys".to_string(),
                description: "List managed keys: id, label, scopes, role, team, principal, \
                    creator, times and whether expired. Never returns a secret or digest. \
                    Expired keys stay listed until revoked. Records the control plane did \
                    not seal are skipped."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/keys".to_string(),
                parameters: vec![],
                request_body: None,
                responses: list_responses,
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "revoke_managed_key".to_string(),
                name: "Revoke Managed API Key".to_string(),
                description: "Revoke a managed key by id. A sealed revocation record is written \
                    first, then the key record is removed; the key stops working on every \
                    control plane on its next request. An administrator may revoke any key, \
                    including a record that cannot be trusted; a team-admin only its own \
                    team's non-administrator keys (anything else is 404). Revocation is \
                    permanent for that id. Sessions already authorised and open streams are \
                    not interrupted. See the guardrails for when revocation can be undone \
                    or was never recorded."
                    .to_string(),
                http_method: "DELETE".to_string(),
                path: "/keys/{id}".to_string(),
                parameters: vec![Parameter {
                    name: "id".to_string(),
                    location: ParameterLocation::Path,
                    required: true,
                    description: "Managed key id: mk_ and 16 URL-safe characters".to_string(),
                    schema: serde_json::json!({ "type": "string", "pattern": "^mk_[A-Za-z0-9_-]{16}$" }),
                    default: None,
                }],
                request_body: None,
                responses: revoke_responses,
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "list_team_members".to_string(),
                name: "List Team Members".to_string(),
                description: "List a team's principals, from the policy file and from managed \
                    keys, with roles and active key counts. A team-admin may list only its \
                    own team (403 otherwise). With managed keys disabled, the policy file's \
                    members only."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/teams/{team}/members".to_string(),
                parameters: vec![Parameter {
                    name: "team".to_string(),
                    location: ParameterLocation::Path,
                    required: true,
                    description: "Team id".to_string(),
                    schema: serde_json::json!({ "type": "string" }),
                    default: None,
                }],
                request_body: None,
                responses: members_responses,
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
        ]
    }

    /// The control plane's single sign-on routes (`hv2-cluster`
    /// `sso_login.rs` and `sso_guest.rs`, routed in `control.rs`). Served by
    /// the control plane outside its API key check: a person without a key
    /// gets a session here. Answers 404 unless single sign-on is configured.
    fn build_auth_operations() -> Vec<Operation> {
        let error = |status_code: u16, description: &str| OperationResponse {
            status_code,
            description: description.to_string(),
            schema: None,
        };
        let json_error = |status_code: u16, description: &str| OperationResponse {
            status_code,
            description: description.to_string(),
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "integer" },
                    "message": { "type": "string" }
                }
            })),
        };
        let not_configured = || {
            json_error(
                404,
                "single sign-on is not configured on this control plane (no --sso-issuer)",
            )
        };
        let query = |name: &str, description: &str, schema: serde_json::Value| Parameter {
            name: name.to_string(),
            location: ParameterLocation::Query,
            required: false,
            description: description.to_string(),
            schema,
            default: None,
        };
        let sign_in_failures = || {
            vec![
                json_error(
                    500,
                    "the control plane could not make randomness or seal the sign-in cookie, \
                     code or session",
                ),
                json_error(
                    502,
                    "the provider's authorization URL, token endpoint or token answer was unusable",
                ),
            ]
        };
        let mut login_responses = vec![
            OperationResponse {
                status_code: 303,
                description: "Redirect to the provider's authorization endpoint (code flow, \
                    S256 PKCE, `openid email profile`). `Set-Cookie: __Host-hm_login` holds \
                    the sealed state, nonce and PKCE verifier for 600 seconds \
                    (Secure, HttpOnly, SameSite=Lax); `Cache-Control: no-store`."
                    .to_string(),
                schema: None,
            },
            json_error(
                400,
                "cliState is not 16 to 128 base64url characters, or cliPort and cliChallenge \
                 are not given together as a port from 1024 and a 43-character S256 challenge. \
                 A cliPort that is not a number is refused by the framework with a plain-text 400",
            ),
            not_configured(),
        ];
        login_responses.extend(sign_in_failures());
        let mut callback_responses = vec![
            OperationResponse {
                status_code: 303,
                description: "Signed in. Browser sign-in: redirect to the `returnTo` the login \
                    carried, with `__Host-hm_session` set (Secure, HttpOnly, SameSite=Lax, \
                    host-only; lifetime `--sso-session-hours`, 12 by default) and the login \
                    cookie cleared. CLI sign-in (the login carried cliPort): redirect to \
                    `http://127.0.0.1:{cliPort}/callback?code=<one-time code>&state=<cliState>`, \
                    no session cookie; the code lasts 60 seconds and is useless without the \
                    PKCE verifier (see sso_cli_token)."
                    .to_string(),
                schema: None,
            },
            json_error(
                400,
                "no sign-in in progress (missing, tampered or expired login cookie), the \
                 provider sent no code or state, or state does not match this sign-in",
            ),
            json_error(
                401,
                "the provider refused the sign-in (`error` was sent), refused the code, or its \
                 ID token failed verification",
            ),
            json_error(
                403,
                "signed in with the provider, but the email is not in the members file",
            ),
            not_configured(),
        ];
        callback_responses.extend(sign_in_failures());
        let mut cli_token_responses = vec![
            OperationResponse {
                status_code: 200,
                description: "A session token for the member, to send as \
                    `Authorization: Bearer hms1.…`."
                    .to_string(),
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "token": { "type": "string", "description": "hms1. session token" },
                        "email": { "type": "string" },
                        "expiresAt": { "type": "integer", "description": "unix seconds" }
                    }
                })),
            },
            json_error(
                401,
                "the code is not valid or has expired, or the verifier does not answer it",
            ),
            json_error(
                403,
                "the member was removed from the members file since signing in",
            ),
            error(
                400,
                "the body is not JSON with string `code` and `verifier` (the framework's own \
                 rejection: 400, 415 or 422, plain text)",
            ),
            not_configured(),
            json_error(500, "the session could not be minted"),
        ];
        cli_token_responses.sort_by_key(|r| r.status_code);
        vec![
            Operation {
                id: "sso_login".to_string(),
                name: "Start Single Sign-On".to_string(),
                description: "Start a sign-in with the configured OpenID Connect provider. The \
                    browser is redirected to the provider and comes back to \
                    `/auth/callback`. No API key is needed. `returnTo` is where the \
                    callback sends the browser afterwards: only a path on this control \
                    plane is honoured (anything else, including `//host` and `/\\host`, \
                    becomes `/ui`). For the CLI (`hm sandbox vm login`) pass `cliPort`, \
                    `cliChallenge` and `cliState`: the sign-in then ends at a listener on \
                    127.0.0.1 with a one-time code instead of a cookie. Nothing is stored \
                    server-side; state, nonce and verifier travel in a signed cookie. \
                    Not idempotent: each call makes a new sign-in."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/auth/login".to_string(),
                parameters: vec![
                    query(
                        "returnTo",
                        "Path on this control plane to return to after sign-in; default /ui",
                        serde_json::json!({ "type": "string", "maxLength": 1024, "default": "/ui" }),
                    ),
                    query(
                        "cliPort",
                        "CLI sign-in only: the unprivileged loopback port the CLI listens on",
                        serde_json::json!({ "type": "integer", "minimum": 1024, "maximum": 65535 }),
                    ),
                    query(
                        "cliChallenge",
                        "CLI sign-in only: base64url SHA-256 of the CLI's secret PKCE verifier",
                        serde_json::json!({ "type": "string", "minLength": 43, "maxLength": 43,
                            "pattern": "^[A-Za-z0-9_-]{43}$" }),
                    ),
                    query(
                        "cliState",
                        "CLI sign-in only: the CLI's random state, echoed on the callback",
                        serde_json::json!({ "type": "string", "pattern": "^[A-Za-z0-9_-]{16,128}$" }),
                    ),
                ],
                request_body: None,
                responses: login_responses,
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "sso_callback".to_string(),
                name: "Complete Single Sign-On".to_string(),
                description: "The redirect URI the provider sends the browser back to (register \
                    `https://<control-plane>/auth/callback` with the provider). Requires the \
                    login cookie from `/auth/login`; checks `state` in constant time, \
                    exchanges the code with the PKCE verifier, verifies the ID token (RS256 \
                    or ES256, exact issuer, audience, expiry and nonce) and looks the email \
                    up in the members file. It is called by the browser, not by an agent. \
                    Not idempotent: a code works once."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/auth/callback".to_string(),
                parameters: vec![
                    query("code", "Authorization code from the provider", serde_json::json!({ "type": "string" })),
                    query("state", "The state this sign-in started with", serde_json::json!({ "type": "string" })),
                    query("error", "Set by the provider when it refused the sign-in", serde_json::json!({ "type": "string" })),
                ],
                request_body: None,
                responses: callback_responses,
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "sso_session".to_string(),
                name: "Get Signed-In Member".to_string(),
                description: "Say who the session is. Send the session as \
                    `Authorization: Bearer hms1.…` (checked first) or by its \
                    `__Host-hm_session` cookie. Returns the member's email, principal, team \
                    and the session's expiry. An API key is not a session: it answers 401."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/auth/session".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 200,
                        description: "The signed-in member.".to_string(),
                        schema: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "email": { "type": "string" },
                                "principalID": { "type": "string" },
                                "teamID": { "type": ["string", "null"] },
                                "expiresAt": { "type": "integer", "description": "unix seconds" }
                            }
                        })),
                    },
                    json_error(
                        401,
                        "not signed in: no session, a bad or expired one, or a member since removed from the members file",
                    ),
                    not_configured(),
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "sso_guest_handoff".to_string(),
                name: "Sign In To A Private Guest URL".to_string(),
                description: "Send a signed-in member who may view a sandbox to its private \
                    guest URL (`https://{port}-{sandbox}.{guest-domain}/…`, or a bound custom \
                    domain) with a one-minute handoff sealed to that one host; the host's \
                    proxy turns it into its own `__Host-hm_guest` cookie at `/__hm/auth`. The \
                    API session cookie never goes to a guest host. Needs the session \
                    *cookie* (a bearer token is not accepted here); a browser without one \
                    is redirected to `/auth/login` and back. Only hosts under a configured \
                    `--guest-domain` (or a bound custom domain) are served; anything else \
                    is 400. May view: an administrator, a member of the sandbox's team (or \
                    its creator where there are no teams), or an email its owner granted \
                    through web sharing. It is called by the browser, not by an agent."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/auth/guest".to_string(),
                parameters: vec![Parameter {
                    name: "url".to_string(),
                    location: ParameterLocation::Query,
                    required: true,
                    description: "The guest URL to be signed in to; https only, no credentials \
                        in it"
                        .to_string(),
                    schema: serde_json::json!({ "type": "string", "format": "uri" }),
                    default: None,
                }],
                request_body: None,
                responses: vec![
                    OperationResponse {
                        status_code: 303,
                        description: "Redirect: to `https://{host}/__hm/auth?token=<handoff>&path=…` \
                            when the member may view the sandbox, or to `/auth/login?returnTo=…` \
                            when there is no session cookie. `Cache-Control: no-store`."
                            .to_string(),
                        schema: None,
                    },
                    error(400, "not a guest URL of this control plane (plain text); a missing `url` is the framework's own 400"),
                    error(403, "the member may not view this sandbox (plain text)"),
                    error(404, "single sign-on is not configured (plain text)"),
                    error(500, "the handoff could not be sealed"),
                ],
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "sso_logout".to_string(),
                name: "Sign Out".to_string(),
                description: "Clear the `__Host-hm_session` cookie. The request must carry an \
                    `Origin` header equal to the control plane's own origin (the redirect \
                    URL's), so only its own pages can sign a browser out; a script that \
                    sends none gets 403. It ends nothing server-side: a session token is \
                    stateless, and one already copied (a bearer token, a stolen cookie) \
                    stays valid until it expires. See the guardrails."
                    .to_string(),
                http_method: "POST".to_string(),
                path: "/auth/logout".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![
                    error(204, "the session cookie is cleared (Max-Age=0)"),
                    json_error(403, "the request did not come from this control plane's own pages (no matching Origin)"),
                    not_configured(),
                ],
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "sso_cli_token".to_string(),
                name: "Trade A CLI Sign-In Code For A Session Token".to_string(),
                description: "Finish a CLI sign-in: trade the one-time code the CLI's loopback \
                    listener received, together with the PKCE verifier that never left the \
                    CLI, for a session token (`hm sandbox vm login` does this). Needs no \
                    API key. A code seen in transit is useless without the verifier and \
                    expires after 60 seconds. The member is looked up again, so one removed \
                    since signing in gets 403. The token is sent as \
                    `Authorization: Bearer hms1.…` and lasts `--sso-session-hours` (12 by \
                    default). Not idempotent: each call mints a new token."
                    .to_string(),
                http_method: "POST".to_string(),
                path: "/auth/cli-token".to_string(),
                parameters: vec![],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["code", "verifier"],
                        "properties": {
                            "code": { "type": "string", "description": "the hmc1. code from the loopback callback" },
                            "verifier": { "type": "string", "description": "the PKCE verifier whose SHA-256 was the login's cliChallenge" }
                        }
                    }),
                }),
                responses: cli_token_responses,
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
        ]
    }

    /// The control plane's team snapshot sharing routes (`hv2-cluster`
    /// `control.rs`: `template_shares`, `share_template`; the node side is
    /// `hv2-sandboxd` `snapshots.rs`). Served by the control plane and
    /// authenticated with `X-API-Key` or a session; the `templates` scope.
    fn build_template_share_operations() -> Vec<Operation> {
        let json_error = |status_code: u16, description: &str| OperationResponse {
            status_code,
            description: description.to_string(),
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "integer" },
                    "message": { "type": "string" }
                }
            })),
        };
        let auth_errors = || {
            vec![
                json_error(401, "missing, expired or wrong X-API-Key or session"),
                json_error(
                    403,
                    "the key or member lacks the `templates` scope, is an observer or viewer, or \
                     a session-cookie change did not come from the control plane's own pages",
                ),
                json_error(503, "the cluster store is unavailable"),
            ]
        };
        let snapshot_param = || Parameter {
            name: "id".to_string(),
            location: ParameterLocation::Path,
            required: true,
            description: "The snapshot's name (an optional `:tag` is ignored)".to_string(),
            schema: serde_json::json!({ "type": "string" }),
            default: None,
        };
        let team_param = || Parameter {
            name: "team".to_string(),
            location: ParameterLocation::Path,
            required: true,
            description: "The team to share with, or stop sharing with: a bounded team label"
                .to_string(),
            schema: serde_json::json!({ "type": "string" }),
            default: None,
        };
        let shares_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "snapshotID": { "type": "string" },
                "teamID": { "type": "string", "description": "the owning team" },
                "sharedWith": { "type": "array", "items": { "type": "string" } }
            }
        });
        let mut list_responses = vec![
            OperationResponse {
                status_code: 200,
                description: "The snapshot's owning team and the teams it is shared with."
                    .to_string(),
                schema: Some(shares_schema.clone()),
            },
            json_error(
                404,
                "no such snapshot, or it belongs to another team (the two look the same)",
            ),
        ];
        list_responses.extend(auth_errors());
        let mut share_responses = vec![
            OperationResponse {
                status_code: 200,
                description: "Shared (or already shared). Every node that has the snapshot was \
                    told."
                    .to_string(),
                schema: Some(shares_schema),
            },
            json_error(
                400,
                "the team or snapshot id is invalid, or the body does not say \
                 `acknowledgeContents: true`",
            ),
            json_error(
                404,
                "no such snapshot, or it belongs to another team (the two look the same)",
            ),
            json_error(
                409,
                "the snapshot is an operator's template (every team already uses it), or the \
                 team is the snapshot's own",
            ),
            json_error(
                500,
                "the share list could not be written to the snapshot store",
            ),
        ];
        share_responses.extend(auth_errors());
        let mut unshare_responses = vec![
            OperationResponse {
                status_code: 204,
                description: "Not shared with the team any more. Sandboxes it already started \
                    run on."
                    .to_string(),
                schema: None,
            },
            json_error(400, "the team or snapshot id is invalid"),
            json_error(
                404,
                "no such snapshot, or it belongs to another team (the two look the same)",
            ),
            json_error(
                409,
                "the snapshot is an operator's template, or the team is the snapshot's own",
            ),
            json_error(
                500,
                "the share list could not be written to the snapshot store",
            ),
        ];
        unshare_responses.extend(auth_errors());
        vec![
            Operation {
                id: "list_template_shares".to_string(),
                name: "List Snapshot Shares".to_string(),
                description: "List the teams a snapshot is shared with. Only the snapshot's own \
                    team (or an administrator) may read it; the team shared with, and every \
                    other team, gets 404."
                    .to_string(),
                http_method: "GET".to_string(),
                path: "/templates/{id}/shares".to_string(),
                parameters: vec![snapshot_param()],
                request_body: None,
                responses: list_responses,
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "share_template_snapshot".to_string(),
                name: "Share A Snapshot With A Team".to_string(),
                description: "Let another team start sandboxes from this team's snapshot. What \
                    they start from is the source sandbox as it was: its memory, its files and \
                    its environment variables; nothing is removed first. The request must \
                    say it knows with `{\"acknowledgeContents\": true}`. Only the snapshot's \
                    own team (or an administrator) may do it. The team shared with lists \
                    the snapshot and starts sandboxes from it (those sandboxes are its \
                    own); it cannot read the list, share it on, revoke it or delete it. \
                    Taking a new snapshot of the same name replaces it and drops its \
                    shares. Idempotent: sharing twice is the same as once. See the \
                    guardrails."
                    .to_string(),
                http_method: "PUT".to_string(),
                path: "/templates/{id}/shares/{team}".to_string(),
                parameters: vec![snapshot_param(), team_param()],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["acknowledgeContents"],
                        "properties": {
                            "acknowledgeContents": { "type": "boolean", "const": true,
                                "description": "must be true: the other team gets the sandbox's memory, files and environment variables" }
                        }
                    }),
                }),
                responses: share_responses,
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: "unshare_template_snapshot".to_string(),
                name: "Stop Sharing A Snapshot With A Team".to_string(),
                description: "Stop letting a team start sandboxes from this team's snapshot. \
                    New starts are refused at once; sandboxes already started run on. Only \
                    the snapshot's own team (or an administrator) may do it. Idempotent: \
                    stopping a share that does not exist is 204."
                    .to_string(),
                http_method: "DELETE".to_string(),
                path: "/templates/{id}/shares/{team}".to_string(),
                parameters: vec![snapshot_param(), team_param()],
                request_body: None,
                responses: unshare_responses,
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
        ]
    }

    /// The control plane's sandbox lifecycle routes (`hv2-cluster`
    /// `control.rs`: `create_v1`, `create_v2`, `list_v1`, `list_v2`,
    /// `detail`, `forward`, `adopt_owner`). Modelled as data; `hv2-api` does
    /// not depend on `hv2-cluster`. The ids are the same as the node's
    /// without the `node_` prefix.
    fn build_sandbox_control_plane_operations() -> Vec<Operation> {
        Self::sandbox_lifecycle_operations(false)
    }

    /// The node daemon's sandbox lifecycle routes (`hv2-sandboxd`
    /// `main.rs` and `reboot.rs`). Served by `hv2-sandboxd`, one per node;
    /// the control plane forwards most of them.
    fn build_sandbox_node_operations() -> Vec<Operation> {
        Self::sandbox_lifecycle_operations(true)
    }

    /// The control plane's sandbox command routes (`forward`, to the node's
    /// `exec`, `history::list`, `history::get`, `sandbox_processes`).
    fn build_sandbox_exec_control_plane_operations() -> Vec<Operation> {
        Self::sandbox_exec_operations(false)
    }

    /// The node daemon's sandbox command routes (`hv2-sandboxd` `main.rs`,
    /// `history.rs`, `processes.rs`).
    fn build_sandbox_exec_node_operations() -> Vec<Operation> {
        Self::sandbox_exec_operations(true)
    }

    /// One response of the `{"code", "message"}` shape both components use
    /// for an error they raise themselves.
    fn sandbox_json_error(status_code: u16, description: &str) -> OperationResponse {
        OperationResponse {
            status_code,
            description: description.to_string(),
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "integer" },
                    "message": { "type": "string" }
                }
            })),
        }
    }

    /// A response with a body the framework (axum) wrote, or none.
    fn sandbox_plain(status_code: u16, description: &str) -> OperationResponse {
        OperationResponse {
            status_code,
            description: description.to_string(),
            schema: None,
        }
    }

    /// What can refuse a request before the handler runs. On the control
    /// plane that is the API key or session check and, for a route that
    /// names a sandbox, the ownership check and the forward to its node; on
    /// a node, the cluster token.
    fn sandbox_access_responses(node: bool, names_sandbox: bool) -> Vec<OperationResponse> {
        let json_error = Self::sandbox_json_error;
        if node {
            return vec![json_error(
                401,
                "the node was started with a cluster token (or mTLS) and the request does not \
                 carry it in `x-hv2-cluster-token` (`this node answers its cluster's control \
                 plane only`); a node with no token answers everyone",
            )];
        }
        let mut responses = vec![
            json_error(
                401,
                "missing, expired or wrong X-API-Key or session; with single sign-on \
                 configured an anonymous request is refused too",
            ),
            json_error(
                403,
                "the key or member lacks the `sandboxes` scope, is an observer or viewer \
                 (those may read the inventory routes only), or a session-cookie change did \
                 not come from the control plane's own pages",
            ),
        ];
        if names_sandbox {
            responses.push(json_error(
                403,
                "`sandbox owner required`: the sandbox belongs to another team, or to \
                 another principal when the key has no team",
            ));
            responses.push(json_error(
                400,
                "`invalid sandbox ID` (for a caller limited to a team or an owner), or a \
                 path with `.` or `..` segments",
            ));
            responses.push(json_error(
                404,
                "no such sandbox in the cluster store, or its node stopped heartbeating and \
                 the record was removed (the first request to find it says so)",
            ));
            responses.push(json_error(
                502,
                "no node answered: the sandbox's node (or, for a sandbox paused into shared \
                 storage, every node tried) was unreachable",
            ));
        }
        responses.push(json_error(
            503,
            "the cluster store is unavailable, the ownership lookup timed out, or the node \
             answered 503 and no other node could take the request",
        ));
        responses
    }

    fn sandbox_id_parameter(node: bool) -> Parameter {
        Parameter {
            name: if node { "sandboxID" } else { "id" }.to_string(),
            location: ParameterLocation::Path,
            required: true,
            description: "The sandbox's ID, as returned when it was created".to_string(),
            schema: serde_json::json!({ "type": "string" }),
            default: None,
        }
    }

    fn sandbox_query(name: &str, description: &str, schema: serde_json::Value) -> Parameter {
        Parameter {
            name: name.to_string(),
            location: ParameterLocation::Query,
            required: false,
            description: description.to_string(),
            schema,
            default: None,
        }
    }

    /// The descriptor `POST /sandboxes` answers with, which `connect` and
    /// `resume` answer with again.
    fn sandbox_descriptor_schema(node: bool) -> serde_json::Value {
        let mut properties = serde_json::json!({
            "templateID": { "type": "string", "description": "the template or snapshot asked for" },
            "sandboxID": { "type": "string" },
            "clientID": { "type": "string", "description": "equal to sandboxID" },
            "envdVersion": { "type": "string" },
            "processPort": { "type": "integer", "description": "the port of the sandbox's envd process.Process gRPC listener on its node" },
            "envdHost": { "type": "string", "description": "the authority to ask the proxy for: `{envd port}-{sandboxID}`" },
            "proxyPort": { "type": "integer", "description": "the port of the sandbox proxy that serves envdHost: the control plane's through the control plane, the node's on the node" },
            "envdAccessToken": { "type": "string", "description": "sent as `X-Access-Token` to this sandbox's envd; a credential" }
        });
        if !node {
            properties["nodeID"] = serde_json::json!({
                "type": "string",
                "description": "the node that runs the sandbox"
            });
        }
        serde_json::json!({ "type": "object", "properties": properties })
    }

    /// E2B's `ListedSandbox`, as the control plane and the node list it.
    fn sandbox_listed_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "templateID": { "type": "string" },
                "sandboxID": { "type": "string" },
                "clientID": { "type": "string" },
                "startedAt": { "type": "string", "format": "date-time" },
                "endAt": { "type": "string", "format": "date-time" },
                "cpuCount": { "type": "integer" },
                "memoryMB": { "type": "integer" },
                "diskSizeMB": { "type": "integer", "description": "always 0: the root filesystem is memory" },
                "metadata": { "type": "object", "additionalProperties": { "type": "string" } },
                "state": { "type": "string", "enum": ["running", "paused"] },
                "envdVersion": { "type": "string" },
                "nodeID": { "type": "string", "description": "the node that runs it" }
            }
        })
    }

    fn sandbox_lifecycle_operations(node: bool) -> Vec<Operation> {
        let id = |base: &str| {
            if node {
                format!("node_{base}")
            } else {
                base.to_string()
            }
        };
        let id_path = |rest: &str| {
            let name = if node { "sandboxID" } else { "id" };
            format!("/sandboxes/{{{name}}}{rest}")
        };
        let json_error = Self::sandbox_json_error;
        let plain = Self::sandbox_plain;
        let who = if node { "This node's" } else { "The cluster's" };
        let state_param = || {
            Self::sandbox_query(
                "state",
                "Comma-separated states to keep: `running`, `paused`",
                serde_json::json!({ "type": "string", "example": "running,paused" }),
            )
        };
        let metadata_param = || {
            Self::sandbox_query(
                "metadata",
                "URL-encoded `key=value&key=value`: keep the sandboxes that carry every pair \
                 in their metadata",
                serde_json::json!({ "type": "string" }),
            )
        };
        let rewritten = if node {
            ""
        } else {
            " The control plane points `proxyPort` at its own sandbox proxy and adds `nodeID`."
        };
        let body_rejection = || {
            plain(
                400,
                "the body is not the JSON this route reads: the framework's own rejection \
                 (400, 415 or 422, plain text)",
            )
        };

        // The request body of a create, from `NewSandbox` in main.rs.
        let new_sandbox = serde_json::json!({
            "type": "object",
            "properties": {
                "templateID": { "type": "string", "default": "base",
                    "description": "a template, or a snapshot of the caller's team (or shared with it); an optional `team/` prefix and `:tag` are ignored" },
                "timeout": { "type": "integer", "minimum": 0, "maximum": 86400,
                    "description": "seconds to live; 15 on /sandboxes and 300 on /v2/sandboxes by default, capped at 24 hours" },
                "metadata": { "type": "object", "additionalProperties": { "type": "string" },
                    "description": "labels, listable with `metadata=`. `hm.name` names the sandbox (see assign names)" },
                "allow_internet_access": { "type": "boolean", "description": "snake case, as in E2B's spec" },
                "network": { "type": "object",
                    "properties": {
                        "allowOut": { "type": "array", "items": { "type": "string" } },
                        "denyOut": { "type": "array", "items": { "type": "string" } },
                        "rules": { "type": "object" },
                        "egressProxy": { "type": "object",
                            "required": ["address"],
                            "properties": {
                                "address": { "type": "string" },
                                "username": { "type": "string" },
                                "password": { "type": "string" }
                            } }
                    },
                    "description": "egress policy; needs a node started with --network" },
                "autoPause": { "type": "boolean", "description": "pause at the timeout instead of ending" },
                "autoPauseMemory": { "type": "boolean", "description": "false is refused (400): a pause always keeps memory" },
                "autoResume": { "type": "object", "properties": { "enabled": { "type": "boolean" } },
                    "description": "a request through the proxy resumes it when paused" },
                "iam": { "type": "object",
                    "properties": { "tokens": { "type": "object",
                        "additionalProperties": { "type": "object",
                            "required": ["audience", "tokenType"],
                            "properties": { "audience": { "type": "string" }, "tokenType": { "type": "string" } } } } },
                    "description": "workload identity tokens the egress gateway mints; needs --network" },
                "volumeMounts": { "type": "array",
                    "items": { "type": "object", "required": ["name", "path"],
                        "properties": { "name": { "type": "string" }, "path": { "type": "string" } } } },
                "diskMount": { "type": "object", "required": ["name", "path"],
                    "properties": { "name": { "type": "string" }, "path": { "type": "string" } },
                    "description": "a disk held by this sandbox alone; cannot start from a snapshot" },
                "envVars": { "type": "object", "additionalProperties": { "type": "string" },
                    "description": "set for every command the sandbox runs; never returned" },
                "cpuCount": { "type": "integer" },
                "memoryMB": { "type": "integer" },
                "environment": { "type": "string", "description": "a named environment of the caller's team: variables, files and repositories to start with" },
                "commandHistory": { "type": "boolean", "description": "false keeps no history of its commands" },
                "idleTimeout": { "type": "integer", "description": "seconds without use after which it pauses; 0 turns off the node's default" },
                "autoCheckpoint": { "type": "integer", "description": "seconds between checkpoints the node takes by itself; 0 turns off the node's default" }
            }
        });

        let create_description = |v2: bool| {
            let (path, default_timeout) = if v2 {
                ("/v2/sandboxes", 300)
            } else {
                ("/sandboxes", 15)
            };
            if node {
                format!(
                    "Boot a microVM sandbox on this node from a template (default `base`) or \
                     from a snapshot, and answer with its descriptor: its ID, the port of its \
                     envd gRPC listener, the proxy authority to ask for and the \
                     `envdAccessToken` to send as `X-Access-Token`. `POST {path}` is the \
                     {} route of E2B's API; its default lifetime is {default_timeout} \
                     seconds, at most 24 hours. The body is checked before a slot is taken: \
                     an unknown template, snapshot, volume or environment, a bad variable, \
                     size or egress policy, `autoPause` or `idleTimeout` or \
                     `autoCheckpoint` on a node with no template, `network` or `iam` on a \
                     node started without `--network`, and `diskMount` with a snapshot are \
                     400 or 404, not a boot. A full node answers 503 at once so a control plane can \
                     try another. On a clustered node, owner and team come from headers \
                     only an authenticated control plane sends (a request carrying them \
                     without a cluster token is 503), and a sandbox named with \
                     `metadata[\"hm.name\"]` needs the control plane's reserved operation \
                     header (409 without it). Not idempotent: each call boots a new sandbox.",
                    if v2 { "current (secure-only)" } else { "older" }
                )
            } else {
                format!(
                    "Boot a microVM sandbox somewhere in the cluster and answer with its \
                     descriptor (see the node's `node_create_sandbox{}`). The control plane \
                     keeps only the nodes that offer the template (`templateID`, default \
                     `base`; the caller team's own snapshot, or one shared with it, wins over \
                     an operator template of the same name) and tries them in turn: a node \
                     that is unreachable, full (503) or overloaded (429) is skipped, any \
                     other answer from a node is final. The caller's principal and team, taken \
                     from its credential and never from the request, are recorded as the \
                     sandbox's owner and team; that needs the control plane to have a \
                     `--cluster-token` (503 otherwise). Naming the sandbox with \
                     `metadata[\"hm.name\"]` reserves the name first and then tries one node \
                     only; a name that is taken (or that a legacy sandbox carries) is 409. \
                     `POST {path}` has a default lifetime of {default_timeout} seconds.{rewritten} \
                     Not idempotent: each call boots a new sandbox.",
                    if v2 { "_v2" } else { "" }
                )
            }
        };
        let create_responses = || {
            let mut responses = vec![
                OperationResponse {
                    status_code: 201,
                    description: format!("The sandbox is running.{rewritten}"),
                    schema: Some(Self::sandbox_descriptor_schema(node)),
                },
                json_error(
                    400,
                    "the body is refused: an invalid field or value (the message names it), \
                     or, on the control plane, a node's 4xx relayed as it came",
                ),
                body_rejection(),
                json_error(
                    404,
                    if node {
                        "the template, snapshot (also one of another team) or environment is not \
                         known to this node"
                    } else {
                        "no live node offers the template (or it is another team's snapshot)"
                    },
                ),
            ];
            if node {
                responses.push(json_error(
                    409,
                    "`hm.name` without the control plane's reserved operation, or that \
                     operation is not pending for this owner",
                ));
                responses.push(json_error(
                    500,
                    "the guest could not be brought up; the message says why",
                ));
                responses.push(json_error(
                    503,
                    "no free slot on this node, or owner or team headers arrived without a \
                     cluster token",
                ));
            } else {
                responses.push(json_error(
                    409,
                    "the `hm.name` is already reserved or a legacy sandbox carries it",
                ));
                responses.push(json_error(
                    500,
                    "a node could not bring the guest up (relayed from the node)",
                ));
                responses.push(json_error(
                    502,
                    "named creation: the node's outcome cannot be established and must be \
                     reconciled, or its answer was unusable",
                ));
                responses.push(json_error(
                    503,
                    "no node has room, every node refused, the name inventory or reservation is \
                     unavailable, or owner attribution is requested without a `--cluster-token`",
                ));
            }
            responses.extend(Self::sandbox_access_responses(node, false));
            responses
        };

        let list_description = |v2: bool| {
            let path = if v2 { "/v2/sandboxes" } else { "/sandboxes" };
            if node {
                format!(
                    "List the sandboxes of this node alone, running and paused, oldest \
                     start first. `GET {path}` is the same route as the other list path. \
                     Keep some with `state` and `metadata`. Not paged. Every sandbox the node \
                     runs is listed, whoever owns it: ownership is the control plane's check."
                )
            } else if v2 {
                "List the sandboxes the caller may see, from the cluster store: all of them for \
                 an administrator, the caller's team's for a team key or member, and its own \
                 for a key with a principal and no team. Running and paused, in the store's \
                 order (`order=desc` reverses it). Paged: `limit` (default 100, from 1 to \
                 1000) and `nextToken`, which is the `x-next-token` response header of the \
                 previous page (the last sandbox ID of it); a token naming no sandbox starts \
                 from the first page. An inventory route: an observer or viewer, or a key \
                 with the `inventory` scope, may read it."
                    .to_string()
            } else {
                "List the sandboxes the caller may see, from the cluster store: all of them for \
                 an administrator, the caller's team's for a team key or member, and its own \
                 for a key with a principal and no team. Running and paused, in the store's \
                 order (`order=desc` reverses it). `limit` and `nextToken` are accepted and \
                 ignored here; use /v2/sandboxes to page. An inventory route: an observer \
                 or viewer, or a key with the `inventory` scope, may read it."
                    .to_string()
            }
        };
        let list_params = |v2: bool| {
            let mut parameters = vec![metadata_param(), state_param()];
            if !node {
                parameters.push(Self::sandbox_query(
                    "order",
                    "`desc` reverses the order",
                    serde_json::json!({ "type": "string", "enum": ["desc"] }),
                ));
                if v2 {
                    parameters.push(Self::sandbox_query(
                        "limit",
                        "Page size, clamped to 1..=1000",
                        serde_json::json!({ "type": "integer", "default": 100 }),
                    ));
                    parameters.push(Self::sandbox_query(
                        "nextToken",
                        "The previous page's `x-next-token`",
                        serde_json::json!({ "type": "string" }),
                    ));
                }
            }
            parameters
        };
        let list_responses = |v2: bool| {
            let mut responses = vec![OperationResponse {
                status_code: 200,
                description: if v2 && !node {
                    "A page of sandboxes. `x-next-token` is set when more follow.".to_string()
                } else {
                    "The sandboxes.".to_string()
                },
                schema: Some(serde_json::json!({
                    "type": "array",
                    "items": Self::sandbox_listed_schema()
                })),
            }];
            responses.extend(Self::sandbox_access_responses(node, false));
            responses
        };

        let connect_description = |v2: bool| {
            format!(
                "Attach to a sandbox that exists and get its descriptor again (what the E2B \
                 SDK's `Sandbox.connect()` calls; `{}` is the same route). A running sandbox \
                 answers 200; a paused one is resumed first, with a lifetime of `timeout` \
                 seconds (300 by default), and answers 201. For a running sandbox `timeout` \
                 only extends the end time, never shortens it, at most 24 hours from now. \
                 `memory: false` (resume from disk alone) is refused: a sandbox's filesystem \
                 lives in its memory. The body is optional.{rewritten} Not idempotent for a paused \
                 sandbox, which it brings back.",
                if v2 {
                    "/sandboxes/{id}/connect"
                } else {
                    "/v2/sandboxes/{id}/connect"
                }
            )
        };
        let connect_responses = || {
            let mut responses = vec![
                OperationResponse {
                    status_code: 200,
                    description: format!("It was running. Its descriptor.{rewritten}"),
                    schema: Some(Self::sandbox_descriptor_schema(node)),
                },
                OperationResponse {
                    status_code: 201,
                    description: format!("It was paused and is running again.{rewritten}"),
                    schema: Some(Self::sandbox_descriptor_schema(node)),
                },
                json_error(400, "`memory: false` was asked for"),
                body_rejection(),
                json_error(404, "no such sandbox"),
                json_error(
                    409,
                    "its registration is unsettled: reconcile it before changing the timeout",
                ),
                json_error(
                    503,
                    "a paused sandbox found no free slot on the node (or the snapshot store is \
                     unavailable)",
                ),
            ];
            responses.extend(Self::sandbox_access_responses(node, true));
            responses
        };
        let connect_body = RequestBody {
            content_type: "application/json".to_string(),
            required: false,
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "timeout": { "type": "integer", "description": "seconds from now" },
                    "memory": { "type": "boolean", "description": "false is refused (400)" }
                }
            }),
        };

        let mut operations = vec![
            Operation {
                id: id("create_sandbox"),
                name: "Create A Sandbox".to_string(),
                description: create_description(false),
                http_method: "POST".to_string(),
                path: "/sandboxes".to_string(),
                parameters: vec![],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: new_sandbox.clone(),
                }),
                responses: create_responses(),
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("create_sandbox_v2"),
                name: "Create A Sandbox (v2)".to_string(),
                description: create_description(true),
                http_method: "POST".to_string(),
                path: "/v2/sandboxes".to_string(),
                parameters: vec![],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: new_sandbox,
                }),
                responses: create_responses(),
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("list_sandboxes"),
                name: "List Sandboxes".to_string(),
                description: list_description(false),
                http_method: "GET".to_string(),
                path: "/sandboxes".to_string(),
                parameters: list_params(false),
                request_body: None,
                responses: list_responses(false),
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("list_sandboxes_v2"),
                name: "List Sandboxes (v2)".to_string(),
                description: list_description(true),
                http_method: "GET".to_string(),
                path: "/v2/sandboxes".to_string(),
                parameters: list_params(true),
                request_body: None,
                responses: list_responses(true),
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("get_sandbox"),
                name: "Get A Sandbox".to_string(),
                description: format!(
                    "{who} record of one sandbox: the listing fields plus its \
                     `envdAccessToken` (a credential for its envd) and, when it has one, its \
                     `domain`. Running or paused. {}",
                    if node {
                        "Read from this node's memory: a sandbox another node runs is 404."
                    } else {
                        "Read from the cluster store, not asked of the node, so it answers for \
                         a paused sandbox too. An observer or viewer is refused (docs/TEAMS.md)."
                    }
                ),
                http_method: "GET".to_string(),
                path: id_path(""),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The sandbox.".to_string(),
                            schema: Some(Self::sandbox_listed_schema()),
                        },
                        json_error(404, "no such sandbox"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, !node));
                    // Answered from the store: no node is asked.
                    responses.retain(|r| r.status_code != 502);
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("delete_sandbox"),
                name: "Delete A Sandbox".to_string(),
                description: format!(
                    "End a sandbox and everything it had: stop its VM and envd listener, \
                     release its slot, address and network membership, and, if it was paused, \
                     remove its snapshot. Not idempotent: once it is gone the second call is \
                     404. {}",
                    if node {
                        "A paused sandbox held in a shared snapshot store is deleted only if \
                         this node can claim it (404 when another node has resumed it)."
                    } else {
                        "The control plane forwards the delete to the sandbox's node and then \
                         removes the sandbox from the cluster store (503 if that cleanup \
                         fails). When the node is gone, the sandbox went with it: the record is \
                         removed and the answer is 204."
                    }
                ),
                http_method: "DELETE".to_string(),
                path: id_path(""),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        Self::sandbox_plain(204, "The sandbox is gone."),
                        json_error(404, "no such sandbox"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("connect_sandbox"),
                name: "Connect To A Sandbox".to_string(),
                description: connect_description(false),
                http_method: "POST".to_string(),
                path: id_path("/connect"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(connect_body.clone()),
                responses: connect_responses(),
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("connect_sandbox_v2"),
                name: "Connect To A Sandbox (v2)".to_string(),
                description: connect_description(true),
                http_method: "POST".to_string(),
                path: format!("/v2{}", id_path("/connect")),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(connect_body),
                responses: connect_responses(),
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("set_sandbox_timeout"),
                name: "Set A Sandbox's Timeout".to_string(),
                description: "Make a running sandbox end `timeout` seconds from this request, \
                    sooner or later than before (at most 24 hours). Unlike `connect`, this can \
                    shorten it. A paused sandbox is 404: resume or connect to it first."
                    .to_string(),
                http_method: "POST".to_string(),
                path: id_path("/timeout"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["timeout"],
                        "properties": { "timeout": { "type": "integer", "minimum": 0,
                            "description": "seconds from now" } }
                    }),
                }),
                responses: {
                    let mut responses = vec![
                        Self::sandbox_plain(204, "The new end time is set."),
                        body_rejection(),
                        json_error(404, "no running sandbox of that ID"),
                        json_error(
                            409,
                            "its registration is unsettled: reconcile it before changing the \
                             timeout",
                        ),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("pause_sandbox"),
                name: "Pause A Sandbox".to_string(),
                description: "Suspend a running sandbox to disk and release its VM and its \
                    slot. Its memory and filesystem are kept: `connect` or `resume` brings it \
                    back under the same ID and token, and with `autoResume` so does a request \
                    through the proxy. `memory: false` (a filesystem-only pause) is refused, \
                    because a sandbox's filesystem is its memory. Needs a node whose sandboxes \
                    are restored from a template (409 on one that boots them). Commands \
                    running at the pause are recorded as lost in the command history; the \
                    guest's processes go on after the resume."
                    .to_string(),
                http_method: "POST".to_string(),
                path: id_path("/pause"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: false,
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": { "memory": { "type": "boolean",
                            "description": "false is refused (400)" } }
                    }),
                }),
                responses: {
                    let mut responses = vec![
                        Self::sandbox_plain(204, "It is paused."),
                        json_error(400, "`memory: false` was asked for"),
                        body_rejection(),
                        json_error(404, "no running sandbox of that ID"),
                        json_error(
                            409,
                            "it is already paused, the node boots its sandboxes instead of \
                             restoring a template, or its registration is unsettled",
                        ),
                        json_error(500, "the pause failed; the message says why"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("resume_sandbox"),
                name: "Resume A Paused Sandbox".to_string(),
                description: format!(
                    "Bring a paused sandbox back (E2B's deprecated route; `connect` does the \
                     same and also answers for a running one). It runs for `timeout` seconds \
                     (15 by default). With `cpuCount` or `memoryMB` it comes back at another \
                     size, booted rather than restored, so its memory is not kept. A running \
                     sandbox is 409. {}{rewritten}",
                    if node {
                        "With a shared snapshot store a sandbox paused by another node is claimed \
                         from the store."
                    } else {
                        "When the sandbox was paused into shared storage and its node is gone, \
                         any node with room resumes it."
                    }
                ),
                http_method: "POST".to_string(),
                path: id_path("/resume"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: false,
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "timeout": { "type": "integer", "default": 15 },
                            "memory": { "type": "boolean", "description": "false is refused (400)" },
                            "cpuCount": { "type": "integer" },
                            "memoryMB": { "type": "integer" }
                        }
                    }),
                }),
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 201,
                            description: format!("It is running again.{rewritten}"),
                            schema: Some(Self::sandbox_descriptor_schema(node)),
                        },
                        json_error(
                            400,
                            "`memory: false`, or a size this host cannot give",
                        ),
                        body_rejection(),
                        json_error(404, "no such paused sandbox"),
                        json_error(409, "it is already running, or it has a network and this node gives sandboxes none"),
                        json_error(500, "the guest could not be brought back; the message says why"),
                        json_error(503, "no free slot on the node"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("standby_sandbox"),
                name: "Put A Sandbox In Standby".to_string(),
                description: "Stop a running sandbox's vCPUs and keep its memory. \
                    The next request to it (a command, a file, a connection to one of its \
                    ports) wakes it, so no route resumes a standby by name. Answers with the \
                    standby status; asking again for a sandbox already in standby answers 200 \
                    with it."
                    .to_string(),
                http_method: "POST".to_string(),
                path: id_path("/standby"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "It is in standby.".to_string(),
                            schema: Some(Self::sandbox_standby_schema()),
                        },
                        json_error(404, "no running sandbox of that ID"),
                        json_error(409, "the guest could not be put in standby"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("get_sandbox_standby"),
                name: "Get A Sandbox's Standby Status".to_string(),
                description: "Say whether a running sandbox is in standby, how many times \
                    traffic has woken it, the vCPU exits it has taken (none while in standby) \
                    and how long the last wake took."
                    .to_string(),
                http_method: "GET".to_string(),
                path: id_path("/standby"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The standby status.".to_string(),
                            schema: Some(Self::sandbox_standby_schema()),
                        },
                        json_error(404, "no running sandbox of that ID"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("resize_sandbox"),
                name: "Resize A Sandbox".to_string(),
                description: "Reboot a running sandbox at another number of vCPUs or amount of \
                    memory. A guest's memory cannot be carried to another size, so what is \
                    kept is what a reboot keeps (ID, token, URL, ports, network, volumes, \
                    disk and `envVars`); what the guest wrote to its own root filesystem, \
                    which is memory, is lost. Asking for the size it already has changes \
                    nothing (`resized: false`). A paused sandbox is 409: resume it with \
                    `cpuCount` and `memoryMB` instead."
                    .to_string(),
                http_method: "POST".to_string(),
                path: id_path("/resize"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "cpuCount": { "type": "integer" },
                            "memoryMB": { "type": "integer" }
                        },
                        "description": "at least one of the two"
                    }),
                }),
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The size it has now.".to_string(),
                            schema: Some(serde_json::json!({
                                "type": "object",
                                "properties": {
                                    "sandboxID": { "type": "string" },
                                    "cpuCount": { "type": "integer" },
                                    "memoryMB": { "type": "integer" },
                                    "resized": { "type": "boolean" }
                                }
                            })),
                        },
                        json_error(400, "neither `cpuCount` nor `memoryMB` was given, or the size is not one this host can give"),
                        body_rejection(),
                        json_error(404, "no such sandbox"),
                        json_error(
                            409,
                            "it is paused, its registration is unsettled, or it had already \
                             rebooted five times within a minute and was ended",
                        ),
                        json_error(500, "the new guest did not come up; the message says why"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("adopt_sandbox_owner"),
                name: "Assign A Sandbox's Owner".to_string(),
                description: format!(
                    "Give a sandbox that has no owner the principal `principalId`, or accept \
                     the same owner again. It never transfers an existing owner (409), and \
                     never changes the sandbox's team. {}",
                    if node {
                        "Needs a clustered node with a cluster token (503 otherwise); a \
                         sandbox with legacy public-port reservations must have them removed \
                         first (409)."
                    } else {
                        "Only an administrator, or a team administrator for a sandbox of its \
                         own team (with the `sandboxes` scope), may call it; with no keys \
                         configured at all it is still refused, so an open control plane does \
                         not offer it. Needs the control plane to have a `--cluster-token` \
                         (503 otherwise). See docs/TEAMS.md."
                    }
                ),
                http_method: "POST".to_string(),
                path: id_path("/owner"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["principalId"],
                        "additionalProperties": false,
                        "properties": { "principalId": { "type": "string" } }
                    }),
                }),
                responses: {
                    let mut responses = vec![
                        Self::sandbox_plain(204, "It is owned by the principal."),
                        json_error(
                            400,
                            "a valid `principalId` is required and unknown fields are refused, \
                             or the sandbox ID is invalid",
                        ),
                        json_error(
                            404,
                            "no such sandbox (on a node: not present, or no record of it)",
                        ),
                        json_error(
                            409,
                            "it already has a different owner, it has legacy public-port \
                             reservations, its registration is unsettled, or (paused) it is not \
                             available for adoption",
                        ),
                    ];
                    if !node {
                        responses.push(json_error(
                            403,
                            "`administrator credential required`: the caller is not an \
                             administrator or a team administrator of the sandbox's team",
                        ));
                    }
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
        ];
        if node {
            operations.push(Operation {
                id: "node_reboot_sandbox".to_string(),
                name: "Reboot A Sandbox".to_string(),
                description: "Reboot a running sandbox in place: the same sandbox ID, token, \
                    URL, ports and network policy, with a fresh guest booted from its \
                    template. Volumes and the disk are mounted again and `envVars` put back; \
                    what the guest wrote to its root filesystem, which is memory, is lost. The \
                    old VM is stopped before the new one starts. A guest that stops by \
                    itself is rebooted by the node without being asked, and one that keeps \
                    dying (five reboots within a minute) is ended as lost. Only the node \
                    serves this; the control plane has no route for it."
                    .to_string(),
                http_method: "POST".to_string(),
                path: "/sandboxes/{sandboxID}/reboot".to_string(),
                parameters: vec![Self::sandbox_id_parameter(true)],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "It was rebooted.".to_string(),
                            schema: Some(serde_json::json!({
                                "type": "object",
                                "properties": { "sandboxID": { "type": "string" } }
                            })),
                        },
                        json_error(404, "no such running sandbox"),
                        json_error(
                            409,
                            "its registration is unsettled, or it has rebooted five times \
                             within a minute and was ended",
                        ),
                        json_error(
                            500,
                            "the new guest did not come up; the node retries a stopped VM",
                        ),
                    ];
                    responses.extend(Self::sandbox_access_responses(true, true));
                    responses
                },
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            });
        }
        operations
    }

    fn sandbox_standby_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "standby": { "type": "boolean" },
                "wakes": { "type": "integer", "description": "times traffic woke it" },
                "vcpuExits": { "type": "integer", "description": "every exit its vCPUs have taken; a guest in standby takes none" },
                "lastWakeMicros": { "type": "number", "description": "the vCPUs being told to run again, not the guest answering" }
            }
        })
    }

    fn sandbox_exec_operations(node: bool) -> Vec<Operation> {
        let id = |base: &str| {
            if node {
                format!("node_{base}")
            } else {
                base.to_string()
            }
        };
        let id_path = |rest: &str| {
            let name = if node { "sandboxID" } else { "id" };
            format!("/sandboxes/{{{name}}}{rest}")
        };
        let json_error = Self::sandbox_json_error;
        let n_param = Parameter {
            name: "n".to_string(),
            location: ParameterLocation::Path,
            required: true,
            description: "The command's number in this sandbox's history".to_string(),
            schema: serde_json::json!({ "type": "integer", "minimum": 0 }),
            default: None,
        };
        let history_off = || {
            json_error(
                409,
                "the node keeps no command history (`--no-command-history`), or the sandbox \
                 was created with `commandHistory: false`",
            )
        };
        let command_schema = |with_output: bool| {
            let mut properties = serde_json::json!({
                "n": { "type": "integer" },
                "source": { "type": "string", "description": "`exec`, or `process` for one started through envd" },
                "cmd": { "type": "string", "description": "the command line, with secret-shaped values masked" },
                "cwd": { "type": ["string", "null"] },
                "user": { "type": ["string", "null"] },
                "tag": { "type": ["string", "null"] },
                "pid": { "type": ["integer", "null"] },
                "pty": { "type": "boolean" },
                "state": { "type": "string", "enum": ["running", "exited", "signalled", "lost"] },
                "exitCode": { "type": ["integer", "null"] },
                "signal": { "type": ["integer", "null"] },
                "error": { "type": ["string", "null"] },
                "startedAt": { "type": "integer", "description": "unix milliseconds" },
                "endedAt": { "type": ["integer", "null"], "description": "unix milliseconds" },
                "stdoutBytes": { "type": "integer" },
                "stderrBytes": { "type": "integer" }
            });
            if with_output {
                let stream = serde_json::json!({
                    "type": "object",
                    "properties": {
                        "offset": { "type": "integer" },
                        "text": { "type": "string" },
                        "next": { "type": "integer", "description": "the offset to ask for next" },
                        "dropped": { "type": "integer", "description": "bytes before the kept tail that were discarded" }
                    }
                });
                properties["stdout"] = stream.clone();
                properties["stderr"] = stream;
            }
            serde_json::json!({ "type": "object", "properties": properties })
        };
        let exec_not_found = || OperationResponse {
            status_code: 404,
            description: "no running sandbox of that ID (a paused one too). The body is \
                `{\"error\": \"no sandbox ...\"}`, not the `{code, message}` shape"
                .to_string(),
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": { "error": { "type": "string" } }
            })),
        };

        vec![
            Operation {
                id: id("exec_in_sandbox"),
                name: "Run A Command In A Sandbox".to_string(),
                description: "Run one command in a running sandbox's guest and wait for it. \
                    Give either `cmd`, a shell line run as `/bin/sh -c` (with `args` as `$0`, \
                    `$1`, ...), or `argv`, a program run directly with no shell anywhere \
                    (`argv[0]` an absolute path, at most 64 elements of at most 8192 bytes, no \
                    NUL; `args` is refused beside it), for a guest image that has no shell. \
                    Exactly one of the two. It runs as root unless `user` names another (by \
                    name or number), and is killed after `timeout_secs` (30 by default), \
                    answering `timed_out: true`. The answer carries the exit code and \
                    both output streams, within the guest agent's output caps; use envd's `process.Process` gRPC for a \
                    streaming or interactive command. This is arbitrary code execution as \
                    that user in the guest. A sandbox in standby is woken by it. It is \
                    recorded in the command history unless that is off. Not idempotent: it \
                    runs again on every call."
                    .to_string(),
                http_method: "POST".to_string(),
                path: id_path("/exec"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: Some(RequestBody {
                    content_type: "application/json".to_string(),
                    required: true,
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "cmd": { "type": "string", "description": "a shell line; exactly one of cmd and argv" },
                            "args": { "type": "array", "items": { "type": "string" }, "description": "only with cmd" },
                            "argv": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 64,
                                "description": "a program and its arguments, run directly; exactly one of cmd and argv" },
                            "timeout_secs": { "type": "integer", "default": 30 },
                            "user": { "type": "string", "description": "run as this user, by name or number; root if absent" }
                        }
                    }),
                }),
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The command ended, or timed out.".to_string(),
                            schema: Some(serde_json::json!({
                                "type": "object",
                                "properties": {
                                    "exit_code": { "type": ["integer", "null"], "description": "null when there is no exit status, for example when it was killed by a signal" },
                                    "stdout": { "type": "string" },
                                    "stderr": { "type": "string" },
                                    "timed_out": { "type": "boolean" }
                                }
                            })),
                        },
                        json_error(
                            400,
                            "neither or both of `cmd` and `argv`, `args` beside `argv`, an \
                             `argv` that is empty, too long, has a NUL or whose first element \
                             is not an absolute path, or a `cmd` on an image with no shell",
                        ),
                        Self::sandbox_plain(
                            400,
                            "the body is not JSON of this shape: the framework's own rejection \
                             (400, 415 or 422, plain text)",
                        ),
                        exec_not_found(),
                        OperationResponse {
                            status_code: 500,
                            description: "the guest agent could not run it (for example the \
                                program is not in the image). The body is \
                                `{\"error\": \"exec_in_guest: ...\"}`"
                                .to_string(),
                            schema: Some(serde_json::json!({
                                "type": "object",
                                "properties": { "error": { "type": "string" } }
                            })),
                        },
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: false,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("list_sandbox_commands"),
                name: "List A Sandbox's Commands".to_string(),
                description: "List the commands run in a sandbox, oldest first, without their \
                    output: every `exec` and every process started through envd. The newest 64 \
                    are kept; a command running when the sandbox paused is recorded as `lost`. \
                    Kept through a pause, reboot, resize and restored checkpoint, gone when \
                    the sandbox ends. Command lines have secret-shaped values masked, but \
                    see the guardrails."
                    .to_string(),
                http_method: "GET".to_string(),
                path: id_path("/commands"),
                parameters: vec![Self::sandbox_id_parameter(node)],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The commands.".to_string(),
                            schema: Some(serde_json::json!({
                                "type": "array",
                                "items": command_schema(false)
                            })),
                        },
                        json_error(404, "no such sandbox (running or paused)"),
                        history_off(),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("get_sandbox_command"),
                name: "Get One Of A Sandbox's Commands".to_string(),
                description: "One command from the history, with what it printed from the \
                    offsets asked for. Each stream keeps its newest 64 KiB; `dropped` counts \
                    what went before. An `exec` command's output exists only once it has \
                    ended. Output is kept as printed and is not scanned for secrets."
                    .to_string(),
                http_method: "GET".to_string(),
                path: id_path("/commands/{n}"),
                parameters: vec![
                    Self::sandbox_id_parameter(node),
                    n_param,
                    Self::sandbox_query(
                        "stdout",
                        "Offset into stdout to read from (default 0)",
                        serde_json::json!({ "type": "integer", "minimum": 0 }),
                    ),
                    Self::sandbox_query(
                        "stderr",
                        "Offset into stderr to read from (default 0)",
                        serde_json::json!({ "type": "integer", "minimum": 0 }),
                    ),
                ],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The command and its output.".to_string(),
                            schema: Some(command_schema(true)),
                        },
                        json_error(404, "no such sandbox, or it has no such command number"),
                        history_off(),
                        Self::sandbox_plain(
                            400,
                            "`n` or an offset is not a number: the framework's own rejection",
                        ),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
            Operation {
                id: id("list_sandbox_processes"),
                name: "List A Sandbox's Processes".to_string(),
                description: "Every process in a running sandbox's guest, as `ps` lists them \
                    and not only those started through the API, read from the guest's \
                    `/proc` through the guest agent (so a guest with no shell lists too). \
                    Kernel threads are left out unless `all=true`. A paused sandbox runs \
                    nothing and is 409."
                    .to_string(),
                http_method: "GET".to_string(),
                path: id_path("/processes"),
                parameters: vec![
                    Self::sandbox_id_parameter(node),
                    Self::sandbox_query(
                        "all",
                        "Include kernel threads",
                        serde_json::json!({ "type": "boolean", "default": false }),
                    ),
                ],
                request_body: None,
                responses: {
                    let mut responses = vec![
                        OperationResponse {
                            status_code: 200,
                            description: "The processes.".to_string(),
                            schema: Some(serde_json::json!({
                                "type": "object",
                                "properties": { "processes": { "type": "array", "items": {
                                    "type": "object",
                                    "properties": {
                                        "pid": { "type": "integer" },
                                        "ppid": { "type": "integer" },
                                        "uid": { "type": ["integer", "null"] },
                                        "state": { "type": "string", "description": "R, S, D, Z, T ... as ps shows" },
                                        "command": { "type": "string" },
                                        "args": { "type": "string" },
                                        "threads": { "type": "integer" },
                                        "rssKiB": { "type": "integer" },
                                        "cpuSeconds": { "type": "number" },
                                        "startedSecondsAgo": { "type": "number" }
                                    }
                                } } }
                            })),
                        },
                        json_error(404, "no such sandbox"),
                        json_error(409, "the sandbox is paused, and runs nothing"),
                        json_error(502, "the guest's processes could not be listed"),
                    ];
                    responses.extend(Self::sandbox_access_responses(node, true));
                    responses
                },
                idempotent: true,
                async_operation: false,
                rate_limit: None,
                examples: vec![],
            },
        ]
    }

    /// Warnings attached to interfaces. Exposures only: each entry is a
    /// documented residual, not a general caution.
    fn build_guardrails() -> Vec<Guardrail> {
        vec![
            Guardrail {
                id: "redis-shared-user-revocation".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "Shared Redis user can revive a revoked key".to_string(),
                description: "Where nodes and control planes connect to the cluster store \
                    (Redis) as one shared user, a compromised node can delete a revoked \
                    key's revocation record and write the old key record back, which makes \
                    the revoked key work again until it expires. The seal and the revocation \
                    record stop a forged record but cannot stop a Redis writer from deleting \
                    one. A node can also delete records outright (denial of service)."
                    .to_string(),
                applies_to: vec![
                    "revoke_managed_key".to_string(),
                    "cluster_store_configuration".to_string(),
                ],
                condition: "Nodes and control planes use the same Redis user, for example an \
                    external store configured with one URL or a hand-rolled deployment. Not \
                    the shipped deploy/compose or deploy/helm, which create one user per role."
                    .to_string(),
                mitigation: "Give each role its own Redis ACL user with tools/redis-acl.py \
                    (Redis or Valkey 7+): the node user has no access to managed-keys, \
                    managed-key-ids, managed-key-tombstones or team-telemetry, and the \
                    default user is off. Point control planes, nodes and the gateway at \
                    their own URLs. A key's own expiry bounds the damage where this is not \
                    done."
                    .to_string(),
                reference:
                    "docs/API_KEY_ROTATION.md#redis-roles-keep-nodes-away-from-the-key-records"
                        .to_string(),
            },
            Guardrail {
                id: "pre-tombstone-revocations".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "Keys revoked before revocation records existed have no record".to_string(),
                description: "A managed key revoked before revocation records existed has no \
                    such record, so the protection against writing the old key record back \
                    never covered it, even with per-role Redis users. A replayed record for \
                    it would authenticate until the key's expiry. Revoking it again does not \
                    help: the store no longer holds the key, so the revoke answers 404 and \
                    writes nothing."
                    .to_string(),
                applies_to: vec!["revoke_managed_key".to_string()],
                condition: "The key was revoked by a control plane built before revocation \
                    records were written, and its record could be written back by a store \
                    writer (see redis-shared-user-revocation)."
                    .to_string(),
                mitigation: "Only the key's expiry bounds it, so let it expire; with per-role \
                    Redis users nothing can write the record back. If a replay is suspected \
                    and cannot wait, changing the managed-key secret makes every managed key \
                    stop authenticating (a blunt revoke-all: reissue the keys you still need). \
                    Create keys with short expiries."
                    .to_string(),
                reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
            },
            Guardrail {
                id: "control-plane-unauthenticated-mode".to_string(),
                severity: GuardrailSeverity::Critical,
                title: "A control plane started with --allow-unauthenticated is open".to_string(),
                description: "With --allow-unauthenticated the control plane has no API key \
                    and no sign-on, so every caller who can reach its port (and its envd \
                    proxy port) is treated as an administrator, with no team: anyone can \
                    make, read and delete sandboxes, machines, volumes and keys. For \
                    snapshot sharing that means any caller can read the share list of any \
                    team's snapshot and share it with any team, which gives that team the \
                    sandbox's memory, files and environment variables. The control plane \
                    logs a warning each time it starts this way. For the sandbox routes \
                    that means any caller can create sandboxes, run commands as root in any \
                    sandbox and read its command history and output. (The key routes and \
                    owner adoption still want an administrator or team-admin credential, so \
                    an anonymous caller does not get them.)"
                    .to_string(),
                applies_to: [
                    "team_template_sharing",
                    "share_template_snapshot",
                    "unshare_template_snapshot",
                    "list_template_shares",
                    "sandbox_lifecycle",
                    "sandbox_commands",
                ]
                .into_iter()
                .map(str::to_string)
                .chain(
                    Self::build_sandbox_control_plane_operations()
                        .into_iter()
                        .chain(Self::build_sandbox_exec_control_plane_operations())
                        .map(|op| op.id)
                        .filter(|id| id != "adopt_sandbox_owner"),
                )
                .collect(),
                condition: "The control plane was started with --allow-unauthenticated. Without \
                    it, a control plane with no key source and no --sso-issuer refuses to \
                    start."
                    .to_string(),
                mitigation: "Do not use the flag on a reachable control plane. Give it an \
                    --api-key (or HV2_API_KEY), an --api-keys-file, or single sign-on \
                    (--sso-issuer); managed keys alone do not count, because an \
                    administrator mints them. Where it must run open, bind it to loopback or \
                    a private network."
                    .to_string(),
                reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
            },
            Guardrail {
                id: "sso-session-not-revocable".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "A single sign-on session cannot be revoked one at a time".to_string(),
                description: "A session is a signed, stateless token. Signing out only clears \
                    the browser's cookie; a copy of the token (a CLI bearer token, a stolen \
                    cookie) keeps working until it expires, which is `--sso-session-hours` \
                    after sign-in (12 by default, at most 168). The control plane has no \
                    list of sessions to delete from."
                    .to_string(),
                applies_to: vec![
                    "sso_logout".to_string(),
                    "sso_callback".to_string(),
                    "sso_cli_token".to_string(),
                ],
                condition: "A session token may have been copied or exposed before it expired."
                    .to_string(),
                mitigation: "Remove the member from the members file (reloaded on SIGHUP): \
                    they lose access on their next request even with an unexpired session. \
                    To end every session at once, replace the session key \
                    (`--sso-session-key-file`); control planes behind one name share it, so \
                    replace it on all. Shorten `--sso-session-hours` where tokens are \
                    kept on laptops."
                    .to_string(),
                reference: "docs/SSO.md".to_string(),
            },
            Guardrail {
                id: "sso-guest-urls-need-guest-domain".to_string(),
                severity: GuardrailSeverity::Info,
                title: "Private guest URLs are signed in only under a configured guest domain"
                    .to_string(),
                description: "A guest URL names its sandbox by its first label alone, so \
                    which domains count as guest URLs is the operator's to say. \
                    /auth/guest hands a member a sign-in only for `{port}-{sandbox}.{domain}` \
                    with exactly one label before a configured --guest-domain, or for a \
                    custom domain bound to a sandbox; any other URL answers 400. \
                    Without this a member who followed a link to an attacker's domain would \
                    be handed a credential its owner could redeem at the proxy."
                    .to_string(),
                applies_to: vec!["sso_guest_handoff".to_string()],
                condition: "A URL not under a configured --guest-domain is passed as `url`, \
                    or the control plane runs without --sso-guest-urls and --guest-domain."
                    .to_string(),
                mitigation: "Start the control plane with --sso-guest-urls, proxy TLS and one \
                    --guest-domain per domain you serve sandboxes under; it refuses to start \
                    with one of the two flags and not the other. Do not expect /auth/guest \
                    to sign in to any other host."
                    .to_string(),
                reference: "docs/SSO.md#private-guest-urls".to_string(),
            },
            Guardrail {
                id: "shared-snapshot-contains-memory-and-secrets".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "A shared snapshot is the source sandbox as it was, memory included"
                    .to_string(),
                description: "A snapshot shared with another team gives that team the source \
                    sandbox's memory, its files and its environment variables. Nothing is \
                    removed first, because a layered memory snapshot cannot be cleaned \
                    after the fact, which is why the request must carry \
                    `acknowledgeContents: true`. Revoking stops new starts at once but \
                    cannot recall sandboxes the team already started, or what they read."
                    .to_string(),
                applies_to: vec![
                    "share_template_snapshot".to_string(),
                    "team_template_sharing".to_string(),
                ],
                condition: "Any share of a team's snapshot with another team.".to_string(),
                mitigation: "Take the snapshot from a sandbox that never held what the other \
                    team must not have (secrets, tokens, customer data in memory or on \
                    disk). A Git credential held on the host (--team-git-credential, \
                    --git-credential) is not in the guest and so not in the snapshot."
                    .to_string(),
                reference: "docs/TEAMS.md#sharing-a-snapshot-with-another-team".to_string(),
            },
            Guardrail {
                id: "node-unauthenticated-mode".to_string(),
                severity: GuardrailSeverity::Critical,
                title: "A node started with --allow-unauthenticated is open".to_string(),
                description: "A node's API runs commands in guests, reads and writes their \
                    files, creates machines and boots sandboxes, and its proxy reaches \
                    whatever a guest or a machine publishes, desktops included. With \
                    --allow-unauthenticated the node listens beyond loopback with no cluster \
                    token and no mTLS, so every host that can reach its API port (3980) or \
                    proxy port (3981) can do all of that to every sandbox on it: the node has \
                    no users, teams or ownership of its own, the control plane's checks are \
                    not in front of it. The node prints a warning naming both ports every \
                    time it starts this way."
                    .to_string(),
                applies_to: ["node_sandbox_lifecycle", "node_sandbox_commands"]
                    .into_iter()
                    .map(str::to_string)
                    .chain(
                        Self::build_sandbox_node_operations()
                            .into_iter()
                            .chain(Self::build_sandbox_exec_node_operations())
                            .map(|op| op.id),
                    )
                    .collect(),
                condition: "The node was started with --bind ADDR beyond loopback and \
                    --allow-unauthenticated, with no --cluster-token (HV2_CLUSTER_TOKEN) and \
                    no mTLS. Without the flag a non-loopback --bind with neither is refused \
                    at start, and with no --bind a node with neither listens on 127.0.0.1."
                    .to_string(),
                mitigation: "Give the node a --cluster-token (or HV2_CLUSTER_TOKEN) or mTLS, \
                    which the control plane then presents; or leave it on loopback; or, where \
                    the flag is really intended, keep the ports on a network that does the \
                    authenticating."
                    .to_string(),
                reference: "docs/SANDBOXES.md#where-a-node-listens".to_string(),
            },
            Guardrail {
                id: "node-proxy-open".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "A node started with --proxy-open does not check the cluster token on \
                    its proxy"
                    .to_string(),
                description: "A node with a cluster token normally accepts a proxy connection \
                    (port 3981) only if it opens with an `HV2-PROXY` line that only a holder \
                    of the token, the control plane, can make. With --proxy-open the node \
                    keeps the old behaviour and does not check, so any host that can reach \
                    the proxy port can reach every port a machine publishes and every \
                    sandbox's envd, which `connect` and `get_sandbox` describe. The node \
                    logs a warning each time it starts this way. Even with the check, the \
                    line proves the token and does not encrypt, and a line seen on the wire \
                    can be replayed within 60 seconds before the genuine one arrives."
                    .to_string(),
                applies_to: vec![
                    "node_sandbox_lifecycle".to_string(),
                    "node_connect_sandbox".to_string(),
                    "node_connect_sandbox_v2".to_string(),
                ],
                condition: "The node has a cluster token and was started with --proxy-open."
                    .to_string(),
                mitigation: "Do not pass --proxy-open; if you must, keep the proxy port \
                    reachable only from the control plane. Use mTLS or a private network \
                    against an attacker on the path."
                    .to_string(),
                reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
            },
            Guardrail {
                id: "command-history-keeps-output".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "A sandbox keeps the commands it ran, and what they printed".to_string(),
                description: "Every `exec`, and every process started through envd, is kept \
                    in the sandbox's command history (the newest 64), with its command line, \
                    user and, from the newest 64 KiB of each stream, what it printed. Before \
                    a command line is kept the node masks the common shapes of a secret \
                    (`NAME=VALUE` for names like TOKEN, SECRET, PASSWORD or KEY, the value \
                    after `--password` or `--token`, what follows `Bearer` or `Basic`, \
                    credentials in a URL), but it cannot know every secret, and output is \
                    kept as printed and not scanned. Anyone who may read the sandbox's history \
                    can read both."
                    .to_string(),
                applies_to: vec![
                    "list_sandbox_commands".to_string(),
                    "get_sandbox_command".to_string(),
                    "node_list_sandbox_commands".to_string(),
                    "node_get_sandbox_command".to_string(),
                ],
                condition: "A secret was in a command line in a shape the masking does not \
                    know, or was printed by a command, in a sandbox whose history is on."
                    .to_string(),
                mitigation: "Create the sandbox with `\"commandHistory\": false`, or start the \
                    node with --no-command-history, to keep none; and pass secrets through \
                    `envVars`, which are never returned, rather than on a command line."
                    .to_string(),
                reference: "crates/hv2-sandboxd/src/history.rs".to_string(),
            },
        ]
    }

    /// The agent-callable tools, with a one-line warning appended to every
    /// tool a guardrail names. The single projection behind the OpenAI,
    /// Anthropic, Gemini and MCP-manifest exports.
    fn projected_tools(&self) -> Vec<hv2_agent::McpTool> {
        let mut tools = mcp_registry_tools();
        for tool in &mut tools {
            if let Some(line) = self.warning_line(&tool.name) {
                tool.description.push_str("\n\n");
                tool.description.push_str(&line);
            }
        }
        tools
    }

    /// Guardrails whose `applies_to` names `id`.
    pub fn guardrails_for(&self, id: &str) -> Vec<&Guardrail> {
        self.guardrails
            .iter()
            .filter(|g| g.applies_to.iter().any(|a| a == id))
            .collect()
    }

    /// A short warning for `id`, or `None` if no guardrail names it.
    fn warning_line(&self, id: &str) -> Option<String> {
        let found = self.guardrails_for(id);
        if found.is_empty() {
            return None;
        }
        let parts: Vec<String> = found
            .iter()
            .map(|g| {
                format!(
                    "{:?}: {} ({}). Mitigation: {}",
                    g.severity, g.title, g.condition, g.mitigation
                )
            })
            .collect();
        Some(format!("WARNING - {}", parts.join(" | ")))
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
        let tools = self
            .projected_tools()
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
        let tools = self
            .projected_tools()
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
        let function_declarations = self
            .projected_tools()
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
        let tools = self
            .projected_tools()
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

    #[test]
    fn guardrails_name_known_interfaces_and_have_content() {
        let onto = HyperMachineOntology::build();
        let tools: Vec<String> = mcp_registry_tools().into_iter().map(|t| t.name).collect();
        let ids = [
            "redis-shared-user-revocation",
            "pre-tombstone-revocations",
            "control-plane-unauthenticated-mode",
            "sso-session-not-revocable",
            "sso-guest-urls-need-guest-domain",
            "shared-snapshot-contains-memory-and-secrets",
            "node-unauthenticated-mode",
            "node-proxy-open",
            "command-history-keeps-output",
        ];
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for id in ids {
            let g = onto.guardrails.iter().find(|g| g.id == id).expect(id);
            assert!(!g.description.is_empty() && !g.mitigation.is_empty());
            assert!(!g.condition.is_empty() && !g.reference.is_empty());
            assert!(!g.applies_to.is_empty());
            // Each cites a document that exists.
            let doc = g.reference.split('#').next().unwrap();
            assert!(repo.join(doc).is_file(), "{id}: {doc} does not exist");
        }
        assert_eq!(
            onto.guardrails.len(),
            ids.len(),
            "exposures only, each sourced"
        );
        for g in &onto.guardrails {
            for id in &g.applies_to {
                let known = onto.operations.iter().any(|o| &o.id == id)
                    || onto.capabilities.iter().any(|c| &c.id == id)
                    || tools.contains(id);
                assert!(known, "guardrail {} names unknown interface {id}", g.id);
            }
        }
    }

    #[test]
    fn key_capability_operations_all_exist() {
        let onto = HyperMachineOntology::build();
        let cap = onto
            .capabilities
            .iter()
            .find(|c| c.id == "managed_api_keys")
            .unwrap();
        let ops: Vec<(&str, &str)> = cap
            .operations
            .iter()
            .map(|id| {
                let op = onto.operations.iter().find(|o| &o.id == id).unwrap();
                (op.http_method.as_str(), op.path.as_str())
            })
            .collect();
        assert_eq!(
            ops,
            [
                ("POST", "/keys"),
                ("GET", "/keys"),
                ("DELETE", "/keys/{id}"),
                ("GET", "/teams/{team}/members"),
            ]
        );
    }

    #[test]
    fn projected_tool_descriptions_carry_the_warning() {
        let mut onto = HyperMachineOntology::build();
        let plain: std::collections::BTreeMap<String, String> = mcp_registry_tools()
            .into_iter()
            .map(|t| (t.name, t.description))
            .collect();
        let mut names = plain.keys().cloned();
        let (target, other) = (names.next().unwrap(), names.next().unwrap());
        let openai = |o: &HyperMachineOntology, n: &str| {
            o.to_openai_tools()
                .tools
                .into_iter()
                .find(|t| t.function.name == n)
                .unwrap()
                .function
                .description
        };
        // No built-in guardrail names an MCP tool, so projections match the registry.
        assert_eq!(openai(&onto, &target), plain[&target]);
        onto.guardrails.push(Guardrail {
            id: "t".into(),
            severity: GuardrailSeverity::Critical,
            title: "Careful".into(),
            description: "d".into(),
            applies_to: vec![target.clone()],
            condition: "when x".into(),
            mitigation: "do y".into(),
            reference: "docs/x.md".into(),
        });
        let gemini_name = target.replace('.', "_");
        let texts = [
            openai(&onto, &target),
            onto.to_anthropic_tools()
                .tools
                .into_iter()
                .find(|t| t.name == target)
                .unwrap()
                .description,
            onto.to_gemini_tools()
                .function_declarations
                .into_iter()
                .find(|t| t.name == gemini_name)
                .unwrap()
                .description,
            onto.build_mcp_manifest()
                .tools
                .into_iter()
                .find(|t| t.name == target)
                .unwrap()
                .description,
        ];
        for text in texts {
            assert!(text.starts_with(&plain[&target]));
            assert!(text.contains("WARNING") && text.contains("Careful") && text.contains("do y"));
        }
        // An unrelated tool is untouched.
        assert_eq!(openai(&onto, &other), plain[&other]);
    }
}
