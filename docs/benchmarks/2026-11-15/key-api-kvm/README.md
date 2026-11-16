# Managed API keys and team members, on KVM

`tools/check-key-api-kvm.py` covers the key-management API: `POST /keys`, `GET /keys`,
`DELETE /keys/{id}` and `GET /teams/{team}/members`. It runs that API against real
processes:
- an owned Redis 8.0.2;
- one `hv2-sandboxd` node on KVM;
- two `hv2-control-plane` processes sharing that Redis, with verified TLS and node mTLS.

It uses the public HTTP API only. See [API_KEY_ROTATION.md](../../../API_KEY_ROTATION.md#managed-keys)
and [TEAMS.md](../../../TEAMS.md#members).

## Result

22 of 22 cases passed in each of three runs: [`run1`](run1/report.json),
[`run2`](run2/report.json) and [`run3`](run3/report.json). Each node log shows
`platform: Kvm` twice and `platform: Tcg` never. The host is Linux under WSL2 with
nested KVM.

The cases, in each run:
- **Creating a key:** an administrator makes a member key. The secret (`hm_…`) is shown
  once. `GET /keys` never shows a secret or a digest.
- **Using it:** the managed member key creates a sandbox that boots on KVM and runs a
  command. Inside its team it works; on the key routes it gets 403.
- **No escalation:** a team-admin gets 403 for each of these:
  - another team;
  - a principal another team's key carries;
  - the team-admin role, by role or by preset;
  - the `admin` scope, by scope or by preset;
  - a scope it does not hold, in three forms;
  - a lifetime longer than its own, in two forms.
- **Team scoping:** a team-admin lists and revokes only its own team's keys. Another
  team's key is 404, and another team's members are 403. A key it makes is forced to its
  own team.
- **Members:** `GET /teams/red/members` lists the principals of file and managed keys.
- **Two control planes:** a managed key works on both control planes over the same Redis,
  and after a control-plane restart.
- **Revocation:** after a revoke, the next request is 401 on both control planes. The
  sandbox the revoked key made is untouched.
- **Expiry:** a 60 s key works, then is 401 once it expires, and the listing marks it
  `expired`.
- **Logs:** no created secret, and no digest of one, appears in any service log.
- **Redis down:** with Redis stopped, a managed key's request is 503 and an unknown key's
  is not let through.

The reports record every request's status. The service logs are not kept, because they
hold host paths. Each report has the node log's platform lines.

## Inputs

| | sha256 |
|---|---|
| `hv2-control-plane` (release, cargo 1.100.0, `-D warnings`) | `e000f12f0b61908a640be538cb8eb5c6aa6992f8ea145c4fd805aef6f3b0acfa` |
| `hv2-sandboxd` | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |

The kernel and initrd were `bzImage` and the busybox initrd, as in
[key-presets-roles-kvm](../../2026-11-14/key-presets-roles-kvm/README.md). Their digests
are in each report.

The control plane was built from the working tree on top of `e0dc63c4`, with the key API
change applied. That includes the rule that a team-admin may not name a principal that a key
outside its team, or an administrator's key, carries. The reviewing session added that rule
after the first draft. The driver's row for it was added before these three runs. An
earlier run without that row also passed, and is not kept.

## NOT VERIFIED

- An SSO session making keys, on KVM. It is covered by tests, not KVM: `tests/sso_login.rs`
  drives a fixture OpenID Connect provider and signs in an administrator member, a
  team-admin member and a plain member. The administrator makes, lists and revokes keys (one
  outliving the session), and a team-admin is held to its team, the admin scope, scopes it
  holds, the team-admin role, the session's expiry and principals of members outside its
  team (`an_sso_administrator_makes_lists_and_revokes_keys`,
  `an_sso_team_admin_makes_keys_for_its_own_team_only`,
  `a_cookie_session_makes_keys_only_from_the_control_planes_own_pages`). A session's keys are
  recorded as created by the member's principal, not the email. Not covered: a real
  identity provider, a browser, more than one node.
- More than one node, and concurrency: two revokes racing, or creates racing past the
  1,000-key bound.
- Load on the per-request store lookup that an unknown key costs.
- Pruning of expired managed keys. There is none: an expired key stays listed until it is
  deleted.
- Durable membership apart from keys. A member is its keys.
