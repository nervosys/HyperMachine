# Automatic checkpoints on real KVM: a sandbox saved on an interval, unasked

**What this checks:** a sandbox checkpointed by its node on an interval, with
its memory, and rolled back to that checkpoint. boat documents "automatic
snapshots" every minute that keep files and not running processes; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-auto-checkpoint-kvm.py` runs an owned `hv2-sandboxd` with a
template, and two sandboxes with one vCPU and 512 MiB each: one created with
`"autoCheckpoint": 60`, the least the node accepts, and one without. A process
in the first increments a counter in a file ten times a second.

1. **Unasked, an interval after it starts.** The sandbox has no checkpoint at
   first. One named `auto` appears without a request for it, and not before
   the interval has passed.
2. **Roll back.** A file is changed after the checkpoint, and the sandbox is
   restored to `auto`. The file must read as it did, and the counter must be
   past zero and behind where it had got to: the process that was running is
   running again from where the checkpoint caught it. Two seconds later it
   must have moved on.
3. **Replaced.** The roll back and the commands after it are use, so a newer
   `auto` must appear within the next interval, and there must still be
   exactly one checkpoint.
4. **Left alone.** The counter is stopped, which is itself use and is followed
   by one more checkpoint. After that the sandbox is left for two intervals
   and ten seconds with nothing running in it, and `auto` must not change.
5. **Not without asking.** The sandbox created without an interval must have
   no checkpoint at the end of the run.
6. **Refusals.** `"autoCheckpoint": 5` on create is a 400, and so is a caller
   saving a checkpoint under the name `auto`. A checkpoint under another name
   is still saved beside the automatic one.

## Result

All six passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-10.
[`report.json`](report.json) has the detail:

- The first checkpoint came 60.2 seconds after the sandbox started.
- The counter read 623 before the roll back, 592 straight after it, and 611
  two seconds later.
- The sandbox without an interval had no checkpoint after 310 seconds.
- The node's log records three automatic checkpoints for the run, which is
  the first, the one after the roll back, and the one after the counter was
  stopped.

## What this does not show

- How long a checkpoint stops a guest for. The guest here is small and nearly
  idle; a large or busy one takes longer, and that is why the interval has a
  floor.
- A node-wide default (`--auto-checkpoint-every`): the check sets the interval
  per sandbox only. The two share everything after the request is read.
- A sandbox holding a disk, which cannot be checkpointed, or one moved to
  another node, which leaves its checkpoints behind.
