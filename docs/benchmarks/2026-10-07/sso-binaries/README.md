# Single sign-on with the shipped binaries

**What this checks:** [single sign-on](../../../SSO.md) end to end, using the release
`hv2-control-plane` with its `--sso-*` options and the release `hm` CLI. Both run
against an OpenID Connect provider over real TLS.

## Method

`tools/check-sso.py` sets up an owned CA, a fixture OpenID Connect provider and a
control plane:

- **The provider** serves HTTPS from the owned CA and implements discovery,
  JWKS, authorize (approves at once) and token. The token endpoint requires
  `client_secret_basic` and checks the PKCE verifier. It signs RS256 ID tokens
  with the `openssl` CLI, an implementation independent of the IronCrypto
  verifier in the control plane.
- **The control plane** runs on a memory store with API TLS and
  `--sso-provider-ca`. The members file lists one person in team `red`.

It then checks, through HTTPS only:

1. **No anonymous access.** With SSO on, an anonymous `GET /sandboxes` gets 401,
   although no API keys are configured.
2. **Browser sign-in.** The flow runs `/auth/login` → provider → `/auth/callback`
   and ends in a `__Host-hm_session` cookie carrying `Secure`, `HttpOnly`,
   `SameSite=Lax` and `Path=/`. `/auth/session` names the member and team, and
   `/sandboxes` admits the cookie.
3. **Origin check.** A cookie-authenticated `POST /sandboxes` with no `Origin`,
   or a sandbox page's origin, gets 403. With the control plane's own origin it
   passes authentication and fails only for lack of a node (503).
4. **Non-members.** A provider identity that is not in the members file gets 403
   at the callback, and no session.
5. **CLI sign-in.** `hm sandbox vm login --no-browser` prints a URL. The script
   acts as the browser, and the provider's redirect lands on the CLI's loopback
   listener. The CLI trades the code and its verifier for a session, stored in
   `sessions.json` with mode 0600. `hm sandbox vm list` then succeeds with no
   `HV2_API_KEY`. After `hm sandbox vm logout`, the same call gets 401.
6. **Revocation on reload.** Rewriting the members file without the member and
   sending `SIGHUP` makes the browser session get 401 on the next request.

## Result: all six pass

The full record is in [`report.json`](report.json). The control plane's log shows
the member signing in, then the non-member being refused, then the CLI sign-in,
then the members reload. It contains no errors.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-control-plane` (release, branch `feat/sso-login`) | `d69aecbb58622d8146baad987db2690051ad9977d5c7187de148ad19d2dd46da` |
| `hm` (same build) | `ee94ba5bffc9c4cd9bb128050bcb4ec8315b33a4813cb07a32c0a31f7eaa6abb` |
| `tools/check-sso.py` | `5489392dc4bc107ddb6df81f14221aeaaf66a8d06b8f51cb46cd385b755a1d9c` |

The host was WSL2 kernel 6.18.33.2 with OpenSSL 3.5.7.

## Not shown here

- A commercial provider (Google, Entra ID, Okta). Only the standard flow
  against a fixture was exercised.
- Sandboxes. No node was running. Team isolation for sessions follows from the
  same policy path API keys use, which [the two-team KVM
  check](../teams-kvm/README.md) covers.
- SSO for private guest URLs, which is not implemented.
