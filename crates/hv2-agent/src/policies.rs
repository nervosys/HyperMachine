//! Agent policies — rules and constraints for AI agent operations.
//!
//! A policy framework for expressing what agents may and may not do:
//! - Permission policies (allow/deny rules)
//! - Time-based policies (scheduling)
//! - Resource policies (quotas)
//! - Behavioral policies (safety constraints)
//!
//! # Where this is enforced
//!
//! [`PolicySet::evaluate`] answers a question; it does not intercept anything on
//! its own. It takes effect wherever a caller asks it and acts on the answer.
//!
//! The MCP tool surface is such a caller, but only on request:
//! [`McpServer::set_policy_set`](crate::mcp::McpServer::set_policy_set) installs
//! a set, after which every tool call is evaluated before dispatch and a denial
//! is refused and audited. With no set installed — the default — an agent is
//! gated by capabilities and VM ownership alone.
//!
//! Note that [`PolicySet::new`] denies by default, so an installed set must name
//! everything agents may do, including tools added after it was written.
//!
//! See [`limits`](crate::limits), which is still consult-only, and
//! [`permissions`](crate::permissions), which `hv2-api`'s permission middleware
//! wires into a request path.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

/// Result type for policy operations
pub type PolicyResult<T> = Result<T, PolicyError>;

/// Policy errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// Permission denied by policy
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    /// Policy not found
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),
    /// Policy conflict detected
    #[error("Policy conflict: {0}")]
    PolicyConflict(String),
    /// Policy evaluation failed
    #[error("Evaluation failed: {0}")]
    EvaluationFailed(String),
    /// Time-based restriction
    #[error("Time restriction: {0}")]
    TimeRestriction(String),
    /// Resource quota exceeded
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),
    /// Invalid policy configuration
    #[error("Invalid policy: {0}")]
    InvalidPolicy(String),
}

/// Policy effect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PolicyEffect {
    /// Allow the action
    Allow,
    /// Deny the action
    #[default]
    Deny,
}

/// Policy action types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyAction {
    /// VM lifecycle actions
    VmStart,
    VmStop,
    VmReboot,
    VmPause,
    VmResume,
    VmCreate,
    VmDelete,

    /// Resource actions
    ResourceModify,
    ResourceRead,
    ResourceAllocate,
    ResourceDeallocate,

    /// Snapshot actions
    SnapshotCreate,
    SnapshotRestore,
    SnapshotDelete,

    /// Network actions
    NetworkAttach,
    NetworkDetach,
    NetworkConfigure,

    /// Storage actions
    StorageAttach,
    StorageDetach,
    StorageResize,

    /// Run a program inside a guest.
    ///
    /// Its own action rather than a resource read or modify: running a command
    /// in a guest is neither, and folding it into either would let a policy
    /// that meant to allow reading a VM allow running anything inside it.
    GuestExec,

    /// Run a confined program on the host itself.
    ///
    /// Separate from [`Self::GuestExec`] because the blast radius is: a
    /// program in a guest cannot reach the host, and one on the host can,
    /// however well confined.
    HostExec,

    /// Debug actions
    DebugAttach,
    DebugInspect,
    DebugModify,

    /// Administrative actions
    AdminConfigure,
    AdminAudit,

    /// Custom action
    Custom(String),
}

impl PolicyAction {
    /// Check if this is a destructive action
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            Self::VmStop
                | Self::VmDelete
                | Self::SnapshotDelete
                | Self::StorageDetach
                | Self::DebugModify
        )
    }

    /// Check if this is a read-only action
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::ResourceRead | Self::DebugInspect | Self::AdminAudit
        )
    }
}

/// Resource identifier for policies
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId {
    /// Resource type (vm, network, storage, etc.)
    pub resource_type: String,
    /// Resource identifier (name, UUID, or pattern)
    pub identifier: String,
}

impl ResourceId {
    /// Create a new resource ID
    pub fn new(resource_type: impl Into<String>, identifier: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            identifier: identifier.into(),
        }
    }

    /// Create a wildcard resource (matches all)
    pub fn wildcard(resource_type: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            identifier: "*".to_string(),
        }
    }

    /// Check if this matches another resource ID
    pub fn matches(&self, other: &ResourceId) -> bool {
        if self.resource_type != other.resource_type && self.resource_type != "*" {
            return false;
        }

        if self.identifier == "*" {
            return true;
        }

        // Pattern matching with wildcards
        if self.identifier.contains('*') {
            let pattern = self.identifier.replace('*', "");
            if self.identifier.starts_with('*') && self.identifier.ends_with('*') {
                other.identifier.contains(&pattern)
            } else if self.identifier.starts_with('*') {
                other.identifier.ends_with(&pattern)
            } else if self.identifier.ends_with('*') {
                other.identifier.starts_with(&pattern)
            } else {
                self.identifier == other.identifier
            }
        } else {
            self.identifier == other.identifier
        }
    }
}

/// Time window for time-based policies
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    /// Start hour (0-23)
    pub start_hour: u8,
    /// Start minute (0-59)
    pub start_minute: u8,
    /// End hour (0-23)
    pub end_hour: u8,
    /// End minute (0-59)
    pub end_minute: u8,
    /// Days of week (0 = Sunday, 6 = Saturday)
    pub days_of_week: HashSet<u8>,
}

impl TimeWindow {
    /// Create a new time window
    pub fn new(start_hour: u8, start_minute: u8, end_hour: u8, end_minute: u8) -> Self {
        Self {
            start_hour: start_hour.min(23),
            start_minute: start_minute.min(59),
            end_hour: end_hour.min(23),
            end_minute: end_minute.min(59),
            days_of_week: (0..7).collect(),
        }
    }

    /// Create a business hours window (9 AM - 5 PM, Mon-Fri)
    pub fn business_hours() -> Self {
        Self {
            start_hour: 9,
            start_minute: 0,
            end_hour: 17,
            end_minute: 0,
            days_of_week: (1..6).collect(), // Monday through Friday
        }
    }

    /// Create an off-hours window (outside business hours)
    pub fn off_hours() -> Self {
        Self {
            start_hour: 17,
            start_minute: 0,
            end_hour: 9,
            end_minute: 0,
            days_of_week: (0..7).collect(),
        }
    }

    /// Check if a given time falls within this window
    pub fn contains_time(&self, hour: u8, minute: u8, day_of_week: u8) -> bool {
        if !self.days_of_week.contains(&day_of_week) {
            return false;
        }

        let time_mins = hour as u32 * 60 + minute as u32;
        let start_mins = self.start_hour as u32 * 60 + self.start_minute as u32;
        let end_mins = self.end_hour as u32 * 60 + self.end_minute as u32;

        if start_mins <= end_mins {
            // Normal window (e.g., 9 AM to 5 PM)
            time_mins >= start_mins && time_mins <= end_mins
        } else {
            // Overnight window (e.g., 10 PM to 6 AM)
            time_mins >= start_mins || time_mins <= end_mins
        }
    }
}

/// Condition for policy evaluation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PolicyCondition {
    /// Always true
    Always,
    /// Never true
    Never,
    /// Time-based condition
    TimeWindow(TimeWindow),
    /// Resource attribute condition
    ResourceAttribute { key: String, value: String },
    /// Agent attribute condition
    AgentAttribute { key: String, value: String },
    /// Combined conditions (AND)
    And(Vec<PolicyCondition>),
    /// Combined conditions (OR)
    Or(Vec<PolicyCondition>),
    /// Negated condition
    Not(Box<PolicyCondition>),
}

impl PolicyCondition {
    /// Evaluate this condition
    pub fn evaluate(&self, context: &PolicyContext) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::TimeWindow(window) => {
                window.contains_time(context.hour, context.minute, context.day_of_week)
            }
            Self::ResourceAttribute { key, value } => context
                .resource_attributes
                .get(key)
                .map(|v| v == value)
                .unwrap_or(false),
            Self::AgentAttribute { key, value } => context
                .agent_attributes
                .get(key)
                .map(|v| v == value)
                .unwrap_or(false),
            Self::And(conditions) => conditions.iter().all(|c| c.evaluate(context)),
            Self::Or(conditions) => conditions.iter().any(|c| c.evaluate(context)),
            Self::Not(condition) => !condition.evaluate(context),
        }
    }
}

/// Context for policy evaluation
#[derive(Debug, Clone)]
pub struct PolicyContext {
    /// Agent ID
    pub agent_id: String,
    /// Current hour (0-23)
    pub hour: u8,
    /// Current minute (0-59)
    pub minute: u8,
    /// Day of week (0-6)
    pub day_of_week: u8,
    /// Resource attributes
    pub resource_attributes: HashMap<String, String>,
    /// Agent attributes
    pub agent_attributes: HashMap<String, String>,
}

impl PolicyContext {
    /// Create a new policy context
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            hour: 12,
            minute: 0,
            day_of_week: 1, // Monday
            resource_attributes: HashMap::new(),
            agent_attributes: HashMap::new(),
        }
    }

    /// Set the current time
    pub fn with_time(mut self, hour: u8, minute: u8, day_of_week: u8) -> Self {
        self.hour = hour.min(23);
        self.minute = minute.min(59);
        self.day_of_week = day_of_week.min(6);
        self
    }

    /// Add a resource attribute
    pub fn with_resource_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.resource_attributes.insert(key.into(), value.into());
        self
    }

    /// Add an agent attribute
    pub fn with_agent_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.agent_attributes.insert(key.into(), value.into());
        self
    }
}

/// A policy rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Rule name
    pub name: String,
    /// Rule description
    pub description: String,
    /// Effect (allow or deny)
    pub effect: PolicyEffect,
    /// Actions this rule applies to
    pub actions: HashSet<PolicyAction>,
    /// Resources this rule applies to
    pub resources: Vec<ResourceId>,
    /// Conditions that must be met
    pub condition: PolicyCondition,
    /// Priority (higher = evaluated first)
    pub priority: i32,
}

impl PolicyRule {
    /// Create a new allow rule
    pub fn allow(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            effect: PolicyEffect::Allow,
            actions: HashSet::new(),
            resources: Vec::new(),
            condition: PolicyCondition::Always,
            priority: 0,
        }
    }

    /// Create a new deny rule
    pub fn deny(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            effect: PolicyEffect::Deny,
            actions: HashSet::new(),
            resources: Vec::new(),
            condition: PolicyCondition::Always,
            priority: 0,
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add an action
    pub fn with_action(mut self, action: PolicyAction) -> Self {
        self.actions.insert(action);
        self
    }

    /// Add multiple actions
    pub fn with_actions(mut self, actions: impl IntoIterator<Item = PolicyAction>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// Add a resource
    pub fn with_resource(mut self, resource: ResourceId) -> Self {
        self.resources.push(resource);
        self
    }

    /// Set the condition
    pub fn with_condition(mut self, condition: PolicyCondition) -> Self {
        self.condition = condition;
        self
    }

    /// Set the priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Check if this rule matches the given action and resource
    pub fn matches(&self, action: &PolicyAction, resource: &ResourceId) -> bool {
        if !self.actions.is_empty() && !self.actions.contains(action) {
            return false;
        }

        if self.resources.is_empty() {
            return true;
        }

        self.resources.iter().any(|r| r.matches(resource))
    }
}

/// A collection of policies
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicySet {
    /// Policy name
    pub name: String,
    /// Policy description
    pub description: String,
    /// Rules in this policy set
    pub rules: Vec<PolicyRule>,
    /// Default effect when no rule matches
    pub default_effect: PolicyEffect,
}

impl PolicySet {
    /// Create a new policy set with deny-by-default
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            rules: Vec::new(),
            default_effect: PolicyEffect::Deny,
        }
    }

    /// Create a new permissive policy set (allow-by-default)
    pub fn permissive(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            rules: Vec::new(),
            default_effect: PolicyEffect::Allow,
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a rule
    pub fn with_rule(mut self, rule: PolicyRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Evaluate this policy set
    pub fn evaluate(
        &self,
        action: &PolicyAction,
        resource: &ResourceId,
        context: &PolicyContext,
    ) -> PolicyEffect {
        // Sort rules by priority (descending)
        let mut matching_rules: Vec<_> = self
            .rules
            .iter()
            .filter(|r| r.matches(action, resource) && r.condition.evaluate(context))
            .collect();

        matching_rules.sort_by_key(|r| std::cmp::Reverse(r.priority));

        // Return the effect of the first matching rule
        matching_rules
            .first()
            .map(|r| r.effect)
            .unwrap_or(self.default_effect)
    }
}

/// Rate limit specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitSpec {
    /// Maximum number of operations
    pub max_operations: u64,
    /// Time window duration
    pub window: Duration,
}

impl RateLimitSpec {
    /// Create a new rate limit
    pub fn new(max_operations: u64, window: Duration) -> Self {
        Self {
            max_operations,
            window,
        }
    }

    /// Create a per-minute rate limit
    pub fn per_minute(max: u64) -> Self {
        Self::new(max, Duration::from_secs(60))
    }

    /// Create a per-hour rate limit
    pub fn per_hour(max: u64) -> Self {
        Self::new(max, Duration::from_secs(3600))
    }
}

/// Resource quota specification
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaSpec {
    /// Maximum number of VMs
    pub max_vms: Option<u32>,
    /// Maximum total memory (bytes)
    pub max_memory: Option<u64>,
    /// Maximum total CPU cores
    pub max_cpus: Option<u32>,
    /// Maximum storage (bytes)
    pub max_storage: Option<u64>,
    /// Maximum network interfaces
    pub max_network_interfaces: Option<u32>,
    /// Maximum snapshots per VM
    pub max_snapshots_per_vm: Option<u32>,
}

impl QuotaSpec {
    /// Create an unlimited quota
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Create a basic quota
    pub fn basic() -> Self {
        Self {
            max_vms: Some(5),
            max_memory: Some(16 * 1024 * 1024 * 1024), // 16 GB
            max_cpus: Some(8),
            max_storage: Some(100 * 1024 * 1024 * 1024), // 100 GB
            max_network_interfaces: Some(10),
            max_snapshots_per_vm: Some(5),
        }
    }

    /// Check if VM quota allows another VM
    pub fn allows_vm(&self, current_count: u32) -> bool {
        self.max_vms.map(|max| current_count < max).unwrap_or(true)
    }

    /// Check if memory quota allows allocation
    pub fn allows_memory(&self, current: u64, additional: u64) -> bool {
        self.max_memory
            .map(|max| current.saturating_add(additional) <= max)
            .unwrap_or(true)
    }

    /// Check if CPU quota allows allocation
    pub fn allows_cpus(&self, current: u32, additional: u32) -> bool {
        self.max_cpus
            .map(|max| current.saturating_add(additional) <= max)
            .unwrap_or(true)
    }
}

/// Complete agent policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPolicy {
    /// Policy ID
    pub id: String,
    /// Policy name
    pub name: String,
    /// Policy version
    pub version: u32,
    /// Permission policies
    pub permissions: PolicySet,
    /// Resource quotas.
    ///
    /// **Recorded, not enforced.** [`AgentPolicy::allows`] consults `enabled`
    /// and `permissions` and nothing else, so a quota set here does not stop a
    /// sixth VM being created under a `max_vms` of five. Nothing in this crate
    /// constructs [`PolicyError::QuotaExceeded`].
    ///
    /// Said here because the sibling field is wired up and the module
    /// documentation explains that it is -- which makes silence about this one
    /// read as endorsement. Enforcing it needs usage counters that do not
    /// exist: [`PolicyContext`] carries an agent id and a clock, not a tally,
    /// and where such a tally should live (per session, per agent, across
    /// restarts) is a design decision rather than an oversight.
    pub quotas: QuotaSpec,
    /// Rate limits by action.
    ///
    /// **Recorded, not enforced**, for the same reason as
    /// [`quotas`](Self::quotas) and with the same requirement: rate limiting
    /// needs a count of recent actions, and nothing here keeps one.
    ///
    /// Note that [`McpConfig::rate_limit`](crate::mcp::McpConfig) *is* enforced
    /// -- it bounds calls per session on the tool surface. This field is a
    /// different thing that looks like it.
    pub rate_limits: HashMap<PolicyAction, RateLimitSpec>,
    /// Enabled state
    pub enabled: bool,
    /// Creation time
    pub created_at: SystemTime,
    /// Last update time
    pub updated_at: SystemTime,
}

impl AgentPolicy {
    /// Create a new agent policy
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = SystemTime::now();
        Self {
            id: id.into(),
            name: name.into(),
            version: 1,
            permissions: PolicySet::new("permissions"),
            quotas: QuotaSpec::default(),
            rate_limits: HashMap::new(),
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a read-only policy
    pub fn read_only(id: impl Into<String>) -> Self {
        let mut policy = Self::new(id, "Read-Only Policy");
        policy.permissions = PolicySet::new("read-only").with_rule(
            PolicyRule::allow("allow-reads")
                .with_description("Allow all read operations")
                .with_action(PolicyAction::ResourceRead)
                .with_action(PolicyAction::DebugInspect)
                .with_action(PolicyAction::AdminAudit),
        );
        policy
    }

    /// Create an operator policy (read + basic operations)
    pub fn operator(id: impl Into<String>) -> Self {
        let mut policy = Self::new(id, "Operator Policy");
        policy.permissions = PolicySet::new("operator")
            .with_rule(
                PolicyRule::allow("allow-operations")
                    .with_description("Allow standard operations")
                    .with_actions([
                        PolicyAction::ResourceRead,
                        PolicyAction::VmStart,
                        PolicyAction::VmStop,
                        PolicyAction::VmReboot,
                        PolicyAction::VmPause,
                        PolicyAction::VmResume,
                        PolicyAction::SnapshotCreate,
                        PolicyAction::SnapshotRestore,
                    ]),
            )
            .with_rule(
                PolicyRule::deny("deny-destructive")
                    .with_description("Deny destructive operations")
                    .with_priority(10)
                    .with_actions([
                        PolicyAction::VmDelete,
                        PolicyAction::SnapshotDelete,
                        PolicyAction::AdminConfigure,
                    ]),
            );
        policy.quotas = QuotaSpec::basic();
        policy
    }

    /// Create an admin policy (full access)
    pub fn admin(id: impl Into<String>) -> Self {
        let mut policy = Self::new(id, "Admin Policy");
        policy.permissions = PolicySet::permissive("admin");
        policy.quotas = QuotaSpec::unlimited();
        policy
    }

    /// Set permissions
    pub fn with_permissions(mut self, permissions: PolicySet) -> Self {
        self.permissions = permissions;
        self.updated_at = SystemTime::now();
        self
    }

    /// Set quotas
    pub fn with_quotas(mut self, quotas: QuotaSpec) -> Self {
        self.quotas = quotas;
        self.updated_at = SystemTime::now();
        self
    }

    /// Add a rate limit
    pub fn with_rate_limit(mut self, action: PolicyAction, limit: RateLimitSpec) -> Self {
        self.rate_limits.insert(action, limit);
        self.updated_at = SystemTime::now();
        self
    }

    /// Check if an action is allowed
    pub fn allows(
        &self,
        action: &PolicyAction,
        resource: &ResourceId,
        context: &PolicyContext,
    ) -> PolicyResult<()> {
        if !self.enabled {
            return Err(PolicyError::PermissionDenied("Policy is disabled".into()));
        }

        match self.permissions.evaluate(action, resource, context) {
            PolicyEffect::Allow => Ok(()),
            PolicyEffect::Deny => Err(PolicyError::PermissionDenied(format!(
                "Action {:?} denied on resource {:?}",
                action, resource
            ))),
        }
    }
}

/// Policy engine for evaluating multiple policies
#[derive(Debug, Default)]
pub struct PolicyEngine {
    /// Registered policies by ID
    policies: HashMap<String, AgentPolicy>,
    /// Agent to policy mappings
    agent_policies: HashMap<String, Vec<String>>,
}

impl PolicyEngine {
    /// Create a new policy engine
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a policy
    pub fn register_policy(&mut self, policy: AgentPolicy) {
        self.policies.insert(policy.id.clone(), policy);
    }

    /// Remove a policy
    pub fn remove_policy(&mut self, policy_id: &str) -> Option<AgentPolicy> {
        self.policies.remove(policy_id)
    }

    /// Get a policy by ID
    pub fn get_policy(&self, policy_id: &str) -> Option<&AgentPolicy> {
        self.policies.get(policy_id)
    }

    /// Assign a policy to an agent
    pub fn assign_policy(&mut self, agent_id: &str, policy_id: &str) -> PolicyResult<()> {
        if !self.policies.contains_key(policy_id) {
            return Err(PolicyError::PolicyNotFound(policy_id.to_string()));
        }

        self.agent_policies
            .entry(agent_id.to_string())
            .or_default()
            .push(policy_id.to_string());

        Ok(())
    }

    /// Remove a policy from an agent
    pub fn unassign_policy(&mut self, agent_id: &str, policy_id: &str) {
        if let Some(policies) = self.agent_policies.get_mut(agent_id) {
            policies.retain(|p| p != policy_id);
        }
    }

    /// Get policies for an agent
    pub fn get_agent_policies(&self, agent_id: &str) -> Vec<&AgentPolicy> {
        self.agent_policies
            .get(agent_id)
            .map(|policy_ids| {
                policy_ids
                    .iter()
                    .filter_map(|id| self.policies.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Evaluate if an action is allowed for an agent
    pub fn evaluate(
        &self,
        agent_id: &str,
        action: &PolicyAction,
        resource: &ResourceId,
        context: &PolicyContext,
    ) -> PolicyResult<()> {
        let policies = self.get_agent_policies(agent_id);

        if policies.is_empty() {
            // No policies assigned - default deny
            return Err(PolicyError::PermissionDenied(
                "No policies assigned to agent".into(),
            ));
        }

        // All assigned policies must allow the action
        for policy in policies {
            policy.allows(action, resource, context)?;
        }

        Ok(())
    }

    /// Get all registered policies
    pub fn list_policies(&self) -> Vec<&AgentPolicy> {
        self.policies.values().collect()
    }

    /// Get policy count
    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_error_display() {
        let err = PolicyError::PermissionDenied("action denied".into());
        assert!(err.to_string().contains("Permission denied"));

        let err = PolicyError::QuotaExceeded("memory limit".into());
        assert!(err.to_string().contains("Quota exceeded"));
    }

    #[test]
    fn test_policy_action_classification() {
        assert!(PolicyAction::VmDelete.is_destructive());
        assert!(PolicyAction::SnapshotDelete.is_destructive());
        assert!(!PolicyAction::VmStart.is_destructive());

        assert!(PolicyAction::ResourceRead.is_read_only());
        assert!(PolicyAction::AdminAudit.is_read_only());
        assert!(!PolicyAction::VmCreate.is_read_only());
    }

    #[test]
    fn test_resource_id_matching() {
        let specific = ResourceId::new("vm", "test-vm-1");
        let wildcard = ResourceId::wildcard("vm");
        let pattern = ResourceId::new("vm", "test-*");

        let target = ResourceId::new("vm", "test-vm-1");

        assert!(specific.matches(&target));
        assert!(wildcard.matches(&target));
        assert!(pattern.matches(&target));

        let other = ResourceId::new("vm", "prod-vm-1");
        assert!(!specific.matches(&other));
        assert!(wildcard.matches(&other));
        assert!(!pattern.matches(&other));
    }

    #[test]
    fn test_resource_id_type_mismatch() {
        let vm_resource = ResourceId::new("vm", "test-1");
        let network_resource = ResourceId::new("network", "test-1");

        assert!(!vm_resource.matches(&network_resource));
    }

    #[test]
    fn test_time_window_normal() {
        let window = TimeWindow::new(9, 0, 17, 0);

        // Within window
        assert!(window.contains_time(12, 0, 1)); // Monday noon
        assert!(window.contains_time(9, 0, 2)); // Tuesday 9 AM
        assert!(window.contains_time(17, 0, 3)); // Wednesday 5 PM

        // Outside window
        assert!(!window.contains_time(8, 59, 1)); // Before 9 AM
        assert!(!window.contains_time(17, 1, 1)); // After 5 PM
    }

    #[test]
    fn test_time_window_overnight() {
        let window = TimeWindow::new(22, 0, 6, 0);

        // Within window (late night)
        assert!(window.contains_time(23, 0, 1));
        assert!(window.contains_time(0, 0, 1));
        assert!(window.contains_time(5, 0, 1));

        // Outside window (daytime)
        assert!(!window.contains_time(12, 0, 1));
        assert!(!window.contains_time(21, 0, 1));
    }

    #[test]
    fn test_time_window_business_hours() {
        let window = TimeWindow::business_hours();

        // Monday at noon - allowed
        assert!(window.contains_time(12, 0, 1));

        // Sunday at noon - not allowed (weekend)
        assert!(!window.contains_time(12, 0, 0));

        // Friday at 5 PM - allowed
        assert!(window.contains_time(17, 0, 5));

        // Saturday at noon - not allowed
        assert!(!window.contains_time(12, 0, 6));
    }

    #[test]
    fn test_policy_condition_always_never() {
        let context = PolicyContext::new("agent-1");

        assert!(PolicyCondition::Always.evaluate(&context));
        assert!(!PolicyCondition::Never.evaluate(&context));
    }

    #[test]
    fn test_policy_condition_time_window() {
        let context = PolicyContext::new("agent-1").with_time(12, 0, 1);

        let condition = PolicyCondition::TimeWindow(TimeWindow::business_hours());
        assert!(condition.evaluate(&context));

        let off_hours_context = context.with_time(20, 0, 1);
        assert!(!condition.evaluate(&off_hours_context));
    }

    #[test]
    fn test_policy_condition_attributes() {
        let context = PolicyContext::new("agent-1")
            .with_resource_attribute("env", "production")
            .with_agent_attribute("role", "operator");

        let resource_cond = PolicyCondition::ResourceAttribute {
            key: "env".into(),
            value: "production".into(),
        };
        assert!(resource_cond.evaluate(&context));

        let agent_cond = PolicyCondition::AgentAttribute {
            key: "role".into(),
            value: "operator".into(),
        };
        assert!(agent_cond.evaluate(&context));

        let wrong_cond = PolicyCondition::AgentAttribute {
            key: "role".into(),
            value: "admin".into(),
        };
        assert!(!wrong_cond.evaluate(&context));
    }

    #[test]
    fn test_policy_condition_combinators() {
        let context = PolicyContext::new("agent-1").with_time(12, 0, 1);

        // AND condition
        let and_cond = PolicyCondition::And(vec![PolicyCondition::Always, PolicyCondition::Always]);
        assert!(and_cond.evaluate(&context));

        let and_false = PolicyCondition::And(vec![PolicyCondition::Always, PolicyCondition::Never]);
        assert!(!and_false.evaluate(&context));

        // OR condition
        let or_cond = PolicyCondition::Or(vec![PolicyCondition::Never, PolicyCondition::Always]);
        assert!(or_cond.evaluate(&context));

        // NOT condition
        let not_cond = PolicyCondition::Not(Box::new(PolicyCondition::Never));
        assert!(not_cond.evaluate(&context));
    }

    #[test]
    fn test_policy_rule_creation() {
        let rule = PolicyRule::allow("test-rule")
            .with_description("Test rule")
            .with_action(PolicyAction::VmStart)
            .with_action(PolicyAction::VmStop)
            .with_resource(ResourceId::wildcard("vm"))
            .with_priority(10);

        assert_eq!(rule.name, "test-rule");
        assert_eq!(rule.effect, PolicyEffect::Allow);
        assert_eq!(rule.actions.len(), 2);
        assert_eq!(rule.priority, 10);
    }

    #[test]
    fn test_policy_rule_matching() {
        let rule = PolicyRule::allow("vm-ops")
            .with_action(PolicyAction::VmStart)
            .with_resource(ResourceId::new("vm", "test-*"));

        let test_vm = ResourceId::new("vm", "test-vm-1");
        let prod_vm = ResourceId::new("vm", "prod-vm-1");

        assert!(rule.matches(&PolicyAction::VmStart, &test_vm));
        assert!(!rule.matches(&PolicyAction::VmStop, &test_vm)); // Wrong action
        assert!(!rule.matches(&PolicyAction::VmStart, &prod_vm)); // Wrong resource
    }

    #[test]
    fn test_policy_set_evaluation() {
        let policy_set = PolicySet::new("test")
            .with_rule(PolicyRule::allow("allow-reads").with_action(PolicyAction::ResourceRead))
            .with_rule(
                PolicyRule::deny("deny-deletes")
                    .with_action(PolicyAction::VmDelete)
                    .with_priority(10),
            );

        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        assert_eq!(
            policy_set.evaluate(&PolicyAction::ResourceRead, &resource, &context),
            PolicyEffect::Allow
        );
        assert_eq!(
            policy_set.evaluate(&PolicyAction::VmDelete, &resource, &context),
            PolicyEffect::Deny
        );
        // Default deny for unmatched actions
        assert_eq!(
            policy_set.evaluate(&PolicyAction::VmCreate, &resource, &context),
            PolicyEffect::Deny
        );
    }

    #[test]
    fn test_policy_set_default_allow() {
        let policy_set = PolicySet::permissive("permissive")
            .with_rule(PolicyRule::deny("deny-deletes").with_action(PolicyAction::VmDelete));

        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // Explicitly denied
        assert_eq!(
            policy_set.evaluate(&PolicyAction::VmDelete, &resource, &context),
            PolicyEffect::Deny
        );
        // Default allow for unmatched
        assert_eq!(
            policy_set.evaluate(&PolicyAction::VmCreate, &resource, &context),
            PolicyEffect::Allow
        );
    }

    #[test]
    fn test_quota_spec_basic() {
        let quota = QuotaSpec::basic();

        assert!(quota.allows_vm(4));
        assert!(!quota.allows_vm(5));

        assert!(quota.allows_memory(0, 16 * 1024 * 1024 * 1024));
        assert!(!quota.allows_memory(1, 16 * 1024 * 1024 * 1024));

        assert!(quota.allows_cpus(0, 8));
        assert!(!quota.allows_cpus(1, 8));
    }

    #[test]
    fn test_quota_spec_unlimited() {
        let quota = QuotaSpec::unlimited();

        assert!(quota.allows_vm(1000));
        assert!(quota.allows_memory(0, u64::MAX / 2));
        assert!(quota.allows_cpus(0, u32::MAX / 2));
    }

    #[test]
    fn test_rate_limit_spec() {
        let per_minute = RateLimitSpec::per_minute(100);
        assert_eq!(per_minute.max_operations, 100);
        assert_eq!(per_minute.window, Duration::from_secs(60));

        let per_hour = RateLimitSpec::per_hour(1000);
        assert_eq!(per_hour.max_operations, 1000);
        assert_eq!(per_hour.window, Duration::from_secs(3600));
    }

    #[test]
    fn test_agent_policy_read_only() {
        let policy = AgentPolicy::read_only("ro-policy");
        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // Read actions allowed
        assert!(policy
            .allows(&PolicyAction::ResourceRead, &resource, &context)
            .is_ok());
        assert!(policy
            .allows(&PolicyAction::DebugInspect, &resource, &context)
            .is_ok());

        // Write actions denied
        assert!(policy
            .allows(&PolicyAction::VmStart, &resource, &context)
            .is_err());
        assert!(policy
            .allows(&PolicyAction::VmDelete, &resource, &context)
            .is_err());
    }

    #[test]
    fn test_agent_policy_operator() {
        let policy = AgentPolicy::operator("op-policy");
        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // Basic operations allowed
        assert!(policy
            .allows(&PolicyAction::VmStart, &resource, &context)
            .is_ok());
        assert!(policy
            .allows(&PolicyAction::VmStop, &resource, &context)
            .is_ok());
        assert!(policy
            .allows(&PolicyAction::SnapshotCreate, &resource, &context)
            .is_ok());

        // Destructive operations denied
        assert!(policy
            .allows(&PolicyAction::VmDelete, &resource, &context)
            .is_err());
        assert!(policy
            .allows(&PolicyAction::AdminConfigure, &resource, &context)
            .is_err());
    }

    #[test]
    fn test_agent_policy_admin() {
        let policy = AgentPolicy::admin("admin-policy");
        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // Everything allowed
        assert!(policy
            .allows(&PolicyAction::VmDelete, &resource, &context)
            .is_ok());
        assert!(policy
            .allows(&PolicyAction::AdminConfigure, &resource, &context)
            .is_ok());
    }

    #[test]
    fn test_agent_policy_disabled() {
        let mut policy = AgentPolicy::admin("admin-policy");
        policy.enabled = false;

        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        let result = policy.allows(&PolicyAction::ResourceRead, &resource, &context);
        assert!(matches!(result, Err(PolicyError::PermissionDenied(_))));
    }

    #[test]
    fn test_policy_engine_registration() {
        let mut engine = PolicyEngine::new();

        engine.register_policy(AgentPolicy::read_only("policy-1"));
        engine.register_policy(AgentPolicy::operator("policy-2"));

        assert_eq!(engine.policy_count(), 2);
        assert!(engine.get_policy("policy-1").is_some());
        assert!(engine.get_policy("policy-3").is_none());
    }

    #[test]
    fn test_policy_engine_assignment() {
        let mut engine = PolicyEngine::new();
        engine.register_policy(AgentPolicy::operator("op-policy"));

        // Assign to agent
        assert!(engine.assign_policy("agent-1", "op-policy").is_ok());

        // Try to assign non-existent policy
        let result = engine.assign_policy("agent-1", "non-existent");
        assert!(matches!(result, Err(PolicyError::PolicyNotFound(_))));

        // Check assignments
        let policies = engine.get_agent_policies("agent-1");
        assert_eq!(policies.len(), 1);
    }

    #[test]
    fn test_policy_engine_evaluation() {
        let mut engine = PolicyEngine::new();
        engine.register_policy(AgentPolicy::operator("op-policy"));
        engine.assign_policy("agent-1", "op-policy").unwrap();

        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // Allowed operation
        assert!(engine
            .evaluate("agent-1", &PolicyAction::VmStart, &resource, &context)
            .is_ok());

        // Denied operation
        assert!(engine
            .evaluate("agent-1", &PolicyAction::VmDelete, &resource, &context)
            .is_err());
    }

    #[test]
    fn test_policy_engine_no_policies() {
        let engine = PolicyEngine::new();
        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // No policies assigned - should deny
        let result = engine.evaluate("agent-1", &PolicyAction::ResourceRead, &resource, &context);
        assert!(matches!(result, Err(PolicyError::PermissionDenied(_))));
    }

    #[test]
    fn test_policy_engine_multiple_policies() {
        let mut engine = PolicyEngine::new();

        // Policy 1: Allow reads (permissive to allow unmatched actions)
        let read_policy = AgentPolicy::new("read-policy", "Read Policy").with_permissions(
            PolicySet::permissive("read-perms")
                .with_rule(PolicyRule::allow("reads").with_action(PolicyAction::ResourceRead)),
        );

        // Policy 2: Deny all writes (high priority)
        let deny_policy = AgentPolicy::new("deny-writes", "Deny Writes").with_permissions(
            PolicySet::permissive("deny-perms").with_rule(
                PolicyRule::deny("no-writes")
                    .with_action(PolicyAction::VmCreate)
                    .with_priority(100),
            ),
        );

        engine.register_policy(read_policy);
        engine.register_policy(deny_policy);
        engine.assign_policy("agent-1", "read-policy").unwrap();
        engine.assign_policy("agent-1", "deny-writes").unwrap();

        let context = PolicyContext::new("agent-1");
        let resource = ResourceId::wildcard("vm");

        // Read should be allowed by permissive policy
        assert!(engine
            .evaluate("agent-1", &PolicyAction::ResourceRead, &resource, &context)
            .is_ok());

        // Create should be denied by explicit deny rule in second policy
        assert!(engine
            .evaluate("agent-1", &PolicyAction::VmCreate, &resource, &context)
            .is_err());
    }

    #[test]
    fn test_policy_engine_unassign() {
        let mut engine = PolicyEngine::new();
        engine.register_policy(AgentPolicy::admin("admin-policy"));
        engine.assign_policy("agent-1", "admin-policy").unwrap();

        assert_eq!(engine.get_agent_policies("agent-1").len(), 1);

        engine.unassign_policy("agent-1", "admin-policy");
        assert_eq!(engine.get_agent_policies("agent-1").len(), 0);
    }

    #[test]
    fn test_policy_engine_remove_policy() {
        let mut engine = PolicyEngine::new();
        engine.register_policy(AgentPolicy::admin("admin-policy"));

        assert_eq!(engine.policy_count(), 1);

        let removed = engine.remove_policy("admin-policy");
        assert!(removed.is_some());
        assert_eq!(engine.policy_count(), 0);
    }

    #[test]
    fn test_policy_engine_list_policies() {
        let mut engine = PolicyEngine::new();
        engine.register_policy(AgentPolicy::read_only("policy-1"));
        engine.register_policy(AgentPolicy::operator("policy-2"));
        engine.register_policy(AgentPolicy::admin("policy-3"));

        let policies = engine.list_policies();
        assert_eq!(policies.len(), 3);
    }
}
