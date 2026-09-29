# Beating CubeSandbox and Agent Substrate: a feature and performance roadmap

Status: **Phases 0-6 built and verified, except arm64 execution.** Phase 0 benchmarked honestly
before anything was promised; Phase 1 met its exit criterion, an unmodified
E2B SDK client running against a HyperMachine endpoint by changing only where
it points; Phase 2 moves a guest between VMs through a file and restores
faster than that guest boots; Phase 3 gives a sandbox a network whose every
connection and DNS query passes E2B's own egress policy, with credentials
injected on the host so the guest never holds them; Phase 4 runs many hosts
behind stateless control planes coordinating through Redis/Valkey; Phase 5
creates a sandbox by restoring a template instead of booting one, in tens of
milliseconds and a few MiB each, and ships images, a Helm chart, Terraform,
Compose, metrics and a web UI; Phase 6 answers Google's Agent Substrate
with E2B's pause, resume, auto-pause, traffic-driven resume and fork, and
runs 240 stateful sandboxes on 8 VM slots. Each
section below carries its own status and the measurements behind it.

`e2b_compat`, which the Phase 1-3 sections run, is now the `hv2-sandboxd`
crate: same code, same flags, `cargo run -p hv2-sandboxd` instead of
`--example e2b_compat`.

This began as a planning document -- the line here read "nothing built as a
result of it yet" until 2026-09-22, long after that stopped being true.
Written
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

### Phase 1 — E2B compatibility layer (highest leverage-to-effort ratio) — **exit criterion met**

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

**Every process RPC but `Update` is now real,** because the guest agent
gained a way to start a program and *keep* it (`Operation::Start`, protocol
version 2) instead of only running one to completion. `Start` streams output
as it arrives, `Connect` reattaches to a running process by pid or tag,
`SendInput`/`StreamInput` write to its stdin, `CloseStdin` sends EOF, and
`SendSignal` signals it. `pid` is the guest's own pid throughout, not a
synthetic counter.

**What's still simplified, stated plainly:** output is *polled* from the
guest every 50ms rather than pushed, so "as it arrives" means within that
interval. A `Connect` sees events from the reattach onwards — nothing keeps
scrollback. `Update` is PTY resize and stays `unimplemented`: the agent gives
a program pipes, so there is no terminal with a size to change, and for the
same reason `DataEvent::pty` is never emitted and `ProcessInput::pty` is
refused. Per-process environment variables are refused rather than silently
dropped.

Live-verified against a booted guest: a `/bin/sh` read loop started over
gRPC, fed by `SendInput` and `StreamInput`, watched simultaneously by the
`Start` stream and a `Connect` stream reattached by tag (both saw the same
output), ended by `CloseStdin`; and a `/bin/sleep 300` killed by
`SendSignal`, reported as `exitCode: 137, status: "signalled"`.

**`List` is real — backed by an actual table of running processes.**
`StartEvent::pid` and `ProcessInfo::pid` are the guest's own pids now, which
is what makes them usable as selectors for the other RPCs. A `Start`
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
`Remove` are real, and so are `WatchDir`/`CreateWatcher`/
`GetWatcherEvents`/`RemoveWatcher`, once the guest agent could keep a
long-lived process: a watcher is an `sh` loop in the guest printing a
`find`+`stat` snapshot whenever the tree differs from the previous one,
and the host diffs consecutive snapshots into events. That is a poll, not
`inotify` — this guest's busybox has no `inotifyd` — with the costs stated
in the module's doc comment: changes faster than the interval are
collapsed, and a rename reads as a remove plus a create, so
`EVENT_TYPE_RENAME` is never emitted. `allow_network_mounts` is ignored,
and correctly so: it exists because `inotify` is unreliable on NFS/CIFS,
which polling `find` is not.

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
— not a real E2B field, but a real, working port. Real E2B routes to a
per-sandbox envd through a shared proxy keyed by domain; that is
`hv2_api::sandbox_proxy`, added later and described under the exit
criterion below, so `processPort` is now the direct route and the hostname
is the one an SDK would use.
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

**Exit criterion — met.** An existing E2B SDK client (Python, unmodified)
runs against a HyperMachine-backed endpoint by changing only where it
points. The run, and the two caveats it came with, are below.

Every `process.Process` RPC but `Update`, every `filesystem.Filesystem` RPC
including all four watch RPCs, and domain routing are built and
live-verified. `hv2_api::sandbox_proxy` terminates HTTP/2 on one port,
reads the authority the client asked for, and forwards to the sandbox's own
listener — it has to terminate rather than splice, because the authority
lives in an HPACK-compressed HEADERS frame that no TCP-level forwarder can
read. `e2b_compat` wires it up, with TLS and `h2` over ALPN, which a gRPC
client will not negotiate without.

#### What happened when a real SDK was finally pointed at it

The E2B Python SDK (`e2b` 2.50.0, unmodified, from PyPI) was run against
`e2b_compat` with `E2B_API_URL` and `E2B_DOMAIN` set and nothing else
changed. `Sandbox.connect()` succeeded. Every call after it failed, and not
for any of the reasons listed above.

**The SDK does not speak gRPC.** It speaks the **Connect protocol**, over
HTTP/1.1. Captured from the wire, byte for byte:

```
POST /filesystem.Filesystem/ListDir HTTP/1.1
  user-agent: connectrpc/0.11.1
  connect-protocol-version: 1
  content-type: application/json
  e2b-sandbox-id: sbx_test
  e2b-sandbox-port: 49983
  [body 25 bytes] b'{"path": "/", "depth": 1}'

POST /process.Process/Start HTTP/1.1
  user-agent: connectrpc/0.11.1
  content-type: application/connect+json
  transfer-encoding: chunked
```

The service and method names match ours exactly — the protos are right. But
a unary call is an ordinary HTTP/1.1 POST carrying **protobuf-JSON**, and a
streaming call uses Connect's own enveloped framing under
`application/connect+json`. `tonic` serves `application/grpc`: HTTP/2,
length-prefixed binary protobuf. The two do not interoperate, and the SDK
says so itself when handed a gRPC-shaped reply:

```
Code.UNKNOWN: invalid content-type: 'application/json';
              expecting 'application/connect+json'
```

So the thing that blocked the exit criterion was a Connect-protocol
surface, not domain routing and not any missing RPC. **That is now built**
(`hv2_api::connect`), and the criterion is met — see below.

**Two real bugs in the proxy, both found by this and both fixed.**

First, the SDK addresses a sandbox as plain `localhost:49983` in its
default mode and puts the identity in `e2b-sandbox-id` /
`e2b-sandbox-port` headers — there is no hostname to route on at all.
`sandbox_proxy` now routes on those headers when present, falling back to
the `{port}-{sandboxID}.{domain}` authority.

Second, and worse: **the proxy only spoke HTTP/2 with prior knowledge.**
Its own comment said so, reasoning that "gRPC clients send the h2 preface
directly, and there is no h1 traffic to this port". That reasoning was
wrong about the one client that matters. Every SDK request was rejected at
the connection preface, before any routing ran, and the only trace was a
debug line reading `http2 error`. It now uses hyper's auto builder and
sniffs the preface, serving either.

Neither would have been found by reading the code — the header routing
added *before* this test was written would have looked correct and never
once run. With both fixed, routing is verified against the real SDK:

| request | result |
| --- | --- |
| no headers, unroutable host | `400` — nothing to route on |
| `e2b-sandbox-id` of a live sandbox | `200` — reached its listener |
| `e2b-sandbox-id: sbx_nope` | `404` — no such sandbox |

and the SDK's own call now gets all the way to the sandbox's real service,
where exactly one thing is left to disagree about:

```
Code.INTERNAL: invalid content-type: 'application/grpc'; expecting 'application/json'
```

That single line is the whole remaining gap, isolated.

#### The Connect surface, and the criterion met

`hv2_api::connect` is a second surface over the *same* service objects, not
a second implementation: `EnvdProcess` and `EnvdFilesystem` clone into
shared state, so a process started over gRPC is visible to a `List` over
Connect. gRPC and Connect share every path, so the dispatch is by
content-type — `application/grpc*` to tonic, Connect's own media types to
this — on one port per sandbox, over HTTP/1.1 or HTTP/2 as the client
prefers.

Three pieces were needed. Protobuf-JSON, which is a specified mapping and
not what `#[derive(Serialize)]` produces (lowerCamelCase fields, 64-bit
integers as strings, enums by name, `Timestamp` as RFC 3339); `pbjson-build`
generates it from the descriptor set the tonic build already had to emit.
Connect's error shape, where the HTTP status and the code in the body both
have to be right. And its streaming envelope — a flags byte and a
big-endian length — whose last frame carries the end-of-stream metadata,
which is where a stream that fails *after* its first message has to report
it, the status having been sent long before.

**The exit criterion, run:** `e2b` 2.50.0 from PyPI, unmodified, against
`e2b_compat`:

```text
OK   files.make_dir('/tmp/sdk'): True
OK   files.list('/tmp'): [EntryInfo(name='sdk', type=<FileType.DIR: 'dir'>,
       path='/tmp/sdk', size=40, mode=493, permissions='755',
       modified_time=datetime.datetime(2026, 9, 16, 20, 1, 47, tzinfo=utc))]
OK   files.exists('/tmp/sdk'): True
OK   commands.run('echo hello from the sdk'):
       CommandResult(stderr='', stdout='hello from the sdk\n', exit_code=0)
OK   files.remove('/tmp/sdk'): None
OK   background start: pid 80     # Start, server-streaming
OK   send_stdin: None             # SendInput
OK   kill: True                   # SendSignal
```

and the two calls that raise, raising correctly: a command exiting 3 with
output on stderr becomes `CommandExitException: Command exited with code 3`
with the stderr attached, and listing a path that is not there becomes the
SDK's own typed `FileNotFoundException` — which found a real bug on the way,
since `ListDir` had been reporting a missing directory as `internal`.

**And again over the path a deployment actually uses.** The run above set
`E2B_DEBUG=true`, which makes the SDK talk to `http://localhost:49983` and
skip both TLS and hostname routing. Repeating it without that — a CA and a
leaf for `*.hm.local`, the proxy on 443 with the leaf, `E2B_DOMAIN` set, the
SDK told to trust the CA through `SSL_CERT_FILE` — worked:

```text
connected: sbx-18d5ec42a320f4e4
host it will use: 49983-sbx-18d5ec42a320f4e4.hm.local
OK   files.make_dir('/tmp/tls'): True
OK   files.list('/tmp'): [EntryInfo(name='tls', type=DIR, path='/tmp/tls', ...)]
OK   commands.run('echo over tls'): CommandResult(stdout='over tls\n', exit_code=0)
```

Getting there cost four real defects, none of which the debug-mode run could
have shown:

- **`POST /sandboxes/{id}/connect` did not exist.** The SDK calls it on
  every `Sandbox.connect()` outside debug mode. Added, answering with the
  same descriptor `POST /sandboxes` returned rather than a second one
  assembled from parts.
- **Error bodies were not JSON.** The SDK's generated client parses the body
  of every non-2xx reply *before* it looks at the status, so a missing route
  surfaced as a `JSONDecodeError` from inside its parser, naming nothing.
  Every error, including the router's fallback, is now
  `{"code", "message"}`.
- **`envdVersion` has to be a version.** The honest string this returned
  — `"hv2-guest-agentd/0 (not envd)"` — made `Sandbox.connect()` raise
  `InvalidVersion` before doing anything. It is now `0.6.3`, with the
  capability table that number claims, and which parts of it are real,
  written out beside the constant.
- **Sandbox ids contained an underscore,** which is not legal in a DNS
  label. `sbx_...` produced a hostname the SDK's own resolver refused:
  "Label contains invalid characters". Now `sbx-...`; the proxy splits on
  the first hyphen, so the rest is harmless.

**What still differs from a deployment:** the SDK wraps every command in
`/bin/bash -l -c` and the test initramfs has busybox only, so a `/bin/bash`
wrapper script was added to that fixture; a real template ships a real bash.
There is no wildcard DNS here either, so the one hostname the run needed
went in `/etc/hosts` rather than being resolved.

One deviation worth knowing about: `pbjson-types` renders a `Timestamp` as
`1970-01-01T00:01:40+00:00` where protobuf-JSON's spec says `Z`. Both are
the same instant and RFC 3339, the SDK parses it into a `datetime`
correctly, and a stricter protobuf-JSON parser could still object.

#### Terminals

`Start` with a `pty` now opens a real pseudo-terminal in the guest rather
than pipes, so the program has a controlling terminal: it line-buffers,
draws prompts, answers `isatty`, and takes Ctrl-C as a signal rather than a
byte. Its output arrives as `DataEvent::pty`, because a terminal has one
stream and splitting it into stdout and stderr would mean inventing the
split. `Update` resizes it, `ProcessInput::pty` types into it, and
`CloseStdin` on one sends Ctrl-D, which is what the proto's own comment
says to do.

Built from `posix_openpt`/`grantpt`/`unlockpt`/`ptsname_r` rather than
`openpty`, which lives in libutil: the agent links static-pie against glibc
for a guest with no shared libraries, and one more library to link is one
more way not to link at all. The child gets `setsid` and `TIOCSCTTY` before
`exec` — without both it has a terminal on its descriptors but no
*controlling* terminal, so Ctrl-C signals nothing and a shell reports "no
job control".

`ProcessConfig::envs` is passed through too, which the same exercise
forced: the SDK sets `TERM`, `LANG` and `LC_ALL` on every pty it opens, so
refusing environment variables meant refusing every terminal. They are
added to the agent's environment rather than replacing it — a program
started with an empty one has no `PATH`.

Verified through the SDK's own `pty` API, not just grpcurl:

```text
OK   pty.create: pid 54
     ... BusyBox v1.37.0 built-in shell (ash) | ~ # tty; stty size; echo TERM=$TERM
     | /dev/pts/0 | 30 100 | TERM=xterm-256color | ~ #
OK   pty.resize
     ~ # stty size | 50 200 | ~ #
OK   pty.kill: True
```

The banner, the `~ #` prompt and the echoed command are all things that
only happen on a terminal; `/dev/pts/0` is the terminal itself; `30 100`
then `50 200` is the running shell seeing the resize.

Remaining gaps: output that is polled rather than pushed, and compression —
`connect-accept-encoding` is ignored and nothing is compressed, which the
protocol allows and which costs bandwidth on a large `ListDir`.

### Phase 2 — Snapshot/clone (CubeCoW-equivalent) — **works, and is now faster than booting**

A guest can be written to a file and restored into a different VM, which then
resumes where the first one left off. `VM::snapshot` and `VM::restore`, over
`snapshot::file`'s format; `HypervisorBackend::save_vcpu`/`restore_vcpu` with
a KVM implementation underneath. Verified across two VMs, with a marker
written into the first guest's memory so the comparison is not a tautology:

```text
marker        : "this guest was snapshotted, not booted" at 0x3000000
second VM     : booted on its own in 8.7 ms
restore       : the snapshot is now this VM, in 92.4 ms
vcpu 0        : rip=0x105731 — the instruction the snapshot was taken on
memory        : snapshot 0x1811fb18fc2c65d9
                before   0x4de186574ba3bb1b  (this VM's own boot)
                after    0x1811fb18fc2c65d9
marker        : found in the second VM
```

This phase was described above as needing sizing, "since snapshot restore
latency is itself a cold-start number that competes with CubeSandbox's". It
is now sized, and the sizing drove three optimisations — two of which taught
the opposite of what was expected:

| change | file | restore | what it showed |
| --- | --- | --- | --- |
| everything written | 64.0 MiB | 92.4 ms | — |
| sparse: skip all-zero pages | 0.1 MiB | 70.7 ms | file I/O was never the cost |
| coalesce runs of absent pages | 0.1 MiB | 64.0 ms | nor was per-call overhead |
| skip zeroing what is already zero | 0.1 MiB | **18.2 ms** | it was *allocating* pages |

The first change was the obvious one and produced a 640x smaller file for a
24% saving, which says plainly that the bytes on disk were not the problem.
The third is the one that mattered: a restore has to make the destination's
memory match the snapshot, and the destination has *already booted*, so most
of its pages are untouched anonymous memory the kernel still maps to one
shared zero page. Writing zeroes over them forces the kernel to allocate a
real page for each; reading them first costs almost nothing and finds that
nearly all are already correct. 64 MiB of writes became 64 MiB of reads plus
a handful of writes.

| change | file | restore | what it showed |
| --- | --- | --- | --- |
| discard the pages instead of reading them | 0.1 MiB | **1.8 ms** | the cheapest read is the one not done |

| | |
| --- | --- |
| boot this guest from its ELF | **10.2 ms** |
| restore it from a snapshot | **1.8 ms** |

Restore is now **5.7x faster than booting** this guest, where it had been
1.8x slower. The fourth change came from asking why the read pass existed at
all: it read the destination's 64 MiB to discover the pages were already
zero, which is the answer the *kernel* could have given for free.
`madvise(MADV_DONTNEED)` on the guest's `MAP_PRIVATE | MAP_ANONYMOUS`
mapping frees every page, and the documented behaviour for such a range is
that the next access gets a zero-fill page. One syscall replaces 64 MiB of
reads, and it un-allocates the guest's footprint on the way through.

Measured as a paired A/B, arms alternating within each iteration because the
host was busy and absolute numbers moved between runs. Five pairs, and the
two arms do not overlap:

| arm | runs (ms) | median |
| --- | --- | --- |
| discard | 1.2, 1.8, 1.6, 1.8, 1.5 | **1.6** |
| read-before-write | 23.1, 23.3, 23.4, 35.8, 24.3 | **23.4** |

(Both inflated by load; the 1.8 and 18.2 in the tables above come from quiet
runs where boot measures 10.2--10.4 ms, so those are the comparable pair.)

### The version of this that would have been wrong

The note here used to say that
`HypervisorBackend::guest_memory_starts_zeroed` "already answers this
question and nothing asks it", so the read pass could simply be skipped. It
could not, and the reason is worth keeping:

- That method reports a property of the *allocation path* -- memory was zero
  when handed over. Its own doc says a backend restoring into existing memory
  "would have to answer differently". Restore is exactly that case.
- `restore` requires the VM to be paused, so it has necessarily run and
  dirtied memory. The win only exists for a VM that never ran, which the API
  did not allow.
- Tracking "nothing has written here yet" does not close the gap either:
  `MemoryRegion::host_addr` is a public field written through directly
  elsewhere, and a running guest dirties pages without passing through any
  Rust path. An invariant a public field can break is the wrong foundation
  for a memory-correctness decision.

`mincore(2)` looks like a sound alternative -- ask which pages are resident
rather than reading them -- and is not: a page that was written and then
swapped out is non-resident and non-zero. It would corrupt guests rarely and
silently.

What makes the implemented version safe is that it does not infer anything.
It performs the zeroing and reports that it did, so the caller's decision to
skip work rests on an action rather than an assumption about history. The
backend trait method is `reset_guest_memory_to_zero`, returning `false` when
a backend cannot do it, in which case the read-before-write path above still
runs. Restore also declines the fast path entirely if any region is
read-only, since the loop skips those and would leave them discarded --
no such region exists today, which is why it is a check and not a comment.

The useful reading is about *which* workloads this is for. Snapshots win
where boot is slow and the image is warm — a loaded language runtime, a
primed interpreter, a model already in RAM — which is exactly the case
`memory_cow.rs` describes and the reason CubeSandbox has CubeCoW at all. They
lose on a 64 MiB unikernel that boots in milliseconds. Anyone reaching for
this to make *this* guest start faster is reaching for the wrong tool, and
the number says so rather than leaving it to be discovered.

What would move it further: at 1.8 ms the remaining cost is no longer the
memory at all -- it is reading the file and restoring vCPU and device
state. Compression is a different trade (CPU for I/O);
`MemorySnapshotConfig` describes it and nothing applies it, and at this
size it would likely cost more than it saves.

Host-side device state travels too. The rings live in guest memory and go
with it; what the *device* holds — where those rings are, how big they are,
what features the driver agreed to, and how far the device has read and
written — is captured per virtqueue, along with the transport's status and
negotiation registers. A guest restored without them finds a device that has
forgotten the conversation they were in the middle of. Verified by comparing
the second VM's device state against the file field by field, not by the
restore call returning without error:

```text
devices       : 1 captured, device_state_included=true
                'virtio-vsock': status=0xf features=0x100000000, 3 queue(s)
                  queue 0: ready=true size=8 desc=0x200000 avail_idx=0 used_idx=0
                  queue 1: ready=true size=8 desc=0x203000 avail_idx=0 used_idx=0
                  queue 2: ready=false size=0 desc=0x0 avail_idx=0 used_idx=0
devices       : 1 restored — identical to the snapshot
```

What is still not captured is what a device knows about the world *outside*
the VM: a vsock device's connection table, a net device's queued frames.
Those point at a host socket or a host link that no longer exists when the
snapshot is restored, so they are dropped rather than half-restored — a guest
that had a vsock connection open finds it gone. A PCI-attached vsock is also
skipped, and says so in a log line, because its configuration lives in
guest-visible BAR space rather than in host-side registers. The vCPU capture now includes the model-specific registers a guest
notices losing — `SYSCALL`'s entry point and flag mask (`STAR`/`LSTAR`/
`CSTAR`/`SFMASK`), the `FS`/`GS` bases, the `SYSENTER` trio and `PAT` — which
is what a 64-bit Linux guest needs to survive its next system call. Proven by
writing a marker into `LSTAR` and reading it back, because every MSR this
unikernel reports is zero and "11 captured" is equally true of eleven zeroes
and of an ioctl that does nothing.

The local APIC's register page and the XSAVE area come too, so a restored
guest keeps its armed timer and its AVX registers. Wiring XSAVE turned up a
latent bug: `KVM_SET_XSAVE` was defined as `0x5000aea3`, whose `nr` of `0xa3`
is `KVM_ENABLE_CAP` — nothing had ever called it, so it had never had the
chance to fail. Putting the old value back makes the restore fail with
`Invalid argument (os error 22)`, which is how the fix was confirmed rather
than argued.

One thing is still omitted, and it is a decision rather than a gap: the TSC.
Restoring it jumps the guest's clock by however long the snapshot sat on disk;
not restoring it jumps the clock to the host's uptime. Both are wrong, and
choosing needs a caller who knows what the guest does with time.
`VCpuSnapshot::is_complete` therefore still answers `false`, and `missing()`
names that one item.

### Phase 3 — Network security (CubeVS/CubeEgress-equivalent) — **built, on the live path, verified with the real SDK**

A sandbox can now have a network, and every packet of it passes a policy.
`hv2-sandboxd --network` gives each sandbox a virtio-net NIC whose far end is
`hv2_net::gateway::Gateway`: a userspace TCP/IP stack (smoltcp) that is the
guest's router (10.0.2.2) and DNS server (10.0.2.3), ends every TCP connection
the guest opens, and opens the corresponding one from the host only if the
sandbox's policy allows it. The guest configures itself from the kernel
command line (`ip=`, `CONFIG_IP_PNP`), so no guest software changed. Nothing
needs privilege: no TAP device, no `CAP_NET_ADMIN`, no host firewall rules.

Without `--network` a sandbox still has no NIC at all, and that remains the
default.

**The policy is E2B's own API**, not a new one: `allow_internet_access`,
`network.allowOut` (CIDRs, addresses, `name`, `*.name`), `network.denyOut`
(addresses only, as the spec says), `network.rules[name].transform.headers`,
`network.egressProxy` (SOCKS5), and `PUT /sandboxes/{id}/network` to replace
it on a running sandbox. `hv2_net::network_policy` implements the spec's
semantics -- allow beats deny, wildcards cover subdomains but not the apex,
exact rules beat the longest matching wildcard, rule sets are not merged.

**Verified with the unmodified E2B Python SDK** (`e2b` 2.51.0 from PyPI),
against a real booted guest and the real internet:

```text
created: sbx-18d904800277d754
OK   injected header seen upstream: from-the-host
OK   allowed name: 200
OK   refused name: curl: (6) Could not resolve host: www.google.com
OK   after update_network, google: 200
OK   after update_network, example.com: curl: (6) Could not resolve host: example.com
OK   allow_internet_access=False: curl: (6) Could not resolve host: example.com
```

The first line is the credential-injection control CubeEgress sells: the
sandbox asked for `https://postman-echo.com/headers` with a placeholder
`Authorization`, and the upstream received the host-held value -- while
`curl` in the guest *verified* the certificate it was shown, against the
sandbox CA the gateway installed. `grep` for the secret anywhere in the
guest finds nothing.

#### How a connection is decided

At the guest's SYN, from the address where that is enough:

- **Refused** -- a reserved address, a `denyOut` address with no name rule
  that could override it, or the default: no socket is created and the stack
  answers with a RST. The guest sees "connection refused" in microseconds.
- **Allowed by address**, or because *this gateway's own DNS* answered an
  allowed name with that address: the host dials first with the guest's SYN
  held unanswered (smoltcp's `pause_synack`), so an upstream's refusal reaches
  the guest as its own RST rather than a connection that opens and dies.
- **Needs a name**: the handshake completes and the first bytes are read for
  a TLS SNI or HTTP `Host`. The name must be allowed *and* resolve, from the
  host, to the address the guest connected to.

Five things in that design are there because the obvious version is
exploitable, and each has a test that fails without it (checked by removing
the guard and watching it fail, not by reading the test):

1. **DNS is egress.** `secret.attacker.example` carries its payload in the
   question. A name the policy would not let the sandbox reach is answered
   `REFUSED`, not resolved -- so a deny-by-default sandbox cannot exfiltrate
   through lookups either.
2. **A name never opens a reserved address.** Loopback, RFC 1918, CGNAT,
   link-local (including `169.254.169.254`, every cloud's metadata service)
   need an explicit `allowOut` CIDR. An allowed name that resolves to
   `10.0.0.5` is DNS rebinding, not a trip to that name.
3. **The guest chooses both the address and the name.** An allowed `Host`
   aimed at an address it does not resolve to is reset.
4. **A shared CDN address is not a shared allowance.** On an address opened
   by the gateway's DNS answer for an allowed name, a TLS ClientHello naming
   a refused one is reset.
5. **A credential only goes where the rule says.** Over HTTPS the upstream
   must prove the name with a certificate from the public web PKI. Over plain
   HTTP there is no proof, so injection happens only if the address is where
   the name resolves; otherwise `Host: api.vendor.example` sent to the
   guest's own server would collect the credential.

Also deliberate: `egressProxy` fails closed (a proxy that is down or refuses
credentials fails the connection instead of letting it out directly), and a
transform rule on a server started without interception refuses the
connection rather than sending the request without the credential it was
meant to carry.

**Where this differs from E2B, on purpose.** A sandbox that configures
nothing gets the operator's `--egress-default`, which is `deny` unless told
otherwise; E2B's is allow. And an `egressProxy` that resolves into a private
range is refused unless the operator passes `--allow-private-egress-proxy`,
which matches E2B's own check.

#### A hole found on the way: envd had no authentication

Current SDKs create sandboxes through `POST /v2/sandboxes`, which is
secure-only: the response carries an `envdAccessToken` and the SDK sends it
back as `X-Access-Token` on every envd call. Nothing here issued or checked
one, which meant **anyone who could reach the proxy -- or the per-sandbox
envd port directly -- could run commands in any sandbox whose ID they
learned.** Each sandbox now gets a 244-bit token, compared in constant time
on the envd listener itself (not only in the proxy, which the listener does
not need to be reached through). Verified both ways: no token or a wrong one
is `401`, the right one `200`, through the proxy and directly.

#### Performance

Measured on this machine (nested KVM under WSL2; bare metal will differ),
64 MiB over HTTP from a host-side server:

| path | before | after |
| --- | --- | --- |
| download through the gateway | 15.5 MB/s | **130.6 MB/s** |
| TCP connect | 3.47 ms | **1.13 ms** |
| small HTTP request | 11.0 ms | **4.65 ms** |
| host-side baseline, no VM | 127-155 MB/s | -- |

The first number was a ceiling, and arithmetic said which one: 64 KiB of
send buffer over a 3.5 ms round trip is 16 MB/s. Two changes removed it. The
bridge stopped polling -- the device gained a transmit wake (fired from the
guest's own kick; the old doc comment said nothing could be signalled from
there, and nothing had to *block*, which is different) and the gateway
signals when it has frames, so a round trip no longer waits out two 1 ms
sleeps. And the send buffer is 256 KiB. Receive-side interrupts are also
coalesced now: one per burst rather than one per frame.

~~**Upload is still slow: 5.1 MB/s.**~~ **Corrected in Phase 6: upload was
never slow; the benchmark was.** It piped `dd` into busybox `nc`, which writes
about 1 KiB per system call -- the guest's own counters showed 1,080-byte
packets, each a kick, an interrupt and a legacy-PIC acknowledgement, ~12 VM
exits apiece. An HTTP POST with curl from the same guest, through the same
gateway, runs at **146-163 MB/s** with nothing else changed. What was changed
on the way, measured separately: the gateway now acknowledges at once
(smoltcp's default delays an ACK 10 ms) with a 256 KiB receive window, and
sandboxes' NICs offer checksum and TCP segmentation offload to a host side
that takes a segment of any size, which took uploads to **165-174 MB/s**
(~9%, 32 KiB segments instead of 1.5 KiB frames; `--no-net-offload` turns it
off). Download on the same runs: 370-485 MB/s either way.

#### What is still not built

- **eBPF per-sandbox conntrack** (CubeVS). The gateway does in userspace what
  CubeVS does in the kernel; at thousands of sandboxes per node the per-packet
  cost will matter, and this has not been measured at that density.
- **IPv6**, and **UDP other than DNS**: dropped. So is ICMP -- a guest `ping`
  gets no answer rather than one the gateway fabricated.
- **HTTP/2 and WebSocket inside injected connections**: interception offers
  only `http/1.1` in ALPN, which clients fall back to; an `Upgrade` request is
  not relayed.
- **busybox's built-in TLS client** cannot complete a handshake with the
  gateway's interception (it stalls after rustls's ServerHello; reproduced on
  the host with no VM involved). OpenSSL-based clients -- curl, Python, Node
  -- work. It also never validates certificates, so it was never going to
  prove the trust chain anyway.

#### History worth keeping

This section said "Written, and not yet on a live path" until 2026-09-26,
and before 2026-09-22 it opened "started, at the layer that was carrying the
traffic", which read as though `EgressPolicy` were running. It was not:
`Bridge` was constructed in exactly one place outside its own module, an
example. That wording travelled -- into `filter.rs`'s `#[deprecated]` notes,
`policies.rs`'s module header, and a project memory recording `permissions/`
as wired into a request path when nothing installed that middleware either
-- four copies, each reading like independent confirmation of the others.
Worth remembering when a security claim is easy to repeat and expensive to
verify.

The push came from [NVIDIA's sandboxing
guidance](https://developer.nvidia.com/blog/practical-security-guidance-for-sandboxing-agentic-workflows-and-managing-execution-risk/),
which puts blocking "outbound network access to unknown destinations" first
among its mandatory controls, and recommends full virtualization over
kernel-sharing for this workload because agentic tools "perform arbitrary
code execution by design". `hv2_core::networking::filter`, a packet filter
with connection tracking, is still called by no data path; the gateway is
the control that is.

### Phase 4 — Multi-node cluster orchestration — **built, verified on a live cluster with the real SDK**

CubeMaster's shape, and CubeMaster's store. Two new crates:

- **`hv2-sandboxd`** -- the node daemon. What was `hv2-api/examples/e2b_compat.rs`
  (moved with `git mv`, so its history follows), now with what a node needs:
  a capacity it enforces, E2B sandbox lifetimes, and cluster membership.
- **`hv2-cluster`** -- a shared store (Redis or Valkey; in-memory for tests
  and one host), the node agent, a scheduler, a reaper, and
  **`hv2-control-plane`**: E2B's API for the whole cluster, stateless.

```text
                     E2B SDK / any HTTP client
                 X-API-Key │           │ envd (X-Access-Token)
          ┌────────────────┴─┐   ┌─────┴────────────┐
          │ hv2-control-plane│ … │ hv2-control-plane│   any number, no state
          └────────┬─────────┘   └─────────┬────────┘
                   │  schedule, forward,   │  route envd to the owning
                   │  list from the store  │  node's proxy
          ┌────────┴─────── Redis / Valkey ┴────────────┐
          │ node:{id} (TTL) · sandbox:{id} · events      │
          └────────┬───────────────────────────┬────────┘
       heartbeat,  │ x-hv2-cluster-token        │
       records     │                            │
          ┌────────┴────────┐          ┌────────┴────────┐
          │  hv2-sandboxd   │   …      │  hv2-sandboxd   │  microVMs
          └─────────────────┘          └─────────────────┘
```

**Who is authoritative for what**, which is the design:

- A **node** owns its sandboxes. It writes a record when it creates one and
  deletes it when the sandbox ends -- by request, by timeout, by shutdown --
  and on restart it deletes whatever records a previous run left behind,
  because those VMs died with that process. The store says what nodes are
  running, not what a control plane last asked for.
- A node also owns **its capacity**. The scheduler's view of load is advice;
  a full node refuses a create with 503 and the control plane tries the next
  one. That is what lets any number of control planes schedule at once with
  no lock between them -- verified with two control planes racing for the
  last slot on each of two nodes.
- **Liveness is the store's.** A node's record has a TTL each heartbeat
  renews; a node that stops is simply absent. Control planes never judge
  staleness. The reaper, which may run on every control plane at once,
  removes records whose node is gone; whichever instance deletes a record
  first is the only one that reports it.
- A **control plane** owns nothing. Creates are scheduled and forwarded,
  per-sandbox calls go to the owning node, list and detail come from the
  store, and envd traffic is proxied to the owning node's proxy.

**Verified on a live cluster** -- Valkey, two `hv2-sandboxd` nodes (capacity
3, networking on) booting real microVMs, two control planes -- with the
unmodified E2B SDK (`e2b` 2.51.0):

```text
OK   4 sandboxes created via cp-a in 3.0s
OK   placement: {'node-a': 2, 'node-b': 2}
OK   commands.run on all 4 through cp-a's envd proxy
OK   via cp-b: 6.6.52                      # a sandbox cp-a created, run via cp-b
OK   Sandbox.list via cp-b: 4 sandboxes
OK   7th create refused: 503 ... no node has room (2 alive)
```

and then the failure cases, by hand:

| scenario | result |
| --- | --- |
| control plane A killed with SIGKILL | B still serves all 6 sandboxes |
| node API, no or wrong cluster token | 401 |
| control plane, no API key | 401 |
| node B killed with SIGKILL | gone from the store in 8 s (TTL 9 s); its 3 sandboxes reaped, 3 `sandbox-lost` events; connect to one: 404 |
| node A full after the loss | create: 503, until a sandbox ends |
| sandbox created with `timeout: 4` | gone at 6 s; `sandbox-expired` event |
| node A sent SIGTERM | ends its sandboxes and leaves the store at once, not at its TTL |

**Found by the tests, not by reading:**

- **Bursts all landed on one node.** A node reported its load only on the
  heartbeat -- every 3 s -- so for that long after each create it still
  looked empty, and four creates in a row went to the same node. A node now
  re-announces its load on every create and end; the heartbeat only renews
  liveness. The spread test failed before this and passes after.
- **A second proxy lost the route.** `sandbox_proxy` rewrites the authority
  to the backend's, which erases a route that was read from the hostname: a
  control plane's proxy in front of a node's proxy handed the node a request
  it could not route. Once a proxy has resolved a route it now stamps
  `e2b-sandbox-id`/`e2b-sandbox-port` onto the forwarded request.
- **Per-sandbox ports came from a `u16` counter** that would overflow after
  ~56,000 sandboxes and hand out ports still bound by earlier ones. Now the
  kernel picks, on loopback -- the proxy is the way in from anywhere else.
- **Sandbox IDs came from the clock.** Fine for one process, not for many
  nodes minting into one namespace. Now random.

**E2B surface added on the way**, because the SDK's list and detail models
need it: sandbox lifetimes (`timeout`, defaulting to the spec's 15 s on v1
and 300 s on v2, capped at 24 h), expiry, `POST /sandboxes/{id}/timeout`,
`connect` extending the lifetime ("TTL is only extended"), `GET /sandboxes`,
`GET /v2/sandboxes` with metadata/state filters and `x-next-token`
pagination, `GET /sandboxes/{id}` (`SandboxDetail`), and
`/v2/sandboxes/{id}/connect`.

**Why not `hv2-runtime`**, which this section proposed extending: its own
scaling table promises "Multi-host: per-host runtime, shared state store",
and its store is an in-process `BTreeMap` behind a lock whose external
backend is documented as "Placeholder for etcd, Postgres, S3". The shared
store is the whole of a stateless control plane, so there was nothing to
extend. The prediction above -- that this phase would find gaps between
`hv2-runtime`'s doc comments and its code -- held.

**Not built:** pause/resume and snapshots through the cluster API (Phase 2
works per VM; nothing moves a sandbox between nodes yet), templates (every
node boots the same kernel and initramfs), and TLS to the store (front it
with a private network or a TLS tunnel). A store outage stops creates and
routing -- running sandboxes keep running, and nodes re-announce when it
returns -- so production wants a replicated Redis/Valkey.

### Phase 5 — Density, ops tooling, deployment convenience — **built; arm64 execution is the gap**

#### Cold start and density: restore, don't boot

CubeSandbox's headline numbers are <60 ms cold start and <5 MB per sandbox,
and neither is achievable by booting Linux -- this repo's guest takes
640-960 ms to an answering agent. So a sandbox is now not booted: one
template guest boots when a node starts, is configured (resolver, egress CA),
and is snapshotted with its memory written as a raw, sparse image; every
sandbox after that is a fresh VM whose RAM is that image mapped
`MAP_PRIVATE` -- copy-on-write, shared through the page cache until the guest
writes.

| | measured here |
| --- | --- |
| restore to a running guest | **1.2 ms** (provision 0.8 + map 0.2) |
| restore to an answering agent | **12.9 ms** median (boot: 640-960 ms; ~50x) |
| `POST /v2/sandboxes` on a node, envd and gateway included | **~31 ms** |
| through a control plane to a node | **~25 ms** |
| `Sandbox.create()` from the unmodified E2B SDK, in Python | **59 ms** |
| memory per live sandbox (PSS, each having run a command) | **2.4 MiB** |
| template image on disk, 1 GiB guest | 77 MiB allocated |

On nested KVM under WSL2; bare metal will be faster, not slower. Against
CubeSandbox's published figures this is under both, on this box; the
comparison that would settle it is the two on one bare-metal host, which has
not been run.

Getting there took four things a Linux guest needs from a snapshot that the
Phase 2 unikernel never did, each found by a restored guest failing, not by
reading:

- **Interrupt controllers and the PIT.** The guest's interrupts go through
  the 8259 PICs and its tick is the PIT (its own `/proc/interrupts` says
  so); fresh ones deliver at vectors it never programmed. Captured now with
  `KVM_GET_IRQCHIP`/`_PIT2`, plus kvmclock.
- **XCR0.** The first AVX instruction after restore was `#UD`, and the kernel
  reported "Bad FPU state".
- **`IA32_XSS`.** Linux saves task FPU state with `XSAVES` in compacted form;
  with XSS at zero every `XRSTORS` faulted.
- **The clock.** The guest's clocksource is the TSC with kvmclock registered.
  `ClockOnRestore::Continue` restores both -- the choice Phase 2 deliberately
  left to the caller, which a sandbox daemon is.

And two that are about clones rather than snapshots:

- **Every clone had the same RNG.** Measured: five sandboxes restored from
  one template drew one distinct value from `/dev/urandom` between them.
  Linux reseeds on its own only every minute or so, and writing to
  `/dev/urandom` mixes without forcing it. A new guest-agent operation sets
  the clock and reseeds (`RNDADDENTROPY` + `RNDRESEEDCRNG`) with host entropy
  in one round trip; after it, five of five distinct. The daemon refuses a
  sandbox whose reseed fails rather than hand it out.
- **Page-cache sharing is a side channel** in principle -- the same property
  that makes density cheap -- as it is for every CoW-snapshot sandbox,
  CubeSandbox's included. Not mitigated here beyond `MAP_PRIVATE`.

Two defects that predated all of this, found by measuring it:

- **Every VM leaked.** Four delivery threads held a strong `Arc<VM>` while
  blocking on channels whose senders lived in that VM's devices, so no VM
  was ever dropped: deleting 20 sandboxes freed nothing. With weak
  references the daemon goes 20 → 68 → 28 MiB PSS across 0 → 20 → 0
  sandboxes.
- **Guest-agent calls polled at 5 ms, twice per request**: 10.3 ms for a
  ping to a warm guest. The vsock device signals delivery now; 3.7 ms, most
  of it VM exits under nested virtualisation.

#### Ops

- **Images** (`Containerfile.sandbox`): `control-plane` on distroless,
  non-root; `node` with the daemon, the guest kernel and the initramfs.
  Both build under Docker Desktop: 46 MB and 157 MB. The first node build
  compiled the kernel for twenty minutes and then failed to install it --
  `build-kernel.sh` did not create its output directory, which a host
  build never needed.
- **Guest image, reproducibly** (`tools/guest-image/`): `build.sh` assembles
  the initramfs from busybox, the static agent, and extras
  (`--extra SRC[:NAME]`), byte-for-byte reproducible (`cpio --reproducible`,
  `gzip -n`); `build-kernel.sh` builds Linux 6.6.52 from a checked-in
  defconfig with a fixed build identity. A kernel built by it on this machine
  is the same size as the one the measurements above used, boots, and restores
  to an answering agent in 11.9 ms median.
- **Kubernetes** (`deploy/helm/hypermachine-sandbox`): control planes, a
  node DaemonSet on labelled KVM nodes, Valkey, generated-once secrets, and
  NetworkPolicies so only control planes reach nodes and the store. Lints,
  and all rendered resources pass `kubeconform -strict` against 1.30. Not
  installed on a cluster.
- **Terraform** (`deploy/terraform`): an optional `.metal` EKS node group,
  labelled and tainted to match the chart, with a validation that refuses a
  non-metal type (an EC2 VM has no `/dev/kvm`). `terraform validate` passes
  against the AWS provider. Not applied.
- **Compose** (`deploy/compose`): store, control plane and one node on a
  single KVM host. Brought up under Docker Desktop, which passes `/dev/kvm`
  through: the node builds its template in the container in 1.1 s, a create
  through the control plane answers in 25 ms, and the Phase 6 SDK test --
  pause, resume, fork, auto-pause, auto-resume -- passes against it.
- **Metrics**: `/metrics` in Prometheus text on control planes (cluster
  gauges from the store, creates by outcome, create latency, reaps) and
  nodes (running, booting, capacity, template, creates, latency, ends).
- **Web UI**: `/ui` on any control plane -- nodes with load, sandboxes with
  create and delete, the event stream. Driven in headless Chrome against a
  live two-node cluster: loads with the key, lists both nodes, creates and
  deletes, and renders a sandbox whose metadata was
  `<img src=x onerror=...>` as that text, with no element created and no
  script run.

Also: `reqwest` in the workspace uses rustls on `ring` now. Its default,
native-tls, had brought OpenSSL in as a second TLS stack -- which the repo's
own "one TLS backend" commit meant to rule out -- and it was found because
it stopped an aarch64 build.

#### ARM64: compiles, does not run

Every crate in the sandbox stack builds for `aarch64-unknown-linux-gnu`. The
KVM backend is x86 in substance -- register and special-register ioctls,
CPUID, the PIC/IOAPIC irqchip, the bzImage loader -- so on an arm64 host it
now refuses at construction with the reason, rather than failing at its
first ioctl. What arm64 needs is a backend of its own: `KVM_ARM_VCPU_INIT`,
a GICv3 through `KVM_CREATE_DEVICE`, a device tree describing the
virtio-mmio devices, the arm64 `Image` boot protocol and PSCI. That is real
work and cannot be verified on the x86 machine this was written on.

### Phase 6 — Suspend, resume, fork and oversubscription (Agent Substrate) — **built, verified with the real SDK, across nodes**

Google's [Agent Substrate](https://github.com/agent-substrate/substrate)
(Apache-2.0, pre-1.0; on GKE for evaluation) is a second competitor with a
different idea than CubeSandbox's: most agents are idle most of the time, so
suspend an idle one to storage, give its worker to another, and resume it
on the next request. Its published figures: resume "sub-500ms", "over 500
suspend/resume activations per second", a demo of ~250 stateful actors on 8
pods (30x). Isolation is gVisor or Cloud Hypervisor; snapshots go to Cloud
Storage and restore on any worker; a router parks requests during
saturation rather than refusing them. Its GKE documentation also lists what
it does not do yet: EgressPolicy hostname and IP rules, GPUs, and keeping
open connections across a suspend.

Before this phase HyperMachine had no per-sandbox pause at all. Now, all
through E2B's own API:

- **Pause** (`POST /sandboxes/{id}/pause`): the sandbox is written to the
  node's disk as a *layered* snapshot -- only the pages it wrote since it was
  restored from its template, found from the host kernel's page map rather
  than by reading guest memory -- and its VM and slot are released. 7 ms; a
  paused sandbox costs ~2-5 MiB of disk and no memory.
- **Resume**: `connect` (answering 201, as E2B's does) or the deprecated
  `resume`. The template is mapped copy-on-write and the written pages are
  laid over it. 18-23 ms to an answering sandbox, same ID, same token, same
  processes: a background loop counting in the guest was still counting.
- **Timeout pauses instead of killing** (`lifecycle.on_timeout = "pause"`,
  E2B's `autoPause`), and **traffic resumes** (`auto_resume`): a request
  through the proxy for a paused sandbox resumes it and is then served.
- **Fork** (`POST /sandboxes/{id}/fork`, E2B's newest API): the source is
  checkpointed in place -- paused for the few milliseconds a layered
  snapshot takes -- and `count` sandboxes start from it, each with its own
  ID, token and gateway (the source's policy) and a reseeded RNG. Five
  forks in 50 ms. For multi-agent work this is the fan-out primitive: set a
  workspace up once, hand N agents an identical copy.
- **Oversubscription** (`--evict-idle-after SECS`): a full node pauses the
  sandbox idle longest among those with `auto_resume` and no request in
  flight, and parks a create or a resume for a slot rather than answering
  503. Slots are a FIFO semaphore, so a parked request is not overtaken.
- Paused sandboxes list with `state: paused` and filter by it, through a
  node or a cluster's control planes; the control plane rewrites `resume`
  and `fork` answers as it does `connect`'s. Prometheus has pauses,
  resumes, auto-resumes, evictions, forks and their latencies.

Measured on the same nested-KVM development box as every other phase, with
the unmodified E2B Python SDK (2.51) unless it says HTTP:

| | HyperMachine | Agent Substrate (published) |
| --- | --- | --- |
| resume a paused sandbox, alone | **18-23 ms** | "sub-500ms", "under a second" |
| resume, 96 at once, 24 in flight (HTTP) | p50 117 ms, p99 242 ms, **188/s** | -- |
| pause, 96 at once (HTTP) | p50 73 ms, **283/s** | -- |
| create, 96 at once (HTTP) | p50 152 ms, **145/s** | Agent Sandbox: 300/s at sub-200 ms |
| stateful sandboxes on 8 VM slots | **240 (30x)**, every one kept its state | ~250 on 8 pods (30x) |
| a request to one of those 240 (evict + resume + command) | p50 ~135 ms, p99 ~220 ms, 58/s | -- |
| fork x5 | 50 ms | not in its documentation |
| paused sandbox, storage | ~2.2 MiB each (240 in 523 MiB) | Cloud Storage, size not published |

The throughput rows are one node. Agent Substrate's 500/s and Agent
Sandbox's 300/s do not say how many machines they are, so the comparison is
not like for like either way; a cluster here adds nodes linearly for
creates, since nothing is shared but the store. What limits a node here is
CPU: a create costs ~25 ms of host CPU, about its whole latency, and
concurrency past eight adds contention.

#### Restore on any node

Agent Substrate's snapshots go to Cloud Storage and restore on any worker.
Here, with `--snapshot-store DIR` -- a directory every node mounts at the
same path (NFS, EFS, Filestore, CephFS; a ReadWriteMany PVC in the chart):

- **The template is shared, by content.** Its name is a hash of everything
  that makes the guest what it is -- kernel, initramfs, command line, size,
  the egress CA. The first node to need it builds it in a scratch directory
  and renames it into place; a node that loses the race uses the winner's.
  A layered snapshot names its template, so every node sharing the store can
  lay it over the same bytes.
- **So is the egress CA**, published the same way: a guest trusts the CA it
  was created with, and must still after resuming elsewhere. Its key and the
  paused sandboxes' descriptions -- which hold egress-proxy credentials and
  injected headers -- are written owner-only.
- **A paused sandbox is claimed, not assumed.** Resuming renames its
  description aside, which is atomic, so two nodes asked at once cannot
  both resume it; a node's own note of a sandbox it paused is checked
  against the store, since another may have resumed it since.
- **It outlives its node.** The record says it is paused into shared
  storage; a restarting node keeps it, the reaper keeps it, and a control
  plane sends a request for it to any node with room when its own has gone
  -- `connect`, `resume`, `DELETE`, and the envd proxy, where the request
  that wakes it is served by whichever node resumed it. A proxy whose cached
  route no longer connects asks again once.
- **Draining a node loses nothing.** On SIGTERM a node pauses what it runs
  into the store instead of ending it, and the next request for each
  resumes it elsewhere.

Verified on two nodes sharing a directory, two control planes, the
unmodified SDK: a sandbox with a counting loop and an injected header,
paused on node A; node A SIGKILLed; `Sandbox.connect` through the other
control plane resumed it **on node B in 53 ms**, file kept, loop still
counting, and the header still injected -- the guest trusted node B's
leaves. Node A drained by SIGTERM: a command through a control plane's
proxy resumed its sandbox on node B in 95 ms. A paused sandbox whose node
was dead deleted cleanly, record and snapshot. The two nodes started at
once and raced to publish the template; one won and the other used it.

What it does not do: move a *running* sandbox (it pauses first), or keep
paused sandboxes anywhere but a filesystem -- object storage would need the
template fetched to local disk before it can be mapped.

#### Mutual TLS inside the cluster

Agent Substrate uses Kubernetes pod certificates between its components.
Until this, a node here trusted a control plane by a shared token sent over
plain HTTP -- anyone on the path had the token, and the envd traffic a
control plane relays (commands, their output, files) with it. Now, with
`--mtls-ca/--mtls-cert/--mtls-key` on both sides (`tools/mtls-certs.sh`
makes a CA and both certificates; the chart takes cert-manager-shaped
Secrets):

- a node serves its API **and** its envd proxy only to a peer presenting a
  certificate the cluster's CA signed; a control plane talks only to a node
  whose certificate it signed too, and relays envd traffic over TLS;
- nodes are verified by one shared DNS name (`hv2-node`) rather than by
  address, since a node's address is whatever its pod got -- the chain, the
  signatures and validity are all still checked;
- the cluster token still rides every request: TLS says the peer holds a
  key the CA vouched for, the token says it is this cluster's.

Verified: a node answers plain HTTP, HTTPS without a client certificate,
and HTTPS with another CA's certificate by refusing the handshake, and the
token with the cluster's certificate with 200. A control plane given another
CA's certificate reaches no node ("every node refused"). The cluster SDK
tests -- pause, resume, fork, auto-resume through the proxy, a node
SIGKILLed and its sandbox resumed elsewhere, a node drained -- pass over
mTLS unchanged. A unit test does real handshakes for each case.

The store too: a `rediss://` URL is TLS, verified against the system's
roots or `HV2_STORE_CA`, with `HV2_STORE_CERT`/`HV2_STORE_KEY` for a store
that requires client certificates (the chart's `store.tlsSecret`), through
rustls on the same `ring` provider. Verified against Valkey built with TLS
and `--tls-auth-clients yes`: it refused a plaintext client and a TLS client
without a certificate; two nodes and two control planes ran on it, and the
cluster SDK test -- pause, resume, fork, auto-resume -- passed.

Still plaintext unless configured: the client-facing E2B API
(`--tls-cert/--tls-key`, or an ingress), and the chart's own single Valkey,
which only its NetworkPolicy reaches.

#### Workload identity

Agent Substrate authenticates agents to Google Cloud through GKE Workload
Identity. E2B has its own design, which the SDK already speaks, and which
this now implements: `iam.tokens` registers named tokens (an audience, and
`tokenType: JWT-SVID`, the one type E2B accepts), and a network rule writes
`${e2b.identity.tokens.NAME}` into a header it injects. The egress gateway
replaces the placeholder with a token minted for that request. The guest
wrote a placeholder and never holds a token; the token exists only between
the gateway and the destination.

- The token is an ES256 JWT-SVID: `sub` the sandbox's SPIFFE ID
  (`spiffe://TRUST_DOMAIN/sandbox/ID`), `aud` the registered audience,
  five minutes, a fresh `jti` each request, `iss` the configured issuer.
- Nodes and control planes serve `/.well-known/jwks.json` and
  `openid-configuration` without an API key -- what AWS STS
  `AssumeRoleWithWebIdentity` or GCP workload identity federation fetch.
  A control plane's JWKS is every live node's key, once each.
- With a snapshot store, every node signs with one key published there,
  so a sandbox paused on one node and resumed on another keeps its identity
  and one JWKS entry covers the cluster.

Verified with the unmodified SDK, and checked by a verifier that shares no
code with the signer -- `openssl dgst -verify` against a public key rebuilt
from the JWKS: a sandbox with `Authorization: Bearer
${ctx.iam.tokens['aws']}` curled httpbin twice; the upstream saw two
different JWTs, both verified, with the right subject, audience, issuer and
expiry; nothing in the guest held one; a type other than `JWT-SVID` was
refused with 400. Across nodes: tokens minted on node A and, after it was
SIGKILLed and the sandbox resumed, on node B carried one SPIFFE ID and one
key, and both verified against a control plane's JWKS.

Not verified: federation with a real cloud provider, which needs the
issuer reachable from it over public HTTPS.

#### Templates and files

Until this, every sandbox was the one `base` guest and `templateID` was
echoed back unread -- while E2B, CubeSandbox and Agent Substrate all start
agents from images their users build. Now:

- **Templates from OCI images.** `tools/guest-image/from-oci.sh IMAGE`
  exports any image (with `docker`), adds the guest agent and init, and
  writes an initramfs; its `ENV` is loaded before the agent starts.
  `--template NAME=INITRAMFS`, as many as wanted, beside `base`; each is
  booted and snapshotted once, and `Sandbox.create(template=NAME)`
  restores from its own. An unknown template is a 404, before a slot is
  taken. `GET /templates` lists them, on a node and on a control plane.
- **Scheduling by template.** Nodes advertise the templates they offer, and
  a control plane sends a create only to one that has it.
- **Files.** envd's `GET /files` and `POST /files` (multipart and
  octet-stream), on the same port as its RPCs, through two new guest-agent
  operations that move bytes in 4 MiB chunks -- binary content arrives as
  sent. `sandbox.files.write` and `files.read` work; before this they had
  no route at all.

Verified with the unmodified SDK and `python:3.12-slim` (130 MiB of root
filesystem, 45 MiB compressed): `Sandbox.create(template="python")` in
**91 ms**, Python 3.12 running with the image's `ENV` and Debian root; a
script written with `files.write` ran; five more Python sandboxes, each
having run Python, cost **7.7 MiB** each -- the root filesystem is in the
template's memory image, shared copy-on-write. Booted rather than restored
-- as happened once when the template's boot outlasted a 15 s timeout, now
120 s for templates -- the same sandbox took 4.9 s and 282 MiB. In a
cluster where one node of two had the template, four Python creates through
a control plane all went to it; base creates went anywhere.

#### Templates built by the platform

`from-oci.sh` needs Docker on the operator's machine. Now a node builds a
template itself: `POST /templates {"templateID": "node", "image":
"node:22-slim"}` answers 202, and the node

- pulls the image from its registry over HTTPS -- anonymous bearer tokens
  as Docker Hub and most registries issue them, the `linux/amd64` manifest
  chosen from an index -- verifying every blob against its sha256 digest;
- applies the layers in memory with OCI whiteouts (`.wh.NAME`, opaque
  directories), never unpacking a layer's paths onto the host;
- writes the initramfs itself, `newc` cpio, deterministic, hard links kept
  as links (busybox's image is ~400 links to one binary: as copies it was
  270 MiB, as links 4 MiB), with the guest kit (`--guest-kit`, in the node
  image at `/opt/hv2/kit`) added where the image lacks it -- an image's own
  busybox is kept;
- snapshots it and offers it; `GET /templates` shows `building`, `ready` or
  `error`.

With a shared snapshot store, the other nodes adopt a template one node
built -- its record and initramfs are in the store and its snapshot is
content-addressed there -- within a few seconds and without pulling. A
control plane's `POST /templates` fans the build out to every node, which
a cluster without a store needs.

Verified: `alpine:3.20` built in 5-6 s and `node:22-slim` in 27-42 s on one
node, pulled from Docker Hub by the node; through the unmodified SDK a
`node` sandbox was created in 136-236 ms and ran Node.js 22, npm, and a
script written with `files.write`; `alpine` ran `apk`. With two nodes and a
store, `alpine` built on node A was offered by node B without a pull; a
`busybox:1.36` build through a control plane reached both nodes, and
sandboxes of both templates ran through the control plane. In the Compose
stack the node container, with the kit its image now carries, built `alpine`
through a control plane and a create from it answered in 85 ms.

Private registries and zstd: a registry's `Basic` challenge is answered with
a username and password, a `Bearer` challenge's token requested with them
(`POST /templates` takes `username`/`password`; the SDK's
`from_image(image, username=, password=)` sends them), never stored or
logged. Layers are decompressed by their magic bytes: gzip, zstd (a pure
Rust decoder), or plain tar. Verified against a password-protected
`registry:2` holding an image built with zstd layers only: no login was
refused ("the registry wants a username and password"), a wrong password
refused with 401, the right one built in 2.0 s and a sandbox read the zstd
layer's file; the password appeared nowhere in the node's log. A
`localhost` registry is spoken to over plain HTTP, as Docker does.

#### Snapshots and builds: E2B's Template SDK and Dockerfiles

`sandbox.create_snapshot()` (`POST /sandboxes/{id}/snapshots`) checkpoints a
running sandbox in place -- layered, only what it changed since its
template -- and offers it as a template; `Sandbox.create(snapshot_id)`
restores it as a fork is restored. `GET /snapshots` and
`Sandbox.delete_snapshot` (`DELETE /templates/{id}`) complete E2B's set. With
a store, a snapshot taken on one node restores on any node at once: a
create for a name a node has not seen yet reads the store's record, and the
taking node announces it to the cluster without waiting for a heartbeat.

On that sits E2B's build API -- `POST /v3/templates`, file upload links and
uploads, `POST /v2/templates/{id}/builds/{build}`, build status with logs --
which is what `Template.build()` calls, and so what
`Template().from_dockerfile(...)` builds with, the SDK having turned the
Dockerfile into steps. A build restores its base (an image, pulled into a
template of its own and cached by name; or a template, or a snapshot), runs
`RUN`, `COPY`, `ENV`, `WORKDIR` and `USER` in that sandbox with each line of
output in the build log, starts the start command, waits for the readiness
check, and snapshots the sandbox into the template. Every sandbox created
from it is restored with that process already running. The build's commands
run in a microVM, never on the node, with no container runtime; a `COPY`'s
upload is repacked on the host with Docker's placement rules and unpacked
by the guest's own tar. `ENV` and `WORKDIR` hold for every later command:
the agent reads them from the template at each start. Through a control
plane, a template's build calls all reach one node, chosen from its name by
rendezvous hashing, and uploads stream through unbuffered.

Verified with the unmodified SDK: a builder template on `base` (COPY, ENV,
WORKDIR, RUN, `busybox httpd` started with a `wget` readiness check) built in
0.9 s, and a sandbox from it answered in 42 ms with httpd already serving
the copied file, in the template's directory and environment. A Dockerfile
(`FROM python:3.12-slim`, `ENV`, `RUN pip install six`, `WORKDIR`, `COPY
app.py`, `RUN python app.py`) built in 24.7 s including the pull from Docker
Hub; its sandbox was created in 126 ms and ran the app with the installed
package. Through a control plane over two nodes and a store, a build took
0.9 s and its sandboxes were created in 23-29 ms through the control plane
and on either node. A sandbox of a built template paused in 7 ms, resumed in
20 ms and forked with its server and state intact. Snapshots: taken in
8-11 ms, created from in 17-30 ms on the taking node, the other node, or
through the control plane.

Users, as envd has them: a template's last `USER` is who its sandboxes'
commands run as when the SDK names no one, and who owns what
`files.write` makes, directories included; `user="root"` (the SDK's
`Authorization: Basic` of `root:`, or `?username=` for files) overrides it.
The agent drops from root itself -- supplementary groups, then gid, then
uid, between fork and exec -- and sets `HOME`, `USER` and `LOGNAME` from
`/etc/passwd`; a template with no `USER` runs as root, as before. Build
steps switch user the same way, not through `su`. Verified: a Dockerfile
ending `USER user` / `WORKDIR /home/user` ran `id -un` as `user` in
`/home/user` with `HOME=/home/user`, an earlier `USER nobody` step had run
as `nobody`, `user="root"` ran as root with `HOME=/root`, a `files.write`
of `/home/user/notes/a.txt` left both the new directory and the file owned
by `user` (with `user="root"`, root), and the default user could not
overwrite a file a root step made.

#### Sizes, and guests with more than one CPU

A template has its own size: `cpuCount` and `memoryMB` on
`Template.build(...)` or `POST /templates`, a node's `--cpu-cores` and
`--memory-mb` otherwise. The size is the template's snapshot's, so every
sandbox restored from it, and every template built on it, is that size; a
build asking to resize one is refused and told to build from the image. An
image is pulled once per size.

Sizing CPUs found that no guest had ever had more than one. hv2-core wrote
no MP table and no ACPI MADT, so Linux said "SMP disabled" whatever the vCPU
count. A VM of more than one vCPU now gets an MP table (spec 1.4, at
`0x9FC00`, up to 32 processors, the I/O APIC routing ISA IRQs 0-23) and
per-vCPU CPUID (APIC ID and logical count in leaf 1, x2APIC ID in 0xB and
0x1F); one-vCPU VMs are untouched. The second CPU then still did not come
up: `KVM_RUN` on an application processor blocks until its startup IPI and
then returns `EAGAIN`, which the run loop treated as fatal, ending that
vCPU's thread the moment Linux woke it ("CPU1 failed to report alive
state"). It is retried now.

A third defect, in every guest: the vsock device queued a host write as
one packet of up to the credit window (256 KiB), but Linux posts 4 KiB rx
buffers, so the packet fitted none, was dropped, and the stream stalled. An
8 MiB `files.write` hung on one vCPU as on four. Stream data is now split
across the buffers the guest offers.

Verified, with the unmodified SDK:
- **Guest sizes:**
  - 1, 2 and 4-vCPU guests report `nproc` 1, 2 and 4;
  - N busy loops take as long as one (0.11-0.12 s either way);
  - an 8 MiB `files.write`, HTTPS egress, and pause/resume all work at each size;
  - sandboxes are created from each size's template in 43-51 ms.
- **Forks and snapshots:** three forks of a 4-vCPU sandbox with all four CPUs busy were made in 42 ms, each with its four busy loops still running; a snapshot of it restored with 4 CPUs.
- **Per-template sizes:**
  - templates of 2 vCPU / 2048 MiB and 1 vCPU / 512 MiB were built from `alpine:3.20` in 2.6-5.3 s and created in 17-37 ms, their guests seeing exactly that;
  - a `POST /templates` of 2 vCPU / 768 MiB was listed with its size and ran at it;
  - out-of-range sizes are refused with a 400.

#### Metrics and logs

E2B's `GET /sandboxes/{id}/metrics` (`get_metrics`), `GET /sandboxes/metrics`, and `GET /sandboxes/{id}/logs` (v1 and v2). Metrics are the guest's own account -- CPU from `/proc/stat`, memory and page cache from `/proc/meminfo`, the root filesystem from `statvfs` -- read in one agent round trip (`Stats`) every 5 s, as E2B samples, and kept for an hour; paused sandboxes are not woken to be sampled. Logs are the node's lifecycle events for the sandbox: created, volumes mounted, paused, resumed, forked. A control plane forwards each to the sandbox's node and merges `/sandboxes/metrics` across nodes.

Verified with the unmodified SDK:
- an idle 2-vCPU sandbox read 0.1% CPU, 34 of 971 MiB and 9 of 481 MiB disk;
- with one CPU spinning, steady samples read 40-47% (the guest's own `top`: 47%);
- writing 200 MiB added 197 MiB of used memory, as page cache;
- samples arrived every 5 s, and the start/end window filtered them;
- the log read created, paused, resumed;
- through a control plane over two nodes, four sandboxes each had metrics, `/sandboxes/metrics` returned all four, and logs came back.

Not done: logs of the guest's own processes (only lifecycle events are logged), and events/webhooks.

#### E2B's Code Interpreter template, and what building it taught the builder

E2B's own open-source Code Interpreter template (`e2b-dev/code-interpreter`, `template/template.py`) builds on HyperMachine through the SDK's `Template.build`, with its Docker variant and Python kernels, at 4 vCPU and 8 GiB:
- 29 steps: apt, NodeSource, the full pip requirements, the IJavascript kernel, E2B's server and its virtualenv, configuration;
- then its start command.

It does not yet come up here. E2B's `start-up.sh` gives Jupyter 10 s to become healthy, and its server gives each kernel 5 s to start. On this development host -- KVM nested under WSL2, the Windows host at about 75% CPU from other work -- Jupyter was healthy after 7-18 s and the kernel took longer than 5 s:
- With the health window lengthened as a test, Jupyter came up, E2B's server started, and creating its default kernel then failed with `httpx.ConnectTimeout`.
- In the sandbox, Jupyter itself reached `/api/status` 200 between 7 and 18 s after launch, at 100% of a CPU.
- A second start in the same guest took 2-4 s.
- CPU-bound Python ran at half the host's speed.

This needs a faster host to finish. It is not a defect found in the sandbox, but it is also not shown to work: the SDK's `run_code` has not run here.

What the builder gained from it:
- **A step cache.** After the last step the build's sandbox is checkpointed under a key of its base, size and every step (files by hash). A build with the same steps starts from it, at the start command, as E2B's layer cache does; `skip_cache` forces the steps. The Code Interpreter's 55 minutes of steps then took 24.5 s to keep, and each later attempt at its start about a minute. A small template: 3.5 s built, 0.2 s again.
- **The start command's output in the build log**, as it comes. A start command that exits before its readiness check passes ends the build at once, with its status. Without it, the Code Interpreter's failure was ten silent minutes.
- **Bounded readiness attempts**, 30 s each: a check that hangs is a failed one.

#### Guests larger than 3 GiB

No guest above about 3.25 GiB could run, and the size check promised up to 64 GiB. Guest RAM was one flat range from address 0, so it ran into the virtio register windows at `0xd000_0000`, and from 4 GiB over the I/O APIC and local APICs. Found building E2B's Code Interpreter template at 8 GiB, which failed attaching its first device.

Guest RAM now has a PC's hole below 4 GiB:
- the first 3 GiB at address 0, the rest from 4 GiB, both from one host buffer;
- two KVM memory slots (the read-only shared region moved from slot 1 to slot 2);
- the device model maps two regions;
- the `e820` map reports both ranges, and the initrd is placed below the hole;
- a raw memory image -- a template's, a pause's, a layered snapshot's base -- is laid out by offset in the host buffer, which is what it is mapped over;
- device windows are refused only if they overlap actual RAM.

A guest of 3 GiB or less is laid out byte for byte as before, so existing templates and snapshots are unaffected.

Verified:
- a 4 GiB guest's kernel reported the three ranges in its `e820` map, and 3.5 GiB written to it survived pause and resume;
- in an 8 GiB guest the kernel's `Normal` zone, its RAM above 4 GiB, went from 1,266,317 free pages to 3,025 as 6 GiB was written. That RAM was in use, and the 6 GiB was intact after pause (38.6 s) and resume (23.7 s) and in a fork;
- the unit tests, and the build, lifecycle, volume, size, SMP-fork and port regressions, passed at the old sizes.

An intermittent failure seen twice this session is not explained: an image template's guest (512 MiB once, 1 GiB once) did not answer within 120 s, and passed on the next runs. The node keeps no guest console output from a failed template boot, which is what finding it would need.

#### A guest's console, kept for failures

Every VM now has a serial console (COM1, `0x3F8`), and the guest kernel logs at `loglevel=3` (errors and worse) instead of not at all. When a template's guest or a sandbox's guest never answers, the error carries the last 15 lines the guest wrote -- or says it wrote nothing, which is itself a finding.

Verified:
- a template built on an initramfs whose init has no agent failed with `its console ended: broken init: no agent in this image | sleeping forever`, where before it said only that the agent never answered;
- the build, lifecycle, port, SMP and volume regressions passed. Sandbox creates measured 50-112 ms in that run, against 27-66 ms before, on a host whose load varies; this was not separated from the change.

The intermittent failure recurred once in that regression: a sandbox from a built template did not answer within 15 s, and **its guest wrote nothing to its console** -- no kernel error, no panic. Four reruns passed. A sandbox is restored from a running snapshot, so it has no boot to print, but a panic or an error would still appear. So the guest either did not run, or ran without error and its agent did not answer. The error now also says whether they ran: each vCPU's VM exits over half a second, and in all, and the VM's state. A guest that never ran shows none; one spinning shows many; one halted and waiting shows a few timer exits (the broken initramfs above: 2 in 0.5 s, 99,919 in all). Ten more runs of the lifecycle regression passed, fourteen in a row, so the report is waiting for the failure to recur.

#### A sandbox's own ports

E2B's `sandbox.get_host(port)` -- `{port}-{sandboxID}.{domain}` -- reaches any port a sandbox serves, not only envd's: a web server, a dev server, the Code Interpreter's Jupyter. Until now the proxy routed envd's port alone, so every other one answered 404.

How it works:
- The first request for a port makes a loopback listener for it on the node.
- Each connection it accepts rides a vsock connection of its own: the guest agent connects to the port inside the guest, answers, and copies bytes both ways (`Forward`). No network interface is involved, so a sandbox without one serves its ports all the same.
- For any port but envd's the proxy speaks HTTP/1.1 to its backend, at the node and between a control plane and a node, and splices upgraded connections, so WebSockets work.
- A sandbox's listeners and connections close when it pauses or ends; the first request after a resume makes them again.

Verified, through the unmodified SDK's `get_host`:
- `busybox httpd` in a sandbox answered its first request in 90 ms (the listener made) and later ones in about 10 ms;
- 64 MiB came through at 153 MiB/s with a matching checksum -- 91 MiB/s on a node without `--network`;
- a port nothing listens on answered 502;
- after pause and resume the server answered again;
- a chunked response streamed: its first chunk arrived after 10 ms, the rest over 1.5 s;
- a WebSocket upgrade came back `101` with the right `Sec-WebSocket-Accept`, and messages echoed both ways;
- through a control plane over two nodes, 32 MiB came through at 261-274 MiB/s.

#### Cloud registry logins

E2B's `from_aws_registry` and `from_gcp_registry` work. For AWS the node signs `ecr:GetAuthorizationToken` with Signature Version 4 and logs in with the token it answers; `HV2_ECR_ENDPOINT` points at a FIPS or VPC endpoint instead of the region's. For Google the service account's key signs an RS256 JWT, which its token endpoint exchanges for an access token, used as the password of `oauth2accesstoken`. Neither credential is stored or logged.

Verified:
- the SigV4 code reproduces AWS's published worked example (IAM `ListUsers`) to the signature;
- a unit test signs a Google assertion with a throwaway key and verifies it with the public half;
- end to end, against a password-protected `registry:2` and stand-ins for the two token endpoints:
  - a fake ECR re-derived the node's signature in an independent Python implementation before answering;
  - a fake Google endpoint verified the JWT with `openssl`;
  - builds from both logins pulled the private image and their sandboxes ran;
  - a wrong AWS key was refused with AWS's own message;
  - no secret appeared in the node's log.
- AWS and Google themselves were not called: there are no cloud credentials here.

Found on the way: a forced rebuild (`skip_cache`) of a template on a node without a snapshot store deleted the new template's snapshot, because the old and new shared a directory and the old one cleaned up on drop. Each build now has its own directory.

Note that a node caches an image by name: a second build from a private image reuses the first pull without logging in again, unless `skip_cache` is set. That fits a single-tenant cluster, which this is.

#### Events and webhooks

E2B's `GET /events/sandboxes[/{id}]` and `/events/webhooks` -- create, list, update, delete, deliveries grouped by event, and hourly stats -- served by a node alone and by a control plane, over the cluster store's event stream (a node without a cluster keeps its own in memory).

Events:
- Lifecycle events carry E2B's types: `sandbox.lifecycle.created`, `paused`, `resumed`, `killed`.
- A delivery is signed as E2B signs one: `e2b-signature` is base64, unpadded, of SHA-256 over the secret followed by the body. That scheme is from E2B's documentation as recalled here -- its OpenAPI spec does not state it -- and is worth checking against a receiver built on E2B's own verifier.
- A delivery is retried after 1 s and 4 s, and every attempt is recorded.
- The node that emitted an event delivers it, so each goes out once however many control planes run.

Webhook URLs are user-supplied, so an address that is not global -- loopback, private, link-local, which includes the cloud metadata service, and shared -- is refused unless the node runs with `--allow-private-webhooks`. The address checked is the one connected to, so DNS rebinding cannot swap it, and redirects are not followed.

Verified:
- on one node, a receiver checking signatures itself got created, paused, resumed and killed, all valid;
- a receiver that answered 500 twice got the event on the third attempt, and the deliveries and stats showed all three;
- a disabled webhook was listed as such; the secret is never returned;
- without the flag, `127.0.0.1`, `169.254.169.254` and `localhost` were each refused with `request_error`;
- through a control plane over two nodes, a webhook registered there got each node's created and killed exactly once, and the control plane listed both nodes' events.

The regression run after this change passed with every result as before, but its timings -- pause 83 ms, one create from a snapshot 1.3 s -- were taken with the Windows host at 97% CPU from other work, and are not comparable to the earlier numbers; they were not re-measured.

#### Volumes

E2B's persistent storage, as its SDK's `Volume` uses it:
- the volume API (`POST/GET /volumes`, `GET/DELETE /volumes/{id}`);
- the content API (`/volumecontent/{id}/{file,dir,path}`, authenticated by the volume's own bearer token);
- `volumeMounts` on create.

A volume is a directory on the node -- in the snapshot store when there is one, so every node has every volume -- mounted into sandboxes live and shared, and it outlives them.

How a mount works: the guest agent answers a `MountVolume` request and then hands that very vsock connection to its kernel as a 9P2000.L mount (`trans=fd`). The node runs a 9P server on the other end, one thread per mount. The guest kernel had 9P already; nothing new was needed in it or in the VMM.

The guest is treated as hostile:
- Every path is resolved with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` beneath the volume.
- Every name-taking operation acts on one validated name within a directory resolved that way.
- Attributes change through the resolved inode.

The guest's ownership is kept in an extended attribute, because the node's own user owns the host files. A mount the snapshot held is detached and remade on resume and on fork. A volume ID is derived from its name, so a control plane routes a create and every later call for it to the same node, streaming uploads and downloads through.

Verified with the unmodified SDK:
- **Mounting and sharing:**
  - a sandbox came up with a volume mounted in 133-298 ms;
  - a second sandbox read the first's 16 MiB file with a matching checksum;
  - each saw the other's appends, renames and symlinks live;
  - the API read what they wrote;
  - 16 MiB read through 9P in 212-283 ms.
- **Persistence and lifecycle:**
  - files outlived every sandbox that mounted them;
  - a sandbox paused and resumed wrote to its remounted volume;
  - two forks of it each wrote there, and the API saw all three.
- **Escapes:**
  - `../../meta.json`, a host symlink planted in the volume, and a path through it were all refused by the API;
  - through the mount, the host symlink resolves in the guest's own namespace, never the host's;
  - unit tests walk, open and create through symlinks and `..` and reach nothing outside.
- **Across two nodes and a control plane:**
  - a volume made through the control plane was mounted on both nodes, each seeing the other's writes;
  - a file written by the API as uid 1000, mode 600 showed exactly that in the guest;
  - a `chown` in the guest showed in the API.

`--volume-dir` places volumes elsewhere. Volumes must live on a filesystem with user extended attributes (ext4, xfs) for guest ownership to hold: on WSL's `/mnt/c` every file reads 777 and owners are not kept.

Not done:
- memory hot-plug and ballooning (a sandbox's size is fixed at its template);
- more than 32 vCPUs;
- volume quotas;
- volumes in a cluster without a shared snapshot store, where a volume lives on one node and a sandbox mounting it must be created there.

What Agent Substrate has that this does not:

- **Kubernetes-native objects** (`ActorTemplate`, `WorkerPool`,
  `kubectl-ate`). This is E2B's API instead, deployed by a Helm chart.

What this has that it does not: E2B compatibility (the unmodified SDK),
egress policy by hostname and CIDR with DNS control and credential
injection, fork, a KVM guest with its own kernel per sandbox, and running
anywhere with `/dev/kvm` rather than on Kubernetes.

#### Found on the way

Four defects, each found by a measurement or a test failing, not by
reading:

- **A restored guest's idle vCPU spun at 100%.** 315,000 `HLT` exits a
  second from a guest whose own accounting said it was idle -- read off KVM's
  debugfs counters. The snapshot captured `MSR_KVM_ASYNC_PF_EN` (async page
  faults, delivered by interrupt) but not `MSR_KVM_ASYNC_PF_INT`, the
  vector: restored guests had async page faults on vector 0. The first
  page-ready notification could never be delivered, and a pending one makes
  every halt return at once. Latent in every Phase 5 template restore; hit
  every time here, because a resumed guest's memory is a file mapping and
  file mappings are what fault asynchronously. Idle CPU for 16 resumed
  sandboxes went from 1613% of a core to 4%.
- **A snapshot could hang every guest restored from it.** The guest agent
  served one connection at a time, and a snapshot taken before the guest
  saw the host close its last connection left every copy blocked reading a
  connection whose host side no longer existed. The agent now serves each
  connection on its own thread -- which also stops one long command from
  holding the agent against every other request.
- **Requests counted as finished while still streaming.** The proxy held a
  request's in-flight guard in the backend connection's future, which hyper
  resolves once the request's sender is dropped -- not when the streamed
  response ends. A command 40 ms into its run counted as idle, and its
  sandbox was paused under it. The guard now lives in the response body.
- **A request arriving during a pause was refused as unknown**, between the
  sandbox leaving the running set and entering the paused one; and a
  resume could be overtaken by creates for longer than the SDK's 10 s
  timeout. Requests now wait out a transition in progress, and slots are
  granted in order.

Two things tried and not kept on by default: the guest agent logged two
lines to the serial console per request, ~250 VM exits, and those are now
opt-in (`HV2_AGENT_TRACE`) -- which made no measurable latency difference;
and prefaulting a restore's measured working set (`KVM_PRE_FAULT_MEMORY`,
`--prefault`) halves a restore's page faults and exits, and also made no
measurable latency difference here. Under nested virtualisation the cost
of a restore's first request is somewhere the host's counters do not show;
both are left for a bare-metal host to measure.

## What this roadmap deliberately does not do

- It does not commit to building all five phases — that is a resourcing
  decision for whoever owns HyperMachine's priorities, not something a
  planning document should presume.
- It does not claim a head-to-head result. Every number here was measured
  on one nested-KVM development box; CubeSandbox's are its own published
  figures. The two have not been run on the same bare-metal host.
- It does not resolve the AGPL/Apache-2.0 licensing question — flagged
  above as a real strategic input, decided by whoever owns that call.
