//! The `/machines` routes, as the control plane (`hv2-cluster`, `control.rs`:
//! `list_machines`, `create_machine`, `to_machine_node`,
//! `machine_reverse_tunnel`) and the node (`hv2-sandboxd`, `machines.rs` and
//! the modules under `machines/`) serve them. Modelled as data: `hv2-api`
//! depends on neither crate.
//!
//! The control plane serves 35 of the node's 45 machine routes. It does not
//! route conversations or the desktop. Every one it does route is the node's
//! own handler behind a lookup: it asks each live node which one holds the
//! machine, then relays.

use super::spec::{body_rejection, fail, ok, path_param, plain, query_param, required_query, Spec};
use super::{HyperMachineOntology, Operation, OperationResponse, Parameter};
use serde_json::{json, Value};

/// A machine's name or ID, in a path.
fn machine_param() -> Parameter {
    path_param(
        "name",
        "The machine's name (one of the caller's own) or its ID, `vm-` and 16 hexadecimal digits",
        json!({ "type": "string" }),
    )
}

/// What a node describes a machine as (`describe` in `machines.rs`).
fn machine_schema(node: bool) -> Value {
    let mut properties = json!({
        "machineID": { "type": "string", "description": "`vm-` and 16 hex digits, derived from the team and the name" },
        "name": { "type": "string" },
        "teamID": { "type": ["string", "null"] },
        "templateID": { "type": "string", "description": "empty for a machine from an image" },
        "cpuCount": { "type": "integer" },
        "memoryMB": { "type": "integer" },
        "diskGiB": { "type": "integer" },
        "autostart": { "type": "boolean" },
        "restartPolicy": { "type": "string", "enum": ["always", "never"] },
        "desiredState": { "type": "string", "enum": ["running", "stopped"] },
        "state": { "type": "string", "enum": ["running", "stopped"] },
        "startedAt": { "type": ["integer", "null"], "description": "unix milliseconds of this boot" },
        "createdAt": { "type": "integer", "description": "unix milliseconds" },
        "network": { "type": ["object", "null"], "description": "its egress rules as asked for: allowInternetAccess, allowOut, denyOut" },
        "boot": { "type": "string", "enum": ["kernel", "firmware"] },
        "image": { "type": ["string", "null"] },
        "cloudInit": { "type": "boolean", "description": "whether it was given user data, which is never returned" },
        "guestAgent": { "type": "boolean" },
        "agents": { "type": "array", "items": { "type": "string" } },
        "desktop": { "type": ["object", "null"], "properties": { "width": { "type": "integer" }, "height": { "type": "integer" } } },
        "ports": { "type": "array", "items": { "type": "object", "properties": {
            "hostPort": { "type": "integer" }, "guestPort": { "type": "integer" }, "proxy": { "type": "boolean" } } } }
    });
    if !node {
        properties["nodeID"] = json!({
            "type": "string",
            "description": "added to a listing: the node that holds the machine"
        });
    }
    json!({ "type": "object", "properties": properties })
}

/// A run as `Run::describe` gives it, with or without its output.
fn run_schema(with_output: bool) -> Value {
    let mut properties = json!({
        "runID": { "type": "integer" },
        "cmd": { "type": "string", "description": "the command line, as given: not masked" },
        "pid": { "type": "integer" },
        "state": { "type": "string", "enum": ["running", "exited", "lost"] },
        "exitCode": { "type": ["integer", "null"] },
        "signal": { "type": ["integer", "null"] },
        "reason": { "type": ["string", "null"], "description": "why a run is `lost`" },
        "startedAt": { "type": "integer", "description": "unix milliseconds" },
        "endedAt": { "type": ["integer", "null"] },
        "stdoutBytes": { "type": "integer", "description": "everything it wrote, kept or not" },
        "stderrBytes": { "type": "integer" }
    });
    if with_output {
        let stream = json!({
            "type": "object",
            "properties": {
                "offset": { "type": "integer", "description": "where the text really starts: later than asked when what was asked for was dropped" },
                "text": { "type": "string" },
                "next": { "type": "integer", "description": "the offset to ask for next" },
                "dropped": { "type": "integer", "description": "bytes before the kept tail that were discarded" }
            }
        });
        properties["stdout"] = stream.clone();
        properties["stderr"] = stream;
    }
    json!({ "type": "object", "properties": properties })
}

/// A prompt, which is a run described by what was asked: `runID` is
/// `promptID` and `cmd` is left out.
fn prompt_schema(with_events: bool) -> Value {
    let mut properties = run_schema(false)["properties"].clone();
    if let Some(object) = properties.as_object_mut() {
        object.remove("cmd");
        object.remove("runID");
    }
    properties["promptID"] = json!({ "type": "integer", "description": "the run's ID" });
    properties["provider"] = json!({ "type": "string", "enum": ["claude", "codex", "custom"] });
    properties["prompt"] = json!({ "type": "string", "description": "kept, and returned to whoever may read the machine" });
    properties["model"] = json!({ "type": ["string", "null"] });
    properties["resume"] = json!({ "type": ["string", "null"] });
    if with_events {
        properties["session"] = json!({ "type": ["string", "null"], "description": "the agent's own session ID, for `resume`" });
        properties["events"] = json!({ "type": "array", "items": { "type": "object", "properties": {
            "offset": { "type": "integer" },
            "event": { "type": "object", "description": "a line the agent printed that is a JSON object" },
            "text": { "type": "string", "description": "any other line" } } } });
        properties["next"] =
            json!({ "type": "integer", "description": "the `after` to ask with next" });
    }
    json!({ "type": "object", "properties": properties })
}

/// A published port (`describe` in `machines/ports.rs`).
fn port_schema() -> Value {
    json!({ "type": "object", "properties": {
        "hostPort": { "type": "integer", "description": "the node's port" },
        "guestPort": { "type": "integer" },
        "address": { "type": "string", "description": "what the node listens on: its `--machine-port-address`" },
        "listening": { "type": "boolean", "description": "false when the node could not take the port back after a restart" },
        "proxy": { "type": "boolean" },
        "proxyLabel": { "type": ["string", "null"], "description": "`{guestPort}-{machineID}`, the first label of the proxy name, when `proxy`" }
    } })
}

/// What a snapshot of a machine's disk is described as.
fn disk_snapshot_schema() -> Value {
    json!({ "type": "object", "properties": {
        "snapshotID": { "type": "string", "description": "`snap-` and 15 hex digits" },
        "name": { "type": "string" },
        "createdAt": { "type": "string", "format": "date-time" },
        "diskGiB": { "type": "integer", "description": "the disk's size then, which a restore gives back" },
        "bytesOnNode": { "type": "integer", "description": "what it holds on the node's disk, not its length" }
    } })
}

/// A conversation, listed (`full` false) or described.
fn conversation_schema(full: bool) -> Value {
    let mut properties = json!({
        "conversationID": { "type": "integer" },
        "name": { "type": ["string", "null"] },
        "provider": { "type": "string", "enum": ["claude", "codex"] },
        "model": { "type": ["string", "null"] },
        "user": { "type": ["string", "null"] },
        "cwd": { "type": ["string", "null"] },
        "unattended": { "type": "boolean" },
        "from": { "type": ["integer", "null"], "description": "the conversation it carries on from" },
        "createdAt": { "type": "integer" },
        "state": { "type": "string", "enum": ["running", "idle"] },
        "runningPromptID": { "type": ["integer", "null"] },
        "turns": { "type": "array", "items": { "type": "integer" }, "description": "prompt IDs" },
        "messageCount": { "type": "integer" }
    });
    if full {
        properties["session"] = json!({ "type": ["string", "null"] });
        properties["messages"] = json!({ "type": "array", "items": { "type": "object", "properties": {
            "prompt": { "type": "string" },
            "promptID": { "type": "integer" },
            "steered": { "type": "boolean" },
            "sentAt": { "type": "integer" } } } });
    }
    json!({ "type": "object", "properties": properties })
}

/// What can refuse a request for a machine before or around the handler.
///
/// On the control plane that is the key or session check, the `machines`
/// scope, and, for a named machine, the search of every node for it. On a
/// node, the cluster token, the team the control plane names in
/// `x-hv2-sandbox-team`, and a host that cannot run machines.
fn access(node: bool, named: bool) -> Vec<OperationResponse> {
    let mut responses = Vec::new();
    if node {
        responses.push(fail(
            401,
            "the node was started with a cluster token (or mTLS) and the request does not \
             carry it in `x-hv2-cluster-token`; a node with no token answers everyone",
        ));
        responses.push(fail(
            400,
            "the control plane's `x-hv2-sandbox-team` header is repeated or is not a valid team",
        ));
        responses.push(fail(
            503,
            "a team header arrived on a node that is not part of a cluster with a cluster \
             token (`team attribution requires an authenticated cluster node`)",
        ));
        responses.push(fail(
            501,
            "the node is not a Linux host with KVM: every machine route answers `machines \
             need a Linux host with KVM`",
        ));
        if named {
            responses.push(fail(
                404,
                "no machine of that name or ID that the caller may reach: a machine of \
                 another team is one that does not exist",
            ));
        }
        return responses;
    }
    responses.push(fail(
        401,
        "missing, expired or wrong X-API-Key or session; with single sign-on configured an \
         anonymous request is refused too",
    ));
    responses.push(fail(
        403,
        "the key or member lacks the `machines` scope (a key with only `inventory` does not \
         reach machines, since a machine's record carries its network rules, and neither does \
         an observer or viewer), or a session-cookie change did not come from the control \
         plane's own pages",
    ));
    responses.push(fail(400, "a path with `.` or `..` segments"));
    if named {
        responses.push(fail(
            404,
            "no machine of that name or ID that the caller's team may see, on any node: \
             another team's machine is one that does not exist, by name and by ID \
             (administrators and keys with no team see every machine)",
        ));
        responses.push(fail(
            503,
            "a node did not answer while the machine was being looked for, so it may be \
             there (the answer is not 404), or the cluster store is unavailable",
        ));
        responses.push(fail(
            502,
            "the node that holds the machine did not answer the relayed request",
        ));
    } else {
        responses.push(fail(503, "the cluster store is unavailable"));
    }
    responses
}

/// A response for a machine that is not running, with the reason.
fn not_running() -> OperationResponse {
    fail(409, "the machine is not running")
}

/// A response for a guest with no agent answering.
fn no_agent(what: &str) -> OperationResponse {
    fail(
        409,
        &format!(
            "the machine is not running, or has no guest agent answering and {what} go \
             through it: it was booted from an image and either was not created with \
             `guestAgent` or its agent has not started yet"
        ),
    )
}

impl HyperMachineOntology {
    /// The control plane's machine routes.
    pub(super) fn build_machine_control_plane_operations() -> Vec<Operation> {
        Self::machine_operations(false)
    }

    /// The node's machine routes, including the two the control plane does
    /// not route: conversations and the desktop.
    pub(super) fn build_machine_node_operations() -> Vec<Operation> {
        Self::machine_operations(true)
    }

    fn machine_operations(node: bool) -> Vec<Operation> {
        let mut specs: Vec<Spec> = Vec::new();
        specs.extend(Self::machine_lifecycle_specs(node));
        specs.extend(Self::machine_command_specs(node));
        specs.extend(Self::machine_agent_specs(node));
        specs.extend(Self::machine_port_specs(node));
        specs.extend(Self::machine_snapshot_specs(node));
        if node {
            specs.extend(Self::machine_conversation_specs());
            specs.extend(Self::machine_desktop_specs());
        }
        specs.into_iter().map(|spec| spec.build(node)).collect()
    }

    fn machine_lifecycle_specs(node: bool) -> Vec<Spec> {
        let create_body = json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,63}$", "description": "unique among the caller's team's machines" },
                "templateID": { "type": "string", "default": "base", "description": "a template the node has, its file tree becoming the root disk; refused beside `image`" },
                "image": { "type": "string", "description": "a raw disk image in the node's `--image-dir`, booted by firmware (needs `--firmware`); copied, not changed" },
                "userData": { "type": "string", "description": "cloud-init user data, at most 1 MiB; only with `image`; never returned" },
                "guestAgent": { "type": "boolean", "description": "only with `image`: install this project's guest agent through cloud-init, so exec, runs, files and ports work; needs the node's `--guest-agent`" },
                "desktop": { "description": "only with `guestAgent`: `true` (1280x800) or {width, height}, even, 640x480 to 3840x2160", "oneOf": [
                    { "type": "boolean" },
                    { "type": "object", "properties": { "width": { "type": "integer" }, "height": { "type": "integer" } } } ] },
                "agents": { "type": "array", "items": { "type": "string", "enum": ["claude", "codex"] }, "description": "only with `guestAgent`: coding agents installed from npm at first boot, which needs a network" },
                "cpuCount": { "type": "integer", "minimum": 1, "maximum": 32, "description": "the node's default; 1 from an image" },
                "memoryMB": { "type": "integer", "description": "the node's default; held to 128 to 1048576 rather than refused" },
                "diskGiB": { "type": "integer", "minimum": 1, "maximum": 2048, "default": 8 },
                "autostart": { "type": "boolean", "default": true, "description": "start it again when the daemon starts, if it was running" },
                "restartPolicy": { "type": "string", "enum": ["always", "never"], "default": "always", "description": "`always` boots it again when its guest reboots or crashes, at most 5 times in 5 minutes" },
                "start": { "type": "boolean", "default": true },
                "network": { "type": "object", "description": "gives it one NIC behind the node's egress gateway; without it the machine has no network device",
                    "properties": {
                        "allowInternetAccess": { "type": "boolean" },
                        "allowOut": { "type": "array", "items": { "type": "string" } },
                        "denyOut": { "type": "array", "items": { "type": "string" } } } },
                "nodeID": { "type": "string", "description": "control plane only: place it on this node" }
            }
        });
        let mut create_body = create_body;
        if node {
            if let Some(properties) = create_body["properties"].as_object_mut() {
                properties.remove("nodeID");
            }
        }
        vec![
            Spec::new("list_machines", "List Machines", "GET", "/machines")
                .about(if node {
                    "List the machines on this node, with their state and settings, sorted by \
                     name. A node reached through a control plane lists the caller's team's \
                     machines only; a standalone node lists them all. The record carries the \
                     machine's egress rules and published ports; user data is never in it."
                } else {
                    "List the machines of every live node that the caller may see, each with \
                     the `nodeID` that holds it. A team's key sees its own team's machines; an \
                     administrator, or a key with no team, sees all. The control plane asks the \
                     nodes on each request (no store records where a machine is), and a node \
                     that does not answer is left out of the listing without saying so, so a \
                     listing is not proof that a machine does not exist."
                })
                .answers(ok(
                    200,
                    "The machines.",
                    Some(json!({ "type": "array", "items": machine_schema(node) })),
                ))
                .answers_all(access(node, false))
                .repeatable(),
            Spec::new("create_machine", "Create A Machine", "POST", "/machines")
                .about(if node {
                    "Create a long-lived VM with its own ext4 root disk, from a template or from \
                     a raw disk image booted by firmware, and start it unless `start` is false. \
                     The name is the caller's team's own: two teams may each have `web-01`, and \
                     the machine's ID is derived from both. A machine with no `network` has no \
                     network device at all. A machine from an image has this project's guest \
                     agent only when `guestAgent` is true, and exec, runs, prompts, files and \
                     published ports need it. A failed start leaves the machine created and \
                     stopped (503). Records a `machine-created` event for the team's webhooks. \
                     Not idempotent: a second call is 409."
                } else {
                    "Create a machine on the live node with the most free capacity that offers \
                     its template (`base` when none is given), or on `nodeID`. It is refused (503) \
                     while any node that may hold machines is not answering, since the name may \
                     already be taken there, and a name the caller's team already has is 409. The \
                     answer is the node's, with the node in the `x-hv2-node` header. The body \
                     is read in full, at most 64 KiB, so a `userData` over that is refused here \
                     though a node allows 1 MiB; a body that is not JSON is passed on as it \
                     is and the node refuses it. The control plane picks a node by template \
                     even for a machine made from `image`, so a node must offer `base`. Not \
                     idempotent."
                })
                .takes(create_body, true)
                .answers(ok(
                    201,
                    if node {
                        "The machine, running unless `start` was false."
                    } else {
                        "The machine, as its node describes it; the `x-hv2-node` header names the node."
                    },
                    Some(machine_schema(node)),
                ))
                .answers(fail(
                    400,
                    "an invalid name, cpuCount, diskGiB, network, desktop or agent; `image` \
                     with `templateID`; `userData`, `guestAgent`, `agents` or `desktop` on a \
                     machine that cannot take them; a node with no `--firmware`, `--image-dir` \
                     or `--guest-agent` for what was asked",
                ))
                .answers(body_rejection())
                .answers(fail(
                    404,
                    "no template of that name on the node (on the control plane, relayed from the node)",
                ))
                .answers(fail(409, "a machine of that name exists"))
                .answers(fail(
                    500,
                    "the root disk could not be made, or the node's `--guest-agent` is not a usable binary",
                ))
                .answers(fail(
                    503,
                    if node {
                        "the machine is created but could not start (`starting: ...`: no free \
                         slot, or its guest never answered within 60 s); it stays, stopped"
                    } else {
                        "a node that may hold machines is not answering; no live node offers \
                         the template, or `nodeID` is not alive or does not offer it; or the \
                         node's start failed (`starting: ...`) and the machine stays, stopped"
                    },
                ))
                .answers_all(access(node, false)),
            Spec::new("get_machine", "Get A Machine", "GET", "/machines/{name}")
                .about(
                    "One machine's record: its settings, whether it is running, its egress \
                     rules and its published ports. A machine of another team answers as one \
                     that does not exist.",
                )
                .with(machine_param())
                .answers(ok(200, "The machine.", Some(machine_schema(node))))
                .answers_all(access(node, true))
                .repeatable(),
            Spec::new("resize_machine", "Resize A Stopped Machine", "PATCH", "/machines/{name}")
                .about(
                    "Give a stopped machine other processors, other memory or a larger disk, \
                     any of the three alone or together; it has them the next time it starts. \
                     Nothing is rounded: a size that cannot be is refused. The disk only \
                     grows (a smaller one would lose what is on it) and a template machine's \
                     filesystem is checked and grown with it; an image machine's partitions are \
                     left for the guest to grow. A running machine is refused (409). Unknown \
                     fields are refused.",
                )
                .with(machine_param())
                .takes(
                    json!({
                        "type": "object",
                        "properties": {
                            "cpuCount": { "type": "integer", "minimum": 1, "maximum": 32 },
                            "memoryMB": { "type": "integer", "minimum": 128, "maximum": 1_048_576 },
                            "diskGiB": { "type": "integer", "maximum": 2048, "description": "not smaller than now" }
                        }
                    }),
                    true,
                )
                .answers(ok(200, "The machine, with its new settings.", Some(machine_schema(node))))
                .answers(fail(
                    400,
                    "none of the three given, a size out of range, or a smaller disk",
                ))
                .answers(body_rejection())
                .answers(fail(409, "the machine is running; stop it first"))
                .answers(fail(
                    500,
                    "the disk's filesystem did not pass its check, or the disk or filesystem could not be grown",
                ))
                .answers_all(access(node, true))
                .repeatable(),
            Spec::new("delete_machine", "Delete A Machine", "DELETE", "/machines/{name}")
                .about(
                    "Delete a stopped machine and its disk, and with it its snapshots, run \
                     output, prompts, conversations and published ports. A running machine is \
                     refused (409); stop it first. Records a `machine-deleted` event. The \
                     second call is 404.",
                )
                .with(machine_param())
                .answers(ok(204, "Deleted.", None))
                .answers(fail(409, "the machine is running; stop it first"))
                .answers(fail(500, "the machine's directory could not be removed"))
                .answers_all(access(node, true)),
            Spec::new(
                "control_machine",
                "Start, Stop Or Restart A Machine",
                "POST",
                "/machines/{name}/{action}",
            )
            .about(
                "Start, stop or restart a machine and record that as what it should be, so \
                 `autostart` brings a running one back when the daemon starts. `start` of a \
                 running machine and `stop` of a stopped one change nothing and answer 200. \
                 `stop` syncs the guest's filesystems through its agent first; a machine from \
                 an image with no agent is stopped as pulling the plug would, and a guest's \
                 own `poweroff` leaves it stopped whatever its restart policy. Boot waits for \
                 the guest to answer, up to 60 s. Records `machine-started` and \
                 `machine-stopped` events for the team's webhooks.",
            )
            .with(machine_param())
            .with(path_param(
                "action",
                "What to do",
                json!({ "type": "string", "enum": ["start", "stop", "restart"] }),
            ))
            .answers(ok(200, "The machine, as it is now.", Some(machine_schema(node))))
            .answers(fail(
                404,
                "no such machine, or an action that is not start, stop or restart (the \
                 message says which)",
            ))
            .answers(fail(
                503,
                "it could not start (`starting: ...`): no free slot on the node, or its \
                 guest never answered; the machine stays stopped",
            ))
            .answers(fail(500, "its desired state could not be saved"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "get_machine_console",
                "Read A Machine's Serial Console",
                "GET",
                "/machines/{name}/console",
            )
            .about(
                "Read the end of a running machine's serial console. With `tail=N`, its last \
                 N bytes exactly as the guest wrote them (up to 1 MiB, all it keeps), as \
                 plain text; without it, a one-line summary of what its vCPUs are doing over \
                 half a second and the console's last lines. The console is the only way into \
                 an image machine that has no guest agent. It can hold anything the guest \
                 printed.",
            )
            .with(machine_param())
            .with(query_param(
                "tail",
                "Return this many of the console's last bytes",
                json!({ "type": "integer", "minimum": 0 }),
            ))
            .answers(ok(200, "Plain text.", None))
            .answers(not_running())
            .answers(plain(
                400,
                "`tail` is not a number: the framework's own rejection",
            ))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "send_machine_console_input",
                "Type At A Machine's Serial Console",
                "POST",
                "/machines/{name}/console",
            )
            .about(
                "Type the request body at a running machine's serial console, as a terminal \
                 would send it, 1 to 4096 bytes a request. This is input to whatever is \
                 reading the console in the guest, a login prompt or a root shell included, \
                 and needs no guest agent. Nothing is returned; read the result with the \
                 console route. Not idempotent.",
            )
            .with(machine_param())
            .takes_raw("application/octet-stream", json!({ "type": "string" }), true)
            .answers(ok(204, "The bytes were sent.", None))
            .answers(fail(400, "the body is empty or over 4096 bytes"))
            .answers(not_running())
            .answers(fail(409, "the machine has no serial console"))
            .answers(fail(502, "the console would not take the input"))
            .answers_all(access(node, true)),
            Spec::new(
                "get_machine_network_decisions",
                "Read What A Machine's Gateway Decided",
                "GET",
                "/machines/{name}/network/decisions",
            )
            .about(
                "What the egress gateway behind a running machine's NIC allowed and refused \
                 since its last boot: the first 256 and the most recent 256 decisions, each \
                 with a `seq` (from 1, so a gap shows), `at` (unix milliseconds), the kind, \
                 destination and name, a verdict and a reason; `recorded` is every decision \
                 made and `dropped` those not returned; `stats` are the gateway's counters.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The decisions.",
                Some(json!({ "type": "object", "properties": {
                    "decisions": { "type": "array", "items": { "type": "object", "properties": {
                        "seq": { "type": "integer" }, "at": { "type": "integer" },
                        "kind": { "type": "string" }, "destination": { "type": "string" },
                        "name": { "type": ["string", "null"] },
                        "verdict": { "type": "string" }, "reason": { "type": "string" } } } },
                    "recorded": { "type": "integer" },
                    "dropped": { "type": "integer" },
                    "stats": { "type": "object", "description": "connectionsAllowed, connectionsRefused, dnsAnswered, dnsRefused, intercepted and the like" }
                } })),
            ))
            .answers(fail(400, "the machine has no network"))
            .answers(not_running())
            .answers_all(access(node, true))
            .repeatable(),
        ]
    }

    fn machine_command_specs(node: bool) -> Vec<Spec> {
        let offsets = |what: &str| {
            query_param(
                what,
                &format!(
                    "Read standard {} from this byte on (default 0)",
                    if what == "stdout" { "output" } else { "error" }
                ),
                json!({ "type": "integer", "minimum": 0 }),
            )
        };
        vec![
            Spec::new(
                "exec_in_machine",
                "Run A Command In A Machine",
                "POST",
                "/machines/{name}/exec",
            )
            .about(
                "Run one shell line in a running machine's guest as `/bin/sh -c`, as root, and \
                 wait for it. It needs the guest agent: a machine from a template always has \
                 it, one from an image only when created with `guestAgent`. The guest agent \
                 gives the command 80% of `timeout_secs` (default 30, held to 1 to 3600), and \
                 the answer has no `timed_out` flag, so a command killed for time is not told \
                 apart from one that ended. For a command that outlives a request, use a run. \
                 This is arbitrary code execution as root in the guest. Not idempotent.",
            )
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "required": ["cmd"],
                    "properties": {
                        "cmd": { "type": "string", "description": "a shell line" },
                        "timeout_secs": { "type": "integer", "default": 30, "minimum": 1, "maximum": 3600 }
                    }
                }),
                true,
            )
            .answers(ok(
                200,
                "The command ended.",
                Some(json!({ "type": "object", "properties": {
                    "exit_code": { "type": ["integer", "null"], "description": "null when a signal ended it" },
                    "signal": { "type": ["integer", "null"] },
                    "stdout": { "type": "string" },
                    "stderr": { "type": "string" } } })),
            ))
            .answers(fail(400, "the guest has no `/bin/sh`: the message says how to run without one"))
            .answers(body_rejection())
            .answers(no_agent("commands"))
            .answers(fail(502, "the guest agent could not run it (`exec: ...`)"))
            .answers_all(access(node, true)),
            Spec::new(
                "list_machine_processes",
                "List A Machine's Processes",
                "GET",
                "/machines/{name}/processes",
            )
            .about(
                "Every process in a running machine's guest, as `ps` lists them and not only \
                 those started through the API, read from the guest's `/proc` through its \
                 agent (so a guest with no shell lists too). Kernel threads are left out \
                 unless `all=true`; at most 4096 are returned.",
            )
            .with(machine_param())
            .with(query_param(
                "all",
                "Include kernel threads",
                json!({ "type": "boolean", "default": false }),
            ))
            .answers(ok(
                200,
                "The processes.",
                Some(json!({ "type": "object", "properties": { "processes": { "type": "array", "items": {
                    "type": "object",
                    "properties": {
                        "pid": { "type": "integer" }, "ppid": { "type": "integer" },
                        "uid": { "type": ["integer", "null"] },
                        "state": { "type": "string", "description": "R, S, D, Z, T ... as ps shows" },
                        "command": { "type": "string" }, "args": { "type": "string" },
                        "threads": { "type": "integer" }, "rssKiB": { "type": "integer" },
                        "cpuSeconds": { "type": "number" },
                        "startedSecondsAgo": { "type": "number" } } } } } })),
            ))
            .answers(no_agent("its processes are listed"))
            .answers(fail(502, "the guest's processes could not be listed"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "read_machine_file",
                "Read A File From A Machine",
                "GET",
                "/machines/{name}/files",
            )
            .about(
                "Read one file out of a running machine's guest by absolute path, as root, \
                 through its guest agent: at most 64 MiB, held whole in the node's memory on \
                 the way. The path names the file itself (a path ending in `/` is refused). \
                 A refusal says whose it is: 404 for a file that is not there, 403 where the \
                 guest forbids it, 413 over the limit. `user` is for writes and is refused \
                 here.",
            )
            .with(machine_param())
            .with(required_query(
                "path",
                "Absolute path in the guest, at most 4096 bytes",
                json!({ "type": "string" }),
            ))
            .answers(ok(200, "The file's bytes.", Some(json!({ "type": "string", "format": "binary" }))))
            .answers(fail(
                400,
                "the path is not absolute, has a NUL, ends in `/` or is too long; `user` was \
                 given; or it names a directory",
            ))
            .answers(fail(403, "the guest forbids the read"))
            .answers(fail(404, "no such file (or machine)"))
            .answers(fail(413, "the file is over 64 MiB"))
            .answers(no_agent("files"))
            .answers(fail(502, "between the node and the guest"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "write_machine_file",
                "Write A File Into A Machine",
                "PUT",
                "/machines/{name}/files",
            )
            .about(
                "Write the request body, 64 MiB at most, to an absolute path in a running \
                 machine's guest, making the directories above it. With `user` the file and \
                 the directories it made belong to that guest user, root otherwise. The write \
                 is in place, a piece at a time: one that fails part way leaves what it had \
                 written. It overwrites a file that is there. Needs the guest agent.",
            )
            .with(machine_param())
            .with(required_query(
                "path",
                "Absolute path in the guest",
                json!({ "type": "string" }),
            ))
            .with(query_param(
                "user",
                "The guest user that owns what is written (name or number); root if absent",
                json!({ "type": "string" }),
            ))
            .takes_raw(
                "application/octet-stream",
                json!({ "type": "string", "format": "binary" }),
                true,
            )
            .answers(ok(204, "Written.", None))
            .answers(fail(
                400,
                "the path is not absolute, has a NUL, ends in `/` or is too long; `user` is \
                 empty or unknown; a directory is where a file belongs or the reverse",
            ))
            .answers(fail(403, "the guest forbids the write"))
            .answers(fail(413, "the body is over 64 MiB"))
            .answers(no_agent("files"))
            .answers(fail(502, "between the node and the guest"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "list_machine_runs",
                "List A Machine's Runs",
                "GET",
                "/machines/{name}/runs",
            )
            .about(
                "Every run kept for a machine, oldest first, without output: a command \
                 started and left running whose output the node keeps. 256 are kept; the \
                 oldest finished one makes room. The command line of each is shown as it was \
                 given, not masked. Runs are kept across a node restart (one still going \
                 then is `lost`) and are gone when the machine is deleted.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The runs.",
                Some(json!({ "type": "array", "items": run_schema(false) })),
            ))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "start_machine_run",
                "Start A Run In A Machine",
                "POST",
                "/machines/{name}/runs",
            )
            .about(
                "Start a shell line in a running machine's guest and leave it: the answer \
                 comes at once with the run's ID, and the node polls it and keeps what it \
                 prints, the newest 4 MiB of each stream, in memory and on disk where it \
                 outlives the run, a stop and a node restart. Read it with the run route. It \
                 has pipes, not a terminal. `env` is where a key goes: it is not kept or \
                 returned. The command line is kept and returned as given. Runs as root \
                 unless `user`. Needs the guest agent. Not idempotent.",
            )
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "required": ["cmd"],
                    "properties": {
                        "cmd": { "type": "string", "description": "a shell line, run by `/bin/sh -c`" },
                        "cwd": { "type": "string" },
                        "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "set for the command; not kept, not returned" },
                        "user": { "type": "string" },
                        "stdin": { "type": "string", "description": "written to its standard input, which is then closed; leave out to keep it open for the stdin route" }
                    }
                }),
                true,
            )
            .answers(ok(201, "The run, running.", Some(run_schema(false))))
            .answers(fail(400, "`cmd` is empty, or the guest has no `/bin/sh`"))
            .answers(body_rejection())
            .answers(no_agent("runs are started"))
            .answers(fail(
                409,
                "the machine already has 256 runs still running, the most it keeps",
            ))
            .answers(fail(
                502,
                "the run could not be started; or it was started and its `stdin` could not be \
                 written (`run N started, and ...`): it stays and can be read by its ID",
            ))
            .answers_all(access(node, true)),
            Spec::new(
                "get_machine_run",
                "Get A Machine's Run",
                "GET",
                "/machines/{name}/runs/{runID}",
            )
            .about(
                "One run, and what it printed from the offsets asked for. The answer says \
                 where to read from next (`next` of each stream), so a caller follows a run \
                 by asking again. Each stream keeps its newest 4 MiB and `dropped` counts \
                 what went before. A run ends `exited` with a code or a signal, or `lost` \
                 with a reason when the node can no longer reach it, as when its machine \
                 stopped or the node restarted. Output is kept as printed and not scanned \
                 for secrets.",
            )
            .with(machine_param())
            .with(path_param("runID", "The run's number", json!({ "type": "integer", "minimum": 1 })))
            .with(offsets("stdout"))
            .with(offsets("stderr"))
            .answers(ok(200, "The run and its output.", Some(run_schema(true))))
            .answers(fail(404, "the machine has no such run"))
            .answers(plain(400, "`runID` or an offset is not a number: the framework's own rejection"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "write_machine_run_stdin",
                "Write To A Machine Run's Standard Input",
                "POST",
                "/machines/{name}/runs/{runID}/stdin",
            )
            .about(
                "Write text to the standard input of a run that is still going, and with \
                 `close` close it afterwards. A run started with `stdin` has had its input \
                 closed already. Nothing is returned; read the run for its effect. Not \
                 idempotent.",
            )
            .with(machine_param())
            .with(path_param("runID", "The run's number", json!({ "type": "integer", "minimum": 1 })))
            .takes(
                json!({
                    "type": "object",
                    "properties": {
                        "data": { "type": "string", "default": "" },
                        "close": { "type": "boolean", "default": false }
                    }
                }),
                true,
            )
            .answers(ok(204, "Written.", None))
            .answers(body_rejection())
            .answers(fail(404, "the machine has no such run"))
            .answers(fail(409, "the run has ended, or the machine is not running or has no agent answering"))
            .answers(fail(502, "the guest could not take it (`writing to run N: ...`)"))
            .answers_all(access(node, true)),
            Spec::new(
                "signal_machine_run",
                "Signal A Machine's Run",
                "POST",
                "/machines/{name}/runs/{runID}/signal",
            )
            .about(
                "Send a signal to a run that is still going: a name with or without `SIG` \
                 (HUP, INT, QUIT, KILL, USR1, USR2, TERM, CONT, STOP) or a number from 1 to \
                 64. Nothing is returned; read the run to see it end.",
            )
            .with(machine_param())
            .with(path_param("runID", "The run's number", json!({ "type": "integer", "minimum": 1 })))
            .takes(
                json!({
                    "type": "object",
                    "required": ["signal"],
                    "properties": { "signal": { "type": "string", "description": "`INT`, `TERM`, `KILL` and the like, or a number" } }
                }),
                true,
            )
            .answers(ok(204, "Sent.", None))
            .answers(fail(400, "not a signal name or a number from 1 to 64"))
            .answers(body_rejection())
            .answers(fail(404, "the machine has no such run"))
            .answers(fail(409, "the run has ended, or the machine is not running or has no agent answering"))
            .answers(fail(502, "the guest could not deliver it"))
            .answers_all(access(node, true)),
        ]
    }

    fn machine_agent_specs(node: bool) -> Vec<Spec> {
        let prompt_id = || {
            path_param(
                "promptID",
                "The prompt's number, which is its run's",
                json!({ "type": "integer", "minimum": 1 }),
            )
        };
        vec![
            Spec::new(
                "list_machine_prompts",
                "List A Machine's Prompts",
                "GET",
                "/machines/{name}/prompts",
            )
            .about(
                "Every prompt kept for a machine, oldest first, without events. A prompt is \
                 a run of a coding agent, so it is kept and lost as runs are, and one whose \
                 run made room for a newer one is gone from the list. The prompt text is \
                 returned as it was given.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The prompts.",
                Some(json!({ "type": "array", "items": prompt_schema(false) })),
            ))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "start_machine_prompt",
                "Prompt A Coding Agent In A Machine",
                "POST",
                "/machines/{name}/prompts",
            )
            .about(
                "Run a coding agent that is already in a running machine, for one prompt: a \
                 run of `claude -p ... --output-format stream-json --verbose`, `codex exec \
                 --json ...` or the caller's own `command` (provider `custom`, a shell line \
                 that reads `$HM_PROMPT`), as `user` or root. The prompt reaches the agent in \
                 its environment, never on a command line. `env` is where the agent's key \
                 goes, not kept or returned; no key comes with the machine. `unattended` \
                 lets the agent act without asking, with the flag each calls dangerous \
                 (`--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox`), \
                 and is off unless given; Claude Code will not take it as root. An agent that \
                 is not installed ends with exit code 127. Needs the guest agent. Not \
                 idempotent. See the guardrails.",
            )
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "required": ["provider", "prompt"],
                    "properties": {
                        "provider": { "type": "string", "enum": ["claude", "codex", "custom"] },
                        "prompt": { "type": "string", "description": "at most 128 KiB" },
                        "model": { "type": "string" },
                        "resume": { "type": "string", "description": "an earlier prompt's `session`" },
                        "unattended": { "type": "boolean", "default": false },
                        "command": { "type": "string", "description": "only with provider `custom`" },
                        "cwd": { "type": "string" },
                        "user": { "type": "string", "description": "root if absent" },
                        "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "the agent's key and the like; not kept, not returned" }
                    }
                }),
                true,
            )
            .answers(ok(201, "The prompt, running.", Some(prompt_schema(false))))
            .answers(fail(
                400,
                "an empty or over-long prompt, model or resume; an unknown provider; \
                 `command` without `custom` or `custom` without it; `unattended` or `resume` \
                 with `custom`; `claude` unattended as root",
            ))
            .answers(body_rejection())
            .answers(no_agent("prompts are started"))
            .answers(fail(409, "the machine already has 256 runs still running"))
            .answers(fail(502, "the run could not be started"))
            .answers_all(access(node, true)),
            Spec::new(
                "get_machine_prompt",
                "Get A Machine's Prompt And Its Events",
                "GET",
                "/machines/{name}/prompts/{promptID}",
            )
            .about(
                "One prompt, and the events its agent has printed from byte `after` of its \
                 output on. An event is a line: a JSON object is given as `event`, any other \
                 line as `text`; half a line is not given until the rest arrives. `next` is \
                 the `after` to ask with next, and `session` is the agent's own session ID, \
                 for `resume`. The agent's standard error is not an event; it is the run's, \
                 under the runs route. Events are kept as printed and not scanned for secrets.",
            )
            .with(machine_param())
            .with(prompt_id())
            .with(query_param(
                "after",
                "Read events from this byte of the agent's output on: the `next` of the last answer",
                json!({ "type": "integer", "minimum": 0, "default": 0 }),
            ))
            .answers(ok(200, "The prompt and its events.", Some(prompt_schema(true))))
            .answers(fail(404, "the machine has no such prompt"))
            .answers(plain(400, "`promptID` or `after` is not a number: the framework's own rejection"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "interrupt_machine_prompt",
                "Interrupt A Machine's Prompt",
                "POST",
                "/machines/{name}/prompts/{promptID}/interrupt",
            )
            .about(
                "Interrupt the agent a prompt is running, as Ctrl-C would (SIGINT to its \
                 process, through the guest agent). Nothing is returned; read the prompt to \
                 see it end. A prompt that has already ended is refused, and so is one whose \
                 machine has stopped. Not idempotent in effect: a second signal may land on \
                 an agent that has caught the first.",
            )
            .with(machine_param())
            .with(prompt_id())
            .answers(ok(204, "Sent.", None))
            .answers(fail(404, "the machine has no such prompt"))
            .answers(fail(409, "the prompt has ended, or the machine is not running or has no agent answering"))
            .answers(fail(502, "the guest could not deliver it"))
            .answers_all(access(node, true)),
        ]
    }

    /// Conversations: the node serves them and the control plane does not
    /// route them.
    fn machine_conversation_specs() -> Vec<Spec> {
        let conversation = || {
            path_param(
                "conversation",
                "The conversation's number, or the name it was given",
                json!({ "type": "string" }),
            )
        };
        let node = true;
        let no_conversation = || fail(404, "the machine has no such conversation");
        vec![
            Spec::new(
                "list_machine_conversations",
                "List A Machine's Conversations",
                "GET",
                "/machines/{name}/conversations",
            )
            .about(
                "Every conversation of a machine: a coding agent talked to over many turns, \
                 kept on the node in memory and gone when the node restarts or the machine \
                 is deleted. Each is listed without its messages.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The conversations.",
                Some(json!({ "type": "array", "items": conversation_schema(false) })),
            ))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "create_machine_conversation",
                "Start A Conversation With A Coding Agent",
                "POST",
                "/machines/{name}/conversations",
            )
            .about(
                "Make a conversation with `claude` or `codex` in a machine. Nothing runs \
                 yet: the first message starts a turn. `provider`, `model`, `user`, `cwd` and \
                 `unattended` are the conversation's and the same for every turn; \
                 `unattended` carries the agent's dangerous flag and Claude Code refuses it \
                 as root. `from` names another conversation of this machine, by number or \
                 name, whose transcript (its newest 64 KiB) is put before the first message, \
                 so a conversation can carry on with another agent; what moves is what was \
                 said, not the agent's session. A machine keeps 64. Not idempotent.",
            )
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "required": ["provider"],
                    "properties": {
                        "provider": { "type": "string", "enum": ["claude", "codex"] },
                        "name": { "type": "string", "description": "letters, digits, - and _, up to 63, and not a number" },
                        "model": { "type": "string" },
                        "user": { "type": "string" },
                        "cwd": { "type": "string" },
                        "unattended": { "type": "boolean", "default": false },
                        "from": { "type": "string", "description": "a conversation of this machine to carry on from" }
                    }
                }),
                true,
            )
            .answers(ok(201, "The conversation, idle.", Some(conversation_schema(true))))
            .answers(fail(
                400,
                "a provider that is not claude or codex, an empty model, `claude` unattended \
                 as root, or an invalid name",
            ))
            .answers(body_rejection())
            .answers(fail(404, "no machine, or no conversation named by `from`"))
            .answers(fail(409, "the machine has 64 conversations already, or one of that name"))
            .answers_all(access(node, true)),
            Spec::new(
                "get_machine_conversation",
                "Get A Machine's Conversation",
                "GET",
                "/machines/{name}/conversations/{conversation}",
            )
            .about(
                "One conversation: its settings, whether a turn is running, its turns (which \
                 are prompts, read through the prompts routes), the agent's session and every \
                 message sent, with whether each steered a running turn.",
            )
            .with(machine_param())
            .with(conversation())
            .answers(ok(200, "The conversation.", Some(conversation_schema(true))))
            .answers(no_conversation())
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "delete_machine_conversation",
                "Delete A Machine's Conversation",
                "DELETE",
                "/machines/{name}/conversations/{conversation}",
            )
            .about(
                "Forget a conversation that has no turn running. Its turns remain as prompts \
                 and runs, with the prompt text and output they kept. One with a turn \
                 running or starting is refused; interrupt it first. The second call is 404.",
            )
            .with(machine_param())
            .with(conversation())
            .answers(ok(204, "Forgotten.", None))
            .answers(no_conversation())
            .answers(fail(409, "a turn is running or starting; interrupt it first"))
            .answers_all(access(node, true)),
            Spec::new(
                "send_machine_conversation_message",
                "Say Something In A Conversation",
                "POST",
                "/machines/{name}/conversations/{conversation}/messages",
            )
            .about(
                "Send a message. With no turn running it starts one (201): a prompt that \
                 carries the agent's session on, with this message's `env` (the agent's key; \
                 not kept or returned). With a turn of Claude Code's running it goes into \
                 that turn's standard input and steers it (202); a steering message is given \
                 the environment its turn started with. Codex takes a turn at a time, so a \
                 message while one runs is refused (409): interrupt it or wait. The prompt \
                 is at most 128 KiB. This runs the agent with the conversation's `user` and \
                 `unattended` setting. Not idempotent.",
            )
            .with(machine_param())
            .with(conversation())
            .takes(
                json!({
                    "type": "object",
                    "required": ["prompt"],
                    "properties": {
                        "prompt": { "type": "string" },
                        "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "for a turn this message starts; not kept, not returned" }
                    }
                }),
                true,
            )
            .answers(ok(
                201,
                "A turn was started.",
                Some(json!({ "type": "object", "properties": {
                    "conversationID": { "type": "integer" },
                    "promptID": { "type": "integer" },
                    "steered": { "type": "boolean", "const": false } } })),
            ))
            .answers(ok(
                202,
                "The message went into the running turn.",
                Some(json!({ "type": "object", "properties": {
                    "conversationID": { "type": "integer" },
                    "promptID": { "type": "integer" },
                    "steered": { "type": "boolean", "const": true } } })),
            ))
            .answers(fail(400, "an empty or over-long prompt"))
            .answers(body_rejection())
            .answers(no_conversation())
            .answers(fail(
                409,
                "another message is starting a turn; a Codex turn is running; the machine is \
                 not running or has no agent answering; or it already has 256 runs running",
            ))
            .answers(fail(
                502,
                "the turn could not be started, or it was started and its message could not \
                 be written (the message says why it ended)",
            ))
            .answers_all(access(node, true)),
            Spec::new(
                "get_machine_conversation_transcript",
                "Read A Conversation's Transcript",
                "GET",
                "/machines/{name}/conversations/{conversation}/transcript",
            )
            .about(
                "What was said, by whom, in order: the caller's messages and the agent's \
                 replies, read from the agent's own events. Tool calls and their results \
                 are in the turns' events and not here. A turn whose run made room for newer \
                 ones is left out.",
            )
            .with(machine_param())
            .with(conversation())
            .answers(ok(
                200,
                "The lines.",
                Some(json!({ "type": "array", "items": { "type": "object", "properties": {
                    "role": { "type": "string", "description": "`user` or the agent's" },
                    "text": { "type": "string" } } } })),
            ))
            .answers(no_conversation())
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "interrupt_machine_conversation",
                "Interrupt A Conversation's Turn",
                "POST",
                "/machines/{name}/conversations/{conversation}/interrupt",
            )
            .about(
                "Interrupt the turn that is running, as Ctrl-C would (SIGINT to the agent's \
                 process, through the guest agent). Nothing is returned; read the \
                 conversation to see the turn end. A conversation with no turn running is \
                 refused (409).",
            )
            .with(machine_param())
            .with(conversation())
            .answers(ok(204, "Sent.", None))
            .answers(no_conversation())
            .answers(fail(409, "the conversation has no turn running"))
            .answers(fail(502, "the guest could not deliver it"))
            .answers_all(access(node, true)),
        ]
    }

    /// The desktop: node only.
    fn machine_desktop_specs() -> Vec<Spec> {
        let node = true;
        let no_desktop = || {
            fail(
                404,
                "no such machine, or it was not created with a `desktop` (from an image with \
                 `guestAgent`)",
            )
        };
        let desktop_not_ready = || {
            fail(
                409,
                "the machine is not running, or has no guest agent answering yet",
            )
        };
        vec![
            Spec::new(
                "get_machine_desktop",
                "Describe A Machine's Desktop",
                "GET",
                "/machines/{name}/desktop",
            )
            .about(
                "The desktop a machine was created with: a virtual display `:1` drawn in the \
                 guest's memory by Xvfb with a window manager and a terminal, noVNC on guest \
                 port 6080 at `/vnc.html`, and computer use for the coding agents installed \
                 with it. `ready` says whether the first-boot install has finished (it needs \
                 a network); `log` is the guest file that says what it did. noVNC has no \
                 password: see the guardrails before publishing its port.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The desktop.",
                Some(json!({ "type": "object", "properties": {
                    "width": { "type": "integer" }, "height": { "type": "integer" },
                    "display": { "type": "string" }, "ready": { "type": "boolean" },
                    "webPort": { "type": "integer" }, "webPath": { "type": "string" },
                    "log": { "type": "string" } } })),
            ))
            .answers(no_desktop())
            .answers(desktop_not_ready())
            .answers_all(access(node, false))
            .repeatable(),
            Spec::new(
                "get_machine_desktop_screenshot",
                "Take A Screenshot Of A Machine's Desktop",
                "GET",
                "/machines/{name}/desktop/screenshot",
            )
            .about(
                "A PNG of the whole screen, up to 32 MiB, taken in the guest and read out \
                 through its agent; the picture is removed from the guest afterwards. The \
                 screen can show anything the guest displays, credentials included.",
            )
            .with(machine_param())
            .answers(ok(200, "A PNG.", Some(json!({ "type": "string", "format": "binary" }))))
            .answers(no_desktop())
            .answers(desktop_not_ready())
            .answers(fail(502, "the picture could not be taken or read"))
            .answers(fail(503, "the desktop did not answer: it may not be installed yet"))
            .answers_all(access(node, false))
            .repeatable(),
            Spec::new(
                "act_on_machine_desktop",
                "Act On A Machine's Desktop",
                "POST",
                "/machines/{name}/desktop/actions",
            )
            .about(
                "Do one thing at the desktop: move the pointer, click, drag, type text (up to \
                 4096 characters, as keystrokes), press keys or chords, scroll, or ask where \
                 the pointer is. The node acts as root on the display. Answers with where the \
                 pointer is afterwards. A point off the screen, a key that is not a name and \
                 an action that is not one are refused (400). This drives whatever is on the \
                 screen with the user's own authority there. Not idempotent.",
            )
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "required": ["action"],
                    "properties": {
                        "action": { "type": "string", "enum": ["move", "click", "drag", "type", "key", "scroll", "cursor"] },
                        "x": { "type": "integer" }, "y": { "type": "integer" },
                        "toX": { "type": "integer" }, "toY": { "type": "integer" },
                        "button": { "type": "string", "enum": ["left", "middle", "right"] },
                        "count": { "type": "integer", "minimum": 1, "maximum": 3 },
                        "text": { "type": "string", "maxLength": 4096 },
                        "keys": { "type": "string", "description": "X key names or chords separated by spaces, such as `Return` or `ctrl+shift+t`" },
                        "direction": { "type": "string", "enum": ["up", "down", "left", "right"] },
                        "amount": { "type": "integer" }
                    }
                }),
                true,
            )
            .answers(ok(
                200,
                "Done.",
                Some(json!({ "type": "object", "properties": {
                    "cursor": { "type": "array", "items": { "type": "integer" }, "description": "[x, y]" } } })),
            ))
            .answers(fail(400, "the body is not a JSON object, or the action is refused: a point off the screen, an unknown key or action"))
            .answers(body_rejection())
            .answers(no_desktop())
            .answers(desktop_not_ready())
            .answers(fail(502, "the desktop's answer was not understood"))
            .answers(fail(503, "the desktop did not answer: it may not be installed yet"))
            .answers_all(access(node, false)),
        ]
    }

    fn machine_port_specs(node: bool) -> Vec<Spec> {
        vec![
            Spec::new(
                "list_machine_ports",
                "List A Machine's Published Ports",
                "GET",
                "/machines/{name}/ports",
            )
            .about(
                "The ports of the node that carry connections to ports of a machine's guest. \
                 They are kept with the machine and listened on for as long as they are \
                 published, whether the machine runs or not.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The published ports.",
                Some(json!({ "type": "array", "items": port_schema() })),
            ))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "publish_machine_port",
                "Publish A Machine's Port",
                "POST",
                "/machines/{name}/ports",
            )
            .about(if node {
                "Give a port of the guest one of this node's: a connection to the node's port \
                 is carried through the guest agent to the port on the guest's loopback, over \
                 TCP, with no TLS, HTTP or login added. The operator decides the range \
                 (`--machine-ports`, 20000-29999) and the address (`--machine-port-address`, \
                 loopback unless said); a caller picks `hostPort` inside the range or lets the \
                 node pick. At most 16 a machine, and the machine must have an agent. With \
                 `proxy` the port is also reached by name through the node's sandbox proxy, \
                 `{guestPort}-{machineID}.{domain}`, which has no login of its own. While the \
                 machine is stopped, or nothing listens, a connection is accepted and closed. \
                 Not idempotent."
            } else {
                "Give a port of the guest one of the holding node's, through the node's own \
                 handler (see `node_publish_machine_port`): TCP, carried through the guest \
                 agent, with no TLS, HTTP or login added. The port is on the machine's node at \
                 that node's address and within its operator's range, not on the control \
                 plane. With `proxy` it is also reached by name through the node's proxy and, \
                 where the control plane's web access is configured, behind its login. At most \
                 16 a machine. Not idempotent."
            })
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "required": ["guestPort"],
                    "properties": {
                        "guestPort": { "type": "integer", "minimum": 1, "maximum": 65535 },
                        "hostPort": { "type": "integer", "description": "a port of the node inside its `--machine-ports` range; the node chooses if absent" },
                        "proxy": { "type": "boolean", "default": false, "description": "also route it through the node's sandbox proxy by name" }
                    }
                }),
                true,
            )
            .answers(ok(201, "Published.", Some(port_schema())))
            .answers(fail(
                400,
                "`guestPort` is 0; the machine was made from an image without `guestAgent`; \
                 it has 16 published ports; or `hostPort` is outside the node's range. Unknown \
                 fields are refused (plain 422).",
            ))
            .answers(body_rejection())
            .answers(fail(
                409,
                "the node's port is in use, or every port in its range is",
            ))
            .answers(fail(500, "the node could not listen, or could not save the record"))
            .answers_all(access(node, true)),
            Spec::new(
                "unpublish_machine_port",
                "Take Back A Machine's Published Port",
                "DELETE",
                "/machines/{name}/ports/{hostPort}",
            )
            .about(
                "Stop listening on a published port and forget it, so that it is not \
                 listened on again when the node restarts. New connections stop at once; \
                 one already carried is the guest's to end. The second call is 404.",
            )
            .with(machine_param())
            .with(path_param(
                "hostPort",
                "The node's port, as published",
                json!({ "type": "integer", "minimum": 1, "maximum": 65535 }),
            ))
            .answers(ok(204, "Taken back.", None))
            .answers(fail(404, "the machine has no such published port"))
            .answers(plain(400, "`hostPort` is not a port number: the framework's own rejection"))
            .answers(fail(500, "the record could not be saved"))
            .answers_all(access(node, true)),
            Spec::new(
                "open_machine_reverse_port",
                "Receive A Connection A Machine's Guest Makes",
                "GET",
                "/machines/{name}/ports/{hostPort}/reverse",
            )
            .about(
                "Upgrade the request to `hv2-reverse/1` (a bodyless HTTP/1.1 GET with \
                 `Connection: Upgrade` and `Upgrade: hv2-reverse/1`) and carry one connection \
                 a program in the guest makes to guest port `{hostPort}` out to the caller. \
                 The node asks the guest agent to listen on that port, answers 101, and the \
                 stream's first byte marks that a program connected, its bytes following. \
                 The path's number is the guest's port. A machine has at most 64 connections \
                 waiting. Needs the guest agent.",
            )
            .with(machine_param())
            .with(path_param(
                "hostPort",
                "The guest port to listen on (named hostPort by the route; it is the guest's)",
                json!({ "type": "integer", "minimum": 1, "maximum": 65535 }),
            ))
            .answers(plain(101, "Switching protocols: the tunnel is open."))
            .answers(fail(
                400,
                "the request is not that upgrade, or the port is 0",
            ))
            .answers(fail(
                409,
                "the machine is not running or has no guest agent answering",
            ))
            .answers(fail(429, "64 connections are waiting for this machine already"))
            .answers(fail(
                502,
                "the guest could not listen on the port, or a guest image built before \
                 reverse ports has an agent that cannot",
            ))
            .answers_all(access(node, true)),
        ]
    }

    fn machine_snapshot_specs(node: bool) -> Vec<Spec> {
        let snapshot = || {
            path_param(
                "snapshotID",
                "The snapshot's ID, `snap-` and 15 hex digits",
                json!({ "type": "string" }),
            )
        };
        let no_snapshot = || fail(404, "the machine has no such snapshot");
        let partition = || {
            query_param(
                "partition",
                "The partition (1-based) whose ext filesystem is read; else the whole disk if \
                 it is one, or the largest ext filesystem",
                json!({ "type": "integer", "minimum": 1 }),
            )
        };
        let read_failures = || {
            vec![
                plain(
                    400,
                    "an unknown query parameter (the framework's own rejection), a path that \
                     is not absolute or has a `.` or `..`, something that is not a directory \
                     or file as the route needs, a symbolic link (listed, never followed), or \
                     a disk with no ext2, ext3 or ext4 filesystem (XFS and Btrfs are not read)",
                ),
                fail(404, "no such path in the snapshot, or no such snapshot"),
                fail(500, "the disk could not be read"),
            ]
        };
        vec![
            Spec::new(
                "list_machine_snapshots",
                "List A Machine's Disk Snapshots",
                "GET",
                "/machines/{name}/snapshots",
            )
            .about(
                "The snapshots of a machine's root disk, oldest first. A machine's snapshot \
                 is its stopped disk, copied and kept beside it: files, not memory, and not \
                 the cloud-init seed.",
            )
            .with(machine_param())
            .answers(ok(
                200,
                "The snapshots.",
                Some(json!({ "type": "array", "items": disk_snapshot_schema() })),
            ))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "take_machine_snapshot",
                "Snapshot A Stopped Machine's Disk",
                "POST",
                "/machines/{name}/snapshots",
            )
            .about(
                "Copy a stopped machine's root disk and keep it beside the machine, with \
                 `cp --sparse=always --reflink=auto`: on XFS or Btrfs it shares blocks until \
                 they change, elsewhere it costs what the disk holds; `bytesOnNode` says \
                 which. A running machine is refused (409), since a disk copied under a \
                 running guest is caught half-written. At most 32 a machine. The snapshot \
                 holds every file on the disk, secrets included, and is read back in full by \
                 the disk route. Unknown fields are refused. Not idempotent.",
            )
            .with(machine_param())
            .takes(
                json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "maxLength": 128, "description": "no control characters or space at either end; the time if absent" } }
                }),
                false,
            )
            .answers(ok(201, "The snapshot.", Some(disk_snapshot_schema())))
            .answers(fail(400, "the name is not 1 to 128 characters of text"))
            .answers(body_rejection())
            .answers(fail(
                409,
                "the machine is running (stop it first), or it already has 32 snapshots",
            ))
            .answers(fail(500, "the disk could not be copied"))
            .answers_all(access(node, true)),
            Spec::new(
                "get_machine_snapshot",
                "Get A Machine's Disk Snapshot",
                "GET",
                "/machines/{name}/snapshots/{snapshotID}",
                )
            .about(
                "One snapshot of a machine's disk: its name, when it was taken, the disk's \
                 size then (which a restore gives the machine back) and what it holds on the \
                 node's disk, which is not its length. It does not read the disk: use the \
                 files, file, archive or disk routes for that.",
            )
            .with(machine_param())
            .with(snapshot())
            .answers(ok(200, "The snapshot.", Some(disk_snapshot_schema())))
            .answers(no_snapshot())
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "delete_machine_snapshot",
                "Delete A Machine's Disk Snapshot",
                "DELETE",
                "/machines/{name}/snapshots/{snapshotID}",
            )
            .about(
                "Remove a snapshot of a machine's disk and free what it holds on the node, \
                 which can be done while the machine runs. Deleting the machine removes its \
                 snapshots with it. The second call is 404.",
            )
            .with(machine_param())
            .with(snapshot())
            .answers(ok(204, "Removed.", None))
            .answers(no_snapshot())
            .answers(fail(500, "it could not be removed"))
            .answers_all(access(node, true)),
            Spec::new(
                "restore_machine_snapshot",
                "Restore A Machine's Disk From A Snapshot",
                "POST",
                "/machines/{name}/snapshots/{snapshotID}/restore",
            )
            .about(
                "Put a snapshot back over a stopped machine's root disk, and the disk's size \
                 with it (a snapshot taken before a resize gives the machine its earlier \
                 size). The copy is made beside the disk and renamed over it, so a restore \
                 that fails leaves the disk as it was. Everything written to the disk since \
                 is lost. A running machine is refused (409). The cloud-init seed is left as \
                 it is. Needs no body.",
            )
            .with(machine_param())
            .with(snapshot())
            .answers(ok(200, "The machine, with the disk's old size.", Some(machine_schema(node))))
            .answers(no_snapshot())
            .answers(fail(409, "the machine is running; stop it first"))
            .answers(fail(500, "the copy failed; the disk is as it was"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "download_machine_snapshot_disk",
                "Download A Machine's Snapshot As A Raw Disk",
                "GET",
                "/machines/{name}/snapshots/{snapshotID}/disk",
            )
            .about(
                "Stream the whole snapshot as one raw disk image, as long as the disk is: \
                 its holes come as zeros, so a download is as large as the disk. It is the \
                 image the guest boots, so every file on it, secrets and keys included, \
                 can be read with `debugfs` or a loop mount without booting anything.",
            )
            .with(machine_param())
            .with(snapshot())
            .answers(ok(
                200,
                "The raw disk, as `application/octet-stream` with a `content-disposition` \
                 naming `{name}-{snapshotID}.img`. Through the control plane the content type is \
                 kept and `content-length` and `content-disposition` are not.",
                Some(json!({ "type": "string", "format": "binary" })),
            ))
            .answers(no_snapshot())
            .answers(fail(500, "the snapshot's disk could not be opened"))
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "list_machine_snapshot_files",
                "List A Directory In A Machine's Snapshot",
                "GET",
                "/machines/{name}/snapshots/{snapshotID}/files",
            )
            .about(
                "List a directory of a snapshot's ext2, ext3 or ext4 filesystem without \
                 booting it, read with `debugfs`, which changes nothing: a template \
                 machine's disk, or, in a disk image, the partition named by `partition`, \
                 else the largest ext one. A path is walked a name at a time, so it is never \
                 read as a command. Symbolic links are listed, not followed. `path` is `/` \
                 by default. Unknown query parameters are refused.",
            )
            .with(machine_param())
            .with(snapshot())
            .with(query_param("path", "An absolute directory path (default `/`)", json!({ "type": "string" })))
            .with(partition())
            .answers(ok(
                200,
                "The entries.",
                Some(json!({ "type": "object", "properties": {
                    "path": { "type": "string" },
                    "partition": { "type": ["integer", "null"] },
                    "entries": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" },
                        "type": { "type": "string", "enum": ["directory", "file", "symlink", "other"] },
                        "mode": { "type": "string", "description": "permission bits in octal" },
                        "uid": { "type": "integer" }, "gid": { "type": "integer" },
                        "size": { "type": ["integer", "null"], "description": "null for a directory" } } } } } })),
            ))
            .answers_all(read_failures())
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "read_machine_snapshot_file",
                "Read A File From A Machine's Snapshot",
                "GET",
                "/machines/{name}/snapshots/{snapshotID}/file",
            )
            .about(
                "One regular file out of a snapshot's ext filesystem, without booting it, \
                 up to 64 MiB, as `application/octet-stream`. A directory, a symbolic link \
                 (never followed) or any other kind is refused, and so is a file over the \
                 limit (413; download the disk).",
            )
            .with(machine_param())
            .with(snapshot())
            .with(required_query("path", "The file's absolute path", json!({ "type": "string" })))
            .with(partition())
            .answers(ok(200, "The file's bytes.", Some(json!({ "type": "string", "format": "binary" }))))
            .answers(fail(413, "the file is over 64 MiB"))
            .answers_all(read_failures())
            .answers_all(access(node, true))
            .repeatable(),
            Spec::new(
                "archive_machine_snapshot_directory",
                "Download A Directory Of A Machine's Snapshot As A Tar",
                "GET",
                "/machines/{name}/snapshots/{snapshotID}/archive",
            )
            .about(
                "A directory of a snapshot and everything below it as a gzipped tar (`/` for \
                 the whole disk), dumped with `debugfs` into a scratch directory of the \
                 node's own, packed with `tar` and `gzip`, and removed. Symbolic links are \
                 archived as links; sockets, devices and pipes are left out. At most 64 MiB \
                 of archive, and 256 MiB or 500,000 entries read from the disk before it is \
                 stopped (413); a build over 3 minutes is stopped (504). A snapshot of a \
                 machine stopped without unmounting may have a filesystem `debugfs` reads \
                 as the journal had not replayed it.",
            )
            .with(machine_param())
            .with(snapshot())
            .with(query_param("path", "An absolute directory path (default `/`)", json!({ "type": "string" })))
            .with(partition())
            .answers(ok(
                200,
                "The archive, `application/gzip`. Through the control plane `content-disposition` is not kept.",
                Some(json!({ "type": "string", "format": "binary" })),
            ))
            .answers(fail(413, "over 64 MiB of archive, 256 MiB read or 500,000 entries"))
            .answers(fail(504, "the build took over 3 minutes"))
            .answers_all(read_failures())
            .answers_all(access(node, true))
            .repeatable(),
        ]
    }
}
