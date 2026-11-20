//! The HTTP servers `hm` starts on the local machine, which are not the
//! cluster control plane or a node: `hm serve` (`hm-cli`, `mcp_server.rs`),
//! `hm sandbox vm mcp-http` (`sandbox_vm_mcp_http.rs`) and `hm jobs serve`
//! (`hv2-jobs`, `http.rs`). Modelled as data from the handlers: `hv2-api`
//! depends on none of these crates.
//!
//! Operation ids of `hm serve` and the MCP HTTP transport start `hm_`, and
//! those of the jobs server `jobs_`; none of the three has a node or control
//! plane twin. `operation_component` maps the first set to `hm-cli` and the
//! second to `hv2-jobs`.

use super::spec::{body_rejection, ok, path_param, plain, query_param, Spec};
use super::{
    Capability, CapabilityCategory, Guardrail, GuardrailSeverity, HyperMachineOntology, Operation,
    OperationResponse, Parameter, ParameterLocation,
};
use serde_json::{json, Value};

// ----------------------------------------------------------------------------
// hm serve
// ----------------------------------------------------------------------------

/// The 429 the rate limiter, which wraps every route of `hm serve`, gives.
fn hm_rate_limited() -> OperationResponse {
    plain(
        429,
        "the caller's bucket has used its 100 requests in the current 60-second window \
         (empty body). The bucket is the request's socket peer address. `X-Forwarded-For` is \
         read only when that peer is a `--trusted-proxy` address, and then the bucket is the \
         rightmost entry that is not itself a trusted proxy; from any other peer the header is \
         ignored, so a caller cannot pick its own bucket. A missing or malformed entry falls \
         back to the peer address. Refused requests count against the bucket, and the limiter \
         runs before the key check, so unauthenticated requests spend it too",
    )
}

/// The 401 of a protected route of `hm serve` when `HM_API_KEY` is set.
fn hm_unauthorized() -> OperationResponse {
    plain(
        401,
        "the server was started with `HM_API_KEY` and the request has no `Authorization: \
         Bearer <key>` header, or the token is not the key (empty body). Returned on every \
         route except GET /health, and only while `HM_API_KEY` is set. Never returned when \
         `HM_API_KEY` is unset, which is the default",
    )
}

fn hm_vm_name() -> Parameter {
    path_param(
        "name",
        "The VM's name, as given when it was created",
        json!({ "type": "string" }),
    )
}

fn hm_vm_info_schema() -> Value {
    json!({ "type": "object", "properties": {
        "name": { "type": "string" },
        "cpu_cores": { "type": "integer" },
        "memory_gb": { "type": "integer" },
        "gpu_enabled": { "type": "boolean" },
        "network_enabled": { "type": "boolean" },
        "state": { "type": "string", "enum": ["Created", "Running", "Paused", "Stopped", "Error"],
            "description": "capitalised, as recorded in the registry" },
        "created_at": { "type": "string", "description": "RFC 3339" },
        "updated_at": { "type": "string", "description": "RFC 3339; the last state change" }
    } })
}

/// The header that names an MCP session.
fn mcp_session_header(required: bool) -> Parameter {
    Parameter {
        name: "Mcp-Session-Id".to_string(),
        location: ParameterLocation::Header,
        required,
        description: "The session the first `initialize` answered with. Required on every             message but that first one, and on GET and DELETE"
            .to_string(),
        schema: json!({ "type": "string" }),
        default: None,
    }
}

fn hm_status_schema() -> Value {
    json!({ "type": "object", "properties": {
        "status": { "type": "string" },
        "name": { "type": "string" }
    } })
}

fn hm_boot_source_schema() -> Value {
    json!({
        "type": "object",
        "description": "What the VM boots, as `hv2_core::BootSource`: an object with a \
            `type` of `linux` (`kernel`, optional `initrd`, `cmdline`), `multiboot` (`kernel`, \
            `modules`), `raw` (an image with `load_addr` and `entry`) or `pvh` (firmware). \
            Every image is a path on the machine running `hm serve`",
        "properties": { "type": { "type": "string", "enum": ["linux", "multiboot", "raw", "pvh"] } },
        "required": ["type"]
    })
}

fn hm_tool_definition_schema() -> Value {
    json!({ "type": "object", "properties": {
        "name": { "type": "string", "description": "`vm.create`, `vm.list`, `vm.get`, `vm.start`, `vm.stop`, `vm.delete`, `vm.metrics` or `vm.execute_script`" },
        "description": { "type": "string" },
        "parameters": { "type": "object", "description": "a JSON Schema for the tool's arguments" }
    } })
}

impl HyperMachineOntology {
    /// The 22 routes of `hm serve` and of `hm sandbox vm mcp-http`.
    pub(super) fn hm_cli_operations() -> Vec<Operation> {
        let vm_failures = |protected: bool| {
            let mut responses = vec![hm_rate_limited()];
            if protected {
                responses.push(hm_unauthorized());
            }
            responses
        };
        let discovery = |id: &'static str, name: &'static str, path: &'static str, what: &str| {
            Spec::new(id, name, "GET", path)
                .about(format!(
                    "{what} Served by `hm serve` out of the `hm` binary's own agent module \
                     (`agentic/`), which is a smaller, separate description of the local \
                     `vm.*` tools: it is not the ontology the control plane's or `hv2-api`'s \
                     `/agentic/ontology` serves, and it lists none of the sandbox, node or \
                     cluster operations. Built fresh on every request from static data; the \
                     order of tools inside it is not stable between calls. Needs the API \
                     key when `HM_API_KEY` is set. The rate limiter and CORS apply as to \
                     every route."
                ))
                .repeatable()
                .answers(ok(
                    200,
                    "The document.",
                    Some(json!({ "type": ["object", "array"] })),
                ))
                .answers(hm_rate_limited())
                .answers(hm_unauthorized())
        };

        let specs = vec![
            Spec::new("hm_get_health", "Local Server Health", "GET", "/health")
                .about(
                    "Report that `hm serve` is up: `{\"status\": \"healthy\", \"service\": \
                     \"hypermachine-mcp\", \"version\": ...}`. It looks at nothing: no VM \
                     registry, no hypervisor, no disk, so it answers 200 for as long as the \
                     process accepts connections and says nothing about whether VMs can be \
                     started. Public: never needs the API key. The rate limiter applies, so a \
                     prober that shares its socket peer address with other clients can be \
                     refused.",
                )
                .repeatable()
                .answers(ok(
                    200,
                    "The service name and the build's version.",
                    Some(json!({ "type": "object", "properties": {
                        "status": { "type": "string", "enum": ["healthy"] },
                        "service": { "type": "string" },
                        "version": { "type": "string" } } })),
                ))
                .answers(hm_rate_limited()),
            Spec::new("hm_list_tools", "List The Local VM Tools", "GET", "/mcp/tools")
                .about(
                    "List the eight tools `hm_call_tool` runs (`vm.create`, `vm.list`, \
                     `vm.get`, `vm.start`, `vm.stop`, `vm.delete`, `vm.metrics`, \
                     `vm.execute_script`) as `{name, description, parameters}` objects, which \
                     OpenAI function calling and Anthropic tool use can take as they are. The \
                     list is static and describes the arguments more strictly than the server \
                     checks them: `cpu_cores` 1 to 128, `memory_gb` 1 to 1024 and a VM name \
                     are advertised and not enforced by `hm_create_vm` or `vm.create`, and \
                     the `gpu_enabled` text says creation refuses it where the refusal \
                     happens when the VM is started. `vm.execute_script` is described as \
                     running a Rhai script on the host against a read-only view of the VM: it \
                     does not run commands in the guest. Needs the API key when `HM_API_KEY` is set.",
                )
                .repeatable()
                .answers(ok(
                    200,
                    "The tool definitions.",
                    Some(json!({ "type": "array", "items": hm_tool_definition_schema() })),
                ))
                .answers(hm_rate_limited())
                .answers(hm_unauthorized()),
            Spec::new("hm_call_tool", "Call A Local VM Tool", "POST", "/mcp/call")
                .about(
                    "Run one of the tools `hm_list_tools` lists, named in `tool`, with its \
                     `arguments`: `vm.create` (the same arguments and effects as \
                     `hm_create_vm`, answering `{name, state, cpu_cores, memory_gb}`), \
                     `vm.list`, `vm.get` (adds `gpu_enabled` and `network_enabled`), \
                     `vm.start`, `vm.stop`, `vm.delete` (`{status, name}` each), `vm.metrics` \
                     and `vm.execute_script` (the script's own result). The answer is always \
                     `200 {success, result, error}`: a tool that fails, an unknown tool name, \
                     arguments that do not fit `vm.create` and a missing VM all come back as \
                     `success: false` with the message in `error`, not as an HTTP error. \
                     `name` is read as an empty string when absent, so `vm.get` with no name \
                     reports `VM '' not found`. `vm.delete` of a VM that does not exist \
                     reports success (see `hm_delete_vm`). `agent_id` is accepted and unused: \
                     no per-agent session is kept. This route needs the API key when \
                     `HM_API_KEY` is set, including for its read-only tools. Not idempotent as a whole: it depends on the \
                     tool.",
                )
                .takes(
                    json!({
                        "type": "object",
                        "required": ["tool", "arguments"],
                        "properties": {
                            "tool": { "type": "string", "enum": [
                                "vm.create", "vm.list", "vm.get", "vm.start", "vm.stop",
                                "vm.delete", "vm.metrics", "vm.execute_script"] },
                            "arguments": { "type": "object", "description": "required, even for `vm.list`, where it may be `{}`" },
                            "agent_id": { "type": "string", "description": "accepted and ignored" }
                        }
                    }),
                    true,
                )
                .answers(ok(
                    200,
                    "`success` says whether the tool ran; on failure `result` is null and \
                     `error` is the message.",
                    Some(json!({ "type": "object", "properties": {
                        "success": { "type": "boolean" },
                        "result": { "type": ["object", "array", "null"] },
                        "error": { "type": ["string", "null"] } } })),
                ))
                .answers(body_rejection())
                .answers_all(vm_failures(true)),
            Spec::new("hm_list_vms", "List Local VMs", "GET", "/vms")
                .about(
                    "List every VM in the local registry: the file `registry.json` in a                      `hypermachine` directory under `~/.local/share` on Linux,                      `~/Library/Application Support` on macOS or `%LOCALAPPDATA%` on                      Windows, which the `hm t2` commands share, so a VM created from the                      command line is listed here. An array of VM records in no stable order. \
                     The registry says what was recorded last, not what is running: after \
                     the server restarts a VM recorded `Running` has no process behind it \
                     (see `hm_start_vm`). Needs the API key when `HM_API_KEY` is set.",
                )
                .repeatable()
                .answers(ok(
                    200,
                    "Every VM.",
                    Some(json!({ "type": "array", "items": hm_vm_info_schema() })),
                ))
                .answers(hm_rate_limited())
                .answers(hm_unauthorized()),
            Spec::new("hm_create_vm", "Create A Local VM", "POST", "/vms")
                .about(
                    "Register a VM in the local registry, in state `Created`, and save the \
                     registry. Nothing is started and no hypervisor is touched; the only \
                     work beyond the record is that a `boot` source is loaded once to check \
                     that its images (host paths on this machine) can be read, and one that \
                     cannot is refused. Defaults: `cpu_cores` 2, `memory_gb` 4, no GPU, no \
                     network, no boot source (a VM with no guest code). The name and the \
                     sizes are not validated: any string, including an empty one, is a name, \
                     and the 1 to 128 core and 1 to 1024 GB limits the tool list and the \
                     capabilities summary advertise are not enforced. `gpu_enabled: true` is \
                     recorded and the VM then cannot be started, because building it \
                     refuses a GPU it cannot attach. Answers 200 (not 201) with the record. \
                     Needs the API key when `HM_API_KEY` is set. Not idempotent: a second \
                     create of the name is 400.",
                )
                .takes(
                    json!({
                        "type": "object",
                        "required": ["name"],
                        "properties": {
                            "name": { "type": "string" },
                            "cpu_cores": { "type": "integer", "default": 2 },
                            "memory_gb": { "type": "integer", "default": 4 },
                            "gpu_enabled": { "type": "boolean", "default": false },
                            "network_enabled": { "type": "boolean", "default": false },
                            "boot": hm_boot_source_schema()
                        }
                    }),
                    true,
                )
                .answers(ok(200, "The new record, state `Created`.", Some(hm_vm_info_schema())))
                .answers(plain(
                    400,
                    "plain text: `VM '<name>' already exists`; `cannot create VM '<name>': \
                     its boot image <path> is unusable` (with the reason, and the host path); \
                     or the registry file could not be written",
                ))
                .answers(body_rejection())
                .answers_all(vm_failures(true)),
            Spec::new("hm_get_vm", "Get A Local VM", "GET", "/vms/{name}")
                .about(
                    "Read one VM's record from the registry: sizes, flags, `state` and the \
                     creation and last-change times. The state is what the registry last \
                     recorded; it does not ask a running VM. Needs the API key when \
                     `HM_API_KEY` is set.",
                )
                .with(hm_vm_name())
                .repeatable()
                .answers(ok(200, "The record.", Some(hm_vm_info_schema())))
                .answers(plain(404, "plain text: `VM '<name>' not found`"))
                .answers(hm_rate_limited())
                .answers(hm_unauthorized()),
            Spec::new("hm_delete_vm", "Delete A Local VM", "DELETE", "/vms/{name}")
                .about(
                    "Stop the VM if it has a running instance in this process, ignoring any \
                     failure to stop it, remove its record and save the registry. A name \
                     that is not in the registry is not an error: the handler removes \
                     nothing and answers 200 `{status: \"deleted\"}` all the same, so a \
                     caller cannot tell a deleted VM from one that never existed. The 404 is \
                     what every failure of the call is mapped to, which in practice means the \
                     registry file could not be written. Disks and images the VM referenced \
                     are not touched. Needs the API key when `HM_API_KEY` is set.",
                )
                .with(hm_vm_name())
                .repeatable()
                .answers(ok(200, "`{status: \"deleted\", name}`.", Some(hm_status_schema())))
                .answers(plain(
                    404,
                    "plain text; the registry could not be saved (a missing VM is 200)",
                ))
                .answers_all(vm_failures(true)),
            Spec::new("hm_start_vm", "Start A Local VM", "POST", "/vms/{name}/start")
                .about(
                    "Build the VM from its record and bring it up in this process, record \
                     it `Running` and save the registry. With a `boot` source it provisions \
                     the hypervisor backend, loads the images and runs the guest, which is \
                     executing in the background when the call returns. A VM with no boot \
                     source is brought up with nothing to execute (the server logs a \
                     warning): `Running` then means a VM object exists, not that a guest \
                     runs. The instance lives in this process's memory only. The registry \
                     keeps the state across restarts, so after a restart a VM recorded \
                     `Running` is refused here as already running and has no guest; \
                     `hm_stop_vm` resets it. Every failure is 400: no such VM, already \
                     running, a GPU requested, no hypervisor backend on the host, or the \
                     guest could not be launched. Needs the API key when `HM_API_KEY` is set.",
                )
                .with(hm_vm_name())
                .answers(ok(200, "`{status: \"started\", name}`.", Some(hm_status_schema())))
                .answers(plain(
                    400,
                    "plain text: `VM '<name>' not found`, `VM '<name>' is already running`, \
                     `Failed to build VM: ...` (including a GPU the VM cannot attach), \
                     `Failed to launch VM: ...` or `Failed to start VM: ...`",
                ))
                .answers_all(vm_failures(true)),
            Spec::new("hm_stop_vm", "Stop A Local VM", "POST", "/vms/{name}/stop")
                .about(
                    "Stop the VM's running instance, if this process has one, then record \
                     the VM `Stopped` and save the registry. It does not check the current \
                     state: a VM that was never started, or whose recorded `Running` state is \
                     left over from before a restart, is recorded `Stopped` and the call \
                     succeeds. The VM is removed from the running set before it is stopped, \
                     so a failure to stop (400) leaves a record that is not updated and no \
                     handle to retry the stop with. A missing VM is 400, not 404. Needs the \
                     API key when `HM_API_KEY` is set.",
                )
                .with(hm_vm_name())
                .repeatable()
                .answers(ok(200, "`{status: \"stopped\", name}`.", Some(hm_status_schema())))
                .answers(plain(
                    400,
                    "plain text: `VM '<name>' not found`, `Failed to stop VM: ...`, or the \
                     registry could not be written",
                ))
                .answers_all(vm_failures(true)),
            Spec::new(
                "hm_get_vm_metrics",
                "Get A Local VM's Metrics",
                "GET",
                "/vms/{name}/metrics",
            )
            .about(
                "Read the VM's sizes and state and, when this process has a running \
                 instance, its memory use, CPU use and uptime. A VM with no running \
                 instance (never started, stopped, or started by an earlier process) \
                 answers with `memory_used_gb`, `cpu_usage_percent` and `uptime_seconds` \
                 null. `state` is the registry's capitalised name (`Running`), not the lower \
                 case some older documentation shows. Needs the API key when `HM_API_KEY` is set.",
            )
            .with(hm_vm_name())
            .repeatable()
            .answers(ok(
                200,
                "The metrics.",
                Some(json!({ "type": "object", "properties": {
                    "name": { "type": "string" },
                    "state": { "type": "string" },
                    "cpu_cores": { "type": "integer" },
                    "memory_gb": { "type": "integer" },
                    "memory_used_gb": { "type": ["number", "null"] },
                    "cpu_usage_percent": { "type": ["number", "null"] },
                    "uptime_seconds": { "type": ["integer", "null"] } } })),
            ))
            .answers(plain(
                404,
                "plain text: `VM '<name>' not found`; also any error reading a running \
                 instance's metrics",
            ))
            .answers(hm_rate_limited())
            .answers(hm_unauthorized()),
            Spec::new(
                "hm_execute_vm_script",
                "Run A Rhai Script Against A Local VM",
                "POST",
                "/vms/{name}/script",
            )
            .about(
                "Evaluate a Rhai script in this process, on the host, against a read-only \
                 view of a VM this process is running, and answer with the value it \
                 returns as JSON. It does not run anything inside the guest: there is no \
                 shell, no exec and no guest file access, and Rhai's default engine has no \
                 file or network functions. The agent engine bounds it by wall-clock time \
                 and an operation limit. `timeout_seconds` (default 300) is accepted and \
                 not used: the handler passes only the script on, so it cannot raise or \
                 lower the limit. The VM must have been started by this process: one that \
                 is merely recorded `Running`, or was never started, is 400 `is not \
                 running`. Needs the API key when `HM_API_KEY` is set.",
            )
            .with(hm_vm_name())
            .takes(
                json!({
                    "type": "object",
                    "required": ["script"],
                    "properties": {
                        "script": { "type": "string", "description": "Rhai source" },
                        "timeout_seconds": { "type": "integer", "default": 300, "description": "accepted and ignored" }
                    }
                }),
                true,
            )
            .answers(ok(200, "The script's result, any JSON value.", None))
            .answers(plain(
                400,
                "plain text: `VM '<name>' is not running`, or `Script execution failed: ...` \
                 (a Rhai error, a timeout, or the operation limit)",
            ))
            .answers(body_rejection())
            .answers_all(vm_failures(true)),
            discovery(
                "hm_get_agentic_ontology",
                "Get The Local Server's Ontology",
                "/agentic/ontology",
                "The `hm` binary's own ontology: concepts (VM, metrics, script), types, the \
                 `vm.*` operations with their parameters, relationships and examples.",
            ),
            discovery(
                "hm_get_agentic_schema",
                "Get The Local Server's JSON Schema",
                "/agentic/schema",
                "A full JSON Schema document (`$schema`, `$id`, definitions, operations and \
                 examples) for the local `vm.*` tools.",
            ),
            discovery(
                "hm_get_agentic_schema_compact",
                "Get The Local Server's Compact Schema",
                "/agentic/schema/compact",
                "A compact schema for small context windows: a version and, for each tool, \
                 `{id, desc, params, example}`.",
            ),
            discovery(
                "hm_get_agentic_capabilities",
                "Get The Local Server's Capabilities Summary",
                "/agentic/capabilities",
                "A short summary: the service name, the eight `vm.*` operations, flags for \
                 what is supported (create, start, stop, delete, list, get, metrics, \
                 scripts) and a `constraints` object. The constraints (a VM name matching \
                 `^[a-zA-Z][a-zA-Z0-9-]{0,62}$`, `cpu_cores` 1 to 128, `memory_gb` 1 to \
                 1024) are guidance only: `hm_create_vm` does not enforce them.",
            ),
            Spec::new(
                "hm_get_provider_config",
                "Get A Model Provider's Configuration",
                "GET",
                "/agentic/providers/{provider}",
            )
            .about(
                "Return what a model provider's client needs to use the local tools: \
                 `{provider, tools, system_prompt, hints}`, with the tools in that provider's \
                 native format, a recommended system prompt and generation hints (temperature, parallel \
                 tool calls, a token ceiling). `provider` is matched case-insensitively \
                 against a list of names and model names that fall in four families: \
                 `openai`, `gpt`, `chatgpt`, `gpt-4`, `gpt-4o`, `gpt-5`, `o1`, `o3` and \
                 their variants; `anthropic`, `claude`, `sonnet`, `opus`, `haiku` and the \
                 `claude-3`, `claude-3.5`, `claude-4`, `claude-4.5` names; `google`, \
                 `gemini` and the `gemini-2.0`, `gemini-2.5` and `-flash`, `-pro`, \
                 `-ultra` names; and `generic`, `other`, `custom`. Anything else is a 400 \
                 with an empty body. Needs the API key when `HM_API_KEY` is set.",
            )
            .with(path_param(
                "provider",
                "A provider or model name from the families above",
                json!({ "type": "string" }),
            ))
            .repeatable()
            .answers(ok(
                200,
                "The configuration.",
                Some(json!({ "type": "object", "properties": {
                    "provider": { "type": "string" },
                    "tools": {},
                    "system_prompt": { "type": "string" },
                    "hints": { "type": "object" } } })),
            ))
            .answers(plain(400, "the name is not one the server knows (empty body)"))
            .answers(hm_rate_limited())
            .answers(hm_unauthorized()),
            discovery(
                "hm_get_openai_tools",
                "Get The Local Tools For OpenAI",
                "/agentic/tools/openai",
                "The local `vm.*` tools as OpenAI function-calling tools: an array of \
                 `{type: \"function\", function: {name, description, parameters}}`.",
            ),
            discovery(
                "hm_get_anthropic_tools",
                "Get The Local Tools For Anthropic",
                "/agentic/tools/anthropic",
                "The local `vm.*` tools as Anthropic tool-use tools: an array of `{name, \
                 description, input_schema}`.",
            ),
            discovery(
                "hm_get_gemini_tools",
                "Get The Local Tools For Gemini",
                "/agentic/tools/gemini",
                "The local `vm.*` tools as a Gemini tool: `{function_declarations: [{name, \
                 description, parameters}]}`.",
            ),
        ];
        let mut operations: Vec<Operation> =
            specs.into_iter().map(|spec| spec.build(false)).collect();
        operations.extend(Self::hm_mcp_http_operations());
        operations
    }

    /// `hm sandbox vm mcp-http`: one route, three methods.
    fn hm_mcp_http_operations() -> Vec<Operation> {
        let origin_and_token = || {
            vec![
                plain(
                    400,
                    "a header the transport reads (`Origin`, `Authorization`, `Accept`, \
                     `Content-Type`, `Mcp-Session-Id`, `Mcp-Protocol-Version`) was sent twice \
                     or is not text; or an `Mcp-Protocol-Version` other than `2025-11-25`",
                ),
                plain(
                    401,
                    "the `Authorization` header is not `Bearer <HM_MCP_TOKEN>` (compared \
                     without stopping at the first wrong byte), with `WWW-Authenticate: \
                     Bearer`. Checked after the Origin, so a wrong Origin is 403 even with \
                     a good token",
                ),
                plain(
                    403,
                    "the request has an `Origin` header and it is not one of the exact HTTPS \
                     origins given with `--allow-origin` (with none configured every request \
                     that carries an Origin is refused). A request with no `Origin`, as a \
                     native client sends, passes this check",
                ),
            ]
        };
        let specs = vec![
            Spec::new(
                "hm_mcp_http_post",
                "Send An MCP Message Over HTTP",
                "POST",
                "/mcp",
            )
            .about(
                "Send one JSON-RPC 2.0 message of the Model Context Protocol (version \
                 2025-11-25) to the sandbox tools `hm sandbox vm mcp-http` serves, and read \
                 the answer as one JSON body. It is a single route that serves the \
                 sandbox API of the `--endpoint` it was started with (a control plane or \
                 node, with that process's `HV2_API_KEY`), not the local VM registry of \
                 `hm serve`. The first message of a session must be `initialize` with an `id`; \
                 the answer carries a new `Mcp-Session-Id` header, which every later \
                 message must send. Then `notifications/initialized`, `tools/list` and \
                 `tools/call` follow. Twelve tools: `sandbox_create`, `sandbox_list`, \
                 `sandbox_inspect` (without the access token), `sandbox_exec` (an argv \
                 vector), `sandbox_pause`, `sandbox_resume`, `sandbox_fork`, \
                 `sandbox_delete`, `checkpoint_save`, `checkpoint_list`, \
                 `checkpoint_restore` and `checkpoint_delete`; with `--envd-endpoint` also \
                 `file_upload` and `file_download` (256 KiB at most). Access tokens are \
                 stripped from results. The caller acts with the one API role configured for \
                 the process, whoever holds the bearer token. A notification (no `id`) is \
                 answered 202 with no body; a `notifications/cancelled` for the request in \
                 flight releases that request's wait with 202, and the cancelled request \
                 ends with an empty `text/event-stream` body and no JSON-RPC result, while \
                 work the remote API already accepted may continue. One request runs per \
                 session at a time. Limits: 1 MiB per body, 16 bodies and 16 workers at once \
                 process-wide, 64 sessions, each expiring 10 minutes after it was created \
                 (not after its last use). The listener binds loopback only; it has no TLS.",
            )
            .with(mcp_session_header(false))
            .takes_raw(
                "application/json",
                json!({
                    "type": "object",
                    "required": ["jsonrpc", "method"],
                    "properties": {
                        "jsonrpc": { "type": "string", "enum": ["2.0"] },
                        "id": { "type": ["string", "integer"], "description": "absent for a notification" },
                        "method": { "type": "string", "description": "`initialize`, `tools/list`, `tools/call`, `notifications/initialized`, `notifications/cancelled`" },
                        "params": { "type": "object" }
                    }
                }),
                true,
            )
            .answers(ok(
                200,
                "The JSON-RPC response; for a fresh `initialize` also an `Mcp-Session-Id` \
                 header. A cancelled request instead answers 200 `text/event-stream` with an \
                 empty body.",
                Some(json!({ "type": "object" })),
            ))
            .answers(plain(
                202,
                "a notification, or a cancellation, accepted; no body",
            ))
            .answers_all(origin_and_token())
            .answers(plain(
                400,
                "the body is not JSON, is not a JSON-RPC 2.0 request object with a string \
                 `method`, has an `id` that is not a string or an integer, or there is no \
                 `Mcp-Session-Id` and the message is not an `initialize` with an `id`",
            ))
            .answers(plain(
                404,
                "the `Mcp-Session-Id` is unknown or has expired",
            ))
            .answers(plain(
                406,
                "`Accept` does not list both `application/json` and `text/event-stream`",
            ))
            .answers(plain(408, "the body was not read within the request deadline"))
            .answers(plain(413, "the body is over 1 MiB"))
            .answers(plain(
                415,
                "`Content-Type` is not `application/json`",
            ))
            .answers(plain(
                429,
                "another request of the same session is still running; the transport refuses \
                 overlap rather than queueing",
            ))
            .answers(plain(
                500,
                "the worker task failed (a fresh session is dropped)",
            ))
            .answers(plain(
                503,
                "16 bodies are being read or 16 requests are running process-wide, or 64 \
                 sessions exist",
            )),
            Spec::new(
                "hm_mcp_http_get",
                "Open An MCP Event Stream (Not Supported)",
                "GET",
                "/mcp",
            )
            .about(
                "The MCP transport's optional server-sent event stream. This server has none \
                 and answers 405 for every GET that gets past authentication and names a \
                 live session, so a client falls back to POST alone. The checks come first: \
                 the Origin and bearer token as for `hm_mcp_http_post`, then a session id \
                 (400 without one, 404 for an unknown or expired one). There is no response \
                 replay and no server push, so nothing can be read from this route; it is \
                 listed because the gate counts it as served.",
            )
            .with(mcp_session_header(true))
            .repeatable()
            .answers(plain(
                405,
                "there is no event stream (the session exists and the credentials are good)",
            ))
            .answers_all(origin_and_token())
            .answers(plain(400, "no `Mcp-Session-Id` header (or one of the cases above)"))
            .answers(plain(404, "the session is unknown or has expired")),
            Spec::new(
                "hm_mcp_http_delete",
                "End An MCP Session",
                "DELETE",
                "/mcp",
            )
            .about(
                "End the session named by the `Mcp-Session-Id` header: it is forgotten at \
                 once, and a request of that session still waiting for the remote API is \
                 released (the work the API already accepted may continue and is not rolled \
                 back). 204 with no body. A second DELETE of the same id is 404. The Origin \
                 and bearer checks are the same as for `hm_mcp_http_post`.",
            )
            .with(mcp_session_header(true))
            .repeatable()
            .answers(plain(204, "the session ended"))
            .answers_all(origin_and_token())
            .answers(plain(400, "no `Mcp-Session-Id` header (or one of the cases above)"))
            .answers(plain(404, "the session is unknown, has expired or was already ended")),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }
}

// ----------------------------------------------------------------------------
// hm jobs serve
// ----------------------------------------------------------------------------

/// The `{"error": "..."}` shape of the jobs server.
fn jobs_fail(status_code: u16, description: &str) -> OperationResponse {
    OperationResponse {
        status_code,
        description: description.to_string(),
        schema: Some(json!({ "type": "object", "properties": { "error": { "type": "string" } } })),
    }
}

/// The 401 of every jobs route when the server has a token.
fn jobs_unauthorized() -> OperationResponse {
    jobs_fail(
        401,
        "the server was started with a token (`--token` or `HM_JOBS_TOKEN`) and the request \
         has no `Authorization: Bearer <token>` header or a wrong one (compared in constant \
         time). Never returned when no token is set, which `hm jobs serve` allows only on a \
         loopback address",
    )
}

/// What a schedule route answers when the store cannot do what it was asked.
fn jobs_store_failure() -> OperationResponse {
    jobs_fail(
        500,
        "the store could not be read or written, a stored record is corrupt, or the blocking \
         task failed",
    )
}

fn jobs_id() -> Parameter {
    path_param(
        "id",
        "The job's id, as `jobs_submit` answered with (letters, digits and hyphens)",
        json!({ "type": "string" }),
    )
}

fn schedule_id() -> Parameter {
    path_param(
        "id",
        "The schedule's name: 1 to 64 ASCII letters, digits or hyphens. Names are chosen by \
         the caller, are never reused and cannot be changed",
        json!({ "type": "string", "pattern": "^[A-Za-z0-9-]{1,64}$" }),
    )
}

fn scheduled_ms() -> Parameter {
    path_param(
        "scheduled_ms",
        "The occurrence's scheduled time, in milliseconds since the Unix epoch, as `scheduled_ms` of \
         a published occurrence. A value that is not an unsigned integer is the framework's own \
         400",
        json!({ "type": "integer", "minimum": 0 }),
    )
}

fn job_spec_schema() -> Value {
    json!({
        "type": "object",
        "required": ["command"],
        "additionalProperties": false,
        "properties": {
            "name": { "type": "string" },
            "command": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "the program and its arguments; run directly, not through a shell; the first element may not be blank" },
            "workdir": { "type": "string" },
            "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "added over a minimal base; names may not be empty or contain `=` or NUL" },
            "sandbox": { "type": "object", "additionalProperties": false, "properties": {
                "memory": { "type": "string", "description": "for example `8G`" },
                "cpu_time_secs": { "type": "integer" },
                "wall_clock_secs": { "type": "integer" },
                "max_processes": { "type": "integer" },
                "net": { "type": "string", "enum": ["deny", "host"], "default": "deny" },
                "fs": { "type": "string", "description": "`host` (the default) or `isolated:ROOT`" },
                "ro": { "type": "array", "items": { "type": "string" } },
                "isolate_processes": { "type": "boolean" },
                "no_new_privileges": { "type": "boolean" },
                "strict": { "type": "boolean", "description": "refuse to run if a control cannot be enforced, instead of running without it" } } },
            "labels": { "type": "array", "items": { "type": "string" }, "description": "letters, digits and - _ . only; a worker takes the job only if it offers every label" },
            "max_attempts": { "type": "integer", "minimum": 1, "default": 1 },
            "graceful_stop": { "type": "object", "required": ["create_file", "grace_secs"], "properties": {
                "create_file": { "type": "string" }, "grace_secs": { "type": "integer" } } },
            "not_before_ms": { "type": "integer", "description": "earliest start, Unix milliseconds; an earliest start, not a deadline" }
        }
    })
}

fn job_state_schema() -> Value {
    json!({ "type": "object", "properties": {
        "id": { "type": "string" },
        "state": { "type": "string", "enum": ["queued", "running", "succeeded", "failed", "cancelled"] },
        "submitted_ms": { "type": "integer" },
        "attempts": { "type": "integer" },
        "worker": { "type": "string", "description": "`name#attempt`" },
        "started_ms": { "type": "integer" },
        "heartbeat_ms": { "type": "integer" },
        "finished_ms": { "type": "integer" },
        "exit_code": { "type": "integer" },
        "signal": { "type": "integer" },
        "killed_by": { "type": "string", "description": "the limit that killed it" },
        "unenforced": { "type": "array", "items": { "type": "string" }, "description": "controls dropped because the host cannot enforce them" },
        "cancel_requested": { "type": "boolean" },
        "message": { "type": "string" } } })
}

fn schedule_spec_schema() -> Value {
    json!({
        "type": "object",
        "required": ["first_ms", "job"],
        "additionalProperties": false,
        "properties": {
            "first_ms": { "type": "integer", "description": "the first occurrence, Unix milliseconds" },
            "every_ms": { "type": "integer", "description": "the interval; positive, and not set together with `cron`" },
            "cron": { "type": "object", "required": ["expression", "timezone"], "properties": {
                "expression": { "type": "string", "description": "a five-field cron expression" },
                "timezone": { "type": "string", "description": "an IANA zone name" },
                "tzdb_version": { "type": "string", "description": "when given it must be the version this build was compiled with" } } },
            "missed_policy": { "type": "string", "enum": ["catch_up", "coalesce"], "default": "catch_up" },
            "vm": { "type": "object", "required": ["sandbox_id", "connection_profile", "timeout_secs"], "properties": {
                "sandbox_id": { "type": "string", "description": "1 to 128 ASCII letters, digits, hyphens or underscores" },
                "connection_profile": { "type": "string", "description": "an operator-managed profile name, 1 to 64 letters, digits or hyphens; never an endpoint or a credential" },
                "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 86400 } } },
            "job": job_spec_schema()
        }
    })
}

fn occurrence_schema() -> Value {
    json!({ "type": "object", "properties": {
        "schedule_id": { "type": "string" },
        "scheduled_ms": { "type": "integer" },
        "vm": { "type": "object" },
        "job": job_spec_schema() } })
}

fn completion_schema() -> Value {
    json!({ "type": "object", "required": ["claim_token", "exit_code", "timed_out"], "properties": {
        "origin": { "type": "string", "enum": ["unknown", "api_response", "operator_recorded"], "description": "on POST this is ignored and replaced with `operator_recorded`" },
        "claim_token": { "type": "string", "description": "the token of the existing claim" },
        "exit_code": { "type": ["integer", "null"], "minimum": 0, "maximum": 255 },
        "timed_out": { "type": "boolean" },
        "stdout": { "type": ["string", "null"], "description": "at most 65536 bytes" },
        "stderr": { "type": ["string", "null"], "description": "at most 65536 bytes" },
        "stdout_truncated": { "type": "boolean", "default": false },
        "stderr_truncated": { "type": "boolean", "default": false } } })
}

impl HyperMachineOntology {
    /// The 13 routes of `hm jobs serve`.
    pub(super) fn jobs_operations() -> Vec<Operation> {
        let specs = vec![
            Spec::new("jobs_submit", "Queue A Job", "POST", "/api/v1/jobs")
                .about(
                    "Queue a program to run on a worker's host. The body is a job spec (`command` \
                     is required; unknown fields are refused, so a misspelt limit is a 400, \
                     not a job that runs without it). The call only queues: a worker started \
                     with `hm jobs worker` over the same store directory claims the job \
                     later, if it offers every one of the job's `labels` and the \
                     `not_before_ms` has passed, and runs it under the process sandbox. The \
                     `command` runs directly, not through a shell, over a minimal environment \
                     plus `env` and `HM_JOB_ID`. The spec chooses its own sandbox: `net` may \
                     be `host` and `fs` `host`, which are the host's network and filesystem, \
                     and the sandbox defaults deny only the network. This is a remote \
                     execution route: whoever may call it may run a program as the worker's \
                     user. `max_attempts` defaults to 1, so a program is not started twice \
                     unless the spec says so. 201 with the new id; each call makes a new job, \
                     so it is not idempotent.",
                )
                .takes(job_spec_schema(), true)
                .answers(ok(
                    201,
                    "The job is queued.",
                    Some(json!({ "type": "object", "properties": { "id": { "type": "string" } } })),
                ))
                .answers(jobs_fail(
                    400,
                    "the spec is invalid: an empty or blank `command`, `max_attempts` 0, an \
                     unusable `env` name, a bad label, `sandbox.net` not `deny` or `host`, \
                     `sandbox.fs` not `host` or `isolated:ROOT`, or an unparseable `memory`",
                ))
                .answers(jobs_fail(500, "the job could not be written to the store"))
                .answers(body_rejection())
                .answers(jobs_unauthorized()),
            Spec::new("jobs_list", "List Jobs", "GET", "/api/v1/jobs")
                .about(
                    "List every job's state, oldest first (sorted by id), optionally only those \
                     in one state. `state` is compared as text with `queued`, `running`, \
                     `succeeded`, `failed` or `cancelled`: any other value matches nothing \
                     and gives an empty list, not a 400. Listing is not read-only: it first \
                     checks for running jobs whose worker has lost its lease and requeues or \
                     fails them, as every store operation does. The list is the whole store, \
                     with no paging.",
                )
                .with(query_param(
                    "state",
                    "Keep only jobs in this state",
                    json!({ "type": "string", "enum": ["queued", "running", "succeeded", "failed", "cancelled"] }),
                ))
                .repeatable()
                .answers(ok(
                    200,
                    "The matching job states.",
                    Some(json!({ "type": "array", "items": job_state_schema() })),
                ))
                .answers(jobs_fail(500, "the store could not be read"))
                .answers(jobs_unauthorized()),
            Spec::new("jobs_get", "Get A Job", "GET", "/api/v1/jobs/{id}")
                .about(
                    "Read a job's state (attempts, worker, start, heartbeat and finish times, exit \
                     code or signal, the limit that killed it, controls the host could not \
                     enforce, whether a cancel was asked for, and a message when it failed \
                     without running to an exit) and the spec it was submitted with, \
                     including its `env`. An id that is not letters, digits and hyphens is \
                     404, as is one that does not exist.",
                )
                .with(jobs_id())
                .repeatable()
                .answers(ok(
                    200,
                    "`{state, spec}`.",
                    Some(json!({ "type": "object", "properties": {
                        "state": job_state_schema(), "spec": job_spec_schema() } })),
                ))
                .answers(jobs_fail(404, "no such job"))
                .answers(jobs_fail(500, "the state or spec file could not be read or is corrupt"))
                .answers(jobs_unauthorized()),
            Spec::new("jobs_get_logs", "Read A Job's Log", "GET", "/api/v1/jobs/{id}/logs")
                .about(
                    "Read a job's captured standard output or error as `text/plain`. Without \
                     `follow` the log as it is now, which is empty for a job that has not \
                     started. With `follow=true` the response is streamed: what is there, then \
                     what is appended (checked every 250 ms), ending once the job has finished \
                     or been cancelled and the log was read to its end one more time. A follow \
                     stream sends only appended bytes, so a log that is replaced is not \
                     resent, and the connection stays open for as long as the job runs. A \
                     `stream` other than `stdout` or `stderr` is 400.",
                )
                .with(jobs_id())
                .with(query_param(
                    "stream",
                    "`stdout` (the default) or `stderr`",
                    json!({ "type": "string", "enum": ["stdout", "stderr"], "default": "stdout" }),
                ))
                .with(query_param(
                    "follow",
                    "Stream the log until the job ends",
                    json!({ "type": "boolean", "default": false }),
                ))
                .repeatable()
                .answers(plain(200, "The log as `text/plain; charset=utf-8`."))
                .answers(jobs_fail(400, "`stream` is neither `stdout` nor `stderr`"))
                .answers(jobs_fail(404, "no such job"))
                .answers(jobs_fail(500, "the log file could not be read"))
                .answers(jobs_unauthorized()),
            Spec::new("jobs_cancel", "Cancel A Job", "POST", "/api/v1/jobs/{id}/cancel")
                .about(
                    "Ask a job to stop. A queued job is cancelled at once (state `cancelled`). \
                     A running job gets a cancel marker: its worker creates the spec's \
                     `graceful_stop.create_file`, if it has one, waits up to `grace_secs` for \
                     the program to exit, then kills the whole process tree, and records the \
                     job `cancelled` whatever the exit was. The answer is 202 with the job's \
                     state as it was when the marker was written, which for a running job is \
                     not yet `cancelled`: read `jobs_get` to see the end. A job that is \
                     already `succeeded`, `failed` or `cancelled` is a 409.",
                )
                .with(jobs_id())
                .answers(ok(
                    202,
                    "Accepted; the job's state, `cancelled` if it had not started.",
                    Some(job_state_schema()),
                ))
                .answers(jobs_fail(404, "no such job"))
                .answers(jobs_fail(409, "the job is already over"))
                .answers(jobs_fail(500, "the marker could not be written"))
                .answers(jobs_unauthorized()),
            Spec::new("jobs_list_schedules", "List Schedules", "GET", "/api/v1/schedules")
                .about(
                    "List schedule names in lexical order, one page at a time: `after` is an \
                     exclusive cursor (the last name already seen) and `limit` is 1 to 1024 \
                     (default 100). Cancelled schedules are listed. The store's schedule \
                     directory is scanned and at most a page is kept in memory; pages are not \
                     a snapshot, so a name created behind the cursor is missed until the \
                     listing starts again. Unknown query fields are a 400.",
                )
                .with(query_param(
                    "after",
                    "Exclusive cursor: list names after this one",
                    json!({ "type": "string" }),
                ))
                .with(query_param(
                    "limit",
                    "Page size",
                    json!({ "type": "integer", "minimum": 1, "maximum": 1024, "default": 100 }),
                ))
                .repeatable()
                .answers(ok(
                    200,
                    "The names.",
                    Some(json!({ "type": "array", "items": { "type": "string" } })),
                ))
                .answers(jobs_fail(400, "`limit` is outside 1 to 1024"))
                .answers(plain(400, "a query field is unknown or not a number (the framework's own rejection)"))
                .answers(jobs_store_failure())
                .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_create_schedule",
                "Create A Schedule",
                "POST",
                "/api/v1/schedules/{id}",
            )
            .about(
                "Create a schedule named in the path: an immutable recurrence of a job, either \
                 every `every_ms` from `first_ms` or by a `cron` calendar in an IANA \
                 timezone (not both), with a `missed_policy` for time the publisher was not \
                 running (`catch_up`, the default, publishes every missed occurrence oldest \
                 first; `coalesce` only the latest) and an optional `vm` target (a sandbox \
                 id, an operator-managed connection profile name and a guest timeout) that \
                 the VM dispatcher uses. A schedule with a `vm` target may not carry host \
                 sandbox limits or a graceful-stop file, and its `workdir` must be an \
                 absolute guest path. A cron schedule pins the build's timezone database \
                 version. Creating a schedule publishes nothing and queues no job: it only \
                 stores the recurrence, and it cannot be edited afterwards, only cancelled. \
                 A name is used once. 201 with the id.",
            )
            .with(schedule_id())
            .takes(schedule_spec_schema(), true)
            .answers(ok(
                201,
                "Created.",
                Some(json!({ "type": "object", "properties": { "id": { "type": "string" } } })),
            ))
            .answers(jobs_fail(
                400,
                "the name is not 1 to 64 letters, digits or hyphens, or the schedule is \
                 invalid: no or both of `every_ms` and `cron`, a bad expression or \
                 timezone, a `tzdb_version` this build does not have, a `first_ms` outside \
                 years 1970 to 9999 for cron, a bad VM target or an invalid job",
            ))
            .answers(jobs_fail(409, "a schedule of that name already exists"))
            .answers(jobs_store_failure())
            .answers(body_rejection())
            .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_get_schedule",
                "Get A Schedule",
                "GET",
                "/api/v1/schedules/{id}",
            )
            .about(
                "Read a schedule as it was created, whether it has been cancelled, and \
                 `publication_through_ms`: the scheduled time of the last occurrence \
                 published, null before the first. That is a publication watermark, not an \
                 acknowledgement that anything ran. A name that is not 1 to 64 letters, \
                 digits and hyphens is 400, not 404.",
            )
            .with(schedule_id())
            .repeatable()
            .answers(ok(
                200,
                "The schedule and its progress.",
                Some(json!({ "type": "object", "properties": {
                    "id": { "type": "string" },
                    "schedule": schedule_spec_schema(),
                    "cancelled": { "type": "boolean" },
                    "publication_through_ms": { "type": ["integer", "null"] } } })),
            ))
            .answers(jobs_fail(400, "the name is not a valid schedule name"))
            .answers(jobs_fail(404, "no such schedule"))
            .answers(jobs_store_failure())
            .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_publish_schedule",
                "Publish A Schedule's Due Occurrences",
                "POST",
                "/api/v1/schedules/{id}/publish",
            )
            .about(
                "Commit one bounded batch of the schedule's due occurrences: the records for \
                 the times after the watermark that are at or before `now_ms` (the server's \
                 wall clock when omitted), at most `limit` of them (1 to 1024, default 100), \
                 oldest first for `catch_up` or only the latest for `coalesce`. Each \
                 occurrence is stored with the schedule's job and its `not_before_ms` set to \
                 its time, then the watermark advances. This queues no job and runs \
                 nothing: it produces records that `hm jobs schedule dispatch` or `schedule \
                 worker` read. Repeating the call with the same clock returns nothing new \
                 once the watermark has reached it. `now_ms` is the caller's to choose: a \
                 value in the future commits occurrences that are not yet due, and a \
                 committed record cannot be withdrawn. Publishers race with exclusive file \
                 creation, so a competing writer is a 409 to retry. An empty batch is 200 \
                 with `[]`. The body must be JSON (`{}` is enough) and unknown fields are \
                 refused.",
            )
            .with(schedule_id())
            .takes(
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "now_ms": { "type": "integer", "description": "the horizon, Unix milliseconds; default the server's clock" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 1024, "default": 100 }
                    }
                }),
                true,
            )
            .answers(ok(
                200,
                "The occurrences committed by this call, possibly none.",
                Some(json!({ "type": "array", "items": occurrence_schema() })),
            ))
            .answers(jobs_fail(
                400,
                "the name is invalid, or `limit` is outside 1 to 1024",
            ))
            .answers(jobs_fail(404, "no such schedule"))
            .answers(jobs_fail(
                409,
                "the schedule is cancelled, or the publication progress changed while the \
                 batch was being committed",
            ))
            .answers(jobs_store_failure())
            .answers(body_rejection())
            .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_cancel_schedule",
                "Cancel A Schedule",
                "POST",
                "/api/v1/schedules/{id}/cancel",
            )
            .about(
                "Stop a schedule from publishing any further occurrence. It is permanent: the \
                 name cannot be reactivated or reused. Occurrences already committed stay \
                 readable and their dispatch claims and receipts are untouched, so it does \
                 not cancel guest work that was started. Cancelling a cancelled schedule \
                 succeeds. It races with publication for the same exclusive record: if a \
                 batch wins, the cancellation is retried behind it, up to 32 times, after \
                 which the call is a 409 and should be repeated.",
            )
            .with(schedule_id())
            .repeatable()
            .answers(ok(
                200,
                "Cancelled.",
                Some(json!({ "type": "object", "properties": {
                    "id": { "type": "string" }, "cancelled": { "type": "boolean" } } })),
            ))
            .answers(jobs_fail(400, "the name is not a valid schedule name"))
            .answers(jobs_fail(404, "no such schedule"))
            .answers(jobs_fail(
                409,
                "publication kept changing the progress for 32 attempts; repeat the call",
            ))
            .answers(jobs_store_failure())
            .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_list_occurrences",
                "List A Schedule's Committed Occurrences",
                "GET",
                "/api/v1/schedules/{id}/occurrences",
            )
            .about(
                "Read a page of the schedule's committed occurrence records in time order, \
                 after an exclusive `after_ms` cursor (the last `scheduled_ms` already \
                 consumed), at most `limit` of them (1 to 1024, default 100). Records of a \
                 publication that did not finish, and the losing batches of a competing \
                 coalesce, are not included. The read does not record that anyone consumed \
                 a record. Works on a cancelled schedule. Unknown query fields are a 400.",
            )
            .with(schedule_id())
            .with(query_param(
                "after_ms",
                "Exclusive cursor: only occurrences after this time",
                json!({ "type": "integer", "minimum": 0 }),
            ))
            .with(query_param(
                "limit",
                "Page size",
                json!({ "type": "integer", "minimum": 1, "maximum": 1024, "default": 100 }),
            ))
            .repeatable()
            .answers(ok(
                200,
                "The occurrences.",
                Some(json!({ "type": "array", "items": occurrence_schema() })),
            ))
            .answers(jobs_fail(400, "the name is invalid, or `limit` is outside 1 to 1024"))
            .answers(plain(400, "a query field is unknown or not a number (the framework's own rejection)"))
            .answers(jobs_fail(404, "no such schedule"))
            .answers(jobs_store_failure())
            .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_get_receipt",
                "Read An Occurrence's Dispatch Receipt",
                "GET",
                "/api/v1/schedules/{id}/receipts/{scheduled_ms}",
            )
            .about(
                "Read the durable record of a VM occurrence's dispatch: the claim (the \
                 occurrence, the worker that claimed it and the claim `token`) and the \
                 completion, which is null while the claim is unresolved. An unresolved \
                 claim means the guest command may be running, may have finished with no \
                 result recorded, or may never have started; the route does not execute or \
                 reconcile anything and does not say which. A completion holds the exit \
                 code, whether it timed out, up to 65536 bytes of each of stdout and stderr \
                 with truncation flags, and an `origin` (`api_response` for a result the \
                 dispatcher observed, `operator_recorded` for one recorded through \
                 `jobs_record_receipt` or the CLI, `unknown` for receipts written before \
                 the field existed). The response includes the claim token that \
                 `jobs_record_receipt` needs. Readable after the schedule is cancelled. 404 \
                 when the schedule or the claim does not exist, which is every occurrence \
                 of a schedule that has no VM target.",
            )
            .with(schedule_id())
            .with(scheduled_ms())
            .repeatable()
            .answers(ok(
                200,
                "The claim and its completion, or `completion: null`.",
                Some(json!({ "type": "object", "properties": {
                    "claim": { "type": "object", "properties": {
                        "occurrence": occurrence_schema(),
                        "worker": { "type": "string" },
                        "token": { "type": "string" } } },
                    "completion": { "oneOf": [completion_schema(), { "type": "null" }] } } })),
            ))
            .answers(jobs_fail(400, "the schedule name is invalid"))
            .answers(jobs_fail(404, "no such schedule, or no claim for that occurrence"))
            .answers(jobs_fail(
                500,
                "a stored claim or result is corrupt (for example a result with a different \
                 claim token, or output over the bounds), or the store failed",
            ))
            .answers(plain(400, "`scheduled_ms` is not an unsigned integer (the framework's own rejection)"))
            .answers(jobs_unauthorized()),
            Spec::new(
                "jobs_record_receipt",
                "Record An Operator-Verified Completion",
                "POST",
                "/api/v1/schedules/{id}/receipts/{scheduled_ms}",
            )
            .about(
                "Write the result of a VM occurrence whose claim is unresolved, from evidence \
                 the operator gathered outside this server. It checks the claim exists, that \
                 `claim_token` is the claim's token (from `jobs_get_receipt`) and that the \
                 result is within bounds (an exit code of 0 to 255 or null, at most 65536 \
                 bytes per stream), then stores it as an immutable completion with \
                 `origin: operator_recorded`, whatever `origin` the body sent. Recording \
                 identical content again succeeds; a different result, or the same content \
                 that an API response already recorded with another origin, is a 409. The \
                 server does not look at the guest: it trusts the assertion. Recording a \
                 completion is what lets the next occurrence of the schedule be claimed, so \
                 a wrong assertion can start guest work that overlaps a command that is \
                 still running. The operator must establish that the original execution \
                 has finished. There is no way to reset or retry a claim.",
            )
            .with(schedule_id())
            .with(scheduled_ms())
            .takes(completion_schema(), true)
            .repeatable()
            .answers(ok(
                200,
                "Recorded (or recorded already, identically).",
                Some(json!({ "type": "object", "properties": {
                    "schedule_id": { "type": "string" },
                    "scheduled_ms": { "type": "integer" },
                    "completion_recorded": { "type": "boolean" } } })),
            ))
            .answers(jobs_fail(
                400,
                "the exit code is outside 0 to 255, a stream is over 65536 bytes, or the \
                 schedule name is invalid",
            ))
            .answers(jobs_fail(404, "no such schedule, or no claim for that occurrence"))
            .answers(jobs_fail(
                409,
                "`claim_token` is not the claim's token, or a different result is already \
                 recorded",
            ))
            .answers(jobs_store_failure())
            .answers(body_rejection())
            .answers(jobs_unauthorized()),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }

    /// The capabilities of the local servers.
    pub(super) fn build_local_server_capabilities() -> Vec<Capability> {
        let hm_key = "`HM_API_KEY` as a bearer token on every route except GET /health, when \
                      the server was started with one (none by default)";
        let jobs_token = "the `--token` / `HM_JOBS_TOKEN` bearer token, when the server has one \
                          (required off loopback)";
        let capability = |id: &str,
                          name: &str,
                          description: &str,
                          category: CapabilityCategory,
                          ids: &[&str],
                          prerequisites: &[&str],
                          permissions: &[&str]| {
            super::spec::api_capability(
                id,
                name,
                description,
                category,
                ids,
                prerequisites,
                permissions,
            )
        };
        vec![
            capability(
                "hm_cli_local_vms",
                "Local VM Registry (hm serve)",
                "Create, start, stop, inspect and delete VMs of the local registry that `hm \
                 serve` manages in its own process, and run a Rhai script against a running \
                 one. This is the single-machine server: it binds 127.0.0.1 by default, has no TLS, and \
                 asks for a key on every route except GET /health when `HM_API_KEY` is set. \
                 With no key it refuses a non-loopback bind unless `--allow-unauthenticated` is \
                 given. See docs/AGENTIC_INTERFACE.md and \
                 docs/src/ai/mcp-server.md",
                CapabilityCategory::VirtualMachine,
                &[
                    "hm_list_vms",
                    "hm_create_vm",
                    "hm_get_vm",
                    "hm_delete_vm",
                    "hm_start_vm",
                    "hm_stop_vm",
                    "hm_get_vm_metrics",
                    "hm_execute_vm_script",
                ],
                &["`hm serve` running (REST port 8080 by default)"],
                &[hm_key],
            ),
            capability(
                "hm_cli_tool_calls",
                "Local Tool Calls (hm serve)",
                "List the eight `vm.*` tools and call them through one route, in the shape \
                 OpenAI function calling and Anthropic tool use take. It is the local server's \
                 own JSON interface, not the Model Context Protocol transport, and tool \
                 failures answer 200 with `success: false`.",
                CapabilityCategory::AgentExecution,
                &["hm_list_tools", "hm_call_tool"],
                &["`hm serve` running"],
                &[hm_key],
            ),
            capability(
                "hm_cli_agent_discovery",
                "Local Server Discovery And Health",
                "Read the local server's own ontology, schemas, capabilities summary and \
                 per-provider tool lists, and its health. Health needs no key; the rest need \
                 `HM_API_KEY` when it is set. They describe the `vm.*` tools only.",
                CapabilityCategory::Monitoring,
                &[
                    "hm_get_health",
                    "hm_get_agentic_ontology",
                    "hm_get_agentic_schema",
                    "hm_get_agentic_schema_compact",
                    "hm_get_agentic_capabilities",
                    "hm_get_provider_config",
                    "hm_get_openai_tools",
                    "hm_get_anthropic_tools",
                    "hm_get_gemini_tools",
                ],
                &["`hm serve` running"],
                &[],
            ),
            capability(
                "hm_cli_mcp_http",
                "Sandbox MCP Over HTTP (hm sandbox vm mcp-http)",
                "Model Context Protocol 2025-11-25 over a JSON-response HTTP transport at \
                 `/mcp`, in front of the sandbox API the command was configured with: \
                 twelve lifecycle, exec and checkpoint tools, and two file tools when an \
                 envd route is configured. One operator-set bearer token (`HM_MCP_TOKEN`, \
                 32 to 4096 visible bytes) and an exact-origin allowlist; the listener \
                 refuses any address but loopback and has no TLS, so remote use goes through \
                 an operator's HTTPS proxy. See docs/src/ai/mcp-server.md",
                CapabilityCategory::AgentExecution,
                &["hm_mcp_http_post", "hm_mcp_http_get", "hm_mcp_http_delete"],
                &[
                    "`hm sandbox vm mcp-http` running, with `HM_MCP_TOKEN` set",
                    "a sandbox API endpoint and `HV2_API_KEY` for the role the agent gets",
                ],
                &["`HM_MCP_TOKEN` as a bearer token on every request"],
            ),
            capability(
                "jobs_queue",
                "Jobs Queue (hm jobs serve)",
                "Queue host programs, list and read jobs, read their logs (optionally \
                 streamed) and cancel them, over the REST mirror of `hm jobs`. The queue is a \
                 directory shared with every worker started on it; submitting runs a \
                 program on a worker's host. See docs/JOBS.md",
                CapabilityCategory::Compute,
                &[
                    "jobs_submit",
                    "jobs_list",
                    "jobs_get",
                    "jobs_get_logs",
                    "jobs_cancel",
                ],
                &[
                    "`hm jobs serve` running (127.0.0.1:7878 by default)",
                    "workers on the store",
                ],
                &[jobs_token],
            ),
            capability(
                "jobs_schedules",
                "Job Schedules (hm jobs serve)",
                "Create immutable interval or cron schedules, publish their due occurrences \
                 as records, read the records and cancel a schedule. Publication stores \
                 records; it queues and runs nothing, and the existing `hm jobs worker` \
                 does not read them. See docs/JOBS.md",
                CapabilityCategory::Compute,
                &[
                    "jobs_list_schedules",
                    "jobs_create_schedule",
                    "jobs_get_schedule",
                    "jobs_publish_schedule",
                    "jobs_cancel_schedule",
                    "jobs_list_occurrences",
                ],
                &["`hm jobs serve` running"],
                &[jobs_token],
            ),
            capability(
                "jobs_vm_receipts",
                "VM Occurrence Receipts (hm jobs serve)",
                "Read the durable claim and completion of a VM schedule occurrence, and record \
                 a completion an operator verified by other means so that the next occurrence \
                 may be claimed. Recovery for dispatch that ended without a result.",
                CapabilityCategory::Compute,
                &["jobs_get_receipt", "jobs_record_receipt"],
                &["a schedule with a `vm` target and a claimed occurrence"],
                &[jobs_token],
            ),
        ]
    }

    /// The guardrails of the local servers, each from a document and the code.
    pub(super) fn local_server_guardrails() -> Vec<Guardrail> {
        let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        vec![
            Guardrail {
                id: "hm-serve-open-and-plain-by-default".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "`hm serve` is plain HTTP, and open to every caller when started with \
                    `--allow-unauthenticated`"
                    .to_string(),
                description: "`hm serve` binds `127.0.0.1` by default and refuses a \
                    non-loopback bind unless `HM_API_KEY` is set or `--allow-unauthenticated` \
                    is given. With a key set, every route except `GET /health` requires \
                    `Authorization: Bearer <key>`, checked against the key byte by byte with no \
                    early exit (a length mismatch is rejected first). With no key, no route \
                    checks anything: that is the default only on loopback, where only local \
                    processes can connect, and it is what `--allow-unauthenticated` accepts \
                    elsewhere. Traffic is plain HTTP with no TLS, so the key and the responses \
                    cross the network unencrypted. CORS allows every origin, method and header. \
                    The rate limit (100 requests a minute) is keyed on the socket peer address; \
                    `X-Forwarded-For` is honoured only from `--trusted-proxy` peers, so a caller \
                    cannot choose its own bucket with the header. The per-client map of the \
                    limiter is not bounded. There is one key for every caller and no per-route \
                    permission."
                    .to_string(),
                applies_to: [
                    "hm_cli_local_vms",
                    "hm_cli_tool_calls",
                    "hm_create_vm",
                    "hm_start_vm",
                    "hm_delete_vm",
                    "hm_execute_vm_script",
                    "hm_call_tool",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                condition: "`hm serve` runs with `--allow-unauthenticated`, or with \
                    `HM_API_KEY` on a path where other parties can read or alter plain HTTP \
                    traffic."
                    .to_string(),
                mitigation: "Keep the default `127.0.0.1` bind. For any other address set \
                    `HM_API_KEY` and front the port with a TLS proxy that authenticates callers \
                    and sets `X-Forwarded-For` itself, and pass that proxy's address as \
                    `--trusted-proxy`. For TLS and per-user rights use the control plane or \
                    `hm sandbox vm mcp-http`."
                    .to_string(),
                reference: "docs/src/ai/mcp-server.md".to_string(),
            },
            Guardrail {
                id: "mcp-http-single-shared-credential".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "The MCP HTTP transport has one bearer token and one API role".to_string(),
                description: "`hm sandbox vm mcp-http` accepts one `HM_MCP_TOKEN` and then acts \
                    with the single sandbox API role the process was configured with \
                    (`HV2_API_KEY`), for every caller that holds the token: there is no \
                    per-user authorization, tenant isolation, OAuth, token rotation or browser \
                    sign-in. It refuses any listen address but loopback and has no TLS, so \
                    remote use depends on an operator's proxy that keeps the `Authorization`, \
                    `Origin` and MCP headers. A request with no `Origin` header is accepted \
                    (native clients send none), so the origin allowlist protects browsers \
                    only. Sessions expire ten minutes after they are created, not after use, \
                    and at most 64 exist. Cancelling a request releases the client's wait; \
                    work the sandbox API already accepted can continue, and a disconnect \
                    alone cancels nothing."
                    .to_string(),
                applies_to: [
                    "hm_cli_mcp_http",
                    "hm_mcp_http_post",
                    "hm_mcp_http_get",
                    "hm_mcp_http_delete",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                condition: "More than one person or agent can use the endpoint, or the token \
                    is shared beyond the process that needs it."
                    .to_string(),
                mitigation: "Run one process per role with its own token and a role-limited \
                    API key (an observer role caps even an admin scope), keep the token out of \
                    logs, and terminate TLS in a proxy that you control."
                    .to_string(),
                reference: "docs/src/ai/mcp-server.md".to_string(),
            },
            Guardrail {
                id: "jobs-serve-is-remote-execution".to_string(),
                severity: GuardrailSeverity::Critical,
                title: "The jobs server runs programs on the workers' hosts for its callers"
                    .to_string(),
                description: "`jobs_submit` queues a program that a worker then runs as its own \
                    user, in the sandbox the job's own spec asks for. The spec can set the \
                    network to `host` and the filesystem to `host` (the defaults deny the \
                    network only), so the route is remote code execution on every host that \
                    runs workers over the store. With no token the server answers everyone \
                    who can reach it; `hm jobs serve` refuses a non-loopback address without \
                    one, but a loopback listener with no token is open to every local user and \
                    every process, and one token is shared by every caller. The schedule \
                    routes use the same token. Job specs, including `env` values, are read \
                    back by `jobs_get`, and logs by `jobs_get_logs`."
                    .to_string(),
                applies_to: strings(&[
                    "jobs_queue",
                    "jobs_schedules",
                    "jobs_submit",
                    "jobs_get",
                    "jobs_get_logs",
                    "jobs_create_schedule",
                ]),
                condition: "`hm jobs serve` is reachable by anyone who must not run programs on \
                    the workers, or started without a token."
                    .to_string(),
                mitigation: "Always set `--token` (or `HM_JOBS_TOKEN`), keep the listener on \
                    loopback or behind TLS, put no secrets in `env`, and run workers as an \
                    unprivileged user with `strict` sandbox settings where the host supports \
                    them."
                    .to_string(),
                reference: "docs/JOBS.md".to_string(),
            },
            Guardrail {
                id: "schedule-receipts-trust-the-operator".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "Recording a completion is an assertion that unblocks the next occurrence"
                    .to_string(),
                description: "`jobs_record_receipt` does not look at the guest. It checks the \
                    claim token (which `jobs_get_receipt` returns to any caller with the \
                    server's token) and the result's bounds, and stores the result as \
                    immutable. A completion is what lets the next occurrence of the schedule be \
                    claimed, so recording one for a command that is still running can start \
                    overlapping guest work; there is no retry or claim reset to undo it. An \
                    unresolved claim means the guest command may be running, finished or never \
                    started, and a lost worker is not evidence that running it again is safe. \
                    Ordering is per schedule: two schedules aimed at one VM are not excluded \
                    from each other."
                    .to_string(),
                applies_to: strings(&[
                    "jobs_vm_receipts",
                    "jobs_record_receipt",
                    "jobs_get_receipt",
                ]),
                condition: "A claim is unresolved and someone considers recording a result."
                    .to_string(),
                mitigation: "Establish independently that the original execution has finished \
                    and keep that evidence; do not record a result to get past a claim."
                    .to_string(),
                reference: "docs/JOBS.md".to_string(),
            },
        ]
    }
}
