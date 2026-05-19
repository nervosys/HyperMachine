# BIS TSU Notification — HyperMachine v1.0.0

**To:** `crypt@bis.doc.gov`, `enc@nsa.gov`
**Subject:** TSU Notification — Publicly Available Encryption Source Code — HyperMachine v1.0.0

---

Dear Sir or Madam,

Pursuant to 15 CFR §742.15(b) and §740.13(e), this notification is to inform the Bureau of Industry and Security (BIS) and the National Security Agency (NSA) of the public availability of encryption source code.

## Software Identification

| Field | Detail |
|-------|--------|
| **Product Name** | HyperMachine |
| **Version** | 1.0.0 |
| **Developer** | Nervosys (nervosys.ai) |
| **Repository URL** | https://github.com/nervosys/HyperMachine |
| **License** | AGPL-3.0-only (no restrictions on redistribution of source code) |
| **Language** | Rust |
| **Primary Function** | General-purpose hypervisor framework with AI agent support |

## Encryption Functionality

The software contains the following encryption and cryptographic capabilities:

### Symmetric Encryption

- **AES-256-GCM / AES-128-GCM** — Authenticated encryption for VM data protection. Implemented via the `ring` (v0.17) open-source library (ISC license). A software fallback using AES-CTR + HMAC-SHA256 is included for environments without `ring`.

### Asymmetric Encryption

- **RSA-2048 / RSA-3072 / RSA-4096** — Digital signatures via the `ring` library; encryption/decryption via software modular exponentiation implementation.
- **ECDSA P-256 / P-384** — Digital signatures via the `ring` library.

### Key Exchange / Key Derivation

- **HKDF-SHA256** — Key derivation via the `ring` library.

### Hash / MAC

- **SHA-256, SHA-384, SHA-512** — Cryptographic hashing via the `ring` library.
- **HMAC-SHA256, HMAC-SHA512** — Message authentication via the `ring` library.

### Network Encryption

- **TLS 1.2 / 1.3** — HTTPS for REST and gRPC API. Implemented via the `rustls` (v0.23) open-source library (Apache-2.0/ISC/MIT license).

### Security Infrastructure

- **Virtual TPM 2.0** — Software TPM emulation for guest integrity (uses the above crypto primitives).
- **Secure Boot** — UEFI Secure Boot chain verification (uses RSA/ECDSA from above).
- **Memory Encryption Management** — Configuration interface for hardware AMD SEV / Intel TDX encryption engines (does not implement encryption itself).

### Post-Quantum Cryptography (API Prototypes Only)

- ML-KEM, ML-DSA, SLH-DSA API type definitions and simplified placeholder implementations using SHA-256/HMAC. These do **not** contain real lattice-based or hash-tree cryptography and are not suitable for production use.

## Classification

We believe the software is classifiable under **ECCN 5D002** and qualifies for the publicly available source code exception under **15 CFR §742.15(b)** because:

1. The complete source code is publicly available at the URL above.
2. The AGPL-3.0 license imposes no restrictions on further dissemination of the source code.
3. The encryption is not "specifically designed" for non-standard cryptographic functions — all algorithms are NIST standards.

## Third-Party Cryptographic Dependencies

| Library | Version | License | Function |
|---------|---------|---------|----------|
| `ring` | 0.17 | ISC | AES-GCM, SHA-2, HMAC, HKDF, RSA, ECDSA |
| `rustls` | 0.23 | Apache-2.0/ISC/MIT | TLS protocol |
| `rand` | (latest) | MIT/Apache-2.0 | OS CSPRNG access |

## Contact

| | |
|--|--|
| **Organization** | Nervosys |
| **Website** | https://nervosys.ai |

---

*This notification is submitted in compliance with 15 CFR §742.15(b). Please confirm receipt.*
