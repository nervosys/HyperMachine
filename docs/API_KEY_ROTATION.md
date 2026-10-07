# Rotate scoped control-plane keys

On Unix, a control plane started with `--api-keys-file PATH` reloads that same file when its process receives `SIGHUP`. Write a complete replacement JSON array to a temporary file, atomically replace the configured file, then send `kill -HUP PID` to the control-plane process you operate. Observe its `API key policies reloaded` message. Check the new credential and verify the revoked credential is refused through the API. Signal every control-plane replica; replacements are local to each process.

Policy documents are limited to 1 MiB and must be UTF-8. Startup and reload read at most the limit plus one byte; embedded JSON replacement applies the same limit before parsing. Oversized and invalid UTF-8 files are refused with errors that omit file contents. The full replacement is parsed and validated before it becomes active. Invalid JSON, empty arrays, duplicate credentials, invalid scopes and collisions with the legacy admin credential are rejected; active policies remain unchanged. A read or task error also preserves active policies. Error messages omit credential values and policy contents. Expiry remains checked on every request. Existing authorized requests and open streams continue; revocation applies to subsequent authorization decisions.

This replaces only scoped policies. The legacy `HV2_API_KEY` admin credential remains unchanged and requires restarting the process to rotate. An empty policy list is refused, so a reload cannot accidentally open an unauthenticated API. To revoke all scoped credentials, replace them with a valid policy whose expiry has already passed. Newly created policy files and their replacement must remain readable by the service account.

On other platforms, signal reload is unavailable; restart the binary with the replacement file. Embedded users can call `ControlPlane::replace_api_key_policies` to apply the same validated atomic replacement. Unix signal handling is installed only when `--api-keys-file` is configured; do not send SIGHUP to an instance without that option.

A real HTTP integration test checks old-key revocation, replacement-key access, unchanged admin access, scope enforcement and rejected updates preserving active policies. [Process-level SIGHUP verification](benchmarks/2026-10-01/api-key-reload.md) passed 16 real HTTP checks using an owned Unix control-plane process. The feature provides operator rotation within the existing single team; it does not add tenant roles, central policy distribution or interruption of already-authorized work.

[Bounded-reader process verification](benchmarks/2026-10-01/api-key-bounds.md) passed 20 HTTP checks, including oversized and invalid UTF-8 reloads preserving active policies and subsequent valid recovery. Startup rejection uses the same reader and has library coverage; these process checks cover reload.

## Single-team observer and operator roles

Policies accept an optional `role`: `operator` (the default for existing policies)
or `observer`. Unknown values and explicit null are rejected by startup and
atomic replacement. Roles apply to the configured single team, without tenant
isolation. Public-port management separately requires matching creator ownership.

An observer can only GET/HEAD `/sandboxes`, `/v2/sandboxes`, `/templates`,
`/sandboxes/metrics` and `/cluster/nodes`, and only where its scopes also allow
the route. The role caps even `admin` scope. All other protected routes are
denied, including sandbox detail/name lookup, volume credentials, upload URLs,
execution and GET TCP upgrades. Sandbox listings omit the guest access token.
Operators retain the existing scope behavior. For an inventory observer, add
`"role":"observer"` to a policy with `"scopes":["inventory"]`.

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
