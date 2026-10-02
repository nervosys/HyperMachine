# HyperMachine against boxd and exe.dev

The goal: every capability [boxd](https://boxd.sh/) and [exe.dev](https://exe.dev/) offer, and more.
This page tracks it. Their feature lists were reviewed against their public docs on 2026-10-01
(`docs.boxd.sh/llms-full.txt`, `exe.dev/docs/all`). HyperMachine's statuses come from its code,
not its docs. **Real** means wired to a shipped binary and checked. **Partial** says what is missing.

All three products give each user a real Linux VM behind a hardware boundary, rather than a
container. The comparison is about what surrounds the VM.

## The matrix

| Capability | boxd | exe.dev | HyperMachine |
|---|---|---|---|
| Create, list, delete over an API | yes | yes | **Real**: E2B's API (`hv2-sandboxd`, `hv2-control-plane`), so the unmodified E2B SDKs work |
| A CLI | yes | yes (over ssh) | **Yes**: `hm sandbox vm` manages lifecycle, commands, binary files and checkpoints; verified on a real KVM node and authenticated control plane/proxy |
| Fork a running VM, memory included | yes; ~160 ms provider example | `cp` exists; live-memory copying not established | **Real**: `POST /sandboxes/{id}/fork`, 1-100 copies |
| Named snapshots, and new VMs from them | yes | not documented | **Real**: snapshots become templates |
| Checkpoint and restore in place | yes, 10 per VM | not documented | **Real**: 10 per sandbox; memory and disk; same ID, token and URL; a failed restore changes nothing |
| Pause and resume | yes | not documented | **Real**: to disk; any node resumes |
| Suspend when idle, wake on traffic | yes | not documented | **Real**: `idleTimeout` or `--idle-pause-after`, plus `autoResume`. Idle means no traffic *and* a quiet guest CPU, so unwatched work is never frozen |
| HTTPS URL per VM | yes | yes | **Partial**: `{port}-{id}.{domain}` over TLS; you bring the wildcard certificate (no ACME) |
| Per-port URLs, raw TCP/UDP | yes | ports 3000-9999 | **Partial**: every port over HTTP(S); authenticated raw TCP through the node/control-plane API and loopback CLI, [verified with KVM/TLS and lifecycle operations](benchmarks/2026-10-01/tcp-tunnel.md); UDP absent |
| Custom domains | yes | yes | **Real**: authenticated cluster bindings to guest HTTP ports, Memory/Redis ownership, HTTPS forwarding; [operator DNS and certificates](CUSTOM_DOMAINS.md) |
| DNS validation and automatic domain TLS | yes | not checked | **Absent**: operators provide DNS and certificates |
| Private URLs with login, identity headers | public web URL; team shell sharing | yes (`X-ExeDev-Email`) | **Absent** |
| SSH to a VM by name | yes | yes | **Partial**: [persisted metadata names](benchmarks/2026-10-01/ssh-name.md) and authenticated stdio transport verified with real KVM/TLS, binary transfer and duplicate/key rejection; [reserved aliases for existing VMs](benchmarks/2026-10-02/reserved-alias/README.md) verified with deletion/reuse and fork ownership; [control-plane named creation](benchmarks/2026-10-02/reserved-create/README.md) is KVM/TLS verified, including deletion/reuse; [forked children omit the parent name](benchmarks/2026-10-02/fork-name/README.md), preserving parent lookup; [node-side atomic completion and discarded-descriptor recovery](benchmarks/2026-10-02/node-completion/README.md) are KVM-verified; [committed-name recovery after real node response loss and timeout](benchmarks/2026-10-02/node-response-loss/README.md) passes 24 KVM/TLS checks; [guest preservation after post-commit event failure](benchmarks/2026-10-02/node-publication-fault/README.md) is KVM-verified; [registration ACL partial-write prevention](benchmarks/2026-10-02/registration-acl/README.md) is verified against Redis and [real guest registration-write failure preservation](benchmarks/2026-10-02/node-registration-fault/README.md) passes 26 combined KVM/TLS checks; [authenticated node reconciliation of the same preserved guest](benchmarks/2026-10-02/registration-reconcile/README.md) is KVM-verified; [uncertain-registration lifecycle guards](benchmarks/2026-10-02/registration-lifecycle/README.md) are KVM-verified across the idle deadline, including [connect and checkpoint mutation protection](benchmarks/2026-10-02/registration-mutations/README.md); fleet migration, in-flight recovery and guest SSH provisioning remain incomplete |
| exec, and file copy in and out | yes | ssh/scp | **Real**: `/exec`, envd processes with PTY and stdin, files |
| **Env vars for every command in a VM** | org-wide | creation env supported; command inheritance not checked | **Real**: E2B's `envVars`, kept in the guest so pause, fork and snapshots carry them |
| Secrets held off the VM, injected at the edge | platform-held integration credentials; header-injection parity not established | yes | **Real**: header injection at the egress gateway, which the guest never sees |
| Workload identity (AWS/GCP federation) | no | yes | **Real**: JWT-SVIDs minted at the gateway, JWKS and OIDC discovery |
| Egress policy per VM | egress allowlist documented; enforcement details not checked | no | **Real**: allow/deny lists, live updates, decision log, reserved ranges refused |
| VM-to-VM networks by tag | yes | via proxy | **Absent** |
| Teams, roles, sharing | yes | yes, with SSO | **Partial**: one team, with operator/observer key roles; no tenant isolation or sharing |
| Scoped, expiring API keys | yes | yes | **Real on the control plane**: hashed operator-provisioned keys, request-time expiry and capability scopes; single team; [atomic policy replacement and Unix signal reload](API_KEY_ROTATION.md) are verified through real HTTP and a running process |
| Persistent volumes shared between VMs | no | no | **Real**: E2B volumes over 9P, live and shared (Linux hosts) |
| Build images from Dockerfiles or OCI | compose | Dockerfile | **Real**: E2B template builds, no Docker |
| Backups to object storage | yes | no | **Absent** |
| Scheduled jobs and event triggers | `*.run.ts` | no | **Partial**: lifecycle webhooks, [durable delayed host-process jobs and interval publication](JOBS.md), and [explicit VM dispatch verified with KVM/TLS](benchmarks/2026-10-01/scheduled-dispatch.md); an [automatic VM worker is verified with KVM/TLS](benchmarks/2026-10-01/scheduled-worker.md), with operator-recorded completion recovery; [calendar catch-up execution is KVM/TLS-verified](benchmarks/2026-10-01/scheduled-calendar.md), with [bounded batch publication and restart verified on KVM](benchmarks/2026-10-01/calendar-publication-kvm.md) and [matched local publication timings](benchmarks/2026-10-01/calendar-publication.md); live DST scheduling, guest execution throughput and automatic guest reconciliation remain incomplete |
| Desktop in a browser, browser for agents | yes | web terminal | **Absent** |
| MCP for agents | skill + MCP | remote MCP with browser login; Shelley agent | **Partial**: 12 lifecycle/exec/checkpoint tools plus 2 opt-in binary file tools over MCP stdio, with cancellable client waits checked on real KVM; accepted remote work can continue, and remote authenticated MCP, streaming plus the wider `hv2-agent` surface remain absent |
| Email in and out | no | yes | **Absent** |
| Multi-node, self-hosted | contact sales | enterprise | **Real**: control plane, Redis store, cross-node resume, mTLS, Helm chart |
| GPU | no | no | **Partial**: VFIO code, not wired to sandboxes |
| Hypervisor you can read | no | no | **Real**: our own VMM; KVM, WHPX, HVF; a Type-1 path |

Custom domains now have authenticated management endpoints, proxy routing and
CLI bind/list/unbind commands. Memory/Redis claims enforce sandbox ownership
and deletion cleanup. A real KVM guest served HTTPS content through both
proxies, followed a guest-port update, retained its Redis binding across a
control-plane restart, resumed on alias access with `autoResume` enabled, and
released its hostname after deletion. HTTP/1.1 and HTTP/2 fixture tests preserve
the public host, path, query and body. See [setup and evidence](CUSTOM_DOMAINS.md).
Operators still provide DNS and certificates; ACME and DNS ownership verification
are not implemented. These functional passes establish no performance win.

The [Boxd documentation](https://docs.boxd.sh/llms-full.txt) distinguishes public
web access from team shell sharing, describes platform-held integration credentials,
automatic domain TLS, and forked egress allowlists. The [exe.dev documentation](https://exe.dev/docs/all)
describes remote MCP and VM copying, but does not establish live-memory copying.
These are documentation findings, not independent runtime tests. “Not documented”
and “not checked” do not establish that a competitor lacks a capability.

## HyperMachine capabilities to compare

Cold boot now has an [optional per-node admission budget](COLD_START_ADMISSION.md).
At 100 concurrent requests on the eight-CPU native fixture, the final binary's
eight-slot budget reduced P50 readiness from 6702 to 3639 ms; P99 increased
from 7898 to 8434 ms. All 800 attempts passed, and paired mean readiness improved
in four of four pairs. [Raw comparisons and controls](benchmarks/2026-10-02/cold-start-admission/README.md)
retain earlier failures and the slower default-disabled C8 control. This is an
opt-in tradeoff, with no managed competitor performance claim.

A subsequent [matched native comparison](benchmarks/2026-10-02/cold-budget-tuning/README.md)
passed 400/400 attempts per engine at C100. HyperMachine with eight slots had
lower P50 readiness (3530 versus Firecracker's 5678 ms) and lower paired means
in four of four pairs, but worse P99 (12857 versus 9168 ms) and higher held PSS
(8617.59 versus 8360.78 MiB). A four-slot experiment also worsened the tail.
The settings remain optional; these local results establish no managed-service
or across-the-board win.

[Two direct eight/sixteen-slot repeats](benchmarks/2026-10-02/cold-budget-comparison/README.md)
passed all 1600 attempts. Sixteen improved paired P99 in six of eight pairs but
paired means in only four. A matched sixteen-slot native comparison passed 400/400
attempts per engine: HyperMachine had lower P50 (3807 versus 5579 ms) and worse
P99 (11741 versus 11042 ms) and held PSS (8658.38 versus 8361.85 MiB). These
tradeoffs leave tail latency and held memory as performance gaps.

A subsequent [owned heap-reclamation diagnostic](benchmarks/2026-10-02/heap-reclaim/README.md)
passed all 432 guest attempts and all post-probe guest checks. At C100, two pairs
observed approximately 100–112 MiB reclaimable while guests remained running,
after no-op adjustment. This identifies a memory optimization candidate; the
diagnostic helper changes the fixture, and concurrent latency, lifecycle and
competitor comparisons remain untested. No heap policy or default was adopted.

An isolated [250 ms periodic-reclamation candidate](benchmarks/2026-10-02/periodic-heap/README.md)
passed all 800 C100 attempts and reduced held PSS in four of four pairs, but
increased pooled P50 by 23.9% and P99 by 103.6%; only one paired mean improved.
That tested policy was rejected and is not part of the deployed daemon. The
memory gap remains; reclaimability alone does not establish a performance win.

A [reclamation candidate guarded by the entire cold-start budget](benchmarks/2026-10-02/guarded-heap/README.md)
verified seven reclamation calls with no admitted-cold overlap, failure recovery
and snapshot resume. Its C100 cohort retained 725/800 passes and 75 disabled-worker
creation failures. Only one of three complete pairs improved mean readiness and
a different one improved held memory; none improved both. The guarded policy was
not adopted, and the pass-count difference establishes no reliability fix.

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
| Firecracker | Specification targets at most 125 ms from InstanceStart to init and at most 5 MiB VMM overhead for its specified minimal guest | Engine isolation and efficiency | Matched native cold bursts at concurrency 1/8/50/100 passed 1800/1800 attempts per engine; Firecracker was faster in every profile. At concurrency 100, HyperMachine P99 was 10.71 s versus Firecracker 7.18 s. Earlier failures remain recorded; bounded passes do not establish universal reliability. [Current-daemon refresh](benchmarks/2026-10-02/current-native-engines/README.md): HyperMachine 218/318 attempts passed versus Firecracker 318/318, with a failed HyperMachine 100-guest batch; Firecracker has lower successful P50 in all profiles. Specification metrics differ from application readiness and PSS |

These figures describe the linked providers' own claims or repository runs,
not an independently reproduced ranking. A lower headline number does not
pass the acceptance criteria below.

| Workstream | Acceptance criterion | Current gap |
|---|---|---|
| Creation and execution latency | Same guest workload and readiness command; raw samples, failure rate, P50/P95/P99 at concurrency 1, 8, 50 and 100; lower latency than each tested competitor with repeatable results | Native Firecracker sweep verified at all four concurrencies; HyperMachine still trails. Managed competitor runs and bare-metal HyperMachine runs are missing |
| Stateful resume, pause and fork | Verify live process memory and filesystem state, then measure API-to-first-successful-command latency under the same concurrency | Paired synchronized SDK sweep at concurrency 1/8/50/100: event-preserving KVM runtime passed 1008/1008, parent 1007/1008. Separate synchronized pause/resume bursts passed 503/504 with one resume timeout retained. Pending-event restoration is verified by a direct guest-handler control. A reliability SLA and matched competitor runs remain missing |
| Memory and density | Incremental PSS after the same command and idle period; same guest resources; document shared-template memory; preserve state through oversubscription | Fixed five-second idle PSS and per-batch empty-node baselines verified at concurrency 1/8/50/100, 477/477 attempts per engine; HyperMachine held PSS was higher in all 12 paired batches. Retained-memory allocation source, shared-template attribution and stateful density remain unverified. Vendor VMM overhead and PSS are different quantities |
| Throughput and tails | Sustained arrivals on identical host resources; include failures, queueing, and recovery rather than counting accepted requests | Matched short fixed-rate schedules at 5/25 arrivals per second passed 560/560, including client queue delay and full cleanup/drain. HyperMachine trails Firecracker in both profiles. Longer arrivals, overload failures, load-change recovery and equivalent fleet scale remain unverified |
| CLI and SDK usability | Shipped client for lifecycle, execution, files and checkpoints, tested against a real node and control plane | VM CLI verified on a real KVM node and authenticated control plane, including binary files through the control-plane proxy |
| Isolation and governance | Enforced tenant boundaries, scoped expiring keys, roles, auditable access, and escape tests | Scoped expiring keys, [single-team observer/operator roles](API_KEY_ROTATION.md#single-team-observer-and-operator-roles), and [durable protected-API admission/completion records](ACCESS_AUDIT.md) are verified, including restart, storage failure and KVM/TLS lifecycle requests. Tenant boundaries, team membership/sharing, retention automation and guest/proxy activity auditing remain incomplete; [opt-in keyed sandbox target references](ACCESS_AUDIT.md) are verified on protected control-plane routes, while creation-result, template/volume and proxy attribution remain incomplete |
| Networking and access | Custom domains, certificate automation, authenticated private URLs, SSH, raw TCP/UDP and isolated VM groups | Custom-domain HTTP/HTTPS routing, authenticated raw TCP and egress are implemented. [The API socket fix](benchmarks/2026-10-01/tcp-api-buffering.md) reduced observed 1 MiB native TCP median latency from 48.5–50.8 ms to 7.0–7.9 ms in bracketed runs (1600/1600 transfers verified). That sequential comparison did not establish a median win over Firecracker; large tails and TLS performance remain unresolved. [Eight-stream controls](benchmarks/2026-10-01/tcp-concurrent.md) corrected the shared guest-agent backlog and verified 1920/1920 transfers on both native paths; observed median/tail rankings vary by profile. A [Unix relay prototype](benchmarks/2026-10-01/tcp-unix-relay.md) passed 5440 transfers but did not establish a repeatable gain and was reverted. Certificate automation, private browser URLs, UDP and isolated groups remain absent; [control-plane named creation](benchmarks/2026-10-02/reserved-create/README.md) is KVM/TLS verified, [fork name inheritance is fixed and KVM-verified](benchmarks/2026-10-02/fork-name/README.md), [node-side atomic completion](benchmarks/2026-10-02/node-completion/README.md) is verified, as is [committed-name recovery after response loss and timeout](benchmarks/2026-10-02/node-response-loss/README.md), while fleet name migration and in-flight recovery remain incomplete; [reserved aliases for existing VMs](benchmarks/2026-10-02/reserved-alias/README.md) pass real KVM/TLS verification |
| Platforms and workloads | Verified ARM64 execution, GPU sandboxes, browser/desktop workloads, and persistent storage limits | ARM64 execution and GPU sandbox wiring remain unverified or absent |
| Operations | Object-storage backups and recovery, quota enforcement, scheduling/event triggers, load-tested multi-node failover | Shared-directory snapshots and host job queues do not cover all these capabilities |

Mark a capability complete only after checking its effect through a shipped
interface. Mark a performance win only after equivalent runs establish it;
do not infer it from a vendor headline or a component microbenchmark.

The [fixed-idle memory comparison](benchmarks/2026-10-01/idle-memory.md) records
held totals, same-batch increments, individual idle ages and post-cleanup
baselines. At concurrency 100, median held PSS was 8503.60 MiB for HyperMachine
versus 8360.42 MiB for Firecracker; median same-batch increments were 8320.55
versus 8360.42 MiB. The difference reflects a daemon baseline that grew after
earlier batches. Three alternating pairs and shared nested hardware do not
establish a repeatable memory or density win.

Follow-up [mapping diagnostics](benchmarks/2026-10-01/memory-retention.md)
verified that KVM VM/vCPU handles and mappings of at least 1 GiB disappeared
after each eight-guest batch, while smaller anonymous mappings remained
resident. A separate arena-limited diagnostic reduced median post-cleanup
process PSS from 193.52 to 110.02 MiB across three batches. All 48 guest attempts
passed, but the runs were sequential and had no matched latency/tail comparison.
This suggested an allocator candidate; the controlled evaluation below did not
support adoption. Production defaults remain unchanged.

That candidate was then [counterbalanced at concurrency 100](benchmarks/2026-10-01/allocator-experiment.md)
across four blocks and 3200 attempts. HyperMachine's arena-limited process PSS
was higher in all three complete paired blocks, and its mean readiness was
lower in only one. Both engines had readiness failures (3029/3200 attempts
passed overall), with clean teardown and unchanged artifacts. The arena limit
was not adopted; neither a memory win nor a reliability fix is established.

A separate [boot-sizing allocation candidate](benchmarks/2026-10-01/boot-sizing.md)
removed temporary kernel/initrd copies during Linux memory sizing. Allocation
and guest-state checks passed, followed by 3328/3328 passing, cleaned-up native
attempts against a matched current-parent build. At concurrency 8 both paired
blocks improved readiness and memory. At concurrency 100 the candidate was
slower in three of four blocks, with a median paired mean-readiness increase
of 1455.22 ms and inconsistent memory changes. The runtime change was reverted;
exact candidate sources, tests, raw reports and comparison tables are retained.
This micro-level allocation reduction did not establish a product improvement.

A [refused-agent connection backoff candidate](benchmarks/2026-10-01/refusal-retry.md)
was evaluated against a current-source release baseline with synchronized
command rounds at concurrency 1/8/50/100, two alternating pairs per profile.
All 15600 commands and 636 guest preparations passed with clean teardown,
but candidate mean latency was higher in seven of eight pairs. The change was
reverted. Exact candidate source/tests, build hashes, raw cohorts and the
initial permission-related setup failures are retained; no command attempts
were made in those setup failures. Production keeps the prior retry cadence.

The [synchronized stateful SDK sweep](benchmarks/2026-10-01/stateful-sweep.md)
now prepares whole resume/fork batches before timing them, with observed client
start spreads. Resume passed 504/504; fork passed 503/504 at concurrency
1/8/50/100. Every successful sample checks retained live-process memory and
filesystem state; forks also check an independent child write and unchanged
parent state. At concurrency 100, conditional P99 readiness was 1089.49 ms for
resume and 1064.27 ms for fork. One concurrency-1 fork missed the unchanged
15-second child-agent readiness deadline; its nonzero cohort and interrupt
diagnostics remain recorded. Teardown and artifact checks passed throughout.
The initial 636/636 passing sweep is retained too. This expands verification;
it establishes neither universal reliability nor a matched competitor win.

New KVM snapshots now [preserve vCPU event handoffs](benchmarks/2026-10-01/kvm-events.md)
alongside LAPIC state. A direct real-KVM control verifies the restored interrupt
enters a guest handler, while the legacy omission takes the main path; NMI,
shadow and exception state also round-trip. Real checkpoint/resume/fork checks
passed. The paired SDK sweep retained all 2016 attempts: updated runtime
1008/1008, parent 1007/1008, with the parent's resume timeout kept intact.
Timings are mixed, including a higher updated resume P99 at concurrency 100.
This fixes the demonstrated event-state omission, without establishing the
cause of earlier timeouts, a speed win or universal reliability. Old snapshots
remain readable but require recapture to include event state never stored.

The separate [synchronized pause/resume sweep](benchmarks/2026-10-01/pause-bursts.md)
prepares all live-state probes before concurrently pausing each batch, verifies
paused state, then synchronizes resume independently. It passed 503/504 across
concurrency 1/8/50/100. At concurrency 100, conditional pause P99 was 961.88 ms
and resume-to-command P99 was 1095.98 ms. One concurrency-1 resume timed out
despite event preservation; its raw diagnostics and nonzero cohort are retained.
This closes the pause-burst measurement gap without proving a reliability fix
or a comparative performance win.

A [timer-only hardware control](benchmarks/2026-10-01/deadline-timer.md) restores
a halted real-mode guest with a captured TSC deadline and verifies entry into
its interrupt handler. Omitting the deadline leaves the control halted until
the bounded test kicks and joins its runner. Both controls passed, narrowing
the resume investigation without proving the cause of the retained timeout.
The extended control also measures that guest TSC has passed the captured
deadline before first entry, then still reaches the handler. This verifies
that specific delayed-start wakeup window without explaining the SDK failure.

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

The [fixed-rate native comparison](benchmarks/2026-10-01/fixed-arrivals.md)
adds planned arrival times, submission lag, client worker queues and complete
lifecycle cleanup/drain to the engine harness. Two alternating pairs at
5 and 25 offered arrivals/second passed all 560 attempts. At the higher rate,
scheduled command P99 was 2683.92 ms for HyperMachine versus 2034.47 ms for
Firecracker; both accumulated queues with eight workers. Cleanup occupies
workers, so these short runs do not measure server-only maximum capacity or
prove sustained fleet throughput. Longer overload and recovery work remains.

Build the client with `cargo build -p hm-cli --bin hm`. It talks to sandboxd
or the control plane, selected by `--endpoint` or `HV2_SANDBOX_URL`.
`HV2_API_KEY` supplies authentication without putting the key in shell arguments.
Use TLS when connecting to a remote endpoint.

```sh
export HV2_SANDBOX_URL=http://127.0.0.1:3980
hm sandbox vm create --template base
hm sandbox vm list
hm sandbox vm exec SANDBOX_ID -- /bin/sh -c 'printf hello'
hm sandbox vm --endpoint https://sandbox-api.example.com tcp SANDBOX_ID --port 5432 --listen 127.0.0.1:15432
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
provenance are checked by `tools/test-bench-e2b-sdk.py` (19 tests). The CI
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
immediate bearer revocation and tenant roles remain open. Single-team observer/operator roles are described in [key policy configuration](API_KEY_ROTATION.md#single-team-observer-and-operator-roles); durable protected-API records are described in [access audit configuration](ACCESS_AUDIT.md).

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


## Concurrent native cold-start comparison (2026-09-30)

`tools/bench-local-engines-concurrent.py` compares barrier-released batches
on the same shared WSL nested-KVM host. The driver and both engines inherit
CPU affinity 0–7; one controlled worker runs on CPU 0. Each guest has 1 vCPU
and 1024 MiB, boots the same fixed-UART Linux kernel and guest-agent initrd,
and must return an exact unique shell marker. Resource declarations are
checked afterward. Engine batch order alternates AB/BA, and every guest
remains held until all attempts finish validation and aggregate process
PSS is read. Cleanup is then released; known guests/processes are deleted
or stopped, the node list is checked empty, and the owned daemon and worker
are reaped. Both guest-readiness budgets remain 15 seconds.

| Concurrent guests | Engine | Passing attempts | Readiness P50 | P95 | P99 | Median burst ready rate |
|---:|---|---:|---:|---:|---:|---:|
| 1 | HyperMachine | 100/100 | 398.10 ms | 440.86 ms | 457.89 ms | 2.48/s |
| 1 | Firecracker 1.17.0 | 100/100 | 371.15 ms | 400.68 ms | 415.58 ms | 2.67/s |
| 8 | HyperMachine | 200/200 | 514.61 ms | 663.18 ms | 690.03 ms | 12.22/s |
| 8 | Firecracker 1.17.0 | 200/200 | 427.53 ms | 573.48 ms | 606.44 ms | 13.96/s |
| 50 | HyperMachine | 500/500 | 3219.51 ms | 4591.53 ms | 4892.71 ms | 14.86/s |
| 50 | Firecracker 1.17.0 | 500/500 | 2560.56 ms | 2746.57 ms | 2778.06 ms | 18.23/s |
| 100 | HyperMachine | 1000/1000 | 6466.88 ms | 10046.07 ms | 10709.99 ms | 14.67/s |
| 100 | Firecracker 1.17.0 | 1000/1000 | 5450.96 ms | 7019.59 ms | 7175.85 ms | 17.55/s |

All 3600 attempts passed. The 100/25/10/10 paired batches per concurrency
are the independent workload repetitions; guests within a batch share load
and are not independent repetitions. Burst ready rate divides validated,
cleanup-checked passing attempts by the interval from barrier release to
the last guest/resource validation, excluding cleanup. It is not sustained
arrival throughput. Per-attempt latency starts at its native creation path,
while measured launch spread captures scheduling and driver preparation.
Median launch spreads for HyperMachine/Firecracker were 0/0 ms at concurrency
1, 1.26/5.75 ms at 8, 39.59/51.27 ms at 50 and 102.83/107.81 ms at 100.
The host is shared, and HyperMachine uses a persistent HTTP node while
Firecracker starts a process and configures its Unix API for each guest.
These eight-CPU profiles cannot be substituted into earlier one-CPU cohorts.

| Held guests | HyperMachine node PSS, median | Sum of held Firecracker process PSS, median |
|---:|---:|---:|
| 1 | 183.03 MiB | 85.90 MiB |
| 8 | 918.55 MiB | 670.98 MiB |
| 50 | 4315.47 MiB | 4180.95 MiB |
| 100 | 8568.50 MiB | 8360.36 MiB |

These are aggregate process footprints immediately after each batch's last
validation. HyperMachine includes its daemon and allocations retained from
prior batches; Firecracker sums fresh processes. The initial empty-node PSS
was approximately 7 MiB, but no per-batch empty-node baseline or fixed idle
period was measured. Kernel allocations are excluded, earlier-ready guests
have longer idle time, and the readings are neither per-VM incremental
memory nor evidence of a stateful density win.

The sweep establishes measured native-engine concurrency coverage and an
observed performance gap: HyperMachine remains slower, with a larger tail
gap at 50 and 100 guests. It does not isolate a cause or establish managed
platform, SDK, snapshot, bare-metal or fleet superiority. Raw cohorts and
the matrix summary use the `benchmarks/2026-09-30/local-engines-bursts-`
prefix; exact coordinator and analysis sources are retained alongside them.
The analysis checks ordering, attempts, deadlines, resource/CPU profiles,
artifact identities, timing consistency, worker liveness and cleanup.
Ten synchronization/workload/failure tests pass on Windows and Linux and
are included in the existing benchmark CI gate. Earlier harnesses remain
unchanged so their recorded source hashes are preserved. Reproduction
requires updating local artifact/output paths in the recorded coordinator.

## Concurrent startup diagnostics (2026-10-01)

Two diagnostic cohorts each ran two alternating engine batch pairs at
concurrency 100, with the same fixed-UART guest, eight-CPU affinity and one
controlled CPU-0 worker. All 800 attempts passed; each cohort matched all
200 HyperMachine request IDs to daemon stage logs. Artifact identities,
worker liveness, sandbox deletion and process cleanup passed. These traced
cohorts are investigative evidence, excluded from the scored comparison.

| Mean daemon stage | Stage-only cohort | Guest-probe cohort |
| --- | ---: | ---: |
| Build | 6.63 ms | 6.06 ms |
| Launch | 108.36 ms | 99.01 ms |
| Agent readiness | 7254.91 ms | 5471.78 ms |
| Network and envd | 0.11 ms | 0.08 ms |
| Total bring-up | 7370.00 ms | 5576.93 ms |

The second cohort collected guest logs from every tenth request after all
100 guests in each batch validated readiness. Those extra commands affect
memory readings and cleanup duration, but occur after recorded startup.
The 20 sampled guests handed off to `/init` at a mean guest timestamp of
1531.49 ms; their paired host agent-wait duration was about 3985.26 ms longer.
Host durations and guest timestamps cannot isolate dispatch, guest userspace
or handshake delay. All 20 sampled logs show CRNG initialization before the
handoff; this does not establish entropy behavior for unsampled guests.

Agent readiness dominates both cohorts. Image build/launch timing does not
support prioritizing image-loading optimization on this workload. Existing
code already offloads cold guest connection/ping to a blocking task and
runs each vCPU on a dedicated thread; finer queue, dispatch and handshake
tracing is needed before selecting a runtime change. Only two batch pairs,
shared nested hardware and asymmetric tracing limit causal conclusions.

Exact reports, coordinators, the original stage-only wrapper and analysis
are retained in `benchmarks/2026-10-01`. The optional wrapper
`tools/diagnose-concurrent-startup.py` leaves scored harnesses unchanged;
three duration/identity parser tests pass on Windows and Linux and run in CI.
No production performance change or competitor win follows from this step.

Cold readiness now also exposes opt-in debug events under the dedicated
`hv2_agent::cold_readiness` target. Enable it with
`RUST_LOG=hv2_sandboxd=debug,hv2_agent::cold_readiness=debug` to separate
blocking-task queue time, vsock connection time and ping time by VM name.
Connection failures record queue/connect timing and `phase=connect`;
completed ping attempts include all three durations and their success state.
This instrumentation preserves the existing blocking-task execution and
timeout arguments. Formatting and Linux all-targets agent compilation pass;
a finer-stage cohort is recorded below.

The dedicated traced daemon (`c3e96cab…`) then completed two alternating
C100 batch pairs: all 400 attempts passed and all 200 HyperMachine IDs
matched both daemon and cold-readiness stages. Guest probes were disabled;
the scored daemon was restored to its original `8ea52a36…` hash.

| Cold readiness component | Mean | Median | Maximum |
| --- | ---: | ---: | ---: |
| Blocking-task queue | 8.73 ms | 5.19 ms | 116.07 ms |
| Vsock connection | 6239.36 ms | 6225.59 ms | 8902.98 ms |
| Ping | 15.62 ms | 6.14 ms | 168.63 ms |

Connection timing includes waiting for guest boot, vsock driver and listener
readiness; it is not isolated transport overhead. This cohort directs the
next investigation toward that path rather than blocking-pool queueing or
ping handling. Two correlated batches per engine, shared nested hardware
and tracing do not establish causal attribution or a performance win.
Cleanup, worker liveness and artifact checks passed. The raw log, matched
stages, exact coordinator and verifying analysis are retained under the
`cold-readiness-` prefix in `benchmarks/2026-10-01`; five parser tests pass
on Windows and Linux. Historical wrapper versions remain archived so their
recorded hashes survive the optional finer-stage collection mode.

The guest init script now offers two opt-in kernel-timestamped milestones
with `HV2_BOOT_TRACE=1` on its kernel command line: `HV2_BOOT
core_mounts_ready` after proc/sys/dev mounts, and `HV2_BOOT agent_launch`
after the remaining mounts, network and environment setup, just before
starting the agent. These use `/dev/kmsg` and appear in guest `dmesg`.
Together with the existing kernel init-handoff timestamp, they can bound
pre-launch userspace work. They do not mark socket bind or acceptance.
Normal boots emit neither milestone. Linux shell syntax validation passes;
runtime collection follows below; its timing impact has not been isolated.

The diagnostic image (`781c3944…`) enables these milestones identically for
both engines. Archive verification confirms equal entry sets and modes,
with only `init` content changed; BusyBox and the guest agent are byte-identical
to the prior image. Another two alternating C100 batch pairs passed all 400
attempts, matched all 200 HyperMachine stage IDs and recorded both milestones
in all 20 sampled guests. Artifact, worker and cleanup checks passed.

| Guest-clock interval, 20 samples | Mean | Median | Maximum |
| --- | ---: | ---: | ---: |
| Init handoff to core mounts | 18.45 ms | 11.81 ms | 63.92 ms |
| Core mounts to agent launch | 11.79 ms | 1.20 ms | 61.69 ms |
| Init handoff to agent launch | 30.24 ms | 26.45 ms | 103.58 ms |

Mean guest agent-launch timestamp was 1810.64 ms. Host queue/connect/ping
means were 8.11/5585.95/9.28 ms for all 200 HyperMachine requests. The small
init interval does not support prioritizing shell setup optimization for
these samples. Guest scheduling and socket/packet readiness remain candidate
paths; clock origins differ and no socket bind/accept milestone was collected.
Probes occur after batch readiness and affect memory/cleanup readings.
These traced, sampled, shared-host cohorts are excluded from scored comparisons
and establish neither a runtime improvement nor a competitor win.
The `boot-milestones-` reports, build/coordinator/analysis sources and separate
archive verifier are retained in `benchmarks/2026-10-01` with exact hashes.

A subsequent diagnostic guest-agent build adds best-effort, opt-in
`agent_listening` and `agent_first_accept` kernel markers. Two C100 batch
pairs passed all 400 attempts with 200 matched HyperMachine stage IDs and
20 complete guest observations. Mean guest launch-to-listen was 16.72 ms
(median 1.99 ms, maximum 67.85 ms); listen-to-first-accept was 23.93 ms
(median 20.11 ms, maximum 135.55 ms). Host queue/connect/ping means were
9.06/6052.85/12.71 ms. These samples do not show seconds of delay after
listening. Aligning host VM dispatch and guest boot clocks remains necessary
before attributing the larger host connection wait.

The first marker implementation wrote formatted fragments as separate kmsg
records. This cohort's analysis uses the individually timestamped label
records; its exact executed agent source is archived. Current source submits
each marker as one buffer. All 16 Linux guest-agent tests and formatting pass
after that correction; corrected-marker runtime collection remains pending.
Archive verification confirms only init and agent content changed, equal modes
and entry sets, and identical BusyBox. Cleanup and artifact checks passed.
Sources and reports use `listener-milestones-`, `listener-image-` and
`guest-listener-trace-` names in the same evidence directory. This remains
sampled diagnostic evidence, excluded from performance rankings.

Paired host/guest clock observations then completed another 400/400 C100
attempts with all 200 HyperMachine stage IDs matched. Each of 20 sampled
guests supplied two uptime reads, each bounded by host monotonic request
start/end timestamps. All 20 guest elapsed intervals fit the host bounds
after allowing two centisecond quantization bins. This checks only those
post-readiness intervals, not earlier boot-time clock stability.

Conditionally assuming stable elapsed clocks back through boot, the mean
inferred guest-clock epoch bounds were 4422.50–4449.58 ms after native
creation began. The guest clock epoch is not VM entry: this does not prove
dispatch delay or isolate transport behavior. It directs the next measurement
toward host VM/vCPU dispatch and early guest clock initialization. Reports,
exact executed wrapper, coordinator and analysis use `clock-alignment-`
names in the evidence directory. Cleanup and artifact checks passed; memory
readings follow the extra probes. The scored harnesses remain unchanged.

Host dispatch tracing is available under the dedicated
`hv2_core::cold_dispatch=debug` filter. `VM background dispatch` records
time queued before the background run task starts. `vCPU owner thread entry`
records wrapper-task queueing and time from wrapper entry to OS-thread
entry, with VM name and vCPU ID. It precedes affinity/runtime setup and
the first backend run; it is not a guest-entry timestamp. Linux all-targets
core compilation and formatting pass. Runtime collection is pending and
these events establish no scheduling improvement.

The traced daemon (`08e8aa52…`) then completed two alternating C100 batch
pairs, passing all 400 attempts and matching all 200 HyperMachine IDs to
complete dispatch/readiness records. Mean/maximum VM dispatch queue times
were 5.00/31.49 ms; vCPU wrapper queue times 0.0043/0.0289 ms; wrapper-to-owner
thread entry times 7.91/125.45 ms. All 20 paired guest uptime intervals fit
their host bounds. These dispatch intervals do not account for the seconds
between native creation and the conditional guest clock epoch. Affinity and
runtime setup, first backend run, and early guest work before clock initialization
remain unmeasured boundaries; no scheduling root cause is established.
Artifact, worker and cleanup checks passed. Exact sources, raw log and analysis
use `dispatch-diagnostic-` names in the same directory. Seven parser tests pass
on Windows and Linux. Tracing and post-readiness probes exclude this cohort
from scored comparisons; the original scored binary hash is restored.

First-backend probes extend the dedicated dispatch target with
`owner_setup_ms` (owner-thread entry through affinity/runtime setup and
control checks to the first backend invocation) and `first_backend_ms`
(that invocation through its first successful return). Each emits once per
vCPU execution loop, not once per VM exit. The first invocation is not a
guaranteed KVM guest-entry timestamp, and an error does not emit the successful
return marker. The diagnostic wrapper requires all five dispatch fields for
each passed guest. Linux all-targets compilation, formatting and parser tests
pass; runtime first-backend measurements remain pending. The prior dispatch
wrapper is archived as `dispatch-harness-v1.py` for its recorded source hash.

The first-backend traced daemon (`6718bad3…`) completed another two C100
batch pairs: all 400 attempts passed and all 200 HyperMachine IDs matched
complete five-field dispatch records. Owner setup mean/median/maximum was
1.37/0.087/46.40 ms. First backend call mean/median/maximum was
3480.50/3328.36/4924.20 ms. Thus a substantial measured interval is inside
the backend run operation, rather than affinity/runtime setup. That interval
includes host scheduling and guest execution through the first return; the
exit kind and guest instruction address were not collected. It does not
isolate kernel decompression, CPU work or a hypervisor defect.

All 20 paired uptime intervals fit host bounds, and artifact/worker/cleanup
checks passed. Sources and reports use `first-backend-diagnostic-` names in
the evidence directory. The shared host showed large variation in the two
Firecracker batches; this asymmetric tracing cohort remains excluded from
performance rankings. The original scored binary hash is restored. First-exit
identity and early kernel execution are the next measurement boundaries.

An experimental Linux 6.6.52 LZ4-compressed kernel build completed to test
the compressed early-boot path. The retained `lz4-guest.defconfig` differs
from the default only by disabling kernel gzip compression and selecting
LZ4. The owned build's resolved configuration enables LZ4, strict kernel and
module RWX protection, and DEBUG_WX. It uses the existing source-checksummed
builder with eight-CPU affinity and separate output paths; defaults remain
unchanged. Reproduce with LZ4 installed:

```sh
taskset -c 0-7 bash tools/guest-image/build-kernel.sh \
  --config docs/benchmarks/2026-10-01/lz4-guest.defconfig \
  --config-output /var/tmp/hm-kernel-lz4/resolved.config \
  -o /var/tmp/hm-kernel-lz4/bzImage
```

The kernel hash is `d0b6b580…`; its 17,327,104 bytes exceed the gzip kernel's
12,952,576 bytes. The exact builder, resolved config and build identities are
retained alongside the experimental defconfig. A concurrency-1 smoke cohort
passed all four native attempts (two per engine), including readiness, resource
validation, artifact stability and cleanup. This is functional evidence only.
Six counterbalanced C100 blocks completed, with two alternating engine pairs
per kernel per block. Native timing includes larger image-loading costs.

| Kernel / engine | Passed / attempts | Successful P50 | Successful P99 |
| --- | ---: | ---: | ---: |
| Gzip / HyperMachine | 1190 / 1200 | 6801.47 ms | 15626.92 ms |
| LZ4 / HyperMachine | 1200 / 1200 | 6523.37 ms | 11610.63 ms |
| Gzip / Firecracker | 1200 / 1200 | 5594.62 ms | 11149.61 ms |
| LZ4 / Firecracker | 1200 / 1200 | 4746.36 ms | 5063.31 ms |

All ten failures occurred in HyperMachine's first gzip batch of the final
block. They timed out waiting for guest readiness; retained diagnostics show
halted vCPU samples and continued I/O exits. Artifact stability, controlled
worker liveness, node-empty checks and daemon cleanup passed for every cohort.
Failed create rows have no returned sandbox ID and no client delete success;
the isolated node cleanup checks still passed. The cohort remains failed and
its raw errors are retained. Successful latency quantiles exclude these ten
timeouts and cannot represent unconditional completion latency.

For the five HyperMachine blocks with complete success on both kernels, LZ4
reduced the block mean in only 2/5, with a median delta of **+31.05 ms**
(LZ4 minus gzip). Firecracker improved in all six complete blocks, with a
median delta of -950.88 ms. Shared-host variation, within-batch correlation
and excluding the failed HyperMachine block from paired latency analysis
limit causal conclusions. Zero LZ4 failures here does not establish a
reliability fix. HyperMachine still trails Firecracker on the LZ4 profile.

LZ4 checkpoint restore, pause/resume and two stateful forks pass against a
snapshot-backed template, with guest state, command readiness, I/O APIC
discovery, artifact stability and cleanup verified. Exact cohort sources,
manifest, raw reports and analysis use the `kernel-compression-` prefix;
state evidence is in `lz4-state.json` and `verify-lz4-state.py`.
This experiment does not establish a repeatable HyperMachine performance
improvement or decompression as the root cause. The default kernel is unchanged.

All ten gzip timeouts share owner RIP `0xffffffff81eda95f`, halted run state
and LAPIC timer value `0x400ec`. A separate matched-kernel Firecracker guest
resolved `/proc/kallsyms`: `default_idle` starts at `0xffffffff81eda950`,
15 bytes before the sampled RIP. That diagnostic guest passed readiness and
cleanup; kernel and helper hashes were unchanged. The exact query and source
are retained as `gzip-timeout-halt-symbol.json` and its resolver script.
This confirms a shared idle-path address, not a root cause.

Failure reports now include raw `TSC` and `TSC_DEADLINE` values already captured
by the owner-safe vCPU snapshot. Missing values print `unavailable`, distinct
from a captured zero. No extra register reads, snapshot schema changes or
runtime timer behavior are introduced. MSR reads are sequential and these
values do not form an atomic timer comparison. All six Linux boot-diagnostic
tests and formatting pass, including missing/zero/full-width clock values.
Runtime capture of these fields during another failed boot remains pending.

A separate failure-diagnostic daemon (`0341fe7e…`) then ran ten alternating
C100 engine pairs with the original gzip guest, eight-CPU affinity and one
CPU-0 worker. All 2000 attempts passed (1000 per engine), with stable artifacts,
worker liveness, empty-node checks and daemon cleanup. Warning-level logging
was retained. This cohort had no failures and therefore collected no failed-boot
timer samples. It does not resolve the ten prior timeouts or establish a
reliability fix; only failure-message formatting changed, not timer behavior.
The exact coordinator, raw report and analysis use `timer-diagnostic-` names
in the same evidence directory. The original scored binary hash is restored.

Failure reports now also decode the already captured KVM I/O APIC irqchip
payload. They include controller base/select/id/IRR and all 24 GSI routes,
with full-width raw values, vector, mask, level-trigger, remote-IRR and
destination fields. Decoding requires the complete 512-byte union and ignores
its trailing padding; missing or malformed captures print `unavailable`.
These machine reads remain sequential, taken before the owner kick, and are
not an atomic interrupt snapshot. No additional ioctls, snapshot schema or
interrupt behavior change. All eight Linux boot-diagnostic tests and formatting
pass, including full-width routes, boundary pins, poisoned padding and malformed
lengths. The existing workspace CI test job includes these daemon tests.
Unforced failed-boot runtime I/O APIC route capture remains pending; no wakeup
fix follows from this formatter change.

A negative-control image omits the agent launch line while retaining all other
archive content and modes, including identical agent/BusyBox binaries. Against
the diagnostic daemon (`5e6916bf…`), its create request returned the expected
503 readiness failure after 15.56 seconds. The API error contained captured
TSC/deadline values and all 24 I/O APIC routes in order. The node was empty,
daemon cleanup passed and artifact hashes were unchanged. This verifies live
failure formatting; the deliberate missing agent does not reproduce the ten
earlier timeouts. Image build/verification and API evidence use the `no-agent-`
names in the evidence directory.

The same daemon then passed all 400 normal-guest attempts in two alternating
C100 pairs with the original gzip image and controlled host profile. Artifact,
worker and node/daemon cleanup checks passed, but no intermittent failure or
its route state was observed. Reports and sources use `ioapic-diagnostic-`
names. Both controls are diagnostic evidence, excluded from scored comparisons;
the original scored binary hash is restored. No reliability fix is claimed.

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

The [current native-engine comparison](benchmarks/2026-10-02/current-native-engines/README.md) retains all 636 attempts, including 100 HyperMachine creation failures at concurrency 100. Profiles 1/8/50 fully passed; Firecracker had lower successful P50 and held/incremental process PSS in those profiles. The 100-guest successful latency and memory results are conditional on incomplete HyperMachine evidence. Cleanup was verified and no runtime optimization was adopted. This is a shared-host baseline refresh, not a causal comparison with earlier feature versions. High-concurrency boot/agent readiness remains a priority gap.

[Current high-concurrency readiness diagnosis](benchmarks/2026-10-02/current-readiness/README.md) retained the failed scored batch and ran a separate unchanged-artifact diagnostic. All 400 diagnostic attempts passed, without reproducing the failure. HyperMachine's 200 instrumented attempts had median blocking-worker queue 7.32 ms, agent connection wait 8986.04 ms and ping 4.99 ms. Connection wait includes guest boot, driver and listener readiness; tracing changes timing and does not establish a transport cause or a reliability fix. No runtime change was adopted.

The [event-driven connection-wait experiment](benchmarks/2026-10-02/connection-wait/README.md) passed all 800 baseline/candidate attempts at concurrency 100. The candidate improved batch means in only two of four matched pairs; P50 was 6469.43 ms versus baseline 6460.28 ms, with lower candidate P99. The mixed result did not justify adoption. No wakeup savings, reliability fix or competitor win was established.

The [current-kernel symbol and first-return diagnostic](benchmarks/2026-10-02/first-exit/README.md) maps the retained timeout RIP to `default_idle+15` using the exact failed-cohort kernel. A separate C100 diagnostic passed 400/400 attempts and matched 200 HyperMachine first returns, all OUT to PCI CONFIG_ADDRESS (`0xCF8`). The first backend interval had median 3098.63 ms; owner setup had median 0.09 ms. This directs investigation toward early guest execution and host scheduling before the first PCI probe. It does not isolate CPU work, reproduce or fix the timeout, or establish a performance win. The accepted benchmark baseline remains unchanged.

The [first backend CPU/wall diagnostic](benchmarks/2026-10-02/first-cpu/README.md) passed all 436 attempts at C1/8/100 and matched 218 HyperMachine clock pairs. At C100, median first-call wall time was 2986.04 ms and CPU consumption 218.03 ms, with median CPU/wall fraction 7.30%. Elapsed time not charged to the calling thread dominated this interval; scheduling, blocking and nested-host effects remain unseparated. A missing-clock negative collection preserved four passing guest attempts while correctly failing measurement. This supports testing bounded cold-boot concurrency, without claiming an optimal limit, a timeout fix or competitor superiority. Traced timings remain excluded from scored rankings.

The [durable audit writer comparison](benchmarks/2026-10-02/audit-batching/README.md) retained 204,516/204,516 successful protected-API requests and independently verified 187,416 audit records. Grouping queued records behind the same write-and-sync acknowledgment boundary raised main matched median batch throughput at concurrency 8/50/100 from 219/217/219 to 834/4239/4640 requests per second, with repeat gains at 8/100 after storage timing changed. The one-worker audited path was slightly slower; the longer audit-disabled one-worker control had similar median latency and throughput. All 13 process fault checks and 22 KVM/TLS checks passed, including an actual partial write followed by refusal to append or restart. The grouped writer was adopted for the opt-in audit feature. This is HTTP inventory performance, not guest startup or competitor performance.
