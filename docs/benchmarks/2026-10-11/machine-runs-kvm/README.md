# Runs: a command left running in a machine, its output kept

**What this checks:** [runs](../../../MACHINES.md#runs), a way to start a
command in a machine and not wait for it: its output is kept on the node and
read from any point, it can be written to and signalled, and what it printed
is still there after it ends. `exec` waits for a command and answers once.
boat has `exec --detach`, `--attach` and `--kill`, and `boat history`; see
the [comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-runs-kvm.py` runs an owned `hv2-sandboxd` with one
machine from the base template, one vCPU and 512 MiB, and drives
`/machines/{name}/runs`.

1. **Follows.** A command that prints a line a second for four seconds, then
   a line to standard error, then exits 3. The start must answer while it is
   still going. Its output is read by asking again from the offset the last
   answer gave, and must arrive in more than one piece, add up to the four
   lines, and end with exit code 3 and the standard-error line.
2. **Is told.** A command that reads two lines. One is written to it, its
   answer is read while it is still running, the second is written with
   standard input closed, and it ends. Also a command given its input whole
   when it is started.
3. **Is signalled.** `sleep 300`, sent `TERM`: it must end with signal 15 and
   no exit code.
4. **Two at once**, each with only its own lines, the second started before
   the first ended.
5. **Kept.** The first run's output read again from the start, and from part
   way; the machine's runs listed in the order they were started, with no
   output in the listing.
6. **Environment.** A value given in a run's `env` must reach its command and
   be in nothing the API returns about the run except the command's own
   output.
7. **Lost.** A run still going when the machine is stopped must end as
   `lost`, say that its machine stopped, and keep what it had printed.
   Earlier runs are still readable; a run cannot be started in the stopped
   machine; one started after it is started again works.
8. **Refusals.** An empty command, a run that is not there, input or a signal
   to one that has ended, a signal that is not one, a machine that is not
   there.

## Result

All eight passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-11. From [`report.json`](report.json):

- The four-second command's start was answered in under a tenth of a second,
  and its output arrived in four pieces over fifteen reads.
- The signalled run ended with signal 15.
- The run lost with its machine gives its reason as `its machine stopped`.

The first run of the check failed in case 6, in the check: it looked for the
variable's *name* as well as its value, and the name is in the command line
the caller wrote, which a run's record keeps. The value was nowhere it
should not have been. The check now asks about the value.

## What this does not show

- **A node restart.** Runs are kept in the node's memory and are gone when it
  restarts; the programs go on in the guest, and nothing reads them.
- **More output than is kept.** A stream keeps its newest 4 MiB and counts
  what it dropped; that is unit-tested and was not driven through a guest.
- **A machine from a disk image.** The check uses a template machine. The
  same routes serve a machine whose agent was installed by cloud-init, and
  were not run against one.
- **The control plane.** It forwards these routes to the machine's node; the
  check talks to the node.
- **A terminal.** A run has pipes, not a terminal, so a program that wants
  one behaves as it does under a pipe.
