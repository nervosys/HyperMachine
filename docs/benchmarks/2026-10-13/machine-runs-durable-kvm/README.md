# A machine's runs kept across a node restart

**What this checks:** that a machine's [runs](../../../MACHINES.md#runs),
and their output, survive the node being killed and started again. Before
this they were kept in the node's memory only. boat keeps and replays past
commands (`boat history`); see the [comparison](../../../BOAT_PARITY.md).

## Method

`tools/check-machine-runs-durable-kvm.py` runs an owned `hv2-sandboxd` with
a `--machine-dir` and one 512 MiB machine. It starts three runs:

- **Ended:** prints two lines to standard output and one to standard error,
  and exits 3.
- **Big:** prints 9 MiB of `x`, 256 KiB at a time with a 0.2 s pause, then
  `big-done`. That is more than twice the 4 MiB kept, so its file is cut
  back.
- **Going:** prints `tick-N` every 0.2 s and never stops.

With the third still printing, the node is killed with SIGKILL, so nothing
is written on the way out. Another node is started on the same
`--machine-dir`. Then:

1. **Ended:** the ended run's state, exit code, command, PID, times and byte
   counts are what they were before, and both streams match byte for byte.
2. **Going:** the run that was going is `lost`, "the node stopped while it
   ran". Its output is every tick, from 1 on, in order, to the node's last
   poll.
3. **Big:** the big run's byte count and the text at its end are the same.
   `dropped` is everything but the newest 4 MiB, a read from 0 starts where
   that kept part does, and its file on disk is at most 8 MiB.
4. **Next run:** a new run takes the next ID (4), prints, and the four are
   listed in order.
5. **Deleted:** the machine, once stopped and deleted, takes its `runs/`
   directory with it, and the run is gone from the API.

## Result

All five cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM. From [`report.json`](report.json):

- The big run: 9,437,194 bytes written, 5,242,890 dropped, and a
  4,980,746-byte file on disk after it was last cut back.
- The going run kept 4 ticks.

## Found on the way: the guest agent drops output uncounted

The first try printed the 9 MiB at full speed. The run exited 0 with only
1,220,618 bytes recorded and `dropped: 0`. That is not this change. The
guest agent holds at most 1 MiB of a started program's output between the
node's polls (every 150 to 750 ms), drops the oldest beyond that, and does
not say so. A program printing faster than the polls take its output loses
some, and the run's byte count and `dropped` are wrong.

Fixing it needs the agent to report what it dropped, and rebuilt guest
images. It is not fixed here; the check paces its output instead.

## What this does not show

- **A power cut.** Output is appended without being synced; a killed node
  loses nothing written, a lost power supply may.
- **A run's machine started again by the new node.** The machine here was
  restarted with the node, and its new runs worked; a run does not outlive
  its machine's guest in any case.
- **Prompts and conversations** started through runs. Their runs are kept
  the same way; their own records are not, and were not checked here.
