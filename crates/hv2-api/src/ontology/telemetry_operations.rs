//! What a sandbox reports and what happened to it: metrics and logs
//! (`hv2-sandboxd` `telemetry.rs`, relayed by the control plane's `forward`
//! and merged by `sandboxes_metrics`), sandbox events (`hv2-cluster`
//! `events.rs`, served by the control plane and, over its own store, the node)
//! and the recovery of a registration left uncertain (`pending_on_node`,
//! `reconcile_on_node`, and the node's `pending_registrations` and
//! `reconcile_registration`). Modelled as data: `hv2-api` depends on neither
//! crate.

use super::network_operations::control_plane_gate;
use super::spec::{fail, ok, path_param, plain, query_param, Spec};
use super::{HyperMachineOntology, Operation, Parameter};
use serde_json::{json, Value};

fn sample_schema() -> Value {
    json!({ "type": "object", "properties": {
        "timestamp": { "type": "string", "format": "date-time" },
        "timestampUnix": { "type": "integer", "description": "seconds" },
        "cpuCount": { "type": "integer" },
        "cpuUsedPct": { "type": "number", "description": "since the last sample; 0 for the first" },
        "memUsed": { "type": "integer", "description": "bytes" },
        "memTotal": { "type": "integer", "description": "bytes" },
        "memCache": { "type": "integer", "description": "bytes" },
        "diskUsed": { "type": "integer", "description": "bytes, of the root filesystem" },
        "diskTotal": { "type": "integer", "description": "bytes" } } })
}

fn log_entry_schema() -> Value {
    json!({ "type": "object", "properties": {
        "timestamp": { "type": "string", "format": "date-time" },
        "level": { "type": "string" },
        "message": { "type": "string" },
        "fields": { "type": "object", "description": "always empty" } } })
}

fn log_parameters() -> Vec<Parameter> {
    vec![
        query_param(
            "start",
            "Entries at or after this time, in unix milliseconds",
            json!({ "type": "integer", "minimum": 0 }),
        ),
        query_param(
            "cursor",
            "Used when `start` is not given: the same, a unix millisecond time",
            json!({ "type": "integer", "minimum": 0 }),
        ),
        query_param(
            "limit",
            "At most this many entries (default and most 1000)",
            json!({ "type": "integer", "minimum": 0, "maximum": 1000 }),
        ),
        query_param(
            "direction",
            "`backward` lists newest first, and the limit then keeps the newest; anything else is oldest first",
            json!({ "type": "string" }),
        ),
        query_param(
            "level",
            "Only entries of exactly this level",
            json!({ "type": "string" }),
        ),
        query_param(
            "search",
            "Only entries whose message contains this text (case-sensitive)",
            json!({ "type": "string" }),
        ),
    ]
}

fn event_schema() -> Value {
    json!({ "type": "object", "properties": {
        "id": { "type": "string" },
        "version": { "type": "string", "enum": ["v2"] },
        "type": { "type": "string", "enum": [
            "sandbox.lifecycle.created", "sandbox.lifecycle.paused", "sandbox.lifecycle.resumed",
            "sandbox.lifecycle.updated", "sandbox.lifecycle.killed"] },
        "eventCategory": { "type": "string", "enum": ["lifecycle"] },
        "eventLabel": { "type": "string", "enum": ["create", "pause", "resume", "update", "kill"] },
        "eventData": { "type": "object", "description": "`sandbox_metadata` (the sandbox's metadata as it was); for a pause or an end, `execution` (started_at, vcpu_count, memory_mb, execution_time in ms); for an end, `kill_reason` (`request`, `timeout` or `lost`); `node_id`; and `detail` where there is one" },
        "timestamp": { "type": "string", "format": "date-time" },
        "sandboxId": { "type": "string" },
        "sandboxExecutionId": { "type": "string", "description": "equal to sandboxId" },
        "sandboxTemplateId": { "type": "string" },
        "sandboxBuildId": { "type": "string", "description": "equal to the template" },
        "sandboxTeamId": { "type": "string", "description": "always the nil UUID: this cluster has one" } } })
}

fn event_query() -> Vec<Parameter> {
    vec![
        query_param(
            "offset",
            "Skip this many events of the answer",
            json!({ "type": "integer", "minimum": 0, "default": 0 }),
        ),
        query_param(
            "limit",
            "At most this many (default 100, most 1000)",
            json!({ "type": "integer", "minimum": 0, "maximum": 1000, "default": 100 }),
        ),
        query_param(
            "orderAsc",
            "`true` for oldest first; the default is newest first",
            json!({ "type": "boolean" }),
        ),
        query_param(
            "types",
            "Comma-separated event types to keep, such as `sandbox.lifecycle.killed`",
            json!({ "type": "string" }),
        ),
    ]
}

impl HyperMachineOntology {
    pub(super) fn telemetry_operations(node: bool) -> Vec<Operation> {
        let sandbox = || Self::sandbox_id_parameter(node);
        let access = |named: bool| Self::sandbox_access_responses(node, named);
        let no_such = || fail(404, "no sandbox of that ID on the node, running or paused");
        let range = || {
            vec![
                query_param(
                    "start",
                    "Samples at or after this time, in unix seconds",
                    json!({ "type": "integer", "minimum": 0 }),
                ),
                query_param(
                    "end",
                    "Samples at or before this time, in unix seconds",
                    json!({ "type": "integer", "minimum": 0 }),
                ),
            ]
        };
        let bad_query = || {
            plain(400, "a query parameter that is not a number or text as asked: the framework's own rejection")
        };

        let mut specs = vec![
            Spec::new(
                "list_sandboxes_metrics",
                "Latest Metrics Of Many Sandboxes",
                "GET",
                "/sandboxes/metrics",
            )
            .about(if node {
                "The latest sample of each sandbox on this node that has one, as \
                 `{\"sandboxes\": {id: sample}}`, optionally only those in `sandbox_ids`. \
                 A node answers for every sandbox it runs, whoever owns them: the control \
                 plane narrows the list to the caller's. Samples are the guest's own account \
                 (CPU from `/proc/stat`, memory from `/proc/meminfo`, the root filesystem), \
                 taken every 5 seconds from running sandboxes and held in the node's memory. \
                 A paused sandbox, or one in standby, is not sampled and not woken."
            } else {
                "The latest sample of each sandbox the caller may reach, from whichever node \
                 runs it, merged as `{\"sandboxes\": {id: sample}}`, optionally only those in \
                 `sandbox_ids`. A node that does not answer is left out without a word, so \
                 a missing sandbox may be one whose node is down. The control plane \
                 asks the store for the caller's sandboxes and filters the nodes' answers by \
                 it. An inventory key or an observer may read this route; every other \
                 sandbox route needs the `sandboxes` scope. Samples are the guest's own \
                 account taken every 5 seconds from running sandboxes; a paused one has none."
            })
            .with(query_param(
                "sandbox_ids",
                "Comma-separated sandbox IDs to keep",
                json!({ "type": "string" }),
            ))
            .answers(ok(
                200,
                "Each sandbox's latest sample, by ID.",
                Some(json!({ "type": "object", "properties": {
                    "sandboxes": { "type": "object", "additionalProperties": sample_schema() } } })),
            ))
            .answers(bad_query())
            .answers_all(if node {
                access(false)
            } else {
                let mut gate = control_plane_gate("sandboxes", false);
                gate.push(fail(400, "a path that is not as the control plane relays it"));
                gate
            })
            .repeatable(),
            Spec::new(
                "get_sandbox_metrics",
                "Read A Sandbox's Metrics",
                "GET",
                "/sandboxes/{SB}/metrics",
            )
            .about(
                "The samples kept for a sandbox, oldest first: one every 5 seconds from the \
                 guest's own account of CPU, memory and its root filesystem, and the last \
                 hour only (720). They are held in the node's memory and go with the \
                 sandbox, a node restart or an hour. A paused sandbox answers what it has, \
                 with no new samples, and one with none yet answers an empty list.",
            )
            .with(sandbox())
            .answers(ok(
                200,
                "The samples.",
                Some(json!({ "type": "array", "items": sample_schema() })),
            ))
            .answers(bad_query())
            .answers(no_such())
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "get_sandbox_logs",
                "Read A Sandbox's Lifecycle Log",
                "GET",
                "/sandboxes/{SB}/logs",
            )
            .about(
                "E2B's v1 logs: what the node noted of the sandbox, not what its programs \
                 printed (use command history for that). The entries are lifecycle events \
                 (`sandbox created from template ...`, `sandbox paused`, volumes mounted, \
                 checkpoints saved and restored, reboots), the newest 1000, held in the node's \
                 memory and gone with the sandbox or a node restart. Each entry comes twice: \
                 as `logs` (`timestamp` and `line`) and as `logEntries` (with `level` and an \
                 empty `fields`).",
            )
            .with(sandbox())
            .answers(ok(
                200,
                "The entries.",
                Some(json!({ "type": "object", "properties": {
                    "logs": { "type": "array", "items": { "type": "object", "properties": {
                        "timestamp": { "type": "string" }, "line": { "type": "string" } } } },
                    "logEntries": { "type": "array", "items": log_entry_schema() } } })),
            ))
            .answers(bad_query())
            .answers(no_such())
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "get_sandbox_logs_v2",
                "Read A Sandbox's Lifecycle Log (v2)",
                "GET",
                "/v2/sandboxes/{SB}/logs",
            )
            .about(
                "E2B's v2 logs: the same entries as `get_sandbox_logs`, only as structured \
                 entries under `logs`, with the same filters. Not what the sandbox's programs \
                 printed.",
            )
            .with(sandbox())
            .answers(ok(
                200,
                "The entries.",
                Some(json!({ "type": "object", "properties": {
                    "logs": { "type": "array", "items": log_entry_schema() } } })),
            ))
            .answers(bad_query())
            .answers(no_such())
            .answers_all(access(true))
            .repeatable(),
        ];

        let events_about = |one: bool| {
            let scope = if node {
                "A node serves the same routes over its own store, which in a cluster is the \
                 cluster's, and filters nothing by team: it answers every team's events to \
                 whoever reaches it with the cluster token."
            } else {
                "A team's key sees the events of its team's sandboxes only; an administrator \
                 or a key with no team sees every team's. The key needs the `events` scope."
            };
            format!(
                "{} are E2B's `SandboxEvent`, newest first: lifecycle events only (created, \
                 paused, resumed, updated, killed). Each carries the sandbox's metadata as it \
                 was, and an end or pause carries how long it ran and on what; the control \
                 plane's `node_id` and any `detail` are added. {scope} They are read from \
                 the cluster's event log, whose newest 10,000 are all that exist, filtered \
                 afterwards, so a quiet team may see fewer than the limit. Events of \
                 machines are webhook events and are not here.{}",
                if one {
                    "The events of one sandbox"
                } else {
                    "Every sandbox's events"
                },
                if one && !node {
                    " A caller limited to a team or an owner needs the sandbox to be its own \
                     (403 otherwise, 404 if it does not exist); one that sees every team gets \
                     an empty list for an unknown ID."
                } else {
                    ""
                }
            )
        };
        let event_access = |named: bool| {
            if node {
                Self::sandbox_access_responses(true, false)
            } else {
                let mut responses = control_plane_gate("events", named);
                responses.push(fail(503, "`the cluster store is unavailable`"));
                responses
            }
        };
        let mut list = Spec::new(
            "list_sandbox_events",
            "List Sandbox Lifecycle Events",
            "GET",
            "/events/sandboxes",
        )
        .about(events_about(false));
        for parameter in event_query() {
            list = list.with(parameter);
        }
        let mut one = Spec::new(
            "get_sandbox_events",
            "List One Sandbox's Lifecycle Events",
            "GET",
            "/events/sandboxes/{id}",
        )
        .about(events_about(true))
        .with(path_param(
            "id",
            "The sandbox's ID",
            json!({ "type": "string" }),
        ));
        for parameter in event_query() {
            one = one.with(parameter);
        }
        specs.push(
            list.answers(ok(
                200,
                "The events.",
                Some(json!({ "type": "array", "items": event_schema() })),
            ))
            .answers(bad_query())
            .answers_all(event_access(false))
            .repeatable(),
        );
        specs.push(
            one.answers(ok(
                200,
                "The events of that sandbox.",
                Some(json!({ "type": "array", "items": event_schema() })),
            ))
            .answers(bad_query())
            .answers_all(event_access(true))
            .repeatable(),
        );
        let mut operations: Vec<Operation> = specs
            .into_iter()
            .map(|spec| {
                let params = match spec.id() {
                    "get_sandbox_metrics" => range(),
                    "get_sandbox_logs" | "get_sandbox_logs_v2" => log_parameters(),
                    _ => Vec::new(),
                };
                params.into_iter().fold(spec, Spec::with).build(node)
            })
            .collect();
        operations.extend(Self::registration_operations(node));
        operations
    }

    /// Finding and finishing a registration left uncertain. The control
    /// plane's routes name the node; the node's are the routes they call.
    fn registration_operations(node: bool) -> Vec<Operation> {
        let page = || {
            json!({ "type": "object", "properties": {
                "registrations": { "type": "array", "maxItems": 32, "items": { "type": "object", "properties": {
                    "sandboxID": { "type": "string" },
                    "kind": { "type": "string", "enum": ["named", "unnamed"] } } } },
                "nextCursor": { "type": ["string", "null"], "description": "pass as `after` for the next page; null on the last" } } })
        };
        let after = || {
            query_param(
                "after",
                "The `nextCursor` of the previous page: a sandbox ID",
                json!({ "type": "string" }),
            )
        };
        let (pending_path, pending_id, reconcile_path, reconcile_id) = if node {
            (
                "/registrations/pending",
                "node_list_pending_registrations",
                "/sandboxes/{sandboxID}/registration/reconcile",
                "node_reconcile_sandbox_registration",
            )
        } else {
            (
                "/cluster/nodes/{node}/registrations/pending",
                "list_node_pending_registrations",
                "/cluster/nodes/{node}/sandboxes/{id}/registration/reconcile",
                "reconcile_node_sandbox_registration",
            )
        };
        let node_param = || {
            path_param(
                "node",
                "A registered node's ID",
                json!({ "type": "string" }),
            )
        };
        let sandbox_param = || Self::sandbox_id_parameter(node);
        let admin_gate = || {
            vec![
                fail(401, "missing, expired or wrong X-API-Key or session"),
                fail(
                    403,
                    "`administrator credential required`: only the `admin` scope reaches \
                     the cluster routes, and an open control plane (--allow-unauthenticated) \
                     is not an administrator here",
                ),
                fail(
                    503,
                    "`authenticated cluster required` (the control plane has no cluster \
                     token), or `node inventory unavailable`",
                ),
                fail(404, "`no registered node`"),
                fail(
                    502,
                    "`node discovery unavailable` or `node reconciliation unavailable`: the \
                     node could not be reached",
                ),
            ]
        };
        let node_gate = || {
            vec![
                fail(
                    401,
                    "`registration reconciliation requires cluster authorization`: the \
                     `x-hv2-cluster-token` header is missing or wrong",
                ),
                fail(
                    503,
                    "`registration reconciliation needs a cluster node` or `needs cluster \
                     authentication`: a node outside a cluster, or with no token, refuses \
                     these routes even when it answers every other",
                ),
            ]
        };
        let gate = || if node { node_gate() } else { admin_gate() };

        let mut pending = Spec::new(
            pending_id.trim_start_matches("node_"),
            "List Registrations Left Uncertain",
            "GET",
            pending_path,
        )
        .about(if node {
            "The sandboxes on this node whose record or lifecycle event could not be \
             published to the cluster store, so the guest runs locally but its registration \
             is pending: their IDs and whether the creation was `named` or `unnamed`, sorted \
             by ID, 32 to a page. A page holds no access token. The state is the process's \
             own and is lost when the node restarts. A pending entry can also be a \
             publication still in progress. Needs the cluster token, and a node that is part \
             of a cluster."
        } else {
            "Ask a registered node which of its sandboxes are pending registration (see \
             `node_list_pending_registrations`), through the control plane, which relays \
             with its cluster token and refuses a page that is not exactly the shape \
             above (502 `invalid discovery response`; also over 16 KiB). Administrator \
             only. An unknown node is 404."
        })
        .with(after())
        .answers(ok(200, "A page of registrations.", Some(page())))
        .answers(fail(
            400,
            "`invalid registration cursor` (an `after` that is not a sandbox ID, or an \
             unknown query parameter)",
        ))
        .answers_all(gate())
        .repeatable();
        if !node {
            pending = pending.with(node_param());
        }

        let mut reconcile = Spec::new(
            reconcile_id.trim_start_matches("node_"),
            "Finish A Sandbox's Uncertain Registration",
            "POST",
            reconcile_path,
        )
        .about(if node {
            "Publish again the record and event of a sandbox whose registration is pending, \
             under the sandbox's lifecycle lock, from the trusted ownership kept locally. \
             On success the pending mark is cleared and the answer is the sandbox's \
             descriptor, which carries its `envdAccessToken`: treat it as a credential. \
             A retry may duplicate a lifecycle event; delivery is not exactly once. It \
             repairs a guest preserved on this node, not one lost with a crashed node. A \
             node may do this by itself with `--registration-reconcile-interval`. Needs \
             the cluster token."
        } else {
            "Ask the named node to finish a sandbox's pending registration, after the \
             store outage or permission problem that left it is fixed. The node must \
             still be registered and the guest still running there. The answer is the \
             sandbox's descriptor, rewritten to the control plane's proxy and with the node's \
             name: it carries the `envdAccessToken`, so treat it as a credential; one whose \
             `sandboxID` differs from the path is refused (502). It works when the \
             first record write failed and the control plane's inventory has no entry. If \
             a reply is lost, list the pending registrations before retrying. \
             Administrator only."
        })
        .with(sandbox_param())
        .answers(ok(
            200,
            "The sandbox's descriptor.",
            Some(Self::sandbox_descriptor_schema(node)),
        ))
        .answers(fail(
            400,
            if node {
                "an unreadable path"
            } else {
                "`invalid sandbox ID`"
            },
        ))
        .answers(fail(
            404,
            if node {
                "`no local running sandbox`"
            } else {
                "`no registered node`; or the node's own 404 (no local running sandbox), which \
                 arrives as `node reconciliation refused or remains uncertain`"
            },
        ))
        .answers(fail(
            409,
            "`no uncertain local registration` (nothing pending, or already reconciled), or \
             `registration reconciliation ownership refused`; through the control plane any \
             refusal of the node (401, 404, 409, 503) keeps its status and arrives as \
             `node reconciliation refused or remains uncertain`",
        ))
        .answers(fail(
            503,
            "`registration outcome still requires reconciliation`: the store still fails; \
             the guest and its pending mark are kept",
        ))
        .answers_all(gate());
        if !node {
            reconcile = reconcile.with(node_param()).answers(fail(
                502,
                "`node reconciliation unavailable`, or the node answered with something that                  is not that sandbox's descriptor (`invalid reconciliation response`, over                  16 KiB, or a different sandbox)",
            ));
        }
        vec![pending.build(node), reconcile.build(node)]
    }
}
