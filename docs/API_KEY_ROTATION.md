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
isolation or per-resource ownership.

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
