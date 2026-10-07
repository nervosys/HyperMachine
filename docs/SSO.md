# Single sign-on

The control plane can sign people in through any OpenID Connect provider:
Google, Microsoft Entra ID, Okta, Keycloak, Auth0, and others. A signed-in
member gets a session that the API accepts in place of an `X-API-Key`. The
session carries the member's scopes, role, principal and [team](TEAMS.md),
exactly as a key would.

## Configuring it

Register the control plane with your provider as a web application. Use
`https://<control-plane>/auth/callback` as its redirect URI, and the
`openid email profile` scopes. Then:

```sh
hv2-control-plane ... \
  --sso-issuer https://accounts.example.com \
  --sso-client-id hypermachine \
  --sso-client-secret-file /run/secrets/sso-client-secret \
  --sso-redirect-url https://cp.example.com/auth/callback \
  --sso-members-file /etc/hypermachine/members.json \
  --sso-session-key-file /run/secrets/sso-session-key
```

| Option | |
|---|---|
| `--sso-issuer` | The provider's issuer URL. It must match the `issuer` in the provider's discovery document exactly, and must be HTTPS (loopback is exempt). The control plane fetches discovery and keys at startup, and refuses to start if they are wrong. |
| `--sso-client-id` | The client ID the provider issued. |
| `--sso-client-secret-file` | The client secret, sent with HTTP Basic (`client_secret_basic`). Omit it for a public client, which PKCE alone protects. |
| `--sso-redirect-url` | This control plane's callback, as registered. It must be HTTPS, because session cookies are `Secure`. Its origin is also what cookie-authenticated changes must come from. |
| `--sso-members-file` | Who may sign in (below). Reloaded on `SIGHUP`. |
| `--sso-session-key-file` | At least 32 random bytes that sign sessions, for example `head -c 32 /dev/urandom`. Replacing it ends every session. Control planes behind one name must share it. |
| `--sso-provider-ca` | Extra PEM roots for the provider's TLS, for a private provider. |
| `--sso-session-hours` | How long a session lasts: 1–168, default 12. |

## Members

Only listed, verified emails may sign in. Each entry is shaped like an [API key
policy](API_KEY_ROTATION.md), keyed by email instead of a key digest:

```json
[
  {"email": "alice@example.com", "scopes": ["sandboxes", "inventory", "volumes"],
   "principal_id": "alice", "team_id": "red"},
  {"email": "ops@example.com", "scopes": ["admin"], "principal_id": "ops"}
]
```

- Emails are matched ignoring case. The provider must assert
  `email_verified: true`, because an unverified address is a claim anyone can make.
- Once any member names a `team_id`, every member who is not an administrator
  must name one too, as with keys.
- A member removed from the file loses access on their next request, even with
  a session that has not yet expired.

## Signing in

- **Browser:** visit `/auth/login?returnTo=/ui`. The browser goes to the provider
  and back to `/auth/callback`, which sets the session cookie and returns to the
  page asked for. `GET /auth/session` says who you are; `POST /auth/logout` clears
  the cookie.
- **Scripts:** send the session token as `Authorization: Bearer hms1.…`.

## What it protects against

- **Login state.** The `state`, nonce and PKCE verifier travel in a short-lived
  cookie the control plane signs, so nothing is stored server-side. A callback
  whose `state` or cookie does not match is refused.
- **ID tokens.** They are verified with RS256 or ES256 against the provider's
  published keys, which are refetched when a token names a key not seen before.
  The check covers exact issuer, audience (and `azp`), expiry, issue time and
  nonce. `none`, `HS256` and every other algorithm are refused. See
  [`crates/hv2-cluster/src/sso.rs`](../crates/hv2-cluster/src/sso.rs).
- **The session cookie.** It is `__Host-hm_session`: `Secure`, `HttpOnly`,
  `SameSite=Lax`, host-only, so sandbox hosts never receive it.
- **Cross-site changes.** A request authenticated by the cookie that changes
  anything must carry an `Origin` matching the redirect URL's. Sandboxes serve
  guest pages on the same site, and `SameSite=Lax` alone would still send the
  cookie with their POSTs. Bearer tokens are not affected.
- **No anonymous access.** With SSO on, a request with neither a session nor a
  key gets 401, even when no API keys are configured.

## Not yet

- **`hm login` for the CLI.** Until then, scripts use an API key.
- **Private guest URLs (`{port}-{sandbox}.domain`).** These still use their own
  Basic credentials. Sharing the session with them needs the proxy to strip the
  cookie before it reaches the guest.
- **Revoking one session before it expires.** Remove the member instead, or
  rotate the session key.
- **SAML.**
