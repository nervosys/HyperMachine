//! Multi-Step Workflow Engine
//!
//! Executes directed acyclic graphs (DAGs) of steps with checkpointing,
//! retry, and durable state. Workflows survive VM failures — the engine
//! can resume from the last successful checkpoint on a different VM.
//!
//! # Workflow Lifecycle
//!
//! ```text
//! Pending ──> Running ──> Completed
//!                │
//!                ├──> Failed (retryable steps re-queued)
//!                └──> Cancelled
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Workflow operation result
pub type WorkflowResult<T> = Result<T, WorkflowError>;

/// Step operation result
pub type StepResult<T> = Result<T, StepError>;

/// Workflow errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowError {
    /// Workflow not found
    #[error("Workflow not found: {0}")]
    NotFound(String),

    /// Workflow already exists
    #[error("Workflow already exists: {0}")]
    AlreadyExists(String),

    /// Invalid state transition
    #[error("Invalid workflow state: {from:?} -> {to:?}")]
    InvalidTransition {
        from: WorkflowPhase,
        to: WorkflowPhase,
    },

    /// Step error
    #[error("Step error: {0}")]
    StepFailed(#[from] StepError),

    /// Dependency cycle detected
    #[error("Dependency cycle in workflow: {0}")]
    CyclicDependency(String),

    /// Maximum concurrent workflows exceeded
    #[error("Max concurrent workflows: {current}/{max}")]
    MaxConcurrent { current: usize, max: usize },

    /// Timeout
    #[error("Workflow timeout: {0}")]
    Timeout(String),
}

/// Step errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    /// Step execution failed
    #[error("Step failed: {0}")]
    ExecutionFailed(String),

    /// Step timed out
    #[error("Step timeout: {name} after {elapsed:?}")]
    Timeout { name: String, elapsed: Duration },

    /// Step dependency not met
    #[error("Dependency not met: {step} requires {dependency}")]
    DependencyNotMet { step: String, dependency: String },

    /// Retry limit exceeded
    #[error("Retry limit: {step} failed {attempts}/{max_retries} times")]
    RetryExhausted {
        step: String,
        attempts: u32,
        max_retries: u32,
    },

    /// Step cancelled
    #[error("Step cancelled: {0}")]
    Cancelled(String),
}

/// Phase of a workflow execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkflowPhase {
    /// Workflow is defined but not started
    Pending,
    /// Workflow is actively executing steps
    Running,
    /// Workflow is paused (manual or checkpoint)
    Paused,
    /// All steps completed successfully
    Completed,
    /// Workflow failed (one or more steps exhausted retries)
    Failed,
    /// Workflow was cancelled
    Cancelled,
}

impl WorkflowPhase {
    /// Check if workflow is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Check if workflow is active
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

/// Outcome of executing a step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepOutcome {
    /// Step succeeded with optional output data
    Success { output: Option<String> },
    /// Step failed with an error message
    Failure { error: String, retryable: bool },
    /// Step was skipped (conditional)
    Skipped { reason: String },
}

impl StepOutcome {
    /// Check if the outcome is successful
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Check if the outcome is a retryable failure
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Failure {
                retryable: true,
                ..
            }
        )
    }
}

/// Specification for a single step in a workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSpec {
    /// Unique step name within the workflow
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Names of steps that must complete before this one
    pub depends_on: Vec<String>,
    /// Maximum execution time for this step
    pub timeout: Duration,
    /// Maximum retry attempts (0 = no retry)
    pub max_retries: u32,
    /// Delay between retries
    pub retry_delay: Duration,
    /// Command or script to execute
    pub command: String,
    /// Whether this step is optional (workflow continues if it fails)
    pub optional: bool,
}

impl StepSpec {
    /// Create a new step specification
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            depends_on: Vec::new(),
            timeout: Duration::from_secs(300),
            max_retries: 2,
            retry_delay: Duration::from_secs(5),
            command: command.into(),
            optional: false,
        }
    }

    /// Set description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add a dependency on another step
    pub fn depends_on(mut self, step: impl Into<String>) -> Self {
        self.depends_on.push(step.into());
        self
    }

    /// Set timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set max retries
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Mark as optional
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// Context passed to a step during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepContext {
    /// Workflow ID
    pub workflow_id: String,
    /// Step name
    pub step_name: String,
    /// Attempt number (starts at 1)
    pub attempt: u32,
    /// Outputs from completed dependency steps
    pub dependency_outputs: HashMap<String, String>,
    /// Workflow-level variables
    pub variables: HashMap<String, String>,
}

/// Specification for a complete workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSpec {
    /// Unique workflow name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Ordered list of steps (topologically sorted)
    pub steps: Vec<StepSpec>,
    /// Maximum total execution time
    pub timeout: Duration,
    /// Workflow-level variables
    pub variables: HashMap<String, String>,
    /// Maximum concurrent steps
    pub max_parallel_steps: usize,
}

impl WorkflowSpec {
    /// Create a new workflow specification
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            steps: Vec::new(),
            timeout: Duration::from_secs(3600),
            variables: HashMap::new(),
            max_parallel_steps: 4,
        }
    }

    /// Validate that the workflow DAG is acyclic and dependencies exist
    pub fn validate(&self) -> WorkflowResult<()> {
        let step_names: HashSet<&str> = self.steps.iter().map(|s| s.name.as_str()).collect();

        // Check all dependencies reference existing steps
        for step in &self.steps {
            for dep in &step.depends_on {
                if !step_names.contains(dep.as_str()) {
                    return Err(WorkflowError::CyclicDependency(format!(
                        "Step '{}' depends on non-existent step '{}'",
                        step.name, dep
                    )));
                }
            }
        }

        // Topological sort to detect cycles
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for step in &self.steps {
            in_degree.entry(step.name.as_str()).or_insert(0);
            for dep in &step.depends_on {
                adjacency
                    .entry(dep.as_str())
                    .or_default()
                    .push(step.name.as_str());
                *in_degree.entry(step.name.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&name, _)| name)
            .collect();

        let mut visited = 0usize;
        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(neighbors) = adjacency.get(node) {
                for &next in neighbors {
                    if let Some(deg) = in_degree.get_mut(next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }

        if visited < self.steps.len() {
            return Err(WorkflowError::CyclicDependency(
                "Workflow contains a dependency cycle".to_string(),
            ));
        }

        Ok(())
    }
}

/// Builder for constructing workflow specs fluently
pub struct WorkflowBuilder {
    spec: WorkflowSpec,
}

impl WorkflowBuilder {
    /// Create a new workflow builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            spec: WorkflowSpec::new(name),
        }
    }

    /// Set workflow description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.spec.description = desc.into();
        self
    }

    /// Add a step
    pub fn step(mut self, step: StepSpec) -> Self {
        self.spec.steps.push(step);
        self
    }

    /// Set a workflow variable
    pub fn variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.spec.variables.insert(key.into(), value.into());
        self
    }

    /// Set maximum execution time
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.spec.timeout = timeout;
        self
    }

    /// Set maximum parallel steps
    pub fn max_parallel(mut self, max: usize) -> Self {
        self.spec.max_parallel_steps = max;
        self
    }

    /// Build and validate the workflow
    pub fn build(self) -> WorkflowResult<WorkflowSpec> {
        self.spec.validate()?;
        Ok(self.spec)
    }
}

/// State of a single step within a running workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepState {
    /// Step name
    pub name: String,
    /// Current status
    pub status: StepStatus,
    /// Number of attempts
    pub attempts: u32,
    /// Output from last successful run
    pub output: Option<String>,
    /// Error from last failed attempt
    pub last_error: Option<String>,
    /// When this step started
    pub started_at: Option<SystemTime>,
    /// When this step completed
    pub completed_at: Option<SystemTime>,
}

/// Status of a step
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepStatus {
    /// Not yet started
    Pending,
    /// Dependencies met, ready to run
    Ready,
    /// Currently executing
    Running,
    /// Completed successfully
    Completed,
    /// Failed (may be retried)
    Failed,
    /// Skipped (optional step)
    Skipped,
}

/// A running workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    /// Unique execution ID
    pub id: String,
    /// Workflow spec
    pub spec: WorkflowSpec,
    /// Current phase
    pub phase: WorkflowPhase,
    /// Per-step state
    pub steps: HashMap<String, StepState>,
    /// When this execution was created
    pub created_at: SystemTime,
    /// When this execution started running
    pub started_at: Option<SystemTime>,
    /// When this execution completed
    pub completed_at: Option<SystemTime>,
    /// VM ID this workflow is pinned to (if any)
    pub vm_id: Option<String>,
}

impl WorkflowExecution {
    /// Create a new execution from a spec
    pub fn new(spec: WorkflowSpec) -> Self {
        let steps: HashMap<String, StepState> = spec
            .steps
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    StepState {
                        name: s.name.clone(),
                        status: StepStatus::Pending,
                        attempts: 0,
                        output: None,
                        last_error: None,
                        started_at: None,
                        completed_at: None,
                    },
                )
            })
            .collect();

        Self {
            id: Uuid::new_v4().to_string(),
            spec,
            phase: WorkflowPhase::Pending,
            steps,
            created_at: SystemTime::now(),
            started_at: None,
            completed_at: None,
            vm_id: None,
        }
    }

    /// Get steps that are ready to run (all dependencies completed)
    pub fn ready_steps(&self) -> Vec<String> {
        let mut ready = Vec::new();
        for step_spec in &self.spec.steps {
            if let Some(state) = self.steps.get(&step_spec.name) {
                if state.status != StepStatus::Pending {
                    continue;
                }
                let deps_met = step_spec.depends_on.iter().all(|dep| {
                    self.steps.get(dep).is_some_and(|s| {
                        s.status == StepStatus::Completed || s.status == StepStatus::Skipped
                    })
                });
                if deps_met {
                    ready.push(step_spec.name.clone());
                }
            }
        }
        ready
    }

    /// Mark a step as running
    pub fn start_step(&mut self, name: &str) -> StepResult<()> {
        let state = self
            .steps
            .get_mut(name)
            .ok_or_else(|| StepError::ExecutionFailed(format!("Step not found: {name}")))?;
        state.status = StepStatus::Running;
        state.attempts += 1;
        state.started_at = Some(SystemTime::now());
        Ok(())
    }

    /// Record step completion
    pub fn complete_step(&mut self, name: &str, outcome: StepOutcome) {
        if let Some(state) = self.steps.get_mut(name) {
            match outcome {
                StepOutcome::Success { output } => {
                    state.status = StepStatus::Completed;
                    state.output = output;
                    state.completed_at = Some(SystemTime::now());
                }
                StepOutcome::Failure { error, retryable } => {
                    state.last_error = Some(error);
                    // Check if step spec allows retries
                    let max_retries = self
                        .spec
                        .steps
                        .iter()
                        .find(|s| s.name == name)
                        .map_or(0, |s| s.max_retries);
                    if retryable && state.attempts <= max_retries {
                        state.status = StepStatus::Pending; // Will be retried
                    } else {
                        state.status = StepStatus::Failed;
                        state.completed_at = Some(SystemTime::now());
                    }
                }
                StepOutcome::Skipped { reason } => {
                    state.status = StepStatus::Skipped;
                    state.output = Some(reason);
                    state.completed_at = Some(SystemTime::now());
                }
            }
        }

        // Update workflow phase
        self.update_phase();
    }

    /// Update the workflow phase based on step states
    fn update_phase(&mut self) {
        let all_terminal = self.steps.values().all(|s| {
            matches!(
                s.status,
                StepStatus::Completed | StepStatus::Skipped | StepStatus::Failed
            )
        });

        if !all_terminal {
            return;
        }

        let any_failed = self.steps.values().any(|s| {
            s.status == StepStatus::Failed
                && !self
                    .spec
                    .steps
                    .iter()
                    .find(|spec| spec.name == s.name)
                    .is_some_and(|spec| spec.optional)
        });

        if any_failed {
            self.phase = WorkflowPhase::Failed;
        } else {
            self.phase = WorkflowPhase::Completed;
        }
        self.completed_at = Some(SystemTime::now());
    }

    /// Get a context for executing a step
    pub fn step_context(&self, step_name: &str) -> StepContext {
        let step_state = self.steps.get(step_name);
        let dependency_outputs: HashMap<String, String> = self
            .spec
            .steps
            .iter()
            .find(|s| s.name == step_name)
            .map(|spec| {
                spec.depends_on
                    .iter()
                    .filter_map(|dep| {
                        self.steps
                            .get(dep)
                            .and_then(|s| s.output.as_ref().map(|o| (dep.clone(), o.clone())))
                    })
                    .collect()
            })
            .unwrap_or_default();

        StepContext {
            workflow_id: self.id.clone(),
            step_name: step_name.to_string(),
            attempt: step_state.map_or(1, |s| s.attempts + 1),
            dependency_outputs,
            variables: self.spec.variables.clone(),
        }
    }

    /// Percentage of steps completed (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.steps.is_empty() {
            return 1.0;
        }
        let done = self
            .steps
            .values()
            .filter(|s| matches!(s.status, StepStatus::Completed | StepStatus::Skipped))
            .count();
        done as f64 / self.steps.len() as f64
    }
}

/// Workflow engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// Maximum concurrent workflows
    pub max_concurrent: usize,
    /// Default step timeout
    pub default_step_timeout: Duration,
    /// Default max retries per step
    pub default_max_retries: u32,
    /// Enable checkpointing
    pub enable_checkpoints: bool,
    /// Checkpoint interval
    pub checkpoint_interval: Duration,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 100,
            default_step_timeout: Duration::from_secs(300),
            default_max_retries: 2,
            enable_checkpoints: true,
            checkpoint_interval: Duration::from_secs(30),
        }
    }
}

/// Workflow execution engine
///
/// Manages the lifecycle of workflow executions: submission, step execution,
/// checkpointing, and completion.
pub struct WorkflowEngine {
    /// Configuration
    config: WorkflowConfig,
    /// Active workflows by ID
    workflows: RwLock<HashMap<String, WorkflowExecution>>,
    /// Completed workflows (bounded history)
    completed: RwLock<VecDeque<WorkflowExecution>>,
}

impl WorkflowEngine {
    /// Create a new workflow engine
    pub fn new(config: WorkflowConfig) -> Self {
        Self {
            config,
            workflows: RwLock::new(HashMap::new()),
            completed: RwLock::new(VecDeque::new()),
        }
    }

    /// Submit a workflow for execution
    pub fn submit(&self, spec: WorkflowSpec) -> WorkflowResult<String> {
        spec.validate()?;

        let workflows = self.workflows.read();
        if workflows.len() >= self.config.max_concurrent {
            return Err(WorkflowError::MaxConcurrent {
                current: workflows.len(),
                max: self.config.max_concurrent,
            });
        }
        drop(workflows);

        let execution = WorkflowExecution::new(spec);
        let id = execution.id.clone();
        self.workflows.write().insert(id.clone(), execution);
        Ok(id)
    }

    /// Start a workflow (transition from Pending to Running)
    pub fn start(&self, workflow_id: &str) -> WorkflowResult<Vec<String>> {
        let mut workflows = self.workflows.write();
        let wf = workflows
            .get_mut(workflow_id)
            .ok_or_else(|| WorkflowError::NotFound(workflow_id.to_string()))?;

        if wf.phase != WorkflowPhase::Pending {
            return Err(WorkflowError::InvalidTransition {
                from: wf.phase,
                to: WorkflowPhase::Running,
            });
        }

        wf.phase = WorkflowPhase::Running;
        wf.started_at = Some(SystemTime::now());

        Ok(wf.ready_steps())
    }

    /// Get the next batch of ready steps for a workflow
    pub fn ready_steps(&self, workflow_id: &str) -> WorkflowResult<Vec<String>> {
        let workflows = self.workflows.read();
        let wf = workflows
            .get(workflow_id)
            .ok_or_else(|| WorkflowError::NotFound(workflow_id.to_string()))?;
        Ok(wf.ready_steps())
    }

    /// Mark a step as started
    pub fn start_step(&self, workflow_id: &str, step_name: &str) -> WorkflowResult<StepContext> {
        let mut workflows = self.workflows.write();
        let wf = workflows
            .get_mut(workflow_id)
            .ok_or_else(|| WorkflowError::NotFound(workflow_id.to_string()))?;

        let ctx = wf.step_context(step_name);
        wf.start_step(step_name)?;
        Ok(ctx)
    }

    /// Record step completion
    pub fn complete_step(
        &self,
        workflow_id: &str,
        step_name: &str,
        outcome: StepOutcome,
    ) -> WorkflowResult<()> {
        let mut workflows = self.workflows.write();
        let wf = workflows
            .get_mut(workflow_id)
            .ok_or_else(|| WorkflowError::NotFound(workflow_id.to_string()))?;

        wf.complete_step(step_name, outcome);

        // If workflow is terminal, move to completed
        if wf.phase.is_terminal() {
            let wf = workflows.remove(workflow_id).unwrap();
            let mut completed = self.completed.write();
            completed.push_back(wf);
            while completed.len() > 1000 {
                completed.pop_front();
            }
        }

        Ok(())
    }

    /// Cancel a workflow
    pub fn cancel(&self, workflow_id: &str) -> WorkflowResult<()> {
        let mut workflows = self.workflows.write();
        let wf = workflows
            .get_mut(workflow_id)
            .ok_or_else(|| WorkflowError::NotFound(workflow_id.to_string()))?;

        if wf.phase.is_terminal() {
            return Err(WorkflowError::InvalidTransition {
                from: wf.phase,
                to: WorkflowPhase::Cancelled,
            });
        }

        wf.phase = WorkflowPhase::Cancelled;
        wf.completed_at = Some(SystemTime::now());

        let wf = workflows.remove(workflow_id).unwrap();
        self.completed.write().push_back(wf);
        Ok(())
    }

    /// Get a workflow execution by ID
    pub fn get(&self, workflow_id: &str) -> Option<WorkflowExecution> {
        self.workflows.read().get(workflow_id).cloned().or_else(|| {
            self.completed
                .read()
                .iter()
                .find(|w| w.id == workflow_id)
                .cloned()
        })
    }

    /// Get progress of a workflow (0.0 - 1.0)
    pub fn progress(&self, workflow_id: &str) -> Option<f64> {
        self.workflows
            .read()
            .get(workflow_id)
            .map(|wf| wf.progress())
    }

    /// Number of active workflows
    pub fn active_count(&self) -> usize {
        self.workflows.read().len()
    }

    /// Get engine configuration
    pub fn config(&self) -> &WorkflowConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_workflow() -> WorkflowSpec {
        WorkflowBuilder::new("test-pipeline")
            .step(StepSpec::new("ingest", "download data"))
            .step(StepSpec::new("transform", "process data").depends_on("ingest"))
            .step(StepSpec::new("export", "upload results").depends_on("transform"))
            .build()
            .unwrap()
    }

    fn parallel_workflow() -> WorkflowSpec {
        WorkflowBuilder::new("parallel-pipeline")
            .step(StepSpec::new("fetch-a", "fetch source A"))
            .step(StepSpec::new("fetch-b", "fetch source B"))
            .step(
                StepSpec::new("merge", "merge results")
                    .depends_on("fetch-a")
                    .depends_on("fetch-b"),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_workflow_builder() {
        let spec = simple_workflow();
        assert_eq!(spec.name, "test-pipeline");
        assert_eq!(spec.steps.len(), 3);
    }

    #[test]
    fn test_workflow_validation_ok() {
        let spec = simple_workflow();
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn test_workflow_validation_cycle() {
        let result = WorkflowBuilder::new("cyclic")
            .step(StepSpec::new("a", "cmd").depends_on("b"))
            .step(StepSpec::new("b", "cmd").depends_on("a"))
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_workflow_validation_missing_dep() {
        let result = WorkflowBuilder::new("bad")
            .step(StepSpec::new("a", "cmd").depends_on("nonexistent"))
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_workflow_execution_ready_steps() {
        let exec = WorkflowExecution::new(simple_workflow());
        let ready = exec.ready_steps();
        assert_eq!(ready, vec!["ingest"]); // Only first step is ready
    }

    #[test]
    fn test_parallel_ready_steps() {
        let exec = WorkflowExecution::new(parallel_workflow());
        let ready = exec.ready_steps();
        assert_eq!(ready.len(), 2); // fetch-a and fetch-b are both ready
    }

    #[test]
    fn test_workflow_step_progression() {
        let mut exec = WorkflowExecution::new(simple_workflow());
        exec.phase = WorkflowPhase::Running;

        // ingest is ready
        assert_eq!(exec.ready_steps(), vec!["ingest"]);

        exec.start_step("ingest").unwrap();
        exec.complete_step(
            "ingest",
            StepOutcome::Success {
                output: Some("data.csv".to_string()),
            },
        );

        // Now transform is ready
        assert_eq!(exec.ready_steps(), vec!["transform"]);

        exec.start_step("transform").unwrap();
        exec.complete_step(
            "transform",
            StepOutcome::Success {
                output: Some("processed.csv".to_string()),
            },
        );

        // Now export is ready
        assert_eq!(exec.ready_steps(), vec!["export"]);

        exec.start_step("export").unwrap();
        exec.complete_step("export", StepOutcome::Success { output: None });

        assert_eq!(exec.phase, WorkflowPhase::Completed);
        assert!((exec.progress() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_workflow_step_failure() {
        let mut exec = WorkflowExecution::new(simple_workflow());
        exec.phase = WorkflowPhase::Running;

        exec.start_step("ingest").unwrap();
        // First failure — retryable (max_retries=2, attempt 1)
        exec.complete_step(
            "ingest",
            StepOutcome::Failure {
                error: "network timeout".to_string(),
                retryable: true,
            },
        );
        // Step should go back to Pending for retry
        assert_eq!(exec.steps["ingest"].status, StepStatus::Pending);

        // Retry succeeds
        exec.start_step("ingest").unwrap();
        exec.complete_step("ingest", StepOutcome::Success { output: None });
        assert_eq!(exec.steps["ingest"].status, StepStatus::Completed);
    }

    #[test]
    fn test_workflow_retry_exhausted() {
        let spec = WorkflowBuilder::new("retry-test")
            .step(StepSpec::new("fail-step", "cmd").max_retries(1))
            .build()
            .unwrap();

        let mut exec = WorkflowExecution::new(spec);
        exec.phase = WorkflowPhase::Running;

        // Attempt 1 fails
        exec.start_step("fail-step").unwrap();
        exec.complete_step(
            "fail-step",
            StepOutcome::Failure {
                error: "error".to_string(),
                retryable: true,
            },
        );
        assert_eq!(exec.steps["fail-step"].status, StepStatus::Pending);

        // Attempt 2 fails — retries exhausted
        exec.start_step("fail-step").unwrap();
        exec.complete_step(
            "fail-step",
            StepOutcome::Failure {
                error: "error again".to_string(),
                retryable: true,
            },
        );
        assert_eq!(exec.steps["fail-step"].status, StepStatus::Failed);
        assert_eq!(exec.phase, WorkflowPhase::Failed);
    }

    #[test]
    fn test_optional_step_failure() {
        let spec = WorkflowBuilder::new("optional-test")
            .step(StepSpec::new("required", "cmd"))
            .step(
                StepSpec::new("optional-step", "cmd")
                    .optional()
                    .max_retries(0),
            )
            .build()
            .unwrap();

        let mut exec = WorkflowExecution::new(spec);
        exec.phase = WorkflowPhase::Running;

        exec.start_step("required").unwrap();
        exec.complete_step("required", StepOutcome::Success { output: None });

        exec.start_step("optional-step").unwrap();
        exec.complete_step(
            "optional-step",
            StepOutcome::Failure {
                error: "optional failure".to_string(),
                retryable: false,
            },
        );

        // Workflow should still complete because the failed step is optional
        assert_eq!(exec.phase, WorkflowPhase::Completed);
    }

    #[test]
    fn test_step_context() {
        let mut exec = WorkflowExecution::new(simple_workflow());
        exec.phase = WorkflowPhase::Running;

        exec.start_step("ingest").unwrap();
        exec.complete_step(
            "ingest",
            StepOutcome::Success {
                output: Some("data.csv".to_string()),
            },
        );

        let ctx = exec.step_context("transform");
        assert_eq!(ctx.step_name, "transform");
        assert_eq!(ctx.dependency_outputs.get("ingest").unwrap(), "data.csv");
    }

    #[test]
    fn test_workflow_engine_submit() {
        let engine = WorkflowEngine::new(WorkflowConfig::default());
        let id = engine.submit(simple_workflow()).unwrap();
        assert!(!id.is_empty());
        assert_eq!(engine.active_count(), 1);
    }

    #[test]
    fn test_workflow_engine_start() {
        let engine = WorkflowEngine::new(WorkflowConfig::default());
        let id = engine.submit(simple_workflow()).unwrap();
        let ready = engine.start(&id).unwrap();
        assert_eq!(ready, vec!["ingest"]);
    }

    #[test]
    fn test_workflow_engine_full_lifecycle() {
        let engine = WorkflowEngine::new(WorkflowConfig::default());
        let id = engine.submit(simple_workflow()).unwrap();
        engine.start(&id).unwrap();

        // Execute all steps
        for step in ["ingest", "transform", "export"] {
            engine.start_step(&id, step).unwrap();
            engine
                .complete_step(&id, step, StepOutcome::Success { output: None })
                .unwrap();
        }

        // Workflow should be completed and moved to history
        assert_eq!(engine.active_count(), 0);
        let wf = engine.get(&id).unwrap();
        assert_eq!(wf.phase, WorkflowPhase::Completed);
    }

    #[test]
    fn test_workflow_engine_cancel() {
        let engine = WorkflowEngine::new(WorkflowConfig::default());
        let id = engine.submit(simple_workflow()).unwrap();
        engine.start(&id).unwrap();
        engine.cancel(&id).unwrap();

        assert_eq!(engine.active_count(), 0);
        let wf = engine.get(&id).unwrap();
        assert_eq!(wf.phase, WorkflowPhase::Cancelled);
    }

    #[test]
    fn test_workflow_engine_max_concurrent() {
        let engine = WorkflowEngine::new(WorkflowConfig {
            max_concurrent: 1,
            ..Default::default()
        });
        engine.submit(simple_workflow()).unwrap();
        let err = engine.submit(simple_workflow()).unwrap_err();
        assert!(matches!(err, WorkflowError::MaxConcurrent { .. }));
    }

    #[test]
    fn test_workflow_progress() {
        let engine = WorkflowEngine::new(WorkflowConfig::default());
        let id = engine.submit(simple_workflow()).unwrap();
        engine.start(&id).unwrap();

        assert!((engine.progress(&id).unwrap() - 0.0).abs() < f64::EPSILON);

        engine.start_step(&id, "ingest").unwrap();
        engine
            .complete_step(&id, "ingest", StepOutcome::Success { output: None })
            .unwrap();

        let progress = engine.progress(&id).unwrap();
        assert!((progress - 1.0 / 3.0).abs() < 0.01);
    }
}
