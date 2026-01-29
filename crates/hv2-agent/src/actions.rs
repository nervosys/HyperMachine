//! Agent actions for VM control
//!
//! This module provides a comprehensive set of actions that AI agents can
//! perform on virtual machines, with validation and safety checks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Action result type
pub type ActionResult<T> = Result<T, ActionError>;

/// Action execution errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    /// Action not supported
    NotSupported(String),
    /// Invalid parameters
    InvalidParameters(String),
    /// Permission denied
    PermissionDenied(String),
    /// Resource not available
    ResourceUnavailable(String),
    /// Action timed out
    Timeout(String),
    /// Action already in progress
    AlreadyInProgress(String),
    /// VM not found
    VmNotFound(String),
    /// Action failed
    Failed(String),
    /// Action cancelled
    Cancelled,
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::NotSupported(msg) => write!(f, "Action not supported: {}", msg),
            ActionError::InvalidParameters(msg) => write!(f, "Invalid parameters: {}", msg),
            ActionError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            ActionError::ResourceUnavailable(msg) => write!(f, "Resource unavailable: {}", msg),
            ActionError::Timeout(msg) => write!(f, "Action timed out: {}", msg),
            ActionError::AlreadyInProgress(msg) => write!(f, "Action already in progress: {}", msg),
            ActionError::VmNotFound(msg) => write!(f, "VM not found: {}", msg),
            ActionError::Failed(msg) => write!(f, "Action failed: {}", msg),
            ActionError::Cancelled => write!(f, "Action cancelled"),
        }
    }
}

impl std::error::Error for ActionError {}

/// Action category for organization and permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionCategory {
    /// Power management actions
    Power,
    /// Snapshot actions
    Snapshot,
    /// Resource management
    Resource,
    /// Network actions
    Network,
    /// Storage actions
    Storage,
    /// Debug/introspection
    Debug,
    /// Configuration
    Config,
    /// Migration
    Migration,
    /// Security
    Security,
}

impl std::fmt::Display for ActionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionCategory::Power => write!(f, "power"),
            ActionCategory::Snapshot => write!(f, "snapshot"),
            ActionCategory::Resource => write!(f, "resource"),
            ActionCategory::Network => write!(f, "network"),
            ActionCategory::Storage => write!(f, "storage"),
            ActionCategory::Debug => write!(f, "debug"),
            ActionCategory::Config => write!(f, "config"),
            ActionCategory::Migration => write!(f, "migration"),
            ActionCategory::Security => write!(f, "security"),
        }
    }
}

/// Power action types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerAction {
    /// Start the VM
    Start,
    /// Stop the VM (graceful)
    Stop,
    /// Force stop (power off)
    ForceStop,
    /// Pause VM execution
    Pause,
    /// Resume VM execution
    Resume,
    /// Reboot the VM
    Reboot,
    /// Reset the VM
    Reset,
    /// Suspend to disk
    Hibernate,
    /// Suspend to RAM
    Suspend,
}

impl std::fmt::Display for PowerAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PowerAction::Start => write!(f, "start"),
            PowerAction::Stop => write!(f, "stop"),
            PowerAction::ForceStop => write!(f, "force_stop"),
            PowerAction::Pause => write!(f, "pause"),
            PowerAction::Resume => write!(f, "resume"),
            PowerAction::Reboot => write!(f, "reboot"),
            PowerAction::Reset => write!(f, "reset"),
            PowerAction::Hibernate => write!(f, "hibernate"),
            PowerAction::Suspend => write!(f, "suspend"),
        }
    }
}

/// Snapshot action types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotAction {
    /// Create a new snapshot
    Create {
        name: String,
        description: Option<String>,
    },
    /// Restore from snapshot
    Restore { name: String },
    /// Delete a snapshot
    Delete { name: String },
    /// List all snapshots
    List,
    /// Export snapshot
    Export { name: String, path: String },
    /// Import snapshot
    Import { path: String },
}

/// Resource action types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceAction {
    /// Get current resource usage
    GetUsage,
    /// Set CPU count
    SetCpuCount { count: u32 },
    /// Set memory size (bytes)
    SetMemory { bytes: u64 },
    /// Hot-add CPU
    HotAddCpu { count: u32 },
    /// Hot-remove CPU
    HotRemoveCpu { count: u32 },
    /// Balloon memory (reduce guest memory)
    BalloonMemory { target_bytes: u64 },
    /// Set CPU affinity
    SetCpuAffinity { vcpu: u32, host_cpus: Vec<u32> },
}

/// Network action types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkAction {
    /// Attach a network interface
    AttachInterface { name: String, network: String },
    /// Detach a network interface
    DetachInterface { name: String },
    /// Set interface state (up/down)
    SetInterfaceState { name: String, up: bool },
    /// Configure bandwidth limit
    SetBandwidthLimit { name: String, bytes_per_sec: u64 },
    /// Get network statistics
    GetStats { name: String },
    /// Add firewall rule
    AddFirewallRule { rule: FirewallRule },
    /// Remove firewall rule
    RemoveFirewallRule { rule_id: u64 },
}

/// Firewall rule definition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallRule {
    /// Rule ID
    pub id: u64,
    /// Direction (inbound/outbound)
    pub direction: Direction,
    /// Protocol
    pub protocol: Protocol,
    /// Source address/CIDR
    pub source: Option<String>,
    /// Destination address/CIDR
    pub destination: Option<String>,
    /// Port or port range
    pub port: Option<PortRange>,
    /// Action to take
    pub action: FirewallAction,
    /// Priority (lower = higher priority)
    pub priority: u32,
}

/// Traffic direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Inbound,
    Outbound,
    Both,
}

/// Network protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Any,
}

/// Port range
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub fn single(port: u16) -> Self {
        Self {
            start: port,
            end: port,
        }
    }

    pub fn range(start: u16, end: u16) -> Self {
        Self { start, end }
    }
}

/// Firewall action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FirewallAction {
    Allow,
    Deny,
    Log,
}

/// Storage action types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageAction {
    /// Attach a disk
    AttachDisk { path: String, readonly: bool },
    /// Detach a disk
    DetachDisk { path: String },
    /// Resize a disk
    ResizeDisk { path: String, new_size: u64 },
    /// Create a snapshot of disk
    SnapshotDisk { path: String, snapshot_name: String },
    /// Get disk statistics
    GetDiskStats { path: String },
    /// Set IO throttling
    SetIoThrottle { path: String, iops: u64 },
}

/// Debug action types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugAction {
    /// Dump VM state
    DumpState,
    /// Read memory region
    ReadMemory { address: u64, size: u64 },
    /// Read CPU registers
    ReadRegisters { vcpu: u32 },
    /// Set breakpoint
    SetBreakpoint { address: u64 },
    /// Clear breakpoint
    ClearBreakpoint { address: u64 },
    /// Single step execution
    SingleStep { vcpu: u32 },
    /// Get execution trace
    GetTrace { count: usize },
}

/// Action request with parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    /// Unique request ID
    pub id: u64,
    /// Target VM ID
    pub vm_id: String,
    /// Action to perform
    pub action: AgentAction,
    /// Request timeout
    pub timeout: Option<Duration>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl ActionRequest {
    /// Create a new action request
    pub fn new(vm_id: impl Into<String>, action: AgentAction) -> Self {
        static REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            vm_id: vm_id.into(),
            action,
            timeout: None,
            metadata: HashMap::new(),
        }
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get the action category
    pub fn category(&self) -> ActionCategory {
        self.action.category()
    }
}

/// Agent action enum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentAction {
    /// Power management
    Power(PowerAction),
    /// Snapshot management
    Snapshot(SnapshotAction),
    /// Resource management
    Resource(ResourceAction),
    /// Network management
    Network(NetworkAction),
    /// Storage management
    Storage(StorageAction),
    /// Debug/introspection
    Debug(DebugAction),
}

impl AgentAction {
    /// Get the action category
    pub fn category(&self) -> ActionCategory {
        match self {
            AgentAction::Power(_) => ActionCategory::Power,
            AgentAction::Snapshot(_) => ActionCategory::Snapshot,
            AgentAction::Resource(_) => ActionCategory::Resource,
            AgentAction::Network(_) => ActionCategory::Network,
            AgentAction::Storage(_) => ActionCategory::Storage,
            AgentAction::Debug(_) => ActionCategory::Debug,
        }
    }

    /// Get action name for logging
    pub fn name(&self) -> &'static str {
        match self {
            AgentAction::Power(p) => match p {
                PowerAction::Start => "power.start",
                PowerAction::Stop => "power.stop",
                PowerAction::ForceStop => "power.force_stop",
                PowerAction::Pause => "power.pause",
                PowerAction::Resume => "power.resume",
                PowerAction::Reboot => "power.reboot",
                PowerAction::Reset => "power.reset",
                PowerAction::Hibernate => "power.hibernate",
                PowerAction::Suspend => "power.suspend",
            },
            AgentAction::Snapshot(s) => match s {
                SnapshotAction::Create { .. } => "snapshot.create",
                SnapshotAction::Restore { .. } => "snapshot.restore",
                SnapshotAction::Delete { .. } => "snapshot.delete",
                SnapshotAction::List => "snapshot.list",
                SnapshotAction::Export { .. } => "snapshot.export",
                SnapshotAction::Import { .. } => "snapshot.import",
            },
            AgentAction::Resource(r) => match r {
                ResourceAction::GetUsage => "resource.get_usage",
                ResourceAction::SetCpuCount { .. } => "resource.set_cpu_count",
                ResourceAction::SetMemory { .. } => "resource.set_memory",
                ResourceAction::HotAddCpu { .. } => "resource.hot_add_cpu",
                ResourceAction::HotRemoveCpu { .. } => "resource.hot_remove_cpu",
                ResourceAction::BalloonMemory { .. } => "resource.balloon_memory",
                ResourceAction::SetCpuAffinity { .. } => "resource.set_cpu_affinity",
            },
            AgentAction::Network(n) => match n {
                NetworkAction::AttachInterface { .. } => "network.attach_interface",
                NetworkAction::DetachInterface { .. } => "network.detach_interface",
                NetworkAction::SetInterfaceState { .. } => "network.set_interface_state",
                NetworkAction::SetBandwidthLimit { .. } => "network.set_bandwidth_limit",
                NetworkAction::GetStats { .. } => "network.get_stats",
                NetworkAction::AddFirewallRule { .. } => "network.add_firewall_rule",
                NetworkAction::RemoveFirewallRule { .. } => "network.remove_firewall_rule",
            },
            AgentAction::Storage(s) => match s {
                StorageAction::AttachDisk { .. } => "storage.attach_disk",
                StorageAction::DetachDisk { .. } => "storage.detach_disk",
                StorageAction::ResizeDisk { .. } => "storage.resize_disk",
                StorageAction::SnapshotDisk { .. } => "storage.snapshot_disk",
                StorageAction::GetDiskStats { .. } => "storage.get_disk_stats",
                StorageAction::SetIoThrottle { .. } => "storage.set_io_throttle",
            },
            AgentAction::Debug(d) => match d {
                DebugAction::DumpState => "debug.dump_state",
                DebugAction::ReadMemory { .. } => "debug.read_memory",
                DebugAction::ReadRegisters { .. } => "debug.read_registers",
                DebugAction::SetBreakpoint { .. } => "debug.set_breakpoint",
                DebugAction::ClearBreakpoint { .. } => "debug.clear_breakpoint",
                DebugAction::SingleStep { .. } => "debug.single_step",
                DebugAction::GetTrace { .. } => "debug.get_trace",
            },
        }
    }
}

/// Action response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResponse {
    /// Request ID this responds to
    pub request_id: u64,
    /// Success status
    pub success: bool,
    /// Result data (if successful)
    pub data: Option<serde_json::Value>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Execution duration
    pub duration: Duration,
}

impl ActionResponse {
    /// Create a success response
    pub fn success(request_id: u64, data: serde_json::Value, duration: Duration) -> Self {
        Self {
            request_id,
            success: true,
            data: Some(data),
            error: None,
            duration,
        }
    }

    /// Create a failure response
    pub fn failure(request_id: u64, error: impl Into<String>, duration: Duration) -> Self {
        Self {
            request_id,
            success: false,
            data: None,
            error: Some(error.into()),
            duration,
        }
    }
}

/// Action validator for checking permissions and parameters
pub struct ActionValidator {
    /// Allowed categories
    allowed_categories: Vec<ActionCategory>,
    /// Maximum memory for resource actions
    max_memory: u64,
    /// Maximum CPUs
    max_cpus: u32,
    /// Allow debug actions
    allow_debug: bool,
}

impl ActionValidator {
    /// Create a new validator with all permissions
    pub fn new() -> Self {
        Self {
            allowed_categories: vec![
                ActionCategory::Power,
                ActionCategory::Snapshot,
                ActionCategory::Resource,
                ActionCategory::Network,
                ActionCategory::Storage,
                ActionCategory::Debug,
                ActionCategory::Config,
                ActionCategory::Migration,
                ActionCategory::Security,
            ],
            max_memory: 64 * 1024 * 1024 * 1024, // 64 GB
            max_cpus: 64,
            allow_debug: true,
        }
    }

    /// Create a restricted validator
    pub fn restricted() -> Self {
        Self {
            allowed_categories: vec![ActionCategory::Power, ActionCategory::Resource],
            max_memory: 8 * 1024 * 1024 * 1024, // 8 GB
            max_cpus: 8,
            allow_debug: false,
        }
    }

    /// Set allowed categories
    pub fn with_categories(mut self, categories: Vec<ActionCategory>) -> Self {
        self.allowed_categories = categories;
        self
    }

    /// Set max memory
    pub fn with_max_memory(mut self, bytes: u64) -> Self {
        self.max_memory = bytes;
        self
    }

    /// Set max CPUs
    pub fn with_max_cpus(mut self, cpus: u32) -> Self {
        self.max_cpus = cpus;
        self
    }

    /// Allow/disallow debug actions
    pub fn with_debug(mut self, allow: bool) -> Self {
        self.allow_debug = allow;
        self
    }

    /// Validate an action request
    pub fn validate(&self, request: &ActionRequest) -> ActionResult<()> {
        // Check category is allowed
        let category = request.category();
        if !self.allowed_categories.contains(&category) {
            return Err(ActionError::PermissionDenied(format!(
                "Category '{}' not allowed",
                category
            )));
        }

        // Check debug permission
        if matches!(request.action, AgentAction::Debug(_)) && !self.allow_debug {
            return Err(ActionError::PermissionDenied(
                "Debug actions not allowed".to_string(),
            ));
        }

        // Validate specific action parameters
        match &request.action {
            AgentAction::Resource(ResourceAction::SetMemory { bytes }) => {
                if *bytes > self.max_memory {
                    return Err(ActionError::InvalidParameters(format!(
                        "Memory {} exceeds maximum {}",
                        bytes, self.max_memory
                    )));
                }
            }
            AgentAction::Resource(ResourceAction::SetCpuCount { count }) => {
                if *count > self.max_cpus {
                    return Err(ActionError::InvalidParameters(format!(
                        "CPU count {} exceeds maximum {}",
                        count, self.max_cpus
                    )));
                }
            }
            AgentAction::Resource(ResourceAction::HotAddCpu { count }) => {
                if *count > self.max_cpus {
                    return Err(ActionError::InvalidParameters(format!(
                        "CPU count {} exceeds maximum {}",
                        count, self.max_cpus
                    )));
                }
            }
            _ => {}
        }

        Ok(())
    }
}

impl Default for ActionValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Action executor interface
#[async_trait::async_trait]
pub trait ActionExecutor: Send + Sync {
    /// Execute an action
    async fn execute(&self, request: ActionRequest) -> ActionResult<ActionResponse>;

    /// Check if action is supported
    fn supports(&self, action: &AgentAction) -> bool;

    /// Get executor name
    fn name(&self) -> &str;
}

/// Simple action executor for testing
#[derive(Debug, Default)]
pub struct MockExecutor {
    supported_categories: Vec<ActionCategory>,
}

impl MockExecutor {
    pub fn new() -> Self {
        Self {
            supported_categories: vec![
                ActionCategory::Power,
                ActionCategory::Snapshot,
                ActionCategory::Resource,
            ],
        }
    }

    pub fn with_categories(mut self, categories: Vec<ActionCategory>) -> Self {
        self.supported_categories = categories;
        self
    }
}

#[async_trait::async_trait]
impl ActionExecutor for MockExecutor {
    async fn execute(&self, request: ActionRequest) -> ActionResult<ActionResponse> {
        if !self.supports(&request.action) {
            return Err(ActionError::NotSupported(format!(
                "Action '{}' not supported",
                request.action.name()
            )));
        }

        // Simulate execution
        let start = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let duration = start.elapsed();

        Ok(ActionResponse::success(
            request.id,
            serde_json::json!({
                "action": request.action.name(),
                "vm_id": request.vm_id,
                "mock": true
            }),
            duration,
        ))
    }

    fn supports(&self, action: &AgentAction) -> bool {
        self.supported_categories.contains(&action.category())
    }

    fn name(&self) -> &str {
        "mock_executor"
    }
}

/// Action queue for batching and prioritization
#[derive(Debug)]
pub struct ActionQueue {
    /// Pending actions
    pending: std::sync::Mutex<Vec<ActionRequest>>,
    /// In-progress actions
    in_progress: std::sync::Mutex<Vec<u64>>,
    /// Maximum queue size
    max_size: usize,
}

impl ActionQueue {
    /// Create a new action queue
    pub fn new(max_size: usize) -> Self {
        Self {
            pending: std::sync::Mutex::new(Vec::new()),
            in_progress: std::sync::Mutex::new(Vec::new()),
            max_size,
        }
    }

    /// Enqueue an action
    pub fn enqueue(&self, request: ActionRequest) -> ActionResult<()> {
        let mut pending = self.pending.lock().unwrap();
        if pending.len() >= self.max_size {
            return Err(ActionError::ResourceUnavailable(
                "Action queue full".to_string(),
            ));
        }
        pending.push(request);
        Ok(())
    }

    /// Dequeue the next action
    pub fn dequeue(&self) -> Option<ActionRequest> {
        let mut pending = self.pending.lock().unwrap();
        if pending.is_empty() {
            return None;
        }
        let request = pending.remove(0);
        self.in_progress.lock().unwrap().push(request.id);
        Some(request)
    }

    /// Mark action as complete
    pub fn complete(&self, request_id: u64) {
        self.in_progress
            .lock()
            .unwrap()
            .retain(|id| *id != request_id);
    }

    /// Get pending count
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    /// Get in-progress count
    pub fn in_progress_count(&self) -> usize {
        self.in_progress.lock().unwrap().len()
    }

    /// Check if action is in progress
    pub fn is_in_progress(&self, request_id: u64) -> bool {
        self.in_progress.lock().unwrap().contains(&request_id)
    }
}

impl Default for ActionQueue {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_error_display() {
        let err = ActionError::PermissionDenied("test".to_string());
        assert!(format!("{}", err).contains("Permission denied"));
    }

    #[test]
    fn test_action_category_display() {
        assert_eq!(format!("{}", ActionCategory::Power), "power");
        assert_eq!(format!("{}", ActionCategory::Snapshot), "snapshot");
    }

    #[test]
    fn test_power_action_display() {
        assert_eq!(format!("{}", PowerAction::Start), "start");
        assert_eq!(format!("{}", PowerAction::ForceStop), "force_stop");
    }

    #[test]
    fn test_action_request_creation() {
        let request = ActionRequest::new("vm-123", AgentAction::Power(PowerAction::Start));

        assert!(request.id > 0);
        assert_eq!(request.vm_id, "vm-123");
        assert_eq!(request.category(), ActionCategory::Power);
    }

    #[test]
    fn test_action_request_builder() {
        let request = ActionRequest::new("vm-123", AgentAction::Power(PowerAction::Stop))
            .with_timeout(Duration::from_secs(30))
            .with_metadata("reason", "maintenance");

        assert_eq!(request.timeout, Some(Duration::from_secs(30)));
        assert_eq!(
            request.metadata.get("reason"),
            Some(&"maintenance".to_string())
        );
    }

    #[test]
    fn test_agent_action_category() {
        assert_eq!(
            AgentAction::Power(PowerAction::Start).category(),
            ActionCategory::Power
        );
        assert_eq!(
            AgentAction::Snapshot(SnapshotAction::List).category(),
            ActionCategory::Snapshot
        );
        assert_eq!(
            AgentAction::Resource(ResourceAction::GetUsage).category(),
            ActionCategory::Resource
        );
    }

    #[test]
    fn test_agent_action_name() {
        assert_eq!(AgentAction::Power(PowerAction::Start).name(), "power.start");
        assert_eq!(
            AgentAction::Snapshot(SnapshotAction::List).name(),
            "snapshot.list"
        );
        assert_eq!(
            AgentAction::Debug(DebugAction::DumpState).name(),
            "debug.dump_state"
        );
    }

    #[test]
    fn test_action_response_success() {
        let response = ActionResponse::success(
            123,
            serde_json::json!({"result": "ok"}),
            Duration::from_millis(100),
        );

        assert!(response.success);
        assert!(response.data.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_action_response_failure() {
        let response =
            ActionResponse::failure(123, "Something went wrong", Duration::from_millis(50));

        assert!(!response.success);
        assert!(response.data.is_none());
        assert_eq!(response.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_action_validator_default() {
        let validator = ActionValidator::new();
        let request = ActionRequest::new("vm-1", AgentAction::Power(PowerAction::Start));

        assert!(validator.validate(&request).is_ok());
    }

    #[test]
    fn test_action_validator_category_restriction() {
        let validator = ActionValidator::new().with_categories(vec![ActionCategory::Power]);

        let power_request = ActionRequest::new("vm-1", AgentAction::Power(PowerAction::Start));
        let snapshot_request =
            ActionRequest::new("vm-1", AgentAction::Snapshot(SnapshotAction::List));

        assert!(validator.validate(&power_request).is_ok());
        assert!(matches!(
            validator.validate(&snapshot_request),
            Err(ActionError::PermissionDenied(_))
        ));
    }

    #[test]
    fn test_action_validator_debug_restriction() {
        let validator = ActionValidator::new().with_debug(false);
        let request = ActionRequest::new("vm-1", AgentAction::Debug(DebugAction::DumpState));

        assert!(matches!(
            validator.validate(&request),
            Err(ActionError::PermissionDenied(_))
        ));
    }

    #[test]
    fn test_action_validator_memory_limit() {
        let validator = ActionValidator::new().with_max_memory(1024);
        let request = ActionRequest::new(
            "vm-1",
            AgentAction::Resource(ResourceAction::SetMemory { bytes: 2048 }),
        );

        assert!(matches!(
            validator.validate(&request),
            Err(ActionError::InvalidParameters(_))
        ));
    }

    #[test]
    fn test_action_validator_cpu_limit() {
        let validator = ActionValidator::new().with_max_cpus(4);
        let request = ActionRequest::new(
            "vm-1",
            AgentAction::Resource(ResourceAction::SetCpuCount { count: 8 }),
        );

        assert!(matches!(
            validator.validate(&request),
            Err(ActionError::InvalidParameters(_))
        ));
    }

    #[test]
    fn test_action_validator_restricted() {
        let validator = ActionValidator::restricted();

        // Power should be allowed
        let power = ActionRequest::new("vm-1", AgentAction::Power(PowerAction::Start));
        assert!(validator.validate(&power).is_ok());

        // Debug should be denied
        let debug = ActionRequest::new("vm-1", AgentAction::Debug(DebugAction::DumpState));
        assert!(validator.validate(&debug).is_err());
    }

    #[test]
    fn test_port_range() {
        let single = PortRange::single(80);
        assert_eq!(single.start, 80);
        assert_eq!(single.end, 80);

        let range = PortRange::range(8000, 9000);
        assert_eq!(range.start, 8000);
        assert_eq!(range.end, 9000);
    }

    #[test]
    fn test_firewall_rule() {
        let rule = FirewallRule {
            id: 1,
            direction: Direction::Inbound,
            protocol: Protocol::Tcp,
            source: None,
            destination: None,
            port: Some(PortRange::single(443)),
            action: FirewallAction::Allow,
            priority: 100,
        };

        assert_eq!(rule.id, 1);
        assert_eq!(rule.protocol, Protocol::Tcp);
    }

    #[test]
    fn test_action_queue() {
        let queue = ActionQueue::new(10);

        let request = ActionRequest::new("vm-1", AgentAction::Power(PowerAction::Start));
        let request_id = request.id;

        assert!(queue.enqueue(request).is_ok());
        assert_eq!(queue.pending_count(), 1);

        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().id, request_id);
        assert!(queue.is_in_progress(request_id));

        queue.complete(request_id);
        assert!(!queue.is_in_progress(request_id));
    }

    #[test]
    fn test_action_queue_full() {
        let queue = ActionQueue::new(2);

        queue
            .enqueue(ActionRequest::new(
                "vm-1",
                AgentAction::Power(PowerAction::Start),
            ))
            .unwrap();
        queue
            .enqueue(ActionRequest::new(
                "vm-2",
                AgentAction::Power(PowerAction::Start),
            ))
            .unwrap();

        let result = queue.enqueue(ActionRequest::new(
            "vm-3",
            AgentAction::Power(PowerAction::Start),
        ));

        assert!(matches!(result, Err(ActionError::ResourceUnavailable(_))));
    }

    #[test]
    fn test_action_error_variants() {
        let errors = vec![
            ActionError::NotSupported("test".to_string()),
            ActionError::InvalidParameters("test".to_string()),
            ActionError::PermissionDenied("test".to_string()),
            ActionError::ResourceUnavailable("test".to_string()),
            ActionError::Timeout("test".to_string()),
            ActionError::AlreadyInProgress("test".to_string()),
            ActionError::VmNotFound("test".to_string()),
            ActionError::Failed("test".to_string()),
            ActionError::Cancelled,
        ];

        for err in errors {
            assert!(!format!("{}", err).is_empty());
        }
    }

    #[tokio::test]
    async fn test_mock_executor() {
        let executor = MockExecutor::new();
        let request = ActionRequest::new("vm-1", AgentAction::Power(PowerAction::Start));

        let response = executor.execute(request).await.unwrap();
        assert!(response.success);
    }

    #[tokio::test]
    async fn test_mock_executor_unsupported() {
        let executor = MockExecutor::new().with_categories(vec![ActionCategory::Power]);
        let request = ActionRequest::new("vm-1", AgentAction::Debug(DebugAction::DumpState));

        let result = executor.execute(request).await;
        assert!(matches!(result, Err(ActionError::NotSupported(_))));
    }

    #[test]
    fn test_mock_executor_supports() {
        let executor = MockExecutor::new();

        assert!(executor.supports(&AgentAction::Power(PowerAction::Start)));
        assert!(executor.supports(&AgentAction::Resource(ResourceAction::GetUsage)));
        assert!(!executor.supports(&AgentAction::Debug(DebugAction::DumpState)));
    }

    #[test]
    fn test_snapshot_action_variants() {
        let actions = vec![
            SnapshotAction::Create {
                name: "test".to_string(),
                description: Some("desc".to_string()),
            },
            SnapshotAction::Restore {
                name: "test".to_string(),
            },
            SnapshotAction::Delete {
                name: "test".to_string(),
            },
            SnapshotAction::List,
            SnapshotAction::Export {
                name: "test".to_string(),
                path: "/tmp/snap".to_string(),
            },
            SnapshotAction::Import {
                path: "/tmp/snap".to_string(),
            },
        ];

        for action in actions {
            let agent_action = AgentAction::Snapshot(action);
            assert_eq!(agent_action.category(), ActionCategory::Snapshot);
        }
    }
}
