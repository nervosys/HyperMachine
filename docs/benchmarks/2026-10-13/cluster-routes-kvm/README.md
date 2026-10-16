# Today's sandbox routes through the control plane

**What this checks:** that the sandbox routes added on 2026-10-13 work
through the control plane, not only against a node directly. Each one's own
check called a node and listed the control plane as not run. They are:

- resize and fork at a size;
- the process list;
- command history;
- checkpoint files;
- the reverse port.

## Method

`tools/check-cluster-routes-kvm.py` runs an owned Redis, one `hv2-sandboxd`
node and the `hv2-control-plane`. The control plane's API is served over
verified TLS, and node traffic uses mTLS, from a private CA the check
makes. API keys are scoped to two teams, red and blue. The guest image
carries the newest guest agent, which has `Listen`. Every request below goes
to the control plane's API, as red:

1. **Sizes:** a sandbox is resized to 2 vCPUs and 512 MiB, and its guest
   counts 2 processors. It is forked at 1 vCPU and 256 MiB, and the fork
   counts 1.
2. **Processes:** its guest's processes are listed, and PID 1 is among them.
3. **History:** a command run through `exec` is listed, and read again with
   its output.
4. **Checkpoint files:** a second sandbox, at its template's size, writes a
   file, is checkpointed, and changes the file. The checkpoint lists the
   directory and reads the file as it was taken.
5. **Reverse port:** an `hv2-reverse/1` upgrade is opened through the
   control plane over TLS. A program in the second sandbox's guest connects
   to its port 7000, and bytes go both ways.
6. **Isolation:** blue tries each of those on red's sandboxes, and every one
   is refused.

## Result

All six cases passed on 2026-10-13, on the first run, on Linux 6.18.33 under
WSL2 with nested KVM, with the release builds of `master` at `2f50531c`. From
[`report.json`](report.json): blue was refused with 403 on resize, fork,
processes, commands, checkpoint file and the reverse upgrade.

## What this does not show

- **A machine's reverse port.** The control plane does not forward it; that
  is a known gap, not run here.
- **Command history's redaction** (#240), which was not on `master` when
  this ran.
- **Several nodes.** One node behind the control plane.
- **A paused sandbox resumed on another node**, which the history check ran
  with two nodes sharing a store, not through a control plane.
