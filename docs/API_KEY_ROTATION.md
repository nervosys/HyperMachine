# Rotate scoped control-plane keys

On Unix, a control plane started with `--api-keys-file PATH` reloads that same file when its process receives `SIGHUP`. Write a complete replacement JSON array to a temporary file, atomically replace the configured file, then send `kill -HUP PID` to the control-plane process you operate. Observe its `API key policies reloaded` message. Check the new credential and verify the revoked credential is refused through the API. Signal every control-plane replica; replacements are local to each process.

Policy documents are limited to 1 MiB and must be UTF-8. Startup and reload read at most the limit plus one byte; embedded JSON replacement applies the same limit before parsing. Oversized and invalid UTF-8 files are refused with errors that omit file contents. The full replacement is parsed and validated before it becomes active. Invalid JSON, empty arrays, duplicate credentials, invalid scopes and collisions with the legacy admin credential are rejected; active policies remain unchanged. A read or task error also preserves active policies. Error messages omit credential values and policy contents. Expiry remains checked on every request. Existing authorized requests and open streams continue; revocation applies to subsequent authorization decisions.

This replaces only scoped policies. The legacy `HV2_API_KEY` admin credential remains unchanged and requires restarting the process to rotate. An empty policy list is refused, so a reload cannot accidentally open an unauthenticated API. To revoke all scoped credentials, replace them with a valid policy whose expiry has already passed. Newly created policy files and their replacement must remain readable by the service account.

On other platforms, signal reload is unavailable; restart the binary with the replacement file. Embedded users can call `ControlPlane::replace_api_key_policies` to apply the same validated atomic replacement. Unix signal handling is installed only when `--api-keys-file` is configured; do not send SIGHUP to an instance without that option.

A real HTTP integration test checks old-key revocation, replacement-key access, unchanged admin access, scope enforcement and rejected updates preserving active policies. [Process-level SIGHUP verification](benchmarks/2026-10-01/api-key-reload.md) passed 16 real HTTP checks using an owned Unix control-plane process. Rotation itself does not change roles (see [team names for roles, and team-admin](#team-names-for-roles-and-team-admin)), and it does not provide central policy distribution or interruption of already-authorized work.

[Bounded-reader process verification](benchmarks/2026-10-01/api-key-bounds.md) passed 20 HTTP checks, including oversized and invalid UTF-8 reloads preserving active policies and subsequent valid recovery. Startup rejection uses the same reader and has library coverage; these process checks cover reload.

## Single-team observer and operator roles

Policies accept an optional `role`: `operator` (the default for existing policies)
or `observer`. Unknown values and explicit null are rejected by startup and
atomic replacement. Roles apply to the configured team; without a `team_id` on the keys there is
no tenant isolation (see below for `team_id`). Public-port management separately requires matching creator ownership.

An observer can only GET/HEAD `/sandboxes`, `/v2/sandboxes`, `/templates`,
`/sandboxes/metrics` and `/cluster/nodes`, and only where its scopes also allow
the route. The role caps even `admin` scope. All other protected routes are
denied, including sandbox detail/name lookup, volume credentials, upload URLs,
execution and GET TCP upgrades. Sandbox listings omit the guest access token.
Operators retain the existing scope behavior. For an inventory observer, add
`"role":"observer"` to a policy with `"scopes":["inventory"]`.

### Team names for roles, and `team-admin`

`role` also accepts the team names for the two roles above and one new role:

| `role` | Same as | Notes |
|---|---|---|
| `viewer` | `observer` | an alias: parsed to the same role, same behaviour |
| `member` | `operator` | an alias: parsed to the same role, same behaviour |
| `team-admin` | an operator, plus owner adoption | needs `team_id`; see [Teams](TEAMS.md#roles) |

A `team-admin` entry that has no `team_id` rejects the whole file (startup fails; a reload is refused and the
active keys stay). So does one that holds the `admin` scope (cluster-wide, which a team role is not), and
`"preset":"read-only"` with `"role":"team-admin"` (read-only stays read-only). Any other preset may carry
`"role":"team-admin"`. A team-admin is never an administrator. Existing entries, which use `operator`,
`observer` or no role, behave exactly as before.

Roles reload with the rest of a policy. They govern subsequent authorization
decisions; they do not revoke previously issued bearer tokens or open streams.
Public endpoints and independently authenticated guest/upload/content routes
keep their existing authentication model. Configure node cluster tokens to
prevent unauthenticated direct node access.

[Preserved observer-role verification](benchmarks/2026-10-02/observer-role/README.md)
includes 66 passing library/HTTP tests on each platform and a 23-case real KVM
run with TLS, node mTLS, unchanged guest state after denied mutations, and 222
independently verified access records.

[Live role-reload verification](benchmarks/2026-10-02/role-reload/README.md)
passed 33 HTTP checks in each of two owned Unix processes. Same-key downgrades
blocked capabilities, upgrades restored access, and unknown/null role updates
preserved the active observer policy after acknowledged rejection. Each check
sequence kept the process running and stopped it afterward.

## Presets

A policy entry may name a `preset` instead of (or as well as) listing `scopes`. A preset expands, when the
file is loaded or reloaded, to a fixed set of scopes and a role:

| `preset` | Scopes | Role |
|---|---|---|
| `read-only` | `inventory` | `observer` |
| `developer` | `sandboxes`, `templates`, `volumes`, `events`, `inventory` | `operator` |
| `machines` | `machines`, `inventory`, `events` | `operator` |
| `admin` | `admin` | `operator` |

```json
[
  {"sha256": "<hex digest>", "expires_at": 1893456000, "preset": "developer",
   "team_id": "red", "principal_id": "alice"},
  {"sha256": "<hex digest>", "expires_at": 1893456000, "preset": "read-only",
   "team_id": "red", "principal_id": "dashboard"},
  {"sha256": "<hex digest>", "expires_at": 1893456000, "preset": "machines",
   "scopes": ["volumes"], "team_id": "red", "principal_id": "fleet-bot"}
]
```

- Explicit `scopes` on the same entry are added to the preset's (a union). `scopes` may be omitted when a
  preset is given; with neither, the entry is rejected as before.
- An explicit `role` of `observer` lowers any preset's role. `role: operator` is the default for every
  preset except `read-only`.
- `read-only` stays read-only: scopes beyond its own, or `role: operator`, reject the whole file. Because
  its role is `observer`, it can only GET/HEAD the five inventory routes listed above, which is why it
  carries `inventory` alone: an observer is refused events and telemetry whatever its scopes.
- Preset names are lower case and exact. An unknown name, an empty or `null` `preset`, or a wrong case
  rejects the whole file: startup fails, and a reload (`SIGHUP` or `replace_api_key_policies`) is refused
  with the active keys left unchanged, as for every other policy error.
- An entry with no `preset` behaves exactly as before. A preset changes nothing else: `team_id`,
  `principal_id`, expiry and the multi-tenant rules apply as they do to any key. An `admin` preset key with
  no team is an administrator and stays global.

Presets work in the file and in the [key API](#managed-keys). [Three real KVM runs of 25
cases](benchmarks/2026-11-14/key-presets-kvm/README.md) check each preset's allowed and refused requests,
a start with an unknown preset, and reloads with bad files.

## Managed keys

The policy file is the operator's. Keys can also be made, listed and revoked at run time through the control
plane's API, and they are kept in the cluster store (Redis or the in-memory store), never in the file, which
the API cannot read or change. A managed key behaves exactly like a policy entry with the same fields: the
same scopes, presets, roles, team and principal rules, and the same expiry check on every request.

| Route | What it does |
|---|---|
| `POST /keys` | Makes a key. Returns 201 with `{id, key, label, scopes, role, teamId, principalId, expiresAt, createdAt}`. **`key` is shown only here.** |
| `GET /keys` | Lists keys: `{keys: [{id, label, scopes, role, teamId, principalId, createdBy, createdAt, expiresAt, expired}]}`. Never a secret or a digest. |
| `DELETE /keys/{id}` | Revokes a key: `{"deleted": id, "kind": "key"}`. An id the store does not hold is 404. |
| `GET /teams/{team}/members` | Lists a team's principals; see [Teams](TEAMS.md#members). |

These routes are on the control plane only, not on `hv2-sandboxd`.

```json
POST /keys
{"label": "ci runner 4", "expiresInSeconds": 86400, "preset": "developer",
 "teamId": "red", "principalId": "ci-4"}
```

- `label` (1-128 characters, no control characters) and `expiresInSeconds` are required. Expiry is from 60 up
  to 90 days; an operator changes the upper limit with `--managed-key-max-ttl-secs` (or
  `ControlPlane::set_managed_key_max_ttl`), up to ten years.
- `preset`, `scopes` and `role` are expanded and validated by the policy file's own code: the same
  presets, the same `viewer`/`member` aliases, the same refusals (`read-only` cannot be raised, a `team-admin`
  needs a team and cannot hold `admin`, scopes may not be empty, unknown fields and explicit `null` are
  400).
- `teamId` is required unless the key is an administrator's (operator role with the `admin` scope). A key
  with no team would be global, and the policy file's rule is that only an administrator may be. A key with
  a team always has a principal: `principalId` if given, otherwise its own `id`. A team-admin may name a
  principal its own team already uses (a new key for a member), but not one a key outside its team, or an
  administrator's key, carries (403): principals are global, and a key for one is a key as that person.
- The secret is `hm_` and 43 characters from 32 bytes of the OS random generator (through the same
  FIPS-capable module the control plane uses for its session tokens). The `id` (`mk_` and 16 characters) is
  random and not derived from it. The store holds the SHA-256 of the secret, the id, the expanded scopes, the
  role, the team, the principal, the label, who created it and when. A secret whose digest equals a policy
  entry's, or the legacy admin key, is refused (not reachable with 256 random bits, but checked).
- At most 1000 unexpired managed keys are held (429 beyond that; `ControlPlane::set_managed_key_limit`).
  Expired keys stay listed, marked `expired`, until deleted.

### Who may use them

| Caller | Create | List | Delete | Members |
|---|---|---|---|---|
| administrator (legacy admin key, or an operator key with `admin`) | any team, any role, any scope, including a team-admin or another administrator | all | any | any team |
| `team-admin` | its own team only | its own team's | its own team's (other: 404) | its own team (other: 403) |
| everyone else: observers, operators, `read-only`, inventory keys, SSO members who are not administrators | 403 | 403 | 403 | 403 |

A team-admin cannot raise anyone above itself. Each rule below answers 403:

1. `teamId` is forced to its own team when omitted and must equal it when given.
2. `role` may be `viewer`/`observer` or `member`/`operator`, never `team-admin`.
3. The `admin` scope, however it is requested (`scopes`, `preset: admin`), is refused.
4. Every scope the key would hold must be one the team-admin itself holds. A preset that expands to a scope it
   lacks is refused as a whole.
5. `expiresAt` (now plus `expiresInSeconds`) may not be later than the team-admin's own expiry. For a
   signed-in member that is the end of the session.

Both kinds of signed-in member are covered by tests (not KVM) against a fixture identity provider. A session's
keys record the member's principal as `createdBy`, never the email. A cookie session may only make a key from
the control plane's own pages (a foreign or missing `Origin` is 403); a bearer token needs no `Origin`. A team-admin
member also cannot name the principal of another team's member or of an administrator member, as it cannot for keys.

A team-admin never sees, and cannot revoke, a managed key that is an administrator's, even one that names its
team, nor another team's key: both are 404 and are not listed. A key it made is a plain `member` or `viewer`:
it cannot manage keys.

### How a managed key authenticates

The policy file is checked first, exactly as before, and the legacy admin key after it. If the presented
key's SHA-256 matches neither, the control plane looks it up in the store, by that digest (one `HGET` in
Redis, no scan), **on that request, with no cache**. So:

- **A revocation takes effect on the next request, on every control plane** sharing the store. In-flight
  requests and open streams continue, as for a policy reload.
- An expired managed key is refused 401, with the same message as an expired file key.
- **A store error while checking a key is 503, never 401 and never a pass.** The key may be fine; the control
  plane cannot tell. Requests with a policy-file key or the legacy admin key do not touch the store and are
  unaffected.
- A request with no key makes no store call.
- Each such request with an unrecognised key costs the store one lookup.

### What the API cannot do

- **Policy-file keys cannot be deleted, listed or changed through the API.** They have no id here, so
  `DELETE /keys/{anything}` for one is 404, as for any id the store does not hold. Rotate them as above.
- **A key made by a team-admin belongs to the team, not to its creator.** If the creator's key is revoked,
  rotated out of the file or expires, the keys it made keep working until their own expiry or until a
  team-admin or administrator revokes them. `createdBy` records who made it.
- A key's scopes, role, team and expiry cannot be changed: revoke and make another.
- Secrets are never logged. The access log and the audit records name a managed key by its id.
  `key-created` and `key-deleted` events (`GET /cluster/events`, administrators only) carry `id=... by=...`
  and the team, and nothing else: no secret, digest or label. Webhooks and the E2B event feed ignore them.
- The API answers 403 to every caller when the deployment has no key and no sign-on, since nobody is an
  administrator then.

With the API in use, "revoke all keys" is two actions: replace the file as above, and delete managed keys
(list, then delete each id).

## Durable access history

Protected control-plane requests can also write synced, tamper-evident admission
and completion records. See [configuration, verification and failure semantics](ACCESS_AUDIT.md).

Policies also accept an optional `principal_id`, an operator-assigned stable identity label such as `team-user-17`. It must contain 1–128 ASCII letters, digits, dots, hyphens or underscores. Use the same label when replacing a person's credential digest; multiple credentials may identify that same principal. The label must identify the person or service, rather than contain a credential, and must not be reassigned to a different principal. Policies without it retain the existing team scope behavior.

Policies may also name a `team_id`, which makes the deployment multi-tenant: each key reaches only its own team's sandboxes, and every non-administrator key must then carry both `team_id` and `principal_id`. See [Teams](TEAMS.md).

Authenticated V1/V2 sandbox creation records this principal in the internal sandbox record through an authenticated control-to-node request. Client owner headers and body/metadata labels cannot assign it. Creation with a configured principal requires a nonempty cluster token and a compatible clustered node; otherwise it fails closed. Legacy admin, anonymous and unlabelled policies create ownerless records. Existing VM ownership is not rewritten when policies reload. Forks inherit the source owner, while pause/resume preserve it; native allocations are not inherited by forks. This creator attribution leaves existing team-wide sandbox routes in place. [Owner-only public-port APIs](NATIVE_PORT_GATEWAY.md) now require matching stored creator identity for reservation, listing and removal.

[Trusted creator verification](benchmarks/2026-10-03/sandbox-owner-context/README.md) passes stable identity across key rotation, forged client context refusal, legacy compatibility and authenticated-node requirements. Actual Redis/mTLS/KVM checks verify initial owner persistence, two forks with no inherited source allocation, pause/resume preservation and ownerless legacy creation. Organization identity provisioning, explicit legacy adoption and ownership transfer remain incomplete.

[Owner API rotation verification](benchmarks/2026-10-03/owner-public-port-api/README.md) uses actual policy-file SIGHUP replacement through Redis/mTLS/KVM. The old key is refused, the replacement key retains the same owner and public allocation, and removal/reexposure work with the replacement. No credential change implicitly adopts an ownerless VM or transfers an existing VM to another principal.

[Shipped CLI verification](benchmarks/2026-10-03/owner-public-port-cli/README.md) creates an owned VM through the V2 CLI command before native expose/list/remove. After actual SIGHUP rotation, the old CLI key is refused and the replacement retains the same creator/allocation. Commands take credentials from `HV2_API_KEY` and accept no owner override.

Administrator legacy adoption uses POST `/sandboxes/{id}/owner` or `hm sandbox vm adopt-owner ID --principal-id LABEL`. It requires a configured legacy administrator or an operator-role key with admin scope; sandbox scope and observer role are insufficient. It assigns only an unowned VM without legacy public-port reservations, never transfers an existing owner, and accepts same-principal retries. [Verified running/paused adoption and key rotation](benchmarks/2026-10-03/legacy-owner-adoption-workflow/README.md) retain the stable principal through fork and pause/resume.
