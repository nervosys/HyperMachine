# Agent sandboxes

Two things in this repository were named like sandboxes and confined nothing.

`hv2-agent`'s `Sandbox` says so itself: it is a policy object whose limits take
effect only where a caller consults them. `hv2-core`'s `container` module is
3,866 lines of namespace, cgroup and seccomp data structures whose
`ContainerRuntime::start` reads:

```rust
// In real implementation, would fork/exec and setup namespaces
// For now, simulate with a PID
container.start(1000 + self.container_count.load(Ordering::Relaxed) as u32)
```

A container reported as `Running` with a fabricated PID. Between them, a
repo-wide search for `seccomp|setrlimit|unshare|prctl|CreateJobObject` found
prose and struct fields, and not one confinement syscall.

`hv2-sandbox` makes some.

## The rule the design is built around

**A sandbox that silently drops a control is worse than no sandbox**, because a
caller who asked for no network and got one believes the opposite of the truth.
So the API is shaped to make that impossible to do by accident:

- `Sandbox::controls()` reports what this backend enforces **on this host**,
  determined by probing rather than by assuming.
- `SandboxSpec` asks for controls.
- Asking for one the backend lacks is `SandboxError::Unsupported`, naming the
  control *and why it is unavailable* — so an operator learns what to change.
- A caller who genuinely wants best-effort says so once, with
  `SandboxSpec::best_effort()`, and reads `SandboxOutput::unenforced` to find
  out what it actually got.

The default outcome of asking for confinement a host cannot provide is a
refusal.

## Two backends, one trait

| | `ProcessSandbox` | `MicroVmSandbox` |
| --- | --- | --- |
| Boundary | the host kernel's | a different kernel |
| Start-up | milliseconds | a VM boot |
| Where | `hv2-sandbox` | `hv2-agent` |

Both implement `Sandbox`, so choosing isolation strength does not change how a
caller asks for it.

### What each control costs, per platform

| Control | Linux process | Windows process | macOS process | microVM |
| --- | --- | --- | --- | --- |
| Memory | cgroup v2 `memory.max` | job object `JobMemoryLimit` | ✗ (`RLIMIT_AS` bounds address space, not usage) | the VM's own memory size |
| Process count | `pids.max` + `RLIMIT_NPROC` | job `ActiveProcessLimit` | `RLIMIT_NPROC` | the guest |
| CPU time | `RLIMIT_CPU` | job `PerJobUserTimeLimit` | `RLIMIT_CPU` | guest agent |
| Wall clock | kill the process group | terminate the job | kill the process group | guest agent |
| Network isolation | `CLONE_NEWNET` | ✗ | ✗ | no network device |
| Filesystem isolation | ✗ (see below) | ✗ | ✗ | the guest's own |
| Process isolation | `CLONE_NEWPID` + `CLONE_NEWIPC` | ✗ | ✗ | a separate kernel |
| No new privileges | `PR_SET_NO_NEW_PRIVS` | ✗ | ✗ | a separate kernel |

Every ✗ is reported at runtime with a reason, not discovered by a caller when
something escapes.

## Notes on the Linux backend

**Ordering is load-bearing**, and the code says why at each step:

1. Join the cgroup **first**, while still the original user — after
   `CLONE_NEWUSER` the process cannot write that file.
2. Resource limits and `no_new_privs`, which need no privileges.
3. One `unshare` for every namespace, so the kernel creates the user namespace
   first and grants the capabilities the rest need.
4. Write the id maps, only possible from inside the new user namespace and only
   after `setgroups` is denied.
5. **Fork again** if a PID namespace was created. `unshare(CLONE_NEWPID)` puts
   the *next* child in the new namespace, not the caller — without this step the
   workload runs in the host's PID namespace while the code claims otherwise.

Everything between `fork` and `exec` allocates nothing and calls only
async-signal-safe functions; every string it needs is built in the parent.

**Filesystem isolation is deliberately not implemented here.** Doing it
properly means `pivot_root` with a prepared root. A `chroot` that a retained
directory descriptor can walk out of would look like isolation and not be one,
which is the exact failure this crate exists to stop repeating. A caller who
needs it uses the microVM sandbox, which gets it from having a different kernel.

## Notes on the Windows backend

A job object is a real kernel limit, but a process must be **in** the job before
it runs — assigning after `spawn` returns leaves a window in which the workload
can allocate past the memory cap or spawn a process that escapes the set. So the
child is created with `CREATE_SUSPENDED`, assigned, and only then resumed, which
is why the backend walks the thread table: `std::process` gives no way to resume
a process, and a sandbox with a start-up hole is not one.

`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` means a panic on the host side cannot leave
a workload running.

## The empty environment

`SandboxCommand` starts with **no** environment variables, not the host's.
Inheriting would hand a sandboxed workload every credential in the parent's
environment, which is not a limit anyone asked to remove. A workload that needs
`PATH` is given `PATH`.

## What is verified, and where

- **Windows**: verified on this host, including a test that gives a workload a
  one-process job and confirms the kernel refuses the process it tries to spawn,
  and one that confirms an overrunning workload is killed. 18 tests.
- **microVM**: the control reporting and every refusal path are tested. Actually
  running a workload in a guest needs a booted guest, which is blocked on the
  same hardware gate as the rest of the boot path.
- **Linux**: type-checked with `--target x86_64-unknown-linux-gnu` under
  `clippy -D warnings`, and **not run on a Linux kernel**. The probe design
  limits the damage — it attempts each operation and reports what worked, so on
  a machine where something fails, the sandbox says so rather than claiming it —
  but "the probe passes" is not the same as "the isolation holds". Treat this
  backend as unverified until it has run somewhere real, and see
  `docs/handoff.html` for why no machine here can.
- **macOS**: type-checked with `--target aarch64-apple-darwin`, not run.

## Reaching it as an agent

Two tools, dispatched against a `SandboxHost` the way `vm.*` dispatches against
a `VmHost`:

- **`sandbox.capabilities`** — what this host can confine, and why it cannot
  confine the rest. Worth asking before `sandbox.run` if a limit matters:
  a request for confinement this host cannot provide is refused, not downgraded.
- **`sandbox.run`** — run a program on the host under confinement.

Three things about that surface are deliberate:

**With no host installed, the tools refuse.** The alternative to confinement is
not running the program unconfined; it is not running it.

**The defaults are the strict ones.** A request naming no limits gets 512 MiB,
30 seconds, no network, no new privileges, processes isolated. A field nobody
set can never mean "unconfined", so the first careless caller does not get the
server's own privileges.

**`Admin` does not imply `HostExec`.** Every other capability is implied by
`Admin`; this one has to be granted by name. Every other tool acts on VMs the
server manages, and this one acts on the machine the server runs on — folding
it into the existing wildcard would have handed host execution to every session
already holding `Admin` the moment the tool shipped, which is a privilege
expansion nobody would have written down.

Unknown request fields are rejected rather than ignored, because a misspelled
`allow_network` should not silently become the default in either direction.

## What this does not replace

`hv2-agent`'s `Sandbox` still bounds Rhai scripts in-process, and that is a
different job: those limits are engine limits, not OS limits, and the module is
honest about which is which. `hv2-core`'s `container` module remains a model.
Nothing here changes either.
