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

### Machines (`machine_lifecycle`, `machine_commands`, `machine_agents`, `machine_ports`, `machine_snapshots`, their `node_` twins, and `node_machine_desktop`)

Long-lived VMs with their own ext4 disk ([MACHINES.md](MACHINES.md)). The
**control plane** needs the `machines` scope: an `inventory` key and an
observer or viewer are refused (403). It asks every live node which holds the
machine and relays; a machine of another team is a 404, and while a node is not
answering the answer is 503 rather than 404. It lists by asking the nodes and
leaves out one that does not answer. It routes 35 of the node's 45 routes:
**conversations and the desktop are the node's only.** The **node** answers 501
off Linux/KVM, takes the team from `x-hv2-sandbox-team` and checks the cluster
token. Handlers: `crates/hv2-cluster/src/control.rs` (`list_machines`,
`create_machine`, `to_machine_node`, `machine_reverse_tunnel`) and
`crates/hv2-sandboxd/src/machines.rs` and `machines/`.

| Operation (`id`, control plane / node) | Method and path | Notes |
| -------------------------------------- | --------------- | ----- |
| `list_machines`, `create_machine` | `GET`, `POST /machines` | Create: 201; 409 name exists; 503 when it was made and did not start (it stays, stopped). Control plane: the body is read to 64 KiB, `x-hv2-node` names the node, and the node is chosen by template even for an `image` machine |
| `get_machine`, `resize_machine`, `delete_machine` | `GET`, `PATCH`, `DELETE /machines/{name}` | Resize and delete need it stopped (409); a disk only grows |
| `control_machine` | `POST /machines/{name}/{action}` | `start`, `stop`, `restart`; any other action is 404 |
| `get_machine_console`, `send_machine_console_input` | `GET`, `POST /machines/{name}/console` | `tail=N` bytes; input 1 to 4096 bytes |
| `get_machine_network_decisions` | `GET /machines/{name}/network/decisions` | 400 if it has no network |
| `exec_in_machine`, `list_machine_processes` | `POST .../exec`, `GET .../processes` | Needs the guest agent; the guest gets 80% of `timeout_secs`; the answer has no `timed_out` |
| `read_machine_file`, `write_machine_file` | `GET`, `PUT .../files?path=` | Up to 64 MiB; 403, 404, 413 as the guest says |
| `list_machine_runs`, `start_machine_run`, `get_machine_run`, `write_machine_run_stdin`, `signal_machine_run` | `.../runs[/{runID}[/stdin or /signal]]` | Newest 4 MiB per stream, kept on the node's disk; `cmd` is not masked, `env` is not kept |
| `list_machine_prompts`, `start_machine_prompt`, `get_machine_prompt`, `interrupt_machine_prompt` | `.../prompts[/{promptID}[/interrupt]]` | `claude`, `codex` or `custom`; `unattended` adds the agent's dangerous flag |
| `list_machine_ports`, `publish_machine_port`, `unpublish_machine_port`, `open_machine_reverse_port` | `.../ports[/{hostPort}[/reverse]]` | 16 a machine; `reverse` upgrades to `hv2-reverse/1` (101) |
| `list_machine_snapshots` ... `archive_machine_snapshot_directory` | `.../snapshots[/{snapshotID}[/restore, /disk, /files, /file or /archive]]` | Disk only, taken stopped (409 running); the disk, files, file and archive routes read the snapshot without booting it; 32 a machine |
| `node_*_machine_conversation*` | `.../conversations[/{conversation}[/messages, /transcript or /interrupt]]` | Node only; in memory; a message is 201 (new turn) or 202 (steered, Claude Code only) |
| `node_get_machine_desktop`, `node_get_machine_desktop_screenshot`, `node_act_on_machine_desktop` | `.../desktop[/screenshot or /actions]` | Node only; 404 without a `desktop` |

**Permissions Required:** `machines` scope (control plane); the cluster token
(node).

### Fork, Snapshots And Checkpoints (`sandbox_fork_and_snapshots`, `sandbox_checkpoints`, and their `node_` twins)

Served by both components: the control plane forwards to the sandbox's node.
Forwarding buffers the node's answer and keeps its status, `content-type`,
`content-disposition` and `x-hv2-archive-skipped` (the list of what an
archive skipped), so a checkpoint archive reaches a control plane caller
complete with both. The `sandboxes` scope; `list_snapshots` needs `templates` and
refuses an observer or viewer.

| Operation | Method and path | Notes |
| --------- | --------------- | ----- |
| `fork_sandbox` | `POST /sandboxes/{id}/fork` | `count` 1 to 100 (8 with a disk); always 201 with an array of `{sandbox}` or `{error}`; carries memory, so env and secrets |
| `create_sandbox_checkpoint`, `list_sandbox_checkpoints`, `delete_sandbox_checkpoint`, `restore_sandbox_checkpoint` | `.../checkpoints[/{name}[/restore]]` | 10 held (`auto` not counted); a disk is copied with it (CHECKPOINTS.md says otherwise; the code, `checkpoints.rs`, does); restore is in place |
| `list_sandbox_checkpoint_files`, `read_sandbox_checkpoint_file`, `archive_sandbox_checkpoint_directory` | `.../checkpoints/{name}/files, /file or /archive?path=` | Each restores into a throwaway guest (2 at a time, else 503); file 64 MiB; archive 64 MiB, 180 s (504) |
| `snapshot_sandbox`, `list_snapshots` | `POST /sandboxes/{id}/snapshots`, `GET /snapshots` | A template that holds the sandbox's memory; 409 for a disk or a paused sandbox; the control plane merges nodes and filters by team |

### Sandbox Network, Exposure, Telemetry And Recovery (`sandbox_egress_network`, `sandbox_port_tunnels`, `sandbox_telemetry`, `sandbox_events`, `registration_recovery`, their `node_` twins; control plane only: `sandbox_public_ports`, `sandbox_web_sharing`, `sandbox_domains_and_names`, `sandbox_private_networks`, `sandbox_refreshes`; node only: `node_sandbox_private_networks`, `node_local_networks`)

Operations that the control plane **relays** to the sandbox's node (rules,
decisions, metrics, logs) are the node's handler behind an ownership check; the
tunnels it **splices** itself (ownership, node lookup, a 15 second upgrade); the
rest it answers from the cluster store. The node serves 22 of these and the
control plane 28. A team's key reaches its own team's sandboxes only. Public
ports, web sharing and private networks need a key with a configured creator
(`principal_id`) that owns the sandbox (a teammate acts as the creator); they
are refused with no credential, so an open control plane does not serve them.

| Operation | Method and path | Notes |
| --------- | --------------- | ----- |
| `update_sandbox_network` | `PUT /sandboxes/{id}/network` (both serve PUT only; the control plane answers any other method with a JSON 405 and `Allow: PUT` itself) | The whole new policy, not a patch: what is left out is cleared. `allowOut`, `denyOut` (no names), `allow_internet_access` (snake case), `rules`, `egressProxy`; `allowOut` alone restricts nothing on a node whose default is allow |
| `get_sandbox_network_decisions` | `GET .../network/decisions` | First and last 256 decisions of the gateway; 400 with no network |
| `open_sandbox_tcp_tunnel`, `open_sandbox_udp_tunnel`, `open_sandbox_udp6_tunnel`, `open_sandbox_reverse_tunnel` | `GET .../ports/{port}/tcp`, `/udp`, `/udp6`, `/reverse` | Upgrade to `hv2-tcp/1`, `hv2-udp/1`, `hv2-udp6/1`, `hv2-reverse/1`; no TLS or login added; paused is 409 on the control plane and 404 on a node; reverse holds at most 64 waiting |
| `list_sandbox_public_ports`, `reserve_sandbox_public_port`, `remove_sandbox_public_port` | `.../public-ports[/{port}]` | Control plane only, needs `--native-port-range`; 202 means stored, not bound; 16 per sandbox |
| `get_sandbox_web_sharing`, `set_sandbox_web_sharing` | `.../web-sharing` | Grants `{subject, expires_at}`, compare-and-set on a UUID v4 revision; 256 at most |
| `list_sandbox_domains`, `get_sandbox_domain_challenge`, `bind_sandbox_domain`, `unbind_sandbox_domain` | `.../domains[/{domain}[/challenge]]` | First claim wins; a DNS TXT proof only with `--domain-verification-file` (428 without, 403 wrong, 501 no policy) |
| `assign_sandbox_name`, `resolve_sandbox_name` | `PUT .../names/{name}`, `GET /sandbox-names/{name}` | One namespace for the whole cluster; another team's name is 403 on lookup, which shows it is taken |
| `get_sandbox_private_networks`, `set_sandbox_private_networks` | `.../private-networks` | At most 8 tags, an empty list removes; compare-and-set; `node_open_sandbox_private_tcp_tunnel` and `..._udp_tunnel` are the receiving end, for other nodes |
| `refresh_sandbox` | `POST .../refreshes` | E2B's keep-alive: `{duration}` seconds (default 300, 0 to 3600) from now, applied only if later than the current end, so it never shortens; 204; 400 for a bad duration; running sandboxes only. `set_sandbox_timeout` can also shorten |
| `list_sandboxes_metrics`, `get_sandbox_metrics`, `get_sandbox_logs`, `get_sandbox_logs_v2` | `/sandboxes/metrics`, `.../metrics`, `.../logs`, `/v2/sandboxes/{id}/logs` | Samples every 5 s, the last hour; the log is the node's lifecycle notes (1000), not program output; both in node memory. The latest-samples route is open to an inventory key or observer |
| `list_sandbox_events`, `get_sandbox_events` | `GET /events/sandboxes[/{id}]` | E2B's `SandboxEvent`, a JSON list (not a stream); needs the `events` scope; a node serves them unfiltered |
| `list_node_pending_registrations`, `reconcile_node_sandbox_registration` (node: `node_list_pending_registrations`, `node_reconcile_sandbox_registration`) | `/cluster/nodes/{node}/registrations/pending`, `.../sandboxes/{id}/registration/reconcile`; node `/registrations/pending`, `/sandboxes/{sandboxID}/registration/reconcile` | Administrator only on the control plane; the answer carries the sandbox's access token |
| `node_*_local_network` | `/networks[/{name}[/connect or /disconnect]]` | Node only, in memory, no owner or team: any caller can join any sandbox ID to a network |

**Permissions Required:** `sandboxes` scope, `events` for events, `admin` for
registration recovery (control plane); the cluster token (node).

### hv2-api's Own Routes (`vm_guest_access`, `image_admission`, `agent_runtime`, `fleet_runtime`, `gpu_fabric`, `vm_snapshot_records`, `event_delivery`, `health_and_metrics`, `agent_discovery_and_plans`)

71 operations, all served by `hv2-api` (the `/api/v1` REST server). Read these
first:

- **Authentication.** None by default. With API keys configured the check is a
  static `Authorization: Bearer <key>` comparison (401 missing, 403 wrong),
  with no per-route rights: the permission middleware in the crate is not
  installed. GET/HEAD/OPTIONS under `/health` and `/agentic` need no key;
  `/metrics`, `/api/v1/health/full` and `/.well-known/*` do. Every POST, PUT
  and PATCH outside `/health` and `/agentic` needs `Content-Type:
  application/json` (415), even without a body.
- **`vm_guest_access`.** `exec_in_vm` answers 503 for every VM made through
  `create_vm` (no guest-agent channel, no `GuestExec` capability), and the
  console reports `attached: false` (no serial device is registered).
- **`image_admission`.** In-memory registry; the launch check is off by
  default and hashes the kernel only. `register_image` stores signatures as
  unverified, so `approve_image` is 422 for all. `deprecate_image` passes the
  same signature gate, so it refuses too on this server. A denied or revoked
  image cannot be approved (409).
- **`agent_runtime`.** Tenant is the caller's `X-Tenant-Id`, unless its key is
  bound to a tenant; `reap_agents` is scoped to the requesting tenant; the
  baseline is a placeholder.
- **`fleet_runtime`.** Bookkeeping only: nothing starts a process;
  `pending_workloads` is always 0; destroy of an unknown session is 200;
  maintenance runs only when called.
- **`gpu_fabric`.** Caller-declared topology, no discovery; reservations
  hold no GPU and check no capacity; the fleet has no register route.
- **`vm_snapshot_records`.** Catalogue entries only; restore changes no VM.
- **`event_delivery`.** Only `publish_event` puts events on the bus. Webhook
  URLs must be public unless `allow_private_webhooks` is set; the signature is
  HMAC-SHA256 when a secret is set; delivery is not retried.
- **`health_and_metrics`.** `/health/ready` and `/api/v1/health/full` cannot
  report failure; `/metrics` renders two series on the shipped server.
- **`agent_discovery_and_plans`.** `execute_plan` runs `create_vm`,
  `start_vm`, `stop_vm`, `pause_vm`, `resume_vm`, `delete_vm`, `get_vm`,
  `list_vms` and `get_metrics` against this server's VMs; every other step
  fails. `validate_plan` and failed executions answer 200. The plugin
  manifest points at `/agentic/openapi.yaml`, which is not routed, and the
  agent card claims auth schemes the server does not check.

**Permissions Required:** an API key when the server has keys configured.

### Templates, Volumes, Platform Routes And The Local Servers

The last 103 routes, plus the 12 the node serves by mounting the control
plane's routers, so every interface the five components serve is described
(115 operations; the allowlist is empty).

- **Templates and builds** (`template_catalogue`, `template_builds`, their
  `node_` twins, and `node_team_template_sharing`): list, alias lookup, build
  from an image, delete, the v2/v3 build routes, the upload link and the
  upload. The upload `PUT` sits outside the key check and the cluster-token
  check; its `token` query parameter stands in. The token is offered for one
  scope (the caller's team, or the node's own for an administrator), template
  and hash, and expires after 30 minutes; a `PUT` to any other template or
  hash is refused, and its bytes go to the scope the link was issued to. Uploads
  and cached build steps are kept per scope, so one team's files cannot stand
  in for another's. The uploaded hash is checked only with
  `--verify-upload-hash` (O8). Pulled images are shared node-wide, and
  templates built before scoping are visible to everyone (O14 residuals). The control plane
  answers 502 to `POST /templates` unless a node answered 202.
- **Volumes and disks** (`volume_management`, `volume_content`, `node_volume_management`,
  `node_volume_content`, `node_block_disks`): `/volumecontent/*` is authenticated
  by the volume's own bearer token alone, on both components, and the control
  plane's operations are method `ANY`. Growth is bounded by a reserve guard
  (5% of the filesystem capped at 2 GiB, plus 1% of inodes) that is on by
  default; `--volume-max-bytes` and `--volume-max-inodes` are off (O7);
  refusals are 507. Disks are node-local, have no team scope, and have no
  control-plane route.
- **Platform** (`sandbox_environments`, `sandbox_event_webhooks`,
  `sandbox_usage_report`, `workload_identity_discovery`, `prometheus_exposition`,
  `team_telemetry`, `cluster_inventory`, `node_warm_pool`): the node mounts
  hv2-cluster's environments and events routers, so it serves environments,
  webhooks and usage too, behind the cluster token only and with no team
  context. On the control plane no key scope covers `/environments` or
  `/usage`, so only an administrator (or an open plane) reaches them.
- **hm-cli** (`hm_cli_local_vms`, `hm_cli_tool_calls`, `hm_cli_agent_discovery`,
  `hm_cli_mcp_http`): `hm serve` binds `127.0.0.1` by default (`--bind`) and is
  plain HTTP with no TLS. It refuses a non-loopback bind without `HM_API_KEY`
  unless `--allow-unauthenticated` is passed, requires the key on every route
  except `/health`, and keys its rate limit on the socket peer, honouring
  `X-Forwarded-For` only from `--trusted-proxy` addresses.
  `hm sandbox vm mcp-http` is loopback only and needs a token of 32 to 4096
  bytes and an allowed `Origin`.
- **hv2-jobs** (`jobs_queue`, `jobs_schedules`, `jobs_vm_receipts`): an optional
  bearer token, loopback by default and refused beyond it without a token.
  Submitting a job is remote execution; a receipt's claim token is readable by
  any holder of the server token.

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
  `team_template_sharing`, `sandbox_lifecycle`, `sandbox_commands`, the
  machine, fork, snapshot, checkpoint, egress, tunnel, domain, name, metrics,
  log and event capabilities and their operations, except owner adoption,
  public ports, web sharing, private networks and registration recovery). A control plane started
  with `--allow-unauthenticated` has no key and no sign-on, so every caller who
  can reach it is an administrator with no team: any caller can read and change
  any team's snapshot shares, which hands that team the sandbox's memory, files
  and environment variables, and can create sandboxes and run commands as root
  in any of them. (The key routes and owner adoption still want an
  administrator or team-admin credential.) Mitigation: do not use the flag on a reachable
  control plane. Finding O4 in
  [the audit](security/SECURITY_AUDIT_2026-11-16.md).
- **`egress-rules-replace-and-default-dependent`** (warning; sandbox creation, egress rules and decisions): `allowOut` alone restricts nothing where the node's default is allow, an update replaces the whole policy, and only host addresses read at node start are refused ([SANDBOXES.md](SANDBOXES.md#allowout-alone-does-not-restrict-egress-when-the-default-is-allow)).
- **`public-ports-expose-guest-services`** (warning; public ports): the native gateway relays raw TCP/UDP with no login, a 202 is not a bound listener and a 204 not a barrier ([NATIVE_PORT_GATEWAY.md](NATIVE_PORT_GATEWAY.md)).
- **`custom-domain-binding-routes-traffic`** (warning; domains): without `--domain-verification-file` any caller with the scope claims any free hostname, and the proxy has no login unless the operator turns one on ([CUSTOM_DOMAINS.md](CUSTOM_DOMAINS.md)).
- **`web-sharing-grants-and-revocation`** (info; web sharing): a grant opens every port to an identity, operator scopes are independent, and an admitted stream keeps its admission ([PRIVATE_GUEST_URLS.md](PRIVATE_GUEST_URLS.md#owner-sharing-management-api)).
- **`private-network-source-trust`** (warning; private networks): the source node label is trusted on the cluster's shared credential, and support is documented as Partial ([PRIVATE_VM_NETWORKS.md](PRIVATE_VM_NETWORKS.md#receiving-node-tcp-route)).
- **`registration-reconcile-returns-access-token`** (warning; registration recovery): the answer is credential-bearing, and a retry may duplicate an event ([NATIVE_PORT_GATEWAY.md](NATIVE_PORT_GATEWAY.md#recovering-uncertain-guest-registration)).
- **`sandbox-events-carry-metadata`** (info; events): metadata travels in every event, and a node does not filter by team (`crates/hv2-cluster/src/events.rs`).
- **`api-unauthenticated-unless-key-configured`** (critical; hv2-api's routes): with no key configured the server answers every caller; it binds loopback by default and refuses other addresses without a key unless `allow_unauthenticated` is set. Keys carry a role and an optional tenant, enforced by the key middleware (`crates/hv2-api/src/middleware.rs`).
- **`guest-exec-runs-programs-in-the-guest`** (warning; `exec_in_vm`): the guest agent authenticates nothing and runs as whoever started it ([GUEST_AGENT.md](GUEST_AGENT.md)).
- **`image-admission-off-by-default-and-unsatisfiable`** (warning; image routes): admission is off by default; over REST no image can be approved, and deprecation does not admit one either, because both pass the signature gate ([SECURITY_AUDIT.md](SECURITY_AUDIT.md)).
- **`agent-runtime-tenant-is-caller-asserted`** (warning; agent sessions): the tenant is the caller-written `X-Tenant-Id` unless the key is bound to a tenant ([SECURITY_AUDIT.md](SECURITY_AUDIT.md)).
- **`webhooks-unsigned-without-secret-and-opt-out-reaches-internal`** (warning; webhooks): non-public targets are refused unless `allow_private_webhooks` is set; a webhook without a secret is unsigned; the signed data has no delivery time, so a delivery can be replayed ([security/SECURITY_AUDIT_2026-11-16.md](security/SECURITY_AUDIT_2026-11-16.md)).
- **`event-publish-injects-events`** (warning; events): any caller who may write can publish (`crates/hv2-api/src/events.rs`).
- **`probes-exempt-from-api-key`** (info; health and `/agentic`): reads need no key (`crates/hv2-api/src/middleware.rs`).
- **`snapshot-routes-record-no-vm-state`** (warning; VM snapshot records): nothing is captured (`crates/hv2-api/src/snapshot_routes.rs`).
- **`template-upload-hash-not-verified-by-default`** (warning; uploads): the hash is checked only with `--verify-upload-hash` ([security/SECURITY_AUDIT_2026-11-16.md](security/SECURITY_AUDIT_2026-11-16.md), O8).
- **`template-upload-route-needs-no-api-key`** (info; uploads): a URL token stands in for the key.
- **`template-build-scope-residuals`** (warning; builds): pulled images are shared node-wide, uploads and build cache are per scope, and pre-scoping templates are visible to all (O14).
- **`volume-growth-limits-and-defaults`** (warning; volumes): the reserve guard is on, the byte and inode limits are off (O7).
- **`volume-content-token-is-the-only-credential`** (warning; volume content): [VOLUMES.md](VOLUMES.md).
- **`volume-writes-are-not-atomic-and-delete-is-immediate`** (info; volumes): [VOLUMES.md](VOLUMES.md).
- **`block-disks-are-node-local-and-team-scoped-by-the-cluster-token`** (warning; disks): [DISKS.md](DISKS.md).
- **`environments-keep-secrets-unencrypted`** (warning; environments): [ENVIRONMENTS.md](ENVIRONMENTS.md).
- **`administrator-environments-are-not-found-by-team-sandboxes`** (warning): [TEAMS.md](TEAMS.md).
- **`webhook-urls-are-checked-at-delivery`** (warning; the control plane's and node's webhooks): [WEBHOOKS.md](WEBHOOKS.md).
- **`node-team-scope-rests-on-the-cluster-token`** (warning; node): the node scopes its environments, webhooks and usage by the team header the control plane sends, so the cluster token is the trust boundary; a request with no header is an administrator's (`crates/hv2-cluster/src/events.rs`).
- **`team-telemetry-leaves-the-platform`** (info): [TELEMETRY.md](TELEMETRY.md).
- **`cluster-event-log-lists-every-team`** (info): [TEAMS.md](TEAMS.md).
- **`platform-discovery-and-metrics-are-public`** (info): [TELEMETRY.md](TELEMETRY.md).
- **`hm-serve-open-and-plain-by-default`** (warning; `hm serve`): plain HTTP always, and every route is open to any caller when started with `--allow-unauthenticated` ([src/ai/mcp-server.md](src/ai/mcp-server.md)).
- **`mcp-http-single-shared-credential`** (warning; MCP over HTTP): [src/ai/mcp-server.md](src/ai/mcp-server.md).
- **`jobs-serve-is-remote-execution`** (critical; hv2-jobs): [JOBS.md](JOBS.md).
- **`schedule-receipts-trust-the-operator`** (warning; hv2-jobs): [JOBS.md](JOBS.md).
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
- **`node-proxy-open`** (warning; applies to `node_sandbox_lifecycle`, the
  node's `connect` operations, `node_machine_ports` and
  `node_publish_machine_port`). `--proxy-open` turns off the check of the
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
- **`machine-runs-keep-command-and-output`** (warning; applies to the run,
  prompt and conversation operations that keep or return them). A run keeps
  its command line as given, unmasked, and the newest 4 MiB of each stream on
  the node's disk until the machine is deleted; `env` is the one thing not
  kept. Source: [MACHINES.md](MACHINES.md#runs).
- **`machine-agents-act-with-the-callers-key`** (warning; applies to
  `start_machine_prompt`, `create_machine_conversation` and
  `send_machine_conversation_message`). A prompt runs a coding agent as root
  unless a user is named, with the key it is given; `unattended` passes the
  agent's dangerous flag so nothing asks for approval. Source:
  [MACHINES.md](MACHINES.md#prompts).
- **`machine-desktop-has-no-password`** (warning; applies to
  `publish_machine_port` and the desktop operations). noVNC has no password;
  a published port gives the desktop to whoever reaches it, and through the
  node's own proxy there is no login. Source: [MACHINES.md](MACHINES.md#desktop).
- **`shared-snapshot-contains-memory-and-secrets`** (warning; applies to
  `share_template_snapshot`, `team_template_sharing`, `snapshot_sandbox`). A shared snapshot is
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
A `.merge(other_crate::module::f(..))` is followed into the other crate (the node
mounts hv2-cluster's environments and events routers), so those routes count
for the node. A route that is neither declared nor listed in
`tools/ontology-coverage-allowlist.txt` fails the build, and so does an
allowlisted route that is now declared or no longer exists, so the list only
shrinks. The declared operations are read from
`docs/ontology-operations.json`, which the `ontology_operations_manifest_is_current`
test keeps identical to the built ontology; after adding an operation run
`UPDATE_ONTOLOGY_OPERATIONS=1 cargo test -p hv2-api --test ontology_tests`
and delete the route's line from the allowlist.
