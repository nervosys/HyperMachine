# Operator-protected guest application URLs

`hv2-control-plane --web-access-file /secure/operator/web-access.json` enables
HTTP Basic browser login for every guest application URL served by that control
plane, including bound custom domains and `{port}-{sandboxID}` URLs. The proxy
authenticates before opening a backend route or waking an automatically resumed
guest. Missing, incorrect or expired credentials receive a 401 browser challenge;
gRPC application requests receive status 16. Rejections are not cached.
The browser challenge uses [HTTP Basic authentication](https://www.rfc-editor.org/info/rfc7617/).

The web credential is separate from API administrator keys and sandbox envd
tokens. An authenticated guest receives `X-HyperMachine-User` from the operator's
configured subject. The proxy replaces client-supplied values and strips the
Basic login `Authorization` header before forwarding. Guest responses receive
`Cache-Control: private, no-store`, overriding public-cache settings from the app.
These directives follow [HTTP caching semantics](https://www.rfc-editor.org/info/rfc9111/).
An application using this mode delegates that HTTP Authorization header to the
gateway; it cannot simultaneously use the same header for its own login.

The policy is a nonempty JSON array of at most 256 entries:

```json
[
  {
    "subject": "operator@example.test",
    "sha256": "REPLACE_WITH_THE_64_HEX_DIGIT_CREDENTIAL_DIGEST",
    "expires_at": 1791072000,
    "sandboxes": ["sandbox-IDENTIFIER"]
  }
]
```

Subjects are opaque operator-provisioned identities, not externally verified
email addresses. They must be unique and contain 1–128 ASCII letters, digits or
`._@+-`. Digests and credentials must be unique. Expiry is Unix time in seconds
and is checked on every new request. Unknown fields and malformed policies are
refused. `sandboxes` is a per-user list of at most 256 exact, case-sensitive
sandbox IDs, each 1�128 ASCII letters, digits, `-` or `_`. It grants application
URL access to those IDs across ports and custom domains. Duplicate IDs, `null`,
malformed IDs and a wildcard mixed with IDs are rejected. `[]` grants no sandbox
access; `["*"]` grants all current and future sandbox IDs. Omitting the field
retains the original all-sandbox behavior, so use an explicit list when sharing
a subset. Names, domains and prefixes are not accepted as substitutes for IDs.

Authorization checks the resolved target ID, including explicit routing headers.
A fork receives a new ID and is not granted by its parent's exact-ID scope.
Rebinding a custom domain changes the ID evaluated for that request. Scope and
credential replacement are atomic: omitted grants revoke new requests, while
already admitted streams keep their original admission. Scope refusal uses the
same 401 challenge as incorrect credentials and occurs before guest wakeup.
These grants authorize web application access only; they do not grant API,
exec, SSH, envd or lifecycle access. Policy administration remains operator-managed.

Generate a separate random credential with at least 32 random bytes;
this SHA-256 policy is intended for high-entropy credentials, not chosen passwords.
For example, after creating a protected directory outside the repository:

```sh
umask 077
python3 - <<'PY'
import hashlib, json, secrets, time
from pathlib import Path
credential = secrets.token_urlsafe(32)
subject = "operator@example.test"
Path("/secure/operator/web-login.txt").write_text(subject + "\n" + credential + "\n")
Path("/secure/operator/web-access.json").write_text(json.dumps([{
    "subject": subject,
    "sha256": hashlib.sha256(credential.encode()).hexdigest(),
    "expires_at": int(time.time()) + 8 * 3600,
    "sandboxes": ["*"]
}]) + "\n")
PY
```

Distribute the login credential through your existing secure channel. In the
browser's login prompt, use the subject as username and the credential as password.
Keep both files private. The server policy holds only the digest. Do not place
plaintext credentials in URLs, command arguments, logs or source control.

This mode requires both public proxy TLS and node mTLS. Startup refuses a missing
transport configuration instead of exposing a plaintext login. Configure the
nodes to require their cluster client certificates and restrict direct node
access; an unprotected alternate proxy can bypass a control-plane-only policy.
Use the wildcard/custom-domain certificate appropriate to your public URL:

```sh
hv2-control-plane --store redis://127.0.0.1:6379 \
  --api-keys-file /secure/operator/api-keys.json \
  --web-access-file /secure/operator/web-access.json \
  --tls-cert /secure/public-chain.pem --tls-key /secure/public.key \
  --api-tls-cert /secure/api-chain.pem --api-tls-key /secure/api.key \
  --mtls-ca /secure/node-ca.pem --mtls-cert /secure/control.pem \
  --mtls-key /secure/control.key --mtls-node-name hm-node.local
```

Existing envd SDK traffic on port 49983 retains its own per-sandbox access-token
authorization; it does not require the web credential. The identity header is
removed from those requests. API authorization and API key policies remain
separate from browser login.

On Unix, replace the policy file atomically and send SIGHUP to the control-plane
PID to reload. A valid replacement revokes omitted credentials for new requests;
an invalid or empty replacement preserves the active policy. To deny all new
logins, retain nonempty entries with `"sandboxes": []` or expired timestamps. Existing authorized
streams retain their original admission. On Windows, restart with the new policy.
Browsers may cache Basic credentials; revocation or expiry is the server-side
way to deny subsequent requests.

This is operator-provisioned access to selected or all guest application URLs
behind the configured proxy. It does not implement account self-service, a
logout portal or guest/proxy access auditing. For sign-in through an OpenID
Connect provider, with the verified email passed to the guest, see
[single sign-on for guest URLs](SSO.md#private-guest-urls), which can run
alongside this. Private mode is opt-in; other proxy instances
must be configured consistently. No authentication-throughput improvement or
managed competitor performance win is implied.

[Verified functional evidence](benchmarks/2026-10-02/private-web/README.md) includes real KVM/TLS/mTLS checks and Linux/Windows HTTP integration tests.

[Sandbox scope evidence](benchmarks/2026-10-02/private-web-scopes/README.md) verifies exact-ID sharing, fork exclusion, domain rebinding and revocation with real KVM.

## Owner sharing management API

Authenticated owners can read `GET /sandboxes/{id}/web-sharing` and replace the stored grant list with `PUT` on the same route. The browser proxy consults stored grants only for credentials explicitly provisioned with `allow_owner_grants: true`. Operator sandbox scopes continue to authorize independently; revoking a stored grant does not revoke an operator-file scope.

The API key requires the `sandboxes` scope (or operator `admin`) and a configured `principal_id` matching the sandbox owner. Observer keys, legacy administrator keys without a creator identity, unassigned identities and ownerless sandboxes cannot use these routes. Client owner headers and JSON fields do not establish ownership.

GET returns `{"revision":null,"grants":[]}` when no sharing row exists. To replace grants, send the revision read from GET as `expectedRevision`, a newly generated canonical UUIDv4 as `revision`, and the entire desired grant list:

```json
{
  "expectedRevision": null,
  "revision": "12345678-1234-4234-8234-123456789abc",
  "grants": [{"subject": "alice", "expires_at": 1893456000}]
}
```

Subjects are exact opaque provisioned credential identities, not verified email claims. Each requires 1–128 ASCII letters, digits or `._@+-`; wildcards are refused. Expiry is a positive Unix timestamp in seconds. Up to 256 unique subjects fit within a 64 KiB request limit. Unknown fields and malformed grants are rejected. An empty `grants` list revokes the stored grants while retaining its revision.

Retry an uncertain PUT with the identical expected revision, new revision and grants. Exact retries succeed; changed contents at the same revision or stale revisions return 409. Owner conflicts return 403, missing sandboxes 404, malformed input 400/422, oversized requests 413 and store errors/timeouts 503. Store calls are bounded to five seconds each; a timed-out update may already have committed. Read current state before preparing a different replacement.

Memory and Redis retain sharing revisions after sandbox deletion. Admission snapshots deny missing records and changed sandbox instances. If an ID is reused, its owner must read the retained revision to replace state. [Local Redis AOF-always hard-restart verification](benchmarks/2026-10-04/web-sharing-aof-restart/README.md) preserves active grants and revocation revisions across five process restarts. [Ten real KVM owner-sharing cases](benchmarks/2026-10-05/owner-sharing-kvm/README.md) also verify the shipped control-plane restart, owner API/CLI, fork exclusion and revocation without waking a paused guest. [Thirteen live KVM/outage cases](benchmarks/2026-10-05/owner-sharing-redis-outage/README.md) also verify paused no-wake denial and AOF grant/revocation recovery during two hard Redis restarts. Retention cleanup, hardware power loss, managed failover and fleet-scale outage recovery remain open.

## Browser credentials eligible for owner grants

An operator must explicitly enable delegation on a provisioned browser credential. For access controlled entirely by sandbox owners, use an empty operator scope list:

```json
{
  "subject": "alice",
  "sha256": "<SHA-256 hex digest of the browser password>",
  "expires_at": 1893456000,
  "sandboxes": [],
  "allow_owner_grants": true
}
```

The policy document is an array of credential objects. `allow_owner_grants` defaults to false. Existing entries with `sandboxes: []` continue to deny all application access unless delegation is explicitly enabled. Omitting `sandboxes` still defaults to the operator wildcard scope, so owners cannot revoke that independently authorized access.

Admission first tries the existing operator scope, then checks an explicitly delegable credential against an atomic current sandbox/grant snapshot. Owner, exact sandbox instance, subject and grant expiry must match. Missing/invalid state, unavailable stores and five-second lookup timeouts deny admission. Credentials are reauthenticated after the awaited lookup to observe rotation, expiry and delegation changes. Authorization precedes backend open/guest wakeup; forged identity headers are removed and successful requests carry the provisioned subject while stripping Basic credentials.

Revocation applies to subsequent requests. Existing admitted streams retain their original admission. Envd port 49983 continues to use its per-sandbox access token and does not receive the browser identity header. Private mode remains opt-in through `--web-access-file`; every public proxy instance must enable it consistently.

## Owner CLI workflow

Use an API key assigned to the sandbox owner through the existing CLI environment or key configuration. Read the current state with `hm sandbox vm web-sharing show <sandbox-id>`. Save the complete desired PUT body in a local JSON file, including `expectedRevision` from that read and a fresh canonical UUIDv4 `revision`, then run:

```sh
hm sandbox vm web-sharing replace <sandbox-id> --request sharing-request.json
```

Keep the file unchanged for an exact retry after an uncertain response. The CLI bounds request files and responses to 64 KiB, validates grant subjects/expiry/uniqueness and revisions before sending, and does not accept an owner override. It does not automatically merge grants or retry a different revision.

To revoke all stored grants, supply the current and new revisions explicitly:

```sh
hm sandbox vm web-sharing revoke <sandbox-id> --expected-revision <current-uuid> --revision <new-uuid>
```

Omit `--expected-revision` only when the sharing row has never existed. Operator-file scopes still authorize independently. Server error bodies are not echoed by these bounded request commands.

## Scoped Redis persistence verification

An owned Redis 8.0.2 fixture with `appendonly yes` and `appendfsync always` passes five hard process restarts. Active grants and exact sandbox-instance timestamps persist; revocation prevents stale replay, deleted records deny access while retaining revisions, recreated instances require the retained revision, and owner changes invalidate former-owner grants. Async admission checks the recovered store. Malformed sharing JSON and incorrect Redis key types fail closed.

A killed Redis connection may fail its first read while reconnecting. The verification retries only that read with a deadline; it does not automatically replay updates. Applications should preserve the exact request revision/payload to recover an uncertain mutation. Store-unavailable admission remains denied.

This verifies that exact local AOF configuration. It does not establish host power-loss guarantees, replication/failover, different persistence settings or real KVM sharing during a Redis outage. The shipped control-plane restart is verified separately by the real KVM owner-sharing gate. The [archive](benchmarks/2026-10-04/web-sharing-aof-restart/README.md) retains the first broken-pipe test failure, corrected source, final Redis load logs and independent cleanup checks.

## Real KVM owner-sharing verification

The [current owner-sharing gate](benchmarks/2026-10-05/owner-sharing-kvm/README.md) passes ten cases using the isolated release control plane and CLI, accepted default-MMIO daemon/kernel/guest, verified API/proxy TLS and node mTLS. Owner CLI replacement/retry/read, wrong-owner and legacy-key refusal, guest identity substitution, credential stripping, custom domains, fork exclusion, revocation and expiry all pass. A revoked request leaves the real guest paused; a current regrant allows auto-resume.

Two actual control-plane process restarts preserve active grants and revocation while the owned Redis remains available. Cleanup deletes parent and fork to an empty inventory, stops all five processes and removes private fixture files. The initial strict-CA fixture failure is retained, and certificate verification remains enabled after adding the required CA key usage.

This verifies that local configuration. The separately verified live Redis outage gate extends this configuration; hardware power loss, managed failover, fleet-scale outage recovery, arbitrary kernels/nodes, SSO, verified email and tenant/team isolation remain open. It establishes no performance improvement over competing products.

## Live Redis outage verification

The [live KVM Redis outage gate](benchmarks/2026-10-05/owner-sharing-redis-outage/README.md) passes thirteen cases with unchanged frozen binaries. With a current grant and paused real guest, killing Redis makes browser admission return 401. An independently authenticated node mTLS detail request confirms the guest remains paused after each denied request. One read fails on the closed connection; another reaches the five-second admission timeout.

Redis restarts from its AOF-always data directory. Active state survives and allowed traffic auto-resumes the guest. A second hard Redis restart preserves revocation: denied traffic leaves the guest paused, stale replay returns 409, and a current revision regrant permits resume. The fixture deletes both guests, empties inventory, stops seven processes and removes private files. It does not measure authentication throughput or establish managed/fleet-scale failover and power-loss guarantees.

Reproduce using `tools/verify-owner-sharing-kvm.py --redis-outage` with the accepted input paths documented in the preceding KVM archive and a new output directory.


Creator-bound control-plane API admission now restricts non-administrator scoped keys with a configured `principal_id` to records owned by that principal. Sandbox-ID routes check the authoritative store with a five-second admission deadline; missing, malformed, failed, and timed-out lookups do not dispatch. Both inventory APIs filter ownership before pagination. Detail, node forwarding, port tunnels, and named lookup also check the record they load. An `admin` scope on an operator policy continues to confer administrator access; legacy, anonymous, and scoped keys without a principal retain their existing global behavior. Owner-only sharing and reservation endpoints retain their separate rules.

This is a creator-bound API boundary, not complete tenant isolation. Shared resource namespaces and templates remain global, and an ownership change does not cancel already admitted operations or streams. Node-side transactional fences for owner transfer remain unimplemented. The change has HTTP integration coverage for filtered listings, cross-owner and ownerless rejection, denied-delete preservation, and administrator compatibility; a rebuilt release passed the [owned KVM gate](benchmarks/2026-10-05/creator-bound-api-kvm/report.json): 14 functional cases, including cross-owner API rejection, filtered inventory, administrator access, guest sharing, control-plane restarts, and live Redis outage recovery. All seven owned processes stopped and the administrator inventory was empty. The initial execute-permission failure is retained in the archive.
