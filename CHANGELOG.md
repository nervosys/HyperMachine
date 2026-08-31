# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **A guest channel can be asked for through the tool surface** (`hv2-agent`).
  `vm.create` takes an optional `guest_cid`; `LocalVmHost` attaches the channel
  before launching and merges `virtio_mmio.device=` into the guest's command
  line first, because virtio-mmio has no enumeration and the address has to be
  on the command line before `VM::new` freezes it. After attaching it checks
  the rendered argument against what the device reports and fails the start on
  a disagreement -- a guest that probes the wrong address finds nothing and
  says nothing, so a warning would be useless.
- **A host can publish to the guest and signal it** (`hv2-core`).
  `VM::notify_vsock` moves packets the host queued into the receive buffers a
  driver posted and then raises the used-queue interrupt -- two steps that have
  to happen together: publishing without signalling leaves data in a ring the
  guest has no reason to read, and signalling without publishing wakes it to
  find nothing. `VsockDevice::deliver_pending` exposes the first half, which
  previously ran only when the *guest* kicked a queue -- the wrong trigger for
  a host-initiated message, since the guest kicks when it posts buffers and
  then waits.
  `VirtioMmioTransport` also routes its interrupt through the VM's
  `InterruptSink` now. It raised only on the userspace `Pic8259`, which a guest
  with an in-kernel irqchip never reads, so every virtio interrupt this device
  raised went nowhere.
- **A Linux guest driver binds to the vsock device** (`hv2-core`). The two
  halves of this feature had never spoken: the device was proven against tests
  that lay out virtqueues by hand, the guest agent over the host kernel's own
  socket. A guest now enumerates it and completes the handshake:

      /sys/bus/virtio/devices/virtio0/device  ->  0x0013   (VIRTIO_ID_VSOCK)
      /sys/bus/virtio/devices/virtio0/status  ->  0x0000000f
      /sys/bus/virtio/devices/virtio0/driver  ->  vmw_vsock_virtio_transport

  `0x0f` is ACKNOWLEDGE | DRIVER | DRIVER_OK | FEATURES_OK -- full feature
  negotiation, by Linux's real driver, against this device. Reaching it needed
  a kernel built with `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`, which the WSL
  kernel does not set, so `virtio_mmio.device=` enumerated nothing before.
- **`Machine::legacy_pc()`** (`hv2-core`): the legacy PC device set in one
  call, replacing three hand-registrations a caller had to get right in order.
  Each entry records what breaks without it, because each was found the same
  way -- a guest hung, and the port it was spinning on named the device.
- **A boot regression test** (`hv2-core`). It asserts the e820 bounds
  *computed from the VM's configured memory*, so a table describing memory the
  guest does not have fails, and it skips -- naming what is missing -- where
  there is no `/dev/kvm` or no kernel image, rather than passing vacuously on a
  host that cannot execute a guest.
- **Userspace output reaches the console** (`hv2-core`). A program running as
  PID 1 inside the guest writes to `/dev/console` and the bytes come back out
  of `VM::console_output`. That completes the path: kernel boots, initramfs
  unpacks, init runs, writes through the tty layer, the 8250 driver takes a
  transmit interrupt, and the device hands the byte to the host.
- **egui, eframe and egui_extras moved 0.31 → 0.36** (`hm-gui`), which is what
  dependabot PRs #65, #67 and #69 each needed and none could do alone -- the
  three move as one ecosystem and CI failed on every one of them separately.
- **Device interrupts have a path to the guest** (`hv2-core`).
  `HypervisorBackend::set_irq_line` asserts and releases a line through the
  interrupt controller, which for KVM means `KVM_IRQ_LINE` on a
  `KvmVm::irq_line` that had been written and never called. It is deliberately
  not `inject_interrupt`: that hands a vector straight to the vCPU, bypassing
  the controller's masking and priority, and is wrong whenever an in-kernel
  irqchip exists. `Device::pending_interrupt` lets a device report the line it
  is asserting, polled after every access because that is when the condition
  changes, and `SerialDevice` implements the 16550's two sources. A backend
  that cannot drive a line says so rather than accepting the call and dropping
  it.
- **A guest reaches userspace** (`hv2-core`). The kernel boots, unpacks an
  initramfs, and executes a statically linked binary as PID 1 inside the guest.
  The proof is the kernel's own: an init that returns 42 produces
  `Attempted to kill init! exitcode=0x00002a00`, which only happens if that
  binary ran. Before it, the full boot -- RCU, the scheduler, SLUB, ftrace, the
  8250 driver finding COM1 at `0x3f8 (irq = 4) is a 16550A`, every filesystem
  registered, `Freeing unused kernel image (initmem) memory`. Userspace
  *output* does not come back yet; see below.
- **A Linux kernel boots** (`hv2-core`). It decompresses itself, enters the
  kernel proper, reads the `e820` map this loader writes, sets up its zones and
  prints ~20 lines of kernel log through `SerialDevice` to `VM::console_output`
  -- the memory ranges it reports back (`0x1000-0x9efff` and
  `0x100000-0x7fffffff`) are exactly the two entries handed to it. It does not
  reach userspace: it currently stops polling the DMA controller, which is the
  next legacy device to bring up. `examples/linux_boot_probe.rs` runs it and
  says how far it got.
- **Single-step tracing** (`hv2-core`). `VM::single_step_trace` steps a guest
  one instruction at a time and reports where it went, which is the only way to
  see either of the two ways a guest goes quiet: a triple fault arrives as
  `VmExit::Shutdown` and KVM resets the vCPU on AMD before returning it, so the
  registers afterwards describe the reset vector and not the fault; and a guest
  spinning in a tight loop never exits at all. Addresses are recorded *before*
  each step for that reason, and only a bounded tail is kept, because a guest
  can execute millions of instructions before it fails and the interesting part
  is always the end. It found the setup-header truncation below in one run,
  after an afternoon of reasoning had not.
- **Context as an environment** (`hv2-context`, `hv2-agent`). An agent's history
  is normally kept by putting it back in the prompt, which forces the decision
  about what matters to be made at *write* time: when a tool returns 40 MB,
  something has to choose what to keep before anyone knows what the next
  question will be, and whatever it discards is gone. The new crate implements
  the alternative from Scroll (arXiv:2608.21690) -- keep the history outside the
  context, as something the agent queries -- built on one invariant: **eviction
  changes the view and never the record**. `EventLog` is append-only and offers
  no operation that edits or removes an event, not as discipline but as absent
  API; every event has a `Seq` that never changes and never repeats. Payloads
  over 8 KiB move to a `PayloadStore` behind a handle, and are still indexed and
  searchable by their whole content rather than by the preview the log keeps.
  `SearchIndex` ranks with BM25 and returns addresses and previews, never
  content. `WorkingView::evict` persists before it selects -- so nothing can
  leave before it is addressable -- then protects the active turn and the recent
  tail, folds unprotected tool payloads down to their addresses, and only then
  evicts, leaving a `Headline` in a tiered `EvictionIndex` that keeps recent
  history detailed, coarsens distant history, and carries the exact span at
  every tier. `ContextRuntime` is somewhere to compute over what was retrieved
  under `hv2-sandbox`, so the answer comes back instead of the data. Exposed as
  seven MCP tools -- `context.search`, `expand`, `record`, `exec`, `compact`,
  `view`, `status` -- dispatched against a `ContextHost` the way `sandbox.*`
  dispatches against a `SandboxHost`; with none installed they refuse, because a
  record that accepts every write and loses it is worse than none. `context.exec`
  needs `HostExec` as well as the new `ContextMemory` capability: being able to
  read the record is not a reason to run code on the machine holding it. The
  runtime is a confined process with a durable workspace and not a resident
  namespace -- files persist between calls, variables do not -- and
  `docs/CONTEXT_AS_ENVIRONMENT.md` says so alongside the rest of what is not
  built.
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

### Known gaps
- **A device interrupt is delivered, but only as a side effect of a guest
  access.** `pending_interrupt` is polled after each I/O access, so a device
  whose condition becomes true while the guest is *not* touching it -- input
  arriving on a serial port, a timer expiring -- has no way to say so until
  the guest happens to ask. That is enough for a UART the guest is actively
  driving and is not enough in general.

### Known gaps
- **No vsock data has moved yet.** The driver is bound and the guest agent is
  listening on port 1024 inside, but a host-initiated connection stays in
  `Connecting`: the guest never answers, so the host-to-guest packet is not
  completing through the receive queue. Enumeration and negotiation are done;
  the data path is not, and `vm.exec` remains an API with nothing behind it.
- **A device interrupt still needs a guest access to be noticed in one case.**
  `pending_interrupt` is polled after an access; self-raised interrupts go
  through `InterruptSink` instead. Devices that use neither -- currently the
  virtio transport -- cannot signal the guest at all.

### Known gaps
- **vsock still moves no data, and the reason is now specific.** With the
  publish-and-signal path in place, a host-initiated connection still stays in
  `Connecting`, and the device can see why: the guest's driver has programmed
  all three ring addresses for the receive queue (`desc`, `avail`, `used`, and
  `QUEUE_READY` set, size 256) but `avail_idx` reads 0 and the descriptor table
  and available ring are both **all zeros** in guest memory at the addresses
  the driver itself supplied. So the device is not failing to find posted
  buffers -- there are none to find. Whether Linux's `virtio_vsock` driver
  never filled the queue, or filled it somewhere this VM's view of guest memory
  does not reach, is the next thing to establish. Writes in the other direction
  are known good: this is how the kernel image gets into the guest.
- **`vm.exec` cannot reach a guest through the tool surface at all**, for a
  reason upstream of the above: nothing on the MCP path ever attaches a vsock
  device. `LocalVmHost::start` never calls `attach_guest_channel`, `VmSpec` has
  no field for a guest CID, and nothing merges `vsock_kernel_args()` into the
  guest's command line -- only the boot probe does that, by hand. The host-side
  chain from `vm.exec` down to `VsockDevice` is otherwise unbroken and drives
  the emulated device directly rather than the host's own `AF_VSOCK` stack.
- **Concurrent `vm.exec` on one VM is not supported**: the host port is a
  constant and `VsockDevice::connect` refuses a duplicate port pair, so a
  second call collides with the first.

### Fixed
- **A timer test measured the host's scheduler rather than the timer**
  (`hv2-core`). It counted PIT ticks across a real wall-clock second and
  allowed 15 to 21. The interval uses `MissedTickBehavior::Skip`, which is
  right for a timer and means a loaded machine genuinely loses ticks, so the
  test failed under parallel builds for a reason unrelated to the code -- it
  had done so repeatedly. It runs on paused time now, where the count is exact
  (19) and the assertion fails only if the configured period changes.
- **The transmit interrupt was never delivered, for three separate reasons**
  (`hv2-core`). Each one hid the next.
  1. `IIR` gated the transmit interrupt on a `thr_empty` flag that was set
     false on every write to the transmit register and only set true again
     when the *host* drained the buffer. So after the first byte, the register
     that a driver's handler reads to find out why it was interrupted said
     "nothing to do" -- forever. This device transmits the instant the guest
     writes, so the holding register is empty again before the write returns.
  2. The interrupt pulse was wired into `handle_exit_static` only, and a
     single-vCPU guest takes the instance path. The plumbing was correct and
     ran zero times; a trace counting pulses reported exactly none.
  3. A FIFO reset erased the transcript, fixed earlier in this release, which
     is why the first two were invisible: the boot log looked empty for a
     reason that had nothing to do with interrupts.
- **A correction to an earlier diagnosis in this changelog.** An entry here
  previously said the guest "writes `IER = 0` every time and never sets the
  `OUT2` bit in `MCR`", concluding it ran the port without interrupts. That
  was wrong, and wrong in an avoidable way: it was measured from a capture
  that never reached the tty driver's startup, because the logging needed to
  see the registers slowed the guest enough that it never got there. Traced
  properly -- narrow filter, full boot -- the driver writes `IER = 0x07`
  sixty-seven times and `MCR = 0x0b`, `OUT2` included. It was asking for
  interrupts all along and not getting them.
- **The initrd was placed where the kernel unpacks itself** (`hv2-core`). It
  went to a fixed 32 MB. A compressed kernel unpacks into `init_size` bytes
  starting from where it will run -- 62 MB from 16 MB for the kernel tested
  here -- so 32 MB is *inside* that region for any kernel of ordinary size, and
  decompression wrote straight over the initrd. The kernel then reported
  `invalid magic at start of compressed archive` about bytes it had destroyed
  itself, fell back to treating it as a block device, and panicked with
  `Unable to mount root fs`. The initrd is now placed as high as it will go,
  under the header's `initrd_addr_max`, clamped to the guest's memory, page
  aligned, and refused with "give the guest more memory" if it cannot be
  placed clear of the unpack region. Two tests that asserted the fixed address
  now assert the property instead, since the constant is what hid the
  collision.
- **A FIFO reset erased the console transcript** (`hv2-core`). Linux resets
  both FIFOs when its 8250 driver takes the port over from earlyprintk, and
  `SerialDevice` responded by clearing the transmit buffer -- so every guest
  that got far enough to initialise its serial driver properly wiped its own
  boot log on the way past, and the VM looked as though it had never printed at
  all. On real hardware a transmit-FIFO reset discards bytes that have not gone
  out yet; here a THR write *is* the transmission, so there is nothing unsent
  to discard and the buffer is the transcript. The receive direction still
  discards, because unread input is exactly what that reset is for.
- **Every device access was four bytes wide, whatever the guest asked for**
  (`hv2-core`). `IoDeviceHandle`/`MmioDeviceHandle` built a `[u8; 4]` and
  handed the whole thing to the device, discarding the access size the exit
  had reported. Register files are byte-wide and their reads have side
  effects, so a one-byte `inb` of a UART made the device pop its receive
  buffer *and* clear its interrupt-identification register; a one-byte `outb`
  of a character wrote the three registers after the target with the padding
  bytes, clearing interrupt-enable, FIFO-control and line-control on every
  character printed. It looked like it worked, which is why it survived a
  kernel boot. The width now travels with the access and a device is handed
  exactly the bytes the guest asked for.
- **An unhandled guest exception livelocked the VM** (`hv2-core`). Only a
  double fault stopped the VM; every other exception was logged at warn level
  and the vCPU resumed, which re-executed the faulting instruction and took the
  same exception again, forever. A guest that hit one stopped making progress
  while the VM stayed `Running` and nothing said why -- observed as roughly
  60,000 exits a second, all identical. An exception reaches userspace only
  because the backend could neither handle nor emulate it, and this VMM does
  neither and cannot yet inject one back into the guest, so it stops and
  reports the vector. `KVM_SET_VCPU_EVENTS` would make injection possible and
  turn this into the fallback rather than the whole answer.
- **The PIT refused wide reads and dropped wide writes** (`hv2-core`, found by
  an audit prompted by the three device defects a kernel boot turned up).
  `TimerDevice::read` returned `Err` for any access that was not exactly one
  byte -- and an error there stops the VM -- while `write` consumed `data[0]`
  and silently ignored the rest. Both now walk consecutive registers a byte at
  a time, which is also what the channel latch semantics require. The same
  audit found the same shape in `IdeController` and `VgaDevice`; neither is
  registered with the device manager, and both now say so in their module
  docs, along with what wiring them up would require.
- **The setup header was truncated at the length it had in 2009** (`hv2-core`).
  `create_boot_params` copied a fixed `0x1f1..0x250` out of the bzImage, which
  was the whole header under boot protocol 2.09 and has not been since. The
  header does not have a fixed length -- the image says where it ends, at
  `0x202 + the byte at 0x201` -- so everything past 0x250 reached the guest as
  zero. `init_size` at 0x260 is the field that bites: the kernel computes its
  stack pointer from it, so zero put `%rsp` somewhere unmapped and the guest
  triple-faulted on the first `push`, with no console, no exception, and a
  vCPU that KVM had already reset. The header extent is read from the image now
  and clamped at both ends, since it is a guest-supplied number deciding how
  much gets copied.
- **Three devices that could not work behind the device manager** (`hv2-core`).
  All three shared a shape: correct when a unit test called them directly, and
  broken the moment a guest did. `SerialDevice::read` refused anything but a
  single byte, and the refusal reached `handle_exit` as a device error that
  stopped the VM -- so a kernel probing the port with a word read killed its own
  guest, where hardware would simply have answered; its `write` silently dropped
  every byte after the first. `RtcDevice` and `KeyboardDevice` decoded the
  absolute ports 0x70/0x71 and 0x60/0x64, but `DeviceManager` passes
  `port - base_port`, so every real access arrived as a small offset, fell
  through to an error arm, and stopped the VM. A 16550 and an i8042 are
  byte-wide register files: a wide access now walks consecutive registers, and
  a port the device does not implement reads as absent rather than as a
  failure. Found by booting a kernel against them, one after the next.
- **A search hit returned the whole payload when it happened to be small**
  (`hv2-context`, found by its own integration test). Previews were bounded in
  the payload store and nowhere else, so an inline payload -- anything under the
  8 KiB externalization threshold -- came back from `search` in full, once per
  hit. A search over a long session was then as expensive as re-reading it,
  which is the exact cost the crate exists to avoid. The index bounds previews
  itself now, whatever the storage decided.
- **A Linux kernel was handed no memory map and no CPU** (`hv2-core`).
  `BootSource::Linux` has always described itself as implementing the Linux
  boot protocol. Running one found two pieces of it missing. `boot_params`
  carried no `e820` map: the byte at `0x1e8` was zero and the table at `0x2d0`
  was empty, and a guest booted this way runs no BIOS, so there is no
  `INT 15h` for the kernel to fall back on -- it finds no RAM at all and stops
  before it has a console to say so on. The map is built now from the guest's
  memory size, which `LoadedBoot::set_memory_size` threads in from the VM
  (separate from `load`, because callers validate images before a VM exists),
  and asking for the memory regions without one is refused rather than
  answered with an empty map. Separately, no KVM vCPU was ever given a CPUID
  configuration: `set_cpuid` and `get_supported_cpuid` were both written and
  neither was ever called, so the guest's `CPUID` reported no vendor, no
  features and a maximum leaf of zero. A Linux kernel asks within its first
  few dozen instructions. `KvmVm::create_vcpu` now applies the host's
  supported set. The visible effect is a guest that executes instead of
  spinning: it now runs and triple-faults, which is a failure that can be
  worked on. `examples/linux_boot_probe.rs` is the program that reports it.
  The 32-bit protected-mode entry state itself was verified separately and is
  correct -- a hand-assembled 32-bit image entered at 1 MB executes and drives
  COM1 -- so what remains is specific to the Linux path. The kernel still does
  not boot; nothing here claims otherwise.
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
- **The release workflow never ran because of a billing block, not a defect.**
  All four build jobs at tag `v1.1.0` reported `steps: []` and no runner, and
  the check annotation reads "The job was not started because recent account
  payments have failed or your spending limit needs to be increased." No
  amount of workflow repair fixes that. Real defects behind it were fixed
  anyway, so the first run after billing is resolved has a chance: `protoc` was
  never installed although `hv2-cli` pulls in `hv2-api`, whose build script
  needs it; the aarch64 Linux target was built through `cross` in a container
  the host's `protoc` could not reach, and is now cross-compiled directly with
  `gcc-aarch64-linux-gnu` and an explicit linker.
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
