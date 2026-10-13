# A guest's processes listed from outside

**What this checks:** `GET /sandboxes/{id}/processes` and
`GET /machines/{name}/processes`. Before this, only processes started
through the API and still running could be listed. boat lists a VM's
processes (`boat ps`); see the [comparison](../../../BOAT_PARITY.md) and
[how it works](../../../PROCESSES.md).

## Method

`tools/check-guest-processes-kvm.py` runs an owned `hv2-sandboxd` with a
template of one vCPU and 512 MiB and a `--machine-dir`. Its guest initrd is
busybox with the guest agent. One sandbox and one machine are used.

In the sandbox, a shell started three things with `setsid`, none through
the API's runs:

- a loop spinning on the CPU;
- `sleep 4321`;
- a shell that started `sleep 0.1` and then exec'd `sleep 3333` without
  ever reaping it, which leaves a zombie.

1. **Sandbox:** the spinner is `R` with more than half a second of CPU. The
   sleeper is `S`, with its command line, user 0 and its age. The zombie is
   `Z`, with an empty command line, under the `sleep 3333`. Init is PID 1.
2. **Kernel threads:** with `?all=true` the kernel threads are added, and
   only they. Neither listing has the shell that reads `/proc` in it.
3. **Killed:** after `kill -9` of the spinner, it no longer runs.
4. **Machine:** `sleep 5555` started by a shell is listed with its memory
   and one thread.
5. **Refusals:** a paused sandbox (409), a sandbox that is not there (404),
   a stopped machine (409).

## Result

All five cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM. From [`report.json`](report.json):

| | |
|---|---|
| Sandbox listing | 6 processes, 0.728 s (one vCPU, shared with the spinner) |
| With kernel threads | 44 |
| Machine listing | 3 processes, 0.109 s |
| The spinner | `R`, 1.81 CPU seconds over 1.54 s |
| The killed spinner | `Z`: this guest's init, a shell script, had not reaped it yet |

The first run of the check expected the killed process to be gone, and
found it a zombie instead. That is what `ps` would show too: an orphan's
parent is init, and this guest's init reaps only when it next waits. The
check now accepts either.

## What this does not show

- **A full distribution.** The guest here is busybox. The listing uses only
  `/proc` and a POSIX shell, so an Ubuntu guest should list the same way;
  that was not run.
- **The CLI and the SDKs** against a real node. Their own tests ran against
  a stand-in.
- **A cluster**, where the request is forwarded to the node.
- **Many processes.** At most 4,096 are returned; six were.
