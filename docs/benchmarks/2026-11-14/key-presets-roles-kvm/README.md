# API key scopes and roles on a real KVM node

**What this checks:** the two claims in the [boat parity scorecard](../../../BOAT_PARITY.md) that had no
kept report: "presets" for [scoped API keys](../../../API_KEY_ROTATION.md) and "roles within a team"
([Teams](../../../TEAMS.md)). It goes through the shipped `hv2-control-plane` and `hv2-sandboxd`
binaries over verified TLS and node mTLS, with an owned Redis, and boots one real sandbox on KVM.

## What exists, and what does not

- **There is no "preset".** The word appears nowhere in the code or the docs. What a key may do is set by
  its `scopes` (`admin`, `inventory`, `sandboxes`, `templates`, `volumes`, `events`, `machines`,
  `telemetry`), which an operator writes into the key policy file by hand. The driver treats each scope as
  the nearest thing, a preset. There are no named bundles, no CLI or API to create a key from one, and no
  key-creation API at all: keys are operator-provisioned.
- **There are two roles, not a team hierarchy.** `role` is `operator` (the default) or `observer`
  (`ApiRole` in `crates/hv2-cluster/src/keys.rs`). An administrator is an operator whose scopes include
  `admin`. There is no team owner, admin, member or viewer, and no API to add, remove or change a team
  member. A team is just the `team_id` on a key. [TEAMS.md](../../../TEAMS.md) says membership as durable
  records with an API is not done, and the check agrees. Roles from single sign-on (`sso.rs`) were not run.

## Method

`tools/check-key-presets-roles-kvm.py` writes a policy with one key per scope (all in team `red`, except
`admin`, which stays global), an operator key (`sandboxes`, `inventory`), an observer key (`inventory`,
role `observer`), an observer key whose scopes include `admin`, an operator key in team `blue`, and a key
whose expiry is in the past. The `sandboxes` key creates one sandbox, which boots on KVM and runs
`echo keyroles`. Every case then makes real requests and compares the status. A case passes only if every
request in it got the expected status (and any body check held). The run passes only if all 16 cases do.
`report.json` records every request, the status expected and got, and the first 240 bytes of the response.

## Result: 16 of 16 cases, three runs, all pass

| Case | Allowed (status) | Refused (status) |
|---|---|---|
| ran on KVM | node log: 2 `Kvm`, 0 `Tcg` platform lines; the guest ran a command | |
| preset admin | `/cluster/nodes`, `/sandboxes`, sandbox detail, `/volumes`, `/machines` (200) | nothing: admin reaches all |
| preset inventory | `GET /sandboxes` lists (200) | sandbox detail, `POST /v2/sandboxes`, `/volumes` (403) |
| preset sandboxes | detail, `exec` returned `keyroles` (200) | `/volumes`, `/templates`, `/events/sandboxes`, `/machines` (403) |
| preset templates | `GET /templates` (200) | create sandbox, sandbox detail, `/volumes` (403) |
| preset volumes | create volume (201), list (200) | sandbox detail, `/sandboxes` (403) |
| preset events | `/events/sandboxes`, `/events/sandboxes/{id}` (200) | `/sandboxes`, `/volumes` (403) |
| preset machines | `GET /machines` (200) | `/sandboxes`, `/volumes` (403) |
| preset telemetry | `GET /telemetry` (404, "this team has no telemetry destination": reached, nothing set) | `/sandboxes`, `/volumes` (403) |
| unknown, expired keys | | `GET /sandboxes`, `POST /v2/sandboxes` (401) |
| role operator | list, detail, `exec`, `GET /cluster/nodes` (200) | `POST /sandboxes/{id}/owner`, `/volumes` (403) |
| role observer | `GET /sandboxes`, `/sandboxes/metrics` (200) | detail, `exec`, create, pause, delete, `/volumes` (403) |
| role observer + admin scope | `GET /sandboxes` (200) | detail, create, delete (403) |
| other team's operator | lists only its own (200) | red's sandbox detail, delete (403) |
| role administrator | `/cluster/nodes`, `/sandboxes` lists red's, delete (204) | |
| after delete | red's list no longer holds it (200) | |

## Findings

1. **"Presets" is not a feature.** Scopes are, and each one grants and refuses what its name says. Nothing
   bundles them. The scorecard's "Yes, with presets" for the comparison column is not something HyperMachine
   offers in that form. Not changed: product code.
2. **The only roles are operator and observer.** The "roles within a team" the scorecard row implies (owner,
   admin, member, viewer) do not exist. Observer is the viewer-like role, and it is enforced: it lists
   `/sandboxes`, `/v2/sandboxes`, `/templates`, `/sandboxes/metrics` and `/cluster/nodes` only, and gets 403
   everywhere else, even with the `admin` scope. "Manage members" has no API to refuse.
3. `inventory` scope on a team key reads `/cluster/nodes`, which lists every node and its template names
   across teams (observed with an operator key of team `red`). That matches the documented route list in
   [API_KEY_ROTATION.md](../../../API_KEY_ROTATION.md); it is recorded because a team key sees cluster-wide node data.
4. A policy needs `team_id` and `principal_id` on every non-admin key once any key names a team; this
   driver complied, so a refusal of a key without them was not exercised here.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release, built 2026-11-13) | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| `hv2-control-plane` (release, built from the working tree on 2026-11-14) | `70fd013042b6bdd1578e32ce91322c711f83418ececf7408a0ad3a9975b682b7` |
| Kernel `bzImage` | `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297` |
| Initrd `busybox.cpio.gz` | `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485` |
| `tools/check-key-presets-roles-kvm.py` | `93ed254dfa2cbbf32dfcc16593efd59bb3470fddf2f722d3c459d396ced25dd8` |
| `redis-server` 8.0.2 (unpacked Debian package) | `b747f74364ce61350ce4d2cd0e38ca2eacbfa7fe6f3a2560483ab6403a2e8573` |

Run it with `python3 tools/check-key-presets-roles-kvm.py --daemon … --control … --kernel … --initrd … --output DIR`
(add `--redis` if `redis-server` is not on `PATH`). The host was WSL2 with real KVM. The daemon and control
plane were built at different times, from the same tree apart from the commits between them.

## Not shown here

- A sandbox's `exec` as a non-owner teammate with an observer role is refused, but pause/resume, fork,
  webhooks and volume mounts per role were not each run; the scope check is one gate for all of them.
- Single sign-on members and their roles (`sso.rs`), and durable team membership: none checked.
- Key reload with `SIGHUP` and replacement: see [the 2026-10-01 report](../../2026-10-01/api-key-reload.md).
- More than one node, and TLS to a real client other than this driver.
