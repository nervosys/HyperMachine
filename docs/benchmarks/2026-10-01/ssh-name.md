# Named SSH lookup on a real KVM guest

`hm sandbox vm create --name my-sandbox` persists `hm.name` metadata.
`hm sandbox vm tcp-stdio --name my-sandbox` resolves an exact label through
the authenticated, unpaginated `/sandboxes` API and connects to the returned
sandbox ID. Missing and duplicate names fail before opening a tunnel.
The client also checks the returned metadata itself. Labels permit 1–64
ASCII letters, digits, hyphens, underscores or dots, excluding dot segments.

The current normal CLI build, SHA-256
`6cfc504641f3ed860db427032dcfa460f1d5a9f9f9b6b2567faeaff0096716d3`,
passed all 19 real KVM/TLS cases using named lookup for every OpenSSH
connection. The [same fixture and transport](ssh-stdio.md) preserved the
1 MiB binary roundtrip, remote exit code 7, rejected client key and rejected
guest host key. Forking copied the parent's metadata and made the name
ambiguous; OpenSSH failed with exit 255 and empty stdout, reporting the CLI
ambiguity error. Deleting the duplicate restored a successful named SSH
command. The run left zero guests, no cleanup errors and all 22 owned
services/forwarders stopped.

All ten executable CLI integration tests passed on Windows and Linux,
including named creation's JSON, invalid names, exact lookup, missing and
duplicate names, and binary stdio behavior. Strict all-target CLI Clippy
passed on Windows. The real-guest harness creates the metadata through the
API; the executable fixture separately verifies `create --name` sends it.

The 29-file named archive records raw results/logs, the normal build output,
the changed compiled client source, executed harness and client tests. Its
verifier also checks the preserved 76-file base SSH archive and reuses exact
unchanged source snapshots. No private credentials or guest images are
committed. Reproduce with the commands in [the base SSH report](ssh-stdio.md),
adding `--ssh-by-name` to `tools/e2e-tcp-tunnel.py`, then verify:

```text
python docs/benchmarks/2026-10-01/verify-ssh-name.py
```

`--current` compares current client/tools with the frozen sources;
`--staged` additionally verifies Git-index bytes. This evidence establishes
no performance win.

Names remain metadata labels, not an atomic uniqueness registry. Duplicate
creation and fork inheritance are allowed and then rejected on lookup.
Paused VMs are included but must be resumed before connecting. Guest SSH
servers and credentials are operator-provisioned. Multi-tenant ownership,
name reservations/renames, job control and SFTP verification remain open.
[PTY allocation and canonical input](ssh-pty.md) and
[terminal resize/Ctrl-C](ssh-terminal.md) are verified separately.
See [client configuration](../../TCP_STDIO.md).
