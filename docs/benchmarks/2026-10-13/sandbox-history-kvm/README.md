# A sandbox's commands kept and read again

**What this checks:** `GET /sandboxes/{id}/commands` and `.../commands/{n}`.
Before this a sandbox's command output was streamed once and kept nowhere.
boat stores, lists and replays past commands (`boat history`); see the
[comparison](../../../BOAT_PARITY.md) and
[how it works](../../../COMMAND_HISTORY.md).

## Method

`tools/check-sandbox-history-kvm.py` runs two owned `hv2-sandboxd` nodes
(`a` and `b`) sharing a `--snapshot-store`, with a busybox guest carrying
the guest agent. One sandbox is created on `a` through the E2B Python SDK
(2.51.0), and driven through both the node's API and the SDK. E2B's
`commands.run` starts `/bin/bash -l -c CMD`; this guest has no bash, so the
check links `/bin/bash` to its `sh` first, through `exec`.

1. **`exec`:** `echo hi; echo err >&2; exit 4` is kept as command 1,
   `exec`, exited 4, with `hi` and `err` in its two streams.
2. **The SDK:** `commands.run("echo from-sdk; sleep 0.2; echo done")` is
   kept as `process`, with its PID, the command line envd was given, and
   its output. A `sleep 300` started in the background and killed through
   the SDK is kept as signalled by 9.
3. **More than is kept:** a command printing 200 KiB has every byte
   counted, and the newest 64 KiB kept.
4. **Same node:** paused on `a`, the commands read the same while paused,
   and again after it resumes there.
5. **Other node:** paused on `a` into the store, whose record of it holds
   the five commands, and resumed on `b`. On `b` the five commands read as
   they did on `a`. A command run there is numbered 6.
6. **Secrets and opt-out:**
   - `TOKEN=abc123 true --password hunter2 https://u:pw@h.example/x` is
     kept as `TOKEN=[redacted] true --password [redacted]
     https://[redacted]@h.example/x`, and no secret is anywhere in the
     record;
   - a sandbox created with `"commandHistory": false` runs a command, and
     reading its history is refused (409).
7. **Refusals:** no such command and no such sandbox (404); once it is
   deleted on `b`, its commands are gone (404).

## Result

All eight cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM, on the first run. From [`report.json`](report.json):

| | |
|---|---|
| The SDK's command | n 3, `process`, `/bin/bash -l -c echo from-sdk; sleep 0.2; echo done`, PID 66, exited |
| Killed through the SDK | n 4, signalled, signal 9 |
| 200 KiB printed | 204,800 bytes counted, 139,264 dropped |
| Carried to node `b` | 5 commands; the next was numbered 6 |

## What this does not show

- **A node restarting** while its sandbox runs. The history is in memory,
  and goes with the node.
- **A command running across a pause.** By design it is kept as ended at
  the pause; that was not exercised here.
- **The CLI** (`hm sandbox vm history`) against a real node.
- **A full distribution** with a real bash.
