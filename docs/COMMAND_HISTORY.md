# A sandbox's command history

Every command run in a sandbox through the API is kept with what it
printed, to be listed and read again after it ends:

```sh
curl -s localhost:3980/sandboxes/$SBX/commands          # oldest first, no output
curl -s "localhost:3980/sandboxes/$SBX/commands/3?stdout=0&stderr=0"
# → {"n":3,"source":"process","cmd":"/bin/bash -l -c make test","pid":66,
#    "state":"exited","exitCode":0,"startedAt":…,"endedAt":…,
#    "stdoutBytes":204800,"stderrBytes":0,
#    "stdout":{"offset":139264,"text":"…","next":204800,"dropped":139264},…}

hm sandbox vm history $SBX [N]
```

- **What is kept:** each command run through `POST /sandboxes/{id}/exec`
  (`source: exec`) or started through envd's `process.Process/Start`
  (`source: process`), which is how E2B's SDK runs `commands.run`. For each:
  - its command line with its arguments, working directory, user, tag and
    PID;
  - when it started and ended;
  - how it ended: `exited` with its code, `signalled` with the signal, or
    `lost` with why;
  - the newest 64 KiB of each stream, with `dropped` counting what went
    before.

  A stream is read from an offset as a [machine run's](MACHINES.md#runs) is.
- **Not kept:** the environment a command was given, which is where a
  caller puts a key; and commands a guest's own shell started, which are
  [listed](PROCESSES.md) while they run.

## Secrets

A command line can carry a secret, and the history is returned to anyone
who can reach the sandbox. So before a line is kept, the common shapes of
one are masked as `[redacted]`:

| Shape | Kept as |
|---|---|
| `NAME=VALUE`, for a name holding `TOKEN`, `SECRET`, `PASSWORD`, `PASSWD`, `KEY`, `CREDENTIAL`, `AUTH` or `COOKIE`, any case | `NAME=[redacted]` |
| `--password`, `--token`, `--secret`, `--api-key` and their kind, then a value (or `=VALUE`) | `--password [redacted]` |
| `Bearer` or `Basic`, then a value | `Bearer [redacted]` |
| A URL with credentials, `https://user:pw@host/...` | `https://[redacted]@host/...` |

- **A quoted value is masked whole**, spaces and all; the line's spacing and
  any `;` or `&&` after a value are kept.
- **It over-masks rather than under-masks**: `MONKEY=1` is masked too.
- **It cannot know every secret**, and **output is not scanned**: a command
  that prints a secret keeps it in its output. For either, keep no history:
  - `"commandHistory": false` on a create keeps none for that sandbox, nor
    its forks, and wherever it is resumed. Reading it is refused (409).
  - `--no-command-history` keeps none on the node at all.
- **The newest 64** are kept for a sandbox; the oldest finished one makes
  room.
- **Kept through a pause,** on this node or, with a `--snapshot-store`, on
  whichever node resumes it, where numbering goes on after them. It is also
  kept through a reboot, a resize and a restored checkpoint. It is readable
  while the sandbox is paused, and gone when the sandbox ends. A fork starts
  with none.
- **A command running when its sandbox pauses** is kept as ended there,
  with what it had printed: the node stops watching it then, though the
  guest's process goes on after the resume.
- Kept in the node's memory, and in the store's record of a paused
  sandbox, which is written owner-only. Not across a node's restart while
  the sandbox runs.

Evidence: [real KVM checks](benchmarks/2026-10-13/sandbox-history-kvm/README.md).
