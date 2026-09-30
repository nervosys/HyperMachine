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
| A CLI | yes | yes (over ssh) | **Partial**: `hm sandbox vm` manages lifecycle, commands, binary files and checkpoints; verified on a real KVM node; control-plane verification remains |
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

| Product | Published or repository measurement | Feature comparison target | Comparable win verified? |
|---|---|---|---|
| HyperMachine | Historical nested-KVM SDK create: base 59 ms, Python 91 ms; template restore to answering agent median 12.9 ms | VM lifecycle, checkpoints, forks, egress policy, workload identity, shared volumes | Baseline only; fresh measurements below must specify hardware and workload |
| CubeSandbox | Advertises startup below 60 ms; 50 concurrent starts average 67 ms, P95 90 ms, P99 137 ms on its benchmark host | E2B compatibility, ARM64, BPF networking, secret injection, Kubernetes/Terraform, object-storage resume | No matched run |
| Agent Substrate | Advertises resume below 500 ms and more than 500 activations/second | Kubernetes, gVisor/microVM backends, stateful oversubscription | No matched run |
| Blaxel | Advertises approximately 25 ms stateful resume | Managed lifecycle and automatic suspension | No matched run |
| Daytona | Advertises sandbox startup below 90 ms; default sandbox is a container | Linux/Windows VMs and GPU workloads | No matched run; container startup differs from VM readiness |
| Modal | Reports below 500 ms median API-to-user-code latency in its million-concurrent-sandbox benchmark | Managed fleet scale and GPU workloads | No matched run or equivalent fleet |
| E2B | No precise current latency verified in this review | SDK compatibility and managed execution | No matched run |
| Firecracker | Specification targets at most 125 ms from InstanceStart to init and at most 5 MiB VMM overhead for its specified minimal guest | Engine isolation and efficiency | No matched run; init time and VMM overhead differ from application readiness and incremental PSS |

These figures describe the linked providers' own claims or repository runs,
not an independently reproduced ranking. A lower headline number does not
pass the acceptance criteria below.

| Workstream | Acceptance criterion | Current gap |
|---|---|---|
| Creation and execution latency | Same guest workload and readiness command; raw samples, failure rate, P50/P95/P99 at concurrency 1, 8, 50 and 100; lower latency than each tested competitor with repeatable results | Comparable competitor runs and bare-metal HyperMachine runs are missing |
| Stateful resume, pause and fork | Verify live process memory and filesystem state, then measure API-to-first-successful-command latency under the same concurrency | Existing measurements need a common protocol and competitor adapters |
| Memory and density | Incremental PSS after the same command and idle period; same guest resources; document shared-template memory; preserve state through oversubscription | Vendor VMM overhead and our PSS are different quantities |
| Throughput and tails | Sustained arrivals on identical host resources; include failures, queueing, and recovery rather than counting accepted requests | One-node rates cannot establish a win against a million-sandbox managed fleet |
| CLI and SDK usability | Shipped client for lifecycle, execution, files and checkpoints, tested against a real node and control plane | VM CLI and binary transfers verified on a real KVM node; control-plane CLI verification remains |
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
export HV2_SANDBOX_URL=http://127.0.0.1:3980
hm sandbox vm create --template base
hm sandbox vm list
hm sandbox vm exec SANDBOX_ID -- /bin/sh -c 'printf hello'
hm sandbox vm files SANDBOX_ID --envd-endpoint ENVD_URL upload local.bin /root/file.bin
hm sandbox vm files SANDBOX_ID --envd-endpoint ENVD_URL download /root/file.bin downloaded.bin
hm sandbox vm checkpoint save SANDBOX_ID before-change
hm sandbox vm checkpoint restore SANDBOX_ID before-change
hm sandbox vm pause SANDBOX_ID
hm sandbox vm resume SANDBOX_ID --lifetime 300
hm sandbox vm fork SANDBOX_ID --count 2
hm sandbox vm delete SANDBOX_ID

hm sandbox vm benchmark --template base --samples 100 --concurrency 8 \
  --environment 'CPU model; RAM; OS; nested/bare-metal; image hash; daemon commit' \
  --require-snapshot --max-p99-ready-ms 150 > readiness.json
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
Before timing samples it queries `/templates` and records the selected template's
server metadata, including snapshot availability, CPU and memory sizes. Failure
to obtain metadata is recorded explicitly. `--require-snapshot` refuses to create
any guests unless the server confirms snapshot mode; omit it when intentionally
measuring cold boots. Errors include their underlying cause.

### Fresh KVM readiness baseline

File transfers require a reachable sandbox envd listener or proxy URL. The client
calls the control API's `connect` operation to retrieve the sandbox token, resumes
paused sandboxes and extends their lifetime to at least 300 seconds. A separate
HTTP client sends that token to envd without forwarding `HV2_API_KEY`; redirects
are disabled. Binary contents and URL-special path characters are preserved.
Transfers are bounded to envd's 512 MiB limit. Downloads stream to a temporary
file beside the destination and publish only after completion, refusing to
overwrite even if another process creates the destination during the transfer.
Use TLS for remote envd URLs. A node's direct envd listeners bind on loopback;
remote clients need the proxy. The local KVM check verified a 256 KiB binary
upload/download and guest SHA-256, plus refusal to overwrite the downloaded file.
Protocol fixtures also checked HTTP failures and a concurrent destination creator.

The shipped Windows debug client was checked against the release daemon on
2026-09-30 using `tools/e2e-sandbox-vm-cli.py`. Quoted arguments remained
literal, guest exit 7 propagated, checkpoints restored a changed file,
pause/resume preserved it, both forks inherited it, and deletion cleaned up
the created guests. This verifies the node interface, not the control plane.

Eight runs of 100 samples each completed with **zero failed samples**.
Raw JSON is in [benchmarks/2026-09-30](benchmarks/2026-09-30/), named
`readiness-cN.json` and `readiness-prefault-cN.json`.

| Concurrency | Default P50 / P95 / P99 ready (ms) | Prefault P50 / P95 / P99 ready (ms) |
|---|---|---|
| 1 | 67.37 / 103.12 / 142.32 | 44.88 / 205.65 / 216.17 |
| 8 | 113.36 / 140.62 / 149.04 | 53.94 / 106.48 / 121.42 |
| 50 | 947.43 / 1493.28 / 1641.66 | 882.99 / 1363.84 / 1453.71 |
| 100 | 2679.86 / 2934.73 / 3038.36 | 1848.60 / 2132.19 / 2153.59 |

Hardware: Ryzen 9 9900X, 24 logical CPUs, 45.9 GiB available to WSL2 Debian,
nested Hyper-V/KVM, one vCPU and 1024 MiB per guest, prewarmed BusyBox/static
agent template, networking disabled. Kernel and initrd hashes, client build
mode and run timestamps are recorded in each JSON. The daemon code is based
on commit `2dbf31f`; the client includes the subsequent metadata/default-port
fixes on this branch. Other host workloads were active. Runs were sequential,
with default runs preceding prefault runs, so host variation and order effects
prevent attributing the differences solely to prefaulting. No default changes
or competitor wins follow from this experiment. High-concurrency tail latency
remains an optimization target; dedicated-host repeats and equivalent provider
runs are required.

A subsequent diagnostic daemon startup failed its 120-second template-agent
readiness check and fell back to cold boots. Its API was not yet available when
the diagnostic benchmark began: all 100 requests failed, with null latency
statistics. This is an additional startup-reliability gap, not a measured restore
latency. The original eight runs explicitly announced snapshot-backed templates.
The new snapshot gate prevents comparing an unnoticed fallback with those runs;
the underlying intermittent guest startup failure still needs diagnosis.
Operators can now start `hv2-sandboxd --require-template` to fail startup if
any configured snapshot template cannot be prepared, before API/proxy listeners
or cluster registration. It conflicts with `--no-template`. The default still
permits cold-boot fallback. `tools/e2e-template-policy.py` checks failure behavior
and, with `--check-ready`, verifies successful snapshot-backed startup using
the images supplied through `HV2_KERNEL` and `HV2_INITRD`.
The conflicting-option, strict missing-image failure and default fallback checks
passed against both Windows and Linux daemon binaries. Successful strict startup
with a snapshot-backed template passed on a real KVM guest.

A diagnostic repeat using strict startup and the client snapshot gate completed
20 samples at concurrency 1 and 100 at concurrency 100 with no failures. P99
readiness was 54.03 ms and 1327.48 ms respectively. Raw reports are
`readiness-strict-stages-c1.json` and `readiness-strict-stages-c100.json`; the
concurrency-100 internal timings are in `restore-stages-c100.log` in the same
benchmark directory. The replacement image hashes match the original images.
At concurrency 100, internal P50/P99 build time was 12.16/56.92 ms, launch was
30.68/76.88 ms and guest-agent readiness was 699.67/1030.54 ms. These stage
percentiles are independent and must not be added. Guest readiness dominates
the measured create path and is the next profiling target. Host workloads and
logging settings changed between cohorts, so this is diagnostic evidence,
not proof of an optimization or competitor win.

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
