# A run that prints faster than it is polled

**What this checks:** a defect found while
[checking durable runs](../machine-runs-durable-kvm/README.md). The guest
agent holds at most 1 MiB of a started program's output between the node's
polls. It dropped the oldest bytes beyond that without saying so, so a
run's byte count and `dropped` were wrong. It also turned a character split
across two polls, or cut by a drop, into U+FFFD.

Now the agent's `Poll` answer says how many bytes it dropped on each stream
(`stdout_dropped`, `stderr_dropped`, 0 from an older agent). It keeps a
character it has only begun for the next poll, and drops at a character
boundary. The node counts the dropped bytes, and polls again at once while
a program floods.

## Method

`tools/check-machine-run-flood-kvm.py` runs an owned `hv2-sandboxd` with one
512 MiB machine booted from a guest initrd carrying the new agent. The
agent was built static, as `hv2-guest-agentd`; initrd sha256 `36b4f2f4…`.

1. **Flood:** a run prints 9 MiB of `x` as fast as `head` and `tr` do, then
   `flood-done`. Its byte count is exact, and kept plus dropped is that
   count. The kept text starts where `dropped` says, is all `x`, and ends
   with the last line.
2. **Characters:** a run prints 1,536 lines of 1,024 `é` (two bytes each),
   as fast as `yes` does, about 3 MiB. No replacement character is in what
   is kept, which is only `é` and newlines.
3. **Slow:** a run printing 20 lines over a second has `dropped` 0, and
   every line.

The same check was then run with the guest initrd from before this change
(sha256 `1fcc60fa…`).

## Result

On 2026-10-13, on Linux 6.18.33 under WSL2 with nested KVM.

**With the new agent, all three cases passed** ([`report.json`](report.json)):

| | Bytes written | Counted as dropped | Kept |
|---|---|---|---|
| 9 MiB flood (0.52 s) | 9,437,196 | 8,388,620 | 1,048,576 |
| 3 MiB of `é` | 3,147,275 | 2,098,700 | 524,538 characters |
| 20 slow lines | 150 | 0 | all |

**With the old agent, the first case failed**
([`report-old-agent.json`](report-old-agent.json)). The 9 MiB run reported
1,048,576 bytes written, and nothing dropped.

In the agent's own tests, a program that wrote 3 MiB and 12,345 bytes
without being polled was accounted for to the byte: taken plus dropped.

## What this does not show

- **No drop at all for a fast printer.** The 9 MiB came in half a second,
  faster than the node can read. It is counted now, not prevented.
- **How much a gap costs.** After a gap only what came after it is kept,
  here 1 MiB of the 4 MiB a stream may keep.
- **Sandboxes.** The envd process service and the template builds poll the
  same way, and are given the counts, but do nothing with them yet.
