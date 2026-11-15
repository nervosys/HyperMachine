# Team roles on a real KVM node: viewer, member, team-admin

**What this checks:** the team roles added to the [API key policy file](../../../API_KEY_ROTATION.md#team-names-for-roles-and-team-admin)
and written up in [Teams](../../../TEAMS.md#roles). It extends the earlier
[scopes, presets and roles check](../../2026-11-14/key-presets-roles-kvm/README.md) (its 25 cases are kept
unchanged) with new cases, through the shipped `hv2-control-plane` and `hv2-sandboxd` binaries over verified
TLS and node mTLS, with an owned Redis. Two real sandboxes boot on KVM: one in team `red`, one in team `blue`.

Driver: `tools/check-key-presets-roles-kvm.py`. Functional only; no performance claim.

## What was built

- `role: "viewer"` is a serde alias of `observer` and `role: "member"` of `operator`: the same enum variants, so
  the same behaviour (also in the single sign-on members file, which shares the role type).
- `role: "team-admin"` (new `ApiRole::TeamAdmin`). It needs `team_id` (an entry without one rejects the whole
  file), may not hold the `admin` scope, and cannot be combined with the `read-only` preset. A preset other than
  `read-only` may carry it. It is never an administrator: `is_administrator()` stays false.
- What it gains over an operator with the same scopes: `POST /sandboxes/{id}/owner` (owner adoption), only on a
  sandbox whose team is its own, and only with the `sandboxes` scope. `permits` allows the route for it; the
  middleware's existing team check refuses another team's sandbox (403) or a missing one (404) before any node is
  called; the handler checks the team again before forwarding.
- Owner adoption's existing node rule is unchanged: it assigns an owner to an unowned sandbox, accepts the same
  owner again, and never transfers an existing owner (409). It never changes a sandbox's team, so a team-admin
  cannot reassign a resource into another team.

## Result: 33 of 33 cases, three runs, all pass

Node log in each run: 3 `Kvm`, 0 `Tcg` platform lines; the guest ran commands.

| New case | Allowed (status) | Refused (status) |
|---|---|---|
| `viewer` alias | `GET /sandboxes` (200) | detail, `exec`, delete, owner adoption (403) |
| `member` alias | list, detail, `exec` returned `keyroles` (200) | owner adoption, the other team's sandbox detail (403) |
| `team-admin` (red) in its team | list (red's, not blue's), detail, `exec` (200); owner adoption with the sandbox's own owner (204) | adoption with a different owner (409, the node's no-transfer rule); `/volumes` without the scope (403) |
| `team-admin` across teams | | red lead on blue's sandbox: detail, owner adoption, `exec`, delete; blue lead on red's: owner adoption, detail (all 403) |
| `team-admin` on administrator-only routes | | `/cluster/events`, `/cluster/nodes/{node}/registrations/pending`, `.../registration/reconcile`, `POST /templates`, `/machines` (all 403) |
| policy file refusals | | a `team-admin` with no `team_id`, with the `admin` scope, and on the `read-only` preset: each makes the control plane exit nonzero at start with the reason |

## What stayed administrator-only

Node routes (`registrations/pending`, `registration/reconcile`), `/cluster/events` and template builds (none is
team-scoped in the code), and reassigning an existing owner (the node refuses it for everyone). There is no usage
or billing route to scope, and no member API.

## Unit and integration coverage (Windows, not KVM)

Unit tests in `keys.rs` cover the aliases, a `team-admin` without a team, with `admin`, and on `read-only`, and
a permits matrix against an operator with the same scopes. `a_team_admin_adopts_owners_in_its_own_team_only` in
`tests/control_plane.rs` covers adoption in its own team (including an unowned sandbox, which the KVM case cannot
make because real sandboxes always have an owner), 403 across teams in both directions, 403 for member and
viewer, 404 for a missing sandbox, no node call for any refused request, and an unchanged team.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-control-plane` | `c343ed7c01c7b358218fc814d51f13482ff70e981be77d1d112a970d790f3412` |
| `hv2-sandboxd` | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| driver | `e42b9362caaa560424f833669cfd8180fc21840743dad7c2a7515cca7789b3c1` |

Kernel, initrd and redis hashes are in each `run*/report.json`. The node memory budget was raised from 512 to
1024 MB so the second sandbox fits.

## NOT VERIFIED

- Single sign-on members with these roles (parsing and the team-admin validation are unit-level only; no sign-in
  was run).
- Owner adoption of an unowned sandbox in a team through real nodes (only the in-process fixture covers it).
- More than one node, SIGHUP reload of a file containing `team-admin` (the earlier reload cases ran with it in the
  policy, but none changes its role).
- `hv2-sandboxd` was not rebuilt for this change; it does not read the new role.
