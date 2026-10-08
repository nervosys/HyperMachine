# FIPS 140-3

HyperMachine has two builds:

- **The default build** runs TLS on `ring`, as it always has.
- **The FIPS build** runs TLS on AWS-LC's FIPS 140-3 validated module, through
  `aws-lc-rs`. It is the one to deploy where FIPS 140-3 validated cryptography is
  required, such as on AWS GovCloud under FedRAMP High.

The choice is made in exactly one place, the `hv2-tls` crate. Every TLS endpoint
and HTTPS client listed below asks it, so a build is all one or all the other.

## Building and running it

The FIPS build compiles AWS-LC's FIPS module, which needs CMake, Go and a C
compiler (clang or gcc) on the build host:

```sh
cargo build --release -p hv2-sandboxd --features hv2-sandboxd/fips
cargo build --release -p hv2-cluster --bin hv2-control-plane --features hv2-cluster/fips
cargo build --release -p hm-cli --features hm-cli/fips
```

Run the daemons with `--fips` (or `HV2_FIPS=1`). A binary started that way checks
two things before doing anything else: that it is the FIPS build, and that
AWS-LC reports FIPS mode, which means its power-on self tests passed. If either
fails, it refuses to start. So a default build cannot be deployed by mistake
where FIPS is required.

## Licensing: an open decision

The FIPS build links `aws-lc-fips-sys`, whose license is `ISC AND (Apache-2.0 OR
ISC) AND OpenSSL`. The OpenSSL license is not on this repository's allow list
(`deny.toml`), so `cargo deny --features hv2-tls/fips check licenses` fails. It
is not on the list because its advertising clause is widely considered
incompatible with the GPL family, and HyperMachine is AGPL-3.0-only.

Distributing FIPS binaries therefore needs a decision by the copyright holder,
such as a linking exception for AWS-LC, before `OpenSSL` is added to the allow
list. The default build does not link it and passes the license check.

## What the FIPS build covers

| Path | On the FIPS module |
|---|---|
| The API's TLS (`--api-tls-cert`) and the sandbox proxy's TLS (`--tls-cert`, `--tls-bundle-file`) | yes |
| Control-plane-to-node mutual TLS | yes |
| The egress HTTPS interception relay (secret substitution) | yes |
| Outbound HTTPS from the control plane (nodes, webhooks, the SSO provider, DNS-over-HTTPS) and the node daemon | yes |
| The `hm` CLI's API client and `hm sandbox vm login` | yes |
| Redis/Valkey TLS (`rediss://`) | yes, through the process default the binary installs |

On the FIPS module, TLS offers only AES-GCM suites, with the P-256, P-384 and
X25519+ML-KEM-768 key exchange groups. ChaCha20-Poly1305 and plain X25519 are not
offered, so they cannot be negotiated.

## What it does not cover yet

These still use crypto outside a validated module, in either build. They are
the next steps of Phase 2 in the [VMware replacement plan](VMWARE_REPLACEMENT.md):

- **Non-TLS primitives.**
  - The node's workload-identity JWT signing and cloud registry login sign with
    `ring` directly.
  - Digests and MACs across the control plane (API key digests, the access
    audit MAC, SSO sessions, webhooks) use RustCrypto's `sha2` and `hmac`, or
    IronCrypto.
  - IronCrypto is the organisation's own library and does not yet have a CMVP
    certificate.
- **The egress relay's interception certificates.** These are issued with
  `rcgen` on `ring`.
- **Other clients.**
  - The local `t1` management tool and `hm-gui`.
  - hv2-core's optional remote telemetry, which is off by default.
- **Test fixtures,** which are never shipped.

`ring` is still compiled into the FIPS build for those uses. None of them is a
TLS endpoint.
