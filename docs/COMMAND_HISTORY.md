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
