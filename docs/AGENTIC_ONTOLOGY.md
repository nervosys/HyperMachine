# HyperMachine Agentic Ontology

**Version:** 1.0.0  
**Last Updated:** 2026-02-02  

---

## Overview

HyperMachine is designed as an **agent-first** hypervisor. AI agents (Claude, GPT, Gemini, etc.) are primary users, with human operators providing oversight and strategic direction.

This document describes the **Agentic Ontology** - a machine-readable API description that enables AI agents to discover and use HyperMachine capabilities autonomously.

---

## Quick Start for AI Agents

### 1. Discover Capabilities

```bash
# Get the full ontology (JSON-LD)
curl https://hypermachine.local/agentic/ontology

# Get OpenAI function calling format
curl https://hypermachine.local/agentic/tools/openai

# Get Anthropic MCP format
curl https://hypermachine.local/agentic/tools/anthropic

# Get Google Gemini format
curl https://hypermachine.local/agentic/tools/gemini
```

### 2. Authenticate

```bash
# Request a token
curl -X POST https://hypermachine.local/api/v1/auth/token \
  -H "Content-Type: application/json" \
  -d '{"api_key": "your-api-key"}'

# Use the token
curl https://hypermachine.local/api/v1/vms \
  -H "Authorization: Bearer <token>"
```

### 3. Perform Operations

```bash
# Create a VM
curl -X POST https://hypermachine.local/api/v1/vms \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"name": "ai-worker", "vcpu_count": 4, "memory_gb": 16}'

# Execute an agent script
curl -X POST https://hypermachine.local/api/v1/vms/ai-worker/script \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"script": "let metrics = get_metrics(); metrics.cpu_usage"}'
```

---

## Ontology Structure

### JSON-LD Context

The ontology uses JSON-LD for semantic web compatibility:

```json
{
  "@context": {
    "@vocab": "https://schema.nervosys.ai/hypermachine#",
    "schema": "https://schema.org/",
    "hm": "https://nervosys.ai/hypermachine/ontology/",
    "dcterms": "http://purl.org/dc/terms/"
  }
}
```

### Core Components

| Component        | Description                                           |
| ---------------- | ----------------------------------------------------- |
| `system`         | API identification and authentication info            |
| `capabilities`   | High-level features the system provides               |
| `resources`      | Resource types that can be managed (VM, Agent, etc.)  |
| `operations`     | Specific API operations with parameters and responses |
| `state_machines` | Valid state transitions for resources                 |
| `events`         | Event types for subscriptions                         |
| `guardrails`     | Warnings attached to interfaces (see [Guardrails](#guardrails)) |

---

## Capabilities

### Virtual Machine Management (`vm_management`)

Create, configure, and manage virtual machines.

**Operations:**
- `create_vm` - Create a new VM
- `delete_vm` - Delete an existing VM
- `get_vm` - Get VM details
- `list_vms` - List all VMs
- `update_vm` - Update VM configuration

**Permissions Required:** `vm:create`, `vm:read`

### VM Lifecycle Control (`vm_lifecycle`)

Control VM execution state.

**Operations:**
- `start_vm` - Start a stopped VM
- `stop_vm` - Gracefully stop a running VM
- `pause_vm` - Suspend a running VM; its vCPUs stop executing until resumed
- `resume_vm` - Continue a suspended VM
- `snapshot_vm` - Create a snapshot
- `restore_vm` - Restore from snapshot

**Permissions Required:** `vm:control`

### AI Agent Execution (`agent_execution`)

Execute sandboxed scripts within VMs.

**Operations:**
- `execute_script` - Run a Rhai script on the host against a read-only view of a VM
- `list_agents` - List active agents
- `get_agent_logs` - Get agent execution logs

**Permissions Required:** `agent:execute`

### GPU Passthrough (`gpu_passthrough`)

Manage GPU devices for AI/ML workloads.

**Operations:**
- `attach_gpu` - Attach a GPU to a VM
- `detach_gpu` - Detach a GPU from a VM
- `list_gpus` - List available GPUs

**Permissions Required:** `gpu:manage`

### Metrics & Monitoring (`metrics_monitoring`)

Collect and query performance metrics.

**Operations:**
- `get_metrics` - Get current VM metrics
- `query_metrics` - Query historical metrics
- `set_alert` - Configure metric alerts

**Permissions Required:** `metrics:read`

### Managed API Keys (`managed_api_keys`)

Make, list and revoke API keys at run time. These routes are served by the
**cluster control plane** (`hv2-control-plane`), not by `hv2-api`; the
ontology models them as data. They authenticate with the `X-API-Key` header.
Keys live in the cluster store, sealed under `--managed-key-secret-file`;
without it every route answers 503. Only an administrator (the legacy admin
key, or an operator key with the `admin` scope) or a `team-admin` may call
them (others: 403). A team-admin is confined to its own team and cannot
raise anyone above itself. There is no rotate route: create a replacement,
move clients over, then revoke the old key. Policy-file keys are not
reachable here ([API_KEY_ROTATION.md](API_KEY_ROTATION.md)).

| Operation (`id`)      | Method and path                | Notes |
| --------------------- | ------------------------------ | ----- |
| `create_managed_key`  | `POST /keys`                   | 201 with the secret, shown once. `label` and `expiresInSeconds` (60 s to 90 days by default) required; 429 past the per-team limit |
| `list_managed_keys`   | `GET /keys`                    | Never a secret or digest; a team-admin sees its team's non-administrator keys |
| `revoke_managed_key`  | `DELETE /keys/{id}`            | Writes a sealed revocation record, then removes the key; 404 if the store holds no such key |
| `list_team_members`   | `GET /teams/{team}/members`    | A team-admin: own team only (403 otherwise) |

**Permissions Required:** `administrator` or `team-admin`.
**Prerequisite:** control plane started with `--managed-key-secret-file`.

### Single Sign-On (`sso_sign_in`)

Sign people in to the cluster control plane through an OpenID Connect
provider, from a browser or the CLI. Served by the **control plane**, outside
its API key check (they are how a person without a key gets a session), and
modelled as data like the key routes. Every route answers 404 unless the
control plane was started with `--sso-issuer` and the related `--sso-*`
options ([SSO.md](SSO.md)). Handlers: `crates/hv2-cluster/src/sso_login.rs`
and `sso_guest.rs`.

| Operation (`id`)    | Method and path       | Notes |
| ------------------- | --------------------- | ----- |
| `sso_login`         | `GET /auth/login`     | 303 to the provider; `returnTo` (own paths only, else `/ui`); CLI: `cliPort` (from 1024), `cliChallenge`, `cliState`; 400 on malformed CLI parameters |
| `sso_callback`      | `GET /auth/callback`  | Browser-only. 303 with `__Host-hm_session` (or, for the CLI, to `127.0.0.1:{cliPort}/callback?code=…`); 400 no sign-in or bad state, 401 provider/ID token refused, 403 not in the members file |
| `sso_session`       | `GET /auth/session`   | `Authorization: Bearer hms1.…` or the cookie; 200 `{email, principalID, teamID, expiresAt}`, 401 not signed in |
| `sso_guest_handoff` | `GET /auth/guest?url=` | Cookie session only. 303 to the guest host's `/__hm/auth` with a one-minute handoff; 400 if not under a `--guest-domain`, 403 if the member may not view the sandbox |
| `sso_logout`        | `POST /auth/logout`   | 204 and the cookie cleared; 403 without a matching `Origin`. Revokes nothing server-side |
| `sso_cli_token`     | `POST /auth/cli-token` | `{code, verifier}` for `{token, email, expiresAt}`; 401 bad or expired code or verifier, 403 member removed |

**Permissions Required:** a member listed in the members file.
**Prerequisite:** control plane started with `--sso-issuer`,
`--sso-client-id`, `--sso-redirect-url`, `--sso-members-file` and
`--sso-session-key-file`; guest URLs also need `--sso-guest-urls`, proxy TLS
and a `--guest-domain`.

### Snapshot Sharing Between Teams (`team_template_sharing`)

Let another team start sandboxes from a team's snapshot. Served by the
**control plane**; authenticated with `X-API-Key` or a session with the
`templates` scope. Only the snapshot's own team (or an administrator) may use
these; every other team gets 404 ([TEAMS.md](TEAMS.md#sharing-a-snapshot-with-another-team)).
Handlers: `template_shares` and `share_template` in
`crates/hv2-cluster/src/control.rs`, and `snapshots::share` in
`crates/hv2-sandboxd/src/snapshots.rs`.

| Operation (`id`)            | Method and path                       | Notes |
| --------------------------- | ------------------------------------- | ----- |
| `list_template_shares`      | `GET /templates/{id}/shares`          | 200 `{snapshotID, teamID, sharedWith}` |
| `share_template_snapshot`   | `PUT /templates/{id}/shares/{team}`   | Body `{"acknowledgeContents": true}` required (400 without); 409 for an operator's template or the snapshot's own team |
| `unshare_template_snapshot` | `DELETE /templates/{id}/shares/{team}` | 204; sandboxes already started run on |

**Permissions Required:** `templates` scope, as the snapshot's team or an administrator.

### Sandboxes (`sandbox_lifecycle`, `sandbox_commands`, `node_sandbox_lifecycle`, `node_sandbox_commands`)

The microVM sandbox API, E2B-shaped, described once for each component that
serves it. The **control plane** (`hv2-control-plane`) takes `X-API-Key` (or a
signed-in session) with the `sandboxes` scope, picks a node, records the
owner and team from the credential and forwards to the sandbox's node; a team
key sees its own team's sandboxes only and an observer or viewer may read the
two listings alone ([TEAMS.md](TEAMS.md)). The **node** (`hv2-sandboxd`, port
3980) serves the same paths itself; its ids start `node_`. It has no users or
teams: it checks the cluster token (`x-hv2-cluster-token`) when it has one
and otherwise answers whoever can reach it ([SANDBOXES.md](SANDBOXES.md#where-a-node-listens)).
Handlers: `crates/hv2-cluster/src/control.rs` (`create`, `list_v1`, `list_v2`,
`detail`, `forward`, `adopt_owner`), `crates/hv2-sandboxd/src/main.rs`,
`reboot.rs`, `history.rs`, `processes.rs`. Most control plane routes forward
verbatim to the node, so their success bodies are the node's; the control plane
adds 401/403 (key, scope, ownership), 400 for a bad ID, 404 for an unknown
sandbox, 502 when no node answered and 503 when the store is down, and
rewrites `proxyPort` and adds `nodeID` in the descriptor `connect`, `resume`
and `create` answer with.

| Operation (`id`, control plane / node) | Method and path | Notes |
| -------------------------------------- | --------------- | ----- |
| `create_sandbox`, `create_sandbox_v2` | `POST /sandboxes`, `/v2/sandboxes` | 201 descriptor; default lifetime 15 s (v1) or 300 s (v2), at most 24 h. Control plane: 404 no node offers the template, 409 name taken, 503 no node has room. Node: 503 when full |
| `list_sandboxes`, `list_sandboxes_v2` | `GET /sandboxes`, `/v2/sandboxes` | Control plane: from the store, filtered by caller, `state`, `metadata`, `order`; v2 pages with `limit` and `nextToken` (`x-next-token`). Node: its own sandboxes |
| `get_sandbox` | `GET /sandboxes/{id}` | Includes `envdAccessToken`; observers are refused |
| `delete_sandbox` | `DELETE /sandboxes/{id}` | 204; 404 the second time |
| `connect_sandbox`, `connect_sandbox_v2` | `POST /sandboxes/{id}/connect`, `/v2/...` | 200 running, 201 if it was paused and was resumed; `timeout` only extends |
| `set_sandbox_timeout` | `POST /sandboxes/{id}/timeout` | `{timeout}` seconds from now; 204; running sandboxes only |
| `pause_sandbox`, `resume_sandbox` | `POST /sandboxes/{id}/pause`, `/resume` | `memory: false` is 400; 409 already paused / already running |
| `standby_sandbox`, `get_sandbox_standby` | `POST`, `GET /sandboxes/{id}/standby` | Stops vCPUs, keeps memory; the next request wakes it |
| `resize_sandbox` | `POST /sandboxes/{id}/resize` | `{cpuCount, memoryMB}`; a reboot at another size, memory not kept |
| `adopt_sandbox_owner` | `POST /sandboxes/{id}/owner` | `{principalId}`; administrator or team-admin of the sandbox's team; never transfers (409) |
| `node_reboot_sandbox` | `POST /sandboxes/{sandboxID}/reboot` | Node only: same ID and token, fresh guest |
| `exec_in_sandbox` | `POST /sandboxes/{id}/exec` | `cmd` (shell line) or `argv` (no shell), `timeout_secs`, `user`; 200 `{exit_code, stdout, stderr, timed_out}`; runs as root unless `user` is given |
| `list_sandbox_commands`, `get_sandbox_command` | `GET /sandboxes/{id}/commands[/{n}]` | The newest 64 commands; 409 when history is off |
| `list_sandbox_processes` | `GET /sandboxes/{id}/processes` | `?all=true` adds kernel threads; 409 if paused |

**Permissions Required:** `sandboxes` scope (control plane); the cluster token
(node).
**Not yet described:** fork, checkpoints, snapshots, networking, ports,
domains, web sharing, logs and metrics, `/sandboxes/{id}/names`, and all of
`/machines`; these stay in the coverage allowlist.

### Cluster Store Configuration (`cluster_store_configuration`)

Where control planes, nodes and the native gateway keep shared state,
including managed key records and their revocation records. An operator
deployment setting, not an HTTP operation; it has no operations and exists so
guardrails can name it.

---

## Guardrails

A guardrail is a warning attached to interfaces: a condition under which they
do not give the guarantee a reader would assume, and what to do about it. They
are served as the top-level `guardrails` list of `/agentic/ontology`.

| Field         | Meaning |
| ------------- | ------- |
| `id`          | Stable identifier |
| `severity`    | `info`, `warning` or `critical` |
| `title`, `description` | What the exposure is |
| `applies_to`  | Ids of operations, capabilities or MCP tool names it covers |
| `condition`   | When it applies |
| `mitigation`  | What to do |
| `reference`   | Repository document that explains it |

Guardrails are advisory data: nothing blocks a call because of one. For every
MCP tool named in `applies_to`, the OpenAI, Anthropic, Gemini and
`/agentic/mcp` exports append a `WARNING - ...` line to the tool description,
so a model sees the caveat when it picks the tool. (Today every guardrail
below names control-plane operations, which are not MCP tools, so no tool
description changes yet.) Older documents without the field still deserialize
(`guardrails` defaults to empty).

Current guardrails. Each is a documented residual or dangerous mode and cites
its source; the first two are finding O12 in
[the 2026-11-16 security audit](security/SECURITY_AUDIT_2026-11-16.md):

- **`redis-shared-user-revocation`** (warning; applies to `revoke_managed_key`,
  `cluster_store_configuration`). Where nodes and control planes share one
  Redis user, a compromised node can delete a revoked key's revocation record
  and write the old key record back, reviving the key until it expires.
  Mitigation: one Redis ACL user per role with `tools/redis-acl.py`
  (the shipped `deploy/compose` and `deploy/helm` already do); see
  [API_KEY_ROTATION.md](API_KEY_ROTATION.md#redis-roles-keep-nodes-away-from-the-key-records).
- **`pre-tombstone-revocations`** (warning; applies to `revoke_managed_key`).
  A key revoked before revocation records existed has none, so the protection
  above never covered it. Revoking again does not help (404, nothing
  written); only the key's expiry bounds it. Changing the managed-key secret
  is a blunt revoke-all.
- **`control-plane-unauthenticated-mode`** (critical; applies to
  `team_template_sharing`, `sandbox_lifecycle`, `sandbox_commands` and their
  operations, except owner adoption). A control plane started
  with `--allow-unauthenticated` has no key and no sign-on, so every caller who
  can reach it is an administrator with no team: any caller can read and change
  any team's snapshot shares, which hands that team the sandbox's memory, files
  and environment variables, and can create sandboxes and run commands as root
  in any of them. (The key routes and owner adoption still want an
  administrator or team-admin credential.) Mitigation: do not use the flag on a reachable
  control plane. Finding O4 in
  [the audit](security/SECURITY_AUDIT_2026-11-16.md).
- **`sso-session-not-revocable`** (warning; applies to `sso_logout`,
  `sso_callback`, `sso_cli_token`). A session is a stateless signed token;
  signing out only clears the cookie, and a copied token works until it
  expires (`--sso-session-hours`, 12 by default). Mitigation: remove the member
  from the members file, or replace the session key to end every session
  ([SSO.md](SSO.md)).
- **`sso-guest-urls-need-guest-domain`** (info; applies to
  `sso_guest_handoff`). `/auth/guest` signs a member in only for
  `{port}-{sandbox}.{domain}` under a configured `--guest-domain` (or a bound
  custom domain); any other URL is 400, so a link to an attacker's domain gets
  no credential ([SSO.md](SSO.md#private-guest-urls)).
- **`node-unauthenticated-mode`** (critical; applies to the node capabilities
  and every `node_*` operation). A node started with `--bind ADDR` beyond
  loopback and `--allow-unauthenticated`, with no cluster token and no mTLS,
  lets every host that can reach ports 3980 and 3981 run commands in its
  sandboxes and machines, read and write their files and reach what they
  publish. Mitigation: give it a token or mTLS, or keep it on loopback
  ([SANDBOXES.md](SANDBOXES.md#where-a-node-listens)).
- **`node-proxy-open`** (warning; applies to `node_sandbox_lifecycle` and the
  node's `connect` operations). `--proxy-open` turns off the check of the
  cluster token on the node's proxy port, so any host that reaches it reaches
  every sandbox's envd and every port a machine publishes; even with the check
  a captured line can be replayed for 60 s. Finding O3 in
  [the audit](security/SECURITY_AUDIT_2026-11-16.md).
- **`command-history-keeps-output`** (warning; applies to the control
  plane's and the node's command list and read operations). A sandbox keeps
  its newest 64 commands, their command lines (common secret shapes masked,
  not all) and the newest 64 KiB of each output stream, unscanned. Mitigation:
  `commandHistory: false`, `--no-command-history`, and `envVars` for secrets
  (`crates/hv2-sandboxd/src/history.rs`).
- **`shared-snapshot-contains-memory-and-secrets`** (warning; applies to
  `share_template_snapshot`, `team_template_sharing`). A shared snapshot is
  the source sandbox as it was, memory included, and revoking cannot recall
  sandboxes already started. Take the snapshot from a sandbox that never held
  what the other team must not have
  ([TEAMS.md](TEAMS.md#sharing-a-snapshot-with-another-team)).

---

## Resource Types

### Virtual Machine (`vm`)

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string", "format": "uuid" },
    "name": { "type": "string" },
    "state": { "enum": ["created", "starting", "running", "paused", "stopping", "stopped", "error"] },
    "vcpu_count": { "type": "integer", "minimum": 1, "maximum": 256 },
    "memory_gb": { "type": "integer", "minimum": 1, "maximum": 4096 },
    "enable_gpu": { "type": "boolean" },
    "enable_networking": { "type": "boolean" }
  }
}
```

**State Machine:**

```
created → starting → running ⇄ paused
                  ↓
              stopping
                  ↓
               stopped
```

### AI Agent (`agent`)

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string", "format": "uuid" },
    "vm_id": { "type": "string", "format": "uuid" },
    "script_type": { "enum": ["rhai", "wasm"] },
    "state": { "enum": ["pending", "running", "completed", "failed"] },
    "output": { "type": "string" },
    "error": { "type": "string" }
  }
}
```

---

## AI Agent Tool Formats

### OpenAI Function Calling

**Endpoint:** `GET /agentic/tools/openai`

```json
{
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "create_vm",
        "description": "Create a new virtual machine with specified configuration.",
        "parameters": {
          "type": "object",
          "properties": {
            "name": { "type": "string", "description": "VM name" },
            "vcpu_count": { "type": "integer", "default": 2 },
            "memory_gb": { "type": "integer", "default": 4 },
            "enable_gpu": { "type": "boolean", "default": false }
          },
          "required": ["name"]
        }
      }
    }
  ]
}
```

### Anthropic MCP (Model Context Protocol)

**Endpoint:** `GET /agentic/tools/anthropic`

```json
{
  "name": "hypermachine",
  "version": "0.1.0",
  "tools": [
    {
      "name": "create_vm",
      "description": "Create a new virtual machine with specified configuration.",
      "input_schema": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "vcpu_count": { "type": "integer" },
          "memory_gb": { "type": "integer" }
        },
        "required": ["name"]
      }
    }
  ]
}
```

### Google Gemini

**Endpoint:** `GET /agentic/tools/gemini`

```json
{
  "functionDeclarations": [
    {
      "name": "create_vm",
      "description": "Create a new virtual machine.",
      "parameters": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "vcpu_count": { "type": "integer" }
        },
        "required": ["name"]
      }
    }
  ]
}
```

### ChatGPT Plugin Manifest

**Endpoint:** `GET /.well-known/ai-plugin.json`

Compatible with OpenAI ChatGPT Plugin specification for marketplace integration.

---

## Event Subscriptions

AI agents can subscribe to real-time events via WebSocket:

```javascript
const ws = new WebSocket('wss://hypermachine.local/api/v1/events');
ws.send(JSON.stringify({
  action: 'subscribe',
  events: ['vm.state_changed', 'agent.completed']
}));

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log(`Event: ${data.type}`, data.payload);
};
```

### Event Types

| Event              | Description                     |
| ------------------ | ------------------------------- |
| `vm.state_changed` | VM transitioned between states  |
| `vm.metrics`       | Periodic metrics update         |
| `agent.completed`  | Agent script finished execution |

---

## Agent Scripting

### Rhai Scripts

Rhai is a safe, embeddable scripting language:

```rhai
// Get VM status
let status = vm_status();
print(`VM is ${status}`);

// Conditional logic
if status == "running" {
    let metrics = get_metrics();
    if metrics.cpu_usage > 80.0 {
        // Scale up
        scale_vcpus(4);
    }
}
```

**Available Functions:**
- `vm_status()` - Get current VM state
- `get_metrics()` - Get performance metrics
- `scale_vcpus(count)` - Adjust CPU count
- `scale_memory(gb)` - Adjust memory
- `attach_gpu(id)` - Attach a GPU
- `http_get(url)` - Make HTTP request (if network enabled)

### WASM Modules

For compiled, high-performance agents:

```rust
// agent.rs - Compile to WASM
#[no_mangle]
pub fn run() -> i32 {
    let status = hm::vm_status();
    hm::log(&format!("VM status: {}", status));
    
    if status == "running" {
        let metrics = hm::get_metrics();
        if metrics.cpu_usage > 80.0 {
            hm::scale_vcpus(4);
        }
    }
    0
}
```

Compile and upload:
```bash
cargo build --target wasm32-wasi --release
base64 target/wasm32-wasi/release/agent.wasm > agent.b64
```

---

## Security Considerations

### Capability-Based Security

Agents run with minimal privileges:

```json
{
  "script": "...",
  "capabilities": ["vm:read", "metrics:read"]
}
```

### Sandboxing

- **WASM:** Hardware memory isolation, no direct syscalls
- **Rhai:** No FFI, no file system access, CPU time limits
- **Both:** Rate limiting, timeout enforcement

### Audit Logging

All agent actions are logged:

```json
{
  "timestamp": "2026-02-02T12:34:56Z",
  "agent_id": "agent-123",
  "vm_id": "vm-456",
  "action": "scale_vcpus",
  "parameters": {"count": 4},
  "result": "success"
}
```

---

## Best Practices for AI Agents

### 1. Check State Before Operations

```python
# Good: Check state first
vm = client.get_vm(vm_id)
if vm.state == "running":
    client.stop_vm(vm_id)
```

### 2. Handle Errors Gracefully

```python
try:
    result = client.execute_script(vm_id, script)
except RateLimitError:
    time.sleep(60)
    retry()
```

### 3. Use Idempotent Operations

Most operations are idempotent - calling `start_vm` on a running VM is a no-op.

### 4. Subscribe to Events

Instead of polling, subscribe to events for real-time updates.

### 5. Respect Rate Limits

| Operation        | Rate Limit |
| ---------------- | ---------- |
| `create_vm`      | 10/min     |
| `execute_script` | 60/min     |
| `get_metrics`    | 300/min    |

---

## API Discovery Endpoints

| Endpoint                             | Format  | Description              |
| ------------------------------------ | ------- | ------------------------ |
| `/agentic/ontology`                  | JSON-LD | Full semantic ontology   |
| `/agentic/ontology?format=openai`    | JSON    | OpenAI tools format      |
| `/agentic/ontology?format=anthropic` | JSON    | Anthropic MCP format     |
| `/agentic/ontology?format=gemini`    | JSON    | Gemini functions format  |
| `/agentic/tools/openai`              | JSON    | OpenAI tools (direct)    |
| `/agentic/tools/anthropic`           | JSON    | Anthropic tools (direct) |
| `/agentic/tools/gemini`              | JSON    | Gemini tools (direct)    |
| `/.well-known/ai-plugin.json`        | JSON    | ChatGPT plugin manifest  |

The `guardrails` list is part of `/agentic/ontology`; tool exports carry guardrail warnings in their descriptions.

---

## Integration Examples

### OpenAI GPT-4

```python
from openai import OpenAI
import requests

# Fetch HyperMachine tools
tools = requests.get("https://hypermachine.local/agentic/tools/openai").json()

client = OpenAI()
response = client.chat.completions.create(
    model="gpt-4-turbo",
    messages=[{"role": "user", "content": "Create a VM with 4 CPUs and 8GB RAM"}],
    tools=tools["tools"],
    tool_choice="auto"
)
```

### Anthropic Claude

```python
import anthropic
import requests

# Fetch HyperMachine tools
tools_data = requests.get("https://hypermachine.local/agentic/tools/anthropic").json()

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Create a VM for ML training"}],
    tools=tools_data["tools"]
)
```

### Google Gemini

```python
import google.generativeai as genai
import requests

# Fetch HyperMachine tools
tools = requests.get("https://hypermachine.local/agentic/tools/gemini").json()

model = genai.GenerativeModel('gemini-pro', tools=[tools])
response = model.generate_content("Create a VM with GPU support")
```

---

**Document Control:**
- Author: HyperMachine API Team
- Reviewers: AI Integration Lead, Security Team
- Next Review: 2026-05-02

---

## Coverage gate

The ontology is meant to describe every HTTP interface. Until it does,
`tools/check-ontology-coverage.py` (CI job "Unread Controls") extracts every
route from the axum route tables of `hv2-api`, `hv2-cluster`, `hv2-sandboxd`,
`hm-cli` and `hv2-jobs` and compares them with the operations declared here.
A route that is neither declared nor listed in
`tools/ontology-coverage-allowlist.txt` fails the build, and so does an
allowlisted route that is now declared or no longer exists, so the list only
shrinks. The declared operations are read from
`docs/ontology-operations.json`, which the `ontology_operations_manifest_is_current`
test keeps identical to the built ontology; after adding an operation run
`UPDATE_ONTOLOGY_OPERATIONS=1 cargo test -p hv2-api --test ontology_tests`
and delete the route's line from the allowlist.
