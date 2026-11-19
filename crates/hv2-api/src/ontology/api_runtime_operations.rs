//! `hv2-api`'s fleet runtime (`runtime_routes.rs`, backed by `hv2-runtime`) and
//! GPU fabric (`gpu_fabric_routes.rs`) routes. Modelled as data from the
//! handlers. Both are mounted only while the runtime is enabled (the default),
//! and both are bookkeeping: `hv2-runtime` starts no VM and no process.

use super::spec::{
    api_key_failures, body_rejection, json_content_type_required, ok, path_param, plain, Spec,
};
use super::{HyperMachineOntology, Operation, OperationResponse};
use serde_json::{json, Value};

/// A secondary `{error, code}` shape: the runtime's and the fabric's own errors.
fn fabric_fail(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: Some(json!({
            "type": "object",
            "properties": { "error": { "type": "string" }, "code": { "type": "string" } }
        })),
    }
}

fn duration_schema() -> Value {
    json!({ "type": "object", "required": ["secs", "nanos"], "properties": {
        "secs": { "type": "integer" }, "nanos": { "type": "integer" } } })
}

fn system_time_schema() -> Value {
    json!({ "type": "object", "required": ["secs_since_epoch", "nanos_since_epoch"], "properties": {
        "secs_since_epoch": { "type": "integer" }, "nanos_since_epoch": { "type": "integer" } } })
}

fn placement_schema() -> Value {
    json!({ "type": "object", "properties": {
        "gpu_ids": { "type": "array", "items": { "type": "string" } },
        "host_id": { "type": "string" },
        "affinity_score": { "type": "number" },
        "aggregate_bandwidth_gbps": { "type": "number" },
        "same_numa": { "type": "boolean" } } })
}

fn workflow_spec_schema() -> Value {
    json!({
        "type": "object",
        "required": ["name", "description", "steps", "timeout", "variables", "max_parallel_steps"],
        "properties": {
            "name": { "type": "string" },
            "description": { "type": "string" },
            "steps": { "type": "array", "items": { "type": "object",
                "required": ["name", "description", "depends_on", "timeout", "max_retries", "retry_delay", "command", "optional"],
                "properties": {
                    "name": { "type": "string", "description": "unique in the workflow" },
                    "description": { "type": "string" },
                    "depends_on": { "type": "array", "items": { "type": "string" } },
                    "timeout": duration_schema(),
                    "max_retries": { "type": "integer" },
                    "retry_delay": duration_schema(),
                    "command": { "type": "string", "description": "stored; this server never runs it" },
                    "optional": { "type": "boolean" } } } },
            "timeout": duration_schema(),
            "variables": { "type": "object", "additionalProperties": { "type": "string" } },
            "max_parallel_steps": { "type": "integer" }
        }
    })
}

impl HyperMachineOntology {
    /// The fleet runtime and GPU fabric routes.
    pub(super) fn build_api_runtime_operations() -> Vec<Operation> {
        let keys = api_key_failures;
        let runtime_error = |status: u16, text: &str| fabric_fail(status, text);
        let no_class = || fabric_fail(404, "no VM class of that name (`CLASS_NOT_FOUND`)");

        let specs = vec![
            Spec::new(
                "get_runtime_status",
                "Read The Fleet Runtime's Status",
                "GET",
                "/api/v1/runtime/status",
            )
            .about(
                "Read a snapshot of the fleet runtime: its instance id, the pool's slots by \
                 state (total, warm, assigned, draining, recycling, failed, provisioning), \
                 active gateway routes, active workflows, durable store entries, the health \
                 summary, billing sessions and the autoscaler's cooldowns. `pending_workloads` \
                 is always 0: the runtime does not expose the scheduler's queue length there \
                 (`get_runtime_health` and the metrics do). The pool's \"VMs\" are slot records \
                 (an id, a state, the vCPUs and memory the pool configuration names): the \
                 runtime boots no VM and starts no process, and its slots are not the VMs of \
                 `create_vm`. Read only. Mounted only while the runtime is enabled (the \
                 default). Open unless an API key is configured.",
            )
            .answers(ok(
                200,
                "The status.",
                Some(json!({ "type": "object", "properties": {
                    "instance_id": { "type": "string" },
                    "pool": { "type": "object", "properties": {
                        "total": { "type": "integer" }, "warm": { "type": "integer" },
                        "assigned": { "type": "integer" }, "draining": { "type": "integer" },
                        "recycling": { "type": "integer" }, "failed": { "type": "integer" },
                        "provisioning": { "type": "integer" } } },
                    "active_routes": { "type": "integer" },
                    "pending_workloads": { "type": "integer", "description": "always 0" },
                    "active_workflows": { "type": "integer" },
                    "store_entries": { "type": "integer" },
                    "health_summary": { "type": "object" },
                    "billing_sessions": { "type": "integer" },
                    "scale_up_cooldown": { "type": "boolean" },
                    "scale_down_cooldown": { "type": "boolean" },
                    "timestamp": system_time_schema() } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_runtime_health",
                "Read The Fleet Runtime's Health",
                "GET",
                "/api/v1/runtime/health",
            )
            .about(
                "Read the runtime's health: `status` is `degraded` when any monitored slot is \
                 unhealthy, `warning` when any pool slot has failed, and `healthy` otherwise, \
                 together with the slot counts, healthy, degraded and unhealthy counts, active \
                 billing sessions and the scheduler's pending workloads. It always answers \
                 200, a `degraded` status included, so a probe must read `status`. Read only. Open unless \
                 an API key is configured.",
            )
            .answers(ok(
                200,
                "The health.",
                Some(json!({ "type": "object", "properties": {
                    "status": { "type": "string", "enum": ["healthy", "warning", "degraded"] },
                    "version": { "type": "string" },
                    "uptime_seconds": { "type": "integer" },
                    "instance_id": { "type": "string" },
                    "pool_total": { "type": "integer" }, "pool_warm": { "type": "integer" },
                    "pool_assigned": { "type": "integer" }, "pool_failed": { "type": "integer" },
                    "healthy_vms": { "type": "integer" }, "degraded_vms": { "type": "integer" },
                    "unhealthy_vms": { "type": "integer" },
                    "active_sessions": { "type": "integer" },
                    "pending_workloads": { "type": "integer" } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_runtime_metrics",
                "Read The Fleet Runtime's Metrics",
                "GET",
                "/api/v1/runtime/metrics",
            )
            .about(
                "Read the runtime's full metrics snapshot as JSON: pool slots by state and \
                 the totals of assignments and recycles, scheduler pending, active workflows, \
                 gateway routes, health counts, billing sessions, store entries and the \
                 autoscaler's counters, with the instance id and uptime. Read only. The same \
                 numbers `get_runtime_metrics_prometheus` renders as text. Open unless an API \
                 key is configured.",
            )
            .answers(ok(
                200,
                "The metrics snapshot.",
                Some(json!({ "type": "object", "properties": {
                    "instance_id": { "type": "string" },
                    "uptime_seconds": { "type": "integer" },
                    "pool_total": { "type": "integer" },
                    "scheduler_pending": { "type": "integer" },
                    "workflows_active": { "type": "integer" },
                    "health_healthy": { "type": "integer" },
                    "billing_sessions": { "type": "integer" },
                    "store_entries": { "type": "integer" } },
                    "description": "more counters and gauges than are listed here" })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_runtime_metrics_prometheus",
                "Read The Fleet Runtime's Metrics (Prometheus)",
                "GET",
                "/api/v1/runtime/metrics/prometheus",
            )
            .about(
                "Read the runtime's metrics in the Prometheus text exposition format \
                 (`text/plain; version=0.0.4`): the same snapshot as `get_runtime_metrics`. \
                 It is separate from `get_prometheus_metrics` (`/metrics`), which, on this \
                 server, carries only the API's own two series. Read only. Open unless an \
                 API key is configured; a scraper then needs to send the key.",
            )
            .answers(ok(
                200,
                "Prometheus text.",
                Some(json!({ "type": "string" })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "create_runtime_session",
                "Create A Runtime Session",
                "POST",
                "/api/v1/runtime/sessions",
            )
            .about(
                "Open an agent session in the fleet runtime: take a warm slot from the pool, \
                 register the session for billing under `tier`, route the session to the slot, \
                 register the slot for health monitoring and record the session in the durable \
                 store. The answer names the slot (`vm_id`), which is a pool record and not a \
                 VM from `create_vm`: no VM is booted and nothing runs in it. With no warm \
                 slot it is a 500 (`No warm VMs available`); the server pre-warms \
                 `pre_warm_count` slots (2 by default) when it starts, and only a maintenance \
                 tick or the autoscaler adds more. Billing is the runtime's metering of the \
                 session, not a charge made anywhere. `tier` is `Free`, `Standard` (default), \
                 `Premium` or `Enterprise`. Not idempotent. Open unless an API key is \
                 configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["session_id"],
                    "properties": {
                        "session_id": { "type": "string" },
                        "tier": { "type": "string", "enum": ["Free", "Standard", "Premium", "Enterprise"], "default": "Standard" }
                    }
                }),
                true,
            )
            .answers(ok(
                201,
                "The session and the slot it was given.",
                Some(json!({ "type": "object", "properties": { "session": { "type": "object", "properties": {
                    "session_id": { "type": "string" },
                    "vm_id": { "type": "string", "description": "a pool slot id" },
                    "tier": { "type": "string" } } } } })),
            ))
            .answers(runtime_error(500, "the pool has no warm slot, or another subsystem refused (`INTERNAL_ERROR`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "destroy_runtime_session",
                "Destroy A Runtime Session",
                "DELETE",
                "/api/v1/runtime/sessions/{id}",
            )
            .about(
                "Close a runtime session: generate its final invoice, remove its gateway \
                 route, release and recycle its pool slot, stop monitoring the slot, remove \
                 it from billing and delete its stored record. It answers 200 for any id, \
                 including one that was never created or was already destroyed, with \
                 `invoice_generated: false` when there was nothing to invoice, so a 200 does \
                 not show that a session existed. `total_charges` is the metering's total, \
                 not a payment taken. Open unless an API key is configured, and then any key \
                 may destroy any session.",
            )
            .with(path_param(
                "id",
                "The session id given to `create_runtime_session`",
                json!({ "type": "string" }),
            ))
            .answers(ok(
                200,
                "The session is gone.",
                Some(json!({ "type": "object", "properties": {
                    "session_id": { "type": "string" },
                    "invoice_generated": { "type": "boolean" },
                    "total_charges": { "type": ["number", "null"] } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "submit_runtime_workload",
                "Submit A Workload To The Scheduler",
                "POST",
                "/api/v1/runtime/workloads",
            )
            .about(
                "Queue a workload description (what a placement needs: vCPUs, memory, GPU, \
                 priority, constraints) and try at once to place it on a warm pool slot. 201 \
                 with `placed: true` and the slot when it was placed, 202 with `placed: false` \
                 when no slot fits and it waits in the queue for `schedule_runtime_workloads`. \
                 Placing records a decision (the workload id against a slot id, kept in the \
                 durable store); it starts nothing: the descriptor carries no command, and \
                 no slot is a VM. Every field of the body is required, `submitted_at` and \
                 `placement_timeout` in the serde shapes shown. A full queue (1000 pending) \
                 or another scheduler refusal is a 500. Not idempotent. Open unless an API \
                 key is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["id", "session_id", "required_vcpus", "required_memory", "requires_gpu", "priority", "constraints", "submitted_at", "placement_timeout", "attempts"],
                    "properties": {
                        "id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "required_vcpus": { "type": "integer" },
                        "required_memory": { "type": "integer", "description": "bytes" },
                        "requires_gpu": { "type": "boolean" },
                        "priority": { "type": "integer", "description": "higher is placed first" },
                        "constraints": { "type": "array", "items": { "type": "object", "description": "RequireVm, ExcludeVms, AffinityWith, AntiAffinityWith, MinVcpus or MinMemory, each as `{\"Name\": value}`" } },
                        "submitted_at": system_time_schema(),
                        "placement_timeout": duration_schema(),
                        "attempts": { "type": "integer" }
                    }
                }),
                true,
            )
            .answers(ok(
                201,
                "Placed.",
                Some(json!({ "type": "object", "properties": {
                    "workload_id": { "type": "string" },
                    "vm_id": { "type": ["string", "null"] },
                    "placed": { "type": "boolean" } } })),
            ))
            .answers(ok(202, "Queued, not placed (`placed: false`).", None))
            .answers(runtime_error(500, "the queue is full or the scheduler refused it (`INTERNAL_ERROR`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "schedule_runtime_workloads",
                "Place Every Pending Workload",
                "POST",
                "/api/v1/runtime/workloads/schedule",
            )
            .about(
                "Run the scheduler over the whole queue once and answer the placements it \
                 made (workload id, slot id, score, strategy, when). A workload that still \
                 fits no slot stays queued. As with `submit_runtime_workload` a placement is \
                 a recorded decision and starts nothing. No body is read, but the request \
                 still needs `Content-Type: application/json`. Open unless an API key is \
                 configured.",
            )
            .answers(ok(
                200,
                "The placements made by this call.",
                Some(json!({ "type": "object", "properties": {
                    "placed": { "type": "integer" },
                    "placements": { "type": "array", "items": { "type": "object", "properties": {
                        "workload_id": { "type": "string" },
                        "vm_id": { "type": "string" },
                        "score": { "type": "number" },
                        "strategy": { "type": "string" },
                        "decided_at": system_time_schema() } } } } })),
            ))
            .answers(runtime_error(500, "the scheduler failed (`INTERNAL_ERROR`)"))
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "run_runtime_workflow",
                "Start A Workflow",
                "POST",
                "/api/v1/runtime/workflows",
            )
            .about(
                "Validate a workflow (a DAG of named steps with dependencies: no cycle, every \
                 dependency a step), start it and answer its id. Nothing here executes the \
                 steps: each step's `command` is stored and never run, no process is started, \
                 and the engine only tracks which steps are ready. The caller (or whatever \
                 does the work) reports each step's outcome with \
                 `advance_runtime_workflow_step`, and the answer to that names the steps that \
                 have become ready. The first ready steps are the ones with no dependencies; this route does not list them. A spec that does not validate, or more concurrent \
                 workflows than the engine allows, is 400. Not idempotent. Open unless an API \
                 key is configured.",
            )
            .takes(workflow_spec_schema(), true)
            .answers(ok(
                201,
                "Started.",
                Some(json!({ "type": "object", "properties": { "workflow_id": { "type": "string" } } })),
            ))
            .answers(runtime_error(400, "the workflow is invalid or too many are running (`BAD_REQUEST`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "advance_runtime_workflow_step",
                "Report A Workflow Step's Outcome",
                "POST",
                "/api/v1/runtime/workflows/{id}/steps/{step}",
            )
            .about(
                "Record the outcome of one step and answer the steps now ready. The body is \
                 one of `{\"Success\": {\"output\": <string or null>}}`, `{\"Failure\": \
                 {\"error\": <string>, \"retryable\": <bool>}}` (a retryable failure returns \
                 the step to pending while it has retries left) or `{\"Skipped\": {\"reason\": \
                 <string>}}`. The runtime takes the caller's word: it does not run the step \
                 or check that it ran. A step name the workflow does not have is not an \
                 error: nothing changes and the ready steps are returned. A workflow that \
                 this outcome finishes is moved to the completed list, after which its id is \
                 unknown to `cancel_runtime_workflow`. An unknown workflow is 400, not 404. \
                 Open unless an API key is configured, and then any key may report any \
                 workflow's steps.",
            )
            .with(path_param("id", "The workflow's id", json!({ "type": "string" })))
            .with(path_param("step", "The step's name", json!({ "type": "string" })))
            .takes(
                json!({
                    "type": "object",
                    "oneOf": [
                        { "required": ["Success"], "properties": { "Success": { "type": "object", "properties": { "output": { "type": ["string", "null"] } } } } },
                        { "required": ["Failure"], "properties": { "Failure": { "type": "object", "required": ["error", "retryable"], "properties": { "error": { "type": "string" }, "retryable": { "type": "boolean" } } } } },
                        { "required": ["Skipped"], "properties": { "Skipped": { "type": "object", "required": ["reason"], "properties": { "reason": { "type": "string" } } } } }
                    ]
                }),
                true,
            )
            .answers(ok(
                200,
                "The steps ready after this outcome.",
                Some(json!({ "type": "object", "properties": {
                    "ready_steps": { "type": "array", "items": { "type": "string" } } } })),
            ))
            .answers(runtime_error(400, "no such workflow (or it already finished) (`BAD_REQUEST`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "cancel_runtime_workflow",
                "Cancel A Workflow",
                "DELETE",
                "/api/v1/runtime/workflows/{id}",
            )
            .about(
                "Cancel a workflow that has not finished and delete its stored record. The \
                 workflow moves to the completed list as cancelled; nothing that was running \
                 for it is stopped, because the runtime ran nothing. A workflow that is \
                 unknown or already finished is a 400 (`BAD_REQUEST`), not a 404. Answers 204 \
                 with no body. Open unless an API key is configured.",
            )
            .with(path_param("id", "The workflow's id", json!({ "type": "string" })))
            .answers(plain(204, "Cancelled. No body."))
            .answers(runtime_error(400, "unknown or already finished (`BAD_REQUEST`)"))
            .answers_all(keys()),
            Spec::new(
                "run_runtime_maintenance",
                "Run A Runtime Maintenance Tick",
                "POST",
                "/api/v1/runtime/maintenance",
            )
            .about(
                "Run one maintenance cycle over the runtime, and answer what it did. It \
                 removes the gateway routes (and stored session records) of unhealthy pool \
                 slots and terminates those slots; expires idle gateway sessions and drops \
                 their billing registration (so a session can end without `destroy_runtime_session`); \
                 terminates slots whose idle or lifetime limit has passed; lets the \
                 autoscaler add or remove warm slots; tops the pool up to its minimum of warm \
                 slots; and garbage-collects expired store entries. Nothing in this server \
                 calls it on a timer: it runs only when this route is called, so idle sessions are not expired \
                 and an empty pool is not refilled until someone does. The slots it adds \
                 and removes are records, not VMs. No body is read, but the request needs \
                 `Content-Type: application/json`. Not idempotent. Open unless an API key is \
                 configured, and then any key may run it.",
            )
            .answers(ok(
                200,
                "What the tick did.",
                Some(json!({ "type": "object", "properties": {
                    "unhealthy_removed": { "type": "array", "items": { "type": "string" } },
                    "degraded_detected": { "type": "array", "items": { "type": "string" } },
                    "sessions_expired": { "type": "array", "items": { "type": "string" } },
                    "maintenance_candidates": { "type": "array", "items": { "type": "string" } },
                    "scale_decision": { "type": ["object", "null"] },
                    "vms_provisioned": { "type": "integer" },
                    "vms_terminated": { "type": "integer" },
                    "store_gc_count": { "type": "integer" } } })),
            ))
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "get_gpu_topology",
                "Read The GPU Topology",
                "GET",
                "/api/v1/gpu-fabric/topology",
            )
            .about(
                "Read the GPU topology map: how many devices it holds, how many of them are \
                 not allocated, and the hosts they sit on. The map holds only what callers \
                 declared with `add_gpu_device`: nothing probes the machine's hardware, and \
                 the server starts with an empty map. Read only. Mounted only while the \
                 runtime is enabled. Open unless an API key is configured.",
            )
            .answers(ok(
                200,
                "The overview.",
                Some(json!({ "type": "object", "properties": {
                    "total_devices": { "type": "integer" },
                    "available_devices": { "type": "integer" },
                    "hosts": { "type": "array", "items": { "type": "string" } } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "add_gpu_device",
                "Declare A GPU In The Topology",
                "POST",
                "/api/v1/gpu-fabric/topology/devices",
            )
            .about(
                "Add a GPU to the topology map, by an id the caller chooses, on a host id the \
                 caller chooses. Nothing checks that the device or the host exists: the map is \
                 what callers declare, and `find_gpu_placement` and the class placement read \
                 it. An existing id is replaced, silently (and comes back unallocated), and \
                 the answer is 201 either way, with no body. The state is the server \
                 process's memory. Idempotent for the same body. Open unless an API key is \
                 configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["id", "host_id", "model"],
                    "properties": {
                        "id": { "type": "string" },
                        "host_id": { "type": "string" },
                        "model": { "type": "string" },
                        "numa_node": { "type": "integer", "default": 0 },
                        "pci_address": { "type": "string", "default": "" },
                        "vram_bytes": { "type": "integer", "default": 0 },
                        "compute_capability": { "type": "integer", "default": 0 }
                    }
                }),
                true,
            )
            .answers(plain(201, "Declared. No body."))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "add_gpu_link",
                "Declare A GPU Interconnect Link",
                "POST",
                "/api/v1/gpu-fabric/topology/links",
            )
            .about(
                "Add a link between two GPU ids (both directions) with an interconnect kind \
                 and a count, which placement scores for affinity and bandwidth. The ids are \
                 not checked against the declared devices, and a link between the same pair \
                 replaces the earlier one. `interconnect` is `nvlink`, `nvswitch`, \
                 `pcie_peer`, `pcie_root` or `cross_numa`; anything else is 400 \
                 `INVALID_INTERCONNECT`. The answer is 201 with no body. Idempotent for the \
                 same body. Open unless an API key is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["from", "to", "interconnect"],
                    "properties": {
                        "from": { "type": "string" },
                        "to": { "type": "string" },
                        "interconnect": { "type": "string", "enum": ["nvlink", "nvswitch", "pcie_peer", "pcie_root", "cross_numa"] },
                        "link_count": { "type": "integer", "default": 1 }
                    }
                }),
                true,
            )
            .answers(plain(201, "Declared. No body."))
            .answers(fabric_fail(400, "an unknown interconnect (`INVALID_INTERCONNECT`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "find_gpu_placement",
                "Find A GPU Placement",
                "POST",
                "/api/v1/gpu-fabric/topology/placement",
            )
            .about(
                "Ask the topology map for the best group of unallocated GPUs on one host that \
                 meets the requirements (count, minimum VRAM and compute capability, same NUMA \
                 node, NVLink, minimum bandwidth), scored on interconnect affinity. It is a \
                 query: it allocates nothing and marks nothing, so the same call answers the \
                 same GPUs until the map changes, and nothing in this server attaches the \
                 GPUs it names to a VM. 422 `PLACEMENT_FAILED` when no group fits. A POST \
                 that changes nothing, so repeatable. Open unless an API key is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["gpu_count"],
                    "properties": {
                        "gpu_count": { "type": "integer" },
                        "min_vram_bytes": { "type": "integer", "default": 0 },
                        "min_compute_capability": { "type": "integer", "default": 0 },
                        "same_numa": { "type": "boolean", "default": false },
                        "require_nvlink": { "type": "boolean", "default": false },
                        "min_bandwidth_gbps": { "type": "number", "default": 0 }
                    }
                }),
                true,
            )
            .answers(ok(200, "The placement.", Some(placement_schema())))
            .answers(fabric_fail(422, "no group of GPUs satisfies the requirements (`PLACEMENT_FAILED`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_gpu_fleet",
                "Read The GPU Fleet",
                "GET",
                "/api/v1/gpu-fabric/fleet",
            )
            .about(
                "List the fleet hosts the fabric knows: each host's id, whether it is \
                 healthy, how many VMs are active on it and its tags. This server has no \
                 route and no code path that registers a host, so on it the list is always \
                 empty (`host_count: 0`). Read only. Open unless an API key is configured.",
            )
            .answers(ok(
                200,
                "The hosts.",
                Some(json!({ "type": "object", "properties": {
                    "host_count": { "type": "integer" },
                    "hosts": { "type": "array", "items": { "type": "object", "properties": {
                        "id": { "type": "string" },
                        "healthy": { "type": "boolean" },
                        "active_vm_count": { "type": "integer" },
                        "tags": { "type": "object", "additionalProperties": { "type": "string" } } } } } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_gpu_capacity",
                "Read GPU Capacity Classes And Reservations",
                "GET",
                "/api/v1/gpu-fabric/capacity",
            )
            .about(
                "List the VM classes registered with `register_gpu_vm_class` (name, GPU \
                 count, instance limit) and how many reservations are active. Read only. \
                 Open unless an API key is configured.",
            )
            .answers(ok(
                200,
                "The classes and the active reservation count.",
                Some(json!({ "type": "object", "properties": {
                    "vm_classes": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "gpu_count": { "type": "integer" },
                        "max_instances": { "type": "integer" } } } },
                    "active_reservations": { "type": "integer" } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "register_gpu_vm_class",
                "Register A GPU VM Class",
                "POST",
                "/api/v1/gpu-fabric/capacity/classes",
            )
            .about(
                "Define a named VM class: an SLA tier, vCPUs, memory, a GPU count and model, \
                 the structured GPU requirements placement uses (minimum VRAM and compute \
                 capability, NVLink, same NUMA node, minimum bandwidth), whether it is \
                 dedicated to a host, an hourly rate and a limit on instances. It is a \
                 catalogue entry: registering it provisions nothing, and the rate is stored, \
                 not charged here. `sla_tier` is `best_effort`, `standard`, `premium` or \
                 `dedicated` (400 `INVALID_SLA_TIER` otherwise); a name already registered \
                 is 409 `CLASS_REGISTER_FAILED`, and a class cannot be edited. The answer is \
                 201 with no body. Not idempotent. Open unless an API key is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["name", "sla_tier"],
                    "properties": {
                        "name": { "type": "string" },
                        "sla_tier": { "type": "string", "enum": ["best_effort", "standard", "premium", "dedicated"] },
                        "description": { "type": "string", "default": "" },
                        "vcpus": { "type": "integer", "default": 2 },
                        "memory_bytes": { "type": "integer", "default": 0 },
                        "gpu_count": { "type": "integer", "default": 0 },
                        "gpu_model": { "type": "string", "default": "" },
                        "gpu_min_vram_bytes": { "type": "integer", "default": 0 },
                        "gpu_min_compute_capability": { "type": "integer", "default": 0 },
                        "gpu_require_nvlink": { "type": "boolean", "default": false },
                        "gpu_same_numa": { "type": "boolean", "default": false },
                        "gpu_min_bandwidth_gbps": { "type": "number", "default": 0 },
                        "dedicated_host": { "type": "boolean", "default": false },
                        "rate_per_hour": { "type": "number", "default": 0 },
                        "max_instances": { "type": "integer", "default": 4294967295u32 }
                    }
                }),
                true,
            )
            .answers(plain(201, "Registered. No body."))
            .answers(fabric_fail(400, "an unknown SLA tier (`INVALID_SLA_TIER`)"))
            .answers(fabric_fail(409, "a class of that name exists (`CLASS_REGISTER_FAILED`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "get_gpu_class_placement",
                "Resolve A VM Class Against The Topology",
                "GET",
                "/api/v1/gpu-fabric/capacity/classes/{name}/placement",
            )
            .about(
                "Resolve a registered class's GPU requirements against the declared topology: \
                 200 with a placement (GPU ids, host, scores) when the topology can satisfy \
                 it, 200 with `{\"gpu_required\": false}` for a class with no GPUs, 404 \
                 `CLASS_NOT_FOUND` for an unknown class, and 422 `PLACEMENT_FAILED` when the \
                 topology cannot. As with `find_gpu_placement` it allocates nothing. Read \
                 only. Open unless an API key is configured.",
            )
            .with(path_param("name", "The class's name", json!({ "type": "string" })))
            .answers(ok(
                200,
                "A placement, or `{gpu_required: false}`.",
                Some(placement_schema()),
            ))
            .answers(no_class())
            .answers(fabric_fail(422, "the topology cannot satisfy the class (`PLACEMENT_FAILED`)"))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "create_gpu_reservation",
                "Create A GPU Capacity Reservation",
                "POST",
                "/api/v1/gpu-fabric/capacity/reservations",
            )
            .about(
                "Record a reservation of `count` instances of a registered class for a tenant \
                 for `duration_hours`, and answer its id (`rsv-<n>`). It is a record with an \
                 expiry and nothing more: it does not check the class's instance limit, the \
                 topology, the GPUs free or other reservations, and it holds no GPU; \
                 `tenant_id` is whatever the caller sends. It is refused (422 \
                 `RESERVATION_FAILED`) only for an unknown class or a `count` of 0. Not \
                 idempotent: each call is a new reservation. Open unless an API key is \
                 configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["tenant_id", "vm_class", "count", "duration_hours"],
                    "properties": {
                        "tenant_id": { "type": "string" },
                        "vm_class": { "type": "string" },
                        "count": { "type": "integer", "minimum": 1 },
                        "duration_hours": { "type": "integer" }
                    }
                }),
                true,
            )
            .answers(ok(
                201,
                "Reserved.",
                Some(json!({ "type": "object", "properties": { "reservation_id": { "type": "string" } } })),
            ))
            .answers(fabric_fail(422, "an unknown class or a count of 0 (`RESERVATION_FAILED`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "cancel_gpu_reservation",
                "Cancel A GPU Capacity Reservation",
                "DELETE",
                "/api/v1/gpu-fabric/capacity/reservations/{id}",
            )
            .about(
                "Mark a reservation cancelled, so it is no longer counted as active. The \
                 reservation is kept; cancelling one that is already cancelled, or expired, \
                 is 204 again, and an unknown id is 404 `RESERVATION_NOT_FOUND`. Any caller \
                 may cancel any tenant's reservation. Answers 204 with no body. Open unless \
                 an API key is configured.",
            )
            .with(path_param("id", "The reservation's id", json!({ "type": "string" })))
            .answers(plain(204, "Cancelled. No body."))
            .answers(fabric_fail(404, "no such reservation (`RESERVATION_NOT_FOUND`)"))
            .answers_all(keys())
            .repeatable(),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }
}
