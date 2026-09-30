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
| A CLI | yes | yes (over ssh) | **Partial**: `hm sandbox vm` manages sandboxd lifecycle, commands and checkpoints; protocol tests pass, real-guest client verification pending |
| Fork a running VM, memory included | ~160 ms | `cp` | **Real**: `POST /sandboxes/{id}/fork`, 1-100 copies |
| Named snapshots, and new VMs from them | yes | no | **Real**: snapshots become templates |
| Checkpoint and restore in place | yes, 10 per VM | no | **Real**: 10 per sandbox; memory and disk; same ID, token and URL; a failed restore changes nothing |
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

## Competitive verification work (2026-09-30)

The scope now includes [CubeSandbox](https://github.com/TencentCloud/CubeSandbox),
[Agent Substrate](https://github.com/agent-substrate/substrate),
[E2B](https://e2b.dev/), [Daytona](https://www.daytona.io/docs/en/sandboxes/),
[Blaxel](https://blaxel.ai/platform/sandboxes),
[Modal](https://modal.com/blog/scaling-to-1-million-concurrent-sandboxes-in-seconds),
and [Firecracker](https://github.com/firecracker-microvm/firecracker/blob/main/SPECIFICATION.md).
Firecracker is an engine baseline; the others are sandbox platforms.

**No across-the-board win is established.** Published figures use different
hardware, guest images, concurrency, and readiness criteria. In particular,
HyperMachine's 3.19 ms minimal-unikernel boot is not a Linux/Python sandbox
creation benchmark. Its measured SDK creates are 59 ms for base and 91 ms
for Python. Nested KVM measurements are not substitutes for bare-metal ones.

| Workstream | Acceptance criterion | Current gap |
|---|---|---|
| Creation and execution latency | Same guest workload and readiness command; raw samples, failure rate, P50/P95/P99 at concurrency 1, 8, 50 and 100; lower latency than each tested competitor with repeatable results | Comparable competitor runs and bare-metal HyperMachine runs are missing |
| Stateful resume, pause and fork | Verify live process memory and filesystem state, then measure API-to-first-successful-command latency under the same concurrency | Existing measurements need a common protocol and competitor adapters |
| Memory and density | Incremental PSS after the same command and idle period; same guest resources; document shared-template memory; preserve state through oversubscription | Vendor VMM overhead and our PSS are different quantities |
| Throughput and tails | Sustained arrivals on identical host resources; include failures, queueing, and recovery rather than counting accepted requests | One-node rates cannot establish a win against a million-sandbox managed fleet |
| CLI and SDK usability | Shipped client for lifecycle, execution, files and checkpoints, tested against a real node and control plane | VM CLI implemented; file transfers and real-guest CLI verification remain |
| Isolation and governance | Enforced tenant boundaries, scoped expiring keys, roles, auditable access, and escape tests | The sandbox platform remains single-team |
| Networking and access | Custom domains, certificate automation, authenticated private URLs, SSH, raw TCP/UDP and isolated VM groups | HTTP proxy and egress are implemented; the listed access features remain absent |
| Platforms and workloads | Verified ARM64 execution, GPU sandboxes, browser/desktop workloads, and persistent storage limits | ARM64 execution and GPU sandbox wiring remain unverified or absent |
| Operations | Object-storage backups and recovery, quota enforcement, scheduling/event triggers, load-tested multi-node failover | Shared-directory snapshots and host job queues do not cover all these capabilities |

Mark a capability complete only after checking its effect through a shipped
interface. Mark a performance win only after equivalent runs establish it;
do not infer it from a vendor headline or a component microbenchmark.

### VM client and readiness benchmark

Build the client with `cargo build -p hm-cli --bin hm`. It talks to sandboxd
or the control plane, selected by `--endpoint` or `HV2_SANDBOX_URL`.
`HV2_API_KEY` supplies authentication without putting the key in shell arguments.
Use TLS when connecting to a remote endpoint.

```sh
hm sandbox vm --endpoint http://127.0.0.1:8080 create --template base
hm sandbox vm list
hm sandbox vm exec SANDBOX_ID -- /bin/sh -c 'printf hello'
hm sandbox vm checkpoint save SANDBOX_ID before-change
hm sandbox vm checkpoint restore SANDBOX_ID before-change
hm sandbox vm pause SANDBOX_ID
hm sandbox vm resume SANDBOX_ID --lifetime 300
hm sandbox vm fork SANDBOX_ID --count 2
hm sandbox vm delete SANDBOX_ID

hm sandbox vm benchmark --template base --samples 100 --concurrency 8 \
  --environment 'CPU model; RAM; OS; nested/bare-metal; image hash; daemon commit' \
  --max-p99-ready-ms 150 > readiness.json
```

The benchmark includes CLI HTTP calls, guest execution, and client-side
queueing within each sample's creation-to-ready interval. A sample is ready
only when `/bin/sh` produces its expected unique marker with exit 0 and no
timeout. Creation and first-execution times are reported separately.
Every known sandbox is deleted, including after a failed readiness command;
a cleanup error fails the sample. Raw records and nearest-rank percentiles
are JSON. An empty successful sample set has null latency statistics, not
zero. Any failed sample or exceeded threshold makes the CLI exit nonzero.
The throughput field measures complete create/execute/delete lifecycles,
including cleanup; it is not a create-only throughput figure.

The 150 ms command above demonstrates setting a gate; it is not evidence of
a performance win or a universal target. No warmups are discarded. Template
preparation happens outside the run and must be reported separately. This
benchmark uses HyperMachine's `/exec` extension, so other providers require
equivalent execution adapters before their results can be compared. It does
not measure memory, stateful resume, or general application initialization.

## Changelog of this page

- **2026-09-29, checkpoints.** `POST /sandboxes/{id}/checkpoints` saves a running sandbox, and
  `POST .../checkpoints/{name}/restore` rolls it back in place. The sandbox keeps its ID, access
  token and URL, and its memory and filesystem go back to the saved state. List and delete are
  also available, with at most 10 per sandbox; checkpoints end with the sandbox.
  - The replacement guest boots before the old one stops, so a failed restore changes nothing.
  - Checked on KVM guests (`tools/e2e-checkpoints.sh`: 26 checks). The key check: a process killed
    after the checkpoint runs again after the restore.
  - Limitation: checkpoints stay on the node that took them.
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
