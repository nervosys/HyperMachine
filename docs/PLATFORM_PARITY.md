# HyperMachine against boxd and exe.dev

The goal: every capability [boxd](https://boxd.sh/) and [exe.dev](https://exe.dev/) offer, and more.
This page tracks it. Their feature lists were read from their public docs on 2026-09-29
(`docs.boxd.sh/llms-full.txt`, `exe.dev/docs/all`). HyperMachine's statuses come from its code,
not its docs. **Real** means wired to a shipped binary and checked. **Partial** says what is missing.

All three products give each user a real Linux VM behind a hardware boundary, rather than a
container. The comparison is about what surrounds the VM.

## The matrix

| Capability | boxd | exe.dev | HyperMachine |
|---|---|---|---|
| Create, list, delete over an API | yes | yes | **Real**: E2B's API (`hv2-sandboxd`, `hv2-control-plane`), so the unmodified E2B SDKs work |
| A CLI | yes | yes (over ssh) | **Partial**: `hv2` has demo handlers; no client for sandboxd yet |
| Fork a running VM, memory included | ~160 ms | `cp` | **Real**: `POST /sandboxes/{id}/fork`, 1-100 copies |
| Named snapshots, and new VMs from them | yes | no | **Real**: snapshots become templates |
| Checkpoint and restore in place | yes, 10 per VM | no | **Absent** |
| Pause and resume | yes | no | **Real**: to disk; any node resumes |
| Suspend when idle, wake on traffic | yes | no | **Real**: `idleTimeout` or `--idle-pause-after`, plus `autoResume`. Idle means no traffic *and* a quiet guest CPU, so unwatched work is never frozen |
| HTTPS URL per VM | yes | yes | **Partial**: `{port}-{id}.{domain}` over TLS; you bring the wildcard certificate (no ACME) |
| Per-port URLs, raw TCP/UDP | yes | ports 3000-9999 | **Partial**: every port over HTTP(S); no raw TCP or UDP |
| Custom domains | yes | yes | **Absent** |
| Private URLs with login, identity headers | team-shared | yes (`X-ExeDev-Email`) | **Absent** |
| SSH to a VM by name | yes | yes | **Absent** |
| exec, and file copy in and out | yes | ssh/scp | **Real**: `/exec`, envd processes with PTY and stdin, files |
| **Env vars for every command in a VM** | org-wide | no | **Real**: E2B's `envVars`, kept in the guest so pause, fork and snapshots carry them |
| Secrets held off the VM, injected at the edge | no | yes | **Real**: header injection at the egress gateway, which the guest never sees |
| Workload identity (AWS/GCP federation) | no | yes | **Real**: JWT-SVIDs minted at the gateway, JWKS and OIDC discovery |
| Egress policy per VM | isolated or not | no | **Real**: allow/deny lists, live updates, decision log, reserved ranges refused |
| VM-to-VM networks by tag | yes | via proxy | **Absent** |
| Teams, roles, sharing | yes | yes, with SSO | **Absent**: one team |
| Scoped, expiring API keys | yes | yes | **Absent** |
| Persistent volumes shared between VMs | no | no | **Real**: E2B volumes over 9P, live and shared (Linux hosts) |
| Build images from Dockerfiles or OCI | compose | Dockerfile | **Real**: E2B template builds, no Docker |
| Backups to object storage | yes | no | **Absent** |
| Scheduled jobs and event triggers | `*.run.ts` | no | **Partial**: lifecycle webhooks only |
| Desktop in a browser, browser for agents | yes | web terminal | **Absent** |
| MCP for agents | skill + MCP | Shelley agent | **Partial**: 48 tools in `hv2-agent`, not served by any binary |
| Email in and out | no | yes | **Absent** |
| Multi-node, self-hosted | contact sales | enterprise | **Real**: control plane, Redis store, cross-node resume, mTLS, Helm chart |
| GPU | no | no | **Partial**: VFIO code, not wired to sandboxes |
| Hypervisor you can read | no | no | **Real**: our own VMM; KVM, WHPX, HVF; a Type-1 path |

## What is beyond both today

- Open source and self-hosted, down to the VMM.
- Drop-in for the E2B SDKs, so existing agent code needs no changes.
- Per-sandbox egress policy with a decision log.
- Credential injection and workload identity at the gateway, so tokens never enter the VM.
- Live shared volumes.
- Paused sandboxes resume on any node.
- About 30x oversubscription, with paused sandboxes at about 2.2 MiB each.

## Changelog of this page

- **2026-09-29, idle pause.** A sandbox pauses to disk after `idleTimeout` seconds unused, set per
  sandbox or node-wide with `--idle-pause-after`. With `autoResume`, the next request through the
  proxy wakes it and is answered.
  - "Unused" means three things: no proxy request in flight or begun, no `/exec` running, and every
    CPU sample the node took in the window under 5%. boxd suspends on idleness alone. A long
    build, a training run or a crawler nobody is watching keeps running here.
  - Checked on KVM guests (`tools/e2e-idle-pause.sh`: 11 checks).
  - Two mutations were run: removing the CPU gate pauses the busy guest, and removing the exec
    guard freezes a quiet 50-second command halfway. The checks catch both.
- **2026-09-29, sandbox env vars.** `envVars` on create, as E2B's API has it.
  - They are written into the guest's template defaults, which the guest agent applies to every
    command. That puts them in guest memory, so pause, resume on another node, fork and
    snapshot-templates all carry them.
  - They are write-only: no response returns them.
  - Checked end to end on KVM guests (`tools/e2e-sandbox-env.sh`: 13 checks). The checks cover a
    command seeing the variables, pause and resume, a fork, a sandbox created from its snapshot,
    responses not echoing values, and invalid names being refused.
