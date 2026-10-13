# Coding agents installed at a machine's first boot

**What this checks:** `agents: ["claude", "codex"]` on a machine created
from a distribution's image with `guestAgent`. Before this a prompt could
only reach an agent the caller had installed. boat has its agents installed
already; see the [comparison](../../../BOAT_PARITY.md).

**No key was used.** Each agent was started by a prompt and stopped where it
needed one. No prompt was answered by a model.

## Method

`tools/check-machine-agents-kvm.py` runs an owned `hv2-sandboxd` with
`--firmware`, `--image-dir` and `--guest-agent`. It creates one machine from
Ubuntu 24.04's cloud image as published, with two processors, 2 GiB, an
8 GiB disk and the Internet allowed. The install fetches from Ubuntu's
archive and from npm.

1. **Created:** the record names the agents, each once, though `claude` was
   asked for twice. `exec` answers while the install is still going.
2. **Installed:** `/var/lib/hv2/agents-ready` is written. As `ubuntu`, not
   root, `claude` and `codex` are on the path, and each gives its version.
   npm ran once.
3. **Claude Code:** a prompt with provider `claude`, `unattended`, as
   `ubuntu`. Expected: an `init` event with the session that the prompt
   reports, then a `result`, and a non-zero exit.
4. **Codex:** a prompt with provider `codex`. Expected: `thread.started`,
   then errors, and a non-zero exit.
5. **Refusals:** an agent that is not offered, agents without the guest
   agent, and agents for a machine from a template. Each is refused with
   400, and no machine is made.
6. **Started again:** after a stop and start, the ready marker and the
   install's log are unchanged, and `claude` is still there.

## Result

All six cases passed on 2026-10-12, on Linux 6.18.33 under WSL2 with nested
KVM, on a 24-thread host. From [`report.json`](report.json):

| | |
|---|---|
| `exec` first answered | 24 s after create; the install had not finished |
| Install finished | 215 s after that |
| Node, npm | v18.19.1, 9.2.0, from Ubuntu's archive (362 packages) |
| Claude Code | 2.1.296, `/usr/local/bin/claude` |
| Codex | codex-cli 0.162.1, `/usr/local/bin/codex` |
| Claude Code's prompt | `init` with a session and 22 tools, then `result` "Not logged in · Please run /login"; exit 1 |
| Codex's prompt | `thread.started`, `turn.started`, then 401 Unauthorized from `api.openai.com` until `turn.failed`; exit 1 |
| After a restart | `exec` in 18 s, nothing installed again |

An earlier run of the same check also passed all six cases. That run
installed Node with Ubuntu's recommended packages: 606 of them, against 362
without. The install now leaves recommended packages out. Its total time
went from 134 s to 215 s anyway, because npm took 15 s in the first run and
two minutes in the second. **The time is the network's, not the
machine's.**

## What this does not show

- **A model answering.** That needs a key this project does not hold.
- **Any other distribution.** The `dnf` branch, for Fedora and its kin, has
  not been run.
- **An image with its own Node.** One that already has `npm` skips the
  distribution's packages. Not run.
- **Pinned versions.** npm gives whatever is newest at the time, so two
  machines created a day apart can differ.
- **A machine without the Internet**, where the install fails. That failure
  is logged and leaves the machine reachable, but only the unit test checks
  that.
