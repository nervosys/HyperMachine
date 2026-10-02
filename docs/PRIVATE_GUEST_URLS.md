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
behind the configured proxy. It does not implement SSO/OIDC user login,
self-service sharing management, account self-service, a logout portal, verified email claims
or guest/proxy access auditing. Private mode is opt-in; other proxy instances
must be configured consistently. No authentication-throughput improvement or
managed competitor performance win is implied.

[Verified functional evidence](benchmarks/2026-10-02/private-web/README.md) includes real KVM/TLS/mTLS checks and Linux/Windows HTTP integration tests.

[Sandbox scope evidence](benchmarks/2026-10-02/private-web-scopes/README.md) verifies exact-ID sharing, fork exclusion, domain rebinding and revocation with real KVM.
