# Custom domains

The cluster control plane binds an operator-managed DNS hostname to a sandbox
and guest HTTP port. Requests keep the public hostname, path, query and body
while the proxy sends routing headers to the owning node. HTTP/1.1 and HTTP/2
use the same binding. Existing explicit sandbox routing headers take precedence.

Configure DNS to point at the control-plane proxy, and supply a certificate
whose SANs cover your custom names. The certificate for `*.sandbox.example.com`
does not cover `app.example.com`. Certificate issuance and ACME renewal remain
operator responsibilities. The optional DNS policy below verifies ownership
before accepting a new binding or changing its port.

Start the control plane with its existing TLS options:

```sh
hv2-control-plane --store redis://127.0.0.1:6379 --namespace production \
  --port 5980 --proxy-port 443 --tls-cert /etc/hypermachine/cert.pem \
  --tls-key /etc/hypermachine/key.pem
```

Set `HV2_API_KEY` and `HV2_CLUSTER_TOKEN` in the environment and configure nodes
with the same cluster store, namespace and token. Management API TLS can be
provided by the existing API ingress; the TLS options above secure the workload
proxy. All control planes and nodes need the current domain-aware store code
so sandbox deletion releases its bindings.

Start an HTTP service inside the sandbox on the chosen port, then bind it:

```sh
export HV2_SANDBOX_URL=https://sandbox-api.example.com
hm sandbox vm domain bind SANDBOX_ID app.example.com --port 8080
hm sandbox vm domain list SANDBOX_ID
curl https://app.example.com/
hm sandbox vm domain unbind SANDBOX_ID app.example.com
```

The CLI reads `HV2_API_KEY` and prints JSON. `domain bind` also updates the port
when the same sandbox already owns that name, without restarting the proxy.
The port is the guest service's port, independent of the public HTTPS port.

| Management request | Result |
|---|---|
| `PUT /sandboxes/{id}/domains/{hostname}` with `{"port":8080}` | `200` and `{"domain":"app.example.com","sandbox_id":"...","port":8080}` |
| `GET /sandboxes/{id}/domains` | `200` and an array of bindings sorted by hostname |
| `DELETE /sandboxes/{id}/domains/{hostname}` | `204`; `404` if that sandbox does not own the name |

These routes require the existing admin or `sandboxes` API-key scope when key
authentication is configured. Inventory keys cannot manage or list bindings.
Claims require an existing sandbox (`404` otherwise). A hostname already owned
by another sandbox returns `409`. Names are canonicalized to lowercase with
an optional final DNS dot removed. Use ASCII DNS names, including punycode for
IDNs; IP addresses, authorities, malformed labels, canonical sandbox-route
names and port zero are rejected.

Hostname ownership is exclusive within the configured cluster namespace.
It refers to the sandbox identity, not a separate user or team identity; the
current control plane does not implement tenant or team boundaries. Memory
storage keeps bindings for that process's lifetime. Redis stores them across
control-plane restarts. Pausing and resuming preserve the binding; an alias
request wakes a paused sandbox when it was created with
`"autoResume":{"enabled":true}`. Forks have new identities and need their own
names. Unbinding, sandbox deletion and reaping remove the relevant bindings.
Existing in-flight requests may finish after unbinding.

## Require DNS ownership proof

Start every control-plane replica with `--domain-verification-file /etc/hypermachine/dns-policy.json`
to require a sandbox-specific DNS TXT record before binding or updating a domain.
The policy file contains:

```json
{
  "namespace": "production",
  "resolver_url": "https://cloudflare-dns.com/dns-query",
  "secret_hex": "<64 hex characters from 32 cryptographically random bytes>"
}
```

Protect the file as an operator credential. Its namespace must match
`--namespace`; all replicas in that namespace need the same signing key and
policy. Generate the key from 32 random bytes, for example with `openssl rand
-hex 32`. The optional `resolver_ca_file` names a PEM CA certificate for an
operator's private HTTPS resolver. Certificate verification stays enabled.
Resolver URLs cannot contain credentials, query parameters or fragments.

The resolver must support the [Cloudflare/Google DNS JSON schema](https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-https/make-api-requests/dns-json/).
The control plane trusts that resolver's answer over HTTPS; this does not
establish independent DNSSEC validation. TXT answers must name the exact
challenge owner and contain its literal value in one quoted TXT string.
CNAME indirection and split TXT strings are not accepted.

```sh
hm sandbox vm domain challenge SANDBOX_ID app.example.com
```

Publish the returned `record_value` as a TXT record at `record_name`, normally
`_hypermachine-domain.app.example.com`. Then bind with the returned Unix expiry:

```sh
hm sandbox vm domain bind SANDBOX_ID app.example.com --port 8080 \
  --challenge-expires-at EXPIRES_AT
```

Challenges last ten minutes and bind the hostname, sandbox identity, cluster
namespace and signing key. They can be verified by another replica or after a
restart using the same policy. A proof for one sandbox cannot claim a replacement
or fork. Port updates require a fresh valid verification too. Deletion and
unbinding do not require DNS and remain available if the resolver is down.

| Request or condition | Result with the policy enabled |
|---|---|
| `GET /sandboxes/{id}/domains/{hostname}/challenge` | TXT instructions and `expires_at`; existing sandbox and normal API authorization required |
| `PUT` body missing `challenge_expires_at` | `428`, binding unchanged |
| Expired challenge or missing/mismatched TXT | `403`, binding unchanged |
| Resolver failure, timeout, oversized response or exhausted query capacity | `503`, binding unchanged; retry after resolving the failure |
| Exact current TXT proof | Normal atomic domain claim or port update; existing ownership conflicts still return `409` |

Queries have a five-second deadline, a 64 KiB response limit, no redirects and
at most sixteen concurrent requests per control-plane instance. DNS queries use
a separate client without node or API credentials. The policy document is
limited to 4096 bytes; errors and Debug output exclude its signing key.

Without this flag, existing operator-managed binding behavior remains available.
Enabling it gates future claims and updates; it does not revalidate or remove
existing bindings. Proof expiry or removing the TXT record does not unbind a
domain. Resolver caches can retain TXT answers until their TTL expires. Use the
normal unbind/delete operation to revoke routing. Rotating the signing key requires
a coordinated restart and new challenges, while persisted bindings remain intact.
This feature verifies a domain claim, not its address records or certificate.

## Reload certificate bundles

The control-plane workload proxy accepts an operator-managed certificate bundle
with `--tls-bundle-file /etc/hypermachine/tls-bundle.json`. Use this instead of
`--tls-cert`/`--tls-key`. API listener TLS remains separately configured with
`--api-tls-cert`/`--api-tls-key`.

```json
{
  "default": {
    "cert_path": "/etc/hypermachine/certs/wildcard/fullchain.pem",
    "key_path": "/etc/hypermachine/certs/wildcard/privkey.pem"
  },
  "certificates": [
    {
      "names": ["app.example.com"],
      "cert_path": "/etc/hypermachine/certs/app/fullchain.pem",
      "key_path": "/etc/hypermachine/certs/app/privkey.pem"
    }
  ]
}
```

Certificate and key paths must be absolute. Exact SNI names are canonicalized
to lowercase, with an optional final dot removed; duplicate names are refused.
Each named certificate must cover all of its configured names. A wildcard
certificate can cover an exact registered name. The optional `default` supplies
the certificate for other names or clients without SNI, as the original single
certificate did. Without a default, unconfigured SNI handshakes are refused.

On Unix, atomically publish the manifest and send `SIGHUP` to the control-plane
process. A certificate deploy hook can do this after writing a complete new
certificate/key generation. Immutable generation directories are convenient;
certificate-client symlinks to regular files are also supported. A mixed pair
encountered during replacement is rejected rather than partially activated.

```sh
kill -HUP CONTROL_PLANE_PID
```

The complete proposed bundle is parsed and checked before its resolver replaces
the active generation. Invalid PEM/DER, missing files, key mismatch, invalid
names, duplicate names, expired/not-yet-valid certificates and oversized inputs
leave the active generation intact. Every chain has at most sixteen certificates.
The manifest is limited to 1 MiB, individual certificate PEM files to 1 MiB,
private key files to 64 KiB, total PEM inputs and mapped certificate-chain bytes
to 8 MiB each, named pairs to 128 and exact names to 1024. Certificate dates are
checked when loading/reloading; clients still validate trust and hostname.

New TLS connections use the active generation; existing connections continue.
Server session resumption is disabled in bundle mode so a new connection cannot
silently retain a prior certificate. HTTP/1.1 and HTTP/2 use the same resolver.
`SIGHUP` also reloads configured API-key and browser-access policy files, each
independently; a rejected policy or certificate update retains its prior state.
On hosts without Unix signals, restart the process to load a changed manifest.

## Deploy certificates from Certbot

On Linux, `tools/deploy-tls-certificate.py` connects an operator-configured Certbot
lineage to a running bundle. It updates exactly one existing hostname group; it
does not add domain bindings or change the optional default certificate. Configure
all names in that group with repeated `--domain` flags, up to 128 exact names.
Use absolute paths and run as the manifest owner. The generation directory must
already exist with mode `0700`; the manifest must have one hard link and must not
be writable by group or others. All manifest writers must honor its sibling
`.deploy.lock`. Generation files have mode `0600` and are never overwritten.

First obtain a certificate with Certbot using your configured authenticator.
For HTTP-01, the public hostname must resolve to an operator web server serving
the webroot challenge directory on port 80. This is separate from the workload
TLS proxy. Copy the initial full chain and key into a private generation, configure
the bundle to reference those copies, and start the control plane. Keep the bundle
on copied generations; referencing Certbot's changing `live` symlinks can make its
disk state diverge from the still-active certificate during renewal.

An operator-owned executable wrapper can serve as the per-lineage deploy hook:

```sh
#!/bin/sh
set -eu
exec /usr/bin/python3 /opt/hypermachine/tools/deploy-tls-certificate.py \
  --manifest /etc/hypermachine/tls-bundle.json \
  --lineage /etc/letsencrypt/live/app \
  --generations /etc/hypermachine/certificate-generations \
  --control-plane /opt/hypermachine/bin/hv2-control-plane \
  --pid "$(/usr/bin/systemctl show --property MainPID --value hypermachine-control-plane.service)" \
  --domain app.example.com --port 443 --from-certbot
```

Pass this wrapper to `certbot renew --cert-name app --deploy-hook
/etc/hypermachine/deploy-app`. Certbot invokes a deploy hook after successful
issuance/renewal and supplies `RENEWED_LINEAGE` and `RENEWED_DOMAINS`; the hook
checks them against its explicit configuration. The [built-in renewal worker](TLS_RENEWAL_WORKER.md)
can instead schedule checks and reconcile deployment itself. Alternatively,
schedule the wrapper command with your existing scheduler.
[Certbot's documentation](https://eff-certbot.readthedocs.io/en/stable/using.html)
also explains dry runs and `--run-deploy-hooks`, which deploys the active
certificate rather than the temporary test certificate. Check hook failure logs:
a Certbot exit status alone does not prove successful HyperMachine activation.

The hook locks the manifest, validates certificate/key matching and hostname
coverage, copies a complete private generation, atomically publishes the updated
manifest, and sends `SIGHUP` through a Linux pidfd. The process executable and
`--tls-bundle-file` argument must match the configured binary and manifest. Fresh
TLS connections must present the expected new leaf with normal chain, hostname
and time validation before deployment succeeds. System CA trust is the default;
`--ca-file` supports an operator's private CA. `--connect-address` selects an
explicit IP, defaulting to loopback; TLS still uses each configured DNS name.

The previous leaf is pinned to the operator-owned manifest, so an already-expired
certificate can recover. Failed activation restores the exact prior manifest and
checks the prior leaf by that pin. Restoring an expired previous leaf restores
its earlier state, not valid HTTPS service. A private checksummed deployment
journal now reconciles interruption after publication or activation; unrelated
manifest updates, changed generations and unconfirmed rollback still require
operator reconciliation. The hook retains old and rejected
generations for recovery; remove unused generations under operator maintenance
only after checking every manifest reference and default. Retrying an already
active private generation verifies TLS without rewriting it or creating a copy.

The [integrated ACME deployment fixture](benchmarks/2026-10-03/acme-deployment/README.md)
passes thirteen issuer/deployment checks and fourteen real KVM checks, including
real HTTP-01 renewal while an existing 16 MiB guest download completes. It also
checks failed challenges, rejected keys, untrusted-certificate rollback, process
and manifest identity, concurrent deployment refusal and expired-certificate
recovery. The [earlier issuer-only check](benchmarks/2026-10-03/acme-issuance/README.md)
remains separately preserved. Public CA issuance, public DNS propagation,
automatic provisioning after a domain claim remain unverified or absent.
The later [renewal worker fixture](benchmarks/2026-10-03/tls-renewal-worker/README.md)
verifies built-in scheduling and crash recovery through real ACME-to-KVM traffic.
These results establish no performance win.

For local functional verification with an actual KVM guest:

```sh
python3 tools/verify-custom-domains.py \
  --control-plane target/release/hv2-control-plane \
  --daemon target/release/hv2-sandboxd --kernel /path/to/bzImage \
  --initrd /path/to/guest.cpio.gz --output custom-domains.json
```

Add `--dns-ownership` to exercise the actual control-plane binary against an
owned certificate-verified HTTPS DNS JSON fixture and real KVM guests. This
checks proof refusal, binding, unchanged routing after failed revalidation,
restart, auto-resume, deletion and refusal of a prior sandbox's proof for its
replacement. It does not query or validate a managed public DNS service.

Add `--tls-bundle` to check certificate renewal on the running proxy. The fixture
keeps a real guest download open while a new certificate is activated, verifies
the new leaf on fresh connections, and confirms that a mismatched key reload
retains the active certificate and guest route. With no default certificate,
unconfigured SNI names must fail their handshake. Both options can be combined.

Add `--deploy-hook /absolute/path/to/tools/deploy-tls-certificate.py` to exercise
the actual deployment hook with generated fixture certificates. The ACME checker
can drive this verifier with a real owned CA; see the integrated fixture's command.

This requires Linux KVM, `redis-server`, `openssl`, and a guest image containing
BusyBox `httpd`. It owns isolated services and guest resources, verifies TLS
hostname identity through a local connection, checks port updates, control-plane
restart, auto-resume, removal and name reuse, and reports cleanup and artifact
hashes. It measures functional behavior; it does not establish a performance win.
The [recorded local result](benchmarks/2026-10-01/custom-domains.json) passed all
six checks with clean teardown. The initial verifier run is retained separately;
it stopped at an incorrect expected pause status before exercising auto-resume.

### Provision a first certificate group

For a new non-overlapping hostname group, add `--provision-new-group` to the
existing `deploy-tls-certificate.py` invocation. The lineage must already contain
a valid issued certificate covering every `--domain`. The bundle must contain
an existing default certificate for pinned rollback. The hook preserves the
default and all unrelated groups, publishes an immutable named generation,
and verifies new TLS connections before reporting success. Continue passing
this flag when recovering an interrupted first deployment; ordinary renewals
of the resulting existing group do not require it. Partial overlap is refused.

[Owned ACME/TLS provisioning evidence](benchmarks/2026-10-03/tls-group-provisioning/README.md)
verifies initial group publication and subsequent renewal. Automatic issuance
in response to a domain claim, public CA operation and new-group crash/KVM
verification remain incomplete.

The [first-group publication crash check](benchmarks/2026-10-03/tls-first-group-crash/README.md)
passes fourteen owned ACME/live TLS checks. A hard exit after first-group manifest
publication leaves the live fallback and a durable journal; a retry verifies
recovery without CA reissuance. This covers one new-group crash boundary.
Automatic issuance after a domain claim remains open.

[Configured initial issuance](benchmarks/2026-10-03/initial-tls-worker/README.md)
now passes fifteen owned ACME/live TLS checks. Explicit worker jobs can issue
a missing lineage through HTTP-01 and provision its named TLS group, preserving
the default; non-due retries skip issuance. Eight policy and 21 renewal tests
pass. Automatic job discovery after a domain claim, public DNS/CA behavior
remain incomplete.

The [fresh-account worker check](benchmarks/2026-10-03/fresh-account-tls-worker/README.md)
passes sixteen owned ACME/live TLS checks. A separate, initially absent Certbot
account directory gains exactly one registration before initial issuance and
verified named-group activation; a non-due retry skips issuance. Fresh account
registration is now verified against the owned CA. Public DNS/CA operation,
domain-claim job discovery remain incomplete.

[Initial issuance through KVM guest traffic](benchmarks/2026-10-03/initial-tls-kvm/README.md)
now passes seventeen outer ACME and fifteen real KVM checks. A fresh account
and lineage issue a certificate and provision its named group while an existing
16 MiB guest download completes. Routing, reload refusal, restart and guest
lifecycle checks pass with clean teardown. Configured initial issuance is KVM
verified; automatic domain-claim job discovery and public DNS/CA operation remain
incomplete. Failed fixture runs are retained and excluded.

[Automatic claim discovery through ACME and KVM](benchmarks/2026-10-03/discovery-tls-kvm/README.md)
now passes seventeen outer ACME and fifteen real KVM checks. An empty job list
is populated from authenticated, certificate-verified API inventory under operator
suffix policy; guarded initial issuance provisions the named TLS group while an
existing 16 MiB guest download completes. This supersedes earlier statements
that domain-claim job discovery was unverified. Certificate retirement, real
ownership-change interruption, public DNS/CA operation and fleet scale remain
incomplete. No managed-product performance win is established.

[Real pending-job unbind verification](benchmarks/2026-10-03/discovery-unbind-kvm/README.md)
passes seventeen outer ACME and sixteen KVM checks. An actual issued-but-not-yet
activated job survives a real unbind: discovery refuses without changing its
journal, manifest or certificate. Restoring the proof-authorized binding activates
the retained certificate. Live owner replacement and port-change interruptions,
certificate retirement and public DNS/CA operation remain incomplete.

[Live pending-job ownership and port changes](benchmarks/2026-10-03/discovery-ownership-kvm/README.md)
pass seventeen outer ACME and seventeen KVM checks. Real unbind, replacement
owner and guest-port changes refuse a pending job without altering its journal,
manifest or issued certificate; restoring the original binding recovers deployment.
Replacement guests are cleaned up. This supersedes earlier statements that live
owner/port interruption checks were unverified. Certificate retirement, public
DNS/CA behavior and fleet operation remain incomplete.

[Explicit named TLS group retirement](benchmarks/2026-10-03/tls-retirement/README.md)
now passes fifteen owned ACME/live TLS checks plus eight provisioning and
21 renewal tests. `deploy-tls-certificate.py --retire-group` removes an exact
entry, preserves the existing default and immutable files, verifies its pinned
fallback leaf, retries idempotently and permits reprovisioning. The existing
manifest/lineage/generation/process arguments remain required; lineage contents
are unused in retirement mode. Provision and retirement flags are exclusive.
Automatic completed-job retirement, no-default bundles and retirement-specific
crash/KVM verification remain incomplete. This operation sends no application
data during fallback pin checks and does not revoke CA certificates or remove
bindings, old files or established connections.

[Retirement publication crash recovery](benchmarks/2026-10-03/tls-retirement-crash/README.md)
passes sixteen owned ACME/live TLS checks. A hard exit after retirement manifest
publication leaves the previous live leaf and a durable journal; retry reconciles
the manifest, verifies fallback activation and clears the journal without changing
immutable-file inventory. Automatic retirement, additional crash boundaries,
no-default bundles and retirement-specific KVM traffic remain incomplete.
