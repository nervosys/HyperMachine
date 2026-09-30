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
