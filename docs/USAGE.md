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

Usage is added up from the lifecycle events a cluster keeps
(`/events/sandboxes`); each start event carries the sandbox's size, and each
stop says whether it was running. Nothing else is stored for it, so:

- **It goes back only as far as the events kept.** A cluster keeps the last
  10,000. The answer's `coveredFrom` is the oldest; before it nothing is
  known, and `truncated` says that events have been dropped.
- **A sandbox whose start was dropped** is counted from `coveredFrom`, at
  least as long as it ran, and its group says `partial`.
- **A node that dies** leaves its sandboxes counted as running until it
  comes back and records them lost.
- **A node alone keeps its events in memory**, so a restart forgets its
  usage. In a cluster they are in the cluster store.
- **Machines are not counted**, and neither is storage, network or
  requests. Usage is not sent anywhere: [TELEMETRY.md](TELEMETRY.md) pushes
  the node's own metrics, which are not per team.

[Checked on real KVM](benchmarks/2026-10-12/usage-kvm/README.md).
