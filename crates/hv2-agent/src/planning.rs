//! Agent Planning System
//!
//! Provides hierarchical task planning, goal management, and plan execution
//! for AI agents operating within HyperMachine virtual machines.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Planning-specific error types.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlanningError {
    /// Goal not found.
    #[error("Goal not found: {0}")]
    GoalNotFound(String),
    /// Plan not found.
    #[error("Plan not found: {0}")]
    PlanNotFound(String),
    /// Action not found.
    #[error("Action not found: {0}")]
    ActionNotFound(String),
    /// Precondition not met.
    #[error("Precondition not met: {0}")]
    PreconditionNotMet(String),
    /// Goal already exists.
    #[error("Goal already exists: {0}")]
    GoalAlreadyExists(String),
    /// Planning timeout.
    #[error("Planning timeout")]
    PlanningTimeout,
    /// No valid plan found.
    #[error("No valid plan found")]
    NoPlanFound,
    /// Execution failed.
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    /// Invalid state.
    #[error("Invalid state: {0}")]
    InvalidState(String),
    /// Resource unavailable.
    #[error("Resource unavailable: {0}")]
    ResourceUnavailable(String),
    /// Cycle detected.
    #[error("Cycle detected in plan")]
    CycleDetected,
}

/// Result type for planning operations.
pub type PlanningResult<T> = Result<T, PlanningError>;

/// Priority level for goals and actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Priority {
    /// Critical priority - must be addressed immediately.
    Critical,
    /// High priority.
    High,
    /// Normal priority.
    #[default]
    Normal,
    /// Low priority.
    Low,
    /// Background priority - can be deferred.
    Background,
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value().cmp(&other.value())
    }
}

impl Priority {
    /// Get numeric value for comparison.
    pub fn value(&self) -> u8 {
        match self {
            Self::Critical => 5,
            Self::High => 4,
            Self::Normal => 3,
            Self::Low => 2,
            Self::Background => 1,
        }
    }
}

/// Goal status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GoalStatus {
    /// Goal is pending.
    #[default]
    Pending,
    /// Goal is being pursued.
    Active,
    /// Goal is paused.
    Paused,
    /// Goal was achieved.
    Achieved,
    /// Goal was abandoned.
    Abandoned,
    /// Goal failed.
    Failed,
}

/// A condition that must be satisfied.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    /// Condition identifier.
    pub id: String,
    /// Description of the condition.
    pub description: String,
    /// Required state key.
    pub state_key: String,
    /// Expected value (as string for flexibility).
    pub expected_value: String,
    /// Comparison operator.
    pub operator: ConditionOperator,
}

impl Condition {
    /// Create a new condition.
    pub fn new(
        id: impl Into<String>,
        state_key: impl Into<String>,
        operator: ConditionOperator,
        expected_value: impl Into<String>,
    ) -> Self {
        let id = id.into();
        Self {
            description: id.clone(),
            id,
            state_key: state_key.into(),
            expected_value: expected_value.into(),
            operator,
        }
    }

    /// Check if condition is satisfied given a state.
    pub fn is_satisfied(&self, state: &WorldState) -> bool {
        if let Some(actual) = state.get(&self.state_key) {
            self.operator.evaluate(actual, &self.expected_value)
        } else {
            false
        }
    }
}

/// Condition comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionOperator {
    /// Equal to.
    Equals,
    /// Not equal to.
    NotEquals,
    /// Greater than (numeric).
    GreaterThan,
    /// Less than (numeric).
    LessThan,
    /// Greater than or equal (numeric).
    GreaterOrEqual,
    /// Less than or equal (numeric).
    LessOrEqual,
    /// Contains (string).
    Contains,
    /// Exists (any value).
    Exists,
}

impl ConditionOperator {
    /// Evaluate the operator with actual and expected values.
    pub fn evaluate(&self, actual: &str, expected: &str) -> bool {
        match self {
            Self::Equals => actual == expected,
            Self::NotEquals => actual != expected,
            Self::GreaterThan => actual
                .parse::<f64>()
                .ok()
                .zip(expected.parse::<f64>().ok())
                .map(|(a, e)| a > e)
                .unwrap_or(false),
            Self::LessThan => actual
                .parse::<f64>()
                .ok()
                .zip(expected.parse::<f64>().ok())
                .map(|(a, e)| a < e)
                .unwrap_or(false),
            Self::GreaterOrEqual => actual
                .parse::<f64>()
                .ok()
                .zip(expected.parse::<f64>().ok())
                .map(|(a, e)| a >= e)
                .unwrap_or(false),
            Self::LessOrEqual => actual
                .parse::<f64>()
                .ok()
                .zip(expected.parse::<f64>().ok())
                .map(|(a, e)| a <= e)
                .unwrap_or(false),
            Self::Contains => actual.contains(expected),
            Self::Exists => true,
        }
    }
}

/// World state representation.
pub type WorldState = HashMap<String, String>;

/// Effect of an action on the world state.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    /// State key to modify.
    pub state_key: String,
    /// New value to set.
    pub new_value: String,
    /// Whether to add or remove the key.
    pub operation: EffectOperation,
}

impl Effect {
    /// Create a set effect.
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            state_key: key.into(),
            new_value: value.into(),
            operation: EffectOperation::Set,
        }
    }

    /// Create a remove effect.
    pub fn remove(key: impl Into<String>) -> Self {
        Self {
            state_key: key.into(),
            new_value: String::new(),
            operation: EffectOperation::Remove,
        }
    }

    /// Apply effect to world state.
    pub fn apply(&self, state: &mut WorldState) {
        match self.operation {
            EffectOperation::Set => {
                state.insert(self.state_key.clone(), self.new_value.clone());
            }
            EffectOperation::Remove => {
                state.remove(&self.state_key);
            }
            EffectOperation::Increment => {
                if let Some(val) = state.get(&self.state_key) {
                    if let Ok(num) = val.parse::<i64>() {
                        let delta: i64 = self.new_value.parse().unwrap_or(1);
                        state.insert(self.state_key.clone(), (num + delta).to_string());
                    }
                }
            }
            EffectOperation::Decrement => {
                if let Some(val) = state.get(&self.state_key) {
                    if let Ok(num) = val.parse::<i64>() {
                        let delta: i64 = self.new_value.parse().unwrap_or(1);
                        state.insert(self.state_key.clone(), (num - delta).to_string());
                    }
                }
            }
        }
    }
}

/// Effect operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectOperation {
    /// Set value.
    Set,
    /// Remove key.
    Remove,
    /// Increment numeric value.
    Increment,
    /// Decrement numeric value.
    Decrement,
}

/// A goal to be achieved.
#[derive(Debug, Clone)]
pub struct Goal {
    /// Unique goal identifier.
    pub id: String,
    /// Goal name.
    pub name: String,
    /// Goal description.
    pub description: String,
    /// Goal priority.
    pub priority: Priority,
    /// Goal status.
    pub status: GoalStatus,
    /// Conditions that define goal satisfaction.
    pub success_conditions: Vec<Condition>,
    /// Parent goal (for sub-goals).
    pub parent_id: Option<String>,
    /// Sub-goal IDs.
    pub sub_goal_ids: Vec<String>,
    /// Deadline for the goal.
    pub deadline: Option<Instant>,
    /// Creation time.
    pub created_at: Instant,
    /// Completion time.
    pub completed_at: Option<Instant>,
    /// Associated metadata.
    pub metadata: HashMap<String, String>,
}

impl Goal {
    /// Create a new goal.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            priority: Priority::Normal,
            status: GoalStatus::Pending,
            success_conditions: Vec::new(),
            parent_id: None,
            sub_goal_ids: Vec::new(),
            deadline: None,
            created_at: Instant::now(),
            completed_at: None,
            metadata: HashMap::new(),
        }
    }

    /// Set goal description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set goal priority.
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Add a success condition.
    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.success_conditions.push(condition);
        self
    }

    /// Set deadline.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(Instant::now() + deadline);
        self
    }

    /// Check if goal is satisfied given current state.
    pub fn is_satisfied(&self, state: &WorldState) -> bool {
        if self.success_conditions.is_empty() {
            return false;
        }
        self.success_conditions
            .iter()
            .all(|c| c.is_satisfied(state))
    }

    /// Check if goal has expired.
    pub fn is_expired(&self) -> bool {
        self.deadline.map(|d| Instant::now() > d).unwrap_or(false)
    }

    /// Mark goal as achieved.
    pub fn achieve(&mut self) {
        self.status = GoalStatus::Achieved;
        self.completed_at = Some(Instant::now());
    }

    /// Mark goal as failed.
    pub fn fail(&mut self) {
        self.status = GoalStatus::Failed;
        self.completed_at = Some(Instant::now());
    }
}

/// An action that can be performed.
#[derive(Debug, Clone)]
pub struct PlanAction {
    /// Action identifier.
    pub id: String,
    /// Action name.
    pub name: String,
    /// Action description.
    pub description: String,
    /// Preconditions that must be satisfied.
    pub preconditions: Vec<Condition>,
    /// Effects of the action.
    pub effects: Vec<Effect>,
    /// Estimated cost of the action.
    pub cost: f64,
    /// Estimated duration.
    pub duration: Duration,
    /// Required resources.
    pub required_resources: HashSet<String>,
    /// Whether action is currently available.
    pub available: bool,
}

impl PlanAction {
    /// Create a new action.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            preconditions: Vec::new(),
            effects: Vec::new(),
            cost: 1.0,
            duration: Duration::from_secs(1),
            required_resources: HashSet::new(),
            available: true,
        }
    }

    /// Set action description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a precondition.
    pub fn with_precondition(mut self, condition: Condition) -> Self {
        self.preconditions.push(condition);
        self
    }

    /// Add an effect.
    pub fn with_effect(mut self, effect: Effect) -> Self {
        self.effects.push(effect);
        self
    }

    /// Set action cost.
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = cost;
        self
    }

    /// Set action duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Add a required resource.
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.required_resources.insert(resource.into());
        self
    }

    /// Check if action is applicable in current state.
    pub fn is_applicable(&self, state: &WorldState) -> bool {
        self.available && self.preconditions.iter().all(|c| c.is_satisfied(state))
    }

    /// Apply action effects to state.
    pub fn apply(&self, state: &mut WorldState) {
        for effect in &self.effects {
            effect.apply(state);
        }
    }
}

/// Plan status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanStatus {
    /// Plan is being constructed.
    #[default]
    Building,
    /// Plan is ready for execution.
    Ready,
    /// Plan is being executed.
    Executing,
    /// Plan was completed successfully.
    Completed,
    /// Plan failed during execution.
    Failed,
    /// Plan was cancelled.
    Cancelled,
}

/// A step in a plan.
#[derive(Debug, Clone)]
pub struct PlanStep {
    /// Step index.
    pub index: usize,
    /// Action to execute.
    pub action_id: String,
    /// Step status.
    pub status: StepStatus,
    /// Start time.
    pub started_at: Option<Instant>,
    /// Completion time.
    pub completed_at: Option<Instant>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl PlanStep {
    /// Create a new plan step.
    pub fn new(index: usize, action_id: impl Into<String>) -> Self {
        Self {
            index,
            action_id: action_id.into(),
            status: StepStatus::Pending,
            started_at: None,
            completed_at: None,
            error: None,
        }
    }

    /// Mark step as started.
    pub fn start(&mut self) {
        self.status = StepStatus::Running;
        self.started_at = Some(Instant::now());
    }

    /// Mark step as completed.
    pub fn complete(&mut self) {
        self.status = StepStatus::Completed;
        self.completed_at = Some(Instant::now());
    }

    /// Mark step as failed.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = StepStatus::Failed;
        self.completed_at = Some(Instant::now());
        self.error = Some(error.into());
    }
}

/// Step execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// Step is pending.
    Pending,
    /// Step is running.
    Running,
    /// Step completed successfully.
    Completed,
    /// Step was skipped.
    Skipped,
    /// Step failed.
    Failed,
}

/// A plan to achieve a goal.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Plan identifier.
    pub id: String,
    /// Associated goal ID.
    pub goal_id: String,
    /// Plan status.
    pub status: PlanStatus,
    /// Ordered steps.
    pub steps: Vec<PlanStep>,
    /// Current step index.
    pub current_step: usize,
    /// Total estimated cost.
    pub total_cost: f64,
    /// Total estimated duration.
    pub total_duration: Duration,
    /// Creation time.
    pub created_at: Instant,
    /// Start time.
    pub started_at: Option<Instant>,
    /// Completion time.
    pub completed_at: Option<Instant>,
}

impl Plan {
    /// Create a new plan.
    pub fn new(id: impl Into<String>, goal_id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            goal_id: goal_id.into(),
            status: PlanStatus::Building,
            steps: Vec::new(),
            current_step: 0,
            total_cost: 0.0,
            total_duration: Duration::ZERO,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Add a step to the plan.
    pub fn add_step(&mut self, action_id: impl Into<String>, cost: f64, duration: Duration) {
        let index = self.steps.len();
        self.steps.push(PlanStep::new(index, action_id));
        self.total_cost += cost;
        self.total_duration += duration;
    }

    /// Mark plan as ready.
    pub fn finalize(&mut self) {
        self.status = PlanStatus::Ready;
    }

    /// Start plan execution.
    pub fn start(&mut self) {
        self.status = PlanStatus::Executing;
        self.started_at = Some(Instant::now());
    }

    /// Get current step.
    pub fn current(&self) -> Option<&PlanStep> {
        self.steps.get(self.current_step)
    }

    /// Get current step mutably.
    pub fn current_mut(&mut self) -> Option<&mut PlanStep> {
        self.steps.get_mut(self.current_step)
    }

    /// Advance to next step.
    pub fn advance(&mut self) -> bool {
        if self.current_step < self.steps.len() {
            self.current_step += 1;
            if self.current_step >= self.steps.len() {
                self.status = PlanStatus::Completed;
                self.completed_at = Some(Instant::now());
            }
            true
        } else {
            false
        }
    }

    /// Check if plan is complete.
    pub fn is_complete(&self) -> bool {
        self.status == PlanStatus::Completed
    }

    /// Check if plan has failed.
    pub fn is_failed(&self) -> bool {
        self.status == PlanStatus::Failed
    }

    /// Mark plan as failed.
    pub fn fail(&mut self) {
        self.status = PlanStatus::Failed;
        self.completed_at = Some(Instant::now());
    }

    /// Get progress percentage.
    pub fn progress(&self) -> f64 {
        if self.steps.is_empty() {
            0.0
        } else {
            (self.current_step as f64 / self.steps.len() as f64) * 100.0
        }
    }
}

/// Planning algorithm type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanningAlgorithm {
    /// Forward state-space search.
    #[default]
    ForwardSearch,
    /// Backward (regression) search.
    BackwardSearch,
    /// Greedy best-first search.
    GreedyBestFirst,
    /// A* search.
    AStar,
}

/// Planning configuration.
#[derive(Debug, Clone)]
pub struct PlanningConfig {
    /// Planning algorithm to use.
    pub algorithm: PlanningAlgorithm,
    /// Maximum planning time.
    pub timeout: Duration,
    /// Maximum plan depth.
    pub max_depth: usize,
    /// Maximum nodes to explore.
    pub max_nodes: usize,
    /// Enable plan optimization.
    pub optimize: bool,
}

impl Default for PlanningConfig {
    fn default() -> Self {
        Self {
            algorithm: PlanningAlgorithm::ForwardSearch,
            timeout: Duration::from_secs(30),
            max_depth: 100,
            max_nodes: 10000,
            optimize: true,
        }
    }
}

/// Planner for generating plans.
#[derive(Debug)]
pub struct Planner {
    /// Available actions.
    actions: HashMap<String, PlanAction>,
    /// Configuration.
    config: PlanningConfig,
}

impl Planner {
    /// Create a new planner.
    pub fn new(config: PlanningConfig) -> Self {
        Self {
            actions: HashMap::new(),
            config,
        }
    }

    /// Register an action.
    pub fn register_action(&mut self, action: PlanAction) {
        self.actions.insert(action.id.clone(), action);
    }

    /// Get available actions in current state.
    pub fn get_applicable_actions(&self, state: &WorldState) -> Vec<&PlanAction> {
        self.actions
            .values()
            .filter(|a| a.is_applicable(state))
            .collect()
    }

    /// Generate a plan to achieve goal from current state.
    pub fn plan(&self, goal: &Goal, initial_state: &WorldState) -> PlanningResult<Plan> {
        let start_time = Instant::now();

        // Check if goal already satisfied
        if goal.is_satisfied(initial_state) {
            let mut plan = Plan::new(format!("plan_{}", goal.id), &goal.id);
            plan.finalize();
            return Ok(plan);
        }

        match self.config.algorithm {
            PlanningAlgorithm::ForwardSearch => {
                self.forward_search(goal, initial_state, start_time)
            }
            PlanningAlgorithm::BackwardSearch => {
                self.backward_search(goal, initial_state, start_time)
            }
            PlanningAlgorithm::GreedyBestFirst => {
                self.greedy_best_first(goal, initial_state, start_time)
            }
            PlanningAlgorithm::AStar => self.astar_search(goal, initial_state, start_time),
        }
    }

    /// Forward state-space search.
    fn forward_search(
        &self,
        goal: &Goal,
        initial_state: &WorldState,
        start_time: Instant,
    ) -> PlanningResult<Plan> {
        let mut queue: VecDeque<(WorldState, Vec<String>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        queue.push_back((initial_state.clone(), Vec::new()));
        let mut nodes_explored = 0;

        while let Some((state, action_sequence)) = queue.pop_front() {
            // Check timeout
            if start_time.elapsed() > self.config.timeout {
                return Err(PlanningError::PlanningTimeout);
            }

            // Check node limit
            nodes_explored += 1;
            if nodes_explored > self.config.max_nodes {
                return Err(PlanningError::NoPlanFound);
            }

            // Check depth limit
            if action_sequence.len() >= self.config.max_depth {
                continue;
            }

            // Check if goal reached
            if goal.is_satisfied(&state) {
                return self.build_plan(goal, &action_sequence);
            }

            // Create state signature for visited check
            let state_sig = self.state_signature(&state);
            if visited.contains(&state_sig) {
                continue;
            }
            visited.insert(state_sig);

            // Expand with applicable actions
            for action in self.get_applicable_actions(&state) {
                let mut new_state = state.clone();
                action.apply(&mut new_state);

                let mut new_sequence = action_sequence.clone();
                new_sequence.push(action.id.clone());

                queue.push_back((new_state, new_sequence));
            }
        }

        Err(PlanningError::NoPlanFound)
    }

    /// Backward (regression) search.
    fn backward_search(
        &self,
        goal: &Goal,
        initial_state: &WorldState,
        start_time: Instant,
    ) -> PlanningResult<Plan> {
        // For simplicity, fall back to forward search
        // Full regression planning would require effect inversion
        self.forward_search(goal, initial_state, start_time)
    }

    /// Greedy best-first search with heuristic.
    fn greedy_best_first(
        &self,
        goal: &Goal,
        initial_state: &WorldState,
        start_time: Instant,
    ) -> PlanningResult<Plan> {
        let mut open: Vec<(WorldState, Vec<String>, f64)> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        let h = self.heuristic(goal, initial_state);
        open.push((initial_state.clone(), Vec::new(), h));
        let mut nodes_explored = 0;

        while !open.is_empty() {
            // Check timeout
            if start_time.elapsed() > self.config.timeout {
                return Err(PlanningError::PlanningTimeout);
            }

            nodes_explored += 1;
            if nodes_explored > self.config.max_nodes {
                return Err(PlanningError::NoPlanFound);
            }

            // Get node with lowest heuristic
            open.sort_by(|a, b| {
                a.2.partial_cmp(&b.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .reverse()
            });
            let Some((state, action_sequence, _)) = open.pop() else {
                return Err(PlanningError::NoPlanFound);
            };

            if action_sequence.len() >= self.config.max_depth {
                continue;
            }

            if goal.is_satisfied(&state) {
                return self.build_plan(goal, &action_sequence);
            }

            let state_sig = self.state_signature(&state);
            if visited.contains(&state_sig) {
                continue;
            }
            visited.insert(state_sig);

            for action in self.get_applicable_actions(&state) {
                let mut new_state = state.clone();
                action.apply(&mut new_state);

                let mut new_sequence = action_sequence.clone();
                new_sequence.push(action.id.clone());

                let h = self.heuristic(goal, &new_state);
                open.push((new_state, new_sequence, h));
            }
        }

        Err(PlanningError::NoPlanFound)
    }

    /// A* search with cost and heuristic.
    fn astar_search(
        &self,
        goal: &Goal,
        initial_state: &WorldState,
        start_time: Instant,
    ) -> PlanningResult<Plan> {
        let mut open: Vec<(WorldState, Vec<String>, f64, f64)> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        let h = self.heuristic(goal, initial_state);
        open.push((initial_state.clone(), Vec::new(), 0.0, h));
        let mut nodes_explored = 0;

        while !open.is_empty() {
            if start_time.elapsed() > self.config.timeout {
                return Err(PlanningError::PlanningTimeout);
            }

            nodes_explored += 1;
            if nodes_explored > self.config.max_nodes {
                return Err(PlanningError::NoPlanFound);
            }

            // Get node with lowest f = g + h
            open.sort_by(|a, b| {
                let f_a = a.2 + a.3;
                let f_b = b.2 + b.3;
                f_a.partial_cmp(&f_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .reverse()
            });
            let Some((state, action_sequence, g, _)) = open.pop() else {
                return Err(PlanningError::NoPlanFound);
            };

            if action_sequence.len() >= self.config.max_depth {
                continue;
            }

            if goal.is_satisfied(&state) {
                return self.build_plan(goal, &action_sequence);
            }

            let state_sig = self.state_signature(&state);
            if visited.contains(&state_sig) {
                continue;
            }
            visited.insert(state_sig);

            for action in self.get_applicable_actions(&state) {
                let mut new_state = state.clone();
                action.apply(&mut new_state);

                let mut new_sequence = action_sequence.clone();
                new_sequence.push(action.id.clone());

                let new_g = g + action.cost;
                let h = self.heuristic(goal, &new_state);
                open.push((new_state, new_sequence, new_g, h));
            }
        }

        Err(PlanningError::NoPlanFound)
    }

    /// Simple heuristic: count unsatisfied conditions.
    fn heuristic(&self, goal: &Goal, state: &WorldState) -> f64 {
        goal.success_conditions
            .iter()
            .filter(|c| !c.is_satisfied(state))
            .count() as f64
    }

    /// Create state signature for duplicate detection.
    fn state_signature(&self, state: &WorldState) -> String {
        let mut pairs: Vec<_> = state.iter().collect();
        pairs.sort_by_key(|(k, _)| *k);
        pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Build plan from action sequence.
    fn build_plan(&self, goal: &Goal, action_ids: &[String]) -> PlanningResult<Plan> {
        let mut plan = Plan::new(format!("plan_{}", goal.id), &goal.id);

        for action_id in action_ids {
            if let Some(action) = self.actions.get(action_id) {
                plan.add_step(action_id, action.cost, action.duration);
            } else {
                return Err(PlanningError::ActionNotFound(action_id.clone()));
            }
        }

        plan.finalize();
        Ok(plan)
    }
}

/// Goal manager for tracking goals.
#[derive(Debug)]
pub struct GoalManager {
    /// Active goals.
    goals: HashMap<String, Goal>,
    /// Goal hierarchy (parent -> children).
    hierarchy: HashMap<String, Vec<String>>,
}

impl GoalManager {
    /// Create a new goal manager.
    pub fn new() -> Self {
        Self {
            goals: HashMap::new(),
            hierarchy: HashMap::new(),
        }
    }

    /// Add a goal.
    pub fn add_goal(&mut self, goal: Goal) -> PlanningResult<()> {
        if self.goals.contains_key(&goal.id) {
            return Err(PlanningError::GoalAlreadyExists(goal.id.clone()));
        }

        // Update hierarchy
        if let Some(parent_id) = &goal.parent_id {
            self.hierarchy
                .entry(parent_id.clone())
                .or_default()
                .push(goal.id.clone());
        }

        self.goals.insert(goal.id.clone(), goal);
        Ok(())
    }

    /// Get a goal by ID.
    pub fn get_goal(&self, id: &str) -> Option<&Goal> {
        self.goals.get(id)
    }

    /// Get a goal mutably.
    pub fn get_goal_mut(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.get_mut(id)
    }

    /// Remove a goal.
    pub fn remove_goal(&mut self, id: &str) -> Option<Goal> {
        self.goals.remove(id)
    }

    /// Get all active goals.
    pub fn active_goals(&self) -> Vec<&Goal> {
        self.goals
            .values()
            .filter(|g| g.status == GoalStatus::Active || g.status == GoalStatus::Pending)
            .collect()
    }

    /// Get goals by priority.
    pub fn goals_by_priority(&self) -> Vec<&Goal> {
        let mut goals: Vec<_> = self.goals.values().collect();
        goals.sort_by_key(|g| std::cmp::Reverse(g.priority.value()));
        goals
    }

    /// Update goal status based on world state.
    pub fn update(&mut self, state: &WorldState) {
        for goal in self.goals.values_mut() {
            if goal.status == GoalStatus::Active || goal.status == GoalStatus::Pending {
                if goal.is_satisfied(state) {
                    goal.achieve();
                } else if goal.is_expired() {
                    goal.fail();
                }
            }
        }
    }

    /// Get sub-goals of a goal.
    pub fn sub_goals(&self, parent_id: &str) -> Vec<&Goal> {
        self.hierarchy
            .get(parent_id)
            .map(|ids| ids.iter().filter_map(|id| self.goals.get(id)).collect())
            .unwrap_or_default()
    }

    /// Count goals by status.
    pub fn count_by_status(&self, status: GoalStatus) -> usize {
        self.goals.values().filter(|g| g.status == status).count()
    }

    /// Get total goal count.
    pub fn total_goals(&self) -> usize {
        self.goals.len()
    }
}

impl Default for GoalManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Planning system combining goals, plans, and execution.
#[derive(Debug)]
pub struct PlanningSystem {
    /// Goal manager.
    goal_manager: GoalManager,
    /// Planner.
    planner: Planner,
    /// Active plans.
    plans: HashMap<String, Plan>,
    /// Current world state.
    world_state: WorldState,
}

impl PlanningSystem {
    /// Create a new planning system.
    pub fn new(config: PlanningConfig) -> Self {
        Self {
            goal_manager: GoalManager::new(),
            planner: Planner::new(config),
            plans: HashMap::new(),
            world_state: HashMap::new(),
        }
    }

    /// Register an action.
    pub fn register_action(&mut self, action: PlanAction) {
        self.planner.register_action(action);
    }

    /// Set world state.
    pub fn set_state(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.world_state.insert(key.into(), value.into());
    }

    /// Get world state value.
    pub fn get_state(&self, key: &str) -> Option<&String> {
        self.world_state.get(key)
    }

    /// Add a goal.
    pub fn add_goal(&mut self, goal: Goal) -> PlanningResult<()> {
        self.goal_manager.add_goal(goal)
    }

    /// Plan for a goal.
    pub fn plan_for_goal(&mut self, goal_id: &str) -> PlanningResult<String> {
        let goal = self
            .goal_manager
            .get_goal(goal_id)
            .ok_or_else(|| PlanningError::GoalNotFound(goal_id.to_string()))?
            .clone();

        let plan = self.planner.plan(&goal, &self.world_state)?;
        let plan_id = plan.id.clone();
        self.plans.insert(plan_id.clone(), plan);

        Ok(plan_id)
    }

    /// Get a plan.
    pub fn get_plan(&self, id: &str) -> Option<&Plan> {
        self.plans.get(id)
    }

    /// Execute next step of a plan.
    pub fn execute_step(&mut self, plan_id: &str) -> PlanningResult<bool> {
        let plan = self
            .plans
            .get_mut(plan_id)
            .ok_or_else(|| PlanningError::PlanNotFound(plan_id.to_string()))?;

        if plan.is_complete() {
            return Ok(false);
        }

        if plan.status == PlanStatus::Ready {
            plan.start();
        }

        // Get action_id first to avoid borrow issues
        let action_id = match plan.current() {
            Some(step) => step.action_id.clone(),
            None => return Ok(false),
        };

        // Now we can mutate step
        if let Some(step) = plan.current_mut() {
            step.start();
        }

        // Get action and check applicability
        let action = self.planner.actions.get(&action_id).cloned();

        if let Some(action) = action {
            // Check preconditions
            if !action.is_applicable(&self.world_state) {
                let plan = self
                    .plans
                    .get_mut(plan_id)
                    .ok_or_else(|| PlanningError::PlanNotFound(plan_id.to_string()))?;
                if let Some(step) = plan.current_mut() {
                    step.fail("Preconditions not met");
                }
                plan.fail();
                return Err(PlanningError::PreconditionNotMet(action_id));
            }

            // Apply effects
            action.apply(&mut self.world_state);

            let plan = self
                .plans
                .get_mut(plan_id)
                .ok_or_else(|| PlanningError::PlanNotFound(plan_id.to_string()))?;
            if let Some(step) = plan.current_mut() {
                step.complete();
            }
            plan.advance();

            Ok(!plan.is_complete())
        } else {
            let plan = self
                .plans
                .get_mut(plan_id)
                .ok_or_else(|| PlanningError::PlanNotFound(plan_id.to_string()))?;
            if let Some(step) = plan.current_mut() {
                step.fail("Action not found");
            }
            plan.fail();
            Err(PlanningError::ActionNotFound(action_id))
        }
    }

    /// Update goals based on current state.
    pub fn update_goals(&mut self) {
        self.goal_manager.update(&self.world_state);
    }

    /// Get active goal count.
    pub fn active_goal_count(&self) -> usize {
        self.goal_manager.count_by_status(GoalStatus::Active)
            + self.goal_manager.count_by_status(GoalStatus::Pending)
    }

    /// Get plan count.
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }
}

/// Thread-safe shared planning system.
pub type SharedPlanning = Arc<RwLock<PlanningSystem>>;

/// Create a new shared planning system.
pub fn shared_planning(config: PlanningConfig) -> SharedPlanning {
    Arc::new(RwLock::new(PlanningSystem::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Normal);
        assert!(Priority::Normal > Priority::Low);
        assert!(Priority::Low > Priority::Background);
    }

    #[test]
    fn test_priority_value() {
        assert_eq!(Priority::Critical.value(), 5);
        assert_eq!(Priority::Background.value(), 1);
    }

    #[test]
    fn test_condition_equals() {
        let condition = Condition::new("c1", "status", ConditionOperator::Equals, "active");
        let mut state = WorldState::new();

        assert!(!condition.is_satisfied(&state));
        state.insert("status".into(), "active".into());
        assert!(condition.is_satisfied(&state));
    }

    #[test]
    fn test_condition_numeric() {
        let condition = Condition::new("c1", "count", ConditionOperator::GreaterThan, "5");
        let mut state = WorldState::new();

        state.insert("count".into(), "3".into());
        assert!(!condition.is_satisfied(&state));

        state.insert("count".into(), "10".into());
        assert!(condition.is_satisfied(&state));
    }

    #[test]
    fn test_condition_contains() {
        let condition = Condition::new("c1", "name", ConditionOperator::Contains, "test");
        let mut state = WorldState::new();

        state.insert("name".into(), "my_test_file".into());
        assert!(condition.is_satisfied(&state));
    }

    #[test]
    fn test_effect_set() {
        let effect = Effect::set("key", "value");
        let mut state = WorldState::new();

        effect.apply(&mut state);
        assert_eq!(state.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_effect_remove() {
        let effect = Effect::remove("key");
        let mut state = WorldState::new();
        state.insert("key".into(), "value".into());

        effect.apply(&mut state);
        assert!(!state.contains_key("key"));
    }

    #[test]
    fn test_goal_creation() {
        let goal = Goal::new("g1", "Test Goal")
            .with_description("A test goal")
            .with_priority(Priority::High);

        assert_eq!(goal.id, "g1");
        assert_eq!(goal.priority, Priority::High);
        assert_eq!(goal.status, GoalStatus::Pending);
    }

    #[test]
    fn test_goal_satisfaction() {
        let goal = Goal::new("g1", "Test Goal").with_condition(Condition::new(
            "c1",
            "done",
            ConditionOperator::Equals,
            "true",
        ));

        let mut state = WorldState::new();
        assert!(!goal.is_satisfied(&state));

        state.insert("done".into(), "true".into());
        assert!(goal.is_satisfied(&state));
    }

    #[test]
    fn test_goal_achieve() {
        let mut goal = Goal::new("g1", "Test Goal");
        goal.achieve();

        assert_eq!(goal.status, GoalStatus::Achieved);
        assert!(goal.completed_at.is_some());
    }

    #[test]
    fn test_action_creation() {
        let action = PlanAction::new("a1", "Test Action")
            .with_cost(2.5)
            .with_duration(Duration::from_secs(5));

        assert_eq!(action.id, "a1");
        assert_eq!(action.cost, 2.5);
        assert!(action.available);
    }

    #[test]
    fn test_action_applicability() {
        let action = PlanAction::new("a1", "Test").with_precondition(Condition::new(
            "c1",
            "ready",
            ConditionOperator::Equals,
            "true",
        ));

        let mut state = WorldState::new();
        assert!(!action.is_applicable(&state));

        state.insert("ready".into(), "true".into());
        assert!(action.is_applicable(&state));
    }

    #[test]
    fn test_action_apply() {
        let action = PlanAction::new("a1", "Test").with_effect(Effect::set("result", "done"));

        let mut state = WorldState::new();
        action.apply(&mut state);

        assert_eq!(state.get("result"), Some(&"done".to_string()));
    }

    #[test]
    fn test_plan_creation() {
        let mut plan = Plan::new("p1", "g1");
        plan.add_step("a1", 1.0, Duration::from_secs(1));
        plan.add_step("a2", 2.0, Duration::from_secs(2));
        plan.finalize();

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.total_cost, 3.0);
        assert_eq!(plan.status, PlanStatus::Ready);
    }

    #[test]
    fn test_plan_progress() {
        let mut plan = Plan::new("p1", "g1");
        plan.add_step("a1", 1.0, Duration::from_secs(1));
        plan.add_step("a2", 1.0, Duration::from_secs(1));
        plan.finalize();

        assert_eq!(plan.progress(), 0.0);
        plan.advance();
        assert_eq!(plan.progress(), 50.0);
        plan.advance();
        assert_eq!(plan.progress(), 100.0);
    }

    #[test]
    fn test_planner_simple() {
        let config = PlanningConfig::default();
        let mut planner = Planner::new(config);

        // Register action that sets done=true
        planner.register_action(
            PlanAction::new("do_it", "Do It").with_effect(Effect::set("done", "true")),
        );

        // Goal: done=true
        let goal = Goal::new("g1", "Be Done").with_condition(Condition::new(
            "c1",
            "done",
            ConditionOperator::Equals,
            "true",
        ));

        let state = WorldState::new();
        let plan = planner.plan(&goal, &state).unwrap();

        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].action_id, "do_it");
    }

    #[test]
    fn test_planner_multi_step() {
        let config = PlanningConfig::default();
        let mut planner = Planner::new(config);

        planner.register_action(
            PlanAction::new("step1", "Step 1").with_effect(Effect::set("step1", "done")),
        );

        planner.register_action(
            PlanAction::new("step2", "Step 2")
                .with_precondition(Condition::new(
                    "c1",
                    "step1",
                    ConditionOperator::Equals,
                    "done",
                ))
                .with_effect(Effect::set("step2", "done")),
        );

        let goal = Goal::new("g1", "Complete Step 2").with_condition(Condition::new(
            "c1",
            "step2",
            ConditionOperator::Equals,
            "done",
        ));

        let state = WorldState::new();
        let plan = planner.plan(&goal, &state).unwrap();

        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn test_planner_already_satisfied() {
        let config = PlanningConfig::default();
        let planner = Planner::new(config);

        let goal = Goal::new("g1", "Be Done").with_condition(Condition::new(
            "c1",
            "done",
            ConditionOperator::Equals,
            "true",
        ));

        let mut state = WorldState::new();
        state.insert("done".into(), "true".into());

        let plan = planner.plan(&goal, &state).unwrap();
        assert_eq!(plan.steps.len(), 0); // No actions needed
    }

    #[test]
    fn test_goal_manager() {
        let mut manager = GoalManager::new();

        manager
            .add_goal(Goal::new("g1", "Goal 1").with_priority(Priority::High))
            .unwrap();
        manager
            .add_goal(Goal::new("g2", "Goal 2").with_priority(Priority::Low))
            .unwrap();

        assert_eq!(manager.total_goals(), 2);

        let by_priority = manager.goals_by_priority();
        assert_eq!(by_priority[0].priority, Priority::High);
    }

    #[test]
    fn test_goal_manager_update() {
        let mut manager = GoalManager::new();

        let goal = Goal::new("g1", "Goal 1").with_condition(Condition::new(
            "c1",
            "done",
            ConditionOperator::Equals,
            "true",
        ));
        manager.add_goal(goal).unwrap();

        let mut state = WorldState::new();
        state.insert("done".into(), "true".into());

        manager.update(&state);

        let goal = manager.get_goal("g1").unwrap();
        assert_eq!(goal.status, GoalStatus::Achieved);
    }

    #[test]
    fn test_planning_system() {
        let config = PlanningConfig::default();
        let mut system = PlanningSystem::new(config);

        system.register_action(
            PlanAction::new("complete", "Complete Task").with_effect(Effect::set("task", "done")),
        );

        let goal = Goal::new("g1", "Complete Task").with_condition(Condition::new(
            "c1",
            "task",
            ConditionOperator::Equals,
            "done",
        ));
        system.add_goal(goal).unwrap();

        let plan_id = system.plan_for_goal("g1").unwrap();
        assert!(system.get_plan(&plan_id).is_some());
    }

    #[test]
    fn test_planning_system_execution() {
        let config = PlanningConfig::default();
        let mut system = PlanningSystem::new(config);

        system.register_action(
            PlanAction::new("complete", "Complete Task").with_effect(Effect::set("task", "done")),
        );

        let goal = Goal::new("g1", "Complete Task").with_condition(Condition::new(
            "c1",
            "task",
            ConditionOperator::Equals,
            "done",
        ));
        system.add_goal(goal).unwrap();

        let plan_id = system.plan_for_goal("g1").unwrap();

        // Execute the plan
        let more = system.execute_step(&plan_id).unwrap();
        assert!(!more); // Only one step

        assert_eq!(system.get_state("task"), Some(&"done".to_string()));
    }

    #[test]
    fn test_shared_planning() {
        let system = shared_planning(PlanningConfig::default());
        assert!(Arc::strong_count(&system) == 1);
    }

    #[test]
    fn test_planning_error_display() {
        let err = PlanningError::GoalNotFound("g1".to_string());
        assert!(err.to_string().contains("g1"));
    }

    #[test]
    fn test_step_status() {
        let mut step = PlanStep::new(0, "action1");
        assert_eq!(step.status, StepStatus::Pending);

        step.start();
        assert_eq!(step.status, StepStatus::Running);

        step.complete();
        assert_eq!(step.status, StepStatus::Completed);
    }

    #[test]
    fn test_step_failure() {
        let mut step = PlanStep::new(0, "action1");
        step.fail("Something went wrong");

        assert_eq!(step.status, StepStatus::Failed);
        assert_eq!(step.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_planner_greedy() {
        let config = PlanningConfig {
            algorithm: PlanningAlgorithm::GreedyBestFirst,
            ..PlanningConfig::default()
        };
        let mut planner = Planner::new(config);

        planner.register_action(
            PlanAction::new("act", "Action").with_effect(Effect::set("done", "yes")),
        );

        let goal = Goal::new("g1", "Goal").with_condition(Condition::new(
            "c1",
            "done",
            ConditionOperator::Equals,
            "yes",
        ));

        let plan = planner.plan(&goal, &WorldState::new()).unwrap();
        assert_eq!(plan.steps.len(), 1);
    }

    #[test]
    fn test_planner_astar() {
        let config = PlanningConfig {
            algorithm: PlanningAlgorithm::AStar,
            ..PlanningConfig::default()
        };
        let mut planner = Planner::new(config);

        planner.register_action(
            PlanAction::new("cheap", "Cheap Action")
                .with_cost(1.0)
                .with_effect(Effect::set("done", "yes")),
        );

        planner.register_action(
            PlanAction::new("expensive", "Expensive Action")
                .with_cost(10.0)
                .with_effect(Effect::set("done", "yes")),
        );

        let goal = Goal::new("g1", "Goal").with_condition(Condition::new(
            "c1",
            "done",
            ConditionOperator::Equals,
            "yes",
        ));

        let plan = planner.plan(&goal, &WorldState::new()).unwrap();
        // Should prefer cheaper action
        assert_eq!(plan.steps.len(), 1);
        assert!(plan.total_cost <= 10.0);
    }
}
