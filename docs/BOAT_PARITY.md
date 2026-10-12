# HyperMachine against boat

[boat](https://boat.dev/) is a hosted sandbox service from ASCII: "the
cheapest, most powerful sandbox for agents". A sandbox there is a persistent
Ubuntu virtual machine with SSH, Docker, a public address, a desktop in a
browser, and coding agents already installed and driven by `boat prompt`.

HyperMachine's part that plays the same game is the microVM sandbox and the
machine, served by `hv2-sandboxd` and `hv2-control-plane` and driven by `hm
sandbox vm`. This page tracks one against the other. It sits beside
[boxd and exe.dev](PLATFORM_PARITY.md), which are the same kind of product,
and [mxc](MXC_PARITY.md), which is not.

boat's column is from its site and documentation (`boat.dev`,
`docs.boat.dev`), read on 2026-10-10. It is a reading of what the service
says, not a test of it: boat has not been run, and nothing here is a measured
win. HyperMachine's column is from its code, checked on the same day at
`14aab05c`, and not from its own documentation; where the two disagreed the
code is what is written down.

boat is a service somebody else runs, and HyperMachine is software an operator
runs. Rows about billing, regions and a dashboard are boat's alone for that
reason, and are listed once at the end rather than counted as gaps.

## The matrix

| | boat | HyperMachine |
|---|---|---|
| What a sandbox is | An Ubuntu 24.04 VM, x86-64, with root | A Linux microVM on its own VMM, from a template whose root filesystem is in memory; or a [machine](MACHINES.md) with a disk |
| Sizes | Four, chosen at creation: 2 to 16 vCPUs, 4 to 32 GB, 12 to 251 GB of disk | **A sandbox has no size of its own.** Size is fixed when its template is built (up to 32 vCPUs and 64 GiB), or node-wide. A machine takes `cpuCount` (to 32), `memoryMB` and `diskGiB`, whether from a template or [from a disk image](FIRMWARE_BOOT.md#acpi-tables) |
| Changing size later | On resume and on fork | **A machine, while stopped**: [processors, memory and a larger disk](MACHINES.md#resizing), there at its next start. Not a sandbox, and not on a fork |
| Creating one | "usually under a second on a ready machine"; `--fail-fast` answers within about 1.5 s whether one was ready | From a template: 18.9 ms at the median on this project's own host, and 0.65 ms from the [warm pool](WARM_POOL.md). A full node answers 503 at once by default; there is no request option that says so when the node queues |
| What stopping keeps | **Files only.** "Snapshots capture your sandbox's files, not its running processes or memory"; processes run by hand are restarted by the user | **Memory and processes.** Pause to disk and resume on any node; [standby](STANDBY.md) keeps memory and wakes on the next request |
| Fork | A new sandbox from the latest disk snapshot; the source's processes are not in it | A running sandbox with its memory, 1 to 100 copies, a block disk included |
| Automatic snapshots | Every minute while running, and on stop; incremental; free; files only | **Yes, with memory**: `autoCheckpoint` seconds on create (`hm sandbox vm create --auto-checkpoint`), or `--auto-checkpoint-every` for a whole node, keeps one checkpoint named `auto`, replaced on the interval and rolled back to like any other. At least 60 seconds apart; skipped when nothing has happened in the sandbox since the last. Not for a sandbox holding a disk, and kept on the node that took it |
| Named snapshots, and new sandboxes from them | Yes; ten free | Yes: a snapshot becomes a template |
| Reading a snapshot from outside | A file tree, and download of all or part of it, with the sandbox stopped | **No.** A snapshot is memory pages; there is no tree to list without restoring it |
| Sharing a snapshot with another account | Share codes; secrets and logins removed first | **No.** A snapshot belongs to its team, or to everyone when an operator made it |
| systemd, and Docker, in the guest | Both | **Both, in a machine from a disk image**: [Ubuntu 24.04's image as published](benchmarks/2026-10-11/machine-docker-kvm/README.md) boots on its own 6.8 kernel with systemd started in full, installs Docker with `apt`, and runs containers. That run was typed at its serial console; such a machine can also [take `exec`](benchmarks/2026-10-11/machine-agent-kvm/README.md). **Neither in a sandbox**, whose first process is the project's own and whose kernel is built without what Docker needs |
| `/dev/kvm` in the guest | Yes on its standard hosts, and said to be absent on its fallback hosts | **Not offered.** Not masked either, and never tested |
| Installed for you | Docker, Chrome, VS Code, some twenty languages and runtimes, a dozen agent programs | What the template's image holds. No image is shipped with more than the guest agent needs |
| Commands and files from outside | `exec`, with detach, attach and kill; `scp`; SSH | A sandbox: `exec`, processes with a terminal and standard input, files in and out; SSH by name is partial. A machine: `exec`, and [runs](MACHINES.md#runs), started and left, read from any offset, written to and signalled; and [files](MACHINES.md#files) in and out by path, up to 64 MiB each. No directories or trees in one request, and no SSH |
| Output of past commands | Stored, listed and replayed (`boat history`) | **For a machine's [runs](MACHINES.md#runs)**: listed, and each one's output kept and read again, in the node's memory, so not across a node restart. Not for `exec`, and not for a sandbox, whose output is streamed once |
| The guest's processes, listed from outside | `boat ps` | Only the processes started through the API and still running |
| An HTTPS address for a port | `https://NAME-PORT.on.boat.dev`, behind a token unless made public; fifty ports | `{port}-{id}.{domain}` over TLS, with [custom domains](CUSTOM_DOMAINS.md) and [private URLs with a login](PRIVATE_GUEST_URLS.md), for a sandbox. The operator supplies DNS and certificates. A machine has [published ports](MACHINES.md#published-ports) instead: a TCP port of its node, with no name, TLS or login of the node's |
| A port from the caller's machine into the sandbox, and back | `boat forward`, and `--reverse` | Into the sandbox, TCP and UDP. **Not back**: nothing makes a port of the caller's reachable from the guest |
| A public address of its own | IPv4 or IPv6 per sandbox | Managed public TCP and UDP ports per VM, partial; no address per VM |
| A desktop in a browser | 1920x1080 at 60 frames a second over WebRTC, with sound and a clipboard; VNC over HTTPS as a fallback; a stream of the browser alone; recording to a file | **Absent.** No shipped binary gives a guest a display. About ten thousand lines of display and input devices sit in `hv2-core` attached to nothing |
| Computer use for agents | Screenshots, clicks, typing and browser actions as a tool server every installed agent is given | **Absent**, for want of a display |
| Coding agents run for you | Seven of them. `boat prompt`, conversations, a stream of their events, `steer` into a running turn, `interrupt`, a move from one agent to another with the transcript | **Two of them, in a machine, and not yet with a model behind them.** [`prompts`](MACHINES.md#prompts) runs Claude Code or Codex, or any agent by its own command, on a prompt; gives its events from any offset; interrupts it; and carries Claude Code's session to the next prompt. The agent has to be in the machine already, with the caller's key. No `steer`, no conversations, no move between agents; nothing for a sandbox. Checked with a stand-in for the agent, and each real agent took its command line with no key; none has answered a prompt here |
| A named set of variables, files and repositories for new sandboxes | Environments | **No.** Variables are per sandbox. Setup steps run when a template is built, not when a sandbox is created |
| Secrets | The account's, given to every sandbox unless one is made without them | Per-sandbox variables, and [secrets held off the VM and put in at the edge](EGRESS_SECRETS.md), partial, which boat does not describe |
| What a sandbox may reach on the network | Outbound Internet. No policy is described; the user may run a firewall inside | Allow and deny lists per VM, changed while it runs, with a log of decisions, and reserved ranges refused |
| Networks between sandboxes | Not described | By tag, partial: [same node and across two nodes](PRIVATE_VM_NETWORKS.md) |
| Scoped, expiring API keys | Yes, with presets, rotation and usage per key | Yes on the control plane, with [reload and replacement](API_KEY_ROTATION.md); no usage per key |
| Webhooks | Signed, with a rotated secret; a `degraded` event | Signed and team-scoped, for sandboxes. The signature is a hash of the secret and the body, not an HMAC; the secret is replaced in one step with no overlap; machines send none |
| Telemetry sent where the user says | OTLP over HTTP, set per account | **No.** An exporter exists in `hv2-core` behind a feature no binary turns on, and `config.example.toml` describes settings nothing reads. Prometheus text at `/metrics` is real |
| Teams | Organizations, members, a shared wallet | [Teams](TEAMS.md), partial: keys and sandboxes belong to one, with roles |
| How much was used | Per sandbox and per key | **No.** Lifecycle events with times are kept; nothing adds them up |
| Client libraries | Python and TypeScript | E2B's own, unmodified, against E2B's API. None of HyperMachine's for the VM API; `sdk/node` is for the [process sandbox](SANDBOXES.md) |
| A command line | `boat`, with JSON output on every command | `hm sandbox vm` |
| GPU | None | Partial: passthrough code, not wired to sandboxes |
| Who runs it | ASCII, in Germany, Finland and France | Whoever installs it: one node or [several](DEPLOYMENT_GUIDE.md), with the source |

## Where boat is ahead

In rough order of how much each matters to someone choosing between them:

1. **A guest that is a whole distribution.** boat's sandbox runs systemd,
   Docker and anything `apt` installs, on several processors. HyperMachine's
   sandbox is a small guest in memory. Its machine from a disk image now
   boots Ubuntu 24.04 with systemd on the processors it asks for, and turns
   itself off. It is [given a user at creation](FIRMWARE_BOOT.md#a-user-at-creation-the-cloud-init-seed)
   through cloud-init, on the distribution's image as published. With a NIC it
   [installs Docker and runs containers](benchmarks/2026-10-11/machine-docker-kvm/README.md).
   With `guestAgent` it [runs this project's agent](benchmarks/2026-10-11/machine-agent-kvm/README.md),
   installed by cloud-init, and takes `exec`, runs and
   [files](MACHINES.md#files). What it still lacks: an HTTPS name for a
   port, where it has [a TCP port of its node](MACHINES.md#published-ports);
   and an image with tools already in it. A sandbox, as distinct from a machine, is
   unchanged. Most of the rows below stand on this one: an agent, a browser
   and a desktop are all things a distribution installs.
2. **Coding agents that are already there, and proven.** A machine now
   [runs one on a prompt](MACHINES.md#prompts) and reports what it did, but
   the caller installs it and brings the key, where boat has seven installed;
   there is no steering, no conversation and no move between agents; and no
   prompt has been answered by a model here, which needs a key this project
   does not hold.
3. **A desktop, and computer use.** Absent, and the device code that would
   back it is not attached to anything.
4. **A size per sandbox, and changing it.** A caller here cannot ask for four
   processors and eight gigabytes on one sandbox create, nor change a
   sandbox's size. A machine takes a size when created and
   [another while stopped](MACHINES.md#resizing).
5. **Reading a snapshot from outside.** Automatic checkpoints exist now, and
   keep memory where boat's keep files; but there is one, on one node, and
   nothing lists or downloads what is in it.
6. **Stored command output that survives the node**, and for sandboxes at
   all; the guest's process list; and a port back to the caller's machine.
7. **Environments**: variables, files and repositories applied to every new
   sandbox.
8. **Usage figures and telemetry export.**
9. **Client libraries of its own** in Python and TypeScript.
10. **An image with the tools already in it.**

## Where HyperMachine has something boat's documentation does not claim

- **A stopped sandbox keeps its memory.** boat says in as many words that
  processes and memory are not kept. Here a paused or forked sandbox carries
  on where it was, and a sandbox in standby answers the next request.
- **A warm pool.** Creates in under a millisecond at the median, on this
  project's own host, from sandboxes kept ready.
- **Egress policy**, with a decision log, and secrets that never enter the
  VM.
- **Workload identity** for a sandbox: signed identity documents a cloud
  provider can be told to trust.
- **Volumes shared between sandboxes**, and private networks between them.
- **It can be run by its user**, on one machine or a fleet, and read.
- **A sandbox that is not a VM at all**: [`hv2-sandbox`](SANDBOXES.md)
  confines a process on Linux, Windows and macOS.

None of these is a measured win over boat: boat was not run.

## Figures

boat publishes one benchmark and two statements of time. HyperMachine has
figures of its own from its own host. They do not measure the same thing and
are set side by side only so that nobody has to go looking.

| | boat says | HyperMachine measured, on its own host |
|---|---|---|
| A Node.js build-and-test loop, 1,000 runs, 4 vCPUs and 8 GB | 36 s (27.7 runs a second) on its standard hosts, an AMD Ryzen 9 9950X; 113 s on its fallback hosts. Medians over several machines, from [a public benchmark](https://github.com/AnicetNgrt/hpc-sandbox-benchmarks) | **Not run.** There is no guest image here with Node.js in it, and no sandbox can be created with that size by asking |
| Creating a sandbox | "usually under a second on a ready machine" | [0.65 ms at the median from the warm pool, 18.9 ms without](benchmarks/2026-10-08/warm-pool-kvm/README.md), for the base template, which is a far smaller guest than boat's |
| Coming back from a stop | "usable within a few seconds", files only | [A command to a sandbox in standby: 3.30 ms at the median](benchmarks/2026-10-08/standby-kvm/README.md), memory kept |

boat's loop is mostly a measure of the processor under the VM and the disk
beside it. Running it here would say more about the host it was run on than
about HyperMachine, unless it were run on comparable hardware; that is the
only honest way to put a number in the empty cell.

## What is boat's alone because it is a service

Billing by the second, plans and credit packs, start-rate limits, regions, a
web dashboard, sign-in with Google, a status page, and capacity that is
somebody else's problem. An operator of HyperMachine supplies those or does
without.

## Found while checking

Reading the code for HyperMachine's column turned up things that were wrong
independently of boat:

- `POST /machines` accepted up to 64 processors where the hypervisor refuses
  more than 32, so such a machine was created and then could not boot; and a
  machine from a disk image was given one processor whatever it asked for,
  without being told. Both are now refused when the machine is created.
- `config.example.toml` describes telemetry export settings that no shipped
  binary reads.
- The header of `hv2-sandboxd`'s disk module says a sandbox with a disk
  cannot be paused, forked or restored from a template. It can.
- [The boxd matrix](PLATFORM_PARITY.md) says API keys are for a "single
  team" one row below saying that keys belong to teams.
