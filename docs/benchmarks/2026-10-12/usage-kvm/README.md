# Usage: what sandboxes ran, against a stopwatch

**What this checks:** [`GET /usage`](../../../USAGE.md), which adds up how
long sandboxes ran and with how much. boat reports usage per sandbox and per
key; see the [comparison](../../../BOAT_PARITY.md).

The durations here are this check's own stopwatch and the node's account of
them. They are not a measurement of anything's speed.

## Method

`tools/check-usage-kvm.py` runs an owned `hv2-sandboxd` with a template,
one vCPU and 256 MiB a sandbox, and times each request from either side.

1. **Paused time.** Sandbox A runs about 4 s, is paused about 4 s, is
   resumed and runs about 3 s, and is deleted. Its running time must lie
   between what the stopwatch allows: from the end of its create to the
   start of its pause, plus from the end of its resume to the start of its
   delete, at least; and from the start of the one to the end of the other,
   at most.
2. **Size times time.** A's vCPU-seconds and GiB-seconds must be its size,
   as its own kill event reports it, times its running time.
3. **Still running.** Sandbox B is created and asked about twice, two
   seconds apart: the first answer within the stopwatch's bounds, the second
   at least 1.9 s more.
4. **Deleted while paused.** Sandbox C runs about 2 s, is paused, and is
   deleted 3 s later. Its running time must be the same before and after the
   delete, and within the stopwatch's bounds of its run.
5. **Windows and groups.** Over everything, three sandboxes, and the
   per-sandbox groups add up to the total. By template, one group equal to
   the total. A window before anything ran is empty. A window of the last
   second holds B alone, at most 1.1 s.
6. **The ledger stays in.** `/events/sandboxes`, what an E2B client reads,
   must not hold `hm_usage`.
7. **Refusals.** A time that is not one, `from` after `to`, a grouping that
   is not one.

## Result

All seven passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-12. From [`report.json`](report.json):

- Sandbox A was counted **7.013 s**, within the stopwatch's 6.998 to
  7.316 s. It was paused for 4.0 s, which is not in it.
- C was counted 2.013 s before its delete and after it.
- All three together: 20.168 s, 20.168 vCPU-seconds, 5.042 GiB-seconds.

The first run of the check stopped at its first pause, in the check: it sent
no body, and the node's pause takes JSON.

## What this does not show

- **Teams.** The node ran with no keys, so the caller saw every sandbox.
  Narrowing to a caller's team is unit-tested, including a sandbox lost
  with its node, whose event names no team.
- **A truncated tail.** Fewer than 10,000 events were kept. Counting a
  sandbox whose start was dropped is unit-tested.
- **A cluster and its store**, and the control plane answering.
- **A node that dies**, and a node restart, which forgets a lone node's
  usage.
