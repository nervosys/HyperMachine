# Beating CubeSandbox: a feature and performance roadmap

Status: **planning document, nothing built as a result of it yet**. Written
after reading CubeSandbox's README, architecture doc, and network model doc
(not just its marketing copy), and after confirming which of HyperMachine's
own more ambitious-sounding crates are real and tested rather than
aspirational — this repo has a documented history of the latter (`hv2-sandbox`
exists specifically because two other things "looked like sandboxes and
confined nothing," see `docs/SANDBOXES.md`), so nothing here is taken on
faith from a doc comment alone.

## The honest starting position

[CubeSandbox](https://github.com/tencentcloud/CubeSandbox) (Tencent Cloud) is
not a research project. As of this writing: 12.5k GitHub stars, 1,122 forks,
a "Cube 100" program recruiting production teams, Apache 2.0, in the CNCF
landscape. It is a mature, cluster-scale, production sandbox-as-a-service
platform:

| Component | What it does |
|---|---|
| CubeAPI (Rust/Axum) | E2B-compatible REST gateway |
| CubeMaster (Go) | Stateless cluster scheduler, coordinates through Redis |
| Cubelet (Go) | Node-local lifecycle manager (create→run→pause→resume→snapshot→destroy) |
| CubeShim (Rust) | containerd Shim v2 implementation bridging to the VMM |
| CubeHypervisor (Rust, RustVMM+KVM) | The actual microVM: vCPU, memory, virtio devices, seccomp-hardened |
| CubeVS (eBPF) | Per-sandbox SNAT/DNAT, stateful conntrack, LPM-trie policy, all in-kernel |
| CubeEgress (OpenResty+Lua) | L7 TLS-inspecting egress proxy: domain allowlisting, credential injection, audit log |
| CubeCoW (Rust, XFS reflink) | O(1) snapshot/clone via `FICLONE`, incremental dirty-page tracking |
| CubeProxy + cube-lifecycle-manager | E2B-protocol request routing, transparent auto-pause/resume |
| WebUI | Cluster/template/version management console |

Benchmarked (their numbers, bare metal): **<60ms cold start**, **<5MB memory
overhead** per sandbox, thousands of sandboxes per node. Plus: K8s and
Terraform deploy, native ARM64, a pluggable volume framework, and a stated
roadmap toward full E2B spec compatibility and cross-node live migration.

**HyperMachine today**, checked directly rather than assumed:

- `hv2-sandbox` — OS-level process confinement (Linux namespaces/cgroups,
  Windows job objects, macOS `rlimit`). Real and honestly-scoped (see
  `docs/SANDBOXES.md`'s own account of bugs found by actually running it on a
  kernel), but **this is not a VM** — it is the CubeSandbox-Docker row in
  their own comparison table, not the CubeSandbox row. No snapshot, no CoW,
  no cluster, no egress filtering, no credential vault.
- `hv2-core` + `hv2-unikernel` — a genuine KVM/WHPX/HVF microVM path exists,
  and a Multiboot `no_std` guest was boot-verified for the first time only
  last week (a 57-byte frame round-tripped through a virtio-net driver in
  2.3ms — the round trip, not the boot time). No cold-start benchmark exists.
  No snapshot/restore. No density testing. One guest booted, by hand, once.
- `hv2-net` — TAP/virtio-net/vswitch plus a hand-written NAT module (this
  week's work). Pure userspace, no eBPF, no per-sandbox policy enforcement,
  no connection tracking at line rate.
- No CoW/snapshot engine anywhere in the tree.
- No egress security proxy, no credential vault, no TLS interception.
- No E2B API compatibility.
- No multi-node cluster orchestrator — `hv2-runtime` claims "VM pools...
  scales from a single agent to thousands" and has 61 passing unit tests, but
  that is one host's process pool under test, not a scheduled multi-node
  cluster with a stateless control plane the way CubeMaster/Cubelet are.
- No K8s/Terraform deploy path, no WebUI, no ARM64 story (checked, not
  attempted).

**Where HyperMachine already has something CubeSandbox has nothing like** —
this is the actual opening, and it's worth stating plainly rather than
burying it under the infrastructure gap above:

- `hv2-swarm` — enforced agent-to-agent permission graph (208 passing tests).
  CubeSandbox has no concept of *which agents may talk to which* — it hosts
  arbitrary sandboxed workloads, agent-aware only at the API-compatibility
  layer.
- `hv2-context` — an addressable, append-only event log with externalized
  payloads an agent queries at read time instead of re-summarizing at write
  time (29 tests). This is agent-cognition infrastructure; CubeSandbox has
  none.
- `hv2-runtime` — durable workflow state, scheduling, autoscaling *of agent
  workloads specifically* (not just VMs) (8 tests, thinner coverage than the
  others — flagged, not hidden).
- `hv2-infer` — a real transformer forward pass as a host-side capability a
  sandbox invokes, with the scheduling reasoning for why it lives in the host
  and not the guest already written down (15 tests). CubeSandbox runs
  arbitrary code; it does not run *inference* as a first-class primitive.
- `hv2-gpu` — GPU virtualization (Vulkan/WebGPU passthrough path). Not
  mentioned anywhere in CubeSandbox's docs.

None of this closes the production-infrastructure gap above. It does mean
"beat CubeSandbox" has two different, almost unrelated meanings, and this
roadmap should not pretend they're the same project:

1. **Match or beat CubeSandbox at being CubeSandbox** — fast, dense,
   cluster-scale, E2B-compatible sandbox-as-a-service. This is the
   README's literal ask, and it is a multi-month, multi-engineer
   infrastructure program, not something a design doc turns into working
   software.
2. **Beat CubeSandbox at hosting multi-agent workflows specifically** —
   where HyperMachine already has real (if unevenly tested) primitives
   CubeSandbox has never built, because CubeSandbox's product is
   "sandbox for arbitrary agent-generated code," not "runtime for a
   coordinated fleet of agents that need to know who may talk to whom."

## Strategic considerations before committing to (1)

- **License.** CubeSandbox is Apache 2.0. HyperMachine (and everything this
  roadmap would build on) is `AGPL-3.0-only OR LicenseRef-Commercial`. That is
  a real adoption-friction difference for anyone evaluating both as
  open-source infrastructure to self-host, independent of feature parity —
  worth a deliberate decision, not an oversight, before positioning this as a
  drop-in competitor.
- **E2B compatibility is CubeSandbox's distribution strategy**, not an
  incidental feature — "change one environment variable" is how they capture
  existing E2B users with zero migration cost. Matching that specific
  compatibility layer is disproportionately high-leverage versus building
  every other component first: a HyperMachine backend that speaks the E2B
  wire protocol captures the same switching-cost advantage regardless of
  which other gaps remain open.
- **Tencent Cloud has a cloud business subsidizing this.** Terraform
  one-click deploy, K8s support, and the "Cube 100" production-support
  program are backed by a hyperscaler's infrastructure and sales motion.
  Competing on *deployment convenience* at that level is a different kind of
  investment than competing on the engineering underneath it.

## Phased roadmap

Sequenced so each phase produces something independently measurable, and
nothing later blocks on a phase whose scope hasn't been validated first —
the same discipline `docs/arena/SANDBOX_DESIGN.md` (in the Botnet repo, a
downstream consumer of `hv2-sandbox`) used, and for the same reason: this
repo has already paid once for treating an unverified claim as a plan.

### Phase 0 — Benchmark honestly, before promising anything — **done**

Built `hv2-core/examples/unikernel_cold_start.rs` (reusing `cold_start.rs`'s
phase-breakdown and percentile-reporting shape) and ran it against a real
`/dev/kvm` (via this machine's WSL2 Debian instance — 24 cores, 45GB RAM, not
dedicated bare metal, and not idle: another session's build was competing
for CPU during an earlier boot test this week, a real caveat on anything
measured here, not a hypothetical one). "Ready" is defined as the guest
printing `vsock cid ` — its vsock driver up, the point at which a host↔guest
exec the way `AgentVM::exec_in_guest` uses would be possible. Memory
overhead was **not** measured — printing a guessed number would be exactly
the failure mode this repo's own `cold_start.rs` warns against.

**Concurrency 1** (20 iterations), vs. CubeSandbox's published <60ms:

```
ready   n=20   avg  3.19ms  min  2.22ms  P50  3.06ms  P95  3.95ms  P99  4.50ms  max  4.50ms
```

**~20x faster than the published figure at single concurrency.** Real, but
not the whole story, and reporting only this number would be exactly the
kind of overclaiming this roadmap set out not to do — see the caveat below.

**Concurrency 50** (5 iterations × 50, vs. CubeSandbox's published 67ms avg
/ P95 90ms / P99 137ms):

```
ready   n=250   avg  84.16ms  P50  35.01ms  P95  385.14ms  P99  512.70ms  max  586.88ms
```

**Worse than the published figure, and by a lot at the tail** — nearly 4x
their P99.

**Investigated rather than left as a guess.** Split the `build` phase into
`new_vm` (pure in-process struct allocation, no hypervisor syscall at all)
and `provision` (the actual `KVM_CREATE_VM` ioctl plus boot-image load) to
find out whether the concurrency-50 slowdown was a code-level lock or
something else:

```
new_vm     n=250  avg  14.59ms  P50   6.65ms  P95   48.08ms  P99   65.49ms
provision  n=250  avg  56.84ms  P50  47.60ms  P95  143.99ms  P99  242.57ms
```

`new_vm` — no KVM, no ioctl, nothing but Rust struct allocation — is
*already* 14.6ms average under 50-way concurrency, against near-zero at
concurrency 1. A pure-userspace phase touching no hypervisor state slowing
down that much rules out "one lock inside `VM::new` or `provision`" as the
whole story; something at the level of the whole host is under load, not
one function.

That something turned out to be the test environment itself:
`systemd-detect-virt` on this WSL2 instance reports `wsl`, and
`kvm_amd`'s `nested` parameter is `1` — every boot in this benchmark ran
**KVM nested inside WSL2's own Hyper-V virtualization**, a third layer
(Hyper-V → WSL2's KVM → `hv2-unikernel`), not the direct bare-metal KVM
CubeSandbox benchmarks on. Nested virtualization has well-documented,
disproportionate overhead specifically under concurrent load (every VM-exit
during boot gets trapped and re-injected through an extra hypervisor layer),
which is a more likely explanation for a 20-50x-worse-than-single-VM
slowdown than a lock this repository's own code is holding — and it
explains why even the KVM-untouched `new_vm` phase degraded: general host
contention under nested-virt load doesn't need to go through KVM to slow
every process down.

**This does not resolve the question — it changes what the next
measurement has to be.** The concurrency-50 numbers above are real
measurements of *something*, but not, on this evidence, a clean measurement
of HyperMachine's own scaling behavior. That requires re-running this same
benchmark on real bare-metal Linux with non-nested KVM before either
believing "there's a serialization bug" or "we're fine at scale" — neither
claim is supported by a nested-virtualized test run, and asserting one
anyway would be exactly the failure mode this whole roadmap is trying not
to repeat.

**Two honest caveats, not asterisks to bury:**

1. **Not a capability-equivalent comparison.** `hv2-unikernel` is a bare
   `no_std` guest with no filesystem, no shell, no ability to run arbitrary
   user code yet — it boots to a vsock control channel and nothing else.
   CubeSandbox's <60ms is for a guest that can actually run an agent's code.
   The concurrency-1 result says the underlying KVM-boot mechanism has very
   low inherent overhead, not that HyperMachine has a deployable sandbox
   that's 20x faster.
2. **Not measured on comparable hardware.** CubeSandbox's numbers are bare
   metal; every number in this section came from nested KVM inside a WSL2
   VM. The concurrency-1 result (20x faster) is likely *understating* the
   real advantage, if any, since nested-virt overhead is usually worst
   exactly where this test barely exercises it (concurrent VM-exit storms);
   the concurrency-50 result is close to uninterpretable until re-run on
   bare metal. Re-measuring both on real hardware is the immediate next
   action this phase's own findings point to, not a nice-to-have.

**Exit criterion met**: a real, reproducible number exists where before
there was only an anecdotal 2.3ms *round-trip* figure and no boot-time
number at all — and, just as importantly, this pass also found that the
measurement itself needs to move to bare metal before either number means
anything. That is now the concrete, measured reason Phase 1 shouldn't be
treated as "we're already 20x faster, ship it": not because there's a
proven scaling bug, but because there isn't yet a clean measurement to
build that claim on either way.

### Phase 1 — E2B compatibility layer (highest leverage-to-effort ratio) — **started, first slice live-verified**

Built `crates/hv2-api/examples/e2b_compat.rs`: a real (not mocked) `POST
/sandboxes` against E2B's actual `NewSandbox`/`Sandbox` schemas (field names
— `templateID`, `sandboxID`, `clientID`, `envdVersion` — taken directly from
`e2b-dev/E2B`'s `spec/openapi.yml`, not guessed), backed by a real
`hv2-agent` VM boot on the Linux guest-agent path this roadmap's Phase 0
already verified (923ms cold start). A companion `exec` endpoint
(deliberately **not** E2B's real envd wire protocol — see below) proves the
same underlying capability envd's `run_code` depends on, and a `delete`
endpoint tears the VM down.

**Verified live, full lifecycle, against a real booted guest:**

```
$ curl -X POST localhost:3980/sandboxes -d '{"templateID":"base"}'
{"templateID":"base","sandboxID":"sbx_18d5a9b4324ee231","clientID":"sbx_18d5a9b4324ee231","envdVersion":"hv2-guest-agentd/0 (not envd)"}
HTTP 201

$ curl -X POST localhost:3980/sandboxes/sbx_.../exec -d '{"cmd":"echo hello; uname -a; id"}'
{"exit_code":0,"stdout":"hello from a real microVM\nLinux (none) 6.6.52 #1 SMP ... x86_64 GNU/Linux\nuid=0 gid=0\n","stderr":"","timed_out":false}
HTTP 200

$ curl -X DELETE localhost:3980/sandboxes/sbx_...
HTTP 204
```

The `uname`/`id` output came back from the actual guest kernel (6.6.52,
matching `/var/tmp/kbuild/kernel-6.6.52.config`), not simulated — a REST
call reached a real hardware-isolated microVM, ran a command inside it, and
returned real output, end to end.

**What this is not, stated plainly:** an E2B SDK client cannot point at this
today and work — at the time this was first written. That's since changed
for the process-execution half of envd's protocol:

**envd's actual `process.Process` service, implemented and live-verified.**
Fetched the real `process.proto` from `e2b-dev/runtime`'s
`packages/envd/spec/process/process.proto` and copied it verbatim into
`crates/hv2-api/proto/process.proto` — not reconstructed from
documentation. Compiled it with `tonic_prost_build` (server only: the
proto's own `Connect` RPC collides with the generated client's inherent
`connect()` constructor, E0592, if the client is built too — a real, minor
codegen wrinkle worth knowing about, not a proto bug). Implemented `Start`
(the RPC `run_code` depends on) in a new example,
`crates/hv2-api/examples/envd_process.rs`, wired to `AgentVM::exec_in_guest`
against the same Linux guest-agent path Phase 0 measured.

Tested with `grpcurl` — a real, independent gRPC client, not code that only
talks to itself — against the real compiled service:

```
$ grpcurl -plaintext -import-path crates/hv2-api/proto -proto process.proto \
    -d '{"process":{"cmd":"/bin/sh","args":["-c","echo hello from real envd protocol; uname -a"]}}' \
    localhost:8081 process.Process/Start

{"event": {"start": {}}}
{"event": {"data": {"stdout": "aGVsbG8gZnJvbSByZWFsIGVudmQgcHJvdG9jb2wKTGludXggKG5vbmUpIDYuNi41MiAj..."}}}
{"event": {"end": {"exited": true, "status": "exited"}}}
```

Decoded, that `stdout` is `hello from real envd protocol\nLinux (none)
6.6.52 #1 SMP PREEMPT_DYNAMIC ... x86_64 GNU/Linux` — the real guest kernel,
returned through the real wire protocol's real message shapes
(`StartEvent`/`DataEvent`/`EndEvent`), not a REST shim wearing envd's field
names.

**What's still simplified, stated plainly:** `exec_in_guest` runs a command
to completion and returns whole stdout/stderr at once, so `Start` emits its
three events in a burst rather than streaming output as it's produced —
real envd streams live. `Connect`/`Update`/`StreamInput`/`SendInput`/
`SendSignal`/`CloseStdin` return `unimplemented` — each needs a way to
reach a process that's still running, which one blocking `exec_in_guest`
call doesn't provide.

**`List` is real now, not a stub — backed by an actual table of in-flight
`Start` calls.** `exec_in_guest` reports no real guest PID, so the id used
here (`StartEvent::pid`, `ProcessInfo::pid`) is a synthetic counter this
struct assigns, honestly smaller in scope than the field name suggests —
documented as such rather than presented as a real kernel PID. A `Start`
call registers itself before the guest command runs and a `Drop` guard
removes it on every exit path (success or error) once it ends, so `List`
reflects genuinely in-flight work rather than a permanently-empty table.
Verified live: started `/bin/sleep 3` with `tag: "my-sleep"`, called
`List` while it was running (got back its real config, synthetic pid 1,
and the tag echoed back), then called `List` again after it finished
(empty) — confirming both that entries appear and that they're actually
cleaned up, not just added once.

**`filesystem.Filesystem` — implemented and live-verified too, on the
same port as `process.Process`** (real envd serves both together;
`serve_for_sandbox` now does the same). `AgentVM` has no direct
file-read/stat primitive, so every RPC shells into the guest via
`exec_in_guest` and parses real coreutils/busybox output (`stat -c`,
`mkdir -p`, `mv`, `rm -rf`, `find`) — slower and more fragile than a real
envd's direct syscalls, and said so in the module's own doc comment rather
than presented as equivalent. `Stat`, `MakeDir`, `Move`, `ListDir`,
`Remove` are real; `WatchDir`/`CreateWatcher`/`GetWatcherEvents`/
`RemoveWatcher` are `unimplemented` (would need a long-lived guest-side
watcher this design has nowhere to report through yet).

Verified live, all five, against the real guest:

```
$ grpcurl ... -d '{"path":"/bin/sh"}' localhost:9000 filesystem.Filesystem/Stat
{"entry":{"name":"sh","type":"FILE_TYPE_SYMLINK","path":"/bin/sh","size":"12",
  "mode":511,"permissions":"777","owner":"UNKNOWN","group":"UNKNOWN",
  "modifiedTime":"2026-08-31T17:23:44Z","symlinkTarget":"/bin/busybox"}}
# real data: /bin/sh really is a symlink to /bin/busybox in this initramfs.
# owner/group "UNKNOWN" is honest too -- this minimal guest has no
# /etc/passwd for stat to resolve a UID against, and busybox's stat says so
# rather than guessing.

$ grpcurl ... -d '{"path":"/tmp/testdir"}' localhost:9000 filesystem.Filesystem/MakeDir
{"entry":{"name":"testdir","type":"FILE_TYPE_DIRECTORY", ...}}

$ grpcurl ... -d '{"path":"/tmp","depth":1}' localhost:9000 filesystem.Filesystem/ListDir
{"entries":[{"name":"testdir", ...}]}

$ grpcurl ... -d '{"source":"/tmp/testdir","destination":"/tmp/moved"}' localhost:9000 filesystem.Filesystem/Move
{"entry":{"name":"moved","path":"/tmp/moved", ...}}

$ grpcurl ... -d '{"path":"/tmp/moved"}' localhost:9000 filesystem.Filesystem/Remove
{}

$ grpcurl ... -d '{"path":"/tmp/moved"}' localhost:9000 filesystem.Filesystem/Stat
ERROR: Code: NotFound   # confirms Remove actually removed it, not just returned {}
```

**A real bug found and fixed by watching this fail, not by inspection.**
`MakeDir` first failed with a confusing "stat failed: exit Some(1), stderr
\"\"" on a directory that demonstrably existed. The `Stat` shell script
ends with `readlink path` to check for a symlink target — and `readlink`
exits non-zero *by design* when the path isn't a symlink, which is the
overwhelmingly common case. Since it was the script's last command, its
"not a symlink" exit code silently became the whole script's reported
exit status, misread by the code as "the stat itself failed." Fixed with
`readlink ... || true`. Two other bugs this same session, both found the
same way (running the thing rather than reading the code and assuming it
worked): `tonic_prost_build`'s generated method for the proto RPC named
`Move` is `r#move` (a raw identifier — `move` is a Rust keyword), not the
`move_` guessed at first; and `filesystem.proto`'s
`google.protobuf.Timestamp` import needed both an explicit `prost-types`
dependency and protoc's own well-known-types include directory (shipped
inside its release archive, not obvious from a bare `protoc` binary
fetched standalone — documented in `build.rs` via `PROTOC_WELLKNOWN_INCLUDE`
for the next person who hits the same "File not found" error).

**The two halves are now joined — done, live-verified.** The service
implementation moved out of the standalone example and into the crate
itself (`hv2_api::envd_process`, with `serve_for_sandbox(vm, addr,
shutdown_rx)`), shared by both `envd_process.rs` (still the standalone,
one-VM case) and `e2b_compat.rs`. `POST /sandboxes` now also spawns a
per-sandbox `process.Process` gRPC listener on its own port (starting at
9000, incrementing) and returns it as a non-standard `processPort` field
— not a real E2B field (real E2B routes to a per-sandbox envd through a
shared proxy keyed by domain, not built here), but a real, working port.
`DELETE /sandboxes/{id}` shuts that listener down via a oneshot channel.

Verified the full loop live:

```
$ curl -X POST localhost:3980/sandboxes -d '{"templateID":"base"}'
{"templateID":"base","sandboxID":"sbx_...","clientID":"sbx_...","envdVersion":"hv2-guest-agentd/0 (not envd)","processPort":9000}

$ grpcurl -plaintext -import-path crates/hv2-api/proto -proto process.proto \
    -d '{"process":{"cmd":"/bin/sh","args":["-c","echo integrated test via real grpc port; hostname"]}}' \
    localhost:9000 process.Process/Start
{"event":{"start":{}}}
{"event":{"data":{"stdout":"aW50ZWdyYXRlZCB0ZXN0IHZpYSByZWFsIGdycGMgcG9ydAoobm9uZSkK"}}}
{"event":{"end":{"exited":true,"status":"exited"}}}
# decodes to: "integrated test via real grpc port\n(none)\n"

$ curl -X DELETE localhost:3980/sandboxes/sbx_...
HTTP 204

$ grpcurl -plaintext ... localhost:9000 process.Process/Start
Failed to dial target host "localhost:9000": connection refused
# -- confirms the per-sandbox listener actually tore down with its VM,
#    not left dangling
```

One real bug found and fixed in the process: the first pass serialized the
new field as `process_port` (Rust's default), not the documented
`processPort` — missing `#[serde(rename = "processPort")]`. Caught by
actually reading the curl response rather than assuming the struct
definition matched the doc comment above it.

**Found and fixed along the way, unrelated to this crate's own code:**
`hv2-api`'s gRPC build script needs `protoc`, absent on this dev machine —
installed from the upstream release zip into `~/.local/bin`, no root
needed, since this particular `sudo` did not have a live credential and a
non-interactive shell has no way to supply a password. Same approach for
`grpcurl`, needed only for this verification, not a runtime dependency.

**Exit criterion (still open, but closer):** an existing E2B SDK client
(Python or JS, unmodified) successfully runs against a HyperMachine-backed
endpoint by changing only its base URL. `Start`, `List`, and the core
`filesystem.Filesystem` RPCs (`Stat`/`MakeDir`/`Move`/`ListDir`/`Remove`)
are now real and live-verified, on one per-sandbox port the control plane
wires up automatically. What's left: `Connect`/`Update`/`StreamInput`/
`SendInput`/`SendSignal`/`CloseStdin` (all need a way to reach an
already-running process, which the current one-shot `exec_in_guest` model
doesn't support), watch RPCs, live streaming instead of batched events,
and — the part that actually blocks trying a real SDK, not just
`grpcurl` — routing by domain the way E2B's own `CubeProxy`-equivalent
would, since `processPort` is not a field any real SDK looks for.

### Phase 2 — Snapshot/clone (CubeCoW-equivalent)

`hv2-core` already manages VM memory regions; the gap is a reflink/CoW-style
snapshot of guest memory + rootfs comparable to `FICLONE`-based zero-copy
clone. This is squarely a "build it" phase, not a "wire two things together"
one — size it properly once Phase 0's numbers exist, since snapshot restore
latency is itself a cold-start number that competes with CubeSandbox's.

### Phase 3 — Network security (CubeVS/CubeEgress-equivalent)

`hv2-net`'s NAT module (built this week) is userspace and unaware of policy.
An eBPF-based per-sandbox conntrack + policy layer, and an L7 egress proxy
with credential injection (so a sandboxed agent's outbound API calls never
see the real secret), are both real engineering efforts with no existing
HyperMachine scaffolding to build on — flagged as the least-started phase in
this roadmap, not sequenced first for that reason.

### Phase 4 — Multi-node cluster orchestration

Extend `hv2-runtime`'s VM-pool/scheduler concept from one host managing a
pool to a stateless control plane coordinating multiple hosts (CubeMaster's
actual shape: Redis-backed lifecycle events, any control-plane instance
serves any request). `hv2-runtime`'s 8 tests are the thinnest coverage of
any crate cited as a strength in this doc — expect this phase to surface
real gaps between what its doc comments claim and what it does, the same way
booting `hv2-unikernel` surfaced things `hv2-net`'s tests alone couldn't.

### Phase 5 — Density, ops tooling, deployment convenience

WebUI, K8s/Terraform deploy, ARM64 — genuinely important for adoption, and
deliberately last: none of it matters if Phases 0–1 don't already make this
worth deploying at all.

## What this roadmap deliberately does not do

- It does not commit to building all five phases — that is a resourcing
  decision for whoever owns HyperMachine's priorities, not something a
  planning document should presume.
- It does not touch any code. No crate, no benchmark harness, no E2B shim
  exists as a result of writing this file.
- It does not resolve the AGPL/Apache-2.0 licensing question — flagged
  above as a real strategic input, decided by whoever owns that call.
