# Real ACME renewal and certificate deployment through KVM

The final `python3 -O` run passes **13 issuer/deployment checks and 14 real KVM
checks** using Certbot 5.8.0, Pebble v2.10.1, and the previously validated
certificate-bundle control-plane binary. Six actual HTTP-01 challenge responses
are observed during initial issuance, six during the control-plane renewal, and
six during the guest-traffic renewal. Both configured fixture names are validated.
No public CA is contacted; this is functional evidence, not a performance result.

| Behavior | Evidence |
|---|---|
| Initial issuance and renewal | Issuer and hostname verified with OpenSSL; distinct certificate hashes; actual HTTP-01 responses |
| Certbot deploy-hook activation | Fresh certificate-validated connections to the actual control-plane binary see the renewed leaf |
| Real guest traffic during renewal | Existing 16 MiB zero-filled KVM guest download completes exactly; new connections see the new leaf |
| Failed challenge | Renewal fails without replacing the issued certificate, deployment manifest or active proxy leaf |
| Untrusted certificate | Activation times out, restores the exact prior manifest, and verifies the prior leaf |
| Mismatched key | Specific key mismatch is refused before publication |
| Wrong process or manifest | Specific executable/argument checks refuse mutation or signaling |
| Concurrent deployment | The owned manifest lock refuses the overlapping hook |
| Retried generation | TLS is rechecked without changing manifest bytes or generation inventory |
| Expired active certificate | A short-lived certificate actually expires on the running proxy; its old leaf is pinned, and deployment restores normally validated HTTPS |
| Domain lifecycle | DNS ownership refusals, port update, Redis restart persistence, auto-resume, unbinding, deletion and ownership reuse pass |
| Teardown | Both issuer processes, the independent TLS control plane and all KVM fixture processes are reaped; HTTP/DNS threads stop; isolated node inventory is empty |

The hook copies the new pair into a private generation, preserves the manifest's
mode, publishes it with file/directory fsync, signals a Linux pidfd pinned to the
configured executable and manifest, then verifies every configured SNI name.
New activation uses normal chain, hostname and time validation. The previous
leaf is pinned to the operator-owned manifest so recovery is possible even after
expiry. Rollback restores that prior state; it does not make an expired old leaf
valid. Failed and superseded private generations are retained for operator
recovery. Process death, interruption, noncooperating manifest writers and
generation garbage collection require operator reconciliation/maintenance.

The combined command is reproducible with Linux KVM, Redis, OpenSSL, the official
Pebble Linux AMD64 release executables, an isolated Certbot environment matching
`dependencies.txt`, and the kernel/initrd whose hashes are in `report.json`:

```sh
python3 -O tools/check-acme-issuance.py \
  --pebble /path/to/pebble --challenge-server /path/to/pebble-challtestsrv \
  --certbot /path/to/venv/bin/certbot \
  --control-plane /path/to/hv2-control-plane \
  --deploy-hook /absolute/path/to/tools/deploy-tls-certificate.py \
  --kvm-verifier /absolute/path/to/tools/verify-custom-domains.py \
  --daemon /path/to/hv2-sandboxd --kernel /path/to/bzImage \
  --initrd /path/to/guest-output-drain.cpio.gz --output /new/path/report.json
```

Use the archived `tools/` copies for the exact verified drivers. The report binds
their hashes and executable inputs; `manifest.json` covers the preserved files.
The control-plane source/build evidence is in
[the TLS bundle archive](../tls-bundle/README.md). No Rust source or protected
original core file was changed for this hook. The expanded verifier also passes
the six legacy custom-domain checks with the same binary and clean teardown.

Pebble authorization reuse and its random validation delays are disabled, while
always-valid mode is not enabled. ACME transport TLS validates an owned CA;
temporary account keys, private keys and challenge files are removed. The issuer
and DNS challenge test server bind loopback; existing HyperMachine listeners use
their normal bind behavior with random fixture credentials. Certificate hashes
are public identifiers. Certbot's launcher hash does not cover every installed
Python module; dependency versions are separately recorded. An initial deploy
trial and later recovery/retry refinements preceded the final full run; only the
final source-bound run is used for the table above.

This supplies an operator-managed Certbot deploy hook. It does not supply automatic
issuance after a custom-domain claim, a built-in renewal scheduler, a managed DNS
service, public CA policy verification or fleet-wide deployment. No sandbox-product
performance superiority follows from these checks.

Primary references: [Certbot deploy hooks and renewal](https://eff-certbot.readthedocs.io/en/stable/using.html),
[Pebble v2.10.1](https://github.com/letsencrypt/pebble/releases/tag/v2.10.1), and
[Python pidfd signaling](https://docs.python.org/3/library/signal.html#signal.pidfd_send_signal).
