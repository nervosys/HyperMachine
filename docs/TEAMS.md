# Teams

A team is the isolation boundary between tenants on one control plane. Each
API key belongs to one team. Each sandbox belongs to the team of the key that
created it. A key reaches every sandbox in its team, whoever created it, as far
as its role allows, and nothing in any other team.

## Turning it on

Teams are set in the [API key policy file](API_KEY_ROTATION.md) with a `team_id`
on each key:

```json
[
  {"sha256": "…", "expires_at": 1830000000, "scopes": ["sandboxes", "inventory"],
   "principal_id": "alice", "team_id": "red"},
  {"sha256": "…", "expires_at": 1830000000, "scopes": ["inventory"], "role": "observer",
   "principal_id": "red-dashboard", "team_id": "red"},
  {"sha256": "…", "expires_at": 1830000000, "scopes": ["sandboxes", "inventory"],
   "principal_id": "carol", "team_id": "blue"},
  {"sha256": "…", "expires_at": 1830000000, "scopes": ["admin"]}
]
```

- A file with no `team_id` anywhere behaves exactly as before: one team.
- Once any key names a team, every key that is not an administrator must name
  both `team_id` and `principal_id`. A file that leaves one out is refused at
  startup and on reload. A key with no team would otherwise be global by
  omission.
- An administrator (operator role and `admin` scope) stays global, whatever its
  `team_id` says.
- `team_id` follows the `principal_id` rules: 1–128 ASCII letters, digits,
  dots, hyphens or underscores.
- Reload with `SIGHUP`, as for any key change. A sandbox keeps the team it was
  created in; reloading keys never moves one.

## What a team key can do

| | Same team | Other team, or no team |
|---|---|---|
| List `/sandboxes`, `/v2/sandboxes`, `/sandboxes/metrics` | listed | not listed |
| Detail, exec, pause/resume, fork, ports, logs, checkpoints, delete | yes (operator); observers read the inventory only | 403 |
| Public ports, web sharing, private networks | yes: a teammate acts as the sandbox's creator | 403 |
| `/events/sandboxes/{id}` | yes | 403 |
| `/events/sandboxes` (all events) | its team's sandboxes' events | not listed |
| Webhooks (`/events/webhooks…`) | the team's own: create, list, change, delete, deliveries | not there (404) |

A webhook belongs to the team that created it and receives only that team's
sandboxes' events. An administrator's webhook receives every event. An event
that concerns no sandbox's team (a node joining) is visible to administrators
only.

The team travels with the sandbox. The control plane sends it to the node on
create, in an internal header that clients cannot set, and the node records it.
Forks inherit it. Pause and resume, on any node, keep it. An update
that omits it cannot clear it.

## Not yet partitioned

These resources are still shared by every team. Until each has a per-team
namespace, team keys get 403 on them ("shared by every team"), and only
administrators use them:

- Volumes (`/volumes`), snapshots (`/snapshots`, `POST /sandboxes/{id}/snapshots`)
- Template builds and deletion (`POST /templates`, `/v2/templates`, `/v3/templates`,
  `/templates/{id}…`). Reading `GET /templates` is allowed.
- `/cluster/events`, the operator's view of nodes and sandboxes together

Also not done: single sign-on (keys are still operator-provisioned), team
membership as durable records with an API, and per-team private networks.
Private networks are still per creator.
