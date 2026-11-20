//! The control plane's and the node's platform routes: the environments a
//! team starts sandboxes with, event webhooks and their delivery records, the
//! usage report, a team's own telemetry destination, the cluster's nodes and
//! event log, the workload-identity discovery documents, the Prometheus
//! exposition, the operator page and a node's warm pool. Modelled as data:
//! `hv2-api` depends on none of the crates that serve them.
//!
//! The environments, webhooks and usage routes are one router
//! (`hv2-cluster` `environments.rs`, `events.rs`) that both the control plane
//! (`control.rs`, behind its key check) and the node (`hv2-sandboxd`
//! `main.rs`, behind its cluster-token check) mount. They are written once
//! and built twice, as the other shared operations are: `platform_operations(false)`
//! for the control plane and `platform_operations(true)` for the node, whose
//! ids start `node_`.

use super::spec::{body_rejection, fail, ok, path_param, plain, query_param, Spec};
use super::{
    Capability, CapabilityCategory, Guardrail, GuardrailSeverity, HyperMachineOntology, Operation,
    OperationResponse,
};
use serde_json::{json, Value};

/// What can refuse a request to one of these routes before its handler runs.
/// On the control plane that is the key or session check and the scope the
/// route needs; on a node, the cluster token. `scope` is `admin` for the
/// routes no other scope reaches, else the scope's name.
fn gate(node: bool, scope: &str) -> Vec<OperationResponse> {
    let mut responses = if node {
        HyperMachineOntology::sandbox_access_responses(true, false)
    } else if scope == "admin" {
        vec![
            fail(
                401,
                "missing, expired or wrong X-API-Key or session; with single sign-on configured \
                 an anonymous request is refused too",
            ),
            fail(
                403,
                "the key or member does not hold the `admin` scope (`API key scope does not \
                 permit this operation`, or `this member's scopes do not permit this \
                 operation` for a signed-in member): no other scope reaches this route, and \
                 an observer or viewer may read only the inventory routes. The legacy \
                 --api-key key and a control plane started with --allow-unauthenticated pass. \
                 A session-cookie change that did not come from the control plane's own pages \
                 is 403 too",
            ),
        ]
    } else {
        vec![
            fail(
                401,
                "missing, expired or wrong X-API-Key or session; with single sign-on configured \
                 an anonymous request is refused too",
            ),
            fail(
                403,
                &format!(
                    "the key or member lacks the `{scope}` scope (and is not an administrator), \
                     is an observer or viewer (those may read the inventory routes only), or a \
                     session-cookie change did not come from the control plane's own pages"
                ),
            ),
        ]
    };
    responses.push(fail(
        503,
        "`the cluster store is unavailable`: the store could not be read or written (the \
         detail is logged, not returned)",
    ));
    responses
}

/// The sentence every shared operation uses to say who answers it and what
/// the only check in front of it is.
fn served_by(node: bool, scope: &str) -> String {
    if node {
        "A node serves this route too, from its event store: the cluster store when it is a \
         cluster node, otherwise a store in its own memory that a restart empties. The check in \
         front of it is the cluster token (`x-hv2-cluster-token`) when the node has one, and \
         nothing when it has none. The node sets no team on a request, so it works in the \
         scope that has no team and filters nothing by team."
            .to_string()
    } else if scope == "admin" {
        "The control plane serves it itself, from the cluster store. Its key check lets only an \
         administrator reach it (an `admin`-scope key or member, the legacy --api-key key, or \
         a control plane started with --allow-unauthenticated): the scope table in \
         `ApiKeyPolicy::permits` names no other scope for it, so a team's key gets 403. An \
         administrator has no team, so it works in the scope that has no team."
            .to_string()
    } else {
        format!(
            "The control plane serves it itself, from the cluster store, for a key or member \
             holding the `{scope}` scope (or `admin`). A key with a team works in its team's \
             scope, a key with a principal and no team in a scope of its own, and an \
             administrator, which has no team, in the scope that has no team."
        )
    }
}

fn environment_schema() -> Value {
    json!({ "type": "object", "properties": {
        "name": { "type": "string" },
        "teamID": { "type": ["string", "null"], "description": "null in the scope that has no team" },
        "envVarNames": { "type": "array", "items": { "type": "string" }, "description": "names only, never a value" },
        "files": { "type": "array", "items": { "type": "object", "properties": {
            "path": { "type": "string" },
            "bytes": { "type": "integer", "description": "the file's size; never its contents" } } } },
        "repositories": { "type": "array", "items": { "type": "object", "properties": {
            "remote": { "type": "string" },
            "path": { "type": "string" },
            "branch": { "type": ["string", "null"] },
            "username": { "type": ["string", "null"], "description": "never the password" } } } },
        "createdAt": { "type": "string", "format": "date-time" },
        "updatedAt": { "type": "string", "format": "date-time" }
    } })
}

fn environment_body() -> Value {
    json!({ "type": "object", "additionalProperties": false, "properties": {
        "envVars": { "type": "object", "additionalProperties": { "type": "string" },
            "description": "at most 1,024 names (letters, digits and `_`, not starting with a digit) whose names and values together are at most 128 KiB; no value holds a NUL" },
        "files": { "type": "array", "maxItems": 64, "items": { "type": "object", "additionalProperties": false,
            "required": ["path"], "properties": {
            "path": { "type": "string", "description": "absolute, naming a file, no `..`, no NUL, at most 4096 bytes, each path once" },
            "content": { "type": "string", "description": "text; give this or `contentBase64`, not both" },
            "contentBase64": { "type": "string", "description": "standard base64" } } },
            "description": "at most 4 MiB of contents together" },
        "repositories": { "type": "array", "maxItems": 8, "items": { "type": "object", "additionalProperties": false,
            "required": ["remote", "path"], "properties": {
            "kind": { "type": "string", "enum": ["lit", "git"], "default": "lit" },
            "remote": { "type": "string", "description": "an http, https or lit URL with no credentials in it" },
            "path": { "type": "string", "description": "absolute, not `/`, no `..`, distinct from every file's and repository's" },
            "branch": { "type": "string", "description": "a plain name: printable ASCII, no quotes or backslash, no `..` or empty part, not starting with `-` or `/`" },
            "username": { "type": "string" },
            "password": { "type": "string", "description": "kept, never returned; a `git` repository takes neither username nor password, and a `lit` one both or neither" } } } }
    } })
}

fn webhook_schema() -> Value {
    json!({ "type": "object", "properties": {
        "id": { "type": "string" },
        "teamId": { "type": "string", "description": "always the all-zero UUID E2B's schema asks for; it is not the webhook's real team" },
        "name": { "type": "string" },
        "createdAt": { "type": "string", "format": "date-time" },
        "url": { "type": "string" },
        "enabled": { "type": "boolean" },
        "events": { "type": "array", "items": { "type": "string" }, "description": "empty means every sandbox event, and no machine event" }
    } })
}

fn attempt_schema() -> Value {
    json!({ "type": "object", "properties": {
        "id": { "type": "string" },
        "teamId": { "type": "string" },
        "webhookId": { "type": "string" },
        "eventId": { "type": "string" },
        "sandboxId": { "type": "string", "description": "a machine's name, for a machine event" },
        "eventType": { "type": "string" },
        "status": { "type": "string", "enum": ["success", "failed"] },
        "durationMs": { "type": "integer" },
        "requestBody": { "type": "string" },
        "requestHeaders": { "type": "string", "description": "a fixed string with the signature shown as `[redacted]`" },
        "requestUrl": { "type": "string" },
        "responseBody": { "type": ["string", "null"], "description": "the first 2,048 characters" },
        "responseHeaders": { "type": "null" },
        "responseHttpStatusCode": { "type": ["integer", "null"] },
        "errorClass": { "type": ["string", "null"], "enum": ["http_error", "dns_error", "timeout", "transport_error", "request_error", null] },
        "errorMessage": { "type": ["string", "null"] },
        "timestamp": { "type": "string", "format": "date-time" }
    } })
}

fn duration_schema() -> Value {
    json!({ "type": "object", "properties": {
        "minimum": { "type": "number" }, "average": { "type": "number" }, "maximum": { "type": "number" } } })
}

fn webhook_id() -> super::Parameter {
    path_param(
        "id",
        "The webhook's ID, as `create_sandbox_webhook` returned it",
        json!({ "type": "string" }),
    )
}

fn no_such_webhook() -> OperationResponse {
    fail(
        404,
        "`no webhook <id>`: there is none of that ID, or it belongs to another team (the two \
         look the same)",
    )
}

fn bad_query() -> OperationResponse {
    plain(
        400,
        "a query parameter is not what the route reads (the framework's own rejection, plain \
         text)",
    )
}

impl HyperMachineOntology {
    /// The platform routes of the control plane (`node` false) or the node.
    pub(super) fn platform_operations(node: bool) -> Vec<Operation> {
        let mut specs = Self::environment_specs(node);
        specs.extend(Self::webhook_specs(node));
        specs.extend(Self::usage_and_discovery_specs(node));
        let mut operations: Vec<Operation> = specs.into_iter().map(|s| s.build(node)).collect();
        if node {
            operations.extend(Self::node_only_platform_operations());
        } else {
            operations.extend(Self::control_plane_only_platform_operations());
        }
        operations
    }

    fn environment_specs(node: bool) -> Vec<Spec> {
        let name = || {
            path_param(
                "name",
                "The environment's name: 1 to 64 letters, digits, `-`, `_` and `.`, not \
                 starting with a dot. One name per team",
                json!({ "type": "string", "maxLength": 64 }),
            )
        };
        let served = |scope: &str| served_by(node, scope);
        vec![
            Spec::new(
                "list_environments",
                "List Environments",
                "GET",
                "/environments",
            )
            .about(format!(
                "List the environments of the caller's scope: each as its name, the names of \
                 its variables, its files' paths and sizes and its repositories' remotes, \
                 paths, branches and user names. No value, no file content and no password is \
                 ever returned. Only environments of exactly the caller's scope are listed: a \
                 caller with no team sees those with no team, not every team's. The list is \
                 the whole of it, with no paging. An environment is what a sandbox created \
                 with `environment: NAME` starts with (see `create_sandbox`). {}",
                served("admin")
            ))
            .answers(ok(
                200,
                "The environments (an empty array when there are none).",
                Some(json!({ "type": "array", "items": environment_schema() })),
            ))
            .answers_all(gate(node, "admin"))
            .repeatable(),
            Spec::new(
                "get_environment",
                "Read An Environment",
                "GET",
                "/environments/{name}",
            )
            .about(format!(
                "Read one environment of the caller's scope, as `list_environments` describes \
                 it: the names of its variables, the paths and sizes of its files, its \
                 repositories without their passwords, and when it was made and last \
                 replaced. Never a value or a file's content. {}",
                served("admin")
            ))
            .with(name())
            .answers(ok(200, "The environment.", Some(environment_schema())))
            .answers(fail(
                404,
                "`no environment <name>` in the caller's scope (another scope's of the same \
                 name is not seen)",
            ))
            .answers_all(gate(node, "admin"))
            .repeatable(),
            Spec::new(
                "put_environment",
                "Make Or Replace An Environment",
                "PUT",
                "/environments/{name}",
            )
            .about(format!(
                "Make an environment, or replace the one of that name whole: what the body \
                 leaves out is gone, and `createdAt` is kept. The body holds `envVars`, \
                 `files` (each with a `path` and one of `content` or `contentBase64`) and \
                 `repositories` (a Lit or Git remote the node clones into the guest at \
                 creation); an unknown field is refused. Values, contents and repository \
                 passwords are stored as given, unencrypted, in the cluster store, and are \
                 never returned (see the guardrails). The limits are 1,024 variables and \
                 128 KiB of them, 64 files and 4 MiB of them, 8 repositories, and 256 \
                 environments in a scope; the body may be about 8.25 MiB. The URL of a \
                 repository is checked for its shape here, not against the node's allowed \
                 remotes: a remote the node does not allow is refused (403) when a sandbox is \
                 created with the environment, not now. A change reaches sandboxes created \
                 after it; one already running keeps what it started with. Replacing with \
                 the same body is harmless. {}",
                served("admin")
            ))
            .with(name())
            .takes(environment_body(), true)
            .answers(ok(
                201,
                "Made: the environment as `get_environment` describes it.",
                Some(environment_schema()),
            ))
            .answers(ok(
                200,
                "Replaced: the new environment as `get_environment` describes it.",
                Some(environment_schema()),
            ))
            .answers(fail(
                400,
                "the name is not a valid one, or the body is refused: a variable name that is \
                 not letters, digits and `_` (or starts with a digit), a value with a NUL, \
                 too many or too large variables, files or repositories, a file path that is \
                 not absolute, ends in `/`, has `..` or is given twice, a file with both or \
                 neither of `content` and `contentBase64`, base64 that does not decode, a \
                 repository remote that is not an http, https or lit URL without credentials, \
                 a repository path that is relative, `/`, has `..` or collides with another, \
                 a branch that is not a plain name, a `git` repository with a username or \
                 password, or a Lit one with only one of the two",
            ))
            .answers(fail(
                409,
                "`a team keeps at most 256 environments`: this would be the 257th in the \
                 scope",
            ))
            .answers(body_rejection())
            .answers(plain(
                413,
                "the body is over the route's limit (about 8.25 MiB), the framework's own \
                 rejection",
            ))
            .answers_all(gate(node, "admin"))
            .repeatable(),
            Spec::new(
                "delete_environment",
                "Delete An Environment",
                "DELETE",
                "/environments/{name}",
            )
            .about(format!(
                "Remove the environment of that name from the caller's scope. Sandboxes \
                 already running keep what they started with; a create naming it afterwards \
                 is 404. Deleting it a second time is 404. {}",
                served("admin")
            ))
            .with(name())
            .answers(plain(204, "Removed."))
            .answers(fail(404, "`no environment <name>` in the caller's scope"))
            .answers_all(gate(node, "admin")),
        ]
    }

    fn webhook_specs(node: bool) -> Vec<Spec> {
        let served = || served_by(node, "events");
        let delivery_note = if node {
            "A node delivers the events it emitted itself. A node outside a cluster keeps its \
             webhooks and delivery records in memory, so a restart forgets them. Its \
             deliveries go to a private, loopback or link-local address only when the node \
             was started with --allow-private-webhooks."
        } else {
            "The control plane stores the webhook; it does not deliver. The node that emitted \
             an event delivers it, so each event is sent once however many control planes \
             there are, and a node sends to a private, loopback or link-local address only \
             when it was started with --allow-private-webhooks."
        };
        let scope_note = if node {
            "A node filters nothing by team: it lists, reads and deletes every webhook in the \
             store, and one made on it has no team, so it receives every team's events."
        } else {
            "A key with a team sees and changes only its team's webhooks (another team's is \
             404); an administrator, which has no team, sees every webhook and its own \
             receive every team's events."
        };
        vec![
            Spec::new(
                "list_sandbox_webhooks",
                "List Event Webhooks",
                "GET",
                "/events/webhooks",
            )
            .about(format!(
                "List the webhooks of the cluster's event stream that the caller may see, in \
                 E2B's shape (no secret is ever returned, old or new). {scope_note} Webhooks \
                 are for `sandbox.lifecycle.*` and `machine.lifecycle.*` events. {}",
                served()
            ))
            .answers(ok(
                200,
                "The webhooks.",
                Some(json!({ "type": "array", "items": webhook_schema() })),
            ))
            .answers_all(gate(node, "events"))
            .repeatable(),
            Spec::new(
                "create_sandbox_webhook",
                "Register An Event Webhook",
                "POST",
                "/events/webhooks",
            )
            .about(format!(
                "Register a URL to receive the cluster's sandbox and machine lifecycle events \
                 as signed JSON POSTs. `name`, `url` and `signatureSecret` (at least 16 \
                 characters) are required; `events` lists the types wanted (every one \
                 starts `sandbox.lifecycle.` or `machine.lifecycle.`; empty means every \
                 sandbox event, and a machine event is sent only to a webhook that names it); \
                 `enabled` defaults to true. The URL must be http or https with a host, but \
                 is not resolved or checked against an address list now: a destination that \
                 is not a global address is refused when an event is delivered, and that \
                 shows as a failed delivery. Each delivery carries `hv2-signature` (an HMAC \
                 over `<timestamp>.<body>`, usable to verify) and E2B's `e2b-signature` (not \
                 an HMAC). A scope may hold 100 webhooks. {scope_note} {delivery_note} {}",
                served()
            ))
            .takes(
                json!({ "type": "object", "required": ["name", "url", "signatureSecret"], "properties": {
                    "name": { "type": "string" },
                    "url": { "type": "string", "description": "http or https, with a host" },
                    "events": { "type": "array", "items": { "type": "string" } },
                    "enabled": { "type": "boolean", "default": true },
                    "signatureSecret": { "type": "string", "minLength": 16, "description": "never returned" } } }),
                true,
            )
            .answers(ok(201, "Registered.", Some(webhook_schema())))
            .answers(fail(
                400,
                "the URL is not http or https with a host, `signatureSecret` is under 16 \
                 characters, or an event type does not start `sandbox.lifecycle.` or \
                 `machine.lifecycle.`",
            ))
            .answers(fail(
                429,
                "`at most 100 webhooks per team; delete one first`: the caller's scope already \
                 holds 100 (an administrator's scope counts the webhooks that have no team)",
            ))
            .answers(body_rejection())
            .answers_all(gate(node, "events")),
            Spec::new(
                "get_sandbox_webhook",
                "Read An Event Webhook",
                "GET",
                "/events/webhooks/{id}",
            )
            .about(format!(
                "Read one webhook: its name, URL, events and whether it is enabled. The \
                 secret is never returned. {scope_note} {}",
                served()
            ))
            .with(webhook_id())
            .answers(ok(200, "The webhook.", Some(webhook_schema())))
            .answers(no_such_webhook())
            .answers_all(gate(node, "events"))
            .repeatable(),
            Spec::new(
                "update_sandbox_webhook",
                "Change An Event Webhook",
                "PATCH",
                "/events/webhooks/{id}",
            )
            .about(format!(
                "Change any of a webhook's `enabled`, `name`, `url`, `events` and \
                 `signatureSecret`; a field left out is kept, and a field the route does not \
                 know is ignored. A new `signatureSecret` (at least 16 characters, and \
                 different from the current one) rotates the secret: the old one keeps \
                 signing `hv2-signature` beside the new one for `overlapSeconds` (a day when \
                 absent, 0 for none, a week at most), so a receiver can switch without \
                 losing a delivery; a second rotation inside the window replaces the older \
                 secret. `e2b-signature` always uses the current secret. The URL and events \
                 are checked as `create_sandbox_webhook` checks them. {scope_note} {}",
                served()
            ))
            .with(webhook_id())
            .takes(
                json!({ "type": "object", "properties": {
                    "enabled": { "type": "boolean" },
                    "name": { "type": "string" },
                    "url": { "type": "string" },
                    "events": { "type": "array", "items": { "type": "string" } },
                    "signatureSecret": { "type": "string", "minLength": 16 },
                    "overlapSeconds": { "type": "integer", "minimum": 0, "maximum": 604800,
                        "description": "how long the replaced secret keeps signing; only with a new secret" } } }),
                true,
            )
            .answers(ok(200, "The webhook as changed.", Some(webhook_schema())))
            .answers(fail(
                400,
                "the URL is not http or https with a host, an event type is not a lifecycle \
                 type, `signatureSecret` is under 16 characters, or `overlapSeconds` is over a \
                 week (604,800)",
            ))
            .answers(no_such_webhook())
            .answers(body_rejection())
            .answers_all(gate(node, "events")),
            Spec::new(
                "delete_sandbox_webhook",
                "Delete An Event Webhook",
                "DELETE",
                "/events/webhooks/{id}",
            )
            .about(format!(
                "Delete a webhook. Events not yet delivered to it are not sent; deliveries \
                 already under way may finish. {scope_note} {}",
                served()
            ))
            .with(webhook_id())
            .answers(plain(204, "Deleted."))
            .answers(no_such_webhook())
            .answers_all(gate(node, "events")),
            Spec::new(
                "list_webhook_deliveries",
                "List A Webhook's Deliveries",
                "GET",
                "/events/webhooks/{id}/deliveries",
            )
            .about(format!(
                "List the delivery attempts of a webhook, newest first, grouped by event: each \
                 group is one event with its `attempts`. An event is tried up to three times \
                 (at once, after 1 second and after 4 more) unless the address is refused \
                 (`request_error`), which is not retried; any 2xx is a success, and a \
                 redirect is not followed. Each attempt shows the request body and URL, the \
                 response status and the first 2,048 characters of the response, the error \
                 class and message, and the time; the signature header is shown as \
                 `[redacted]`. Only the last 1,000 attempts of a webhook are kept. \
                 `limit` caps the attempts read (100 by default, 1,000 at most), so the \
                 last group may be short. {}",
                served()
            ))
            .with(webhook_id())
            .with(query_param(
                "limit",
                "At most this many attempts, newest first (default 100, most 1000)",
                json!({ "type": "integer", "minimum": 0, "maximum": 1000, "default": 100 }),
            ))
            .answers(ok(
                200,
                "The events, each with its attempts.",
                Some(json!({ "type": "array", "items": { "type": "object", "properties": {
                    "eventId": { "type": "string" },
                    "eventType": { "type": "string" },
                    "sandboxId": { "type": "string" },
                    "attempts": { "type": "array", "items": attempt_schema() } } } })),
            ))
            .answers(bad_query())
            .answers(no_such_webhook())
            .answers_all(gate(node, "events"))
            .repeatable(),
            Spec::new(
                "get_webhook_stats",
                "Read A Webhook's Delivery Statistics",
                "GET",
                "/events/webhooks/{id}/stats",
            )
            .about(format!(
                "Summarise a webhook's kept delivery attempts (the last 1,000): the total, \
                 how many did not succeed, and the minimum, average and maximum duration in \
                 milliseconds, overall and for each hour in which an attempt was made. Every \
                 attempt counts, so an event retried twice is three. {}",
                served()
            ))
            .with(webhook_id())
            .answers(ok(
                200,
                "The statistics.",
                Some(json!({ "type": "object", "properties": {
                    "buckets": { "type": "array", "items": { "type": "object", "properties": {
                        "timestamp": { "type": "string", "format": "date-time", "description": "the start of the hour" },
                        "total": { "type": "integer" },
                        "failed": { "type": "integer" },
                        "durationMs": duration_schema() } } },
                    "total": { "type": "integer" },
                    "failed": { "type": "integer" },
                    "durationMs": duration_schema() } })),
            ))
            .answers(no_such_webhook())
            .answers_all(gate(node, "events"))
            .repeatable(),
        ]
    }

    fn usage_and_discovery_specs(node: bool) -> Vec<Spec> {
        let usage_scope = if node {
            "A node reports from the store it keeps its events and its usage ledger in: the \
             cluster's when it is a cluster node, otherwise its own memory, which a restart \
             empties. It filters by no team, so it counts every sandbox in that store."
        } else {
            "A key with a team is limited to its team's sandboxes and a key with a principal \
             and no team to its own; an administrator sees every sandbox. The administrator \
             restriction in front of the route is what the access table says (see the \
             guardrails): no scope other than `admin` reaches it."
        };
        let discovery_who = if node {
            "A node serves this about itself, and its key alone: whichever workload-identity \
             key it signs with, which in a cluster with a shared snapshot store is the one \
             every node shares. It is outside the node's cluster-token check, so anyone who \
             can reach the node's port can read it."
        } else {
            "The control plane serves this without an API key or session, as an OIDC issuer \
             does, and so it is outside the key check and single sign-on."
        };
        vec![
            Spec::new(
                "get_usage",
                "Report Sandbox Usage In A Window",
                "GET",
                "/usage",
            )
            .about(format!(
                "How long sandboxes ran in a window, and with how much: the number of \
                 sandboxes, running seconds, vCPU-seconds and memory GiB-seconds, in total \
                 and in groups. Running time is counted and paused time is not; a sandbox \
                 still running counts to the moment of asking, so two calls give two numbers. \
                 The figures come from the usage ledger (kept for good, in the cluster store \
                 for a cluster) joined to the last 10,000 events for the period before the \
                 ledger began; `coveredFrom` is the earliest either knows, `truncated` says \
                 events from before the ledger were dropped, and a group is `partial` when a \
                 sandbox's start was not seen. {usage_scope} See docs/USAGE.md. {}",
                served_by(node, "admin")
            ))
            .with(query_param(
                "from",
                "Start of the window: RFC 3339 or milliseconds since the epoch; a day before `to` when absent",
                json!({ "type": "string" }),
            ))
            .with(query_param(
                "to",
                "End of the window: RFC 3339 or milliseconds since the epoch; now when absent",
                json!({ "type": "string" }),
            ))
            .with(query_param(
                "by",
                "How to group: `sandbox` (the default), `team`, `template` or `owner`",
                json!({ "type": "string", "enum": ["sandbox", "team", "template", "owner"], "default": "sandbox" }),
            ))
            .answers(ok(
                200,
                "The report.",
                Some(json!({ "type": "object", "properties": {
                    "from": { "type": "string", "format": "date-time" },
                    "to": { "type": "string", "format": "date-time" },
                    "by": { "type": "string" },
                    "coveredFrom": { "type": ["string", "null"], "format": "date-time" },
                    "truncated": { "type": "boolean" },
                    "total": { "type": "object", "properties": {
                        "sandboxes": { "type": "integer" },
                        "runningSeconds": { "type": "number" },
                        "vcpuSeconds": { "type": "number" },
                        "memoryGiBSeconds": { "type": "number" },
                        "partial": { "type": "boolean" } } },
                    "groups": { "type": "array", "items": { "type": "object", "properties": {
                        "key": { "type": ["string", "null"], "description": "null for a sandbox with no team, template or owner" },
                        "sandboxes": { "type": "integer" },
                        "runningSeconds": { "type": "number" },
                        "vcpuSeconds": { "type": "number" },
                        "memoryGiBSeconds": { "type": "number" },
                        "partial": { "type": "boolean" } } } } } })),
            ))
            .answers(fail(
                400,
                "`to: ...` or `from: ...`: not RFC 3339 or milliseconds, or before 1970; or \
                 `from is after to`",
            ))
            .answers(plain(
                400,
                "`by` is not one of the four words (the framework's own rejection, plain text)",
            ))
            .answers_all(gate(node, "admin"))
            .repeatable(),
            Spec::new(
                "get_identity_jwks",
                "Read The Workload-Identity Signing Keys",
                "GET",
                "/.well-known/jwks.json",
            )
            .about(format!(
                "The public keys (a JWK set, ES256 over P-256) that verify the JWT-SVID tokens \
                 the egress gateway mints for a sandbox's `iam` tokens, which is what AWS STS \
                 `AssumeRoleWithWebIdentity` or GCP workload identity federation fetch from \
                 an issuer. {}{}",
                if node {
                    "The set is this node's key, and is empty (`{\"keys\": []}`) when the node has no \
                     identity key. "
                } else {
                    "The set is every live node's key once each (nodes sharing a key publish \
                     one `kid`), so it is empty when no node has registered one. "
                },
                discovery_who
            ))
            .answers(ok(
                200,
                "The key set.",
                Some(json!({ "type": "object", "properties": {
                    "keys": { "type": "array", "items": { "type": "object", "properties": {
                        "kty": { "type": "string" }, "crv": { "type": "string" },
                        "x": { "type": "string" }, "y": { "type": "string" },
                        "use": { "type": "string" }, "alg": { "type": "string" },
                        "kid": { "type": "string" } } } } } })),
            ))
            .answers_all(if node {
                Vec::new()
            } else {
                vec![fail(503, "`the cluster store is unavailable`")]
            })
            .repeatable(),
            Spec::new(
                "get_identity_openid_configuration",
                "Read The OIDC Discovery Document",
                "GET",
                "/.well-known/openid-configuration",
            )
            .about(format!(
                "The OpenID Connect discovery document of the workload-identity issuer: the \
                 `issuer` as configured with --identity-issuer, its `jwks_uri` (the issuer \
                 URL with `/.well-known/jwks.json`) and the one signing algorithm, ES256. \
                 Without --identity-issuer it is 404: that process is not an issuer. {}",
                discovery_who
            ))
            .answers(ok(
                200,
                "The discovery document.",
                Some(json!({ "type": "object", "properties": {
                    "issuer": { "type": "string" },
                    "jwks_uri": { "type": "string" },
                    "response_types_supported": { "type": "array", "items": { "type": "string" } },
                    "subject_types_supported": { "type": "array", "items": { "type": "string" } },
                    "id_token_signing_alg_values_supported": { "type": "array", "items": { "type": "string" } } } })),
            ))
            .answers(fail(
                404,
                "`no --identity-issuer`: this process is not an OIDC issuer",
            ))
            .repeatable(),
            Spec::new(
                "get_metrics_exposition",
                "Read Prometheus Metrics",
                "GET",
                "/metrics",
            )
            .about(if node {
                "The node's metrics as Prometheus text (`text/plain; version=0.0.4`): \
                 sandboxes running, paused and booting, capacity, the template count, creates \
                 by result (`ok`, `full`, `rejected`, `error`), sandbox ends by reason, \
                 transitions by kind and the create, pause, resume and checkpoint latency \
                 histograms (`hv2_node_*`), plus OTLP push counts when pushing is on. The \
                 same numbers are pushed over OTLP when --otlp-endpoint is set. The route is \
                 outside the cluster-token check, since a scraper holds no token, and counts \
                 are all it says: nothing per sandbox, team or key. See docs/TELEMETRY.md."
            } else {
                "The control plane's metrics as Prometheus text (`text/plain; version=0.0.4`): \
                 the cluster as the store sees it (`hv2_cluster_nodes`, \
                 `hv2_cluster_capacity`, `hv2_cluster_sandboxes`; left out when the store \
                 cannot be read, which does not fail the scrape) and this instance's own work \
                 (`hv2_control_creates_total{result}`, `hv2_control_create_seconds`, \
                 `hv2_control_reaped_total{reason}`). It is outside the key check, since a \
                 scraper holds no API key, and says counts only. A team's own numbers are \
                 not here: they are pushed (see `put_team_telemetry`). See docs/TELEMETRY.md."
            })
            .answers(ok(
                200,
                "The metrics, as Prometheus text exposition.",
                Some(json!({ "type": "string" })),
            ))
            .repeatable(),
        ]
    }

    fn node_only_platform_operations() -> Vec<Operation> {
        vec![
            Spec::new("get_warm_pool", "Read The Warm Pool", "GET", "/pool")
                .about(
                    "Report the node's warm pool: the `target` size set with --warm-pool (0 when \
             there is none), how many spares are `ready` now, how many creates took a spare \
             (`handedOut`) and how many would have but found the pool empty (`missed`). A \
             spare is a guest restored from the base template, awake once and put in standby; \
             only a plain create from the base template takes one. A rising `missed` says the \
             pool is too small for the rate of creates. The counts are since the node \
             started. The control plane does not relay this route. The only check is the \
             cluster token when the node has one. See docs/WARM_POOL.md.",
                )
                .answers(ok(
                    200,
                    "The pool's counts.",
                    Some(json!({ "type": "object", "properties": {
                "target": { "type": "integer" },
                "ready": { "type": "integer" },
                "handedOut": { "type": "integer" },
                "missed": { "type": "integer" } } })),
                ))
                .answers_all(HyperMachineOntology::sandbox_access_responses(true, false))
                .repeatable()
                .build(true),
        ]
    }

    fn control_plane_only_platform_operations() -> Vec<Operation> {
        let team_headers = json!({ "type": "object", "additionalProperties": { "type": "string" },
            "description": "at most 8; names are lower-cased, and the control plane's own (`host`, `content-length`, `content-type`, `transfer-encoding`, `connection`, `upgrade`, `te`, `trailer`, `expect`) are refused; a value is at most 2,048 bytes" });
        let telemetry_schema = json!({ "type": "object", "properties": {
            "endpoint": { "type": "string" },
            "headers": { "type": "array", "items": { "type": "string" }, "description": "names only, never a value" },
            "intervalSeconds": { "type": "integer" },
            "createdAt": { "type": "string", "format": "date-time" },
            "updatedAt": { "type": "string", "format": "date-time" },
            "lastPush": { "type": ["object", "null"], "description": "null until this control plane instance has pushed since the destination was stored", "properties": {
                "at": { "type": "string", "format": "date-time" },
                "ok": { "type": "boolean" },
                "error": { "type": ["string", "null"] } } } } });
        let telemetry_scope =
            "The control plane serves it itself, for a key or member holding the \
             `telemetry` scope (or `admin`); an observer or viewer is refused. A team has one \
             destination; a key with no team (an administrator's) has one of its own, for \
             the sandboxes with no team. Every control plane given --team-otlp-endpoint pushes, \
             so with several a collector is sent each team once per instance, told apart by \
             `service.instance.id`.";
        let no_destination = || fail(404, "`this team has no telemetry destination`");
        let mut specs = vec![
            Spec::new(
                "get_team_telemetry",
                "Read A Team's Telemetry Destination",
                "GET",
                "/telemetry",
            )
            .about(format!(
                "Read the OTLP/HTTP destination the control plane pushes the caller's team's \
                 numbers to: the endpoint, the *names* of its headers (never a value), the \
                 interval, when it was made and changed, and how the last push went (`lastPush`: \
                 when, whether it succeeded and the error). `lastPush` is kept in the memory \
                 of the instance that answers, so it is null until that instance has pushed, \
                 and another control plane may say something else. {telemetry_scope}"
            ))
            .answers(ok(200, "The destination.", Some(telemetry_schema.clone())))
            .answers(no_destination())
            .answers_all(gate(false, "telemetry"))
            .repeatable(),
            Spec::new(
                "put_team_telemetry",
                "Set A Team's Telemetry Destination",
                "PUT",
                "/telemetry",
            )
            .about(format!(
                "Set (or replace whole) the collector the control plane pushes the caller's \
                 team's numbers to over OTLP/HTTP with JSON: `hm_team_sandboxes_running`, \
                 `hm_team_sandboxes_paused`, `hm_team_vcpus_running`, \
                 `hm_team_memory_mib_running` and the rolling 24-hour \
                 `hm_team_running_seconds_24h`, `hm_team_vcpu_seconds_24h` and \
                 `hm_team_memory_gib_seconds_24h`, as gauges on a resource carrying the team. \
                 A destination whose URL ends in `/v1/metrics` is also sent the team's \
                 sandbox lifecycle events as OTLP logs (to `/v1/logs`, at most 500 a push, \
                 nothing from before it was stored) and a span for each sandbox that has \
                 ended (to `/v1/traces`), with the same headers. The control plane makes \
                 the request from inside the operator's network, so the endpoint is accepted \
                 only under a prefix the operator listed with --team-otlp-endpoint (the same \
                 scheme, host and port, and a path at or below it): with none listed the \
                 route is 403 and nothing is sent, and the check is made again at each push, \
                 so removing a prefix stops what was stored under it. No redirect is \
                 followed. The URL carries no credentials, query or fragment; a key goes in \
                 a header. Headers are stored unencrypted and never returned. A push is \
                 within 5 seconds of a change. {telemetry_scope}"
            ))
            .takes(
                json!({ "type": "object", "additionalProperties": false, "required": ["endpoint"], "properties": {
                    "endpoint": { "type": "string", "description": "an http or https URL, `/v1/metrics` and all" },
                    "headers": team_headers,
                    "intervalSeconds": { "type": "integer", "minimum": 15, "maximum": 3600, "default": 60 } } }),
                true,
            )
            .answers(ok(201, "Stored: the destination, as `get_team_telemetry` shows it.", Some(telemetry_schema.clone())))
            .answers(ok(200, "Replaced: the new destination.", Some(telemetry_schema)))
            .answers(fail(
                400,
                "the endpoint is not a URL, is not http or https, has no host, carries \
                 credentials, a query or a fragment; more than 8 headers, a header name \
                 that is not valid or is one the control plane sets, a value over 2,048 bytes \
                 or that cannot be sent; or an interval outside 15 to 3,600 seconds",
            ))
            .answers(fail(
                403,
                "the operator allowed no destination (`this control plane sends no team \
                 telemetry`), the endpoint is not under one it allowed, or the key or member \
                 lacks the `telemetry` scope",
            ))
            .answers(body_rejection())
            .answers_all(gate(false, "telemetry"))
            .repeatable(),
            Spec::new(
                "delete_team_telemetry",
                "Remove A Team's Telemetry Destination",
                "DELETE",
                "/telemetry",
            )
            .about(format!(
                "Remove the caller's team's destination: nothing more is pushed, and the \
                 record of the last push and the log and trace positions are forgotten. \
                 {telemetry_scope}"
            ))
            .answers(plain(204, "Removed."))
            .answers(no_destination())
            .answers_all(gate(false, "telemetry")),
        ]
        .into_iter()
        .map(|s| s.build(false))
        .collect::<Vec<_>>();
        let admin = gate(false, "admin");
        let cluster = vec![
            Spec::new(
                "list_cluster_events",
                "Read The Cluster Event Log",
                "GET",
                "/cluster/events",
            )
            .about(
                "The newest cluster events, newest first, as the store keeps them: each with \
                 its ID, time in milliseconds, kind (`node-joined`, `sandbox-created`, \
                 `sandbox-deleted`, `sandbox-expired`, `sandbox-lost`, `machine-*`, `key-created`, \
                 `key-deleted` and the like), node, sandbox or machine, detail, template, team \
                 and `data` (a sandbox's metadata, and for an end its execution and usage \
                 record). It lists every team's events at once and filters by none, so it is \
                 administrator-only: a key with a team or a principal is refused even \
                 with more scopes, and no scope but `admin` reaches it. `count` is how many \
                 (100 by default, at most 10,000, the size of the store's tail); a `count` \
                 that is not a number is read as 100. Per-team events are \
                 `list_sandbox_events`.",
            )
            .with(query_param(
                "count",
                "How many events (default 100, most 10000)",
                json!({ "type": "integer", "minimum": 0, "maximum": 10000, "default": 100 }),
            ))
            .answers(ok(
                200,
                "The events.",
                Some(json!({ "type": "array", "items": { "type": "object", "properties": {
                    "id": { "type": "string" },
                    "at_ms": { "type": "integer" },
                    "kind": { "type": "string" },
                    "node_id": { "type": "string" },
                    "sandbox_id": { "type": ["string", "null"] },
                    "machine": { "type": ["string", "null"] },
                    "detail": { "type": ["string", "null"] },
                    "template_id": { "type": ["string", "null"] },
                    "team_id": { "type": ["string", "null"] },
                    "data": { "type": ["object", "null"] } } } })),
            ))
            .answers(fail(
                403,
                "`this resource is shared by every team and is not available to team keys`, or \
                 `the cluster event log is not available to a key without a team` (a key with \
                 a principal and no team is a team of one)",
            ))
            .answers_all(admin)
            .repeatable(),
            Spec::new(
                "list_cluster_nodes",
                "List The Cluster's Nodes",
                "GET",
                "/cluster/nodes",
            )
            .about(
                "The nodes whose heartbeat is current. An administrator, and a caller with no \
                 team and no principal (an inventory key, or an open control plane), gets each \
                 node as it reported itself: ID, API URL, proxy address, capacity, sandboxes \
                 running, last heartbeat, version, workload-identity key, templates offered \
                 and their metadata. Any other caller who may read the inventory gets only \
                 each node's ID, capacity, running count, heartbeat, version and the templates \
                 it may start: no URL, no signing key and no other team's snapshot names. \
                 The route is one of the inventory routes an observer or viewer (and a key \
                 with the `inventory` scope) may read; a key with a resource scope alone \
                 may not.",
            )
            .answers(ok(
                200,
                "The nodes, in the form the caller may see.",
                Some(json!({ "type": "array", "items": { "type": "object", "properties": {
                    "id": { "type": "string" },
                    "api": { "type": "string", "description": "administrators and unscoped callers only" },
                    "proxy": { "type": "string", "description": "administrators and unscoped callers only" },
                    "capacity": { "type": "integer" },
                    "running": { "type": "integer" },
                    "heartbeat_ms": { "type": "integer" },
                    "version": { "type": "string" },
                    "jwk": { "type": "object", "description": "administrators and unscoped callers only" },
                    "templates": { "type": "array", "items": { "type": "string" } },
                    "template_metadata": { "type": "object", "description": "administrators and unscoped callers only" } } } })),
            ))
            .answers_all(vec![
                fail(401, "missing, expired or wrong X-API-Key or session"),
                fail(
                    403,
                    "the key or member holds neither the `inventory` scope nor `admin`, or a \
                     session-cookie change did not come from the control plane's own pages",
                ),
                fail(503, "`the cluster store is unavailable`"),
            ])
            .repeatable(),
            Spec::new(
                "get_control_plane_health",
                "Read The Control Plane's Health",
                "GET",
                "/health",
            )
            .about(
                "A liveness-and-store check: 200 with `{\"status\": \"ok\", \"nodes\": N}` \
                 where N is the number of nodes with a current heartbeat, so a control plane \
                 with an empty cluster is healthy and says 0, or 503 when the cluster store \
                 cannot be read. It is outside the key check and single sign-on, as a load \
                 balancer's probe holds no key.",
            )
            .answers(ok(
                200,
                "The store answered.",
                Some(json!({ "type": "object", "properties": {
                    "status": { "type": "string" }, "nodes": { "type": "integer" } } })),
            ))
            .answers(fail(503, "`the cluster store is unavailable`"))
            .repeatable(),
            Spec::new(
                "get_control_plane_ui",
                "Read The Operator Page",
                "GET",
                "/ui",
            )
            .about(
                "The operator's page: one static HTML document (nodes with load, sandboxes, \
                 events) that calls this API from the browser with a key the viewer enters. \
                 It is outside the key check, so anyone who can reach the port can load the \
                 page; what it shows still needs a key, and it answers with a content \
                 security policy that lets it talk only to its own origin and load nothing \
                 else. Single sign-on's login lands here by default (`returnTo=/ui`).",
            )
            .answers(ok(
                200,
                "The page, as `text/html; charset=utf-8`.",
                Some(json!({ "type": "string" })),
            ))
            .repeatable(),
        ];
        specs.extend(cluster.into_iter().map(|s| s.build(false)));
        specs
    }

    /// The capabilities of these routes, for the control plane (`node`
    /// false) or the node.
    pub(super) fn build_platform_capabilities(node: bool) -> Vec<Capability> {
        let prefix = if node { "node_" } else { "" };
        let ids = |list: &[&str]| -> Vec<String> {
            list.iter().map(|id| format!("{prefix}{id}")).collect()
        };
        let who = if node {
            "the node daemon"
        } else {
            "the cluster control plane"
        };
        let suffix = if node {
            "node daemon"
        } else {
            "cluster control plane"
        };
        let (admin, events): (&[&str], &[&str]) = if node {
            (&["cluster token"], &["cluster token"])
        } else {
            (&["admin"], &["events"])
        };
        let make = |id: &str,
                    name: String,
                    description: String,
                    category: CapabilityCategory,
                    operations: Vec<String>,
                    prerequisites: Vec<String>,
                    permissions: &[&str]| Capability {
            id: format!("{prefix}{id}"),
            name,
            description,
            category,
            operations,
            prerequisites,
            permissions_required: permissions.iter().map(|p| p.to_string()).collect(),
        };
        let mut capabilities = vec![
            make(
                "sandbox_environments",
                format!("Environments ({suffix})"),
                format!(
                    "Keep named sets of variables, files and repositories that a sandbox is \
                     created with (`environment: NAME`), through {who}. Values and contents \
                     are stored unencrypted and never returned. See docs/ENVIRONMENTS.md"
                ),
                CapabilityCategory::Compute,
                ids(&[
                    "list_environments",
                    "get_environment",
                    "put_environment",
                    "delete_environment",
                ]),
                vec![],
                admin,
            ),
            make(
                "sandbox_event_webhooks",
                format!("Event Webhooks ({suffix})"),
                format!(
                    "Register URLs that receive the cluster's sandbox and machine lifecycle \
                     events as signed POSTs, change and rotate them, and read their delivery \
                     attempts and statistics, through {who}. See docs/WEBHOOKS.md"
                ),
                CapabilityCategory::Monitoring,
                ids(&[
                    "list_sandbox_webhooks",
                    "create_sandbox_webhook",
                    "get_sandbox_webhook",
                    "update_sandbox_webhook",
                    "delete_sandbox_webhook",
                    "list_webhook_deliveries",
                    "get_webhook_stats",
                ]),
                vec![],
                events,
            ),
            make(
                "sandbox_usage_report",
                format!("Sandbox Usage ({suffix})"),
                format!(
                    "How long sandboxes ran in a window, in running seconds, vCPU-seconds and \
                     GiB-seconds, grouped by sandbox, team, template or owner, through {who}. \
                     See docs/USAGE.md"
                ),
                CapabilityCategory::Monitoring,
                ids(&["get_usage"]),
                vec![],
                admin,
            ),
            make(
                "workload_identity_discovery",
                format!("Workload Identity Discovery ({suffix})"),
                format!(
                    "The public keys and the OIDC discovery document a cloud fetches to verify \
                     the JWT-SVID tokens of the sandboxes {who} runs or tracks present, with no \
                     credential. See \
                     docs/CUBESANDBOX_PARITY_ROADMAP.md"
                ),
                CapabilityCategory::Security,
                ids(&["get_identity_jwks", "get_identity_openid_configuration"]),
                vec![],
                &[],
            ),
            make(
                "prometheus_exposition",
                format!("Prometheus Metrics ({suffix})"),
                format!(
                    "{who}'s own counts and gauges as Prometheus text, for a scraper that holds \
                     no credential. See docs/TELEMETRY.md"
                ),
                CapabilityCategory::Monitoring,
                ids(&["get_metrics_exposition"]),
                vec![],
                &[],
            ),
        ];
        if node {
            capabilities.push(make(
                "warm_pool",
                format!("Warm Pool ({suffix})"),
                "Read how many pre-restored guests a node keeps ready and how often a create \
                 took one. See docs/WARM_POOL.md"
                    .to_string(),
                CapabilityCategory::Compute,
                ids(&["get_warm_pool"]),
                vec!["a node started with --warm-pool".to_string()],
                &["cluster token"],
            ));
        } else {
            capabilities.push(make(
                "team_telemetry",
                format!("Team Telemetry ({suffix})"),
                "Set the OTLP/HTTP collector the control plane pushes a team's own numbers, \
                 sandbox events and sandbox lifetimes to. The destination must be under a \
                 prefix the operator allowed. See docs/TELEMETRY.md"
                    .to_string(),
                CapabilityCategory::Monitoring,
                ids(&[
                    "get_team_telemetry",
                    "put_team_telemetry",
                    "delete_team_telemetry",
                ]),
                vec!["a control plane started with --team-otlp-endpoint".to_string()],
                &["telemetry"],
            ));
            capabilities.push(make(
                "cluster_inventory",
                format!("Cluster Inventory ({suffix})"),
                "List the cluster's nodes and read its event log, check the control plane's \
                 health and load the operator page. See docs/TEAMS.md and \
                 docs/CUBESANDBOX_PARITY_ROADMAP.md"
                    .to_string(),
                CapabilityCategory::Monitoring,
                ids(&[
                    "list_cluster_nodes",
                    "list_cluster_events",
                    "get_control_plane_health",
                    "get_control_plane_ui",
                ]),
                vec![],
                &["inventory", "admin"],
            ));
        }
        capabilities
    }
}

/// The guardrails of these routes.
pub(super) fn platform_guardrails() -> Vec<Guardrail> {
    let both = |ids: &[&str]| -> Vec<String> {
        ids.iter()
            .flat_map(|id| [id.to_string(), format!("node_{id}")])
            .collect()
    };
    vec![
        Guardrail {
            id: "environments-keep-secrets-unencrypted".to_string(),
            severity: GuardrailSeverity::Warning,
            title: "An environment's values, files and repository passwords are stored in \
                the clear"
                .to_string(),
            description: "An environment is kept in the cluster store as given: variable \
                values, file contents and repository passwords included, unencrypted, so a \
                secret put in one is as safe as the store. The API never returns a value, a \
                content or a password, and the node does not log them, but anyone who can \
                read the store reads them. A repository's password is for an account on a \
                Lit server, and Lit 1.8.0 lets only an administrator account clone, so an \
                environment holding a repository holds an administrator password for that \
                server. A lone node keeps environments in memory and forgets them on \
                restart. A change reaches only sandboxes created afterwards."
                .to_string(),
            applies_to: both(&[
                "sandbox_environments",
                "put_environment",
                "get_environment",
                "list_environments",
            ]),
            condition: "A secret, a certificate or a repository password is put in an \
                environment, or the cluster store's Redis is shared with parties that must \
                not read them."
                .to_string(),
            mitigation: "Give each environment's repositories a Lit server of their own, \
                keep the store's Redis behind its role ACLs (`tools/redis-acl.py`) and its \
                network private, and prefer a short-lived token to a long-lived password."
                .to_string(),
            reference: "docs/ENVIRONMENTS.md".to_string(),
        },
        Guardrail {
            id: "administrator-environments-are-not-found-by-team-sandboxes".to_string(),
            severity: GuardrailSeverity::Warning,
            title: "An environment an administrator makes is not found by a team's sandbox"
                .to_string(),
            description: "On the control plane a team's key reaches `/environments` with the                 `sandboxes` scope (GET, HEAD, PUT, DELETE) and `/usage` with the `events`                 scope (GET, HEAD); observer and read-only keys are refused. Both handlers                 act on the caller's team only: environments are stored per team, and the                 usage report keeps only the caller's team's stretches (an administrator sees                 every team). A sandbox created under a team looks for its environment in that                 team's scope only (404 `no environment` otherwise), so an environment an                 administrator makes, in the scope with no team, is not found by a team's                 sandbox. A node serves both routes with no team at all."
                .to_string(),
            applies_to: both(&[
                "sandbox_environments",
                "sandbox_usage_report",
                "put_environment",
                "list_environments",
                "get_usage",
            ]),
            condition: "An administrator makes an environment expecting a team's sandboxes to                 start with it."
                .to_string(),
            mitigation: "Make a team's environments with that team's key; give a team key the                 `sandboxes` scope for environments and `events` for its usage."
                .to_string(),
            reference: "docs/TEAMS.md".to_string(),
        },
        Guardrail {
            id: "webhook-urls-are-checked-at-delivery".to_string(),
            severity: GuardrailSeverity::Warning,
            title: "A webhook's URL is not checked when it is registered, only when an \
                event is sent"
                .to_string(),
            description: "Registering or changing a webhook checks that the URL is http or \
                https with a host, and nothing about where it points. At each delivery the \
                node resolves the host, refuses an address that is not global (loopback, \
                private, link-local and the cloud metadata service, shared and \
                benchmarking ranges, 6to4, Teredo and NAT64 except `64:ff9b::/96`, where the \
                embedded IPv4 address decides), connects to the address it checked, \
                follows no redirect, and gives up after one refusal without a retry. A node \
                started with --allow-private-webhooks sends to private addresses too. A \
                refused URL therefore shows as failed deliveries (`request_error`), not as an \
                error when it was registered. Each team may hold 100 webhooks. The \
                `e2b-signature` header is not an HMAC and uses the current secret only; \
                verify `hv2-signature`, an HMAC-SHA256 over `<timestamp>.<body>`, and reject \
                an old timestamp."
                .to_string(),
            applies_to: both(&[
                "sandbox_event_webhooks",
                "create_sandbox_webhook",
                "update_sandbox_webhook",
                "list_webhook_deliveries",
            ]),
            condition: "A caller may register URLs that the operator must keep the platform \
                from reaching, or a receiver verifies the E2B signature."
                .to_string(),
            mitigation: "Leave --allow-private-webhooks off unless every receiver is inside \
                the cluster, read the delivery records after registering, and verify \
                `hv2-signature` with a timestamp window."
                .to_string(),
            reference: "docs/WEBHOOKS.md".to_string(),
        },
        Guardrail {
            id: "node-platform-routes-filter-by-no-team".to_string(),
            severity: GuardrailSeverity::Warning,
            title: "A node serves environments, webhooks and usage with no team filter".to_string(),
            description: "The node mounts the control plane's environments, webhooks and \
                usage routes behind the cluster token alone, and sets no team on a request, \
                so a caller that holds the token (or, on a node with no token, anyone \
                reaching the port) lists every webhook in the store, with its URL, reads \
                every team's delivery records, which hold event bodies and sandbox \
                metadata, deletes any webhook, reads every sandbox's usage, and works in the \
                environments that have no team. A webhook made on a node has no team and \
                receives every team's events. In a cluster these are the cluster store's \
                records, not the node's own. The webhook secret is never returned."
                .to_string(),
            applies_to: [
                "node_sandbox_environments",
                "node_sandbox_event_webhooks",
                "node_sandbox_usage_report",
            ]
            .into_iter()
            .map(str::to_string)
            .chain(
                [
                    "list_environments",
                    "get_environment",
                    "put_environment",
                    "delete_environment",
                    "list_sandbox_webhooks",
                    "create_sandbox_webhook",
                    "get_sandbox_webhook",
                    "update_sandbox_webhook",
                    "delete_sandbox_webhook",
                    "list_webhook_deliveries",
                    "get_webhook_stats",
                    "get_usage",
                ]
                .into_iter()
                .map(|id| format!("node_{id}")),
            )
            .collect(),
            condition: "A node's API port is reachable by a party that must not see another \
                team's webhooks, deliveries or usage, or the node has no cluster token."
                .to_string(),
            mitigation: "Keep node ports on a network only the control plane reaches, give \
                every node a cluster token, and see node-unauthenticated-mode."
                .to_string(),
            reference: "crates/hv2-cluster/src/events.rs".to_string(),
        },
        Guardrail {
            id: "team-telemetry-leaves-the-platform".to_string(),
            severity: GuardrailSeverity::Info,
            title: "A team's telemetry destination receives the team's usage, sandbox \
                events and lifetimes, and its headers are stored in the clear"
                .to_string(),
            description: "Once a destination is stored, the control plane sends the team's \
                running counts and rolling usage, one log record per lifecycle event of its \
                sandboxes (with the sandbox ID, node, template and detail) when the URL ends \
                in `/v1/metrics`, and one span per ended sandbox. The control plane makes \
                the request from inside the operator's network, so the endpoint must lie \
                under a --team-otlp-endpoint prefix (the same scheme, host and port, and a \
                path at or below it), which is checked on storing and at each push; with \
                none listed nothing is sent. No redirect is followed. The headers, which \
                are often a credential, are kept unencrypted in the cluster store and \
                returned by name only. Command output is not sent. Every control plane \
                given the flag pushes, so a collector is sent each team once per instance."
                .to_string(),
            applies_to: [
                "team_telemetry",
                "put_team_telemetry",
                "get_team_telemetry",
                "delete_team_telemetry",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            condition: "A team may point telemetry at a collector the operator does not \
                fully control, or the store is shared."
                .to_string(),
            mitigation: "List only collectors the operator trusts with every team's \
                sandbox IDs, keep the store's Redis behind its role ACLs (the node's user \
                has no access to `team-telemetry`), and use a header token scoped to \
                ingestion."
                .to_string(),
            reference: "docs/TELEMETRY.md".to_string(),
        },
        Guardrail {
            id: "cluster-event-log-lists-every-team".to_string(),
            severity: GuardrailSeverity::Info,
            title: "The cluster event log is unfiltered and administrator-only".to_string(),
            description: "`list_cluster_events` returns the store's newest events (up to \
                10,000) with every team's sandbox IDs, metadata (`data`), usage records and \
                the key-management events, filtered by nothing. The control plane refuses \
                it to every key with a team or a principal, and no scope but `admin` \
                reaches it. It is not the per-team event route (`list_sandbox_events`), \
                which filters by team."
                .to_string(),
            applies_to: ["list_cluster_events", "cluster_inventory"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            condition: "An administrator key is shared beyond the operators.".to_string(),
            mitigation: "Keep `admin` keys with the operators; give teams the per-team \
                event routes and webhooks."
                .to_string(),
            reference: "docs/TEAMS.md".to_string(),
        },
        Guardrail {
            id: "platform-discovery-and-metrics-are-public".to_string(),
            severity: GuardrailSeverity::Info,
            title: "Health, metrics, the operator page and the identity documents answer \
                with no credential"
                .to_string(),
            description: "The control plane's `/health`, `/metrics`, `/ui`, \
                `/.well-known/jwks.json` and `/.well-known/openid-configuration` are outside \
                its key check and single sign-on, and a node's `/metrics`, `jwks.json` and \
                `openid-configuration` are outside its cluster-token check: a scraper and a \
                cloud that verifies a workload token hold no credential. They carry counts, \
                public keys and a static page, never a sandbox's identity, but they say the \
                cluster's size and load to anyone who can reach the port."
                .to_string(),
            applies_to: both(&[
                "prometheus_exposition",
                "workload_identity_discovery",
                "get_metrics_exposition",
                "get_identity_jwks",
                "get_identity_openid_configuration",
            ])
            .into_iter()
            .chain(
                [
                    "cluster_inventory",
                    "get_control_plane_health",
                    "get_control_plane_ui",
                ]
                .into_iter()
                .map(str::to_string),
            )
            .collect(),
            condition: "The ports are reachable beyond the operators and the clouds that \
                verify tokens."
                .to_string(),
            mitigation: "Put the control plane behind a proxy that exposes only the paths \
                a verifier or a scraper needs, and keep node ports private."
                .to_string(),
            reference: "docs/TELEMETRY.md".to_string(),
        },
    ]
}
