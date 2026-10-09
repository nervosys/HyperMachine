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
| In-process SDK | Rust, .NET, Node | **Rust only** |
| Standalone executor | `wxc-exec`, taking a JSON request | `hm sandbox run`, taking flags. **No JSON request format** |
| Versioned request and policy schema | yes, in `schemas/stable` | **no** |
| Linux process backend | bubblewrap (default), lxc | own: user, PID, mount, network and IPC namespaces, `pivot_root`, cgroup v2, `no_new_privs`. No external tool |
| Linux VM backend | microvm, hyperlight | microVM on HyperMachine's own VMM |
| Windows process backend | process container (default), plus Windows Sandbox, WSL container, microVM, Hyperlight and isolation session, several marked experimental | job object for memory, process count and CPU time; **AppContainer for no-network**. Nothing else |
| macOS backend | seatbelt | **resource limits only**, and it says so |
| Filesystem policy | read-only, read-write and denied path lists | Linux: a root of the caller's choosing plus read-only mounts. Windows: whatever an AppContainer may read; **no way to grant a path**. No denied list anywhere |
| Network policy | outbound controls, proxy support, host filtering on some backends | process backends: none or the host's, nothing between. MicroVM sandboxes have an egress gateway with allow and deny lists |
| UI policy | clipboard, display and GUI controls | **none** |
| One-shot run | yes | yes |
| Stateful lifecycle (provision, start, exec, stop, deprovision) | yes, in the SDK | through the node daemon's API, not in-process |
| Access-denied diagnostics | yes, and an audit mode that records accesses to help write a policy | a refusal names the control and why; **no audit mode** |
| Streaming output, cancellation, deadline | streaming stdio sample | yes, all three |

## Where HyperMachine is behind

In rough order of how much they matter for the same use:

1. **Path grants on Windows.** A no-network workload there runs in an
   AppContainer, which cannot read the user's files, and there is no way yet to
   grant it a directory. Until there is, such a workload has to live where
   every packaged application may read.
2. **macOS.** Resource limits are not containment. mxc uses seatbelt.
3. **A request format and other languages.** mxc's request is JSON with a
   versioned schema, and its SDKs cover .NET and Node.
4. **Finer network policy for a process.** All or nothing today.
5. **UI controls, and an audit mode.**

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
| Windows no-network | `process::windows::tests`, on a Windows 11 host: a request that reaches a loopback listener with the host's network does not reach it from the container, and the listener sees no connection. `hm sandbox run` was also run by hand: `curl` to the Internet fails by default and succeeds with `--net host` |
| macOS | nothing beyond what the backend reports about itself |
