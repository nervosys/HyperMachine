# Custom domains

The cluster control plane binds an operator-managed DNS hostname to a sandbox
and guest HTTP port. Requests keep the public hostname, path, query and body
while the proxy sends routing headers to the owning node. HTTP/1.1 and HTTP/2
use the same binding. Existing explicit sandbox routing headers take precedence.

Configure DNS to point at the control-plane proxy, and supply a certificate
whose SANs cover your custom names. The certificate for `*.sandbox.example.com`
does not cover `app.example.com`. Certificate issuance, DNS ownership verification
and ACME renewal are operator responsibilities; they are not automated here.

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

For local functional verification with an actual KVM guest:

```sh
python3 tools/verify-custom-domains.py \
  --control-plane target/release/hv2-control-plane \
  --daemon target/release/hv2-sandboxd --kernel /path/to/bzImage \
  --initrd /path/to/guest.cpio.gz --output custom-domains.json
```

This requires Linux KVM, `redis-server`, `openssl`, and a guest image containing
BusyBox `httpd`. It owns isolated services and guest resources, verifies TLS
hostname identity through a local connection, checks port updates, control-plane
restart, auto-resume, removal and name reuse, and reports cleanup and artifact
hashes. It measures functional behavior; it does not establish a performance win.
The [recorded local result](benchmarks/2026-10-01/custom-domains.json) passed all
six checks with clean teardown. The initial verifier run is retained separately;
it stopped at an incorrect expected pause status before exercising auto-resume.
