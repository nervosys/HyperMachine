//! `hv2-api`'s routes that run code or decide what may run: a guest's console
//! and `exec` (`rest.rs`), the boot image allowlist (`image_registry_routes.rs`)
//! and the agent runtime (`agent_runtime_routes.rs`). Modelled as data from
//! the handlers; each is served by this crate's own server, behind its API key
//! middleware when one is configured and open otherwise.

use super::spec::{
    api_fail, api_key_failures, body_rejection, json_content_type_required, ok, path_param, plain,
    query_param, Spec,
};
use super::{HyperMachineOntology, Operation, OperationResponse, Parameter, ParameterLocation};
use serde_json::{json, Value};

fn vm_id_parameter() -> Parameter {
    path_param(
        "id",
        "The VM's name: `create_vm` keys the server's inventory by `name`",
        json!({ "type": "string" }),
    )
}

fn image_reference() -> Parameter {
    path_param(
        "reference",
        "The image's reference exactly as it was registered. It is one path segment, so a \
         reference with a `/` in it must be percent-encoded",
        json!({ "type": "string" }),
    )
}

fn tenant_header() -> Parameter {
    Parameter {
        name: "X-Tenant-Id".to_string(),
        location: ParameterLocation::Header,
        required: false,
        description: "The tenant the request acts for. When the key is bound to a tenant, that tenant is used, and a different value is 403 `TENANT_MISMATCH`. Otherwise the caller chooses it: nothing checks \
                      it against a credential. Absent or empty is `default`"
            .to_string(),
        schema: json!({ "type": "string" }),
        default: Some(json!("default")),
    }
}

fn image_schema() -> Value {
    json!({ "type": "object", "properties": {
        "reference": { "type": "string" },
        "kind": { "type": "string", "description": "vm_image, container, kernel, initramfs, firmware, or the custom string registered" },
        "sha256": { "type": "string" },
        "size_bytes": { "type": "integer" },
        "status": { "type": "string", "enum": ["pending_review", "approved", "denied", "deprecated", "revoked"] },
        "labels": { "type": "object", "additionalProperties": { "type": "string" } },
        "signature_count": { "type": "integer" },
        "has_verified_signature": { "type": "boolean", "description": "always false for an image registered through this API" },
        "notes": { "type": "string", "description": "the reason last given to deny, revoke or deprecate" }
    } })
}

fn review_body() -> Value {
    json!({
        "type": "object",
        "required": ["reviewer"],
        "properties": {
            "reviewer": { "type": "string", "description": "free text the caller supplies; not the authenticated identity" },
            "reason": { "type": "string", "default": "", "description": "kept as the image's notes by deny, revoke and deprecate; ignored by approve" }
        }
    })
}

impl HyperMachineOntology {
    /// The console, exec, image registry and agent runtime routes.
    pub(super) fn build_api_privileged_operations() -> Vec<Operation> {
        let keys = api_key_failures;
        let not_found_vm = || api_fail(404, "the server holds no VM of that name (`VM_NOT_FOUND`)");
        let no_image = || {
            api_fail(
                404,
                "no image is registered under that reference (`NOT_FOUND`, or the review's own \
                 code)",
            )
        };

        let specs = vec![
            Spec::new(
                "get_vm_console",
                "Read A VM's Console",
                "GET",
                "/api/v1/vms/{id}/console",
            )
            .about(
                "Read what the guest has written to its console, without consuming it: every \
                 call returns the whole buffer (at most 1 MiB per console device, the oldest \
                 bytes dropped first, decoded lossily as UTF-8), so poll and compare rather than \
                 expect only what is new. `attached: false` means the VM has no console device, \
                 which is not the same as a guest that has printed nothing (`attached: true` and \
                 an empty `output`). Nothing in this server attaches a serial device when it \
                 creates a VM, so a VM made by `create_vm` reports `attached: false` unless \
                 something else registered a console on it. The VM must be one this server \
                 holds; it need not be running. Read only. What comes back is whatever the \
                 guest chose to print, boot messages and any secret it echoed included. Open \
                 unless an API key is configured.",
            )
            .with(vm_id_parameter())
            .answers(ok(
                200,
                "The console buffer.",
                Some(json!({ "type": "object", "properties": {
                    "id": { "type": "string" },
                    "attached": { "type": "boolean" },
                    "output": { "type": "string" } } })),
            ))
            .answers(not_found_vm())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "exec_in_vm",
                "Run A Program In A VM's Guest",
                "POST",
                "/api/v1/vms/{id}/exec",
            )
            .about(
                "Run a program inside the guest operating system and wait for it to finish, \
                 through `hv2-guest-agentd` over virtio-vsock: `program` is run directly with \
                 `args` already split (not through a shell), as the user the guest agent runs as, \
                 and the agent authenticates nobody: the vsock channel is the boundary. A \
                 non-zero exit is still a 200 carrying that code; a program killed by a signal \
                 has `signal` set and a null `exit_code`; each of stdout and stderr is capped at \
                 1 MiB (`truncated`), and the agent kills an overrunning program (`timed_out`). \
                 `timeout_seconds` defaults to 30, is at least 1, and has no maximum. Anything \
                 that stops the command from running at all is a 503 `GUEST_UNAVAILABLE` whose \
                 message says which: the VM lacks the `GuestExec` capability, no guest channel \
                 was attached before it booted, or nothing in the guest answered. For a VM \
                 made by `create_vm` that is always the first or second: the server builds it \
                 with the default capability set (VmRead and Metrics) and attaches no vsock \
                 channel, so this route answers 503 for every VM created through this API; it \
                 can succeed only for a VM an embedding program built with `GuestExec` and a \
                 channel and put in the server's inventory. This is not `execute_script`, which \
                 evaluates a Rhai script on the host and runs nothing in the guest. Open unless \
                 an API key is configured, and then an operator or admin key may run any program in any VM the \
                 server holds. Not idempotent.",
            )
            .with(vm_id_parameter())
            .takes(
                json!({
                    "type": "object",
                    "required": ["program"],
                    "properties": {
                        "program": { "type": "string", "description": "not empty after trimming; a path or a name the guest resolves" },
                        "args": { "type": "array", "items": { "type": "string" }, "default": [] },
                        "timeout_seconds": { "type": "integer", "minimum": 1, "default": 30 }
                    }
                }),
                true,
            )
            .answers(ok(
                200,
                "What the program did, whatever its exit code.",
                Some(json!({ "type": "object", "properties": {
                    "id": { "type": "string" },
                    "exit_code": { "type": ["integer", "null"] },
                    "signal": { "type": ["integer", "null"] },
                    "stdout": { "type": "string" },
                    "stderr": { "type": "string" },
                    "truncated": { "type": "boolean" },
                    "timed_out": { "type": "boolean" } } })),
            ))
            .answers(api_fail(400, "`program` is empty (`INVALID_REQUEST`)"))
            .answers(body_rejection())
            .answers(not_found_vm())
            .answers(api_fail(
                503,
                "the command could not be run: no `GuestExec` capability, no guest channel, or \
                 no answer from the guest (`GUEST_UNAVAILABLE`)",
            ))
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new("list_images", "List Boot Images", "GET", "/api/v1/images")
                .about(
                    "List the boot images of the allowlist registry, filtered by `status` or by \
                     `kind`. With neither it does not list: it answers `{count, images: []}` \
                     with only the total in `count`. `status` wins when both are sent; an \
                     unknown status is 400 `INVALID_STATUS`, and every `kind` is accepted (a \
                     string other than the five named kinds is a custom kind). The registry \
                     lives in this process's memory, so every registration, approval and \
                     revocation is lost when the server restarts. These routes exist only while \
                     the runtime is enabled (the default). Read only. Open unless an API key \
                     is configured.",
                )
                .with(query_param(
                    "status",
                    "Only images in this state",
                    json!({ "type": "string", "enum": ["approved", "pending_review", "denied", "deprecated", "revoked"] }),
                ))
                .with(query_param(
                    "kind",
                    "Only images of this kind",
                    json!({ "type": "string" }),
                ))
                .answers(ok(
                    200,
                    "The matches; `images` is empty when no filter was sent.",
                    Some(json!({ "type": "object", "properties": {
                        "count": { "type": "integer" },
                        "images": { "type": "array", "items": image_schema() } } })),
                ))
                .answers(api_fail_registry(400, "`status` is not one of the five states (`INVALID_STATUS`)"))
                .answers_all(keys())
                .repeatable(),
            Spec::new("register_image", "Register A Boot Image", "POST", "/api/v1/images")
                .about(
                    "Add a boot image to the registry as `pending_review`: nothing is approved \
                     by registering it. `sha256` is what admission matches: when a VM is \
                     launched the registry is asked about the SHA-256 of the kernel (or raw \
                     image) about to be loaded, compared without regard to case; this route \
                     does not check that the string is 64 hex digits or that such an image \
                     exists, and an initrd is not looked at. `kind` takes any string. Each \
                     signature is recorded but stored with `verified: false` whatever the \
                     client sent, because nothing here checks one cryptographically, so \
                     `has_verified_signature` is false for every image registered through this \
                     API. The answer is 201 with no body; a reference already registered is 409 \
                     `REGISTER_FAILED`, and a registration cannot be edited or replaced. Not \
                     idempotent. Open unless an API key is configured, and then only an admin key may \
                     register.",
                )
                .takes(
                    json!({
                        "type": "object",
                        "required": ["reference", "kind", "sha256"],
                        "properties": {
                            "reference": { "type": "string" },
                            "kind": { "type": "string", "description": "vm_image, container, kernel, initramfs, firmware or any other string" },
                            "sha256": { "type": "string" },
                            "size_bytes": { "type": "integer", "default": 0 },
                            "labels": { "type": "object", "additionalProperties": { "type": "string" } },
                            "signatures": { "type": "array", "items": { "type": "object",
                                "required": ["signer", "algorithm", "signature_hex"],
                                "properties": {
                                    "signer": { "type": "string" },
                                    "algorithm": { "type": "string" },
                                    "signature_hex": { "type": "string" },
                                    "verified": { "type": "boolean", "description": "read and ignored: stored as false" } } } }
                        }
                    }),
                    true,
                )
                .answers(plain(201, "Registered, in `pending_review`. No body."))
                .answers(api_fail_registry(409, "the reference is already registered (`REGISTER_FAILED`)"))
                .answers(body_rejection())
                .answers(json_content_type_required())
                .answers_all(keys()),
            Spec::new(
                "get_image_registry_config",
                "Read The Image Registry's Configuration",
                "GET",
                "/api/v1/images/config",
            )
            .about(
                "Read the registry's enforcement mode and signature policy: `mode` is `audit`, \
                 `enforce` or `disabled`, `require_signature` says whether approval and \
                 admission need a verified signature, and `trusted_signers` limits whose \
                 signature counts (empty trusts any). This server starts the registry with \
                 `require_signature: true`, no trusted signers and the mode from \
                 `image_admission_mode` (`audit` by default). The mode is reported even when \
                 `enforce_image_admission` is off (the default), in which case VMs are not \
                 gated by the registry at all and the mode is a setting nothing consults. \
                 Read only. Open unless an API key is configured.",
            )
            .answers(ok(
                200,
                "The configuration.",
                Some(json!({ "type": "object", "properties": {
                    "mode": { "type": "string", "enum": ["audit", "enforce", "disabled"] },
                    "require_signature": { "type": "boolean" },
                    "trusted_signers": { "type": "array", "items": { "type": "string" } } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "check_image_admission",
                "Ask Whether An Image Is Admitted",
                "POST",
                "/api/v1/images/check-admission",
            )
            .about(
                "Ask the registry what it would decide for a reference: `allowed`, \
                 `allowed_with_warning` (a deprecated image, the reason in `message`) or \
                 `denied`. A denial is still a 200: read `decision`. With the mode `disabled` \
                 everything is allowed. A reference the registry does not hold is denied in \
                 `enforce` and allowed in `audit`. An `approved` image is denied when a \
                 verified signature is required and missing (always, for images registered \
                 through this API) or when trusted signers are set and none signed; `denied` \
                 and `revoked` images are denied in `audit` as well, and \
                 `pending_review` is denied only in `enforce`. This answers for a reference. \
                 The gate that stops a VM from booting asks about the SHA-256 digest of the \
                 image it is about to load, when the VM is launched, and only for VMs this \
                 server creates while `enforce_image_admission` is on; it writes no log of \
                 what `audit` let through. A POST that changes nothing, so repeatable. Open \
                 unless an API key is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["reference"],
                    "properties": { "reference": { "type": "string" } }
                }),
                true,
            )
            .answers(ok(
                200,
                "The decision.",
                Some(json!({ "type": "object", "properties": {
                    "reference": { "type": "string" },
                    "decision": { "type": "string", "enum": ["allowed", "allowed_with_warning", "denied"] },
                    "message": { "type": "string", "description": "present for allowed_with_warning and denied" } } })),
            ))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_image",
                "Read A Boot Image",
                "GET",
                "/api/v1/images/{reference}",
            )
            .about(
                "Read one registered image: its kind, digest, size, labels, status, how many \
                 signatures it carries (and whether any is verified, which is never so for an \
                 image registered through this API) and the notes from its last deny, revoke or \
                 deprecation. Who reviewed it and when are kept but not returned. Read only. \
                 Open unless an API key is configured.",
            )
            .with(image_reference())
            .answers(ok(200, "The image.", Some(image_schema())))
            .answers(no_image())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "approve_image",
                "Approve A Boot Image",
                "POST",
                "/api/v1/images/{reference}/approve",
            )
            .about(
                "Mark an image `approved` and record the reviewer. A denied or revoked image is refused with 409 `APPROVE_FAILED`: those decisions are final for the reference, and admission by digest refuses a digest that any denied or revoked entry carries. Otherwise, when the registry requires a signature, which this server's does (`require_signature: true`), approval is refused with 422 `APPROVE_FAILED` unless the image has a verified signature. This API never records one as verified, so on this server approving an image registered through it fails every time, and under `enforce` such an image stays unadmitted. A pending, approved or deprecated image can be approved; no other state is checked. `reviewer` is free text the caller sends, not the authenticated identity, and only the last reviewer is kept. `reason` is ignored. The answer is 200 with no body. Repeatable. Open unless an API key is configured, and then only an admin key may approve.",
            )
            .with(image_reference())
            .takes(review_body(), true)
            .answers(plain(200, "Approved. No body."))
            .answers(api_fail_registry(404, "no such image (`APPROVE_FAILED`)"))
            .answers(api_fail_registry(409, "the image is denied or revoked, which approval refuses (`APPROVE_FAILED`)"))
            .answers(api_fail_registry(
                422,
                "approval needs a verified signature and the image has none (`APPROVE_FAILED`)",
            ))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "deny_image",
                "Deny A Boot Image",
                "POST",
                "/api/v1/images/{reference}/deny",
            )
            .about(
                "Mark an image `denied`, keeping `reason` as its notes (replacing any earlier note) and the reviewer. It works on an image in any state, an approved one included, and the decision is final for the reference: `approve_image` refuses it with 409. A denied image is refused at launch in `audit` as well as `enforce` (only `disabled` admits everything); the refusal is an error from the VM's launch, which does not touch a VM already running from that image. The answer is 200 with no body; the only failure is an unknown reference. Repeatable. Open unless an API key is configured, and then only an admin key may deny.",
            )
            .with(image_reference())
            .takes(review_body(), true)
            .answers(plain(200, "Denied. No body."))
            .answers(api_fail_registry(404, "no such image (`DENY_FAILED`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "revoke_image",
                "Revoke A Boot Image",
                "POST",
                "/api/v1/images/{reference}/revoke",
            )
            .about(
                "Withdraw an image: mark it `revoked`, keeping `reason` as its notes and the reviewer. It needs no earlier approval and works on an image in any state. A revoked image is refused at launch in `audit` as well as `enforce`. Revocation takes effect for launches that happen after it; no VM is stopped, paused or altered, and no event is published, so a VM already running from the image keeps running. It is final for the reference: `approve_image` refuses it with 409, and admission by digest refuses the digest while any entry for it is denied or revoked. 200 with no body; the only failure is an unknown reference. Repeatable. Open unless an API key is configured, and then only an admin key may revoke.",
            )
            .with(image_reference())
            .takes(review_body(), true)
            .answers(plain(200, "Revoked. No body."))
            .answers(api_fail_registry(404, "no such image (`REVOKE_FAILED`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "deprecate_image",
                "Deprecate A Boot Image",
                "POST",
                "/api/v1/images/{reference}/deprecate",
            )
            .about(
                "Mark an image `deprecated`, keeping `reason` as its notes and the reviewer. Only an `approved` or already `deprecated` image can be deprecated; any other state is refused with 409 `DEPRECATE_FAILED`. A deprecated image passes the same signature gate as an approved one, and an image that passes is admitted with a warning: `check_image_admission` answers `allowed_with_warning`, and a launch logs the warning and proceeds. An image that fails the gate is refused, so deprecation does not get around it: on this server, which records no verified signature, a deprecated image is refused under `enforce` too. 200 with no body; an unknown reference is 404. Repeatable. Open unless an API key is configured, and then only an admin key may deprecate.",
            )
            .with(image_reference())
            .takes(review_body(), true)
            .answers(plain(200, "Deprecated. No body."))
            .answers(api_fail_registry(404, "no such image (`DEPRECATE_FAILED`)"))
            .answers(api_fail_registry(409, "the image is not approved or deprecated (`DEPRECATE_FAILED`)"))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new("spawn_agent", "Spawn An Agent", "POST", "/api/v1/agents")
                .about(
                    "Spawn an agent in the server's agent runtime for a tenant: an MCP session \
                     with the chosen capability set, and a copy-on-write memory sandbox taken \
                     from a warm baseline. The baseline is a 1 MiB block of zeroes (the \
                     server's own comment calls it a placeholder for a real agent image, and \
                     nothing replaces it), no VM or process is started, and this API has no \
                     route that uses the session afterwards: only `list_agents`, \
                     `get_agent_fleet`, `reap_agents` and `release_agent` act on it. The \
                     capability sets are `operator` (the default: VM read, write and manage, \
                     metrics, snapshots), `full` (adds guest and host exec, network, storage, \
                     context memory, debug, coordination and admin), `read_only` (also \
                     `readonly`) and `none`; they are recorded on the session for the MCP \
                     dispatcher to check, and are not enforced by this route. The tenant is the \
                     `X-Tenant-Id` the caller sends, or the key's tenant when the key is bound to one (a different header is 403 `TENANT_MISMATCH`). At most 100 agents live at once: at the \
                     limit idle ones are reclaimed first, whichever tenant owns them, and then \
                     a 503 `Maximum sessions reached`. Not idempotent. Open unless an API key \
                     is configured: the per-route bearer check in this module is never given \
                     tokens by this server.",
                )
                .with(tenant_header())
                .takes(
                    json!({
                        "type": "object",
                        "required": ["agent_id"],
                        "properties": {
                            "agent_id": { "type": "string", "description": "caller's name for the agent; part of the session id" },
                            "capabilities": { "type": "string", "enum": ["full", "operator", "read_only", "readonly", "none"], "default": "operator" }
                        }
                    }),
                    true,
                )
                .answers(ok(
                    201,
                    "The agent.",
                    Some(json!({ "type": "object", "properties": {
                        "session_id": { "type": "string", "description": "`session-<agent_id>-<unique>`" },
                        "agent_id": { "type": "string" } } })),
                ))
                .answers(plain(400, "an unknown capability set, `{\"error\": \"unknown capabilities: ...\"}`"))
                .answers(plain(503, "the runtime is at its session limit and nothing was idle: `{\"error\": \"Maximum sessions reached\"}`"))
                .answers(body_rejection())
                .answers(json_content_type_required())
                .answers_all(keys()),
            Spec::new("list_agents", "List A Tenant's Agents", "GET", "/api/v1/agents")
                .about(
                    "List the session ids of the agents the requesting tenant spawned and that \
                     are still live (an agent reaped or released elsewhere drops out). `agents` \
                     holds session ids, not the `agent_id`s that were sent. The tenant is \
                     the key's tenant when the key is bound to one, otherwise whatever `X-Tenant-Id` the caller names, so a caller with no key or an unscoped key can list any \
                     tenant's agents. Read only. Open unless an API key is configured.",
                )
                .with(tenant_header())
                .answers(ok(
                    200,
                    "The tenant's agents.",
                    Some(json!({ "type": "object", "properties": {
                        "agents": { "type": "array", "items": { "type": "string" } },
                        "total": { "type": "integer" } } })),
                ))
                .answers_all(keys())
                .repeatable(),
            Spec::new(
                "get_agent_fleet",
                "Read The Agent Fleet's Size",
                "GET",
                "/api/v1/agents/fleet",
            )
            .about(
                "Read the tenant's live agent count and the fleet's memory: `baseline_bytes` \
                 is the one shared warm image, and `fleet_memory_bytes` the baseline plus every \
                 agent's private copied pages, across all tenants (not only the caller's). \
                 Read only. The tenant is the key's tenant when the key is bound to one, else the `X-Tenant-Id` the caller sends. Open unless an \
                 API key is configured.",
            )
            .with(tenant_header())
            .answers(ok(
                200,
                "The sizes.",
                Some(json!({ "type": "object", "properties": {
                    "live_agents": { "type": "integer", "description": "this tenant's" },
                    "baseline_bytes": { "type": "integer" },
                    "fleet_memory_bytes": { "type": "integer", "description": "every tenant's agents together" } } })),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new("reap_agents", "Reap Idle Agents", "POST", "/api/v1/agents/reap")
                .about(
                    "Release every agent that has been idle for at least `max_idle_secs`, and \
                     answer how many were. It is scoped to the requesting tenant: the tenant is the key's when the key is bound to one, else the `X-Tenant-Id` the caller sends (absent is `default`), and only that tenant's sessions are considered; other tenants' agents are never touched. `0` releases every agent of that tenant. Releasing drops the agent's sandbox. Repeatable in the sense that a second call finds fewer. Open unless an API key is configured, and then an operator or admin key may reap its own \
                     tenant's agents.",
                )
                .takes(
                    json!({
                        "type": "object",
                        "required": ["max_idle_secs"],
                        "properties": { "max_idle_secs": { "type": "integer", "minimum": 0 } }
                    }),
                    true,
                )
                .answers(ok(
                    200,
                    "How many were released.",
                    Some(json!({ "type": "object", "properties": { "reaped": { "type": "integer" } } })),
                ))
                .answers(body_rejection())
                .answers(json_content_type_required())
                .answers_all(keys())
                .repeatable(),
            Spec::new(
                "release_agent",
                "Release An Agent",
                "DELETE",
                "/api/v1/agents/{session_id}",
            )
            .about(
                "Release one agent by session id, ending its session and dropping its sandbox. \
                 Only the tenant recorded when it was spawned may release it: another tenant \
                 gets the same 404 as for an id that does not exist. The tenant is the \
                 `X-Tenant-Id` the caller sends (or the key's tenant, when the key is bound to one), so naming the owner is enough. Answers 204 \
                 with no body. Open unless an API key is configured.",
            )
            .with(path_param(
                "session_id",
                "The session id `spawn_agent` answered with",
                json!({ "type": "string" }),
            ))
            .with(tenant_header())
            .answers(plain(204, "Released. No body."))
            .answers(plain(404, "no such agent for this tenant: `{\"error\": \"agent not found\"}`"))
            .answers_all(keys()),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }
}

/// The registry's own error shape: `{error, code}`.
fn api_fail_registry(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: Some(json!({
            "type": "object",
            "properties": { "error": { "type": "string" }, "code": { "type": "string" } }
        })),
    }
}
