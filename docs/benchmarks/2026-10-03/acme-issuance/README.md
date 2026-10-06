# Owned ACME issuance and renewal

Certbot 5.8.0 and the official Pebble v2.10.1 test CA pass three protocol checks:

| Check | Result |
|---|---|
| HTTP-01 issuance | Pass; three successful challenge responses; OpenSSL verifies issuer and hostname |
| Forced renewal | Pass; three fresh challenge responses; different certificate with valid issuer and hostname |
| Challenge server unavailable | Pass; renewal refuses issuance and the last certificate remains unchanged |

All listeners use loopback and temporary ports. Pebble uses an owned DNS test
server resolving the fixture hostname to loopback. Its always-valid mode is not
enabled; authorization reuse is disabled. The ACME transport uses an owned CA
with ordinary certificate verification. No public CA is contacted. Temporary
private keys, accounts and challenge files are removed; both CA processes are
reaped and the HTTP thread stops. The report contains public certificate hashes,
executable hashes and the verifier hash, without account keys or challenge tokens.

Run with the official Linux AMD64 release executables and an isolated Certbot
environment matching `dependencies.txt`:

```sh
python3 tools/check-acme-issuance.py \
  --pebble /path/to/pebble --challenge-server /path/to/pebble-challtestsrv \
  --certbot /path/to/venv/bin/certbot --output /new/path/report.json
```

The checker refuses an existing report and bounds subprocess duration. Dependency
versions are recorded; the Certbot executable hash covers its launcher, not every
installed Python module. The first exploratory run rejected an incomplete
transport CA key-usage extension; the fixture was corrected. A subsequent run was
stopped during Certbot's normal randomized renewal delay. The final verifier
disables that delay for the test only and repeats successful issuance and renewal.

This is functional evidence for the external issuer. It does **not** verify
HyperMachine deployment, automated domain provisioning, a renewal scheduler,
public DNS propagation, public CA policy, or managed-service performance. The
previous KVM certificate-bundle test verifies deployment separately with locally
generated certificates; an integrated ACME-to-KVM check remains required.

Primary references: [Pebble](https://github.com/letsencrypt/pebble),
[pinned release](https://github.com/letsencrypt/pebble/releases/tag/v2.10.1), and
[Certbot webroot and renewal documentation](https://eff-certbot.readthedocs.io/en/stable/using.html).
