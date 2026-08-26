# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **A guest executed** (`hv2-core`). The boot path had only ever been
  type-checked: `VM::provision`, `load_boot` and `launch` were described as
  creating a hypervisor VM, writing an image into guest physical memory and
  running it, and no kernel had ever been asked to do any of it. Two new
  examples do. `kvm_probe` reports the platform, `VM::new` and `provision`
  separately, because a host can pass one and fail the next -- Windows does,
  when Hyper-V owns VT-x. `boot_probe` runs the whole path against a 512-byte
  real-mode image and prints what came back: a KVM VM and its vCPUs exist, the
  image is in guest memory, the guest runs, and `Hello, World!` arrives through
  `SerialDevice` and `DeviceManager` into `VM::console_output` -- which
  independently confirms the console plumbing as well. The gate on this was an
  inherited assumption rather than hardware: WSL2 here has nested
  virtualisation, `kvm_amd` loaded and an accessible `/dev/kvm`.
- **A guest can be reached, and a command can be run inside it** (`hv2-core`,
  `hv2-guest-agent`, `hv2-agent`, `hv2-api`). `execute_script` was described in
  four places as running inside the guest and always evaluated a Rhai script on
  the host; the engine was never the problem, because nothing in the repo could
  reach a guest at all. Every virtio device kept its descriptor table in host
  `Vec`s a test filled in, and no virtio register file was ever mapped into
  guest physical address space. Three pieces close that: `GuestQueue` reads the
  descriptor, available and used rings out of guest memory, bounding every
  guest-written field (a chain that cycles is refused, an index past the table
  errors, indirect tables may not nest, a chain may not claim more than 64 MiB);
  `VirtioMmioTransport` is a virtio-mmio v2 register file implementing `Device`,
  so `register_mmio_region` puts it on the MMIO exit path `VM::run` already
  takes; and `VsockDevice` carries the connection state machine and credit
  accounting, both bounded. The new `hv2-guest-agent` crate holds the wire
  protocol and the in-guest binary that answers it, `GuestAgent` is the host
  client, and `AgentVM::exec_in_guest` is the operation an agent calls —
  surfaced as the `vm.exec` MCP tool and `POST /api/v1/vms/{id}/exec`. Four
  ways of having no guest to run in each fail as themselves rather than as a
  timeout, because an agent that receives a timeout when the real problem is a
  missing device retries forever. Gated on `Capability::GuestExec`, which
  existed and had never been consulted.
- **Confinement the operating system enforces** (`hv2-sandbox`, `hv2-agent`).
  Two things here were named like sandboxes and confined nothing: `hv2-agent`'s
  `Sandbox` says so in its own docs, and `hv2-core::container` was 3,866 lines
  of namespace, cgroup and seccomp types whose `start` fabricated a PID. A
  repo-wide search for `seccomp|setrlimit|unshare|prctl|CreateJobObject` found
  prose and struct fields, and not one confinement syscall. The new crate is
  built on one rule: silently dropping a control is worse than no sandbox,
  because a caller who asked for no network and got one believes the opposite
  of the truth. `Sandbox::controls()` reports what *this host* enforces, probed
  by attempting each control rather than assumed; a spec asking for something
  the backend lacks is refused, naming the control and why it is unavailable;
  and `best_effort()` is an explicit opt-in whose result reports what was
  dropped. Two backends behind one trait — `ProcessSandbox` (cgroup v2,
  namespaces, `RLIMIT_*` and `no_new_privs` on Linux; job objects on Windows;
  resource limits and an honest refusal elsewhere) and `MicroVmSandbox`, which
  gets the controls no host kernel gives an unprivileged process by having a
  different kernel. Exposed to agents as `sandbox.run` and
  `sandbox.capabilities`, dispatched against a `SandboxHost` the way `vm.*`
  dispatches against a `VmHost`; with no host installed the tools refuse,
  because the alternative to confinement is not running the program unconfined.
  See `docs/SANDBOXES.md` and `docs/GUEST_AGENT.md`.
- **Boot sources — a VM can now be told what to boot** (`hv2-core`): the new
  `BootSource` (`boot::source`) describes a Linux bzImage (with optional initrd
  and command line), a Multiboot kernel, or a raw image, and resolves to a
  validated `LoadedBoot` with every image read off disk. `VMConfig` gained a
  `boot` field, so a boot source is part of a VM's configuration and survives
  serialization.
- **`VM::provision` and `VM::launch`** (`hv2-core`): `provision()` creates the
  backend VM — the WHPX partition or KVM VM fd, which nothing previously did —
  loads the boot images into guest physical memory, and leaves vCPU 0 at the
  entry point. `launch()` provisions, starts, and runs the guest on a background
  task; `stop()` reaps that task and reports any error from it. Together these
  close the gap between "a VM exists" and "a guest is executing".
- **`HypervisorBackend::load_boot`** (`hv2-core`): backends load a boot source
  into their own guest memory and set up the architectural state the protocol
  needs. Implemented for WHPX (delegating to the existing `WhpxVcpu::boot_linux`
  / `boot_multiboot` sequences, previously unreachable from `VM`), for KVM
  (flat 32-bit protected mode with a boot GDT for Linux, real mode for raw
  images), and for TCG. The default reports `NotSupported`, so a backend that
  cannot boot a guest says so instead of starting a VM that will never execute.
- **`KvmVm::write_guest_memory`** (`hv2-core`, Linux): bounds-checked host-side
  writes into the KVM slot-0 guest allocation.
- **Boot flags on `hm t2 create`** (`hm-cli`): `--kernel`, `--initrd`,
  `--cmdline`, and `--image`. The boot source is validated at create time — a
  bad path fails immediately rather than registering a VM that cannot start —
  persisted in the VM registry, and used by `hm t2 start`, which now launches
  the guest instead of only flipping state. A VM created without a boot image
  says so, at create time and in the logs at start time.
- **`boot` on the REST and MCP VM-creation payloads** (`hv2-api`, `hm-cli`):
  `POST /api/v1/vms` accepts a boot source (rejecting a malformed one with 400
  rather than 500) and `POST /api/v1/vms/{id}/start` launches a VM that has one.
- **`VmHost`: the MCP `vm.*` tools now drive real VMs** (`hv2-agent`): a new
  `vm_host` module defines the `VmHost` trait and an in-process `LocalVmHost`
  backed by `AgentVM`. `McpServer::set_vm_host` installs one; without it the
  server keeps its previous session-state behaviour, so tool schemas and agent
  logic stay testable with no hypervisor present. The server enforces session
  ownership before dispatching, and a non-owner's probe is indistinguishable
  from a VM that does not exist.
- **Multiboot boot on the KVM backend** (`hv2-core`): previously
  `NotSupported`. Enters 32-bit protected mode with `EAX` holding the
  bootloader magic and `EBX` the `multiboot_info` address.
- **`load_boot` on the HVF backend** (`hv2-core`, macOS): previously the
  `NotSupported` default. HVF's `create_vm` now also allocates and maps guest
  RAM at GPA 0 — it never had any — and `HvfBackend::write_guest_memory` writes
  into it. Linux, Multiboot, and raw images are all supported, entered through
  VMCS guest-state fields.
- **`MultibootProtocol::prepare_guest_memory` and `MultibootLayout`**
  (`hv2-core`): one shared Multiboot memory layout that every backend writes,
  so their guest images cannot drift apart.
- **`PlanExecutor`, `SimulatingExecutor`, and `VmHostExecutor`** (`hv2-api`):
  ontology plans now execute for real. `HyperMachineOntology::execute_plan_with`
  dispatches each step through an executor and, on failure, runs a genuine
  compensating action — a rolled-back plan destroys the VM it created. A step
  whose compensation fails is reported as *not* rolled back, with the reason, so
  a leaked resource is visible rather than hidden.
- **`GpuHost`, `InMemoryGpuHost` (`hv2-agent`) and `AgentGpuHost`
  (`hv2-runtime`)**: the `gpu.*` agent tools now reach the real
  `GpuTopologyMap`. Attaching a GPU removes it from the placement pool
  fleet-wide, so the scheduler stops offering a device an agent already took —
  something per-session bookkeeping could not model. Same trait-in-the-caller
  inversion as `VmHost`, which is what avoids the `hv2-runtime` → `hv2-agent`
  dependency cycle.
- **`GpuTopologyMap::devices` / `device` / `contains_device`** (`hv2-runtime`):
  enumerate the full inventory, including allocated devices.
- **Image admission control on the provisioning path** (`hv2-core`):
  `VM::set_image_registry` installs an `ImageRegistry` that `VM::provision`
  consults before loading a boot image, so denying or revoking an image now
  stops a VM booting it rather than only being reportable through
  `POST /api/v1/images/check-admission`. Admission is by SHA-256 of the bytes
  about to be loaded — `LoadedBoot::primary_image_digest`, matched by the new
  `ImageRegistry::check_admission_by_digest` — so renaming or moving a kernel
  cannot change the answer. Initrds and Multiboot modules are excluded from the
  digest; they are separate artifacts with their own registry entries. Opt-in:
  with no registry installed, any readable image boots exactly as before. A
  digest that cannot be computed (the `ring` feature is off) is a denial, never
  a pass.
- **`enforce_image_admission` on the API server** (`hv2-api`): the
  `/api/v1/images` routes and the VMs the API creates now share **one**
  `ImageRegistry`, so approving, denying, or revoking an image there decides
  whether a VM can boot it. `AppState::with_image_registry` installs it, and
  `ApiVmHost` carries the same registry so a plan-created VM is gated
  identically. Off by default and settable from TOML: `RegistryConfig::default`
  enforces, so enabling this against an empty catalogue refuses every boot image
  until images are registered and approved.
- **Policy governance over the MCP tool surface** (`hv2-agent`):
  `McpServer::set_policy_set` installs a `PolicySet` that is evaluated before
  every tool call — the questions capabilities cannot express, such as denying a
  destructive action on one named VM, or outside a maintenance window. A denial
  is refused *and* written to the audit log, since an unrecorded denial is the
  one an incident review most needs. Opt-in: with no set installed, the gate
  stays capabilities plus VM ownership, exactly as before. `PolicySet::new`
  denies by default, so an installed set must name everything agents may do,
  including tools added after it was written.
- **`VmMetrics` and `VmHost::metrics`** (`hv2-agent`): telemetry reaches the
  host. The `get_metrics` plan step and the `vm.metrics` agent tool now report
  a VM's real status, vCPU count, allocated memory, and uptime. Quantities the
  host cannot observe — `cpu_usage_percent`, `memory_used_bytes` — are `null`,
  never a placeholder number, so an agent can distinguish an idle guest from an
  uninstrumented one. `ApiVmHost` serves the same figures as
  `GET /api/v1/vms/{id}/metrics`, so a plan step and a direct request cannot
  disagree about a VM.

### Changed
- **`Admin` no longer implies `HostExec`** (`hv2-agent`). Every other capability
  is implied by the `Admin` wildcard, and every other tool acts on VMs the
  server manages; `sandbox.run` acts on the machine the server runs on. Leaving
  it in the wildcard would have granted host execution to every session already
  holding `Admin` the moment the tool shipped — a privilege expansion nobody
  would have written down. `AgentCapabilities::full()` now names it explicitly,
  so "full" still means full. `PolicyAction::HostExec` and
  `ToolCategory::GuestExecution` are likewise separate from their guest-side
  counterparts: a program in a guest cannot reach the host, and one on the host
  can, however well confined.
- **Documented the agent enforcement boundary** (`hv2-agent`, `hv2-api`). An
  audit of the security-shaped types found several that decide but never
  intercept, described as though they were active. `limits` claimed "real-time
  enforcement to prevent runaway agents"; `policies` claimed to control "what AI
  agents can and cannot do". Both are toolkits that take effect only where a
  caller consults them, and neither had a caller. `policies` has since been
  given an opt-in enforcement point on the MCP tool surface (see Added);
  `limits` remains consult-only, and its `RateLimiter` is *not* the limiter the
  MCP server uses — that is `McpConfig::rate_limit`, which was always enforced.
  `permissions` is wired into a request path by `hv2-api`'s permission
  middleware. `Operation.rate_limit`, published to agents through
  `GET /agentic/ontology`, is advisory: nothing reads it back to reject a
  request.
- The `vm.create` MCP schema advertises a structured `boot` object in place of
  the `boot_image` string, which no handler read.
- `/agentic/plans/execute`, as served by `create_router_with_state`, executes
  plans against the server's own VM inventory instead of simulating. Simulation
  remains available through `execute_plan` and is still tagged
  `"simulated": true`, so the two can never be confused.

### Fixed
- **`HypervisorPlatform::detect` reported KVM on hosts that could not use it**
  (`hv2-core`). It tested `Path::new("/dev/kvm").exists()`, which is true
  wherever the module is loaded -- including the very common case of a user not
  in the `kvm` group. `detect` then returned `Kvm` and every call after it
  failed with a permission error naming none of this. It opens the device now,
  which is the question it was always asking. Verified both ways on one
  machine: as root `Kvm`, unprivileged `Tcg`. Same shape as
  `SetInformationJobObject` succeeding not meaning memory is capped.
- **A container runtime that reported success for things that never happened**
  (`hv2-core`). `ContainerRuntime::start` invented a PID — `1000 + n` — and
  marked the container `Running`, so a caller could not tell it from a working
  runtime: the state was right, the PID was plausible, and nothing was confined
  or even executing. Writing the test for that turned up the same defect beside
  it: `kill()` returned `Ok(())` for a container in any state other than
  `Running`, reporting a signal delivered to a process that did not exist. Both
  refuse now, and the module says plainly that it models the OCI spec without
  implementing it. `Container::start` is unchanged and stays honest — it takes a
  PID from its caller, so whoever supplies one is the party that knows it is
  real.
- **The Linux sandbox claimed isolation the workload could see through**
  (`hv2-sandbox`). Found by running it on a real kernel rather than
  cross-compiling for one. `/proc` and `/sys` are not ordinary directories —
  each is a view of the namespace it was *mounted in* — so an inherited pair
  left a workload correctly PID 1 in its own namespace while it enumerated 48
  host processes, and showed only loopback on netlink while `/sys/class/net`
  listed the host's interfaces. "Cannot signal" was true and "cannot see" was
  not. Both namespaces now carry `CLONE_NEWNS` and remount the filesystem that
  describes them, and both probes rehearse the remount rather than only the
  `unshare`.
- **`best_effort` attempted controls the probe had already refused**
  (`hv2-sandbox`). On a host without cgroup delegation it promised to run with
  whatever was available and then failed trying to create a cgroup its own
  probe had reported unavailable. `Sandbox::run` now filters the spec through
  `SandboxSpec::without_controls` before handing it to a backend, so no backend
  can attempt a control its probe rejected.
- **Boot admission was decided after the backend had already been asked for a
  partition** (`hv2-core`). `VM::provision` called `backend.create_vm` first, so
  a refused image had still cost hypervisor resources — and on a host where
  `create_vm` fails the refusal was masked by the backend's error entirely.
  Resolution, admission, and the memory-fit check now all happen before the
  backend is touched.
- **A flaky PIT timer test** (`hv2-core`) asserted a background task's
  wall-clock throughput. The timer uses `MissedTickBehavior::Skip`, so under a
  loaded machine ticks are dropped rather than queued and the count falls short.
  It now drives `tokio::time::pause`/`advance` against virtual time, bounded so
  a timer that never ticks still fails. Adds `tokio`'s `test-util` feature as a
  dev-dependency.
- **The image allowlist was never consulted before booting an image**
  (`hv2-core`). `ImageRegistry`'s own doc comment claimed admission checks that
  "the scheduler and VM provisioning path call before launching workloads";
  neither did. `check_admission` was reachable only from an advisory REST
  endpoint and tests, so an image could be denied or revoked in the registry and
  still boot. There is now a real enforcement point (see Added), and the doc
  states what is and is not on it.
- **`AgentVMBuilder` had no setter for `capabilities` or `sandbox_config`**
  (`hv2-agent`). Both fields existed, were passed to `ScriptEngine` and
  `Sandbox`, and were unreachable from any caller — so every `AgentVM` ran with
  the defaults and neither control could be tightened. Added
  `AgentVMBuilder::capabilities` and `AgentVMBuilder::sandbox`.
- **`SandboxConfig::max_cpu_time` bounded nothing** (`hv2-agent`). Script
  execution was bounded only by the builder's `script_timeout`, so a caller who
  tightened the sandbox limit got no effect. `AgentVM::effective_script_timeout`
  now takes the stricter of the two.
- **A flaky `hv2-runtime` health test** raced a 1 ms `check_interval`: it
  recorded a probe and immediately asserted no check was due, which fails if the
  thread is descheduled for a millisecond. It now uses a long interval, which a
  never-checked VM is due under regardless.
- **Two rustdoc warnings** (`hv2-agent`, `hv2-core`) — a redundant explicit link
  target and an unresolved link to `LoadedBoot::entry_point` — restoring the
  0-warning rustdoc build.
- **`ScriptEngine` never enforced its capability set** (`hv2-agent`). The engine
  was constructed with a `CapabilitySet` and then never consulted it, so an
  engine deliberately built with no capabilities handed scripts the same VM view
  as a fully privileged one. `execute` now requires `Capability::VmRead`.
- **`execute_script` was described to agents as running inside the guest**
  (`hv2-api`, `hm-cli`). It evaluates a Rhai script *on the host* against a
  read-only view of the VM — there is no in-guest agent — and the scope holds
  four scalars, not the "VM control operations" the ontology advertised. Four
  descriptions across the REST ontology, the CLI ontology, the CLI MCP server,
  and the LLM adapter prompt were corrected, and the documented example was
  changed from `echo 'Hello'` (a shell command Rhai cannot parse) to Rhai.
- **`vm.metrics` returned hard-coded zeros for every counter** (`hv2-agent`).
  Zero is a measurement; an agent reading 0% CPU on a busy VM would draw the
  wrong conclusion. Unmeasured fields are now `null`.
- **Multiboot `mods_addr` pointed at the module data rather than the module
  descriptor array** (`hv2-core`, WHPX). A Multiboot kernel walking that pointer
  read its module's contents as if they were addresses, so it would silently
  fail to find its initrd. Module command-line strings were never written at
  all.
- **Ontology plan steps with no dependencies executed in nondeterministic
  order** (`hv2-api`): `topological_sort` seeded its queue from a `HashMap`, so
  independent steps ran in hash order and a plan that happened to work was a
  coin flip. Ties now break toward declaration order.
- **A `stop()` racing a `launch()`** reported the resulting `InvalidState` as a
  VM failure (`hv2-core`).
- `GpuTopologyMap::devices_on_host` excludes allocated devices; its doc comment
  said otherwise.
- Clippy lints introduced by a newer toolchain (`float_literal_f32_fallback` in
  `hm-gui`, `for_kv_map` and an unused macOS import in `hv2-core`), restoring a
  `-D warnings` clean build on every supported target.

### CI
- **Three workflow steps that had never run once** were repaired. `Miri` asked
  for the `miri` component on the *stable* toolchain: the job installs nightly,
  but `rust-toolchain.toml` pins `channel = "stable"` and overrides whatever the
  action installed, so bare `cargo miri` ran on a toolchain that never ships it
  — the same fix the HV1 jobs already carry. `Coverage` and `Benchmarks` both
  died before doing any work because `hv2-api`'s build script runs `prost-build`
  and neither workflow installed `protoc`, which every building job in `ci.yml`
  does. The `CLA` check pinned `contributor-assistant/github-action@v2`, a tag
  that does not exist — the action publishes only `vX.Y.Z` — so it failed to
  resolve on every pull request; note that repairing it makes an inert check
  enforce again. `Benchmarks` remains red for a separate reason: the action is
  configured with `tool: 'cargo'`, which parses libtest format, and the repo's
  benches are criterion, which does not emit it — `--output-format bencher` is
  not the fix, as it prints no `test <name>` prefix.


Master had been red since June, for reasons predating this work. Six
failures were stacked behind one another, each hidden by the one in front:

- **Tests `include_bytes!`d gitignored build artifacts.**
  `examples/guest_code/*.img` are `build.sh` outputs excluded by
  `.gitignore`, so a clean checkout failed to *compile*, which took down
  `cargo check` for `hv2-core` and cascaded into most jobs.
  `guest_code_integration.rs` and `guest_execution.rs` now skip when an
  image is absent, the way `exit_handling.rs` already skips without a
  hypervisor.
- **The HVF backend could never link on Apple Silicon.** It is built on
  Hypervisor.framework’s VMX API, which exists only on Intel Macs, but was
  gated on `target_os` alone. `cargo check` does not link, so this passed
  every cross-target check for months. Now gated
  `all(target_os = "macos", target_arch = "x86_64")`; Apple Silicon falls
  through to TCG.
- **`hv2-net` used `libc` on macOS without depending on it** — declared
  only under `cfg(target_os = "linux")`.
- **The ARM64 job asked an x86-only crate to build for ARM.** `hv1-core`’s
  own manifest says ARM belongs to `hv1-arm`; the step was removed rather
  than papered over. If `hv1-core` gains an ARM mode, it should return.
- **`coverage.yml` passed `--codecov` and `--html` to one `cargo llvm-cov`
  invocation**, which rejects the combination outright. Now one
  instrumented run plus two reports.
- **Four tests assumed the host had a working hypervisor** and failed on
  runners where `/dev/kvm` exists but is not accessible. The skip lives in
  the shared helper so later tests inherit it.

The HV1 jobs now pin an exact nightly (`nightly-2026-08-17`, the toolchain
that produced the first all-green run). Unpinned, they broke four times in
a day for reasons unrelated to any commit here: a new required method on
`core::iter::Step`, a renamed `rustc-abi` value, `unused_must_use` becoming
an error, and a yanked `spin` release. Note that `rust-toolchain.toml` pins
`channel = "stable"` and overrides the installed default, so those steps
name the toolchain explicitly; keep the date in the `with:` blocks and the
commands in step. `coverage.yml` and `security.yml` remain unpinned — they
did not break and use nightly for different tooling.

### Dependencies

- `x86_64` 0.15.4 → 0.15.5 — a recent nightly added required methods to
  `core::iter::Step`. The two are mutually exclusive by toolchain vintage.
- `bootloader` 0.11.15 → 0.11.17 — 0.11.15’s vendored target specs carry
  `"rustc-abi": "x86-softfloat"`, which current nightly rejects; 0.11.17
  ships `"softfloat"`. Its build script still fails to build its UEFI
  stage (`lock_api` 0.4.10, `can’t find crate for std`), reproducible on
  other nightlies and for the host target, so that step is
  `continue-on-error` pending an upstream fix.
- `spin` 0.9.8 → 0.9.9 — 0.9.8 is yanked.

## [1.1.0] - 2026-06-04

### Added
- **Complete agentic tool ontology** (`hv2-agent` / `hv2-api`): a 30-tool MCP
  registry (vm / gpu / guest / snapshot / network / agent / system — including
  `vm.pause` / `vm.resume` and the `gpu.*` fabric tools) is the single source of
  truth, projected identically to the native MCP manifest and the OpenAI /
  Anthropic / Gemini tool-use formats. All four agent transports are verified in
  lockstep by drift-guard tests.
- **GPU acceleration fast paths** (`hv2-gpu` / `hv2-core`): WGPU compute
  pipelines are cached across dispatches (no per-call recompile), and
  `TRANSFER_TO_HOST_2D` / framebuffer scroll collapse contiguous regions to a
  single `memcpy`.
- **VFIO passthrough mmap fast path** (`hv2-gpu`, Linux): MMIO maps the BAR
  regions (parsing the sparse-mmap capability chain) for lock-free volatile
  access, falling back to positioned `pread` / `pwrite` only for non-mappable
  ranges.
- **Real post-quantum cryptography** (`hv2-core`): ML-KEM (FIPS 203), ML-DSA
  (FIPS 204), and SLH-DSA (FIPS 205) now delegate to the audited pure-Rust
  RustCrypto crates (`ml-kem`, `ml-dsa`, `slh-dsa`) with real
  keygen/encapsulation/sign/verify and canonical byte serialization, replacing
  the previous SHA-256/HMAC API placeholders. Gated behind the default `pqc`
  feature.
- **Real RSA** (`hv2-core`): RSA key generation and PKCS#1 v1.5 / PSS signing
  via the pure-Rust `rsa` crate (the `ring` backend cannot generate RSA keys).
- **Agent-driven VM workflow example** (`hm-cli`): `agent_vm_workflow` shows an
  AI agent discovering tools (OpenAI/Anthropic/Gemini schemas) and driving a
  full VM lifecycle (`vm.create` → `start` → `execute_script` → `delete`)
  through the typed `ToolExecutor`.
- **MCP workload example** (`hv2-agent`): `agent_mcp_workflow` drives a complete
  VM lifecycle over the `McpServer` tool surface — capability-scoped session,
  tool discovery, provision → boot → `guest.exec` → snapshot → resize → restore
  → teardown — and prints the resulting audit log.
- **LLM tool-schema example** (`hv2-agent`): `llm_tool_schemas` projects the MCP
  tool registry into the OpenAI, Anthropic, and Gemini tool-use formats.
- **Multi-agent orchestration example** (`hv2-agent`): `multi_agent_orchestration`
  shows role-scoped agents, exclusive VM claims (conflict-free coordination), and
  inter-agent messaging via `AgentOrchestrator`.
- **GPU-fabric reservation example** (`hv2-runtime`): `gpu_fabric_reservation`
  publishes a GPU VM class, reserves capacity with SLA tiers, consumes/releases
  slots, and cancels — exercising `CapacityManager`.
- **MCP workflow regression tests** (`hv2-agent`): `tests/mcp_workflow.rs` locks
  in the schema↔handler parameter contract for the common-workload tools.
- **Guest-command round-trip** (`hv2-agent`): new `guest.exec.status` MCP tool
  and `AgentSession::deliver_guest_response` API complete the `guest.exec` /
  `guest.file.*` loop — an agent submits a command, the guest-agent channel
  delivers the result, and the agent polls it to completion. Covered by a
  regression test and shown in `agent_mcp_workflow`.
- **Recursive planner** (`hv2-agent`): the GOAP planner now performs real
  recursive backward chaining with backtracking to satisfy action
  preconditions, replacing the previous single-level stub.

### Changed
- **Crypto enabled by default** (`hv2-core`): `default = ["ring", "pqc"]`, so
  AES-256-GCM/SHA use the validated `ring` backend out of the box. The insecure
  software AES-GCM fallback is removed — without `ring` the operations return
  `NotImplemented` instead of weak crypto.
- **README/docs**: reconciled crypto claims — "FIPS-approved algorithm
  implementations, not a FIPS 140-3 validated module"; AES-256-GCM only; added
  an ECDSA P-521 caveat; PQC described as real and RustCrypto-backed.

### Fixed
- **KVM/HVF FFI doc-comment corruption** (`hv2-core`): repaired control bytes
  (VT/FF/ESC/BEL) and a split `///` comment in the Linux (`kvm_ffi.rs`) and
  macOS (`hvf.rs`) backends that produced module-scope syntax errors — invisible
  on Windows (cfg-gated) but breaking Linux/macOS compilation and `cargo fmt`.
- **Clippy**: resolved lints surfaced by enabling `ring`/`pqc` and by the
  toolchain bump (`needless_return`, `collapsible_match`, `unnecessary_sort_by`,
  `manual_checked_ops`); workspace is `clippy -D warnings` and `cargo fmt`
  clean. Aligned `clippy.toml` MSRV to 1.95.
- **MCP tool schema/handler mismatch** (`hv2-agent`): the published tool schemas
  disagreed with the dispatcher, so agents following the advertised contract got
  wrong results — `vm.create` ignored `cpu_cores`/`memory_gb` (silently creating
  a 1-CPU/512 MB VM), and VM/snapshot/network tools advertised `vm_name`/
  `snapshot_name`/`network_name`/`interface_name` while the handler read
  `vm_id`/`snapshot_id`/`network`/`interface_id`. Schemas and handlers are now
  consistent and covered by regression tests.
- **Cross-platform build & test** (workspace): the `cfg(target_os = "linux")`
  paths — VFIO passthrough, the TAP network device, and the KVM FFI — now build
  and lint clean on Linux (previously: undeclared `libc` deps, an escaping
  borrow in `enumerate_gpus`, unconditional WHPX imports, and `derive(Default)`
  on >32-element arrays, all invisible to the Windows dev host). Backend-
  dependent tests skip gracefully where `/dev/kvm` is unavailable, so the full
  suite is green on Windows, Linux, and macOS.

### Performance
- Measured with Criterion on an AMD Ryzen 9 9900X (`cargo bench`): **O(1) agent
  spawn** — a copy-on-write clone is ~9 ns regardless of fleet size (constant at
  1/16/64 baseline units) versus a full copy at 206 µs–9.3 ms; CoW first-write
  fault 73 ns; guest-memory read/write 27–96 ns / 9–19 ns; MCP tool dispatch
  547 ns (47 µs for 64 concurrent); snapshot 18–21 ns; AES-256-GCM ~9–10 GiB/s
  (AES-NI). See the README Benchmarks table.

### Security
- Triaged RUSTSEC-2023-0071 (the `rsa` crate's Marvin-attack timing
  side-channel) in `deny.toml` with justification; no fixed upstream release
  exists and HyperMachine does not expose RSA as an online decryption oracle.

## [1.0.0] - 2026-03-25

### Added
- **TLS/HTTPS support** (`hv2-api`): `TlsConfig`, `build_rustls_config()`, and
  `serve_tls()` for encrypted API transport using rustls.
- **Permission middleware**: Graph-based permission middleware wired into the API
  server with resource scope hierarchy and role-based access control.
- **Fuzz testing targets**: 7 `cargo-fuzz` targets covering API parsing, VM config,
  agent messages, CPU instruction decoding, memory operations, PCI config, and
  interrupt controller state.
- **Integration tests**: 15 tests for `hv2-agent`, 31 for `hv2-cpu`, 15 headless
  CI tests for `hm-gui`, and 15 end-to-end stack tests for `hv2-api`.
- **End-to-end stack tests** (`hv2-api`): Full-router tests exercising health checks,
  VM CRUD lifecycle, runtime sessions, workload scheduling, Prometheus metrics,
  agentic ontology (JSON-LD), AI tool formats, feature toggling, and snapshots.
- **Feature-gated GPU** (`hv2-gpu`): `wgpu-backend` and `vulkan-backend` features
  with optional dependencies so the crate compiles with `--no-default-features`.

### Changed
- **Axum 0.7 → 0.8 upgrade**: Bumped `axum` workspace dependency from 0.7 to 0.8,
  migrated WebSocket `Message::Text` to `Utf8Bytes`, updated all route path parameters
  from `:param` syntax to `{param}` syntax across 7 source files.
- **Version bump**: Workspace version `0.1.0` → `1.0.0` across all 13 crates.
- **Documentation link**: Updated `documentation` field in `Cargo.toml` from invalid
  `docs.rs/hypermachine` to GitHub docs directory.

### Fixed
- **Axum route parameter syntax**: All VM CRUD routes in `rest.rs` and ontology routes
  in `ontology.rs` were using `{id}` (Axum 0.8 syntax) while depending on Axum 0.7,
  causing all parameterized routes to return 404. Fixed by upgrading to Axum 0.8.
- **Unsafe audit**: Added `SAFETY` comments to 33 unsafe blocks across 7 files
  (`kvm.rs`, `interrupt.rs`, `memory.rs`, `serial.rs`, `svm.rs`, `vmx.rs`, `vm.rs`).

## [0.3.0] - 2026-03-24

### Added
- **Graph-based hierarchical permissions**: DAG-structured permission system for
  agentic AI systems with resource scope hierarchy (Root → Org → Tenant → Project → VM),
  role inheritance with cycle detection, controlled delegation with attenuation and
  depth limits, permission resolution engine, and append-only audit trail.
  23 unit tests covering the full permission lifecycle.
- **Unikernel lifecycle integration tests**: 19 tests covering boot protocols, guest
  memory management, and pool operations.
- **Expanded test coverage**: +32 tests across hm-cli, hv2-cpu, hv2-net, and hv2-api.

### Changed
- Removed internal business strategy and session planning documents from public
  repository (moved to .gitignore).
- Removed duplicate docs/GETTING_STARTED.md (root copy is canonical).
- Bumped Containerfile Rust version to 1.87.
- Replaced invalid crates.io category `"os"` with `"emulators"`.

### Fixed
- Resolved all remaining clippy warnings across the workspace.

## [0.2.0] - 2025-07-13

### Fixed
- **PIC 8259 cascade deadlock**: `acknowledge_interrupt()` and `send_eoi()` held master
  lock while re-acquiring it in slave path, deadlocking on `parking_lot::Mutex`. Added
  `drop(master)` before slave branch in both methods.
- **E1000 DMA undefined behavior**: `process_rx_ring_dma()` and `process_tx_ring_dma()`
  took `&[u8]` but wrote through immutable references via raw pointer casts (UB at
  opt-level=1). Changed to `&mut [u8]` with safe `copy_from_slice`.
- **E1000 RX queue drain**: `process_rx_queue()` consumed packets without DMA writeback
  when ring was configured, starving `process_rx_ring_dma()`.
- **IOAPIC EOI re-delivery**: `end_of_interrupt()` now checks `irq_state` bitmap after
  clearing `remote_irr` to re-assert level-triggered interrupts still active.

### Added
- **MCP guest agent operations**: `execute_command`, `read_file`, `write_file`,
  `list_processes` on the MCP guest agent interface.
- **E1000 DMA**: Full RX/TX DMA with guest memory read/write for the Intel 82540EM NIC.
- **VirtIO GPU**: Capset dispatch (Virgl, Virgl2, Venus, Cross-Domain) and
  `transfer_to_host_2d` with guest memory DMA.
- **xHCI USB**: Transfer ring processing (Normal, Setup, Data, Status TRBs).
- **Intel HDA**: CORB verb dispatch (get/set parameters, power, pin config, stream format,
  AMP gain, connections).
- **RSA crypto**: Software modular exponentiation for encrypt/decrypt.
- **Post-quantum crypto**: ML-DSA (Dilithium) and SLH-DSA (SPHINCS+) signature
  verification with hash-based schemes.
- **FIPS AES-GCM fallback**: Hand-rolled GHASH + CTR mode when hardware AES-NI unavailable.
- **TAP loopback mode**: Memory buffer mode for testing without OS TAP devices.
- **DurableStore External backend**: HTTP-based storage with retry and circuit breaker.
- **Linux boot helper**: `boot_with_mapper()` loads kernel, initrd, and cmdline via
  address-space mapper.
- **ACPI DSDT enhancements**: `_CRS` resource blocks, `CPU0` device scope, `\_S5_` sleep
  state object in AML bytecode.
- **PIC cascade tests**: `test_slave_pic_cascade` (fixed, formerly ignored) and
  `test_slave_pic_multiple_irqs` for IRQs 8/10/12/15 through cascade.

## [0.1.0] - 2025-01-01

### Added

- **Type-2 hosted hypervisor** (`hv2-core`) with vCPU, memory, and device management
  - KVM (Linux), WHPX (Windows), and HVF (macOS) backend support
  - Zero-copy memory mapping with `memmap2`
  - Full device model: serial console, block storage, RTC, PCI bus, interrupt controller
  - FIPS 140-3 compliant cryptography (AES-256-GCM, SHA-256/512, ECDSA, RSA)
  - JIT compilation engine for dynamic code
  - Snapshot and restore for VM state
- **Type-1 bare-metal hypervisor** (`hv1-core`, `hv1-boot`) with UEFI bootloader
- **CPU virtualization** (`hv2-cpu`) with x86_64, ARM64, and RISC-V instruction decoding
- **GPU virtualization** (`hv2-gpu`) with Vulkan/WebGPU, passthrough, and virtual GPU
- **Networking** (`hv2-net`) with full TCP/IP stack, TAP/TUN, virtio-net, and DHCP
- **AI agent interface** (`hv2-agent`) with MCP server, WASM plugin runtime, and scripting API
  - OpenAI/Claude/Gemini tool format support
  - Agent lifecycle management and learning framework
- **REST/gRPC API server** (`hv2-api`) with WebSocket streaming
- **CLI tool** (`hm-cli`) with integrated MCP server and VM management commands
- **Desktop GUI** (`hm-gui`) with virt-manager style interface and AI automation API
- **Deployment infrastructure**: Helm chart, Kubernetes manifests, Terraform configs
- **CI/CD**: Build, test, security audit, coverage, benchmarks, release, and deploy workflows
- **Security**: `cargo-deny` configuration, Dependabot, seccomp filtering, capability-based access

[Unreleased]: https://github.com/nervosys/HyperMachine/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/nervosys/HyperMachine/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/nervosys/HyperMachine/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nervosys/HyperMachine/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nervosys/HyperMachine/releases/tag/v0.1.0
