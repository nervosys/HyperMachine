# Usage kept for good

**What this checks:** the usage ledger. Each lifecycle event is folded into
a ledger of running stretches as it is published. `GET /usage` reads that
ledger, rather than only the last 10,000 events. boat keeps usage per
account; see the [comparison](../../../BOAT_PARITY.md) and
[how usage is counted](../../../USAGE.md#what-it-is-counted-from-and-what-that-cannot-know).

## Method

Tests in `hv2-cluster`, one of them against a real Redis 8.0.2 server,
started on loopback for the run with no persistence configured.

1. **The ledger counts what the events do.** A sandbox is created at
   2 vCPUs, paused, resumed, resized to 4 and deleted. Folded through the
   ledger, its totals equal those read from the events: 30 running seconds
   and 80 vCPU-seconds.
2. **A stretch already running when the ledger began** is closed from that
   moment and marked `partial`. A paused sandbox that is deleted adds
   nothing.
3. **No double counting where the two meet.** For a sandbox created before
   the ledger began and deleted after, the events count up to the ledger's
   start and the ledger from there: 8 s, not 12. `coveredFrom` is the
   earlier of the two.
4. **In memory, usage outlives the event tail.** A sandbox runs 10 s. Then
   10,000 other events push its events out of the tail. Its usage is still
   10 s, and `truncated` is false; from the events alone it would be 0 s.
5. **In Redis, usage outlives the events and a reconnect.**
   - One sandbox is created, resized and deleted, and another is created
     and left running.
   - Then eight more are each created and paused by eight connections at
     once, to exercise the compare-and-set.
   - The event stream is trimmed to nothing, and a new connection reads the
     ledger. It has every stretch: the two sizes of the first, the second
     still open, and the eight.
   - A window starting later leaves out stretches that ended before it.

## Result

All 181 `hv2-cluster` tests passed on 2026-10-13 under WSL2, with
`HV2_TEST_REDIS` pointed at that server, so the Redis test ran rather than
being skipped. `cargo fmt --check` and clippy with `-D warnings` were clean.

## What this does not show

- **A running node** publishing to a Redis cluster store end to end. The
  store and the report were tested; a cluster of nodes on KVM was not run.
- **Redis persistence across a server restart.** The test reconnects to a
  live server. Whether a restart keeps the ledger is the Redis server's own
  persistence (AOF or RDB), as for everything else in the store.
- **A ledger's growth** over months. Each stretch is a couple of hundred
  bytes in a sorted set, never trimmed.
- **Machines, storage and network,** which are not counted.
