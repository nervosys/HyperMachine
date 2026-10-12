# Prompts: a coding agent's command line run in a machine

**What this checks:** [prompts](../../../MACHINES.md#prompts), which run a
coding agent that is in a machine on a prompt and report its events. boat has
`boat prompt`, with seven agents installed; see the
[comparison](../../../BOAT_PARITY.md).

**What it does not:** a coding agent. No agent answered a prompt and no key
was used. The check installs a stand-in under the names `claude` and
`codex`: a shell script that records the arguments it is given and prints
lines of JSON. What is checked is everything between a caller and an agent.

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-prompts-kvm.py` runs an owned `hv2-sandboxd` with one
machine from the base template, one vCPU and 512 MiB, and drives
`/machines/{name}/prompts`.

1. **Not installed.** A prompt for `claude` before anything of that name is
   in the guest must end with exit code 127 and the shell's own words on the
   run's standard error.
2. **Command lines.** For `claude` and `codex`, plain and with every option,
   the stand-in must be started under that name with exactly the arguments
   expected, as the user asked for. The prompt holds double and single
   quotes, `$(...)`, backticks, `$HOME`, a backslash, `&&` and a newline; it
   must arrive as one argument, byte for byte, and none of the three files it
   would create if a shell read it may exist. A caller's own `HM_PROMPT` in
   `env` must not replace it.
3. **Events as they come.** An agent that prints a line that is not JSON,
   then five events over three seconds, and exits 7. Reading from `next`
   each time must give every event once, in order, over more than one read;
   the first line as `text`; the session the agent named; exit code 7. The
   session carries the agent's PID, which must be the run's: the agent is
   the run's own process and not a child of a shell. Its standard error must
   be in no event, and on the run.
4. **Half a line.** An agent that prints half a line of JSON, waits three
   seconds and prints the rest. No read in between may give the half, as
   text or as an event.
5. **Interrupt.** An agent that traps SIGINT, says so and exits 130. After
   `interrupt` it must end with that exit code and that last event, and not
   by the signal. A second interrupt is refused.
6. **The key.** A value in `env` must reach the agent, which reports its
   length, and be in nothing returned by the prompt, the listing of prompts,
   the run or the listing of runs, nor in the node's log at `debug`. No
   prompt's text may be in that log either.
7. **Listing.** Prompts in the order they were started, with no events and
   no command line. A run that is not a prompt is not listed, read or
   interrupted as one. A prompt's run shows a command line that names
   `$HM_PROMPT` and does not hold the prompt.
8. **Custom.** A caller's own command, run as it is, reads the prompt and
   the model from its environment and its exit code is kept.
9. **Refusals.** An empty prompt, one over 128 KiB, an unknown provider,
   `custom` with no command, a command for a provider that has one, `resume`
   for `codex`, `claude` unattended as root, an empty model, a prompt that is
   not there, an interrupt of one that has ended, a machine that is not
   there. None may start anything.
10. **Stopped.** Nothing is prompted in a stopped machine, and an earlier
    prompt's events are still read.

## Result

All ten passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-11.
From [`report.json`](report.json):

- The prompt of 140 bytes arrived whole as argument 2 of 5 for `claude` and 4
  of 4 for `codex`; with every option, 10 and 7 arguments.
- The agent's six lines arrived over four of twelve reads. Ten reads were made while
  half a line was written, and none gave it.
- The interrupted agent exited 130.
- The 28-byte key was seen by the agent and was not in 1.3 MB of node log.

## What the check found

**An interrupt did nothing.** On the first run, case 5 failed: `interrupt`
was accepted and the agent went on. The guest's init starts the guest agent
with `&`, and a shell without job control ignores SIGINT and SIGQUIT in what
it starts that way. What is ignored is inherited, and a shell cannot trap a
signal that was ignored when it started. So no program started through the
guest agent in a sandbox or a template machine could be interrupted unless it
set a handler itself, by `signal` or from a terminal. The guest agent now
gives each program it starts the default for SIGINT, SIGQUIT, SIGTERM and
SIGHUP. Its test starts the agent's own process ignoring SIGINT and fails
without the change.

The result above is with a guest image rebuilt with that agent. With the
agent as it was, the first four cases passed and the check stopped at the
fifth; the five after it were not run against that image.

## The real agents, without a key

Separately, on the host and not in a guest, each provider's command line was
given to the agent as published on npm, in an empty environment:

| | Claude Code 2.1.296 | Codex CLI 0.162.1 |
|---|---|---|
| Takes the line | Yes | Yes |
| First event | `{"type":"system","subtype":"init","session_id":...}` | `{"type":"thread.started","thread_id":...}` |
| Ends | Exit code 1, after a `result` event saying `Not logged in` | Exit code 1, after `401 Unauthorized` events |
| Without `--verbose` | Refuses: `stream-json` needs it | |
| Unattended as root | Refuses | Takes the line |
| The `$(...)` in the prompt | Not run | Not run |

So both lines parse, both agents print the JSON lines the node reads, and
the session each names is what a prompt reports as `session`. Claude Code's
line with every option was only ever given to it as root, where it stops at
the refusal; its `--resume` and `--model` are in its `--help` and were not
taken past that.

## What this does not show

- **A prompt answered.** That needs a key. Nothing here says what either
  agent does once it is talking to its model: whether its events stay one to
  a line, how long a turn runs, or what `resume` does with a session ID from
  an earlier prompt.
- **The real agents in a guest.** They were run on the host. The template
  guest is a small one without the libraries or the network to install them;
  a machine from a distribution's image is where one would be installed, and
  that was not done.
- **A machine from a disk image.** The check uses a template machine.
- **The control plane.** It forwards these routes to the machine's node; the
  check talks to the node.
- **`hm sandbox vm machine prompt` against a node.** It is tested against a
  fixture that answers as a node does.
- **A user's own files.** The stand-in runs as a user added to the guest's
  `/etc/passwd` by the check.
