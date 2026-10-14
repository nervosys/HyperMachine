# Usage

How long a caller's sandboxes ran, and with how much, in a window of time.

```sh
curl -sS "$NODE/usage?from=2026-10-01T00:00:00Z&by=template" -H "x-api-key: $KEY"
hm sandbox vm usage --from 2026-10-01T00:00:00Z --by template
```

```json
{
  "from": "2026-10-01T00:00:00.000Z", "to": "2026-10-12T15:40:00.000Z", "by": "template",
  "coveredFrom": "2026-10-03T08:12:44.120Z", "truncated": false,
  "total": {"sandboxes": 3, "runningSeconds": 20.168, "vcpuSeconds": 20.168,
            "memoryGiBSeconds": 5.042, "partial": false},
  "groups": [{"key": "base", "sandboxes": 3, "runningSeconds": 20.168, ...}]
}
```

| Query | |
|---|---|
| `from`, `to` | RFC 3339, or milliseconds since the epoch. `to` is now and `from` a day before it unless given |
| `by` | `sandbox` (the default), `team`, `template`, or `owner`, the label of the key that created it |

- **Running time is counted, paused time is not.** A sandbox runs from its
  creation, fork or resume to its pause, deletion, expiry or loss. A paused
  sandbox holds a snapshot on disk, not a processor or memory. Deleting a
  paused sandbox adds nothing.
- **A sandbox still running counts to the moment asked**, so asking twice
  gives two numbers.
- **vCPU-seconds and memory GiB-seconds** are its size times its running
  time, the units compute is commonly billed in.
- **A caller sees its own team's**, as with events; an operator's key, which
  has no team, sees all.
- **The node and the control plane both answer**, each from the cluster's
  events: a node alone from its own, a control plane from every node's.

## What it is counted from, and what that cannot know

Each lifecycle event carries what it means for usage: a start event, the
sandbox's size; a stop, whether it was running. As each is published, the
store folds it into a **usage ledger**: the stretch each sandbox is running
now, and every stretch that has ended, each with its team, template, owner
and size. The cluster keeps only the last 10,000 events, but the ledger is
kept for good:

- **In a cluster, the ledger is in the cluster store** (Redis) and is never
  trimmed. Usage goes back to when the ledger began, whatever happened to
  the events since. It grows by a couple of hundred bytes for each stretch
  of running.
- **Before the ledger began**, usage comes from the events, as far back as
  they go; the two meet where the ledger began, so nothing is counted twice.
  The answer's `coveredFrom` is the earliest either knows, and `truncated`
  says events from before the ledger were dropped.
- **A sandbox already running when the ledger began**, or whose start event
  was dropped, is counted from then, at least as long as it ran, and its
  group says `partial`.
- **A node that dies** leaves its sandboxes counted as running until it
  comes back and records them lost.
- **A node alone keeps its events and its ledger in memory**, so a restart
  forgets its usage, and it keeps the last 100,000 ended stretches. In a
  cluster they are in the cluster store.
- **Machines are not counted**, and neither is storage, network or
  requests. Usage is not sent anywhere: [TELEMETRY.md](TELEMETRY.md) pushes
  the node's own metrics, which are not per team.

[Checked on real KVM](benchmarks/2026-10-12/usage-kvm/README.md).
