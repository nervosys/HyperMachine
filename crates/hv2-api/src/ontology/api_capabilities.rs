//! The capabilities that group `hv2-api`'s own routes described in
//! `api_privileged_operations`, `api_runtime_operations`,
//! `api_event_operations` and `api_agentic_operations`.

use super::spec::api_capability;
use super::{Capability, CapabilityCategory, HyperMachineOntology};

impl HyperMachineOntology {
    pub(super) fn build_api_capabilities() -> Vec<Capability> {
        let key = "an API key when the server has keys configured (none by default)";
        vec![
            api_capability(
                "vm_guest_access",
                "VM Console And Guest Exec",
                "Read a VM's serial console and run a program in its guest. Both depend on a \
                 device or guest agent the VM must have: for a VM made through \
                 `/api/v1/vms` the console reports `attached: false` and exec answers 503.",
                CapabilityCategory::AgentExecution,
                &["get_vm_console", "exec_in_vm"],
                &["a VM with a serial device (console) or a guest-agent channel (exec)"],
                &[key],
            ),
            api_capability(
                "image_admission",
                "Boot Image Admission",
                "Register boot images and decide which may boot: approve, deny, revoke, \
                 deprecate, check admission and read the registry's configuration. The \
                 registry is in memory and the check at VM creation is off unless the server \
                 is started with image enforcement.",
                CapabilityCategory::Security,
                &[
                    "list_images",
                    "register_image",
                    "get_image_registry_config",
                    "check_image_admission",
                    "get_image",
                    "approve_image",
                    "deny_image",
                    "revoke_image",
                    "deprecate_image",
                ],
                &["the runtime enabled (the default)"],
                &[key],
            ),
            api_capability(
                "agent_runtime",
                "Agent Sessions",
                "Spawn, list, count and release agent sessions per tenant, and reap idle \
                 ones. Sessions are records of the agent runtime; the tenant is the \
                 `X-Tenant-Id` header the caller sends.",
                CapabilityCategory::AgentExecution,
                &[
                    "spawn_agent",
                    "list_agents",
                    "get_agent_fleet",
                    "reap_agents",
                    "release_agent",
                ],
                &[],
                &[key],
            ),
            api_capability(
                "fleet_runtime",
                "Runtime Sessions, Workloads And Workflows",
                "The runtime's bookkeeping: sessions, workload submission and scheduling, \
                 workflows and their steps, maintenance, and its status, health and \
                 metrics. Nothing here starts a process.",
                CapabilityCategory::Compute,
                &[
                    "get_runtime_status",
                    "get_runtime_health",
                    "get_runtime_metrics",
                    "get_runtime_metrics_prometheus",
                    "create_runtime_session",
                    "destroy_runtime_session",
                    "submit_runtime_workload",
                    "schedule_runtime_workloads",
                    "run_runtime_workflow",
                    "advance_runtime_workflow_step",
                    "cancel_runtime_workflow",
                    "run_runtime_maintenance",
                ],
                &["the runtime enabled (the default)"],
                &[key],
            ),
            api_capability(
                "gpu_fabric",
                "GPU Fabric Records",
                "Declare a GPU topology, query placements, capacity and the fleet, register \
                 VM classes and create or cancel reservations. The topology is what callers \
                 declare; reservations hold no GPU.",
                CapabilityCategory::Compute,
                &[
                    "get_gpu_topology",
                    "add_gpu_device",
                    "add_gpu_link",
                    "find_gpu_placement",
                    "get_gpu_fleet",
                    "get_gpu_capacity",
                    "register_gpu_vm_class",
                    "get_gpu_class_placement",
                    "create_gpu_reservation",
                    "cancel_gpu_reservation",
                ],
                &["the runtime enabled (the default)"],
                &[key],
            ),
            api_capability(
                "vm_snapshot_records",
                "VM Snapshot Records",
                "Create, list, read, delete and restore snapshot records of a VM id. They \
                 are catalogue entries: no VM state is captured and restore changes no VM.",
                CapabilityCategory::Storage,
                &[
                    "create_vm_snapshot",
                    "list_vm_snapshots",
                    "get_vm_snapshot",
                    "delete_vm_snapshot",
                    "restore_vm_snapshot",
                ],
                &[],
                &[key],
            ),
            api_capability(
                "event_delivery",
                "Events And Webhooks",
                "Subscribe to the server's event bus (server-sent events or WebSocket), \
                 publish an event, and register webhooks. Only the publish route puts events \
                 on the bus.",
                CapabilityCategory::Monitoring,
                &[
                    "create_webhook",
                    "list_webhooks",
                    "get_webhook",
                    "delete_webhook",
                    "stream_events",
                    "publish_event",
                    "stream_events_websocket",
                ],
                &["events enabled (the default)"],
                &[key],
            ),
            api_capability(
                "health_and_metrics",
                "Health Probes And Metrics",
                "Liveness, readiness and full health, and the Prometheus metrics. The probes \
                 under /health are exempt from the API key; /metrics and /api/v1/health/full \
                 are not.",
                CapabilityCategory::Monitoring,
                &[
                    "get_health",
                    "get_liveness",
                    "get_readiness",
                    "get_full_health",
                    "get_prometheus_metrics",
                ],
                &[],
                &[],
            ),
            api_capability(
                "agent_discovery_and_plans",
                "Ontology Discovery And Plans",
                "Read the ontology and its projections (tool lists, MCP manifest, plugin \
                 manifest, agent card, affordances, contracts, graph, composition rules and \
                 templates), validate plans and execute them. Only `execute_plan` can act, \
                 and only through nine VM operations.",
                CapabilityCategory::AgentExecution,
                &[
                    "get_ontology",
                    "get_openai_tools",
                    "get_anthropic_tools",
                    "get_gemini_tools",
                    "get_affordances",
                    "validate_plan",
                    "execute_plan",
                    "get_composition_rules",
                    "get_resource_graph",
                    "get_operation_contracts",
                    "get_mcp_manifest",
                    "get_ai_plugin_manifest",
                    "get_agent_card",
                    "list_plan_templates",
                    "instantiate_plan_template",
                    "get_plan_template",
                ],
                &[],
                &["an API key for POST routes and /.well-known when keys are configured"],
            ),
        ]
    }
}
