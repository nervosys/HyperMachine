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
The local SSH alias is configuration on the client. Resume paused sandboxes
before connecting. Set an appropriate lifetime before long sessions.

To persist a label in the sandbox API, create with
`hm sandbox vm create --name my-sandbox`, then use
`hm sandbox vm tcp-stdio --name my-sandbox --port 22`. In the SSH configuration
above, set `HostName my-sandbox` and replace `tcp-stdio %h` with
`tcp-stdio --name %h`. Keep a guest-specific `HostKeyAlias` and verify its key.
Names use 1–64 ASCII letters, digits, hyphens, underscores or dots, excluding
`.` and `..`. They are stored as `hm.name` metadata. Lookup includes running
and paused sandboxes and refuses missing or duplicate names, without opening
a tunnel. It resolves to a fixed sandbox ID before connection.

These are metadata labels, with no atomic uniqueness reservation or rename
registry. Creating duplicate labels is allowed; forks copy their parent's
metadata and can therefore make a label ambiguous. Use the explicit sandbox
ID in that case. Listing requires the existing API authorization; labels
introduce no additional ownership or team isolation. Guest SSH provisioning
remains an operator responsibility.

[Real KVM named-SSH verification](benchmarks/2026-10-01/ssh-name.md) checks
duplicate rejection after a fork and successful lookup after its deletion.
An additional [real guest PTY check](benchmarks/2026-10-01/ssh-pty.md) verifies
terminal descriptors and canonical input. A [terminal control check](benchmarks/2026-10-01/ssh-terminal.md)
verifies size propagation, guest SIGWINCH and Ctrl-C. Job control, additional
terminal signals, full-screen programs and SFTP remain unverified.
All ten current CLI integration tests passed on Windows and Linux, with
strict all-target Clippy on Windows.

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
atomic name reservations remain unimplemented; this addition
establishes no performance comparison.

Named connections now prefer authenticated `GET /sandbox-names/{name}` resolution when a bound reservation exists. Pending reservations and authorization or server errors stop the connection. Metadata fallback is limited to an explicitly unreserved name or an older server with an empty route-not-found response; it continues to refuse duplicate names. See [reservation status and remaining creation work](SANDBOX_NAME_RESERVATIONS.md).
