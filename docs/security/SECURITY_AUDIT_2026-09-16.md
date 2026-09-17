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

## Closed, same day: RUSTSEC-2023-0071 — Marvin attack in `rsa`

`rsa` 0.9.10, *potential key recovery through timing sidechannels*, open
upstream since 2023-11-22 with no fixed release. This report first recorded it
as accepted, with a `deny.toml` ignore reading "tracked for migration once a
constant-time release ships".

**It is migrated.** `hv2-core` now uses IronCrypto's `ic-rsa`
(github.com/nervosys/IronCrypto, pinned by revision), whose private-key path
is constant-time in `d`. The `rsa` crate is no longer in `Cargo.lock` at all —
`cargo tree -i rsa` reports no such package — so the advisory is not
suppressed, it is inapplicable, and the ignore was deleted rather than left to
sit over an advisory nothing can raise. `cargo deny check advisories bans
licenses` passes without it.

`ic-rsa` is pure Rust with no dependencies outside its own workspace, and
covers what this module needs: key generation, PKCS#1 v1.5 and PSS signing and
verification. Signing rebuilds the key from its primes so it uses the Chinese
remainder theorem; the alternative is correct and about four times slower.

### What this report got wrong about it

The paragraph above used to end: *"Anyone relying on `asymmetric.rs` for RSA
decryption should treat this as live."* That was the right instinct and the
wrong fact. `asymmetric.rs` did expose `rsa_encrypt`/`rsa_decrypt`, but they
never went through the `rsa` crate at all — they went through a hand-written
`mod_exp_bytes` in the same file, so the Marvin advisory never applied to them.

What applied instead was worse, and is the more serious finding of the two:

**The hand-written modular exponentiation was wrong.** It returned 121 for
`4^13 mod 497` (445) and 7 for `5^117 mod 19` (1). It squared the accumulator
and multiplied by the base — the left-to-right square-and-multiply — while
scanning the exponent's bits least-significant first, which that form cannot
do. A comment reading "advance base by 32 squarings for next limb" sat above
an empty `if`. So `rsa_encrypt` and `rsa_decrypt` did not implement RSA; they
produced numbers.

Nothing caught it because the only test asserted that an empty key is refused.
There was no round-trip test, and an encrypt/decrypt pair that never round-trips
is the first thing such a test would have found.

Two further problems in the same forty lines: both doc comments said
**"RSA-OAEP"** while the code did PKCS#1 v1.5, and the v1.5 decryption path
returned early on each distinct padding failure, which is a Bleichenbacher
oracle by construction.

**Resolution: removed, not repaired.** No caller existed anywhere in the
workspace. IronCrypto omits RSA encryption deliberately — its own module notes
call RSAES-PKCS1-v1_5 "a Bleichenbacher oracle waiting to happen" — and key
transport belongs to ECDH or ML-KEM. RSA's remaining job here is signatures.
The reasoning is recorded in `asymmetric.rs` where the functions used to be, so
the next person to want RSA encryption finds the argument rather than the gap.

**The lesson for this report's method.** The original entry was assembled by
reading `cargo audit` output and grepping for the crate's name. That found a
real advisory and missed a broken primitive sitting beside it, because a
hand-rolled implementation has no advisory to find. Two known-answer tests
would have caught it in seconds, and that is what a crypto audit should run
rather than only a dependency scan.

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
