# Conversations: steering, sessions carried on, and a move between agents

**What this checks:** a machine's [conversations](../../../MACHINES.md#conversations).
These are turns that carry an agent's session on, a message taken into a
turn that is running, a transcript, and a conversation started from
another one, with the other agent. boat has conversations, `steer` and a
move between agents; see the [comparison](../../../BOAT_PARITY.md).

**No key was used.** Every real turn ended with the agent saying it had no
credentials. What a model does with a message that steers it is not shown.

## Method

`tools/check-machine-conversations-kvm.py` runs an owned `hv2-sandboxd`, in
two parts.

**With stand-ins.** A machine from the template has shell scripts installed
as `claude` and `codex`, which do what each harness does.

- **`claude` stand-in:** reads messages as lines of JSON, says each one back,
  works for as long as it is told to, then answers. It goes on until its
  input is closed.
- **`codex` stand-in:** one turn, with the prompt as its last argument.

A turn can then be made slow enough to steer, every time, and the check can
see what reached the agent:

1. Making a conversation runs nothing. Eight bad requests are refused, each
   with why.
2. A message starts a turn. A second, sent a second later while the turn
   works, is taken into the same turn: reply 202, `steered: true`. The turn
   is one process and one session, and it answers twice: `user`,
   `assistant`, `result`, then the same again. It is told its input is
   closed only after the second answer.
3. A message after the turn ended starts a new one, with `--resume` and the
   session.
4. The transcript is the six lines in order, both sides, read from the
   agent's events.
5. An interrupt ends a running turn. A second interrupt is refused with 409.
6. Codex:
   - a message while its turn runs is refused with 409;
   - the next turn is `codex exec … -m a-model resume <thread> "codex two"`;
   - the thread is the same in both turns.
7. A Codex conversation `from` the Claude Code one gives Codex what was
   said, then the new message. A Claude Code conversation `from` the Codex
   one does the same the other way.
8. Conversations are listed, and found by number or name. One is refused
   deletion while a turn runs, and deleted once idle.
9. An agent that closes its input and exits before its first message is
   written. Whichever comes first, the caller is told what it said.

**With the real agents.** A machine is made from Ubuntu 24.04's image as
published, with both agents [installed at first boot](../machine-agents-kvm/README.md):

10. Claude Code 2.1.296 and Codex 0.162.1 are on the path.
11. Real Claude Code, run as `ubuntu`:
    - "Say hello.", then "And goodbye." at once, then "Once more." after
      the turn ended;
    - every message answered, all in one session;
    - the third turn resumes it;
    - the transcript names the three messages in order.
12. Real Codex: the second turn resumes the first's thread.
13. A Codex conversation `from` the Claude Code one. Codex's own session
    files in the guest hold the carried transcript.

## Result

All 13 cases passed on 2026-10-12, on Linux 6.18.33 under WSL2 with nested
KVM, on a 24-thread host. From [`report.json`](report.json):

| | |
|---|---|
| Stand-in, steered turn | 8 events in one process: `system`, then user/assistant/result twice, then the input closed after 2 messages; 6.4 s for both |
| Stand-in, next turn | `--resume standin-85`, with `--replay-user-messages` |
| Codex stand-in, next turn | `exec --json --skip-git-repo-check -m a-model resume standin-thread-270 "codex two"` |
| Carried | 312 bytes to Codex, 251 to Claude Code |
| Real Claude Code | the second message `steered: true`; 2 turns, 3 answers, 1 session |
| Real Codex | 2 turns on thread `01a1337e-…` |
| Real move | the carried text in 3 of Codex's session files |

Real Claude Code without a key answers within about a second, so whether
the second message lands in a turn that is still running is a race. In both
full runs that got that far, this one and one before the fix below, it did.

## A fault this found

The first full run failed at case 11. The first message could not be
written to the turn, which had already exited. The node's reply now
carries what the agent said, and the next failure said: `exec: claude: not
found`, exit 127.

A probe showed why. Two seconds after the first `claude --version` in a
machine, the npm package directory held only `LICENSE.md`. Claude Code's
updater, run for the first time, was reinstalling its own package, and for
that while there was no `claude`. The check's case 10 runs
`claude --version` just before case 11, so a turn could start inside that
window.

The install now turns the updater off: `DISABLE_AUTOUPDATER` in
`/etc/claude-code/managed-settings.json` and `/etc/profile.d`, and in every
turn of Claude Code's the node starts. The same probe, rerun:

- After a `claude --version` with nothing in its environment, so only the
  managed settings applied, the package stayed whole for 42 s.
- After one in a login shell, the same.
- A turn then started normally.

Before the fix, one first turn in twelve failed this way. That includes ten
in a separate probe with a new user each, which never ran `--version`
first. After the fix, none did.

In case 9 the message reached the pipe before the stand-in closed it, so
the turn took it and ended saying why. The other branch, a 502 with the
agent's words, was the one seen live above.

## What this does not show

- **A model taking a steering message.** How Claude Code fits a message into
  work already going is its own, and nothing here ran long enough to need
  it.
- **Codex being steered.** It is refused: `codex exec` takes no input while
  it runs.
- **A conversation surviving a node restart.** It is in memory, as runs are.
- **The control plane.** Requests went to the node.
