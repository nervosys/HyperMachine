# HyperMachine against mxc

[mxc](https://github.com/microsoft/mxc) (Microsoft eXecution Container) is "a
sandboxed code execution system for running untrusted code (model output,
plugins, and tools) on Windows, Linux, and macOS". It is an SDK that builds into
an application: the application gives a JSON request naming a command and its
containment rules, and mxc picks a backend and runs it.

HyperMachine's part that plays the same game is [`hv2-sandbox`](SANDBOXES.md): a
Rust library, in the caller's process, that runs a program under limits the
operating system enforces, with `hm sandbox run` as its command-line executor.
This page tracks one against the other.

mxc's column is from its README, read on 2026-10-09. It is a reading of what the
project says, not a test of it. HyperMachine's column is from its code and its
tests. mxc publishes no performance figures, so there are none to compare.

## The rule HyperMachine is built around

A sandbox that silently drops a control is worse than no sandbox. `hv2-sandbox`
reports which controls this host can enforce, and a request for one it cannot
is refused with the reason, unless the caller asked for best effort, in which
case the dropped controls are reported back. Whether mxc does the same is not
something its README settles, so this is stated as HyperMachine's property and
not as a difference.

## The matrix

| | mxc | HyperMachine |
|---|---|---|
| In-process SDK | Rust, .NET, Node | Rust in-process. For Node, [a client of the executor](../sdk/node/README.md), not an in-process binding, and not on npm. **No .NET** |
| Standalone executor | `wxc-exec`, taking a JSON request | `hm sandbox exec`, [one JSON request in and one response out](SANDBOXES.md#one-json-request); `hm sandbox run` for flags and live output |
| Versioned request and policy schema | yes, in `schemas/stable` | version 1, with a [JSON Schema](schemas/sandbox-request-v1.schema.json). One version so far, so nothing yet shows how a change is carried |
| Linux process backend | bubblewrap (default), lxc | own: user, PID, mount, network and IPC namespaces, `pivot_root`, cgroup v2, `no_new_privs`. No external tool |
| Linux VM backend | microvm, hyperlight | microVM on HyperMachine's own VMM |
| Windows process backend | process container (default), plus Windows Sandbox, WSL container, microVM, Hyperlight and isolation session, several marked experimental | job object for memory, process count and CPU time; AppContainer for no-network and for path confinement. Nothing else |
| macOS backend | seatbelt | seatbelt too, [through `sandbox-exec`](SANDBOXES.md#containment-on-macos): no network, confinement to granted paths, denied paths. CPU time and deadline besides. No memory or process-count limit |
| Filesystem policy | read-only, read-write and denied path lists | Read-only and read-write [path grants](SANDBOXES.md#path-grants), and [`confine`](SANDBOXES.md#only-the-granted-paths) to make them the whole list. Linux: an empty root made for the run holds the grants and nothing else. Windows: an AppContainer denies everything else outside the system directories. A [denied list](SANDBOXES.md#denied-paths) on Linux, which closes a path under a grant or on the host's filesystem. **No denied list on Windows**: refused, with the reason |
| Network policy | outbound controls, proxy support, host filtering on some backends | process backends: none or the host's, nothing between. MicroVM sandboxes have an egress gateway with allow and deny lists |
| UI policy | clipboard, display and GUI controls | Windows: [all of a job object's user-interface restrictions](SANDBOXES.md#ui-isolation-on-windows), as one switch. **Not selectable one by one, and nothing on Linux or macOS** |
| One-shot run | yes | yes |
| Stateful lifecycle (provision, start, exec, stop, deprovision) | yes, in the SDK | through the node daemon's API, not in-process |
| Access-denied diagnostics | yes, and an audit mode that records accesses to help write a policy | a refusal names the control and why; **no audit mode** |
| Streaming output, cancellation, deadline | streaming stdio sample | yes, all three |

## Where HyperMachine is behind

In rough order of how much they matter for the same use:

1. **Other languages.** mxc ships .NET and Node SDKs. HyperMachine has a Rust
   library, a JSON request any language can send to `hm sandbox exec`, and a
   Node package that sends it. That package starts the `hm` binary for each
   run, is not published to npm, and does not stream. There is no .NET
   package.
2. **Finer network policy for a process.** All or nothing today.
3. **A denied path list on Windows.** Linux has one. On Windows a request can
   say "only these paths" but not "this tree except that part of it": a deny
   entry for an AppContainer's own SID was tried and is not enforced.
4. **UI controls one by one, and off Windows.** Windows has one switch for
   all of them; mxc's policy names clipboard, display and GUI separately.
5. **An audit mode.**
6. **macOS limits.** No memory or process-count limit there, and no test on
   more than the one macOS version CI runs.

## Where HyperMachine has something mxc's README does not claim

- **A hypervisor of its own** behind the microVM backend, on KVM, with
  [sandboxes created in under a millisecond from a warm pool](WARM_POOL.md),
  [standby](STANDBY.md), forks with memory and disk, and an egress gateway.
  mxc's VM backends are other projects' (and experimental on Windows).
- **No external sandboxing tool on Linux.** The namespaces and cgroups are set
  up by the library itself.

Neither of these is a measured win over mxc: mxc was not run.

## What has been verified, and where

| Claim | Evidence |
|---|---|
| Linux process controls | `crates/hv2-sandbox` tests, run in CI with the capability they need |
| Windows job-object limits | `crates/hv2-sandbox` tests on Windows |
| Windows path grants | `process::windows::tests` on the same host: a granted file is readable inside the container and an ungranted one is not, a read-only grant refuses a write, a read-write grant's file is on the host afterwards, and the path's access-control list names no container once the run ends. `hm sandbox run --ro/--rw` was also run by hand |
| Only the granted paths, Linux | `process::tests::a_workload_confined_to_its_grants_reaches_them_and_nothing_else`, run as root under WSL2: `/etc/passwd` and an ungranted file are absent, a read-only grant refuses a write, a read-write grant's file is on the host afterwards, and the run's root is gone. Not yet seen in CI |
| Only the granted paths, Windows | `process::windows::tests::a_workload_confined_to_its_grants_cannot_read_the_users_other_files` on a Windows 11 host. `hm sandbox run --confine-paths --net host` was also run by hand: `curl` to the Internet answers 200, a granted file is read, and a file beside it is refused |
| Denied paths, Linux | `process::tests::a_denied_path_is_closed_on_the_hosts_filesystem` and `a_denied_path_is_carved_out_of_a_grant`, run as root under WSL2: a denied directory cannot be listed or read into, a denied file reads empty and a write to it leaves the host's file alone, and what is beside them is open. `hm sandbox run --deny` was also run by hand. Not yet seen in CI |
| Denied paths, Windows | not available. `process::windows::tests::a_denied_path_refuses_the_run`: refused by default, and dropped and reported under best effort. The first implementation, a deny entry for the container's SID, failed its own test: the file was read |
| Node client | `sdk/node` tests against the real binary: Windows 11 with Node 22 and WSL2 with Node 20, 8 of 8. CI runs them on Linux, Windows and macOS after building `hm`. Not yet seen in CI |
| Windows UI isolation | `process::windows::tests::a_ui_isolated_workload_cannot_change_the_desktop_it_runs_on` on a Windows 11 host: the same program sets a system parameter outside the restrictions and is refused inside them. By hand, `hm sandbox run --isolate-ui` running `clip` got "Access is denied". The test tries reading and emptying the clipboard only under CI, where losing its contents costs nothing; the other restrictions are set by the same call and not tried one by one |
| Windows no-network | `process::windows::tests`, on a Windows 11 host: a request that reaches a loopback listener with the host's network does not reach it from the container, and the listener sees no connection. `hm sandbox run` was also run by hand: `curl` to the Internet fails by default and succeeds with `--net host` |
| macOS | `process::unix_fallback::seatbelt::tests`, which can run only on CI's `macos-latest`; there is no Mac among the development hosts. They fail, not skip, where a profile cannot be applied. Not yet run |
