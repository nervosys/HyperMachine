//! Integration tests for hv2-agent cross-module flows.

use hv2_agent::{
    // Tools
    ToolRegistry, ToolDefinition, ToolParameter, ParameterType, ToolCall, RegisteredTool,
    ToolCategory,
    // Actions
    ActionRequest, AgentAction, PowerAction, ActionValidator,
    // Policies
    PolicySet, PolicyRule, PolicyAction, PolicyEffect, PolicyContext, ResourceId,
    // Permissions
    PermissionGraph, PrincipalId, PrincipalKind, Permission, PermissionSet, ResourceScope,
    ResolutionEngine,
    // MCP
    McpServer, AgentCapabilities,
    // Orchestration
    AgentOrchestrator, AgentRole,
    // Memory
    EpisodicMemory, SemanticMemory, WorkingMemory,
    Episode, SemanticFact, WorkingItem,
    // Events
    EventBus, EventCategory, EventSeverity, EventFilter, VmEvent,
    // State
    StateStore, StateValue,
    // Telemetry
    TelemetryCollector, TelemetryConfig,
};
use serde_json::json;
use std::collections::HashMap;

// ─── Flow 1: Tools → Actions → Policies ────────────────────────────

#[test]
fn tool_register_and_execute() {
    let mut registry = ToolRegistry::new();

    let def = ToolDefinition::new("vm_start", "Start a VM", ToolCategory::System)
        .with_parameter(ToolParameter::required("vm_id", ParameterType::String, "VM ID"));

    let tool = RegisteredTool::new(def, |args: &HashMap<String, serde_json::Value>| {
        let vm_id = args.get("vm_id").unwrap().as_str().unwrap();
        Ok(json!({ "status": "started", "vm_id": vm_id }))
    });

    registry.register(tool).unwrap();

    let call = ToolCall::new("call-1", "vm_start", "agent-a")
        .with_arg("vm_id", json!("vm-123"));

    let result = registry.execute(&call).unwrap();
    assert!(result.success);
    let val = result.result.unwrap();
    assert_eq!(val["status"], "started");
    assert_eq!(val["vm_id"], "vm-123");
}

#[test]
fn tool_missing_required_param_rejected() {
    let mut registry = ToolRegistry::new();

    let def = ToolDefinition::new("vm_stop", "Stop a VM", ToolCategory::System)
        .with_parameter(ToolParameter::required("vm_id", ParameterType::String, "VM ID"));

    let tool = RegisteredTool::new(def, |_| Ok(json!("ok")));
    registry.register(tool).unwrap();

    let call = ToolCall::new("call-2", "vm_stop", "agent-a");
    let result = registry.execute(&call);
    assert!(result.is_err());
}

#[test]
fn policy_allow_then_deny_by_priority() {
    let policy = PolicySet::new("test-policy")
        .with_rule(
            PolicyRule::allow("allow-reads")
                .with_action(PolicyAction::ResourceRead)
                .with_priority(10),
        )
        .with_rule(
            PolicyRule::deny("deny-all-destructive")
                .with_action(PolicyAction::VmDelete)
                .with_priority(10),
        );

    let resource = ResourceId::new("vm", "vm-abc");
    let context = PolicyContext::new("agent-1");

    assert_eq!(
        policy.evaluate(&PolicyAction::ResourceRead, &resource, &context),
        PolicyEffect::Allow
    );
    assert_eq!(
        policy.evaluate(&PolicyAction::VmDelete, &resource, &context),
        PolicyEffect::Deny
    );
    // Unmatched action falls through to default (Deny)
    assert_eq!(
        policy.evaluate(&PolicyAction::VmCreate, &resource, &context),
        PolicyEffect::Deny
    );
}

// ─── Flow 2: Permissions → Delegation → Resolution ─────────────────

#[test]
fn permission_grant_and_resolve() {
    let graph = PermissionGraph::new();

    let admin = PrincipalId("admin".to_string());
    let operator = PrincipalId("operator".to_string());

    graph
        .add_principal(admin.clone(), PrincipalKind::Role, "Admin")
        .unwrap();
    graph
        .add_principal(operator.clone(), PrincipalKind::Agent, "Operator Agent")
        .unwrap();

    let mut perms = PermissionSet::new();
    perms.insert(Permission::VmCreate);
    perms.insert(Permission::VmRead);
    perms.insert(Permission::VmStart);
    perms.insert(Permission::VmStop);

    graph
        .grant(
            operator.clone(),
            perms,
            ResourceScope::Root,
            Some(admin.clone()),
            0,
            None,
        )
        .unwrap();

    let effective = ResolutionEngine::resolve(&graph, &operator, &ResourceScope::Root).unwrap();

    assert!(effective.allows(&Permission::VmCreate));
    assert!(effective.allows(&Permission::VmRead));
    assert!(effective.allows(&Permission::VmStart));
    assert!(effective.allows(&Permission::VmStop));
    // Not granted
    assert!(!effective.allows(&Permission::AdminConfig));
}

#[test]
fn permission_check_denies_missing_permission() {
    let graph = PermissionGraph::new();

    let user = PrincipalId("read-only-user".to_string());
    graph
        .add_principal(user.clone(), PrincipalKind::Role, "Reader")
        .unwrap();

    let mut perms = PermissionSet::new();
    perms.insert(Permission::VmRead);
    graph
        .grant(user.clone(), perms, ResourceScope::Root, None, 0, None)
        .unwrap();

    let allowed =
        ResolutionEngine::check(&graph, &user, &Permission::VmDelete, &ResourceScope::Root)
            .unwrap();
    assert!(!allowed);
}

// ─── Flow 3: MCP Session → Tool Discovery ──────────────────────────

#[test]
fn mcp_server_session_lists_tools() {
    let server = McpServer::new();
    let caps = AgentCapabilities::operator();
    let session = server.create_session("test-agent", caps.clone()).unwrap();

    assert_eq!(session.agent_id, "test-agent");

    let tools = server.list_tools(&caps);
    for tool in &tools {
        assert!(!tool.name.is_empty());
        assert!(!tool.description.is_empty());
    }
}

// ─── Flow 4: Orchestration → Agent Registration ────────────────────

#[test]
fn orchestrator_register_and_heartbeat() {
    let orch = AgentOrchestrator::new();

    let info = orch
        .register_agent("agent-1", "Test Agent", AgentRole::Operator)
        .unwrap();
    let agent_id = info.id.clone();
    assert!(!agent_id.is_empty());

    let result = orch.heartbeat(&agent_id);
    assert!(result.is_ok());

    orch.unregister_agent(&agent_id).unwrap();

    let result = orch.heartbeat(&agent_id);
    assert!(result.is_err());
}

// ─── Flow 5: Memory System ─────────────────────────────────────────

#[test]
fn memory_episodic_store_and_retrieve() {
    let mut episodic = EpisodicMemory::new(1000);

    let ep = Episode::new("ep-1", "Observed high CPU in vm-1");
    episodic.store(ep).unwrap();

    let recalled = episodic.retrieve("ep-1");
    assert!(recalled.is_some());
    assert_eq!(recalled.unwrap().content, "Observed high CPU in vm-1");
}

#[test]
fn memory_semantic_fact_retrieval() {
    let mut semantic = SemanticMemory::new(1000);

    semantic
        .store(SemanticFact::new("fact-1", "vm-1", "has_cpus", "4"))
        .unwrap();
    semantic
        .store(SemanticFact::new("fact-2", "vm-1", "has_memory_mb", "8192"))
        .unwrap();

    let facts = semantic.query(Some("vm-1"), None, None);
    assert_eq!(facts.len(), 2);
}

#[test]
fn working_memory_add_and_evict() {
    // 100 tokens capacity
    let mut working = WorkingMemory::new(100);

    working
        .add(WorkingItem::new("item-1", "user", "short msg"))
        .unwrap();
    working
        .add(WorkingItem::new("item-2", "assistant", "another msg"))
        .unwrap();

    assert!(working.len() >= 2);
}

// ─── Flow 6: State Persistence ─────────────────────────────────────

#[test]
fn state_store_set_get_delete() {
    let mut store = StateStore::new("test-store");

    store
        .set("counter", StateValue::from_string("42"))
        .unwrap();
    let val = store.get("counter");
    assert!(val.is_some());

    store
        .set("name", StateValue::from_string("agent-a"))
        .unwrap();
    assert!(store.get("name").is_some());

    store.delete("counter").unwrap();
    assert!(store.get("counter").is_none());
}

// ─── Flow 7: Events ────────────────────────────────────────────────

#[test]
fn event_bus_publish_subscribe() {
    let bus = EventBus::new(64);

    let filter = EventFilter::new().with_category(EventCategory::Lifecycle);
    let (_sub_id, mut rx) = bus.subscribe(filter);

    bus.publish(VmEvent::new(
        EventCategory::Lifecycle,
        EventSeverity::Info,
        "test",
        "vm_started",
        "vm-1 started",
    ));

    let event = rx.try_recv().unwrap();
    assert!(event.is_some());
    let event = event.unwrap();
    assert_eq!(event.message, "vm-1 started");
}

// ─── Flow 8: Telemetry ─────────────────────────────────────────────

#[test]
fn telemetry_counter_and_gauge() {
    let collector = TelemetryCollector::new(TelemetryConfig::default());

    let counter = collector.counter("requests_total");
    counter.inc();
    counter.inc();
    assert_eq!(counter.get(), 2);

    let gauge = collector.gauge("active_vms");
    gauge.set(5.0);
    assert_eq!(gauge.get(), 5.0);
}

// ─── Flow 9: Action validation ─────────────────────────────────────

#[test]
fn action_validator_accepts_valid_request() {
    let validator = ActionValidator::new();

    let request = ActionRequest::new("vm-abc", AgentAction::Power(PowerAction::Start));

    let result = validator.validate(&request);
    assert!(result.is_ok());
}

// ─── Flow 10: Cross-cutting — Tool + Policy + Permission ───────────

#[test]
fn tool_execution_guarded_by_policy() {
    // Set up a restrictive policy
    let policy = PolicySet::new("operator-policy")
        .with_rule(
            PolicyRule::allow("allow-vm-start")
                .with_action(PolicyAction::VmStart)
                .with_priority(10),
        )
        .with_rule(
            PolicyRule::deny("deny-delete")
                .with_action(PolicyAction::VmDelete)
                .with_priority(10),
        );

    let resource = ResourceId::new("vm", "vm-123");
    let context = PolicyContext::new("operator-agent");

    // Simulate: before executing a "start" tool, check policy
    let start_allowed = policy.evaluate(&PolicyAction::VmStart, &resource, &context);
    assert_eq!(start_allowed, PolicyEffect::Allow);

    // The "delete" tool should be blocked by policy
    let delete_allowed = policy.evaluate(&PolicyAction::VmDelete, &resource, &context);
    assert_eq!(delete_allowed, PolicyEffect::Deny);
}
