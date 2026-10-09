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
| Process count | `pids.max` | job `ActiveProcessLimit` | ✗ (`RLIMIT_NPROC` counts the user's processes, not the workload's) | the guest |
| CPU time | `RLIMIT_CPU` | job `PerJobUserTimeLimit` | `RLIMIT_CPU` | guest agent |
| Wall clock | kill the process group | terminate the job | kill the process group | guest agent |
| Network isolation | `CLONE_NEWNET` + its own sysfs | an AppContainer with no capabilities | ✗ | no network device |
| Filesystem isolation | `CLONE_NEWNS` + `pivot_root` | ✗ | ✗ | the guest's own |
| Process isolation | `CLONE_NEWPID` + `CLONE_NEWIPC` + its own `/proc` | ✗ | ✗ | a separate kernel |
| No new privileges | `PR_SET_NO_NEW_PRIVS` | ✗ | ✗ | a separate kernel |
| Path confinement | an empty root made for the run, holding the grants | an AppContainer | ✗ | ✗ (a guest shares no host path) |
| UI isolation | ✗ (no boundary around a display server) | job object user-interface restrictions | ✗ | the host's desktop is not in the guest |
| Path denial | a mount over the path | ✗ (an AppContainer is not refused by a deny entry for its own SID) | ✗ | ✗ (a guest shares no host path) |

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
7. **Mount `/proc` and `/sys`**, so they land inside the new root instead of the
   one that is about to be discarded.
8. **Drop every capability**, last. The workload is root in its user namespace
   and holds none: not in force, not in the bounding set, not ambient. With
   `CAP_SYS_ADMIN` there it owns the mounts made above, and a read-only mount
   could be remounted writable. A program that expects root's powers inside
   the sandbox, to `chown` or to bring an interface up, does not have them.

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

### No network, on Windows

A workload asked to run with no network is started in an **AppContainer** with
no capabilities, inside the job. The kernel then refuses it every socket:
outbound, listening, and loopback. A request that reaches a listener on the
same machine with the host's network does not reach it from inside, and `ping`
reports that it cannot contact the IP driver at all.

Three things come with the container that nobody asked for by name:

- **It sees less of the filesystem.** An AppContainer's token is checked against
  every file it opens. It can read what Windows lets every packaged application
  read, which is mostly the system directories, and its own container folder.
  It cannot read the user's files, or a program installed outside those places.
  This is more confinement than a spec with no filesystem policy asked for,
  never less. A workload that needs something else is [granted](#path-grants)
  it.
- **Its working directory** is the container's own folder unless the caller
  names one, because the host process's directory is very likely one it may not
  open.
- **`LOCALAPPDATA` is set** when the caller's environment does not name it.
  Windows refuses to start a process in an AppContainer without it, and rewrites
  it to the container's folder, so that is what the workload sees.

The profile is made for one run and deleted after it, with its folder.

`hm sandbox run` denies the network unless told otherwise, so on Windows it now
runs its program in a container by default. Grant it what it needs with `--ro`
and `--rw`, or pass `--net host` to run it uncontained as before.

### UI isolation, on Windows

`SandboxSpec::isolate_ui` (`--isolate-ui`; `"isolateUi": true` in a request)
keeps a workload from the desktop it was started on. It is the control `UI
isolation`, and it is the job object's user-interface restrictions, all of
them together:

- reading the clipboard, and writing it. Emptying it is neither, and the
  first run of the test under CI showed a restricted program allowed to: it
  can clear what is on the clipboard, though not read it or put anything
  there;
- the windows and other user-interface handles of processes outside the job,
  so it cannot send them messages or read their contents;
- the system's parameters and the display's settings;
- the global atom table;
- making or switching desktops;
- logging the user off or shutting the machine down.

It is set on the job, which a workload in an AppContainer is in too; the two
together have not been tested. It is off unless asked for, and `SandboxSpec::untrusted` leaves it off, because only Windows has
it and a spec Linux refused outright would not be used. A Linux process has no
such boundary in this backend: a workload that can reach a display server's
socket can use it, and denying the network and confining its paths is what
keeps it from one.

### Path grants

A spec can name paths the workload must be able to reach where its containment
would otherwise hide them (`SandboxSpec::grants`; `--ro PATH` and `--rw PATH` on
`hm sandbox run`): readable, or readable and writable, with everything under
them.

```sh
hm sandbox run --ro C:\tools\mytool --rw C:\work\out -- C:\tools\mytool\run.exe
```

- **On Windows** a grant is an entry on the path's access-control list for that
  run's container, inherited by everything under it. The container's SID is the
  run's alone, so the entry opens the path to nothing else, and it is removed
  when the run ends. A process killed before it can do that leaves an entry for
  a SID that no longer names anything.
- **A grant is a floor, not a ceiling.** Where containment hides nothing, as
  with the host's filesystem on Linux, the path is already reachable and a
  grant changes nothing. It does not take other paths away; isolating the
  filesystem does that.
- **Inside an isolated root** a read-only grant is one more read-only mount. A
  read-write grant is a writable mount at the same path on Linux, and refused
  elsewhere, since there is nowhere to put it.
- A path that is not absolute, or does not exist, refuses the run.
- Granting a large tree rewrites every descriptor under it. Grant the directory
  the workload needs, not its parent.

### Only the granted paths

`SandboxSpec::confine_paths` (`--confine-paths`; `"confine": true` in a
request's `filesystem`) turns the grants from a floor into the whole list: the
workload reaches the paths it was granted and no others of the caller's, with
no root for the caller to build. It is a control of its own, `path
confinement`, reported and refused like the rest.

```sh
hm sandbox run --confine-paths --ro /usr --ro /lib --ro /lib64 --ro /bin \
               --rw /work --net host -- /usr/bin/python3 /work/job.py
```

- **On Linux** the workload is rooted in an empty directory made for the run,
  holding the granted paths at the places they have on the host: read-only
  ones read-only, read-write ones writable. Nothing else is there. Not `/etc`,
  not `/tmp`, not `/dev`, and not the program's own libraries unless they are
  granted, which is why the example grants `/usr` and `/lib`. The directory is
  removed when the run ends. It is available wherever filesystem isolation is.
- **On Windows** the workload runs in an AppContainer, with the network
  capabilities kept if the network was. It reaches its grants, its container's
  folder, and what Windows lets every packaged application read, which is
  mostly the system directories. That last part is why this is not called
  filesystem isolation: the list is the caller's plus the system's. Loopback
  is closed to an AppContainer whatever its capabilities, so a confined
  workload with the host's network reaches the Internet and the local network
  but not a server on the same machine.
- **On macOS** it is not available, and a request for it is refused.
- On top of `--fs isolated:ROOT` it adds nothing: that root already decides
  what is there.

### Denied paths

`PathGrants::denied` (`--deny PATH`; `"denied"` in a request's `filesystem`)
closes a path to the workload, with everything under it, whatever else would
let it in. It is the control `path denial`.

```sh
hm sandbox run --deny ~/.ssh --deny ~/.aws -- ./build.sh
hm sandbox run --confine-paths --ro /usr --ro /lib --rw /work --deny /work/.git -- /usr/bin/make -C /work
```

- **On Linux** the path is covered by a mount in the workload's own mount
  namespace. A directory gets an empty one that nobody may enter; a file gets
  the null device, since only a file can be mounted on a file, so it reads as
  empty and a write to it goes nowhere. The path is still there, covered, not
  removed. It works on the host's filesystem with nothing else taken away,
  under a grant, and under a mount of an isolated root.
- **The cover stays on.** The workload holds no capabilities, so it cannot
  unmount it, and in a user namespace of its own the kernel locks it.
- **On Windows and macOS it is refused.** On Windows this was tried and did
  not hold: an AppContainer is not refused by an access-denied entry for its
  own SID, and the file under one was read all the same. Grant the paths
  beside the one to keep closed instead.
- **A granted path under a denied one refuses the run.** A denial covers
  everything under it, and which of the two was meant is not guessed.
- A denied path must be absolute and exist. One that would not be reachable
  anyway, outside every grant of a confined workload, needs nothing and is
  accepted.
- The path is matched by name. A second name for the same file, a hard link or
  another mount of it, is not covered by denying the first.

Not claimed: filesystem isolation in the sense the Linux backend means it (a
root of the caller's choosing), process isolation, or no-new-privileges. An
AppContainer does restrict all three in its own way, and none has been tested
here against what those controls promise, so they stay reported as unavailable.

## One JSON request

A caller that is a program, in any language, can hand `hm` one document and get
one back:

```sh
hm sandbox exec request.json      # or on standard input
```

```json
{
  "version": 1,
  "command": ["python3", "-c", "print(6 * 7)"],
  "env": { "PATH": "/usr/bin" },
  "limits": { "memoryBytes": 268435456, "timeoutMs": 10000 },
  "network": { "egress": "deny" },
  "filesystem": { "readOnly": ["/usr"], "readWrite": ["/work"] }
}
```

```json
{ "version": 1, "exitCode": 0, "signal": null, "killedBy": null,
  "stdout": "42\n", "stderr": "", "unenforced": [],
  "backend": "process", "os": "linux",
  "controls": [ { "control": "network isolation", "enforced": true } ] }
```

- **The format is versioned**, and its schema is
  [`schemas/sandbox-request-v1.schema.json`](schemas/sandbox-request-v1.schema.json).
  A document with a version this build does not read is told so.
- **What is left out is the careful choice.** No `network` means no network. No
  `bestEffort` means a request this host cannot enforce is refused.
- **An unknown field is an error**, not something skipped. A misspelt `network`
  would otherwise run with the default the caller did not mean.
- **The environment is exactly `env`.** Nothing is inherited, so a request with
  no `PATH` has none.
- **The exit code is `hm`'s, not the workload's.** 0 means the run happened and
  the response has `exitCode`, `stdout` and `stderr`; 2 means it did not, and
  the response has `error.kind` (`invalid`, `unsupported`, `spawn`,
  `confinement` or `runtime`) and `error.message`.
- Output that is not UTF-8 is given as text with the bad bytes replaced, and
  whole in `stdoutBase64` or `stderrBase64`.

In Rust the same document is `hv2_sandbox::request::Request`. From Node,
[`sdk/node`](../sdk/node/README.md) sends it and types it.

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
- **macOS**: run on GitHub's `macos-latest` in the Test job, whose summary
  carries `what_this_host_enforces`'s report for each runner.
- **Linux with every control granted**: the *Sandbox Containment* CI job
  lifts Ubuntu's AppArmor user-namespace restriction and runs the tests in a
  delegated cgroup, and fails if the probe reports fewer than the ten
  controls Linux has or if any test skips. Before it existed, every containment test on
  `ubuntu-latest` passed by skipping.

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

Granting every control found a fifth, of a different shape: a limit that
over-reached instead of under-delivering.

5. `Control::ProcessCount` set `RLIMIT_NPROC` beside `pids.max`, as "belt and
   braces". `RLIMIT_NPROC` counts every task the *user* owns, host-wide and
   threads included. On a machine whose user ran 69 threads, a spec asking for
   64 processes (`SandboxSpec::untrusted`'s default) had every spawn refused
   with `EAGAIN`. Linux now relies on `pids.max` alone, and macOS, where
   `RLIMIT_NPROC` was the only mechanism, reports the control as unenforced.
   It went unseen because no host that had ever run the tests delegated the
   pids controller, so the tests that would have failed skipped.

`cargo run -p hv2-sandbox --example probe` prints what the machine you are on
can enforce, and asks a confined workload what it can see. Run it on any host
before trusting a limit there.

## From the command line: `hm sandbox run`

```
hm sandbox run [--memory 4G] [--cpu-time SECS] [--wall-clock SECS] [--max-processes N]
               [--net deny|host] [--fs host|isolated:ROOT] [--ro PATH]... [--rw PATH]...
               [--deny PATH]... [--confine-paths]
               [--workdir DIR] [--env K=V]... [--pass-env NAME]... [--clean-env]
               [--isolate-processes] [--no-new-privileges] [--isolate-ui]
               [--strict] [--report text|json|none] -- CMD [ARGS...]
```

Runs a host program under `ProcessSandbox`. Output is streamed as it arrives, through
`Sandbox::run_with` and a `RunIo` sink, and none of it is buffered, so a run can last hours.
`hm` exits with the program's exit code.

**The enforcement report.** Before the program starts, stderr lists every control the flags
asked for, marked `enforced` or `NOT ENFORCED` with the reason and the fix. An example is the
missing cgroup delegation that leaves the memory limit unenforced for an unprivileged Linux
user.

**Best effort by default.** A control this host cannot enforce is dropped, and it is named
again after the run. `--strict` refuses the run instead, exiting with 125.

**The environment.** It starts from what a program needs to run: `PATH`, `HOME`, `TEMP` and,
on Windows, `SystemRoot`. Nothing else crosses from the host unless it is named with
`--pass-env` or set with `--env`, so credentials in the caller's environment stay out.
`--clean-env` starts from nothing.

**Exit codes:**
- the program's own code;
- 124 when the wall-clock deadline killed the whole process tree;
- 128+N for signal N (on Linux the CPU-time limit ends in SIGKILL, so 137);
- 125 when the run was refused or confinement failed;
- 127 when the program could not start;
- 130 on Ctrl-C, which kills the tree through `RunIo::cancel`.

Checked by hand on this repo's hosts:
- **Windows:**
  - exit codes pass through;
  - `--wall-clock 2` kills a 30-second `ping` at 2 s (exit 124);
  - a 512 MB allocation under `--memory 64M` is refused;
  - output lines arrive live, 3.4 s apart, as the program prints them.
- **Linux, as an unprivileged user:**
  - the network is loopback only, and `--net host` sees the host's interfaces;
  - the program is PID 1 under `--isolate-processes`;
  - `--cpu-time 1` kills a spin loop (137);
  - a variable in the caller's environment does not reach the program;
  - without a delegated cgroup, the report marks the memory and process limits NOT ENFORCED
    and says what to change.

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
