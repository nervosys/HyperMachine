//! Agent Tasks - Task scheduling and workflow orchestration
//!
//! This module provides task management primitives for AI agents:
//! - Task definition and execution
//! - Task dependencies and DAG workflows
//! - Task scheduling and prioritization
//! - Task retry and error handling
//! - Workflow orchestration

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Result type for task operations
pub type TaskResult<T> = Result<T, TaskError>;

/// Task operation errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    /// Task not found
    TaskNotFound(String),
    /// Task already exists
    TaskExists(String),
    /// Task execution failed
    ExecutionFailed(String),
    /// Task timed out
    Timeout(String),
    /// Task cancelled
    Cancelled(String),
    /// Dependency error
    DependencyError(String),
    /// Invalid state transition
    InvalidState(String),
    /// Workflow error
    WorkflowError(String),
    /// Retry limit exceeded
    RetryLimitExceeded(String),
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TaskNotFound(id) => write!(f, "Task not found: {}", id),
            Self::TaskExists(id) => write!(f, "Task already exists: {}", id),
            Self::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            Self::Timeout(msg) => write!(f, "Task timeout: {}", msg),
            Self::Cancelled(msg) => write!(f, "Task cancelled: {}", msg),
            Self::DependencyError(msg) => write!(f, "Dependency error: {}", msg),
            Self::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            Self::WorkflowError(msg) => write!(f, "Workflow error: {}", msg),
            Self::RetryLimitExceeded(msg) => write!(f, "Retry limit exceeded: {}", msg),
        }
    }
}

impl std::error::Error for TaskError {}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is pending execution
    Pending,
    /// Task is ready to run (dependencies satisfied)
    Ready,
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed
    Failed,
    /// Task was cancelled
    Cancelled,
    /// Task is waiting for retry
    Retrying,
    /// Task is blocked on dependencies
    Blocked,
}

impl TaskStatus {
    /// Check if task is terminal (no more state changes)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Check if task is runnable
    pub fn is_runnable(&self) -> bool {
        matches!(self, Self::Ready | Self::Retrying)
    }

    /// Check if task is active (running or pending)
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

/// Task priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskPriority {
    /// Low priority
    Low = 0,
    /// Normal priority
    Normal = 1,
    /// High priority
    High = 2,
    /// Critical priority
    Critical = 3,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Task output/result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    /// Output data
    pub data: Vec<u8>,
    /// Output type
    pub content_type: String,
    /// Output metadata
    pub metadata: HashMap<String, String>,
}

impl TaskOutput {
    /// Create empty output
    pub fn empty() -> Self {
        Self {
            data: Vec::new(),
            content_type: "none".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Create from string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self {
            data: s.into().into_bytes(),
            content_type: "text/plain".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Create from JSON
    pub fn from_json<T: Serialize>(value: &T) -> TaskResult<Self> {
        let data =
            serde_json::to_vec(value).map_err(|e| TaskError::ExecutionFailed(e.to_string()))?;
        Ok(Self {
            data,
            content_type: "application/json".to_string(),
            metadata: HashMap::new(),
        })
    }

    /// Get as string
    pub fn as_string(&self) -> TaskResult<String> {
        String::from_utf8(self.data.clone()).map_err(|e| TaskError::ExecutionFailed(e.to_string()))
    }

    /// Get as JSON
    pub fn as_json<T: for<'de> Deserialize<'de>>(&self) -> TaskResult<T> {
        serde_json::from_slice(&self.data).map_err(|e| TaskError::ExecutionFailed(e.to_string()))
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get size
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Retry policy for tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retries
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff multiplier
    pub backoff_multiplier: f64,
    /// Jitter factor (0.0 to 1.0)
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            jitter: 0.1,
        }
    }
}

impl RetryPolicy {
    /// No retries
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Fixed delay retries
    pub fn fixed(max_retries: u32, delay: Duration) -> Self {
        Self {
            max_retries,
            initial_delay: delay,
            max_delay: delay,
            backoff_multiplier: 1.0,
            jitter: 0.0,
        }
    }

    /// Exponential backoff retries
    pub fn exponential(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Default::default()
        }
    }

    /// Calculate delay for a given retry attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let base_delay =
            self.initial_delay.as_secs_f64() * self.backoff_multiplier.powi(attempt as i32 - 1);
        let capped_delay = base_delay.min(self.max_delay.as_secs_f64());

        // Add jitter
        let jitter_range = capped_delay * self.jitter;
        let final_delay = capped_delay + (jitter_range * 0.5); // Simplified jitter

        Duration::from_secs_f64(final_delay)
    }

    /// Check if more retries are allowed
    pub fn allows_retry(&self, current_attempts: u32) -> bool {
        current_attempts < self.max_retries
    }
}

/// Task definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task ID
    pub id: String,
    /// Task name
    pub name: String,
    /// Task description
    pub description: String,
    /// Task type/category
    pub task_type: String,
    /// Priority
    pub priority: TaskPriority,
    /// Current status
    pub status: TaskStatus,
    /// Task input/parameters
    pub input: HashMap<String, String>,
    /// Task output
    pub output: Option<TaskOutput>,
    /// Error message if failed
    pub error: Option<String>,
    /// Dependencies (task IDs that must complete first)
    pub dependencies: Vec<String>,
    /// Timeout duration
    pub timeout: Option<Duration>,
    /// Retry policy
    pub retry_policy: RetryPolicy,
    /// Current retry attempt
    pub retry_count: u32,
    /// Creation timestamp
    pub created_at: SystemTime,
    /// Start timestamp
    pub started_at: Option<SystemTime>,
    /// Completion timestamp
    pub completed_at: Option<SystemTime>,
    /// Tags
    pub tags: HashMap<String, String>,
}

impl Task {
    /// Create a new task
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            task_type: "default".to_string(),
            priority: TaskPriority::Normal,
            status: TaskStatus::Pending,
            input: HashMap::new(),
            output: None,
            error: None,
            dependencies: Vec::new(),
            timeout: None,
            retry_policy: RetryPolicy::default(),
            retry_count: 0,
            created_at: SystemTime::now(),
            started_at: None,
            completed_at: None,
            tags: HashMap::new(),
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set task type
    pub fn with_type(mut self, task_type: impl Into<String>) -> Self {
        self.task_type = task_type.into();
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Add input parameter
    pub fn with_input(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.input.insert(key.into(), value.into());
        self
    }

    /// Add dependency
    pub fn with_dependency(mut self, task_id: impl Into<String>) -> Self {
        self.dependencies.push(task_id.into());
        self
    }

    /// Add multiple dependencies
    pub fn with_dependencies(
        mut self,
        task_ids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.dependencies
            .extend(task_ids.into_iter().map(|s| s.into()));
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set retry policy
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Add tag
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Mark task as running
    pub fn start(&mut self) -> TaskResult<()> {
        if !matches!(self.status, TaskStatus::Ready | TaskStatus::Retrying) {
            return Err(TaskError::InvalidState(format!(
                "Cannot start task in {:?} state",
                self.status
            )));
        }
        self.status = TaskStatus::Running;
        self.started_at = Some(SystemTime::now());
        Ok(())
    }

    /// Mark task as completed
    pub fn complete(&mut self, output: TaskOutput) -> TaskResult<()> {
        if self.status != TaskStatus::Running {
            return Err(TaskError::InvalidState(format!(
                "Cannot complete task in {:?} state",
                self.status
            )));
        }
        self.status = TaskStatus::Completed;
        self.output = Some(output);
        self.completed_at = Some(SystemTime::now());
        Ok(())
    }

    /// Mark task as failed
    pub fn fail(&mut self, error: impl Into<String>) -> TaskResult<()> {
        if self.status != TaskStatus::Running {
            return Err(TaskError::InvalidState(format!(
                "Cannot fail task in {:?} state",
                self.status
            )));
        }

        let error_msg = error.into();
        self.error = Some(error_msg);

        // Check if retry is allowed
        if self.retry_policy.allows_retry(self.retry_count) {
            self.status = TaskStatus::Retrying;
            self.retry_count += 1;
        } else {
            self.status = TaskStatus::Failed;
            self.completed_at = Some(SystemTime::now());
        }

        Ok(())
    }

    /// Cancel the task
    pub fn cancel(&mut self) -> TaskResult<()> {
        if self.status.is_terminal() {
            return Err(TaskError::InvalidState(format!(
                "Cannot cancel task in {:?} state",
                self.status
            )));
        }
        self.status = TaskStatus::Cancelled;
        self.completed_at = Some(SystemTime::now());
        Ok(())
    }

    /// Get execution duration
    pub fn duration(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => end.duration_since(start).ok(),
            (Some(start), None) if self.status == TaskStatus::Running => start.elapsed().ok(),
            _ => None,
        }
    }

    /// Check if task has dependencies
    pub fn has_dependencies(&self) -> bool {
        !self.dependencies.is_empty()
    }
}

/// Task queue for scheduling
#[derive(Debug)]
pub struct TaskQueue {
    /// Queued tasks by priority
    tasks: VecDeque<String>,
    /// Task lookup
    task_map: HashMap<String, Task>,
    /// Completed task IDs
    completed: HashSet<String>,
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskQueue {
    /// Create a new task queue
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            task_map: HashMap::new(),
            completed: HashSet::new(),
        }
    }

    /// Add a task to the queue
    pub fn add(&mut self, task: Task) -> TaskResult<()> {
        if self.task_map.contains_key(&task.id) {
            return Err(TaskError::TaskExists(task.id.clone()));
        }

        let id = task.id.clone();
        self.task_map.insert(id.clone(), task);
        self.tasks.push_back(id);
        self.update_ready_status();
        Ok(())
    }

    /// Get a task by ID
    pub fn get(&self, id: &str) -> Option<&Task> {
        self.task_map.get(id)
    }

    /// Get mutable task
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.task_map.get_mut(id)
    }

    /// Remove a task
    pub fn remove(&mut self, id: &str) -> Option<Task> {
        self.tasks.retain(|t| t != id);
        self.task_map.remove(id)
    }

    /// Get the next ready task (highest priority first)
    pub fn next_ready(&mut self) -> Option<&mut Task> {
        self.update_ready_status();

        // Find highest priority ready task
        let ready_id = self
            .task_map
            .values()
            .filter(|t| t.status.is_runnable())
            .max_by_key(|t| t.priority)
            .map(|t| t.id.clone());

        ready_id.and_then(move |id| self.task_map.get_mut(&id))
    }

    /// Update task ready status based on dependencies
    fn update_ready_status(&mut self) {
        let completed = &self.completed;

        for task in self.task_map.values_mut() {
            // Only update Pending or Blocked tasks
            if task.status == TaskStatus::Pending || task.status == TaskStatus::Blocked {
                let deps_satisfied = task.dependencies.iter().all(|d| completed.contains(d));
                if deps_satisfied {
                    task.status = TaskStatus::Ready;
                } else {
                    task.status = TaskStatus::Blocked;
                }
            }
        }
    }

    /// Mark a task as completed
    pub fn mark_completed(&mut self, id: &str) {
        self.completed.insert(id.to_string());
        self.update_ready_status();
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.task_map.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.task_map.is_empty()
    }

    /// Get pending task count
    pub fn pending_count(&self) -> usize {
        self.task_map
            .values()
            .filter(|t| t.status.is_active())
            .count()
    }

    /// Get completed task count
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    /// List all tasks
    pub fn list(&self) -> Vec<&Task> {
        self.task_map.values().collect()
    }

    /// List tasks by status
    pub fn list_by_status(&self, status: TaskStatus) -> Vec<&Task> {
        self.task_map
            .values()
            .filter(|t| t.status == status)
            .collect()
    }
}

/// Workflow definition (DAG of tasks)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Workflow ID
    pub id: String,
    /// Workflow name
    pub name: String,
    /// Description
    pub description: String,
    /// Task definitions
    pub tasks: Vec<Task>,
    /// Workflow status
    pub status: WorkflowStatus,
    /// Creation time
    pub created_at: SystemTime,
    /// Start time
    pub started_at: Option<SystemTime>,
    /// Completion time
    pub completed_at: Option<SystemTime>,
}

/// Workflow status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// Not started
    Pending,
    /// Currently running
    Running,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

impl Workflow {
    /// Create a new workflow
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            tasks: Vec::new(),
            status: WorkflowStatus::Pending,
            created_at: SystemTime::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a task
    pub fn with_task(mut self, task: Task) -> Self {
        self.tasks.push(task);
        self
    }

    /// Get task count
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Check if workflow has cycles (invalid DAG)
    pub fn has_cycles(&self) -> bool {
        let task_ids: HashSet<_> = self.tasks.iter().map(|t| t.id.as_str()).collect();
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();

        for task in &self.tasks {
            if self.has_cycle_from(&task.id, &task_ids, &mut visited, &mut in_stack) {
                return true;
            }
        }
        false
    }

    fn has_cycle_from(
        &self,
        task_id: &str,
        task_ids: &HashSet<&str>,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
    ) -> bool {
        if in_stack.contains(task_id) {
            return true;
        }
        if visited.contains(task_id) {
            return false;
        }

        visited.insert(task_id.to_string());
        in_stack.insert(task_id.to_string());

        if let Some(task) = self.tasks.iter().find(|t| t.id == task_id) {
            for dep in &task.dependencies {
                if task_ids.contains(dep.as_str())
                    && self.has_cycle_from(dep, task_ids, visited, in_stack)
                {
                    return true;
                }
            }
        }

        in_stack.remove(task_id);
        false
    }

    /// Validate workflow
    pub fn validate(&self) -> TaskResult<()> {
        // Check for cycles
        if self.has_cycles() {
            return Err(TaskError::WorkflowError("Workflow contains cycles".into()));
        }

        // Check dependencies exist
        let task_ids: HashSet<_> = self.tasks.iter().map(|t| &t.id).collect();
        for task in &self.tasks {
            for dep in &task.dependencies {
                if !task_ids.contains(dep) {
                    return Err(TaskError::DependencyError(format!(
                        "Task {} depends on non-existent task {}",
                        task.id, dep
                    )));
                }
            }
        }

        Ok(())
    }

    /// Get tasks in topological order
    pub fn topological_order(&self) -> TaskResult<Vec<&Task>> {
        self.validate()?;

        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();

        fn visit<'a>(
            task_id: &str,
            tasks: &'a [Task],
            visited: &mut HashSet<String>,
            temp_visited: &mut HashSet<String>,
            result: &mut Vec<&'a Task>,
        ) -> TaskResult<()> {
            if temp_visited.contains(task_id) {
                return Err(TaskError::WorkflowError("Cycle detected".into()));
            }
            if visited.contains(task_id) {
                return Ok(());
            }

            temp_visited.insert(task_id.to_string());

            if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
                for dep in &task.dependencies {
                    visit(dep, tasks, visited, temp_visited, result)?;
                }
                temp_visited.remove(task_id);
                visited.insert(task_id.to_string());
                result.push(task);
            }

            Ok(())
        }

        for task in &self.tasks {
            visit(
                &task.id,
                &self.tasks,
                &mut visited,
                &mut temp_visited,
                &mut result,
            )?;
        }

        Ok(result)
    }
}

/// Workflow executor
#[derive(Debug)]
pub struct WorkflowExecutor {
    /// Current workflow
    workflow: Option<Workflow>,
    /// Task queue
    queue: TaskQueue,
    /// Execution stats
    stats: ExecutionStats,
}

/// Execution statistics
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    /// Total tasks processed
    pub tasks_processed: u64,
    /// Tasks completed
    pub tasks_completed: u64,
    /// Tasks failed
    pub tasks_failed: u64,
    /// Tasks retried
    pub tasks_retried: u64,
    /// Total execution time
    pub total_duration: Duration,
}

impl Default for WorkflowExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowExecutor {
    /// Create a new workflow executor
    pub fn new() -> Self {
        Self {
            workflow: None,
            queue: TaskQueue::new(),
            stats: ExecutionStats::default(),
        }
    }

    /// Load a workflow
    pub fn load(&mut self, workflow: Workflow) -> TaskResult<()> {
        workflow.validate()?;

        // Add all tasks to queue
        for task in workflow.tasks.clone() {
            self.queue.add(task)?;
        }

        self.workflow = Some(workflow);
        Ok(())
    }

    /// Get next task to execute
    pub fn next_task(&mut self) -> Option<&mut Task> {
        self.queue.next_ready()
    }

    /// Complete a task
    pub fn complete_task(&mut self, task_id: &str, output: TaskOutput) -> TaskResult<()> {
        let task = self
            .queue
            .get_mut(task_id)
            .ok_or_else(|| TaskError::TaskNotFound(task_id.to_string()))?;

        task.complete(output)?;
        self.queue.mark_completed(task_id);
        self.stats.tasks_processed += 1;
        self.stats.tasks_completed += 1;

        Ok(())
    }

    /// Fail a task
    pub fn fail_task(&mut self, task_id: &str, error: impl Into<String>) -> TaskResult<()> {
        let task = self
            .queue
            .get_mut(task_id)
            .ok_or_else(|| TaskError::TaskNotFound(task_id.to_string()))?;

        task.fail(error)?;
        self.stats.tasks_processed += 1;

        if task.status == TaskStatus::Retrying {
            self.stats.tasks_retried += 1;
        } else {
            self.stats.tasks_failed += 1;
        }

        Ok(())
    }

    /// Get execution stats
    pub fn stats(&self) -> &ExecutionStats {
        &self.stats
    }

    /// Check if workflow is complete
    pub fn is_complete(&self) -> bool {
        self.queue.pending_count() == 0
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        let total = self.queue.len();
        if total == 0 {
            return 1.0;
        }
        self.queue.completed_count() as f64 / total as f64
    }
}

/// Thread-safe task scheduler
#[derive(Debug, Clone)]
pub struct TaskScheduler {
    inner: Arc<Mutex<TaskQueue>>,
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskScheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TaskQueue::new())),
        }
    }

    /// Submit a task
    pub fn submit(&self, task: Task) -> TaskResult<String> {
        let id = task.id.clone();
        self.inner.lock().unwrap().add(task)?;
        Ok(id)
    }

    /// Get task status
    pub fn status(&self, id: &str) -> Option<TaskStatus> {
        self.inner.lock().unwrap().get(id).map(|t| t.status)
    }

    /// Cancel a task
    pub fn cancel(&self, id: &str) -> TaskResult<()> {
        let mut guard = self.inner.lock().unwrap();
        let task = guard
            .get_mut(id)
            .ok_or_else(|| TaskError::TaskNotFound(id.to_string()))?;
        task.cancel()
    }

    /// Get pending count
    pub fn pending_count(&self) -> usize {
        self.inner.lock().unwrap().pending_count()
    }

    /// Get total count
    pub fn total_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_error_display() {
        let err = TaskError::TaskNotFound("task-1".into());
        assert!(err.to_string().contains("Task not found"));

        let err = TaskError::Timeout("30s".into());
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_task_status() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());

        assert!(TaskStatus::Ready.is_runnable());
        assert!(TaskStatus::Retrying.is_runnable());
        assert!(!TaskStatus::Pending.is_runnable());

        assert!(TaskStatus::Running.is_active());
        assert!(!TaskStatus::Completed.is_active());
    }

    #[test]
    fn test_task_output() {
        let output = TaskOutput::from_string("hello");
        assert_eq!(output.as_string().unwrap(), "hello");
        assert_eq!(output.content_type, "text/plain");

        let empty = TaskOutput::empty();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_task_output_json() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Data {
            value: i32,
        }

        let data = Data { value: 42 };
        let output = TaskOutput::from_json(&data).unwrap();
        let decoded: Data = output.as_json().unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert!(policy.allows_retry(0));
        assert!(policy.allows_retry(2));
        assert!(!policy.allows_retry(3));
    }

    #[test]
    fn test_retry_policy_delay() {
        let policy = RetryPolicy::fixed(3, Duration::from_secs(5));

        assert_eq!(policy.delay_for_attempt(0), Duration::ZERO);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(5));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(5));
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy = RetryPolicy::exponential(5);

        let d1 = policy.delay_for_attempt(1);
        let d2 = policy.delay_for_attempt(2);
        let d3 = policy.delay_for_attempt(3);

        assert!(d2 > d1);
        assert!(d3 > d2);
    }

    #[test]
    fn test_task_creation() {
        let task = Task::new("task-1", "Test Task")
            .with_description("A test task")
            .with_type("unit-test")
            .with_priority(TaskPriority::High)
            .with_input("key", "value")
            .with_timeout(Duration::from_secs(60));

        assert_eq!(task.id, "task-1");
        assert_eq!(task.name, "Test Task");
        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.input.get("key"), Some(&"value".to_string()));
        assert!(task.timeout.is_some());
    }

    #[test]
    fn test_task_dependencies() {
        let task = Task::new("task-1", "Test")
            .with_dependency("dep-1")
            .with_dependencies(["dep-2", "dep-3"]);

        assert!(task.has_dependencies());
        assert_eq!(task.dependencies.len(), 3);
    }

    #[test]
    fn test_task_lifecycle() {
        let mut task = Task::new("task-1", "Test");
        task.status = TaskStatus::Ready;

        // Start
        task.start().unwrap();
        assert_eq!(task.status, TaskStatus::Running);
        assert!(task.started_at.is_some());

        // Complete
        task.complete(TaskOutput::from_string("done")).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.output.is_some());
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_task_failure_with_retry() {
        let mut task = Task::new("task-1", "Test")
            .with_retry_policy(RetryPolicy::fixed(2, Duration::from_secs(1)));
        task.status = TaskStatus::Ready;

        task.start().unwrap();
        task.fail("error 1").unwrap();
        assert_eq!(task.status, TaskStatus::Retrying);
        assert_eq!(task.retry_count, 1);

        task.status = TaskStatus::Ready; // Simulate retry ready
        task.start().unwrap();
        task.fail("error 2").unwrap();
        assert_eq!(task.status, TaskStatus::Retrying);
        assert_eq!(task.retry_count, 2);

        task.status = TaskStatus::Ready;
        task.start().unwrap();
        task.fail("error 3").unwrap();
        assert_eq!(task.status, TaskStatus::Failed); // No more retries
    }

    #[test]
    fn test_task_cancel() {
        let mut task = Task::new("task-1", "Test");

        task.cancel().unwrap();
        assert_eq!(task.status, TaskStatus::Cancelled);

        // Cannot cancel completed task
        let mut completed = Task::new("task-2", "Test");
        completed.status = TaskStatus::Completed;
        assert!(completed.cancel().is_err());
    }

    #[test]
    fn test_task_queue_basic() {
        let mut queue = TaskQueue::new();

        queue.add(Task::new("task-1", "Task 1")).unwrap();
        queue.add(Task::new("task-2", "Task 2")).unwrap();

        assert_eq!(queue.len(), 2);
        assert!(!queue.is_empty());
    }

    #[test]
    fn test_task_queue_duplicate() {
        let mut queue = TaskQueue::new();

        queue.add(Task::new("task-1", "Task 1")).unwrap();
        let result = queue.add(Task::new("task-1", "Duplicate"));

        assert!(matches!(result, Err(TaskError::TaskExists(_))));
    }

    #[test]
    fn test_task_queue_priority() {
        let mut queue = TaskQueue::new();

        queue
            .add(Task::new("low", "Low").with_priority(TaskPriority::Low))
            .unwrap();
        queue
            .add(Task::new("high", "High").with_priority(TaskPriority::High))
            .unwrap();
        queue
            .add(Task::new("normal", "Normal").with_priority(TaskPriority::Normal))
            .unwrap();

        let next = queue.next_ready().unwrap();
        assert_eq!(next.id, "high");
    }

    #[test]
    fn test_task_queue_dependencies() {
        let mut queue = TaskQueue::new();

        queue.add(Task::new("task-1", "Task 1")).unwrap();
        queue
            .add(Task::new("task-2", "Task 2").with_dependency("task-1"))
            .unwrap();

        // task-2 should be blocked
        assert_eq!(queue.get("task-2").unwrap().status, TaskStatus::Blocked);

        // Complete task-1
        queue.mark_completed("task-1");

        // Now task-2 should be ready
        assert_eq!(queue.get("task-2").unwrap().status, TaskStatus::Ready);
    }

    #[test]
    fn test_workflow_creation() {
        let workflow = Workflow::new("wf-1", "Test Workflow")
            .with_description("A test workflow")
            .with_task(Task::new("task-1", "Task 1"))
            .with_task(Task::new("task-2", "Task 2").with_dependency("task-1"));

        assert_eq!(workflow.id, "wf-1");
        assert_eq!(workflow.task_count(), 2);
    }

    #[test]
    fn test_workflow_validation() {
        let workflow = Workflow::new("wf-1", "Test")
            .with_task(Task::new("task-1", "Task 1"))
            .with_task(Task::new("task-2", "Task 2").with_dependency("task-1"));

        assert!(workflow.validate().is_ok());
    }

    #[test]
    fn test_workflow_missing_dependency() {
        let workflow = Workflow::new("wf-1", "Test")
            .with_task(Task::new("task-1", "Task 1").with_dependency("non-existent"));

        let result = workflow.validate();
        assert!(matches!(result, Err(TaskError::DependencyError(_))));
    }

    #[test]
    fn test_workflow_cycle_detection() {
        let workflow = Workflow::new("wf-1", "Test")
            .with_task(Task::new("a", "A").with_dependency("c"))
            .with_task(Task::new("b", "B").with_dependency("a"))
            .with_task(Task::new("c", "C").with_dependency("b"));

        assert!(workflow.has_cycles());
        assert!(matches!(
            workflow.validate(),
            Err(TaskError::WorkflowError(_))
        ));
    }

    #[test]
    fn test_workflow_topological_order() {
        let workflow = Workflow::new("wf-1", "Test")
            .with_task(Task::new("c", "C").with_dependencies(["a", "b"]))
            .with_task(Task::new("a", "A"))
            .with_task(Task::new("b", "B").with_dependency("a"));

        let order = workflow.topological_order().unwrap();
        let ids: Vec<_> = order.iter().map(|t| t.id.as_str()).collect();

        // a must come before b and c, b must come before c
        let pos_a = ids.iter().position(|&id| id == "a").unwrap();
        let pos_b = ids.iter().position(|&id| id == "b").unwrap();
        let pos_c = ids.iter().position(|&id| id == "c").unwrap();

        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_workflow_executor() {
        let workflow = Workflow::new("wf-1", "Test")
            .with_task(Task::new("task-1", "Task 1"))
            .with_task(Task::new("task-2", "Task 2").with_dependency("task-1"));

        let mut executor = WorkflowExecutor::new();
        executor.load(workflow).unwrap();

        assert!(!executor.is_complete());
        assert_eq!(executor.progress(), 0.0);

        // Get first task
        let task = executor.next_task().unwrap();
        assert_eq!(task.id, "task-1");
        task.start().unwrap();

        // Complete it
        executor
            .complete_task("task-1", TaskOutput::empty())
            .unwrap();

        // Get second task
        let task = executor.next_task().unwrap();
        assert_eq!(task.id, "task-2");
        task.start().unwrap();

        executor
            .complete_task("task-2", TaskOutput::empty())
            .unwrap();

        assert!(executor.is_complete());
        assert_eq!(executor.progress(), 1.0);
        assert_eq!(executor.stats().tasks_completed, 2);
    }

    #[test]
    fn test_task_scheduler() {
        let scheduler = TaskScheduler::new();

        let id = scheduler.submit(Task::new("task-1", "Task 1")).unwrap();
        assert_eq!(id, "task-1");

        assert_eq!(scheduler.status("task-1"), Some(TaskStatus::Ready));
        assert_eq!(scheduler.total_count(), 1);
        assert_eq!(scheduler.pending_count(), 1);

        scheduler.cancel("task-1").unwrap();
        assert_eq!(scheduler.status("task-1"), Some(TaskStatus::Cancelled));
    }

    #[test]
    fn test_execution_stats() {
        let stats = ExecutionStats::default();

        assert_eq!(stats.tasks_processed, 0);
        assert_eq!(stats.tasks_completed, 0);
        assert_eq!(stats.tasks_failed, 0);
    }
}
