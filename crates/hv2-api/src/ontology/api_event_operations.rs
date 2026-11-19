//! `hv2-api`'s VM snapshot catalogue (`snapshot_routes.rs`), event delivery
//! (`events.rs`, `ws_routes.rs`) and health and metrics probes (`rest.rs`,
//! `health_routes.rs`, `metrics_routes.rs`). Modelled as data from the
//! handlers; each is served by this crate's own server.

use super::spec::{
    api_key_failures, body_rejection, json_content_type_required, ok, path_param, plain,
    query_param, Spec,
};
use super::{HyperMachineOntology, Operation, OperationResponse, Parameter};
use serde_json::{json, Value};

/// The snapshot and webhook routes answer `{"error": "..."}` with no code.
fn error_only(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: Some(json!({ "type": "object", "properties": { "error": { "type": "string" } } })),
    }
}

fn vm_id() -> Parameter {
    path_param(
        "vm_id",
        "A VM id. It need not name a VM this server holds: any string is accepted, and a \
         catalogue is created for it on first use",
        json!({ "type": "string" }),
    )
}

fn snapshot_id() -> Parameter {
    path_param(
        "snap_id",
        "The snapshot's numeric id, as `create_vm_snapshot` answered with. A value that is \
         not a number is the framework's own 400",
        json!({ "type": "integer" }),
    )
}

fn snapshot_schema() -> Value {
    json!({ "type": "object", "properties": {
        "id": { "type": "integer" },
        "name": { "type": "string" },
        "description": { "type": ["string", "null"] },
        "state": { "type": "string", "description": "Rust debug name: Creating, Valid, Restoring, Invalid or Deleting" },
        "snapshot_type": { "type": "string", "description": "Full, Incremental, MemoryOnly or Checkpoint" },
        "size_bytes": { "type": "integer", "description": "always 0: nothing is captured" },
        "vcpu_count": { "type": "integer", "description": "always 0: nothing is captured" },
        "created_at": { "type": "string", "description": "the Rust debug rendering of a SystemTime, not ISO 8601" },
        "tags": { "type": "array", "items": { "type": "string" } },
        "parent_id": { "type": ["integer", "null"] }
    } })
}

fn webhook_schema() -> Value {
    json!({ "type": "object", "properties": {
        "id": { "type": "string" },
        "url": { "type": "string" },
        "events": { "type": "array", "items": { "type": "string" } },
        "active": { "type": "boolean" },
        "created_at": { "type": "string" },
        "last_triggered": { "type": ["string", "null"], "description": "never set" },
        "failure_count": { "type": "integer", "description": "never incremented" },
        "headers": { "type": "object", "additionalProperties": { "type": "string" }, "description": "the custom headers, returned as given" }
    }, "description": "the secret is never returned" })
}

fn event_schema() -> Value {
    json!({ "type": "object", "required": ["id", "event_type", "timestamp", "data"], "properties": {
        "id": { "type": "string" },
        "event_type": { "type": "string", "enum": [
            "vm_created", "vm_started", "vm_stopped", "vm_paused", "vm_resumed", "vm_deleted",
            "snapshot_created", "snapshot_restored", "vm_migrated", "resource_alert",
            "health_check_failed", "agent_action_completed", "error", "all"] },
        "timestamp": { "type": "string", "description": "ISO 8601, chosen by the sender of a published event" },
        "vm_id": { "type": ["string", "null"] },
        "data": { "description": "any JSON" },
        "correlation_id": { "type": ["string", "null"] }
    } })
}

impl HyperMachineOntology {
    /// The snapshot, event and health and metrics routes.
    pub(super) fn build_api_event_operations() -> Vec<Operation> {
        let keys = api_key_failures;
        let no_snapshot = || error_only(404, "the VM's catalogue has no snapshot of that id");

        let specs = vec![
            Spec::new(
                "create_vm_snapshot",
                "Create A VM Snapshot Record",
                "POST",
                "/api/v1/vms/{vm_id}/snapshots",
            )
            .about(
                "Add a snapshot to a VM id's catalogue and mark it `Valid`. It is a catalogue \
                 entry only: the route never looks at the VM. Nothing reads the VM's CPU, \
                 memory or devices, `size_bytes` and `vcpu_count` stay 0, and the id need not \
                 name a VM this server holds, so a `Valid` snapshot is not a restore point. \
                 The catalogues live in this process's memory and are gone when the server \
                 restarts. `snapshot_type` is `full` (the default, and what any unknown value \
                 becomes), `incremental` (needs a `parent_id` already in the catalogue), \
                 `memory_only` or `checkpoint`. Tags are a list of strings. Only one snapshot \
                 per VM id can be in progress: with `async_capture` the answer is 202 with \
                 the snapshot `Creating`, a background task marks it `Valid` at once, and a \
                 second create before that is 400. At 100 snapshots for the id the oldest \
                 `Valid` snapshot that is nobody's parent is deleted to make room, with no \
                 notice. 201 with the snapshot. Not idempotent. No event is published. Open \
                 unless an API key is configured.",
            )
            .with(vm_id())
            .takes(
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "default `Snapshot <id>`" },
                        "description": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" }, "default": [] },
                        "snapshot_type": { "type": "string", "enum": ["full", "incremental", "memory_only", "checkpoint"], "default": "full" },
                        "parent_id": { "type": "integer" },
                        "async_capture": { "type": "boolean", "default": false }
                    }
                }),
                true,
            )
            .answers(ok(201, "The snapshot, `Valid`.", Some(snapshot_schema())))
            .answers(ok(202, "The snapshot, still `Creating` (`async_capture`).", Some(snapshot_schema())))
            .answers(error_only(
                400,
                "another snapshot of this VM id is in progress, or `parent_id` is not in the \
                 catalogue",
            ))
            .answers(error_only(500, "the snapshot could not be completed"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "list_vm_snapshots",
                "List A VM's Snapshot Records",
                "GET",
                "/api/v1/vms/{vm_id}/snapshots",
            )
            .about(
                "List the snapshot records in a VM id's catalogue, in no stated order and \
                 without paging. An id the server has never seen answers an empty list, and the \
                 call creates that id's empty catalogue, so a read with arbitrary ids grows \
                 the server's memory. Read only. Open unless an API key is configured.",
            )
            .with(vm_id())
            .answers(ok(
                200,
                "The catalogue.",
                Some(json!({ "type": "object", "properties": {
                    "vm_id": { "type": "string" },
                    "snapshots": { "type": "array", "items": snapshot_schema() },
                    "total": { "type": "integer" } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_vm_snapshot",
                "Read A VM Snapshot Record",
                "GET",
                "/api/v1/vms/{vm_id}/snapshots/{snap_id}",
            )
            .about(
                "Read one snapshot record: its name, description, state (`Creating` while an \
                 asynchronous capture is pending, then `Valid`), type, tags and parent. It \
                 describes a catalogue entry, not stored VM state: see `create_vm_snapshot`. \
                 Read only. Open unless an API key is configured.",
            )
            .with(vm_id())
            .with(snapshot_id())
            .answers(ok(200, "The snapshot.", Some(snapshot_schema())))
            .answers(no_snapshot())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "delete_vm_snapshot",
                "Delete A VM Snapshot Record",
                "DELETE",
                "/api/v1/vms/{vm_id}/snapshots/{snap_id}",
            )
            .about(
                "Remove a snapshot record from the catalogue. It is refused with 409 when it \
                 is the parent of another snapshot (delete the children first) or is the one \
                 in progress. There is no stored data to free. Answers 204 with no body. Open \
                 unless an API key is configured.",
            )
            .with(vm_id())
            .with(snapshot_id())
            .answers(plain(204, "Deleted. No body."))
            .answers(no_snapshot())
            .answers(error_only(409, "the snapshot is another's parent or is in progress"))
            .answers_all(keys()),
            Spec::new(
                "restore_vm_snapshot",
                "Restore A VM Snapshot Record",
                "POST",
                "/api/v1/vms/{vm_id}/snapshots/{snap_id}/restore",
            )
            .about(
                "Begin and at once complete a restore of a snapshot record, and answer \
                 `status: \"restored\"`. Nothing is restored: the handler's own comment reads \
                 `in real system, would replay state`, and today it changes nothing \
                 in any VM (it touches only the catalogue's counters). A `restored` answer \
                 is therefore not evidence that a VM went back to anything. 404 for an unknown \
                 snapshot; 400 when the snapshot is not `Valid` or another snapshot operation \
                 is in progress. Open unless an API key is configured, and then any key may \
                 \"restore\". No event is published.",
            )
            .with(vm_id())
            .with(snapshot_id())
            .answers(ok(
                200,
                "Reported restored.",
                Some(json!({ "type": "object", "properties": {
                    "vm_id": { "type": "string" },
                    "snapshot_id": { "type": "integer" },
                    "status": { "type": "string", "enum": ["restored"] } } })),
            ))
            .answers(no_snapshot())
            .answers(error_only(400, "the snapshot is not `Valid`, or another operation is in progress"))
            .answers(error_only(500, "the restore could not be completed"))
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "create_webhook",
                "Subscribe A Webhook",
                "POST",
                "/api/v1/events/webhooks",
            )
            .about(
                "Register a URL to be POSTed each event of the listed types. The URL is not \
                 validated in any way (no scheme, host or address check, no DNS pinning, \
                 redirects are followed), so the server will send requests, with the event's \
                 JSON, to whatever address is given, internal addresses included. Delivery is \
                 one attempt with no retry, any HTTP answer counts as delivered, a failure is \
                 only logged, and the subscription's `failure_count` and `last_triggered` are \
                 never updated. Each delivery carries `X-HyperMachine-Event` (the dotted type, \
                 such as `vm.created`), `X-HyperMachine-Delivery` (the event id), your custom \
                 `headers` and, with a `secret`, `X-HyperMachine-Signature`, which is a \
                 64-bit non-cryptographic hash of the secret and the body written as \
                 `sha256=<hex>`, not an HMAC-SHA256, and is not verifiable as one. The secret \
                 is never returned but custom `headers` are, in every read. Subscriptions are \
                 memory only. The only events that exist are those `publish_event` posts: \
                 nothing else in this server publishes. Exists only while events are enabled \
                 (the default). Not idempotent. Open unless an API key is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["url", "events"],
                    "properties": {
                        "url": { "type": "string" },
                        "events": { "type": "array", "items": { "type": "string" }, "description": "event types; `all` matches every type" },
                        "secret": { "type": "string" },
                        "headers": { "type": "object", "additionalProperties": { "type": "string" }, "default": {} }
                    }
                }),
                true,
            )
            .answers(ok(201, "The subscription.", Some(webhook_schema())))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "list_webhooks",
                "List Webhook Subscriptions",
                "GET",
                "/api/v1/events/webhooks",
            )
            .about(
                "List every webhook subscription, sorted by id, paged by `offset` and `limit` \
                 (limit 20 by default, at most 100). It is the whole server's list: there is no \
                 owner or tenant. Each carries its custom headers, which may hold credentials. \
                 Read only. Open unless an API key is configured.",
            )
            .with(query_param("offset", "Subscriptions to skip", json!({ "type": "integer", "minimum": 0, "default": 0 })))
            .with(query_param("limit", "Page size", json!({ "type": "integer", "minimum": 1, "maximum": 100, "default": 20 })))
            .answers(ok(
                200,
                "A page.",
                Some(json!({ "type": "object", "properties": {
                    "webhooks": { "type": "array", "items": webhook_schema() },
                    "total": { "type": "integer" },
                    "offset": { "type": "integer" },
                    "limit": { "type": "integer" },
                    "has_more": { "type": "boolean" } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_webhook",
                "Read A Webhook Subscription",
                "GET",
                "/api/v1/events/webhooks/{id}",
            )
            .about(
                "Read one subscription: its URL, event types, active flag, creation time and \
                 custom headers. The secret is never returned. Read only. Open unless an API \
                 key is configured.",
            )
            .with(path_param("id", "The subscription's id", json!({ "type": "string" })))
            .answers(ok(200, "The subscription.", Some(webhook_schema())))
            .answers(error_only(404, "no such subscription (`Webhook not found`)"))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "delete_webhook",
                "Delete A Webhook Subscription",
                "DELETE",
                "/api/v1/events/webhooks/{id}",
            )
            .about(
                "Remove a subscription. Events published after it no longer reach the URL; a \
                 delivery already in flight is not recalled. Answers 204 with no body. Open \
                 unless an API key is configured, and then any key may remove any subscription.",
            )
            .with(path_param("id", "The subscription's id", json!({ "type": "string" })))
            .answers(plain(204, "Removed. No body."))
            .answers(error_only(404, "no such subscription (`Webhook not found`)"))
            .answers_all(keys()),
            Spec::new(
                "stream_events",
                "Stream Events (SSE)",
                "GET",
                "/api/v1/events/stream",
            )
            .about(
                "Open a Server-Sent Events stream of the events published from now on (none \
                 are replayed). Each frame has `id` (the event id), `event` (the dotted type \
                 name, such as `vm.created`; `*` for `all`) and `data` (the JSON event, whose \
                 `event_type` is the snake_case name), and a `ping` comment is sent every 30 \
                 seconds. `events` is a comma-separated list of snake_case type names: a name \
                 that does not parse is dropped, so a list of only unknown names matches \
                 nothing, and `all` passes everything. `vm_id` keeps only events for that VM. \
                 A reader that falls more than 1024 events behind silently loses the ones it \
                 missed. Only events posted with `publish_event` exist; this server's VM, \
                 snapshot and runtime routes publish none. Exists only while events are \
                 enabled. Open unless an API key is configured.",
            )
            .with(query_param(
                "events",
                "Comma-separated event types, snake_case (`vm_started,vm_stopped`)",
                json!({ "type": "string" }),
            ))
            .with(query_param(
                "vm_id",
                "Only events whose `vm_id` is this",
                json!({ "type": "string" }),
            ))
            .answers(plain(200, "An `text/event-stream` that stays open."))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "publish_event",
                "Publish An Event",
                "POST",
                "/api/v1/events/publish",
            )
            .about(
                "Put an event, exactly as the caller wrote it (id, type, timestamp, VM id and \
                 data are all the sender's), on the bus: every SSE and WebSocket subscriber \
                 matching its type receives it, and every active webhook for its type is sent \
                 it. The code names the route a development endpoint, but it is mounted \
                 whenever events are enabled and it is the only publisher in this server, so \
                 any caller (any key holder, when a key is configured) can make subscribers \
                 and webhook targets see an event that never happened, for example a \
                 `vm_stopped` or `health_check_failed` for a real VM id. Answers 202 with the \
                 event echoed; delivery to webhooks happens afterwards and its failures are \
                 not reported here. Not idempotent. Exists only while events are enabled.",
            )
            .takes(event_schema(), true)
            .answers(ok(202, "Accepted, echoed.", Some(event_schema())))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "stream_events_websocket",
                "Stream Events (WebSocket)",
                "GET",
                "/api/v1/events/ws",
            )
            .about(
                "Upgrade to a WebSocket that carries each event published from then on as a \
                 JSON text frame (the same event as the SSE stream's `data`). It answers a \
                 ping with a pong, ignores other client messages and ends on a close frame or \
                 an error. `event_type` is one snake_case type name (`vm_started`; `all` passes \
                 everything); the module's own example writes `VmStarted`, which does not \
                 parse, and a value that does not parse means no filter at all, the reverse of \
                 the SSE stream's `events`. `vm_id` keeps only that VM's events. Events a slow \
                 client missed are skipped without notice. A request that is not a WebSocket \
                 upgrade gets the framework's own refusal. With an API key configured the key \
                 must be in the `Authorization` header of the upgrade request, which browsers \
                 cannot set. Only events posted with `publish_event` exist. Exists only while \
                 events are enabled. Open unless an API key is configured.",
            )
            .with(query_param(
                "event_type",
                "One event type, snake_case",
                json!({ "type": "string" }),
            ))
            .with(query_param(
                "vm_id",
                "Only events whose `vm_id` is this",
                json!({ "type": "string" }),
            ))
            .answers(plain(101, "Switching protocols; JSON text frames follow."))
            .answers(plain(400, "the request is not a valid WebSocket upgrade: the framework's own refusal (400 or 426)"))
            .answers_all(keys())
            .repeatable(),
            Spec::new("get_health", "Read The Server's Health", "GET", "/health")
                .about(
                    "Read the API server's own health: always `healthy` while it can answer, \
                     with its version, uptime in seconds, how many VMs it holds (`vm_count`) \
                     and whether the runtime and events subsystems are enabled. It checks \
                     nothing beneath. Read only. This route (like `/health/live`, \
                     `/health/ready` and everything under `/agentic`) is exempt from the API \
                     key for GET, so it answers anyone who can reach the port even when a key \
                     is required elsewhere, and what it reveals is the version and the VM \
                     count.",
                )
                .answers(ok(
                    200,
                    "The health.",
                    Some(json!({ "type": "object", "properties": {
                        "status": { "type": "string", "enum": ["healthy"] },
                        "version": { "type": "string" },
                        "uptime_seconds": { "type": "integer" },
                        "vm_count": { "type": "integer" },
                        "components": { "type": "object", "properties": {
                            "runtime": { "type": "string", "enum": ["enabled", "disabled"] },
                            "events": { "type": "string", "enum": ["enabled", "disabled"] } } } } })),
                ))
                .repeatable(),
            Spec::new(
                "get_liveness",
                "Liveness Probe",
                "GET",
                "/health/live",
            )
            .about(
                "Answer 200 `{\"status\": \"alive\"}` while the process serves requests; it \
                 checks nothing else. Meant for an orchestrator's liveness check. Exempt from \
                 the API key for GET.",
            )
            .answers(ok(
                200,
                "Alive.",
                Some(json!({ "type": "object", "properties": { "status": { "type": "string", "enum": ["alive"] } } })),
            ))
            .repeatable(),
            Spec::new(
                "get_readiness",
                "Readiness Probe",
                "GET",
                "/health/ready",
            )
            .about(
                "Report per-component readiness: `http_server` is `up`, and `runtime` and \
                 `events` are `up` or `disabled` by configuration. The handler can answer 503 \
                 `not_ready` if a check is `down`, but none is ever `down`, so on this server \
                 it always answers 200 `ready`: it does not test the pool, the registry or any \
                 dependency. Exempt from the API key for GET.",
            )
            .answers(ok(
                200,
                "Ready.",
                Some(json!({ "type": "object", "properties": {
                    "status": { "type": "string", "enum": ["ready", "not_ready"] },
                    "checks": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "status": { "type": "string", "enum": ["up", "disabled", "down"] } } } } } })),
            ))
            .answers(plain(503, "a check is `down`; unreachable today"))
            .repeatable(),
            Spec::new(
                "get_full_health",
                "Read Every Subsystem's Health",
                "GET",
                "/api/v1/health/full",
            )
            .about(
                "Read the API, runtime, events, GPU fabric and image registry in one answer. \
                 The GPU fabric entry carries device, host, fleet host, VM class and active \
                 reservation counts, and the image registry entry its image count and \
                 enforcement mode (`Audit`, `Enforce` or `Disabled`, as the registry's own \
                 setting, which is not consulted by VMs unless `enforce_image_admission` is \
                 on). The GPU fabric and image registry are `disabled` while the runtime is \
                 disabled. A subsystem is only ever `up` or `disabled`, so the answer is 200 \
                 `healthy` in practice (503 `degraded` is the branch for anything else). Read \
                 only. Unlike `/health`, this route is not exempt from the API key.",
            )
            .answers(ok(
                200,
                "The health.",
                Some(json!({ "type": "object", "properties": {
                    "status": { "type": "string", "enum": ["healthy", "degraded"] },
                    "version": { "type": "string" },
                    "uptime_seconds": { "type": "integer" },
                    "subsystems": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "status": { "type": "string", "enum": ["up", "disabled"] },
                        "detail": { "type": "object" } } } } } })),
            ))
            .answers(plain(503, "a subsystem is neither `up` nor `disabled`; unreachable today"))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_prometheus_metrics",
                "Read The API's Prometheus Metrics",
                "GET",
                "/metrics",
            )
            .about(
                "Read metrics in the Prometheus text format, ending with `# EOF`. The code \
                 can render the runtime's pool, scheduler, health and billing series, the GPU \
                 fabric's counts, the image registry's count and enforcement mode and the \
                 webhook count, but it only renders what its state was given, and this \
                 server gives it nothing: it answers just `hypermachine_api_uptime_seconds` \
                 and `hypermachine_api_info{version}`. For the runtime's series use \
                 `get_runtime_metrics_prometheus`. (When the code is given a runtime, its \
                 `hypermachine_scheduler_placed_total` is filled from the sessions-created \
                 counter, not from placements.) Read only. Not exempt from the API key, so a \
                 scraper must send it when one is configured; with none, anyone who reaches \
                 the port can read it.",
            )
            .answers(ok(200, "Prometheus text.", Some(json!({ "type": "string" }))))
            .answers_all(keys())
            .repeatable(),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }
}
