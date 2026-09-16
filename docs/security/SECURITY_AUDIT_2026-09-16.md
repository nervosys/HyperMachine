# HyperMachine Dependency Security Audit — 2026-09-16

**Audit date:** 2026-09-16
**Supersedes:** `SECURITY_AUDIT.md` (2026-02-02), which is kept as a record of
what was true then and should not be read as current — see *What the previous
report got wrong* below.
**Scope:** dependency advisories, bans, licences and sources. **Not** a code
audit: no review of `unsafe`, the hypervisor's own attack surface, or the guest
boundary.

## Method

Everything below was produced by running the tools, on this tree, at the commit
this file was added:

```bash
cargo audit                      # RustSec advisory database
cargo deny check advisories      # the same, plus this repo's deny.toml policy
cargo deny check bans
cargo deny check licenses
cargo deny check sources
```

## Result

| Check | Before | After |
| --- | --- | --- |
| `cargo audit` vulnerabilities | **2** | **1** |
| `cargo audit` warnings | 6 | 4 |
| `cargo deny check advisories` | **FAILED** | **ok** |
| `cargo deny check bans` | ok | ok |
| `cargo deny check licenses` | ok | ok |
| `cargo deny check sources` | ok | ok |

Three things were fixed in the course of this audit rather than only recorded.

### Fixed: RUSTSEC-2026-0285 — rustls TLS 1.3 handshake confusion

`rustls` 0.23.43. *TLS 1.3 handshake messages incorrectly accepted across
encryption level boundaries.* Published **2026-09-14**, two days before this
audit, and directly on the path this repository had just started using: the
sandbox proxy terminates TLS with rustls.

Fixed by `cargo update -p rustls` → **0.23.45**, which is the version the
advisory names. The TLS path was re-verified after the upgrade with a real
gRPC call over HTTPS into a booted microVM, not only rebuilt.

### Fixed: RUSTSEC-2025-0134 — rustls-pemfile unmaintained

This one was introduced *by the work being audited*, which is worth stating
plainly. `crates/hv2-api/Cargo.toml` already carried the line

```toml
# rustls-pemfile is unmaintained (RUSTSEC-2025-0134); use rustls-pki-types' built-in PEM parser instead.
```

and the proxy's first TLS implementation added `rustls-pemfile` as a dependency
immediately above that comment. `cargo deny check advisories` failed on it.
Replaced with `rustls_pki_types::pem`, which the comment names and which the
crate already depended on.

### Fixed: yanked chacha20

`chacha20` 0.10.1 was yanked from crates.io. `cargo update -p chacha20` →
0.10.2.

## Open: RUSTSEC-2023-0071 — Marvin attack in `rsa`

`rsa` 0.9.10. *Potential key recovery through timing sidechannels.* **No fixed
upgrade is available** — the advisory has been open since 2023-11-22 and the
fix requires the 0.10 line, which replaces `num-bigint` with `crypto-bigint`.

This is a **shipped dependency**, not a dev-dependency: it is declared in
`crates/hv2-core/Cargo.toml`'s `[dependencies]` and reaches every binary that
links `hv2-core`. Checked, because a first reading of `cargo tree -i rsa
--depth 1` suggested otherwise.

**Where it is used:** `crates/hv2-core/src/crypto/asymmetric.rs` only — RSA key
generation, and PKCS#1 v1.5 and PSS signing. Nothing else in the workspace
calls it.

**What the exposure is:** the Marvin attack recovers a private key by timing
*decryption* of attacker-chosen ciphertexts. An attacker needs to submit many
ciphertexts to an oracle that decrypts with the key and measures the response
time. A deployment that performs RSA decryption on attacker-supplied input over
a network is exposed; one that only generates keys and signs is far less so,
because signing does not take attacker-chosen ciphertext.

That is a statement about this code's shape, not a clearance. Anyone relying on
`asymmetric.rs` for RSA decryption should treat this as live.

**Options, none free:**

1. Migrate to `rsa` 0.10. Attempted previously in this repository and it breaks
   on the `num-bigint` → `crypto-bigint` change — which is the change that
   fixes the timing leak, so the breakage is the point rather than an obstacle
   to route around.
2. Drop RSA. The post-quantum and ECDSA paths (`ml-dsa`, `slh-dsa`, `p256`,
   `p384`) do not use it.
3. Accept and document, which is the current state.

## Open: four unmaintained crates

Not vulnerabilities. Each is a crate whose author has stepped back, which
matters for how a future advisory would be fixed rather than for today.

| Crate | Advisory | Reached through |
| --- | --- | --- |
| `bincode` 1.3.3 | RUSTSEC-2025-0141 | direct |
| `paste` 1.0.15 | RUSTSEC-2024-0436 | transitive |
| `smartstring` 1.0.1 | RUSTSEC-2025-0134 | transitive |
| `ttf-parser` 0.25.1 | RUSTSEC-2026-0249 | transitive |

## What the previous report got wrong

`SECURITY_AUDIT.md` (2026-02-02) is left unedited, because editing a dated
report's findings falsifies a record. Two of its statements are not true of
this tree and should not be carried forward:

- It documents a `[workspace.metadata.security]` block in `Cargo.toml` with
  `minimum_rust_version`, `deny_unknown_registry`, `deny_git_dependencies` and
  `audit_frequency`. **No such block exists**, and nothing in the repository or
  its CI reads those keys.
- It states a minimum Rust version of **1.87.0**. The manifest has required
  **1.95** since 2026-05-19, and `cargo` refuses to build on 1.87.

## What this audit does not cover

Dependencies only. It says nothing about the hypervisor's own security
properties: the guest/host boundary, `unsafe` blocks in the KVM and WHPX
backends, the capability model, or the sandbox proxy's exposure as a network
listener. Those want a code audit, which this is not.
