# TCP over stdin/stdout

`hm sandbox vm tcp-stdio SANDBOX_ID --port 22` connects standard input and
output to a guest TCP service through the authenticated sandbox API. The
default guest port is 22. Standard output contains only guest bytes; errors
and diagnostics go to standard error. It creates no local listening socket.
API keys come from `HV2_API_KEY`, and `--endpoint`, `--api-ca-cert` and
`--request-timeout` have the same meaning as for the
[loopback TCP forwarder](benchmarks/2026-10-01/tcp-tunnel.md).

For OpenSSH, an operator can put this in their SSH configuration, replacing
the API endpoint and sandbox ID with their values:

```sshconfig
Host my-sandbox
    HostName SANDBOX_ID
    User sandbox
    ProxyCommand hm sandbox vm --endpoint https://sandbox-api.example.com tcp-stdio %h --port %p
    HostKeyAlias hypermachine-SANDBOX_ID
```

Then `ssh my-sandbox` uses the tunnel. The guest must already have an SSH
server listening on its loopback interface and appropriate user credentials.
OpenSSH performs guest authentication and host-key verification normally.
The local SSH alias is configuration on the client; there is no server-side
sandbox name registry or automatic SSH provisioning. Resume paused sandboxes
before connecting. Set an appropriate lifetime before long sessions.

Closing stdin shuts down the tunnel's write direction and waits for the
guest's remaining output. Guest EOF ends the stdio session even when local
stdin remains open. Input buffering is bounded to eight 16 KiB chunks.
The API request timeout bounds the handshake, not an established session.

The executable integration fixture checks authenticated HTTP upgrade,
65,536 binary bytes echoed after stdin EOF, guest EOF while stdin remains
open, and rejection without stdout contamination. Run:

```text
cargo test -p hm-cli --test sandbox_vm_client
```

The protocol fixtures are complemented by [real KVM SSH verification](benchmarks/2026-10-01/ssh-stdio.md)
through API TLS and node mTLS: exact 1 MiB binary roundtrip, remote exit code,
rejected client key and rejected guest host key. Guest SSH provisioning and
server-side name resolution remain operator responsibilities; this addition
establishes no performance comparison.

On 2026-10-01, all nine CLI executable integration tests passed on Windows
and Linux; `cargo clippy -p hm-cli --all-targets -- -D warnings` passed on
Windows.
