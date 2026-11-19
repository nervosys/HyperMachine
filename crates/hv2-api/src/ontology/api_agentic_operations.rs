//! `hv2-api`'s discovery and planning routes under `/agentic` and
//! `/.well-known` (`ontology.rs`, `create_ontology_router_with_executor`).
//! Modelled as data from the handlers. They are the ontology describing
//! itself: everything they return is built from `HyperMachineOntology::build()`
//! on each request; only `plans/execute` can reach a VM.

use super::spec::{
    api_key_failures, body_rejection, json_content_type_required, ok, path_param, query_param, Spec,
};
use super::{HyperMachineOntology, Operation, OperationResponse};
use serde_json::{json, Value};

fn plan_schema() -> Value {
    json!({
        "type": "object",
        "required": ["name", "description", "steps", "rollback_on_failure"],
        "properties": {
            "name": { "type": "string" },
            "description": { "type": "string" },
            "steps": { "type": "array", "items": { "type": "object",
                "required": ["step_id", "operation_id", "parameters", "depends_on"],
                "properties": {
                    "step_id": { "type": "string", "description": "unique in the plan" },
                    "operation_id": { "type": "string", "description": "an `id` from the ontology's operations" },
                    "parameters": { "type": "object", "description": "the operation's parameters; `${name}` is replaced from `variables`" },
                    "depends_on": { "type": "array", "items": { "type": "string" } },
                    "timeout_seconds": { "type": ["integer", "null"] } } } },
            "rollback_on_failure": { "type": "boolean" }
        }
    })
}

fn validation_schema() -> Value {
    json!({ "type": "object", "properties": {
        "valid": { "type": "boolean" },
        "errors": { "type": "array", "items": { "type": "object", "properties": {
            "step_id": { "type": "string" }, "error_type": { "type": "string" }, "message": { "type": "string" } } } },
        "warnings": { "type": "array", "items": { "type": "string" } },
        "estimated_duration_ms": { "type": ["integer", "null"] },
        "resolved_steps": { "type": "array", "items": { "type": "object" } } } })
}

fn execution_schema() -> Value {
    json!({ "type": "object", "properties": {
        "execution_id": { "type": "string" },
        "plan_name": { "type": "string" },
        "status": { "type": "string", "enum": ["Completed", "Failed", "ValidationFailed", "RolledBack"] },
        "step_results": { "type": "array", "items": { "type": "object", "properties": {
            "step_id": { "type": "string" }, "operation_id": { "type": "string" },
            "success": { "type": "boolean" }, "duration_ms": { "type": "integer" },
            "output": {}, "error": { "type": ["string", "null"] }, "rolled_back": { "type": "boolean" } } } },
        "duration_ms": { "type": "integer" },
        "rolled_back_steps": { "type": "array", "items": { "type": "string" } },
        "validation": validation_schema() } })
}

fn json_answer(description: &str, schema: Value) -> OperationResponse {
    ok(200, description, Some(schema))
}

impl HyperMachineOntology {
    /// The `/agentic` and `/.well-known` routes.
    pub(super) fn build_api_agentic_operations() -> Vec<Operation> {
        let keys = api_key_failures;
        let discovery_note = "Built from the ontology on every request; nothing is cached or \
                              stored. A GET under /agentic is exempt from the API key even when \
                              one is configured, so it answers anyone who can reach the port.";

        let specs = vec![
            Spec::new(
                "get_ontology",
                "Read The Agent Ontology",
                "GET",
                "/agentic/ontology",
            )
            .about(format!(
                "Read the whole ontology this document describes (capabilities, operations, \
                 resource types, state machines, guardrails and the rest) as JSON-LD \
                 (`application/ld+json`), or with `format` as the tool list of one provider. \
                 Any other `format`, including `openapi` (the code's comment lists it, the \
                 handler does not serve it), falls back to JSON-LD. It describes what \
                 components serve; it is not the API of this server alone, and it carries \
                 operations of the cluster control plane and the node, which this server does \
                 not route. {discovery_note}"
            ))
            .with(query_param(
                "format",
                "`openai`, `anthropic` or `gemini` for that provider's tool list; anything else is JSON-LD",
                json!({ "type": "string", "enum": ["json-ld", "openai", "anthropic", "gemini"], "default": "json-ld" }),
            ))
            .answers(json_answer("The ontology (or the tool list).", json!({ "type": "object" })))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_openai_tools",
                "Read The Tool List (OpenAI Format)",
                "GET",
                "/agentic/tools/openai",
            )
            .about(format!(
                "Read the agent-callable tools (`vm.*`, `guest.*`, `snapshot.*`, `network.*`, \
                 `gpu.*`, `agent.*`, `system.*`) in OpenAI function-calling form. These are \
                 the MCP registry's tools, one entry each, not this crate's REST operations; \
                 a tool that a guardrail names carries a `WARNING` with its mitigation \
                 appended to its description. Listing a tool calls nothing: running one is \
                 the MCP server's or the agent runtime's business. {discovery_note}"
            ))
            .answers(json_answer(
                "The tools.",
                json!({ "type": "object", "properties": { "tools": { "type": "array", "items": { "type": "object", "properties": {
                    "type": { "type": "string", "enum": ["function"] },
                    "function": { "type": "object", "properties": {
                        "name": { "type": "string" }, "description": { "type": "string" }, "parameters": { "type": "object" } } } } } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_anthropic_tools",
                "Read The Tool List (Anthropic Format)",
                "GET",
                "/agentic/tools/anthropic",
            )
            .about(format!(
                "The same tools as `get_openai_tools` in Anthropic tool-use form (`name`, \
                 `description`, `input_schema`), under the ontology's name and version. \
                 Guardrail warnings are in the descriptions. {discovery_note}"
            ))
            .answers(json_answer(
                "The tools.",
                json!({ "type": "object", "properties": {
                    "name": { "type": "string" }, "version": { "type": "string" },
                    "tools": { "type": "array", "items": { "type": "object", "properties": {
                        "name": { "type": "string" }, "description": { "type": "string" }, "input_schema": { "type": "object" } } } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_gemini_tools",
                "Read The Tool List (Gemini Format)",
                "GET",
                "/agentic/tools/gemini",
            )
            .about(format!(
                "The same tools as `get_openai_tools` as Gemini `functionDeclarations`, with \
                 the dots in a tool's name replaced (`vm.create` is `vm_create`). Guardrail \
                 warnings are in the descriptions. {discovery_note}"
            ))
            .answers(json_answer(
                "The declarations.",
                json!({ "type": "object", "properties": { "functionDeclarations": { "type": "array", "items": { "type": "object", "properties": {
                    "name": { "type": "string" }, "description": { "type": "string" }, "parameters": { "type": "object" } } } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_affordances",
                "Read What A Resource Can Do In A State",
                "GET",
                "/agentic/affordances/{resource_type}/{state}",
            )
            .about(format!(
                "List the operations the ontology allows on a resource type in a given state, \
                 with each one's preconditions, and the states reachable from there. A \
                 resource type or state the ontology does not know is not an error: it answers \
                 200 with empty lists. The answer comes from the ontology's state machines, \
                 not from any live VM. {discovery_note}"
            ))
            .with(path_param("resource_type", "A resource type of the ontology (`vm`)", json!({ "type": "string" })))
            .with(path_param("state", "A state of that type's state machine (`running`)", json!({ "type": "string" })))
            .answers(json_answer(
                "The affordances.",
                json!({ "type": "object", "properties": {
                    "resource_type": { "type": "string" },
                    "current_state": { "type": "string" },
                    "available_operations": { "type": "array", "items": { "type": "object" } },
                    "possible_transitions": { "type": "array", "items": { "type": "object" } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "validate_plan",
                "Validate An Action Plan",
                "POST",
                "/agentic/plans/validate",
            )
            .about(
                "Check a plan against the ontology without running it: that each step names \
                 an operation of the ontology, that dependencies exist and are acyclic, and \
                 that the parameters and the resource states the operations need hold, with \
                 an estimated duration and each step's resolved preconditions. Always 200: read \
                 `valid` and `errors`. A plan that validates names operations the ontology \
                 describes, and that is not a promise that `execute_plan` can carry them out \
                 (only nine VM operations have an executor). Executes nothing and reads no VM. \
                 A POST that changes nothing, so repeatable. It is not exempt from the API \
                 key when one is configured.",
            )
            .takes(plan_schema(), true)
            .answers(json_answer("The verdict.", validation_schema()))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "execute_plan",
                "Execute An Action Plan",
                "POST",
                "/agentic/plans/execute",
            )
            .about(
                "Validate a plan and run its steps in dependency order against this server's \
                 own VM inventory (the one `/api/v1/vms` serves). `create_vm`, `start_vm`, \
                 `stop_vm`, `pause_vm`, `resume_vm`, `delete_vm`, `get_vm`, `list_vms` and \
                 `get_metrics` are carried out for real: a plan can create, start, stop and \
                 delete VMs, with `${name}` in parameters replaced from `variables`, and VMs it \
                 creates are gated by the image registry exactly as `create_vm`'s are. Every \
                 other operation in the ontology, `exec_in_vm`, the image routes and the \
                 runtime routes included, is refused for that step with `has no VM-host \
                 implementation; it is available only in simulated execution`, and the plan \
                 stops there. With `rollback_on_failure` the steps that succeeded are undone by \
                 their compensating action (a created VM is deleted, a started one stopped); \
                 a deleted VM cannot be restored, and a step whose undo fails is reported not \
                 rolled back. `dry_run: true` validates and runs nothing. The answer is 200 \
                 whatever happened: read `status` (`Completed`, `Failed`, `ValidationFailed`, \
                 `RolledBack`) and each step's `success` and `error`. `timeout_seconds` is \
                 accepted and not enforced. Not idempotent. Not exempt from the API key when \
                 one is configured, which is the only thing between a caller and every VM on \
                 the server: with none, any caller can run it.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["plan"],
                    "properties": {
                        "plan": plan_schema(),
                        "dry_run": { "type": ["boolean", "null"], "description": "true: validate only" },
                        "timeout_seconds": { "type": ["integer", "null"], "description": "accepted, not enforced" },
                        "variables": { "type": ["object", "null"], "description": "values for `${name}` in step parameters" }
                    }
                }),
                true,
            )
            .answers(json_answer("What happened.", execution_schema()))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys()),
            Spec::new(
                "get_composition_rules",
                "Read The Composition Rules",
                "GET",
                "/agentic/compose",
            )
            .about(format!(
                "Read the ontology's pre-defined multi-step workflows, the constraints on \
                 composing operations and reusable composition patterns. Static data of the \
                 ontology; it neither runs nor checks anything. {discovery_note}"
            ))
            .answers(json_answer(
                "The rules.",
                json!({ "type": "object", "properties": {
                    "workflows": { "type": "array", "items": { "type": "object" } },
                    "constraints": { "type": "array", "items": { "type": "object" } },
                    "patterns": { "type": "array", "items": { "type": "object" } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_resource_graph",
                "Read The Resource Graph",
                "GET",
                "/agentic/graph",
            )
            .about(format!(
                "Read the ontology's resource types as a graph: a node per type with the \
                 number of operations that act on it, and an edge per relationship. A map \
                 of the ontology, not of any running resource. {discovery_note}"
            ))
            .answers(json_answer(
                "The graph.",
                json!({ "type": "object", "properties": {
                    "nodes": { "type": "array", "items": { "type": "object", "properties": {
                        "id": { "type": "string" }, "resource_type": { "type": "string" },
                        "label": { "type": "string" }, "operations_count": { "type": "integer" } } } },
                    "edges": { "type": "array", "items": { "type": "object" } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_operation_contracts",
                "Read The Operation Contracts",
                "GET",
                "/agentic/contracts",
            )
            .about(format!(
                "Read each contracted operation's preconditions, postconditions, invariants \
                 and the operations it composes with or excludes. The contracts are written \
                 for the VM operations (not every operation in the ontology has one), and are \
                 what `validate_plan` and `get_affordances` consult. {discovery_note}"
            ))
            .answers(json_answer(
                "The contracts.",
                json!({ "type": "array", "items": { "type": "object", "properties": {
                    "operation_id": { "type": "string" },
                    "preconditions": { "type": "array", "items": { "type": "object" } },
                    "postconditions": { "type": "array", "items": { "type": "object" } },
                    "invariants": { "type": "array", "items": { "type": "string" } },
                    "composable_with": { "type": "array", "items": { "type": "string" } },
                    "mutually_exclusive_with": { "type": "array", "items": { "type": "string" } } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_mcp_manifest",
                "Read The MCP Manifest",
                "GET",
                "/agentic/mcp",
            )
            .about(format!(
                "Read the ontology as a Model Context Protocol manifest: name, version, \
                 protocol version, the capability flags, the tools (with guardrail warnings \
                 in their descriptions) and resources. It is a description for a client; this \
                 route does not speak MCP and runs no tool. {discovery_note}"
            ))
            .answers(json_answer(
                "The manifest.",
                json!({ "type": "object", "properties": {
                    "name": { "type": "string" }, "version": { "type": "string" },
                    "protocol_version": { "type": "string" },
                    "capabilities": { "type": "object" },
                    "tools": { "type": "array", "items": { "type": "object" } },
                    "resources": { "type": "array", "items": { "type": "object" } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_ai_plugin_manifest",
                "Read The ChatGPT Plugin Manifest",
                "GET",
                "/.well-known/ai-plugin.json",
            )
            .about(
                "Read the plugin manifest ChatGPT-style clients look for: names, a description \
                 for a model, `auth.type` of `service_http`, and `api.url` pointing at \
                 `/agentic/openapi.yaml`, which this server does not route (it answers 404, \
                 or the JSON fallback of the middleware), so a client that follows it finds no \
                 OpenAPI document. The logo, contact and legal URLs are fixed values. Static. \
                 A GET under /.well-known is not on the API key's exemption list, so with a \
                 key configured it needs the key, unlike `/agentic`.",
            )
            .answers(json_answer(
                "The manifest.",
                json!({ "type": "object", "properties": {
                    "schema_version": { "type": "string" },
                    "name_for_human": { "type": "string" },
                    "name_for_model": { "type": "string" },
                    "description_for_human": { "type": "string" },
                    "description_for_model": { "type": "string" },
                    "auth": { "type": "object", "properties": { "type": { "type": "string" } } },
                    "api": { "type": "object", "properties": { "type": { "type": "string" }, "url": { "type": "string" } } },
                    "logo_url": { "type": "string" },
                    "contact_email": { "type": "string" },
                    "legal_info_url": { "type": "string" } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_agent_card",
                "Read The Agent Card",
                "GET",
                "/.well-known/agent.json",
            )
            .about(
                "Read the agent card an agent-to-agent client discovers: name, version, \
                 description, `url` (`/agentic`), capability flags, the skills with examples, \
                 the authentication it claims (`bearer`, schemes `Ed25519-JWT` and `mTLS`) and \
                 the input and output media types. The claimed schemes are not what this \
                 server's REST middleware checks: it compares a configured API key and has no \
                 JWT or client-certificate check. Static. Not on the API key's exemption \
                 list, so with a key configured it needs the key.",
            )
            .answers(json_answer(
                "The card.",
                json!({ "type": "object", "properties": {
                    "name": { "type": "string" }, "version": { "type": "string" },
                    "description": { "type": "string" }, "url": { "type": "string" },
                    "capabilities": { "type": "object" },
                    "skills": { "type": "array", "items": { "type": "object" } },
                    "authentication": { "type": "object", "properties": {
                        "type": { "type": "string" }, "schemes": { "type": "array", "items": { "type": "string" } } } },
                    "default_input_modes": { "type": "array", "items": { "type": "string" } },
                    "default_output_modes": { "type": "array", "items": { "type": "string" } } } }),
            ))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "list_plan_templates",
                "List Plan Templates",
                "GET",
                "/agentic/templates",
            )
            .about(format!(
                "List the plan templates the ontology ships (id, name, description, category, \
                 version, tags, parameters and the plan), optionally by `category` (a substring \
                 of the category's JSON name) or exact `tag`. The templates are compiled in: \
                 there is no way to add one. {discovery_note}"
            ))
            .with(query_param("category", "A substring of the category's serialized name", json!({ "type": "string" })))
            .with(query_param("tag", "Only templates with exactly this tag", json!({ "type": "string" })))
            .answers(json_answer("The templates.", json!({ "type": "array", "items": { "type": "object" } })))
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "instantiate_plan_template",
                "Instantiate A Plan Template",
                "POST",
                "/agentic/templates",
            )
            .about(
                "Bind parameters to a template and answer the plan it makes, its validation, \
                 the defaults applied and the required parameters still missing. With \
                 `execute: true` it also runs the plan, but only as a simulation: this route \
                 has no executor, so the steps return synthetic results and touch no VM, \
                 unlike `execute_plan`; `dry_run` applies to that simulation. An unknown \
                 template, or a missing required parameter, is still a 200 with \
                 `validation.valid: false` and the reason in `errors`. A POST that changes \
                 nothing, so repeatable. Not exempt from the API key when one is configured.",
            )
            .takes(
                json!({
                    "type": "object",
                    "required": ["template_id"],
                    "properties": {
                        "template_id": { "type": "string" },
                        "parameters": { "type": "object", "default": {} },
                        "execute": { "type": "boolean", "default": false, "description": "simulate the plan too" },
                        "dry_run": { "type": ["boolean", "null"] }
                    }
                }),
                true,
            )
            .answers(json_answer(
                "The plan and its checks.",
                json!({ "type": "object", "properties": {
                    "template_id": { "type": "string" },
                    "plan": plan_schema(),
                    "validation": validation_schema(),
                    "execution": execution_schema(),
                    "defaults_applied": { "type": "array", "items": { "type": "string" } },
                    "missing_parameters": { "type": "array", "items": { "type": "string" } } } }),
            ))
            .answers(body_rejection())
            .answers(json_content_type_required())
            .answers_all(keys())
            .repeatable(),
            Spec::new(
                "get_plan_template",
                "Read A Plan Template",
                "GET",
                "/agentic/templates/{id}",
            )
            .about(format!(
                "Read one template by id: its description, category, version, tags, \
                 parameters (with defaults and whether required) and the plan. 404 \
                 `template_not_found` for an unknown id. {discovery_note}"
            ))
            .with(path_param("id", "The template's id", json!({ "type": "string" })))
            .answers(json_answer("The template.", json!({ "type": "object" })))
            .answers(OperationResponse {
                status_code: 404,
                description: "no template of that id".to_string(),
                schema: Some(json!({ "type": "object", "properties": {
                    "error": { "type": "string", "enum": ["template_not_found"] },
                    "message": { "type": "string" } } })),
            })
            .answers_all(keys())
            .repeatable(),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }
}
