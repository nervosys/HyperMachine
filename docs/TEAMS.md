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

- A file with no `team_id` anywhere is one team, except that keys with different
  principals do not see each other's resources: see
  [a key with a principal and no team](#a-key-with-a-principal-and-no-team).
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

## Roles

A key's `role` (or a signed-in member's) sets what it may do inside its team.
Roles never widen what a team key reaches: every role below sees only its own
team's resources.

| Role | What it can do | What it cannot |
|---|---|---|
| `viewer` (same as `observer`) | `GET`/`HEAD` the inventory routes (`/sandboxes`, `/v2/sandboxes`, `/templates`, `/sandboxes/metrics`, `/cluster/nodes`) its scopes allow; `/cluster/nodes` shows it load and the templates it may start, not node addresses, signing keys or other teams' snapshot names (administrators see everything) | Sandbox detail, exec, create, pause, delete, volumes, events, owner adoption: 403, even with more scopes |
| `member` (same as `operator`, and the default) | Everything its scopes allow on its team's sandboxes, volumes, machines, webhooks, snapshots, whoever created them | Owner adoption, node routes, `/cluster/events`, building templates (403); another team's resources (403 or 404) |
| `team-admin` | Everything a `member` with the same scopes can, plus `POST /sandboxes/{id}/owner` on a sandbox of its own team (it needs the `sandboxes` scope) | Another team's sandbox (403, and 404 when it does not exist); node management, `/cluster/events`, template builds, cluster-wide usage (403); changing a sandbox's team; any use of the `admin` scope |
| administrator (operator with the `admin` scope) | Everything, in every team | Nothing |

`viewer` and `member` are the team names for the existing `observer` and
`operator` roles: the policy file reads them as the same roles, so a key written
either way behaves identically. `team-admin` is the one new role. It needs a
`team_id` (an entry without one rejects the whole file), cannot hold the `admin`
scope, and cannot be combined with the `read-only` preset. It is never an
administrator and it is not global. See the
[policy field](API_KEY_ROTATION.md#team-names-for-roles-and-team-admin).

**What `team-admin` actually adds is small, because the code allows little.**
Every member already acts on every sandbox in its team, whoever created it, so
acting on a teammate's sandbox is not new. Owner adoption is the one
team-management action that today needs the cluster `admin` scope and can be
scoped to a team without changing its meaning: it assigns an owner to a sandbox
that has none, accepts the same owner again, and never transfers an existing
one (409 `existing owner cannot be transferred`), and it never touches the
sandbox's team. A team-admin therefore cannot move a resource into another team,
and in practice adopts owners only for team sandboxes that lost or never had one.

Left administrator-only, because the code has no team-scoped form of them:

- Node routes (`/cluster/nodes/{node}/registrations/pending`,
  `.../registration/reconcile`): they act on a node, which no team owns.
- `/cluster/events`, and building templates (see "Not yet partitioned" below).
- Cluster-wide usage and billing. A team's own key reads its team's usage with
  the `events` scope ([USAGE.md](USAGE.md)), and keeps its team's environments
  with the `sandboxes` scope ([ENVIRONMENTS.md](ENVIRONMENTS.md)).
- Adding, removing or changing members through the policy file. A team-admin
  makes and revokes its own team's keys through the
  [key API](API_KEY_ROTATION.md#managed-keys), which is how it adds and removes
  access (see [Members](#members)); a policy-file key is the operator's.
- Reassigning an owner who exists. The node refuses it for everyone, an
  administrator included.

Evidence: [`team-admin`, `viewer` and `member` on a real KVM node](benchmarks/2026-11-15/team-roles-kvm/README.md).

## Members

```
GET /teams/{team}/members
-> {"teamId": "red", "members": [
     {"principalId": "alice", "roles": ["operator"], "keyCount": 2},
     {"principalId": "lead",  "roles": ["team-admin"], "keyCount": 1}]}
```

A member here is a principal that holds at least one unexpired key of the team: keys from the policy file and
[managed keys](API_KEY_ROTATION.md#managed-keys) alike, one entry per principal, with each role its keys hold
(policy names: `operator` is `member`, `observer` is `viewer`) and how many keys it has. Administrators are
global and are not members of a team, even if their key names one. Expired keys count for nothing, and a
principal whose keys all expired is not listed. People who sign in through single sign-on are not listed
either: their membership is the members file, not a key.

An administrator lists any team; one that has no keys is an empty list. A `team-admin` lists its own team,
and another team's is 403 (not an empty list). Every other caller gets 403. The listing never contains a
secret, a digest or a key id.

Adding and removing members is done with the key API: a team-admin makes a key for a principal
(`POST /keys` with `principalId`; a principal that a key outside the team, or an administrator's key, already
carries is refused with 403, because principals are global), and revokes it (`DELETE /keys/{id}`) to remove the access it gave. Policy-file
members are changed by the operator, in the file. There is still no durable record of a member apart from its
keys.

## What a team key can do

| | Same team | Other team, or no team |
|---|---|---|
| List `/sandboxes`, `/v2/sandboxes`, `/sandboxes/metrics` | listed | not listed |
| Detail, exec, pause/resume, fork, ports, logs, checkpoints, delete | yes (operator); observers read the inventory only | 403 |
| Public ports, web sharing, private networks | yes: a teammate acts as the sandbox's creator | 403 |
| `/events/sandboxes/{id}` | yes | 403 |
| `/events/sandboxes` (all events) | its team's sandboxes' events | not listed |
| Webhooks (`/events/webhooks…`) | the team's own: create, list, change, delete, deliveries | not there (404) |
| Volumes (`/volumes…`, `volumeMounts`) | the team's own, by name | not there (404), and a mount by name finds only the team's |
| Block disks (`/disks…`, `diskMount`; on the node, which the control plane does not route) | the team's own, by name | not there (404), and a `diskMount` by name finds only the team's |
| [Machines](MACHINES.md#through-the-control-plane) (`/machines…`) | the team's own, by name | not there (404), by name and by ID |
| Snapshots and templates (`/snapshots`, `/templates`, aliases, `templateID`) | the team's snapshots, plus every operator template | not listed, not startable (404), not deletable |

A snapshot belongs to the team of the sandbox it was taken from, even when an
administrator takes it, because it holds that sandbox's memory. Only that team
lists it, starts sandboxes from it (an administrator cannot start a teamless
copy either) and deletes it. Snapshot names are one namespace, so a name
another team holds answers 409. Operator templates (`base`, images built by an
administrator) belong to no team: every team may start from them, and only
administrators delete them. A node checks each delete against the team the
control plane sends with it, so a team's delete reaches the node only for its
own snapshots; a call to a node with the cluster token and no team is an
administrator's, and deletes any.

## Sharing a snapshot with another team

A team can let another team start sandboxes from one of its snapshots:

```
PUT    /templates/{snapshot}/shares/{team}   {"acknowledgeContents": true}
DELETE /templates/{snapshot}/shares/{team}
GET    /templates/{snapshot}/shares          -> {"teamID", "sharedWith": [...]}
```

Only the snapshot's own team (or an administrator) may do these; the team it
is shared with cannot read the list, share it on, revoke it or delete the
snapshot (404 each). The team shared with lists it in `/snapshots` and
`/templates` and starts sandboxes from it; those sandboxes are its own team's.
Revoking stops new starts at once; sandboxes already started run on.

**A shared snapshot is the source sandbox as it was: its memory, its files and
its environment variables.** Nothing is removed first (a layered memory
snapshot cannot be cleaned after the fact), which is why the request must say
`acknowledgeContents: true`. Take the snapshot from a sandbox that never held
what you do not want the other team to have. A Git credential held on the host
(`--team-git-credential`, `--git-credential`) is not in the guest, so not in the
snapshot; `--git-credential-in-sandbox`, which this was not checked with, is a
different matter. Taking a new snapshot of the same name replaces
it and drops its shares. An operator's template is every team's already, and a
machine's snapshot cannot be shared. The share list is kept with the snapshot
in the snapshot store, so it survives a node restart. [Checked on
KVM](benchmarks/2026-10-23/snapshot-share-kvm/README.md).

A volume belongs to the team that created it. Its ID is derived from the team
and the name, so two teams can each have a volume called `data`, and a
sandbox's `volumeMounts` finds a name among its own team's volumes only. The
volume content API still uses each volume's own bearer token. Volumes made
before teams belong to no team, and only administrators reach them.

A webhook belongs to the team that created it and receives only that team's
sandboxes' events. An administrator's webhook receives every event. An event
that concerns no sandbox's team (a node joining) is visible to administrators
only.

## A key with a principal and no team

A key that names a principal but no team (a policy file where no key names a
team, or a signed-in member without one; the file rule above refuses it once
any key names a team) acts as a team of one: it sees and changes only what its
principal created. An administrator is unchanged,
and so is a team key.

| | Teamless key with a principal |
|---|---|
| Sandboxes, their events, ports, sharing | the principal's own (unchanged) |
| `/events/sandboxes` | events of its own sandboxes; an event with no principal scope (made before this, or by no sandbox) is not listed |
| Webhooks | its own: create, list, change, delete, deliveries (another's is 404); it is delivered only events about its own sandboxes and machines |
| Volumes, machines | its own; a volume or machine with no owner is not there (404) |
| Snapshots, templates, shares | its own snapshots, those shared with it, and every operator template; it cannot delete an operator's template or share one it does not own |
| `/telemetry` | its own destination (an administrator's is separate) |
| `/cluster/events` | 403: the log lists every principal's and team's events |

How: the control plane gives such a key the scope `principal.` plus 32 hex digits
of the SHA-256 of its principal, sends that to nodes as the key's team, and
every team check applies as it does to a team. A real team may not use a label
beginning `principal.`: a key that does is refused with 403. Two keys with the
same principal share a scope, as two keys of one team do. Records made before
this carry no such scope: events, webhooks, volumes, machines, snapshots and
environments of a teamless key are visible to administrators only, and
sandboxes made before it stay with their owner (the owner still sees its own
sandboxes, but not their old events).

Template builds are scoped the same way, by name. A node has one template
namespace and records no owner on a build, so the control plane builds under
`scope.` + 16 hex digits of the principal's hash + `--` in front of the name
the key gave (`app` is `scope.<hash>--app` on the node, which leaves 40 of a
node's 64 name characters), and finds it again the same way:

| | Teamless key with a principal | Another principal, or a team key |
|---|---|---|
| `POST /templates`, `POST /v3/templates` | builds under its own scope; `app:v1` keeps its tag | cannot name, rebuild or overwrite it: a name under another scope is 403 (a team key is 403 on builds anyway) |
| Build files, start, status (`/templates/{id}/…`, `/v2/templates/{id}/builds/{build}`) | `app` and the ID the build answered with both work; a start may build `fromTemplate` its own | 404, and nothing reaches the node: a build ID is checked to be one of the caller's before any call that names it |
| `GET /templates`, aliases, `/cluster/nodes`, `/snapshots` | its own builds (alias `app`) and every operator template | not listed, no alias (404) |
| `templateID` of a sandbox or machine | `app` means its own build if it has one, else the operator's `app` | 404 |
| `DELETE /templates/{id}` | its own builds | 404 |

An administrator names every scope's templates. A team key cannot name any:
templates called `scope.…` are reserved, so an operator's template must not be
named that way (it would be hidden from every non-administrator). A principal
may build a name an operator also uses; for that principal, its own wins.
Builds made before this have no scope: they are the operator's, as before.
A build a team asks for is recorded on the node as that team's snapshot, as its
sandboxes' snapshots are: only that team deletes it or starts from it (unless
it shares it), and it replaces only that team's own template of the name. A
name another team or an operator holds is 409 on the node. The control plane
refuses team keys on builds (403, below), so this is reached by calling a node
directly with the cluster token.
Still shared on the node: uploads for `COPY` are content-addressed (a hash of
the files, whoever sent them), as are pulled images (`image-…`) and cached
build steps (`build-cache-…`), which two builds with identical inputs share.

The team travels with the sandbox. The control plane sends it to the node on
create, in an internal header that clients cannot set, and the node records it.
Forks inherit it. Pause and resume, on any node, keep it. An update
that omits it cannot clear it.

Evidence: [two teams on one real KVM cluster](benchmarks/2026-10-07/teams-kvm/README.md):
sandboxes, volumes, snapshots, forks, events and webhooks, through the shipped
control plane and node over TLS and mTLS.

## Not yet partitioned

These resources are still shared by every team. Until each has a per-team
namespace, team keys get 403 on them ("shared by every team"), and only
administrators use them:

- Building templates from images (`POST /templates`, `/v2/templates`,
  `/v3/templates`, and build files and status under `/templates/{id}/…`)
- `/cluster/events`, the operator's view of nodes and sandboxes together

Also not done: team membership as durable records apart from keys (a member
is the principal of a key; see [Members](#members)), and per-team private
networks. Private networks are still per creator.
