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
| A CLI | yes | yes (over ssh) | **Yes**: `hm sandbox vm` manages lifecycle, commands, binary files and checkpoints; verified on a real KVM node and authenticated control plane/proxy |
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
| Scoped, expiring API keys | yes | yes | **Real on the control plane**: hashed operator-provisioned keys, request-time expiry and capability scopes; single team, startup-loaded policies |
| Persistent volumes shared between VMs | no | no | **Real**: E2B volumes over 9P, live and shared (Linux hosts) |
| Build images from Dockerfiles or OCI | compose | Dockerfile | **Real**: E2B template builds, no Docker |
| Backups to object storage | yes | no | **Absent** |
| Scheduled jobs and event triggers | `*.run.ts` | no | **Partial**: lifecycle webhooks only |
| Desktop in a browser, browser for agents | yes | web terminal | **Absent** |
| MCP for agents | skill + MCP | Shelley agent | **Partial**: 12 lifecycle/exec/checkpoint tools plus 2 opt-in binary file tools over MCP stdio, with cancellable client waits checked on real KVM; accepted remote work can continue, and streaming plus the wider `hv2-agent` surface remain absent |
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
| Firecracker | Specification targets at most 125 ms from InstanceStart to init and at most 5 MiB VMM overhead for its specified minimal guest | Engine isolation and efficiency | Latest fixed-IRQ comparison: 100/100 passes each, HyperMachine P50 859.85 ms versus Firecracker 773.83 ms. Earlier failures remain recorded; these bounded passes do not establish universal reliability. Specification metrics differ from application readiness and incremental PSS |

These figures describe the linked providers' own claims or repository runs,
not an independently reproduced ranking. A lower headline number does not
pass the acceptance criteria below.

| Workstream | Acceptance criterion | Current gap |
|---|---|---|
| Creation and execution latency | Same guest workload and readiness command; raw samples, failure rate, P50/P95/P99 at concurrency 1, 8, 50 and 100; lower latency than each tested competitor with repeatable results | Comparable competitor runs and bare-metal HyperMachine runs are missing |
| Stateful resume, pause and fork | Verify live process memory and filesystem state, then measure API-to-first-successful-command latency under the same concurrency | Shared SDK harness and real concurrency-8 cohorts verified; full concurrency sweep and matched competitor runs remain missing |
| Memory and density | Incremental PSS after the same command and idle period; same guest resources; document shared-template memory; preserve state through oversubscription | Vendor VMM overhead and our PSS are different quantities |
| Throughput and tails | Sustained arrivals on identical host resources; include failures, queueing, and recovery rather than counting accepted requests | One-node rates cannot establish a win against a million-sandbox managed fleet |
| CLI and SDK usability | Shipped client for lifecycle, execution, files and checkpoints, tested against a real node and control plane | VM CLI verified on a real KVM node and authenticated control plane, including binary files through the control-plane proxy |
| Isolation and governance | Enforced tenant boundaries, scoped expiring keys, roles, auditable access, and escape tests | Scoped expiring keys and protected API tracing verified; tenant boundaries, roles, durable audit retention and resource attribution remain incomplete |
| Networking and access | Custom domains, certificate automation, authenticated private URLs, SSH, raw TCP/UDP and isolated VM groups | HTTP proxy and egress are implemented; the listed access features remain absent |
| Platforms and workloads | Verified ARM64 execution, GPU sandboxes, browser/desktop workloads, and persistent storage limits | ARM64 execution and GPU sandbox wiring remain unverified or absent |
| Operations | Object-storage backups and recovery, quota enforcement, scheduling/event triggers, load-tested multi-node failover | Shared-directory snapshots and host job queues do not cover all these capabilities |

Mark a capability complete only after checking its effect through a shipped
interface. Mark a performance win only after equivalent runs establish it;
do not infer it from a vendor headline or a component microbenchmark.

Native-engine runs now explicitly cap both guests' readiness phase at 15 seconds.
Firecracker's cap begins after `InstanceStart` and is also bounded by its total
startup deadline (30 seconds by default). Older cohorts used HyperMachine's
15-second guest limit but Firecracker's remaining total startup budget; retain
that limitation when interpreting their failure rates. The next reports record
these budgets and the driver's actual CPU affinity. The generic host limitation
no longer says every run is unpinned: pinned coordinator profiles already record
their affinity separately. Eleven harness tests pass, including guest-deadline
clipping and cleanup on timeout. These methodology changes do not establish a
performance or reliability improvement.

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
For a proxy reached by IP or a shared hostname, `--envd-host` sets the sandbox's
virtual host while TLS still authenticates the supplied endpoint URL. The
`tools/e2e-sandbox-vm-cli.py --envd-proxy URL` check exercised binary transfers
through the real control-plane proxy, with an API key and separate cluster token.
Lifecycle, checkpoint rollback, pause/resume, forked state and cleanup also passed
through that control plane. Checkpoint routes require the API key and forward
to the sandbox's owning node with the cluster token.

The control plane's `/templates` now reports snapshot state and guest sizes from
node heartbeats. Snapshot readiness is true only when every offering node confirms
it, false if any reports cold boot, and null when confirmation is missing.
CPU/memory sizes are null unless all offering nodes report identical sizes;
per-node details remain visible. Legacy heartbeats remain readable and are
treated as unknown, so `--require-snapshot` cannot infer readiness from a name.
Advertisements update names and metadata together. These are point-in-time
heartbeat observations; they do not guarantee a fleet cannot change afterward.

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

The authenticated control-plane diagnostic cohort completed 20 samples at
concurrency 8 with no failures, server-confirmed snapshot mode, P50 readiness
223.15 ms and P99 325.55 ms. Raw evidence is
`benchmarks/2026-09-30/readiness-control-plane-c8.json`. Its metadata includes
the offering node, guest sizes and snapshot state. This validates the shipped
benchmark through the cluster interface; the small shared-host cohort does not
establish a latency advantage over other products.

## Shared SDK comparison harness

`tools/bench-e2b-sdk.py` measures create-to-verified-command readiness through
the unmodified [E2B Python SDK 2.51.0](https://pypi.org/project/e2b/2.51.0/).
Install it in an isolated virtual environment. Configure `E2B_API_KEY` in the
environment and supply explicit endpoints; leave `E2B_API_URL`,
`E2B_SANDBOX_URL` and `E2B_ENVD_POOL_SHARDS` unset. For the local fixture:

```powershell
python tools/bench-e2b-sdk.py --provider HyperMachine `
  --api-url http://localhost:14095 --sandbox-url http://localhost:14096 `
  --template base --environment 'Describe the actual client and host' `
  --image-description 'Describe the matched image and preparation' `
  --expected-cpus 1 --expected-memory-mb 1024 --samples 100 --concurrency 8
```

Use `--operation resume` or `--operation fork` to measure stateful lifecycle
readiness. Before the timed request, the harness starts a long-lived process
with a unique marker in its environment and writes a file and boot identity.
The first command after the operation must verify all three plus the original
live PID. Resume requires the API to report `paused` before connecting; fork
requires a distinct child ID, changes and verifies the child's private state
file, then verifies that the parent retains its original process and file state.
The child-write isolation check is outside readiness timing. It expects a
private `/tmp` filesystem; explicitly shared volumes require separate semantics.
These probes detect cold boots and lost process state; they do not exhaustively
validate every application or open socket. Creation and preparation are outside
resume/fork readiness timing. Lifecycle throughput includes them, state checks,
resource inspection and deletion. `pause_ms` measures the pause response alone.

For compatible managed providers, omit `--sandbox-url` to use their sandbox
domains. The harness disables internet access and retries, forces normal SDK
mode so deletion actually occurs, checks server-reported CPU/RAM, and deletes
every known sandbox. Incorrect output, resource mismatch or cleanup failure
fails the run. Dependency versions are captured before execution. Reports also
record `harness_sha256` for the exact script bytes (including checkout line
endings) and `harness_unchanged_during_run`. A source change during the run
fails its overall exit status even when every sample succeeds. This detects
ordinary edits, not malicious source replacement or guest-image equivalence.
Historical reports without these fields retain their original evidence; no
fingerprint is retroactively attributed to them. Failure accounting and
provenance are checked by `tools/test-bench-e2b-sdk.py` (12 tests). The CI
`Sandbox Benchmark and Chart Gates` job runs these tests without
SDK/cloud credentials, and also runs the chart render tests and Helm lint.
The default workload verifies POSIX command readiness; `--workload python` requires a real
Python image. Unknown IDs after an interrupted create cannot be cleaned up by
the client. Provider labels and guest sizes do not establish matched hardware,
image contents or snapshot preparation; those need independent evidence.

The first recorded SDK cohort completed 20/20 samples at concurrency 8 with
1 vCPU and 1024 MiB: readiness P50 **260.40 ms**, P95 **491.52 ms**, P99
**511.19 ms**. Raw evidence is `benchmarks/2026-09-30/readiness-e2b-sdk-c8.json`.
It uses Windows SDK transport, a BusyBox snapshot guest, debug profiling and
a shared nested-KVM host. It is distinct from the native CLI cohorts and does
not demonstrate a performance improvement. Lifecycle throughput also includes
the resource-info check and deletion. All sandboxes were confirmed deleted.

Correlated internal stages for the same 20 IDs are in
`benchmarks/2026-09-30/readiness-e2b-sdk-stages-c8.json`: blocking queue P50/P99
0.08/6.99 ms, guest connection 61.34/134.18 ms, restored clock/entropy exchange
15.77/31.93 ms. Independent percentiles must not be added. The connection
stage is the largest of these measured stages; this does not explain all SDK
latency or establish the cause of earlier concurrency-100 tails. Debug events
record timings and VM IDs without entropy, tokens or request contents.

This profiling exposed a restore-clock correctness issue: host time was
captured before worker scheduling and guest connection, so those waits became
clock lag when the guest applied the timestamp. The restored guest now receives
time sampled on the connected channel. Invalid host clock ranges fail instead
of becoming zero or wrapping. Protocol tests verify sampling after channel
creation, entropy preservation and propagation of RNG reseed failure. Remaining
transport and guest-processing delay is not compensated; this is a correctness
fix, not evidence of a latency reduction or competitor advantage.
The corrected release daemon passed five real SDK creates, five stateful
resumes and five stateful forks at concurrency 1, with no failures and all
known sandboxes deleted. Raw validation reports are
`benchmarks/2026-09-30/readiness-e2b-sdk-fresh-clock-{create,resume,fork}-c1.json`.
These small cohorts validate runtime compatibility and state preservation;
the protocol tests establish timestamp placement, not a measured bound on
guest clock offset. All 13 guest-protocol tests and strict agent Clippy passed.

No competitor endpoints or matched host are available from the user. Published
competitor claims remain separate from measured HyperMachine results; universal
feature or performance superiority is unverified.

Stateful SDK diagnostics on the same shared host completed 20/20 samples per
operation at concurrency 8, with every known sandbox confirmed deleted:

| Operation | Readiness P50 | P95 | P99 | State evidence |
|---|---:|---:|---:|---|
| Resume | 129.73 ms | 183.29 ms | 188.91 ms | Explicit paused state; original live process, memory marker, file and boot identity |
| Fork | 126.63 ms | 183.93 ms | 186.61 ms | Distinct child; preserved child and parent process/memory/file/boot state |

Raw reports are `benchmarks/2026-09-30/readiness-e2b-sdk-resume-state-c8.json`
and `readiness-e2b-sdk-fork-state-c8.json`. Earlier exploratory cohorts without
the explicit paused/running-state gate remain in `readiness-e2b-sdk-resume-c8.json`
and `readiness-e2b-sdk-fork-c8.json`; do not combine their percentiles. Neither
cohort is a matched competitor comparison or proof of a performance improvement.

A stronger fork cohort completed 20/20 samples at concurrency 8, each with
`fork_filesystem_isolation_verified: true`: child mutations did not change the
parent's saved file, and both retained their live process state. All parent
and child sandboxes were confirmed deleted. Raw evidence is
`benchmarks/2026-09-30/readiness-e2b-sdk-fork-isolation-c8.json`, including harness
fingerprint and unchanged-source confirmation. Readiness P50/P95/P99 was
125.49/193.95/194.29 ms. This cohort used warning-level daemon logging on the
shared nested-KVM host; do not infer a latency change from earlier cohorts
with different logging and workloads. Extra isolation commands are included
in lifecycle throughput, not readiness latency. Earlier reports without the
isolation field do not establish this new check. This probes filesystem fork
independence, not comprehensive security isolation or shared-volume behavior.

## Scoped, expiring control-plane keys

`hv2-control-plane --api-keys-file keys.json` loads an operator-provisioned
policy array before opening listeners or the store. Each entry requires
`sha256` (64 hex characters), `expires_at` (Unix UTC seconds) and a nonempty
`scopes` array. Use a unique, randomly generated high-entropy key for each
credential; the policy holds only its digest. For example, with a credential
already configured in `HV2_SCOPED_API_KEY`, generate a one-hour inventory policy:

```python
import hashlib, json, os, time
from pathlib import Path
policy = [{"sha256": hashlib.sha256(os.environ["HV2_SCOPED_API_KEY"].encode()).hexdigest(),
           "expires_at": int(time.time()) + 3600, "scopes": ["inventory"]}]
Path("keys.json").write_text(json.dumps(policy))
```

Clients send the original credential as `X-API-Key`; the existing SDK and CLI
authentication works unchanged. Expiration is checked on every protected API
request, including existing client connections. Missing, wrong and expired
credentials return 401; a valid key without the required scope returns 403.
Malformed, empty and duplicate-key policies fail startup rather than disabling
authentication. A maximum of 256 policies is accepted.

| Scope | Capability |
|---|---|
| `inventory` | GET/HEAD sandbox listings, template listings, aggregate sandbox metrics and cluster nodes; no sandbox detail or access tokens |
| `sandboxes` | Sandbox API operations, including create, detail/access token retrieval, execution, lifecycle, checkpoints and networking |
| `templates` | Template and snapshot APIs, including builds, uploads and deletion |
| `volumes` | Volume management and its credentials |
| `events` | `/events` APIs, including webhook management and delivery inspection |
| `admin` | All protected control-plane APIs |

Scopes combine and apply to the entire configured team, with no tenant or
per-sandbox restriction. Inventory deliberately excludes sandbox detail,
volume APIs and webhook APIs: their credentials can grant mutation through
other interfaces. Public health/metrics/OIDC endpoints and existing bearer-token
upload/content routes retain their existing authentication model. Expiring an
API key does not revoke previously issued envd, upload or volume bearer tokens,
terminate running guest operations, or expire a distinct legacy `HV2_API_KEY`
admin credential. Startup rejects an admin key whose digest appears in any
scoped policy, including an expired one. Embedded library instances that skip
startup validation enforce the scoped policy rather than granting legacy admin
access for an overlapping credential. Nodes in a
cluster must require their cluster token so direct node calls cannot bypass the
control plane. Policies load at startup; rotation requires restarting every
control-plane replica with the same updated policy. Dynamic key administration,
immediate bearer revocation, tenant roles and access audit logs remain open.

The sandbox Helm chart supports a read-only policy Secret:

```yaml
auth:
  apiKeysSecret: sandbox-api-policies
  apiKeysSecretKey: keys.json
  legacyAdminEnabled: false
```

Create the Secret from the policy file in the release namespace, then deploy
with these values and a control-plane image built from this revision. The
existing published image tag is not evidence that it supports the new flag.
The chart projects the selected Secret key to `/etc/hv2-api-keys/keys.json`
and passes `--api-keys-file`. Policy-only mode removes `HV2_API_KEY` from the
control-plane environment; cluster authentication and the store password remain.
The default keeps legacy admin authentication. The chart rejects disabling it
without a policy Secret, nonboolean mode values and an empty policy Secret key.
Missing or malformed policy data prevents startup. After changing the policy,
run `kubectl rollout restart deployment/RELEASE-hv2-control-plane` and wait for
the rollout in the release namespace. Every replica must load the same policy;
Secret projection updates alone do not reload it.

`tools/test-sandbox-chart.py --helm HELM_BINARY` verifies default and policy-only
authentication, all eight policy/mTLS/store-TLS mount combinations and invalid
configuration rejection. Four render tests and Helm 3.17.3 lint passed. These
checks parse rendered YAML and validate mount/argument consistency; no live
Kubernetes deployment has been performed for this change.

Validation: 27 cluster library tests, 13 real-HTTP integration tests and strict
cluster Clippy passed. `tools/e2e-control-keys.py --control-plane BINARY` checks
the shipped Windows binary's scope enforcement, live expiry, token-access
denial, fail-closed empty-policy startup and rejection of admin/scoped credential
overlap without modifying user credentials. Startup errors omit the credential.
Real-HTTP node fixtures additionally verify sandbox creation by a scoped key,
token-free inventory, denied writes and legacy admin compatibility. This
validation does not establish multi-tenant isolation or a performance win.
The CI test matrix is configured to build the shipped control-plane binary
and run `e2e-control-keys.py` on Linux, Windows and macOS. Local Windows
execution is verified; remote matrix results remain subject to CI completion.
The first remote matrix skipped these checks after a `setup-protoc` server
error, so it is not cross-platform authentication evidence. CI now installs
checksum-locked protoc 23.4 from fixed official release URLs, without release
enumeration, and preserves independent platform jobs when one fails. The
installer rejects altered archives and unsupported hosts; its platform and
checksum tests pass, and the pinned Windows and Linux x86-64 compilers run
locally. On CI run `36770058106`, pinned compiler installation and execution
passed on Linux, Windows and macOS. Workspace tests and shipped authentication
checks still need their own completed results; successful setup is not proof
of those downstream checks.

### Protected API access records

The control plane emits structured tracing events under `hv2_cluster::access`
for protected API requests. Accepted requests have a start event and a response
event sharing a generated request ID. Denied requests also have response events.
Fields include the route template, a standard HTTP method category, credential
category, HTTP status and elapsed time until the response is constructed.
Configured credentials are identified by the first 16 hex characters of their
SHA-256 digest; unknown credentials are never fingerprinted. Request headers,
bodies, queries, supplied path values and nonstandard method strings are omitted.
The existing info-level logger emits these events by default; an operator can
select them with `RUST_LOG=warn,hv2_cluster::access=info`.

The shipped Windows binary checks verify allowed and denied status records,
expired-key records, matching start/response IDs and absence of fixture
credentials or private request values from the logs. All 13 real-HTTP
authorization tests and strict cluster Clippy passed after this change.

These are diagnostic access records, not a durable or tamper-evident audit
store. Route templates omit resource attribution. Public and independently
authenticated bearer/proxy routes are outside this middleware. A response
record reports headers/status, not completion of a streamed body; interrupted
accepted requests may have only a start event. Retention, comprehensive guest
activity auditing and performance impact under load remain unverified.

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

## Interactive SDK output validation

`tools/e2e-sdk-interactive.py` exercises the unmodified E2B Python SDK 2.51.0
against a real snapshot guest. Each operation starts a fresh process, varies
input arrival by 0–40 ms outside the timer, then measures the input/EOF/signal
request through verified process exit. It checks the exact stdin marker and
exit status, records failures separately, and deletes its sandbox. SIGKILL
must produce exit status 137. Run with explicit API/proxy URLs, `--environment`,
`--label`, and an `E2B_API_KEY` environment variable; `--samples` is per operation.

Successful input, EOF and signal requests now notify that process's output
poller; idle output retains the 50 ms tick. The first faster-poll cohort exposed
an existing guest race: exit could be published before pipe readers collected
final output. The corrected agent waits for reader completion and observes
completion before draining buffers. A Linux regression checks final stdout and
stderr across 100 rapidly polled child processes. Processes whose descendants
retain output pipes keep their streams open until those pipes close.

| Cohort | Successful checks | Stdin P50/P99 | EOF P50/P99 | Signal P50/P99 |
|---|---:|---:|---:|---:|
| Fixed polling, original guest | 60/60 | 43.20/94.78 ms | 33.21/77.16 ms | 41.61/87.18 ms |
| Input wake, original guest | 59/60 | 9.64/63.41 ms | 6.74/61.08 ms | 8.39/63.31 ms |
| Input wake, corrected output drain | 300/300 | 56.14/99.64 ms | 50.18/100.99 ms | 56.31/101.29 ms |

The failed wake-only stdin sample had an output/exit mismatch; its result is
retained and that cohort is invalid as a passing benchmark. Percentiles include
successful samples only, so its lower timings cannot establish an improvement.
The corrected cohort validates compatibility and cleanup, but is slower than
the baseline in these diagnostics. No latency gain is established. Cohorts ran
sequentially on the shared Windows/WSL nested-KVM host with warning logs, one
sandbox, 1 vCPU/1024 MiB, and 20 versus 100 samples per operation. Host contention,
reader scheduling and different guest binaries prevent causal attribution or a
competitor comparison. Notifications remove an intentional wait when queued;
RPC, process execution and scheduling delay remain.

Raw reports under `benchmarks/2026-09-30/` are
`interactive-fixed-poll-c1.json`, `interactive-input-wake-c1.json`, and
`interactive-wake-output-drain-c1.json`. The corrected image SHA-256 is
`1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c`;
the earlier cohorts use the original image documented above. Reports capture
the interactive script digest at completion; unlike the shared readiness
harness, this diagnostic does not check for source edits during execution.
All known sandboxes were confirmed deleted after each cohort.

The full GitHub CI run [36772587222](https://github.com/nervosys/HyperMachine/actions/runs/36772587222)
passed for commit `a422192`, including Linux/Windows/macOS workspace tests,
shipped control-plane authorization checks, benchmark/chart gates and Clippy.
This predates the interactive wake/output-drain change; its new-head CI must
pass separately. Local checks for that change passed the three notification
tests, strict API Clippy, 15 Linux guest-protocol tests, the 100-process regression,
and 300 real SDK operations. New-head CI at `d306f4a` found the regression test
module placed before production functions (`items_after_test_module`). An
all-targets local check found the same issue in the API notification tests.
Moving both modules to the end preserves runtime behavior; strict Linux guest Clippy
then passed locally using an extracted Debian Clippy 1.95 package matching the
installed Rust compiler. Strict API Clippy also passed with all targets included.
The corrected head still needs its own CI result.

### CI baseline comparison reliability

The separate Benchmarks workflow at `a422192` failed in its comparison job
before any baseline ran: both repositories are checked out under `baseline/`
and `pr/`, but the protoc installer was invoked from the empty workspace root.
It now runs from `pr/`; the baseline checkout uses the exact PR base commit,
and the artifact records both actual commits, compiler versions and runner CPU.
Baseline execution, copying and candidate execution failures now fail the job
instead of being ignored. A fixture verified that nested Criterion baseline
files reach an initially absent candidate target directory. Actionlint passed.
Raw comparison estimates, output and environment metadata are uploaded even
on failure. These are sequential crypto microbenchmarks on a shared runner,
with regression reporting rather than a latency gate; they do not establish
sandbox readiness or superiority over any competing product. Remote execution
of the corrected workflow remains pending.

The primary crypto/API benchmark job also separates measurements from the
restored Cargo cache: a fresh `CRITERION_HOME` under the runner temporary
directory is used for execution, conversion and artifact collection. Both
benchmark commands must succeed before results are stored. The converter
rejects missing/empty results, incomplete estimates, nonnumeric/nonfinite or
invalid timing values, invalid names and duplicate names instead of silently
emitting empty or partial results. Fifteen standalone PowerShell fixture checks
passed, including single/multiple JSON array serialization and sorting;
actionlint passed. CI runs these checks before benchmarks and uploads raw
measurement JSON and logs even when a benchmark fails. This prevents cached
measurements from being reported as a fresh run. The primary Performance
Benchmarks job passed at `518fe70` (run 36781437624); its separate baseline
comparison was cancelled by the newer MCP commit; the new-head comparison remains unverified. These Criterion means and standard errors are component
microbenchmarks, not VM lifecycle P50/P95/P99 or matched competitor measurements.

## Guest boot debug-scan experiment (2026-09-30)

The fixed-UART guest still performs Linux's `CONFIG_DEBUG_WX` boot-time
page-table check. We tested disabling only that defconfig option, retaining
strict kernel/module W^X protections, MP-table/I/O APIC support, serial
console, virtio networking/vsock and 9P. The experimental Linux 6.6.52
kernel built from the checksum-pinned source, then passed real-KVM
checkpoint rollback, pause/resume and two state-preserving forks.

Twenty counterbalanced blocks alternated kernel order; each kernel run
alternated two engine pairs on a fresh isolated node. Both engines received
identical kernel/initrd bytes, 1 vCPU/1024 MiB and a 15-second guest readiness
budget. One pinned CPU worker remained alive throughout. All 160 attempts
passed, artifact identities remained unchanged and owned processes were
cleaned up. This was another shared nested-KVM run, with substantial host
noise; its timings cannot be substituted into earlier cohorts.

| Kernel debug scan | Engine | Passing attempts | Readiness P50 | Readiness P99 |
|---|---|---:|---:|---:|
| Enabled, current default | HyperMachine | 40/40 | 1527.00 ms | 4648.16 ms |
| Disabled, experiment | HyperMachine | 40/40 | 1446.36 ms | 8929.95 ms |
| Enabled, current default | Firecracker 1.17.0 | 40/40 | 1343.26 ms | 6906.98 ms |
| Disabled, experiment | Firecracker 1.17.0 | 40/40 | 1412.88 ms | 12351.56 ms |

Disabling the scan was faster for HyperMachine in only 8/20 blocks and
increased its overall mean by 452.59 ms; the median paired block mean
increased by 61.07 ms. The descriptive paired bootstrap interval for the
enabled-minus-disabled mean was [-953.66, -16.87] ms (10000 resamples,
seed 0); shared-host drift limits causal interpretation. Forty samples per
group also make P99 the maximum observation, a weak estimate of a tail.
**No repeatable improvement was established; the default retains the scan.**
This experiment also does not establish a new engine ranking or regression.

`tools/guest-image/build-kernel.sh` now accepts `--config DEFINITION` and
`--config-output RESOLVED_CONFIG`, while preserving its default defconfig.
It resolves input paths before changing directories, rejects identical
kernel/config output paths, and saves the resolved configuration after a
successful build. The exported experimental config and defconfig are
`benchmarks/2026-09-30/debug-wx-off-resolved.config` and
`debug-wx-off.defconfig`; raw/summary reports, exact benchmark/analysis
sources and the state probe share the `debug-wx-` prefix. Reports record
the kernel, configuration, builder, daemon and harness hashes. Reproduction
requires supplying local artifact paths in the recorded coordinators.

## Shipped MCP sandbox interface

`hm sandbox vm --endpoint https://sandbox-api.example.com mcp` serves MCP
2025-11-25 over standard input/output. It follows the
[stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
[initialization lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
and [tool result](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
interfaces. Configure `HV2_API_KEY` in the server process environment through
your client's environment settings or secret store. `HV2_SANDBOX_URL` also
sets the endpoint. API scope and expiry are enforced by the control plane.
The command opens no additional network listener and uses the same remote
API client as the VM CLI, with redirects disabled and a 120-second default
HTTP deadline. CLI tracing goes to stderr so stdout contains protocol only.

The tools are `sandbox_create`, `sandbox_list`, `sandbox_inspect`,
`sandbox_exec`, `sandbox_pause`, `sandbox_resume`, `sandbox_fork`,
`sandbox_delete`, `checkpoint_save`, `checkpoint_list`, `checkpoint_restore`
and `checkpoint_delete`. Execution accepts an argv array, quotes it for the
guest shell, and reports guest failure/timeout as `isError`. Partial forks
also report `isError` while retaining successful child IDs. Structured
`envdAccessToken` and `accessToken` fields are removed recursively; upstream
HTTP error bodies are not echoed. Guest command output is still returned as
requested and may contain data the command itself prints.

Add `--envd-endpoint https://sandbox-proxy.example.com` after `mcp` to expose
`file_upload` and `file_download`; for a local proxy that routes by Host, also
set `--envd-domain sandbox.local`. This endpoint is operator configuration,
not a tool argument. Each file call connects through the control API, then
uses only the sandbox token at envd; the platform key stays on the control
API client. Tool results do not expose either credential. Uploads replace
the specified guest file. Both tools use base64 and accept at most 256 KiB
of decoded bytes, with a streaming download limit even without Content-Length.
Guest paths are encoded as literal query values, and no host file is read
or written. File results include matching text and structured JSON. The
smaller file limit keeps both representations within the 1 MiB frame budget.

Discovery and tool calls require initialization; notifications do not execute
tool calls or receive responses. Invalid IDs/arguments are protocol errors,
and input frames larger than 1 MiB terminate the session. Tool operations
execute sequentially, while the transport continues reading bounded input.
Streaming output, Streamable HTTP,
resources/prompts/tasks and the wider `hv2-agent` tool surface
remain unsupported. Created VMs persist until deleted or their lifetime
expires; closing the client does not delete them. Interrupted creations can
leave IDs unknown to the client, as with the other API clients.

Six protocol/failure-accounting tests, all six existing shipped CLI HTTP tests,
and strict all-targets CLI Clippy passed.
`tools/e2e-mcp-sandbox.py` used the unmodified official MCP Python client 1.23.3
to launch the shipped debug binary and complete 21 tool calls through all 12
tools on a real nested-KVM release node. It verified exact guest output,
1 vCPU/1024 MiB resources, checkpoint rollback, pause/resume file state,
distinct fork identity and private-file independence, guest exit code 7,
HTTP failure reporting and deletion of both parent and child. The node's
sandbox list was then confirmed empty. Raw functional evidence, with client,
script and binary identities, is `benchmarks/2026-09-30/mcp-stdio-lifecycle.json`.
This is compatibility evidence, not a latency benchmark or a competitor win.

The file-tool extension passed all 15 sandbox CLI/MCP tests and strict
all-targets CLI Clippy. The official client 1.23.3 then discovered all 14
tools and completed 25 calls on an isolated 1 vCPU/1024 MiB real KVM node,
including a 256 KiB roundtrip containing every byte value at a guest path
with a quote and ampersand, plus rejection of a 262145-byte download.
The existing checkpoint, pause/resume, fork isolation and failure checks
also passed. The client deleted both guests, the node list was empty, all
artifacts retained their recorded hashes, and the owned node was stopped.
Raw evidence is `benchmarks/2026-09-30/mcp-stdio-files.json`; its exact
coordinator is `mcp-stdio-files-coordinator.py`. This functional check does
not measure performance or establish full agent-tool parity.

The transport handles MCP `notifications/cancelled` during an active API
wait, matching the original request's string or integer ID. It drops the
client's HTTP future and sends no response for that cancelled request.
Queued requests can also be cancelled before reaching the API. Unknown,
completed and malformed cancellation notifications are ignored, and
`initialize` is not cancellable. Up to 8 queued messages and at most 1 MiB
of aggregate queued input are accepted; exceeding either bound terminates
the session. Partial frames survive a response/cancellation race and retain
the cumulative 1 MiB frame limit. Input EOF drops the active client wait.
This follows the [MCP cancellation protocol](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation).

**Cancellation does not undo accepted remote work or kill a guest process.**
The underlying lifecycle, file and exec APIs may continue their operations;
an interrupted creation can still leave an ID unknown to the client.
Stopping guest processes requires the existing process signal interface,
and deleting sandboxes requires an explicit lifecycle call.

All 22 sandbox CLI/MCP tests and strict all-targets CLI Clippy pass on
Windows and Linux. The official client
1.23.3 cancellation exercise confirmed that a real
guest command had started before sending a cancellation notification. The
same session then answered a ping within the two-second functional budget
(1.73 ms observed), sent no cancelled-request response, and remained usable.
The guest command finished afterward, explicitly verifying the limitation
above. The 25 existing tool calls, one cancelled exec request and one ping
also retained the binary-file, checkpoint, pause/resume, fork and failure
checks. Both guests were deleted, the node list was empty, the owned node
stopped and recorded artifact hashes were unchanged. Raw evidence and its
exact coordinator are `benchmarks/2026-09-30/mcp-stdio-cancellation.json`
and `mcp-stdio-cancellation-coordinator.py`; the fixture uses public official
client APIs and observes the outgoing protocol ID through a transparent
write-stream wrapper. This is a functional responsiveness check, not a
latency benchmark or full remote-operation cancellation.


## Same-host Firecracker cold comparison (2026-09-30)

The user has no competitor endpoints or dedicated matched host. We therefore
ran Firecracker v1.17.0 locally against HyperMachine on the same shared WSL
nested-KVM host. Cohorts contain 20, 100 or 200 alternating AB/BA pairs, concurrency
one, 1 vCPU and 1024 MiB. Both engines boot the exact same Linux kernel and
BusyBox guest-agent initrd and must return an exact unique shell marker.
HyperMachine uses a prestarted HTTP daemon with `--no-template`; Firecracker
starts a process and configures its UNIX API for each sample. Timings end at
the verified first command response. These native control paths differ, and
the host has other workloads. Most cohorts are unpinned; the explicitly labeled
pinned contention cohort places both engines and one CPU worker on CPU 0. This is a cold sandbox
comparison, not SDK, managed-platform, snapshot, density or bare-init timing.

| Cohort | Engine | Valid / attempted | Readiness P50 (ms) | Readiness P99 (ms) |
|---|---|---:|---:|---:|
| Initial | HyperMachine | 19 / 20 | 1913.34 | 4222.52 |
| Initial | Firecracker 1.17.0 | 20 / 20 | 1137.38 | 2468.34 |
| Repeat with memory diagnostics | HyperMachine | 18 / 20 | 1230.74 | 1819.96 |
| Repeat with memory diagnostics | Firecracker 1.17.0 | 20 / 20 | 816.65 | 2408.49 |
| Normal-logging reliability repeat | HyperMachine | 98 / 100 | 1174.44 | 2212.68 |
| Normal-logging reliability repeat | Firecracker 1.17.0 | 100 / 100 | 802.88 | 2485.07 |
| Daemon owner-diagnostic build | HyperMachine | 200 / 200 | 692.47 | 763.00 |
| Daemon owner-diagnostic build | Firecracker 1.17.0 | 200 / 200 | 403.88 | 454.65 |
| Two unpinned CPU workers | HyperMachine | 100 / 100 | 717.83 | 823.35 |
| Two unpinned CPU workers | Firecracker 1.17.0 | 100 / 100 | 414.28 | 470.27 |
| One contending worker, all pinned to CPU 0 | HyperMachine | 91 / 100 | 1159.99 | 1324.16 |
| One contending worker, all pinned to CPU 0 | Firecracker 1.17.0 | 100 / 100 | 820.63 | 874.96 |
| Pinned APIC diagnostic repeat | HyperMachine | 92 / 100 | 2771.78 | 5910.02 |
| Pinned APIC diagnostic repeat | Firecracker 1.17.0 | 94 / 100 | 2266.94 | 4000.67 |
| PIT stub and API-readiness correction, pinned repeat | HyperMachine | 93 / 100 | 1748.13 | 3366.14 |
| PIT stub and API-readiness correction, pinned repeat | Firecracker 1.17.0 | 100 / 100 | 1245.19 | 3125.96 |
| Pre-kick PIC/PIT diagnostics, matched readiness deadlines | HyperMachine | 97 / 100 | 1170.51 | 3865.63 |
| Pre-kick PIC/PIT diagnostics, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 815.35 | 3719.40 |
| APIC base diagnostic, matched readiness deadlines | HyperMachine | 97 / 100 | 1313.56 | 3346.01 |
| APIC base diagnostic, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 913.93 | 2321.68 |
| Singleton topology correction, matched readiness deadlines | HyperMachine | 93 / 100 | 1796.93 | 11752.53 |
| Singleton topology correction, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 1373.81 | 4418.93 |
| Owner interrupt events, matched deadlines and thread probes | HyperMachine | 82 / 100 | 1897.66 | 6517.16 |
| Owner interrupt events, matched deadlines and thread probes | Firecracker 1.17.0 | 100 / 100 | 1488.91 | 5003.76 |
| KVM retry counters, matched readiness deadlines | HyperMachine | 91 / 100 | 1896.21 | 8296.29 |
| KVM retry counters, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 1631.53 | 6573.30 |
| Singleton MP table, matched readiness deadlines | HyperMachine | 100 / 100 | 1107.25 | 2263.95 |
| Singleton MP table, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 804.85 | 1740.15 |
| Singleton MP table, 200-pair pinned repeat | HyperMachine | 200 / 200 | 1079.50 | 1867.38 |
| Singleton MP table, 200-pair pinned repeat | Firecracker 1.17.0 | 200 / 200 | 769.48 | 1573.45 |
| Counterbalanced kernel blocks, original kernel | HyperMachine | 100 / 100 | 1119.17 | 2645.60 |
| Counterbalanced kernel blocks, original kernel | Firecracker 1.17.0 | 100 / 100 | 787.76 | 1956.21 |
| Counterbalanced kernel blocks, fixed UART IRQ | HyperMachine | 100 / 100 | 859.85 | 1922.86 |
| Counterbalanced kernel blocks, fixed UART IRQ | Firecracker 1.17.0 | 100 / 100 | 773.83 | 1375.09 |

Percentiles use nearest rank over successful, cleaned-up samples only; failures
are retained in the reports and invalidate the first three normal-logging comparisons
and all pinned contention comparisons before the singleton MP-table change.
The 200-pair, unpinned two-worker and singleton MP-table cohorts passed. The lower
HyperMachine repeat P99 is not evidence of a performance win. HyperMachine
failed 5/140 attempts in the first three normal-logging cohorts while Firecracker passed 140/140. HyperMachine's median was slower in every recorded normal-logging cohort. We have not achieved across-the-board superiority.

Both memory-diagnostic repeat failures were 15-second guest-agent readiness timeouts with no
console output and 2401 total vCPU exits, with no further exits during the
0.5-second diagnostic window. The startup stall's root cause is not yet known.
Node PSS fell after deletion and plateaued around 72 MiB in the latter half of
the repeat, arguing against a growing per-VM memory leak in this cohort; it
is not a density benchmark. Memory collection occurs outside readiness timing.

Raw evidence: [initial cohort](benchmarks/2026-09-30/local-engines-cold-c1.json),
[diagnostic repeat](benchmarks/2026-09-30/local-engines-cold-memory-c1.json),
and [Firecracker bring-up](benchmarks/2026-09-30/firecracker-cold-bringup.json).
The reports retain failures, cleanup outcomes and before/after source hashes.
`tools/install-firecracker-benchmark.py` pins the official
[1.17.0 release](https://github.com/firecracker-microvm/firecracker/releases/tag/v1.17.0)
archive checksum before executing its binary. `tools/bench-local-engines.py`
reproduces the alternating comparison; its failure-accounting tests run in CI.


Cold-start investigation can opt in to
`--hypermachine-log-filter warn,hv2_core::backends::kvm::boot=trace`.
The KVM trace samples exit reason, RIP, flags and I/O port on the vCPU owner
thread after `KVM_RUN` returns. Each failed HyperMachine sample preserves its
immediate node log tail. Reports mark non-default logging as diagnostic
tracing; the extra register ioctls and log writes perturb timing, so those
reports must not be used as performance comparisons. Normal runs retain the
`warn` default. The tracing is investigative instrumentation, not a startup fix.


Five installer integrity tests cover checksum rejection before execution,
linked/duplicate binary rejection, preservation of an existing different
binary, extraction of only the selected binary, official version parsing with
its extra exit log, and refusal of a mismatched executable version. They run
in the benchmark CI gate without downloading or executing fixture binaries.


The first [boot-tracing diagnostic](benchmarks/2026-09-30/local-engines-boot-trace.json)
passed 20/20 HyperMachine and 20/20 Firecracker workload/resource/cleanup checks.
Before/after artifact hashes matched, and the retained log confirms actual KVM
exit tracing. It did not reproduce a failed startup, so it yields no failure
address or root cause. No startup behavior was changed, and tracing perturbs
timing; this cohort does not establish a reliability fix or performance gain.
The earlier 3/40 HyperMachine startup failures remain relevant evidence.


The [100-pair normal-logging repeat](benchmarks/2026-09-30/local-engines-cold-normal-100.json)
reproduced two HyperMachine readiness failures (pairs 25 and 60), each with no
console output and 2401 total exits. HyperMachine passed 98/100 checks;
Firecracker passed 100/100. Artifact hashes were unchanged, setup and final
node cleanup had no errors, and the overall report correctly failed. Successful
samples had HyperMachine/Firecracker P50 of 1174.44/802.88 ms and P99 of
2212.68/2485.07 ms. The smaller HyperMachine P99 excludes failed startups and
is not evidence of a win. No startup behavior was changed, and the earlier
failure is confirmed to persist with diagnostic tracing disabled.


The [100-pair tracing diagnostic](benchmarks/2026-09-30/local-engines-boot-trace-100.json)
passed 99/100 HyperMachine and 100/100 Firecracker checks. HyperMachine pair 58
failed with the same no-console, 2401-exit signature. The preserved trace ends
at I/O port 1017 (`0x3f9`, COM1 interrupt-enable register), RIP
`0xffffffff81648115`, flags `0x6`, followed by about 15 seconds without another
recorded exit. Disassembly of the exact guest kernel ELF confirms an `out`
instruction at that address. The extracted ELF SHA256 is
`e97635a1ec12e5611b68906eb900baeef2f77ee23a89015810e0efe65255e425`.
This locates the last observed exit during UART initialization, not necessarily
the subsequent stalled guest instruction or root cause. Further architectural
state inspection must run on the vCPU owner after it leaves `KVM_RUN`; reading
registers concurrently with execution would not be sound evidence. This is a
failed diagnostic cohort, not a performance comparison or reliability fix.


Readiness-failure reporting now requests architectural samples through
`VM::diagnostic_vcpu_states`: a bounded channel message plus vCPU kick asks the
execution owner to read its state between backend run calls. The response wait
is bounded to five seconds. Each CPU is sampled independently; these are
investigative samples, not an atomic restore snapshot. Replies include RIP, flags, CR3, run state and RSP, plus the raw LAPIC
timer, priority and ISR/IRR registers when a complete register image is available.
Absent or malformed LAPIC state is explicitly marked unavailable. Successful readiness
checks do not request these samples. A real-KVM regression obtained ten samples
each from an interrupt-disabled halted guest and an interrupt-disabled spinning
guest, checked their exact raw-code instruction positions, then stopped both.
Strict all-targets Clippy passed for the core, agent and daemon on Windows.

`cargo build --locked --profile test -p hv2-agent --example cold_owner_diagnostics`
builds a smaller probe of the same AgentVM cold-boot path. Run it as
`cold_owner_diagnostics KERNEL INITRD 100`. It uses the daemon's 1-vCPU/1024-MiB
configuration and shared kernel arguments, prints one JSON record per attempt,
samples owners after failed 15-second guest readiness, and cleans up every
launched VM before continuing. This is a diagnostic-only probe under the test
profile, not a release API or competitor performance benchmark.


The [test-profile owner probe](benchmarks/2026-09-30/cold-owner-diagnostics-test-profile.json)
completed 100/100 guest-readiness checks and cleanups with unchanged binary,
source, kernel and initrd hashes. It did not reproduce a failure and therefore
captured no stalled guest state. Its lower-level control path and optimization
profile differ from the failing release daemon; it does not establish that the
startup defect is fixed. The later pinned contention cohort verified live daemon failure-report integration
in nine failed readiness responses.


The [release-profile lower-level probe](benchmarks/2026-09-30/cold-owner-diagnostics-release.json)
completed 200/200 readiness checks and cleanups, with unchanged artifact hashes,
but captured no stalled state. Its control path still differs from the daemon:
it uses the machine model's automatic COM1 attachment, whereas the daemon
explicitly initializes and registers COM1 before launch, as well as a different
CID range and no HTTP request lifecycle. No root cause or fix follows from
this non-reproduction. The later pinned contention cohort reproduced the actual daemon failure and
sampled its architectural state beyond the previously captured UART exit.


The [rebuilt-daemon 200-pair cohort](benchmarks/2026-09-30/local-engines-daemon-owner-200.json)
passed 200/200 workload/resource/cleanup checks for each engine, with unchanged
artifacts and no setup or final-cleanup errors. It captured no readiness
failure and therefore no stalled architecture. HyperMachine P50/P99 was
692.47/763.00 ms versus Firecracker 403.88/454.65 ms, so it remained slower
on this measured cold control path. The implementation added only failed-boot
diagnostics, not a startup fix. Differences from older cohorts on this shared
host do not prove a reliability or latency improvement. A direct
`KVM_GET_SUPPORTED_CPUID` query returned 62 entries with MONITOR, MWAITX and
WAITPKG absent, so the proposed advertised-CPU-delay-feature explanation is
unsupported on this host; no CPU features were changed.


The [two-worker CPU-load cohort](benchmarks/2026-09-30/local-engines-owner-load-100.json)
passed 100/100 workload/resource/cleanup checks for each engine. Both controlled
CPU workers stayed alive through the full cohort and were terminated and reaped
afterward. The [coordinator fixture](benchmarks/2026-09-30/owner-load-coordinator.py)
retains the exact workload and this host's paths; its checksum matches the raw
report. Neither workers nor engines were pinned. HyperMachine P50/P99 was
717.83/823.35 ms versus Firecracker 414.28/470.27 ms. No readiness warning or
stalled-owner sample was produced. This supports only these measured attempts,
not a fixed startup defect or superiority under general host contention.


The [pinned contention cohort](benchmarks/2026-09-30/local-engines-owner-pinned-load-100.json)
completed only 91/100 HyperMachine attempts versus 100/100 Firecracker attempts.
Both engines, the driver and one contending CPU worker inherited CPU-0 affinity.
The worker stayed alive throughout and was terminated and reaped afterward;
artifact hashes were unchanged and node cleanup completed. Its
[exact coordinator](benchmarks/2026-09-30/owner-pinned-load-coordinator.py) SHA256
is `c98e6289443031d49dc5cde7dd49727c28bff4aa7e13ec2489c25decf91b60e7`,
matching the raw report. Failed attempts invalidate this performance comparison;
the successful-sample percentiles in the table are not a reliability-adjusted win.

All nine failures had no console output, 2401 total exits and no further exits
during the diagnostic window. Owner samples consistently returned
`RIP=0xffffffff81eda95f`, `CR3=0x2a2e000` and flags with interrupts enabled.
Disassembly of the exact benchmark kernel shows `sti` at `0xffffffff81eda95d`,
`hlt` at `0xffffffff81eda95e`, then `cli` at the captured RIP. This is consistent
with a guest halted awaiting an interrupt. The diagnostic kick can wake the
vCPU before its owner samples state, so `run_state=Runnable` does not establish
that the guest was spinning during the timeout. Linux's
[native safe halt implementation](https://raw.githubusercontent.com/torvalds/linux/v6.6/arch/x86/include/asm/irqflags.h)
uses `sti; hlt`; that source is v6.6, while the measured kernel is 6.6.52.
A [symbol query from a successful boot of the same kernel](benchmarks/2026-09-30/boot-halt-symbol.json)
confirms `ffffffff81eda950 T default_idle` and `ffffffff81648100 t io_serial_out`.
The [query coordinator](benchmarks/2026-09-30/boot-halt-symbol-coordinator.py)
retains the exact command and hashes; its owned Firecracker VM was stopped,
and the response was complete with exit status zero and unchanged artifacts.
This is diagnostic evidence, not an additional performance sample. The missing
wakeup cause remains unconfirmed. No startup
fix or across-the-board feature/performance advantage has been established.


The [APIC diagnostic repeat](benchmarks/2026-09-30/local-engines-owner-apic-pinned-load-100.json)
retained the pinned CPU-0 workload with unchanged artifacts and successful final
cleanup. HyperMachine passed 92/100, with eight readiness failures at 2401 exits.
Seven samples stopped at `default_idle`'s post-HLT `cli`; one captured the next
instruction after `cli`, illustrating that diagnostic wakeup can advance state.
All eight had `LVT_TIMER=0x10000`, zero initial/current timer counts,
`LVT0=0x700`, `TPR=PPR=0x10` and empty ISR/IRR bitmaps. This points to the
legacy interrupt path at that boot stage, not an established root cause.
The [exact coordinator](benchmarks/2026-09-30/owner-apic-pinned-load-coordinator.py)
checksum matches the recorded workload; the worker remained alive and was reaped.

Firecracker passed 94/100 in this repeat. Its six connection-refused samples
contain only the startup banner in the bounded console output, consistent with
an initial API readiness race. These are failed harness attempts, not evidence
that Firecracker's guest has the same boot defect. The harness previously waited
only for socket-file existence; it now waits for a successful read-only
`GET /machine-config` before issuing configuration PUTs, retries only missing
socket/connection-refused errors within the original deadline and checks VMM
exit. It never replays a mutation. Ten harness regressions pass, including
readiness deadline/exit and non-retryable error cases. Both engines' failures
invalidate this cohort for a performance claim. Subsequent runs must record the
changed harness hash and extra readiness probe.


A real-KVM raw guest regression verified a separate timer-model gap: reading
port `0x61` returned unmapped-I/O `0xff` before the fix. The test stopped its VM
before asserting and failed on the reserved high bits. KVM PIT creation now
sets `KVM_PIT_SPEAKER_DUMMY` (value 1 in the installed Linux KVM UAPI), enabling
the channel-2 speaker-port stub used during timer calibration. The same real
guest regression then passed. Firecracker's
[versioned PIT setup](https://raw.githubusercontent.com/firecracker-microvm/firecracker/v1.17.0/src/vmm/src/arch/x86_64/vm.rs)
also enables this stub. This establishes corrected port emulation, not that
the cold-boot failures or latency gap are fixed. The matched repeat below
confirms that this port correction leaves the startup stall unresolved.


The [PIT-corrected daemon repeat](benchmarks/2026-09-30/local-engines-pit-speaker-pinned-load-100.json)
passed 93/100 HyperMachine attempts versus 100/100 Firecracker attempts. The
[coordinator](benchmarks/2026-09-30/pit-speaker-pinned-load-coordinator.py)
retained the CPU-0 contention profile; source/artifact checksums stayed
unchanged, the worker stayed alive and was reaped, and node cleanup succeeded.
All seven HyperMachine failures retained the 2401-exit, no-console,
`default_idle` stall and masked/unarmed LAPIC timer evidence. The PIT port
correction therefore does not resolve this startup defect. Differences between
seven, eight and nine failures across shared-host cohorts do not establish
a reliability improvement. Successful-sample P50/P99 was 1748.13/3366.14 ms
for HyperMachine versus 1245.19/3125.96 ms for Firecracker; the failed cohort
cannot support a performance win.

The corrected Firecracker readiness harness produced no connection-refused
samples in this repeat. Its additional read-only readiness probe changes the
control path and is included in measured time; raw reports retain the changed
harness checksum. This result supports these 100 attempts, not general
competitor reliability or a managed-platform comparison.


Failure reporting now prepares a machine-level PIC/PIT sample before kicking
the vCPU owner. These are independent KVM VM-level GET operations, not concurrent
vCPU register reads and not an atomic restore snapshot. It reports PIC request,
mask and in-service registers, and all three PIT channel counts/modes/gates and
load timestamps. Non-KVM, absent or malformed layouts are explicitly unavailable.
Four decoder tests pass, including missing-state handling, PIC bits and PIT's
valid 65536-count boundary. A real-KVM regression read the controller state while
a raw guest remained halted with interrupts disabled, then stopped it; both
that test and the existing halted/spinning owner regression passed. Strict
all-targets core/daemon Clippy passed on Windows. Live Linux-failure integration
captured controller and owner state in all three failures of the latest cohort.
Successful readiness does not request these samples.

The [pre-kick PIC/PIT cohort](benchmarks/2026-09-30/local-engines-pre-kick-pic-pit-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/pre-kick-pic-pit-pinned-load-coordinator.py)
record 97/100 HyperMachine passes versus 100/100 Firecracker passes. Both engines
have a 15-second guest-readiness budget; driver, engines and one contending worker
inherit CPU 0 affinity. Artifact hashes remained unchanged and all owned processes
were cleaned up. HyperMachine failed pairs 1, 5 and 97 with no console output and
2401 exits, unchanged over the 0.5-second observation window. Before the diagnostic
kick, each master PIC reported IRR=0x11, IMR=0 and ISR=0: timer and UART requests
were pending and unmasked. PIT channel 0 reported count 1193, mode 2 and gate 1.
After the kick, owner samples showed the same default_idle address with IF set,
LVT0=0x700 and empty LAPIC ISR/IRR arrays. Independent controller reads and the
later owner sample do not establish an atomic state or root cause. Interrupt
delivery remains under investigation. This failed cohort cannot support a
performance win; the lower failure count versus earlier shared-host runs does
not establish a reliability improvement.

The next diagnostic build also prints the existing owner-captured APIC base
MSR and CR8 alongside the LAPIC image. This exposes APIC enable/BSP bits and
CPU interrupt priority without additional register reads. The formatting check
preserves both values. The [APIC base cohort](benchmarks/2026-09-30/local-engines-apic-base-pinned-load-100.json)
and [coordinator](benchmarks/2026-09-30/apic-base-pinned-load-coordinator.py)
verify live capture in all three failed boots (pairs 20, 29 and 56), each with
APIC_BASE=0xfee00900 and CR8=1. The bootstrap APIC is enabled in these post-kick
samples; pre-kick PIC requests and the idle address match the previous failures.
Both engines retain 15-second guest-readiness budgets and CPU 0 contention.
All artifact hashes remained unchanged and owned-process cleanup succeeded.
HyperMachine passed 97/100 versus Firecracker 100/100, with slower successful-sample
P50 and P99. This cohort does not establish a startup fix or performance win.

A separate [read-only supported-CPUID probe](benchmarks/2026-09-30/supported-topology-cpu0-probe.py)
records the [CPU 0 KVM table](benchmarks/2026-09-30/supported-topology-cpu0.json).
Leaf 1 EBX is 2099200: its logical-processor count field is 32, while its initial
APIC ID is zero. The current backend only normalizes topology for multi-vCPU VMs,
leaving this supported table unchanged for a one-vCPU guest. This system ioctl
does not sample a running guest and does not establish the startup failure's
cause. It identifies a separate guest-topology consistency gap to address and
verify, including the AMD extended topology leaves.

Single-vCPU cold provisioning now normalizes leaf 1's processor count/APIC ID
and clears HTT; supported extended topology levels describe one processor with
zero shift, while terminators remain terminators. AMD size/core/node leaves and
cache-sharing fields agree with that topology; cache geometry remains intact.
The Linux singleton regression passed, and a real KVM guest executing CPUID
confirmed count 1, APIC ID 0 and HTT clear. Existing halted/spinning owner and
pre-kick machine-state regressions also passed (three KVM tests total).
The [rebuilt-daemon contention cohort](benchmarks/2026-09-30/local-engines-singleton-topology-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/singleton-topology-pinned-load-coordinator.py)
record 93/100 HyperMachine passes versus 100/100 Firecracker passes. Artifact
hashes remained unchanged, the CPU 0 worker stayed alive throughout, and cleanup
passed. Both engines retain 15-second readiness budgets. HyperMachine failed
pairs 31, 35, 48, 60, 61, 76 and 94 with no console output and 2401 exits,
unchanged over the observation window. Five post-kick owner samples retained
RIP=0xffffffff81eda95f with IF set; pairs 35 and 60 instead reported
0xffffffff81ed9b1d and 0xffffffff81ed9c50 with IF clear. These later samples can
reflect execution after the kick. Pending unmasked PIC requests remained present.
This verifies the topology change does not eliminate the startup defect;
the failed cohort supports no performance win or causal reliability comparison.
Topology field interpretation follows the [Linux x86 topology documentation](https://docs.kernel.org/arch/x86/topology.html).

Owner diagnostics now have a separate event observation alongside architecture,
without adding fields to persisted vCPU snapshots. KVM_GET_VCPU_EVENTS executes
on the vCPU owner between run calls and reports injected IRQ/vector, exception
state, NMI state and interrupt shadow. Shadow and NMI-pending fields require
their KVM validity flags; absent flags produce unavailable values. Unsupported
backends and read failures preserve architectural output without inventing zero
event state. The Linux validity regression and five daemon formatting tests
passed. All three real-KVM owner tests and strict all-targets Linux core Clippy
passed; live failed-boot integration remains unverified. These independent post-kick reads
cannot prove the exact pre-kick state or the startup failure's cause.

During the owner-event diagnostic cohort, a separate one-Hz, 60-second
[read-only thread probe](benchmarks/2026-09-30/owner-thread-state-probe.py)
recorded [owner CPU observations](benchmarks/2026-09-30/owner-thread-observations.json)
for benchmark daemon PID 39474. Thread 45765 remained observable from
03:38:19.386971 to 03:38:34.404970 UTC on October 1, gaining 575 user and 82 system
ticks; all 16 observations were runnable with wait channel zero. Its last sample
preceded the [failed-boot report](benchmarks/2026-09-30/owner-events-thread-failure-excerpts.txt)
at 03:38:34.426989 UTC by 22 ms. The sequential one-VM cohort and timestamps
support correlation, not a direct thread-to-sandbox identity mapping. CPU
accounting alone cannot separate guest execution from host retry-loop work;
KVM_RUN EINTR/EAGAIN counters are the next check. A preceding 15-second probe
also ran during this cohort. These observer workloads and the shared host limit
performance interpretation; the samples are diagnostic evidence, not a win.

The [completed owner-event cohort](benchmarks/2026-09-30/local-engines-owner-events-pinned-load-100.json)
and [coordinator](benchmarks/2026-09-30/owner-events-pinned-load-coordinator.py)
record 82/100 HyperMachine passes versus Firecracker 100/100. Artifacts remained
unchanged and owned-process cleanup passed. All 18 post-kick failed-boot event
samples report flags 0xd, IRQ_INJECTED=0, valid SHADOW=0 and valid NMI_PENDING=0.
The vector field is not evidence of a pending IRQ when injection is zero.
The raw EXCEPTION_PENDING=0 values are unavailable: flags lack
KVM_VCPUEVENT_VALID_PAYLOAD (0x10), which the current formatter neglected to
check. This reporting error requires correction; these values cannot support
exception-state conclusions. The [KVM API documentation](https://docs.kernel.org/virt/kvm/api.html)
specifies that validity requirement. The failed shared-host cohort, including
the separately recorded thread probes, supports no performance or causal
reliability win. Startup reliability remains unresolved.

The current diagnostic code corrects EXCEPTION_PENDING to an optional field
guarded by KVM_VCPUEVENT_VALID_PAYLOAD. The Linux validity regression passed.
KVM_RUN now counts EINTR and EAGAIN retries separately from guest exits; counters
advance only on those retry paths and are sampled on the owner. Totals include
the diagnostic kick itself, so a small EINTR count is expected and does not
establish a retry storm. The real-KVM halted/spinning regression verifies
cumulative counts and interruption while both guests remain stoppable; all
three owner tests passed. Strict Windows core/daemon and Linux core Clippy
passed, as did all five final daemon formatter tests. The release rebuild passed.

The [completed retry-counter cohort](benchmarks/2026-09-30/local-engines-run-retries-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/run-retries-pinned-load-coordinator.py)
record 91/100 HyperMachine passes versus Firecracker 100/100. Artifact hashes
remained unchanged, the CPU 0 worker stayed alive throughout, and owned-process
cleanup passed. Both engines retained 15-second readiness deadlines. Failed
pairs 22, 50, 52, 53, 54, 58, 80, 89 and 93 all report EINTR=1 and EAGAIN=0,
including the diagnostic kick. These observations provide no evidence of a
host retry storm in the failed boots. All nine retain pending unmasked PIC
requests, no console output and 2401 guest exits. Eight post-kick owner samples
report RIP=0xffffffff81eda95f with IF set; pair 53 instead reports
0xffffffff8105fb53 with IF clear. The independently sampled post-kick events
again show no injected IRQ, valid zero shadow and valid zero NMI pending;
exception pending is correctly unavailable. The next controlled experiment
tests an MP table for singleton cold boots: the loader in this cohort omitted it
for one vCPU, whereas [Firecracker 1.17.0 installs it for all configured CPU counts](https://github.com/firecracker-microvm/firecracker/blob/v1.17.0/src/vmm/src/arch/x86_64/mod.rs).
This is a hypothesis, not an established root cause. The failed shared-host
cohort supports no performance or causal reliability win. No startup fix is
established.

The singleton cold-boot MP-table experiment is now implemented. The real-KVM
Linux 6.6.52 regression passed: the guest discovers the table, reports one
processor and an I/O APIC, activates its UART console and reaches userspace
handoff. The regression now accepts the older `console [ttyS0] enabled` message
as well as the newer `legacy console` wording. Strict all-targets Linux core
Clippy and the release rebuild passed. Live checkpoint/pause/fork validation
passed, as did the first matched contention cohort. These results establish guest
topology discovery on this kernel; they do not establish a startup-reliability
or performance improvement.

The [live snapshot-state report](benchmarks/2026-09-30/singleton-mptable-state.json)
and [exact probe](benchmarks/2026-09-30/singleton-mptable-state-probe.py)
exercise the rebuilt daemon with a newly created snapshot-backed base template
at one vCPU and 1024 MiB. Guest kernel messages confirm the MP table and I/O
APIC. Checkpoint restore rolls a file back from `after` to `before`;
pause/resume preserves that value and command readiness; two forked guests
both return `before`. Guest interrupt tables retain I/O APIC routes before and
after restoration, with local-timer and virtio counts advancing. Artifact
hashes remained unchanged and the isolated node's sandbox records and process
were cleaned up. The first probe incorrectly used cold-only mode, which
rejects checkpoints, and the second supplied an empty JSON pause body; those
setup errors were corrected before this passing run. This validates snapshots
created by this build, not persisted snapshots from an older build, density,
fleet scale or performance superiority.

The [singleton MP-table contention cohort](benchmarks/2026-09-30/local-engines-singleton-mptable-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/singleton-mptable-pinned-load-coordinator.py)
record 100/100 passes for each engine, with identical kernel/initrd bytes,
one vCPU, 1024 MiB and 15-second readiness budgets. This is the first fully
passing pinned contention cohort in this investigation. Artifact hashes
remained unchanged; the CPU 0 worker stayed alive and was reaped; engine and
daemon cleanup passed. No startup failure was observed in these 100 attempts.
The preceding retry-counter cohort recorded HyperMachine 91/100, but separate
runs on shared hardware do not isolate a causal reliability or timing effect.
HyperMachine remains slower than Firecracker in the passing cohort: successful
readiness P50/P99 were 1107.25/2263.95 ms versus 804.85/1740.15 ms. Zero observed
failures in these cohorts does not establish that startup reliability is fully resolved, and no
performance or across-the-board product win is established.

The [200-pair pinned repeat](benchmarks/2026-09-30/local-engines-singleton-mptable-pinned-load-200.json)
and [exact coordinator](benchmarks/2026-09-30/singleton-mptable-pinned-load-repeat-coordinator.py)
also passed every attempt for both engines. Both MP-table cohorts used the same
daemon binary, kernel, initrd and harness hashes. Artifact stability, worker
liveness and cleanup passed. HyperMachine now records 300/300 passing attempts
across these two pinned cohorts; Firecracker also records 300/300. The repeat
still shows a latency gap: P50/P99 1079.50/1867.38 ms versus 769.48/1573.45 ms.

A separate [five-pair stage diagnostic](benchmarks/2026-09-30/local-engines-stage-timing-pinned-diagnostic-5.json)
and [coordinator](benchmarks/2026-09-30/stage-timing-pinned-diagnostic-coordinator.py)
enable `hv2_sandboxd=debug` to capture existing create-stage fields. All ten
engine attempts, artifact checks, worker liveness and cleanup passed. The five
HyperMachine log entries report construction of 0.38–1.49 ms, launch of
47.47–229.38 ms, launch-to-agent-answer of 1195.70–1769.67 ms, and final setup
of 0.066–0.079 ms. Launch-to-answer includes guest boot, connection setup and
ping; it is not a measurement of transport alone. Its dominant elapsed time
narrows the next investigation to guest boot and readiness rather than the
post-readiness control path. Extra logging and shared-host load make this a
diagnostic, not a performance comparison or evidence of a win.

The [three-pair guest boot diagnostic](benchmarks/2026-09-30/guest-boot-stages-diagnostic-3.json)
and [exact coordinator](benchmarks/2026-09-30/guest-boot-stages-diagnostic-coordinator.py)
collect kernel logs, uptime and interrupt tables after a successful command
from each guest. All six guests, artifact checks, worker liveness and cleanup
passed. The extra guest commands are included in the reported readiness
duration, so those durations must not be used as benchmark results.
HyperMachine's three UART-discovery intervals were 261.407, 268.393 and
262.496 ms; Firecracker's were 0.266, 4.921 and 4.798 ms. HyperMachine reports
a legacy UART on IRQ 4; Firecracker reports an ACPI UART on IRQ 27. The logs
narrow the observed kernel-handoff gap to UART discovery, without establishing
the complete host-side latency cause.

The repository guest configuration enables `CONFIG_SERIAL_8250_DETECT_IRQ`.
[Linux's x86 UART definitions](https://github.com/torvalds/linux/blob/v6.6/arch/x86/include/asm/serial.h)
set `UPF_AUTO_IRQ` when that option is enabled and otherwise retain the standard
COM1 IRQ 4. The [8250 IRQ probe](https://github.com/torvalds/linux/blob/v6.6/drivers/tty/serial/8250/8250_port.c)
calls `probe_irq_on()` twice; the [IRQ probing implementation](https://github.com/torvalds/linux/blob/v6.6/kernel/irq/autoprobe.c)
waits 20 and 100 ms on each call. This supports a fixed-IRQ configuration
experiment for the known virtual board. The guest defconfig now disables IRQ
autodetection while retaining UART and console support; a checksum-verified
Linux 6.6.52 rebuild passed. The original benchmark kernel is preserved.
The effective build configuration was observed with UART and console support
enabled and IRQ autodetection disabled. The real-KVM Linux boot regression
passed with the new image. The [new-kernel snapshot report](benchmarks/2026-09-30/known-uart-irq-state.json)
and [exact probe](benchmarks/2026-09-30/known-uart-irq-state-probe.py)
also passed checkpoint rollback, pause/resume and two forked guests, retaining
I/O APIC UART IRQ 4 and command readiness. Artifact checks and cleanup passed.
The rebuilt kernel SHA-256 is
`afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd`.
A 50-block comparison completed: each block tests both kernels in alternating
order, with two alternating engine pairs per kernel and a fresh isolated
HyperMachine daemon per two-pair run. Both engines use the same kernel within
each run. One pinned CPU worker remains active across all blocks. This design
retains every attempt and reduces time/order bias; it differs from the longer
single-node cohorts above.

The [complete counterbalanced comparison](benchmarks/2026-09-30/uart-irq-kernel-blocks-50.json),
[summary](benchmarks/2026-09-30/uart-irq-kernel-blocks-50-summary.json),
[exact coordinator](benchmarks/2026-09-30/uart-irq-kernel-blocks-coordinator.py)
and [analysis](benchmarks/2026-09-30/uart-irq-kernel-blocks-analysis.py)
record 400/400 successful, cleaned-up attempts: 100 for each engine/kernel
combination. Validation checks all block orders, resources, readiness budgets,
affinity, source/artifact hashes and per-run failure accounting. The worker
stayed alive throughout and was reaped. HyperMachine's original-kernel P50/P99
were 1119.17/2645.60 ms; with the fixed-IRQ kernel they were 859.85/1922.86 ms.
The rebuilt kernel was faster for HyperMachine in 47/50 paired blocks;
the median reduction in each block's mean readiness was 266.46 ms.
Firecracker's corresponding P50/P99 were 787.76/1956.21 and 773.83/1375.09 ms;
its paired-block median reduction was 4.11 ms (27/50 blocks faster).
This records a HyperMachine cold-start improvement on this workload.
HyperMachine still trails Firecracker on both percentiles with the same new
kernel, and shared hardware limits extrapolation, especially for tails.

The [fixed-IRQ guest-log diagnostic](benchmarks/2026-09-30/guest-boot-stages-fixed-irq-diagnostic-3.json)
and [exact coordinator](benchmarks/2026-09-30/guest-boot-stages-fixed-irq-diagnostic-coordinator.py)
also passed all six guest captures, provenance and cleanup checks. The three
HyperMachine UART-discovery intervals were 3.004, 3.770 and 12.542 ms, versus
261.407–268.393 ms with the original kernel. UART IRQ 4 and console support
remain present. Firecracker's intervals were 2.384, 4.436 and 2.635 ms. Other
guest boot stages varied substantially across diagnostic runs; their full
handoff times do not isolate the configuration effect. These extra-command
diagnostics are excluded from the benchmark table. Product feature gaps and
broader performance validation remain open; no across-the-board win is claimed.
