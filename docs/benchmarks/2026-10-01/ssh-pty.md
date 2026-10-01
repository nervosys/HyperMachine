# Guest PTY over authenticated named SSH

A real nested-KVM run passed all 20 functional checks using the same normal
CLI, kernel, guest image and authenticated TLS/mTLS path as the
[named SSH verification](ssh-name.md). The new opt-in `--ssh-pty` check
forced OpenSSH PTY allocation with `-tt`, verified both guest stdin/stdout
were terminal descriptors, and passed an exact generated line through the
guest's canonical terminal input. The guest emitted its success marker only
after checking those descriptors and input; output used terminal CRLF
translation.

All 19 named SSH/TCP authentication, binary transfer, duplicate-label and
lifecycle controls retained their previous results. Cleanup left zero
guests, no errors and all 22 owned services/forwarders stopped. There is no
performance claim or managed-provider comparison.

Reproduce using [the SSH fixture workflow](ssh-stdio.md), adding both
`--ssh-by-name` and `--ssh-pty` to `tools/e2e-tcp-tunnel.py`. Omitting the
new flag keeps the previous non-PTY cases. Verify the archive with:

```text
python docs/benchmarks/2026-10-01/verify-ssh-pty.py
```

`--current` checks the executed harness against current code; `--staged`
checks Git-index bytes. The 25-file PTY archive preserves raw results and
logs and verifies the earlier named/base SSH archives, including the CLI's
compiled-source provenance. No private credentials or images are committed.

This checks PTY allocation and canonical input through a noninteractive
test driver. Terminal resizing, signals, job control, full-screen programs,
and SFTP remain unverified. Guest SSH provisioning and atomic name
reservations remain absent.
