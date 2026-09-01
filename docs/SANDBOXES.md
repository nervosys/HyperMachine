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
| Network isolation | `CLONE_NEWNET` + its own sysfs | ✗ | ✗ | no network device |
| Filesystem isolation | `CLONE_NEWNS` + `pivot_root` | ✗ | ✗ | the guest's own |
| Process isolation | `CLONE_NEWPID` + `CLONE_NEWIPC` + its own `/proc` | ✗ | ✗ | a separate kernel |
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
6. **`pivot_root`** if a root was named. It has to come after everything that
   reads a host path — `/proc/self/uid_map` at step 4, the cgroup file at step
   1 — because after it the host filesystem has no name at all.
7. **Mount `/proc` and `/sys` last**, so they land inside the new root instead
   of the one that is about to be discarded.

Everything between `fork` and `exec` allocates nothing and calls only
async-signal-safe functions; every string it needs is built in the parent.

**`/proc` and `/sys` are remounted, and that is not optional.** Neither is an
ordinary directory: each is a view of the namespace it was *mounted in*.
Inherit them and a workload reads the host's process table and the host's
network interfaces while being genuinely unable to signal or reach either.
Both halves of this were found by running on a real kernel — `$$` was already
`1` and netlink already showed only loopback, while `/proc` listed 48 host
processes and `/sys/class/net` listed the host's interfaces. Half an isolation
claim is worse than none, so the probe now rehearses the remounts and reports
the control only if they worked.

**Filesystem isolation is `pivot_root`, not `chroot`.** `chroot` moves where a
path walk starts and leaves the old root mounted: a retained directory
descriptor plus `fchdir` walks straight out of it. `pivot_root` moves the root
*mount*, and the two-step `pivot_root(".", ".")` followed by
`umount2(".", MNT_DETACH)` leaves the old root mounted nowhere — no path to it,
and no mount for a stale descriptor to resolve against. The descriptors are
closed anyway: everything this crate and `std` open is `O_CLOEXEC`, so the
workload starts holding nothing but its three standard streams.

`FilesystemPolicy::Isolated` names a root and a list of host paths to expose
read-only. The backend does not build a root — it mounts the one it is given, and
a root that does not exist is a refusal before anything is spawned. Read-only
paths appear at the same path inside the root (`/usr` → `/usr`), and their mount
points are created inside the root because a bind onto a path that is not there
fails and `mkdir` after the pivot would be too late.

**An isolated root contains only what the spec put in it**, and that includes
`/dev`. A workload that needs `/dev/null` — which a shell does, the moment
anything redirects to it — gets one by listing `/dev` in `read_only`. Nothing
is mounted on the caller's behalf except `/proc` and `/sys`, and only when the
matching namespace was created.

**Read-only binds are recursive, and both halves of that were forced by the
kernel.** A plain `MS_BIND` of a directory with anything mounted underneath it
is refused outright inside a user namespace (`EINVAL`): the kernel will not let
you create a mount that hides a mount you cannot unmount, and `/usr` on the
kernel this was written against has three submounts under `/usr/lib`. So the
bind is `MS_REC`. But then `mount(MS_REMOUNT | MS_RDONLY)` is not enough either
— it changes the top mount only, and would leave `/usr/lib/wsl/lib` writable
inside a mount the caller was told is read-only. `mount_setattr` with
`AT_RECURSIVE` covers the whole subtree, and sets `nosuid` with it. A kernel
older than 5.12 has no `mount_setattr`, and is reported as unable to isolate the
filesystem rather than quietly given the weaker version.

The probe rehearses all of it in a throwaway child — pivot into a scratch root
with a read-only bind — and then asks the three questions that decide whether
the claim is true: is a file inside the root still visible, is a file outside it
gone, and does the read-only mount refuse a write. A probe that stopped at
"`pivot_root` returned 0" would report the control on a host where the old root
was still reachable.

A caller who wants a boundary stronger than the host kernel's still uses the
microVM sandbox.

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

- **Windows**: verified on this host. Three tests assert enforcement rather
  than configuration: a one-process job where the kernel refuses the process the
  workload tries to spawn, a 256 MiB job where a 1 GiB allocation comes back as
  `OutOfMemoryException`, and an overrunning workload that gets killed. 19 tests.
  Note the memory one does not check the exit code — PowerShell catches the
  allocation failure and still exits 0, so the refusal itself is the evidence.
- **microVM**: the control reporting and every refusal path are tested. Actually
  running a workload in a guest needs a booted guest, which is blocked on the
  same hardware gate as the rest of the boot path.
- **Linux**: **run on a real kernel** (6.18, WSL2 Debian, as an unprivileged
  user). 27 tests, and the isolation assertions ask the *workload* what it can
  see rather than reading `controls()` back — a test that only did the latter
  would pass on a backend that reported the set and applied none of it. On that
  kernel the workload is PID 1 in its own namespace, sees 3 processes where the
  host has hundreds, has one network interface, cannot see a host file outside
  the root it was given, can read one inside it, and gets `EROFS` writing to a
  mount the spec asked to be read-only. Memory and process-count limits are
  *not* enforced there — the cgroup hierarchy is not writable — and the probe
  says so.
- **macOS**: type-checked with `--target aarch64-apple-darwin`, not run.

Running it on a kernel found two defects that type-checking could not, both of
the same shape — a claim that was true in the mechanism and false in what the
workload could observe:

1. `best_effort` was broken on any host without cgroup delegation. It promised
   to run with whatever the host could enforce, then had the backend attempt a
   cgroup the probe had already reported unavailable, and failed. Backends are
   now handed a spec filtered to what the probe said they enforce.
2. `/proc` and `/sys` were inherited, so "cannot see processes outside" and
   "no network" were half-true in the way described above.

Adding filesystem isolation found two more of the same shape, and neither is
visible to a compiler:

3. A non-recursive read-only bind of `/usr` fails with `EINVAL` inside a user
   namespace when `/usr` has submounts, and a recursive one that is then
   remounted read-only leaves those submounts writable. Only `mount_setattr`
   with `AT_RECURSIVE` makes the claim true.
4. The first read-only test passed for the wrong reason: it wrote
   `2>/dev/null`, and an isolated root has no `/dev`, so *both* redirections
   failed and the "refused" it asserted said nothing about the mount. The test
   now checks the writable direction too, which is what caught it.

`cargo run -p hv2-sandbox --example probe` prints what the machine you are on
can enforce, and asks a confined workload what it can see. Run it on any host
before trusting a limit there.

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
