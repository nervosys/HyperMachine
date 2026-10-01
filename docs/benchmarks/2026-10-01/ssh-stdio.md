# Real guest SSH over authenticated stdio

Two local nested-KVM runs passed all 19 functional cases each. Four cases
used OpenSSH against Dropbear in a real 1-vCPU, 1024-MiB guest; the other 15
retained the TCP authentication, binary transfer and lifecycle controls.
Both runs left zero sandboxes, no cleanup errors and all 22 owned services
and CLI forwarders stopped. This is functional evidence, not a latency,
throughput, managed-service or across-the-board comparison.

| SSH check | Observed result in each run |
|---|---|
| Binary upload and download through `tcp-stdio` | Exact 1,048,576-byte echo and matching guest-file SHA-256 |
| Remote failure | OpenSSH returned guest exit code 7 |
| Unregistered client key | Authentication rejected; exit 255, empty stdout |
| Incorrect pinned guest host key | Host-key verification rejected; exit 255, empty stdout |

The path was OpenSSH → `hm sandbox vm tcp-stdio` → API TLS with an explicit
trusted CA → control plane → node mTLS and cluster token → vsock → guest
loopback port 22. OpenSSH used strict pinned guest host-key checking, a
single explicit client identity and no SSH agent or ambient SSH config.
The SSH server listened only on guest loopback, disabled password login
and disabled SSH port forwarding. Its keys were disposable fixture keys.
The guest host public key was also checked through authenticated guest exec
against the independently generated fixture public key before SSH use.

The isolated fixture used Dropbear 2025.89 from Debian trixie packages and
OpenSSH 10.0p2 Debian-7+deb13u4. No SSH service was installed on the host.
The existing larger-backlog TCP image, guest agent, accepted API socket fix,
control plane and kernel were retained; only the separate SSH image gained
the server, runtime libraries and fixture account/key files.

The first run used the CLI produced by Linux integration tests. The final
run used a separately built and frozen normal CLI, SHA-256
`9a3d4c95d73b3844d61393d0a674460de0fb09d28ee4e55ac418f343ea2d6832`.
Its build output, source hashes and exact client/job source bytes are
preserved with both raw reports and logs. The archive contains no private
SSH keys, certificates, API keys or guest images. Package hashes and input
library hashes identify the fixture dependencies. Fresh keys change image
bytes on a new build; reproduction means the same checks, not identical
disposable credentials.

On a Linux x86-64 host with KVM, the existing prepared TCP fixture, `cpio`,
OpenSSH, OpenSSL, Redis, and extracted `dropbear-bin`, `libtomcrypt1` and
`libtommath1` Debian packages, build an unused output directory:

```sh
python3 tools/guest-image/build-ssh-fixture.py \
  --base /path/to/guest-tcp.cpio.gz \
  --package-root /path/to/extracted-packages \
  --output /path/to/new-ssh-fixture

python3 tools/e2e-tcp-tunnel.py \
  --daemon /path/to/hv2-sandboxd \
  --control-plane /path/to/hv2-control-plane \
  --cli /path/to/hm \
  --kernel /path/to/bzImage \
  --initrd /path/to/new-ssh-fixture/guest-ssh.cpio.gz \
  --ssh-fixture /path/to/new-ssh-fixture \
  --output /path/to/ssh-e2e.json

python3 docs/benchmarks/2026-10-01/verify-ssh.py
```

Omitting `--ssh-fixture` retains the original 15-case TCP suite. The verifier
checks archived hashes, source/build relationships, all 19 final cases,
the generated binary payload hash and complete cleanup. `--staged` also
checks Git-index bytes.

Users still provision their guest SSH server and credentials and maintain
local OpenSSH aliases, as described in [the CLI guide](../../TCP_STDIO.md).
Atomic name reservations, automatic SSH provisioning, job control and SFTP
verification remain open. [Named lookup](ssh-name.md),
[PTY allocation/canonical input](ssh-pty.md) and
[terminal resize/Ctrl-C](ssh-terminal.md) have subsequent verification.
The tested noninteractive session supplies no
performance win.
