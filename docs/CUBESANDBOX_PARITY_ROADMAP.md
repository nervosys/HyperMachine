# Beating CubeSandbox: a feature and performance roadmap

Status: **Phases 0-2 built and verified; Phase 3 started.** Phase 0
benchmarked honestly before anything was promised; Phase 1 met its exit
criterion, an unmodified E2B SDK client running against a HyperMachine
endpoint by changing only where it points; Phase 2 moves a guest between VMs
through a file and restores faster than that guest boots. Phases 4 and 5 are
untouched. Each section below carries its own status and the measurements
behind it.

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

### Phase 3 — Network security (CubeVS/CubeEgress-equivalent)

**Written, and not yet on a live path.**
`hv2_net::egress::EgressPolicy` decides whether an outbound frame may leave,
on destination address, port and protocol, and `Bridge` consults it before
NAT so a refused frame leaves no translation entry behind. Default-deny:
`EgressPolicy::default()` is `deny_all`, and `allow_all` exists but has to be
written at the call site, because "nobody configured a policy" and "someone
chose to allow everything" should not look the same in review.

That is what happens *when a `Bridge` exists*, and none does outside a test.
This paragraph opened "started, at the layer that was carrying the traffic"
until 2026-09-22, which read as though the policy were running. It is not:
`Bridge` is constructed in exactly one place outside its own module,
`hv2-net/examples/tap_bridge.rs`, and `hv2-net` is depended on only by the
`hypermachine` facade, which re-exports it without using it. So no frame a
deployed sandbox produces passes through `EgressPolicy`, because no frame
passes through a `Bridge`.

The distinction still worth drawing is with `hv2_core::networking::filter`,
which is wired to nothing at all: `EgressPolicy` sits inside the component
that *would* carry the traffic, so standing a `Bridge` up enforces it. That
is a smaller gap than writing the filter in, and it is still a gap.

Note also how far that wording travelled before being checked. It reached
`filter.rs`'s `#[deprecated]` notes, `policies.rs`'s module header, and a
project memory recording `permissions/` as wired into a request path when
nothing installs that middleware either — four copies, each reading like
independent confirmation of the others. Worth remembering when a security
claim is easy to repeat and expensive to verify.

This came from reading [NVIDIA's sandboxing
guidance](https://developer.nvidia.com/blog/practical-security-guidance-for-sandboxing-agentic-workflows-and-managing-execution-risk/),
which puts blocking "outbound network access to unknown destinations" first
among its mandatory controls — the direct threats being a reverse shell and
exfiltration, neither of which needs the attacker present. Reading it against
this repo found that `Bridge` carried every frame a guest produced, and that
`hv2_core::networking::filter` — a packet filter with connection tracking,
already written — is called by no data path at all. A filter that is never
asked is not a control.

Worth recording from the same source: it recommends full virtualization
("VMs, unikernels, Kata containers") over kernel-sharing for exactly this
workload, because agentic tools "perform arbitrary code execution by design"
and "kernel vulnerabilities can be directly targeted as a path to full system
compromise", rating gVisor as weaker than full virtualization. That is this
project's own premise, from a third party.

What is still not built: names. A rule is an address, so an allowlist for
"our package mirror" has to be resolved by whoever writes it and goes stale
when it moves; filtering on the name a client *asked* for needs TLS
termination or trust in the guest's DNS. Nor is there an L7 egress proxy with
credential injection (so a sandboxed agent's outbound API calls never see the
real secret), or eBPF-based per-sandbox conntrack. Those remain real
engineering efforts.

Note that `e2b_compat` gives a sandbox no network interface at all, so the
deployed shape today is denied by absence rather than by policy.

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
