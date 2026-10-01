# Named SSH terminal resizing and interrupt

The real nested-KVM SSH suite passed all 21 cases with the same frozen
normal CLI, guest image, kernel and verified API TLS/node mTLS path as
[the earlier PTY run](ssh-pty.md). A local OpenSSH pseudo-terminal drove
these additional guest events:

| Control event | Guest observation |
|---|---|
| Initial terminal size | `stty size` returned 24 rows, 80 columns |
| Local resize and SIGWINCH | Guest SIGWINCH handler reported 43 rows, 132 columns |
| Ctrl-C byte from the local raw terminal | Guest foreground shell handled SIGINT, emitted its marker and exited 0 |

The driver sets the local PTY size with `TIOCSWINSZ`, then explicitly sends
SIGWINCH to OpenSSH to simulate a window-manager resize event. OpenSSH's
SSH window-change request updates the guest PTY, whose own SIGWINCH trap
prints its measured size. It sends byte `0x03` only after that marker;
the guest interrupt marker and SSH exit code prove the resulting interrupt.
The bounded transcript and marker order are preserved in the raw report.
This is terminal protocol verification, not GUI/window-manager testing or
a performance benchmark.

Every prior 20 PTY/named-SSH/TCP case retained its results, including binary
transfer, guest/client key rejection, fork ambiguity and lifecycle controls.
Cleanup left zero guests, no errors, and all 23 registered service/client
processes stopped; the added OpenSSH terminal process exited 0.

Reproduce with [the SSH fixture commands](ssh-stdio.md), adding
`--ssh-by-name --ssh-pty --ssh-terminal` to `tools/e2e-tcp-tunnel.py`. No new
package, guest image or production binary is required for this check.
Verify the 25-file transcript/raw-log/source archive with:

```text
python docs/benchmarks/2026-10-01/verify-ssh-terminal.py
```

`--current` checks the current harness; `--staged` checks Git-index bytes.
The verifier checks marker order, dimensions, exit status, cleanup and the
preserved earlier SSH archives, including the CLI compiled-source evidence.
No private credentials or images are committed.

Job control, full-screen applications, additional terminal signals and
SFTP remain unverified. Guest SSH provisioning and atomic name reservations
remain absent. These bounded passes establish no comparative performance
win or general reliability SLA.
