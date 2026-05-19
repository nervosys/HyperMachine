# HyperMachine — Export Control Posture (EAR / ITAR)

**Project:** HyperMachine (nervosys/HyperMachine)  
**Last reviewed:** 2026-03-25  
**Scope:** Full HyperMachine source tree (all crates, docs, deploy, examples).  
**Classification:** Public — describes export-controlled categories applicable to the open-source release.  

> **Purpose.** This document publicly states HyperMachine's understanding of
> its export-control posture under the U.S. Export Administration Regulations
> (EAR) and International Traffic in Arms Regulations (ITAR). It is a
> technical self-assessment intended to support open-source distribution
> under the publicly available source-code exception (EAR §742.15(b)); it is
> not legal advice. Maintainers and downstream redistributors remain
> responsible for their own classification and licensing decisions. Where
> items are described as "planned" or "in progress", treat them as the
> project's stated roadmap, not as current compliance assertions.

---

## Executive Summary

HyperMachine is a Rust hypervisor framework containing export-controlled
technology in two primary areas:

1. **Cryptography** (EAR Category 5, Part 2) — FIPS 140-3 crypto module with
   symmetric (AES-GCM), asymmetric (RSA, ECDSA), hash (SHA-2), and
   post-quantum (ML-KEM, ML-DSA, SLH-DSA) algorithms.
2. **Information Security Software** (EAR Category 5, Part 2) — TLS, vTPM,
   Secure Boot, and memory encryption management.

The virtualization components (VMX/SVM/EPT/NPT) are **not independently
controlled** under EAR but may be relevant when combined with cryptographic
functionality.

**No ITAR (USML) items were identified.** The software is civilian
general-purpose infrastructure with no military-specific functionality.

The project's position is that the open-source publicly-available
source-code exception under EAR §740.13(e) / §742.15(b) applies to source
releases of the AGPL-3.0 codebase, subject to the BIS notification listed
in §7.1. This position has been reviewed internally by the maintainers; it
has not been adjudicated by counsel and downstream parties should obtain
their own legal analysis if relying on it.

---

## 1. CRYPTOGRAPHIC IMPLEMENTATIONS (EAR Category 5, Part 2)

### 1.1 Symmetric Cryptography — AES-GCM

| Attribute                 | Detail                                                                                                                              |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](crates/hv2-core/src/crypto/fips.rs#L388-L470)                                                  |
| **Algorithm**             | AES-128-GCM, AES-256-GCM                                                                                                            |
| **Key Lengths**           | 128-bit, 256-bit                                                                                                                    |
| **Type**                  | Symmetric AEAD                                                                                                                      |
| **Implementation**        | **DUAL**: (1) Wrapper around `ring 0.17` crate when `ring` feature is enabled; (2) Custom software fallback when `ring` is disabled |
| **Purpose**               | VM data encryption, secure communication, FIPS module                                                                               |
| **FIPS Mode**             | FIPS 140-3 Level 1 targeted                                                                                                         |
| **Likely ECCN**           | 5D002.c.1 — "Information security" software using symmetric >56-bit                                                                 |
| **Open-Source Exception** | Likely eligible under §742.15(b)                                                                                                    |

**Detail:** The `ring` feature path uses `ring::aead::AES_256_GCM` for
authenticated encryption (seal/open). The non-`ring` fallback at
[fips.rs](crates/hv2-core/src/crypto/fips.rs#L437-L470) implements a custom
AES-CTR + HMAC-SHA256 construction (encrypt-then-MAC). The fallback is
explicitly marked "NOT FIPS-certified" in comments.

**Risk:** The custom fallback is a from-scratch construction. While it is
encrypt-then-MAC (correct composition), it is **not** standard AES-GCM and
has not undergone cryptanalysis. Export reviewers may scrutinize custom
crypto more heavily.

---

### 1.2 Hash Functions — SHA-2 Family

| Attribute                 | Detail                                                                                            |
| ------------------------- | ------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](crates/hv2-core/src/crypto/fips.rs#L598-L650)                |
| **Algorithms**            | SHA-256, SHA-384, SHA-512                                                                         |
| **Implementation**        | **Wrapper** around `ring::digest` when `ring` feature enabled; returns `NotImplemented` otherwise |
| **Purpose**               | Integrity verification, KAT self-tests, key derivation                                            |
| **Likely ECCN**           | EAR99 — Hash functions alone are generally not controlled                                         |
| **Open-Source Exception** | N/A (not controlled)                                                                              |

---

### 1.3 HMAC

| Attribute                 | Detail                                                                             |
| ------------------------- | ---------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](crates/hv2-core/src/crypto/fips.rs#L660-L700) |
| **Algorithms**            | HMAC-SHA256, HMAC-SHA512                                                           |
| **Implementation**        | **Wrapper** around `ring::hmac` when `ring` feature enabled                        |
| **Purpose**               | Message authentication, AES-GCM fallback tag, key derivation                       |
| **Likely ECCN**           | Part of 5D002 when used in encryption context                                      |
| **Open-Source Exception** | Likely eligible                                                                    |

---

### 1.4 Key Derivation — HKDF

| Attribute                 | Detail                                                                             |
| ------------------------- | ---------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](crates/hv2-core/src/crypto/fips.rs#L710-L740) |
| **Algorithm**             | HKDF-SHA256 (NIST SP 800-56C)                                                      |
| **Implementation**        | **Wrapper** around `ring::hkdf`                                                    |
| **Purpose**               | Key derivation for encryption keys, PQC key generation                             |
| **Likely ECCN**           | Part of 5D002 when used with controlled encryption                                 |
| **Open-Source Exception** | Likely eligible                                                                    |

---

### 1.5 RSA Asymmetric Cryptography

| Attribute                 | Detail                                                                                                                |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/asymmetric.rs](crates/hv2-core/src/crypto/asymmetric.rs) (full file, ~800 lines)          |
| **Algorithms**            | RSA-2048, RSA-3072, RSA-4096                                                                                          |
| **Operations**            | Key generation (stub), encryption (PKCS#1 v1.5), decryption, signing (PKCS#1/PSS), verification                       |
| **Key Lengths**           | 2048, 3072, 4096 bits                                                                                                 |
| **Type**                  | Asymmetric                                                                                                            |
| **Implementation**        | **MIXED**:                                                                                                            |
|                           | — **Signing/Verification**: Wrapper around `ring::signature::RsaKeyPair` (when `ring` feature enabled)                |
|                           | — **Encryption/Decryption**: **FROM-SCRATCH** software implementation using custom big-integer modular exponentiation |
|                           | — **Key Generation**: Returns `NotImplemented` (ring doesn't support RSA keygen)                                      |
| **Purpose**               | Digital signatures, Secure Boot verification, vTPM                                                                    |
| **Likely ECCN**           | 5D002.c.1 — Asymmetric encryption >512-bit                                                                            |
| **Open-Source Exception** | Likely eligible                                                                                                       |

**Critical Finding — Custom RSA Implementation:**
[asymmetric.rs](crates/hv2-core/src/crypto/asymmetric.rs#L701-L760) contains a
from-scratch `mod_exp_bytes()` function implementing big-integer modular
exponentiation (square-and-multiply) with custom `bytes_to_limbs()`,
`limbs_to_bytes()`, `mod_limbs()`, and `mod_mul_limbs()` helper functions.
This constitutes a **custom RSA encryption/decryption implementation** that
does not depend on any external library. The `rsa_encrypt()` function at
[asymmetric.rs](crates/hv2-core/src/crypto/asymmetric.rs#L260-L295) builds
PKCS#1 v1.5 type-2 padding and calls `mod_exp_bytes()` directly.

**Risk:** This is a **from-scratch implementation** of RSA encryption — the
highest scrutiny category for export control. Custom implementations cannot
rely on the "publicly available library" argument for the external dependency.

---

### 1.6 ECDSA / ECDH Elliptic Curve Cryptography

| Attribute                 | Detail                                                                                         |
| ------------------------- | ---------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/asymmetric.rs](crates/hv2-core/src/crypto/asymmetric.rs#L345-L600) |
| **Algorithms**            | ECDSA P-256/SHA-256, ECDSA P-384/SHA-384, ECDH P-256, ECDH P-384                               |
| **Curves**                | NIST P-256 (secp256r1), P-384 (secp384r1), P-521 (defined but not implemented)                 |
| **Implementation**        | **Wrapper** around `ring::signature::EcdsaKeyPair` for keygen/sign/verify                      |
| **Purpose**               | Digital signatures, key exchange, TLS                                                          |
| **Likely ECCN**           | 5D002.c.1 — Asymmetric encryption >512-bit                                                     |
| **Open-Source Exception** | Likely eligible                                                                                |

**Note:** P-521 is defined in the type system but returns
`UnsupportedAlgorithm` at runtime (ring does not support P-521).

---

### 1.7 TLS Configuration

| Attribute                 | Detail                                                                           |
| ------------------------- | -------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-api/src/tls.rs](crates/hv2-api/src/tls.rs) (entire file, ~130 lines) |
| **Protocol**              | TLS 1.2+ (configured via `rustls 0.23` with `ring` backend)                      |
| **Implementation**        | **Wrapper** — uses `rustls`, `tokio-rustls`, `rustls-pemfile`                    |
| **Cipher Suites**         | Determined by rustls defaults (AES-256-GCM, ChaCha20-Poly1305, ECDHE)            |
| **Purpose**               | HTTPS for REST/gRPC API server                                                   |
| **Likely ECCN**           | 5D002.c.1                                                                        |
| **Open-Source Exception** | Likely eligible (rustls is open-source)                                          |

**Note:** ALPN is configured for HTTP/2 (`h2`) and HTTP/1.1. No custom
cipher suite selection beyond rustls defaults.

---

### 1.8 Post-Quantum Cryptography (PQC)

#### 1.8.1 ML-KEM (CRYSTALS-Kyber) — FIPS 203

| Attribute                 | Detail                                                                                                                                                          |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L25-L160) (types), [pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L340-L420) (implementation) |
| **Parameter Sets**        | ML-KEM-512 (L1), ML-KEM-768 (L3), ML-KEM-1024 (L5)                                                                                                              |
| **Type**                  | Key Encapsulation Mechanism (asymmetric)                                                                                                                        |
| **Implementation**        | **SIMPLIFIED / PLACEHOLDER** — NOT a real ML-KEM implementation                                                                                                 |
| **Purpose**               | Quantum-resistant key exchange                                                                                                                                  |
| **Likely ECCN**           | 5D002.c.1 (asymmetric >512-bit equivalent)                                                                                                                      |
| **Open-Source Exception** | Likely eligible                                                                                                                                                 |

**Critical Finding:** The ML-KEM implementation is **not** a genuine
lattice-based KEM. The `ml_kem_keygen()` function generates random bytes
for keys; `ml_kem_encaps()` derives shared secrets using SHA-256 hashes of
random values concatenated with the public key; `ml_kem_decaps()` similarly
uses SHA-256 of the secret key and ciphertext. **There is no NTT, polynomial
multiplication, or lattice arithmetic.** This is a simplified placeholder
using SHA-256 as a PRF, not a cryptographically valid ML-KEM.

**Export Control Implication:** This is *less* controlled than a real ML-KEM
implementation because it does not implement the actual quantum-resistant
algorithm. However, it claims to be ML-KEM in its API and documentation,
which could create compliance confusion.

#### 1.8.2 ML-DSA (CRYSTALS-Dilithium) — FIPS 204

| Attribute          | Detail                                                                                                                                                           |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv2-core/src/crypto/pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L165-L265) (types), [pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L425-L530) (implementation) |
| **Parameter Sets** | ML-DSA-44 (L2), ML-DSA-65 (L3), ML-DSA-87 (L5)                                                                                                                   |
| **Type**           | Digital Signature Algorithm (asymmetric)                                                                                                                         |
| **Implementation** | **SIMPLIFIED / PLACEHOLDER** — Uses HKDF + SHA-256/512 + HMAC, not real Dilithium                                                                                |
| **Purpose**        | Quantum-resistant signatures                                                                                                                                     |
| **Likely ECCN**    | 5D002 (if real); **EAR99** as implemented (no actual PQC)                                                                                                        |

**Same finding as ML-KEM:** Keygen expands a seed with HKDF; signing uses
HMAC-SHA256(secret_key, SHA-512(message)); verification recomputes using
public key. No polynomial arithmetic, no module-LWE.

#### 1.8.3 SLH-DSA (SPHINCS+) — FIPS 205

| Attribute          | Detail                                                                                                                                                           |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv2-core/src/crypto/pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L270-L340) (types), [pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L535-L610) (implementation) |
| **Parameter Sets** | SHA2-128f/s, SHA2-192f, SHA2-256f, SHAKE-128f, SHAKE-256f                                                                                                        |
| **Type**           | Hash-based Digital Signature (asymmetric)                                                                                                                        |
| **Implementation** | **SIMPLIFIED / PLACEHOLDER** — Uses HMAC-SHA256 chains, not real SPHINCS+                                                                                        |
| **Purpose**        | Stateless quantum-resistant signatures                                                                                                                           |
| **Likely ECCN**    | **EAR99** as implemented (no actual PQC Merkle tree)                                                                                                             |

**Same finding:** No WOTS+, no FORS, no Hypertree. Just HMAC-SHA256
deterministic expansion.

#### 1.8.4 Hybrid Schemes

| Attribute          | Detail                                                                                                                        |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv2-core/src/crypto/pqc.rs](crates/hv2-core/src/crypto/pqc.rs#L300-L330)                                              |
| **Schemes**        | X25519+ML-KEM-768, ECDH-P256+ML-KEM-768, ECDH-P384+ML-KEM-1024, ECDSA-P256+ML-DSA-44, ECDSA-P384+ML-DSA-65, Ed25519+ML-DSA-65 |
| **Implementation** | **Type definitions only** — No implementation code found                                                                      |
| **Likely ECCN**    | N/A (not implemented)                                                                                                         |

---

## 2. VIRTUALIZATION TECHNOLOGY

### 2.1 Intel VT-x (VMX) — Type-1 Hypervisor

| Attribute          | Detail                                                                             |
| ------------------ | ---------------------------------------------------------------------------------- |
| **Files**          | [crates/hv1-core/src/vmx.rs](crates/hv1-core/src/vmx.rs) (~600+ lines)             |
| **Instructions**   | VMXON, VMXOFF, VMCLEAR, VMPTRLD, VMPTRST, VMREAD, VMWRITE, VMLAUNCH, VMRESUME      |
| **Features**       | Full VMCS management, EPT configuration, VPID, posted interrupts, preemption timer |
| **Implementation** | **From-scratch** inline assembly (`core::arch::asm!`, `naked_asm!`)                |
| **Likely ECCN**    | **Not independently controlled**                                                   |

The VMX code at [vmx.rs](crates/hv1-core/src/vmx.rs#L456-L585) contains
direct `vmxon`, `vmclear`, `vmptrld`, `vmread`, `vmwrite`, `vmlaunch`
inline assembly instructions. VMCS field encodings cover the complete Intel
specification (~160 fields). This is a **complete VMX implementation**.

### 2.2 AMD-V (SVM) — Type-1 Hypervisor

| Attribute          | Detail                                                                         |
| ------------------ | ------------------------------------------------------------------------------ |
| **Files**          | [crates/hv1-core/src/svm.rs](crates/hv1-core/src/svm.rs) (~500+ lines)         |
| **Instructions**   | VMRUN, VMSAVE, VMLOAD, STGI, CLGI, SKINIT                                      |
| **Features**       | Full VMCB management, NPT, ASID, intercept configuration, decrypt-assist, AVIC |
| **Implementation** | **From-scratch** inline assembly                                               |
| **Likely ECCN**    | **Not independently controlled**                                               |

### 2.3 Nested Virtualization

| Attribute          | Detail                                                                                                                                    |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv2-core/src/nested/](crates/hv2-core/src/nested/) (5 files: mod.rs, types.rs, shadow_vmcs.rs, ept.rs, manager.rs)                |
| **Features**       | L0/L1/L2 model, shadow VMCS, nested EPT, VMX instruction emulation (VMXON/OFF, VMPTRLD/ST, VMREAD/WRITE, VMLAUNCH/RESUME, INVEPT/INVVPID) |
| **Implementation** | **From-scratch** software emulation of VMX for nested guests                                                                              |
| **Likely ECCN**    | **Not independently controlled**                                                                                                          |

### 2.4 Extended Page Tables (EPT) / Nested Page Tables (NPT)

| Attribute          | Detail                                                                                           |
| ------------------ | ------------------------------------------------------------------------------------------------ |
| **Files**          | [crates/hv2-core/src/nested/ept.rs](crates/hv2-core/src/nested/ept.rs) (~250+ lines)             |
| **Features**       | 4-level EPT walk, 4K/2M/1G pages, memory type control, access/dirty bits, EPT violation handling |
| **Implementation** | **From-scratch**                                                                                 |
| **Likely ECCN**    | **Not independently controlled**                                                                 |

### 2.5 ARM64 EL2 Hypervisor

| Attribute          | Detail                                                                                            |
| ------------------ | ------------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv1-arm/](crates/hv1-arm/) (7 modules: el2, stage2, sysreg, vcpu, vgic, vm, error)        |
| **Features**       | EL2 exception handling, Stage-2 address translation, vGIC (GICv2/GICv3), system register trapping |
| **Implementation** | **From-scratch** `#![no_std]`, uses `aarch64-cpu` and `tock-registers` crates                     |
| **Likely ECCN**    | **Not independently controlled**                                                                  |

### 2.6 Virtualization — EAR Classification Analysis

Hypervisor/virtualization technology is **generally not independently
controlled** under EAR. VMX, SVM, EPT, and NPT are standard hardware
features documented in public Intel/AMD manuals. However:

- When combined with **encryption** (AES-GCM, memory encryption), the
  software falls under ECCN 5D002.
- When combined with **information security functionality** (vTPM, Secure
  Boot), it may be classified as ECCN 5D002.
- The **nested virtualization** feature (running hypervisors inside guests)
  could theoretically be relevant for obfuscation/evasion but is a standard
  virtualization feature available in commercial products (VMware, Hyper-V, KVM).

---

## 3. SECURITY INFRASTRUCTURE

### 3.1 Virtual TPM (vTPM 2.0)

| Attribute                 | Detail                                                                                                                                                                                      |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/security/vtpm.rs](crates/hv2-core/src/security/vtpm.rs) (~500+ lines)                                                                                                  |
| **Features**              | TPM 2.0 command processing, PCR banks (SHA-1/256/384/512/SM3), NV storage, key management (RSA/ECC/Symmetric), cryptographic operations (CreatePrimary, Sign, VerifySignature, Hash, Quote) |
| **Implementation**        | **From-scratch** software TPM emulation                                                                                                                                                     |
| **Likely ECCN**           | 5D002 (provides authentication/integrity services)                                                                                                                                          |
| **Open-Source Exception** | Likely eligible                                                                                                                                                                             |

**Note:** PCR extend uses XOR (simplified), not proper hash chaining.
Comments explicitly note "Simplified: XOR for demonstration." The TPM
command dispatch and key hierarchy are structurally complete but the
underlying crypto operations delegate to the FIPS module.

### 3.2 Secure Boot

| Attribute                 | Detail                                                                                                                                                              |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/security/secure_boot.rs](crates/hv2-core/src/security/secure_boot.rs) (~450+ lines)                                                            |
| **Features**              | UEFI Secure Boot chain (PK, KEK, db, dbx), X.509 certificate management, boot component verification, signature verification (RSA-SHA256/384/512, ECDSA-SHA256/384) |
| **Implementation**        | **From-scratch** — certificate and signature structures, verification chain logic                                                                                   |
| **Likely ECCN**           | Part of 5D002 (authentication)                                                                                                                                      |
| **Open-Source Exception** | Likely eligible                                                                                                                                                     |

### 3.3 Memory Encryption Management (SEV/TDX)

| Attribute                 | Detail                                                                                                               |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/security/memory_encryption.rs](crates/hv2-core/src/security/memory_encryption.rs) (~350+ lines) |
| **Technologies**          | AMD SEV, SEV-ES, SEV-SNP, Intel TDX, Intel MKTME                                                                     |
| **Features**              | C-bit position management, encryption key lifecycle, page encryption state tracking, attestation support             |
| **Implementation**        | **Software management layer** — does not implement encryption itself; manages hardware encryption states             |
| **Likely ECCN**           | Part of 5D002 (manages encryption keys/states)                                                                       |
| **Open-Source Exception** | Likely eligible                                                                                                      |

**Note:** The encryption is performed by **hardware** (AMD SEV / Intel TDX
engines). This module manages key IDs, page states, and configuration — it
does not perform software encryption of memory contents.

---

## 4. ITAR ANALYSIS (International Traffic in Arms Regulations)

### 4.1 USML Category Search Results

| Search Term                       | Findings                                                                                                                                       |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| "weapon" / "missile" / "munition" | **None**                                                                                                                                       |
| "military" / "defense"            | References only in CMMC compliance docs (marketing to DoD contractors) and MITRE ATT&CK mapping ("Defense Evasion" — a cybersecurity category) |
| "satellite" / "space"             | **None**                                                                                                                                       |
| "ITAR" / "USML"                   | **None**                                                                                                                                       |
| "Category XI" / "Category XIII"   | **None**                                                                                                                                       |

### 4.2 ITAR Determination

**HyperMachine is NOT subject to ITAR.** Rationale:

1. **No military-specific functionality:** The software is a general-purpose
   hypervisor for AI workloads. It has no weapons control, target tracking,
   missile guidance, or defense-specific capabilities.
2. **No USML-listed items:** No satellite communications, no military
   electronics (Category XI), no auxiliary military equipment (Category XIII).
3. **Civilian dual-use only:** The CMMC compliance documentation is for
   marketing to DoD contractors using the software as IT infrastructure,
   not as a weapons system.
4. **Public availability:** The software has a public GitHub repository URL
   (`https://github.com/nervosys/HyperMachine`).

**Recommendation:** ITAR does not apply. The software falls under EAR
jurisdiction (Commerce Department), not ITAR (State Department).

---

## 5. OPEN-SOURCE EXCEPTION ANALYSIS (EAR §740.13(e) / §742.15(b))

### 5.1 License

- **Primary License:** AGPL-3.0-only (GNU Affero General Public License v3)
- **Alternative License:** Commercial dual-license (`LicenseRef-Commercial`)
- **Source Repository:** `https://github.com/nervosys/HyperMachine`

### 5.2 Open-Source Exception Requirements

Under 15 CFR §742.15(b), encryption source code that is "publicly
available" is released from EAR controls provided:

| Requirement                                                   | Status                                                                                           |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Source code is publicly available                             | ✅ The canonical repository at <https://github.com/nervosys/HyperMachine> is public.              |
| No restrictions on further dissemination                      | ✅ AGPL-3.0 permits unlimited redistribution of source                                            |
| BIS notification (email to crypt@bis.doc.gov and enc@nsa.gov) | 🔄 Tracked as roadmap item **R-1** in §7.1 — to be filed prior to the next signed binary release. |
| No "specifically designed" for non-standard crypto            | ✅ Uses standard NIST algorithms                                                                  |

### 5.3 Exception Applicability

Given that the GitHub repository is public, the project's position is that
the publicly available source-code exception under EAR §742.15(b) applies
to the AGPL-3.0 licensed source release once item **R-1** in §7.1 is
complete. Notes:

1. **BIS notification (roadmap item R-1).** Per §742.15(b), nervosys will
   send an email to `crypt@bis.doc.gov` and `enc@nsa.gov` containing:
   - The URL of the publicly available source code
   - A brief description of the encryption functionality

2. **The commercial license version is analyzed separately.** Distributions
   under `LicenseRef-Commercial` that restrict redistribution may not
   qualify for the open-source exception and are tracked as roadmap item
   **R-2** in §7.1.

3. **Object code / compiled binaries** are not covered by the publicly
   available source-code exception. Binary distributions are tracked as
   roadmap item **R-2** for separate classification.

### 5.4 Third-Party Dependency Analysis

| Dependency          | Role                                   | License            | Publicly Available |
| ------------------- | -------------------------------------- | ------------------ | ------------------ |
| `ring 0.17`         | AES-GCM, SHA-2, HMAC, HKDF, ECDSA, RSA | ISC                | ✅ Yes (GitHub)     |
| `rustls 0.23`       | TLS protocol                           | Apache-2.0/ISC/MIT | ✅ Yes (GitHub)     |
| `tokio-rustls 0.26` | Async TLS                              | MIT/Apache-2.0     | ✅ Yes              |
| `rustls-pemfile 2`  | PEM parsing                            | MIT/Apache-2.0     | ✅ Yes              |
| `rand`              | RNG (OS CSPRNG)                        | MIT/Apache-2.0     | ✅ Yes              |

All cryptographic dependencies are publicly available open-source libraries.

---

## 6. ECCN CLASSIFICATION SUMMARY

| Component                                    | Likely ECCN    | Rationale                             | Exception              |
| -------------------------------------------- | -------------- | ------------------------------------- | ---------------------- |
| AES-128/256-GCM (ring wrapper)               | 5D002.c.1      | Symmetric encryption >56-bit          | §742.15(b) open-source |
| AES-CTR+HMAC fallback (custom)               | 5D002.c.1      | Custom symmetric encryption >56-bit   | §742.15(b) if public   |
| RSA encrypt/decrypt (custom `mod_exp_bytes`) | 5D002.c.1      | Custom asymmetric encryption >512-bit | §742.15(b) if public   |
| RSA sign/verify (ring wrapper)               | 5D002.c.1      | Asymmetric >512-bit                   | §742.15(b) open-source |
| ECDSA P-256/P-384 (ring wrapper)             | 5D002.c.1      | Asymmetric >512-bit equiv             | §742.15(b) open-source |
| SHA-256/384/512 (ring wrapper)               | EAR99          | Hash functions                        | No license needed      |
| HMAC-SHA256/512 (ring wrapper)               | Part of 5D002  | Authentication in crypto context      | §742.15(b)             |
| HKDF-SHA256 (ring wrapper)                   | Part of 5D002  | Key derivation                        | §742.15(b)             |
| TLS (rustls wrapper)                         | 5D002.c.1      | Network encryption                    | §742.15(b) open-source |
| PQC ML-KEM (placeholder)                     | EAR99          | Not real PQC (SHA-256 PRF only)       | N/A                    |
| PQC ML-DSA (placeholder)                     | EAR99          | Not real PQC (HMAC chain only)        | N/A                    |
| PQC SLH-DSA (placeholder)                    | EAR99          | Not real PQC (HMAC chain only)        | N/A                    |
| vTPM 2.0                                     | 5D002          | Authentication/integrity services     | §742.15(b)             |
| Secure Boot                                  | Part of 5D002  | Authentication chain                  | §742.15(b)             |
| Memory Encryption Mgmt                       | Part of 5D002  | Manages HW encryption keys            | §742.15(b)             |
| VMX/SVM hypervisor                           | Not controlled | Standard virtualization               | N/A                    |
| Nested virtualization                        | Not controlled | Standard virtualization               | N/A                    |
| EPT/NPT/Stage-2                              | Not controlled | Standard memory virtualization        | N/A                    |
| ARM64 EL2                                    | Not controlled | Standard ARM virtualization           | N/A                    |
| GPU virtualization                           | Not controlled | No crypto in GPU crate                | N/A                    |

---

## 7. KNOWN LIMITATIONS & ROADMAP

The items below capture known limitations of the current release and the
planned compliance work. They are published openly so that downstream users
and security reviewers can make informed decisions; they are not blockers
on source-code distribution under the open-source exception.

### 7.1 Compliance Roadmap (R-series)

| #       | Item                                                                                                                                                                                            | Plan                                                                                                                                                                                            |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **R-1** | **BIS notification for the open-source exception.** EAR §742.15(b) requires emailing `crypt@bis.doc.gov` and `enc@nsa.gov` with the repository URL and an encryption-functionality description. | File the notification prior to the next signed binary release. Notification text and acknowledgement will be archived under [docs/security/](.).                                                |
| **R-2** | **Commercial-license distributions** under `LicenseRef-Commercial` and signed binary distributions are out of scope of the source-only exception.                                               | Obtain independent legal analysis and, if required, a BIS classification (CCATS) for binary / commercial distributions before publishing them. Source releases under AGPL-3.0 are not affected. |
| **R-3** | **CMVP-validated FIPS 140-3 module.** The crypto module is FIPS-architected (NIST-approved algorithms via `ring`) but has not been submitted for CMVP validation.                               | Tracked in [FIPS_COMPLIANCE.md](FIPS_COMPLIANCE.md). All public docs and code comments use the phrase "FIPS 140-3 architecture; not yet CMVP-certified" to avoid misrepresentation.             |

### 7.2 Known Source-Code Limitations (K-series)

These are honest disclosures of the current implementation — they affect
production-readiness, not the export-control status of the source release.

| #       | Limitation                                                                                                                                                                                                        | Mitigation in current release                                                                                                                                                                                 |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **K-1** | **Custom RSA encrypt/decrypt** ([asymmetric.rs](../../crates/hv2-core/src/crypto/asymmetric.rs)): `mod_exp_bytes()` is a from-scratch big-integer modular exponentiation, not a validated library implementation. | Production deployments should enable the `ring` feature for all RSA signing/verification. The custom path is documented as non-production and is the leading candidate for removal in a future release.       |
| **K-2** | **Custom AES-CTR+HMAC fallback** in `fips.rs` (non-`ring` path) implements encrypt-then-MAC composition without a validated library. Comments mark it as not FIPS-certified.                                      | Default builds enable `ring`. The fallback exists only for `no_std` / cross-compile experimentation; it will be moved behind a `dangerous-fallback` feature flag with compile-time warnings.                  |
| **K-3** | **PQC modules (`pqc.rs`) implement API stubs**, not the FIPS 203 / 204 / 205 algorithms. Names and types match the standards but the underlying math is a SHA-256 / HMAC-SHA256 PRF chain.                        | The module is marked as an API preview in code comments and `FIPS_COMPLIANCE.md`. Real implementations will integrate `pqcrypto` or `oqs-rs`; until then, do not use these APIs for cryptographic protection. |
| **K-4** | **vTPM PCR extend uses XOR** instead of proper hash chaining. Documented inline as "simplified for demonstration".                                                                                                | Correctness issue, not an export-control issue. Tracked in the issue tracker for replacement with proper SHA-2 chaining.                                                                                      |
| **K-5** | **SM3 hash algorithm** appears in the vTPM `HashAlgorithm` enum for TPM 2.0 specification completeness.                                                                                                           | SM3 is included for protocol parsing only; no HyperMachine security function uses SM3 to provide confidentiality, integrity, or authentication.                                                               |
| **K-6** | The `ring` feature is currently optional. When disabled, most crypto APIs return `NotImplemented`, which can be confusing.                                                                                        | A future release will make `ring` a default feature so production builds always link a validated crypto provider.                                                                                             |

---

## 8. COMPLETE CRYPTOGRAPHIC INVENTORY

### 8.1 Algorithms with Active Implementations

| Algorithm                  | Key Length    | Sym/Asym   | File          | Wrapper vs Custom | External Dep |
| -------------------------- | ------------- | ---------- | ------------- | ----------------- | ------------ |
| AES-256-GCM                | 256-bit       | Symmetric  | fips.rs       | Wrapper (ring)    | ring 0.17    |
| AES-128-GCM                | 128-bit       | Symmetric  | fips.rs       | Wrapper (ring)    | ring 0.17    |
| AES-CTR+HMAC fallback      | 128/256-bit   | Symmetric  | fips.rs       | **Custom**        | None         |
| RSA-2048/3072/4096 sign    | 2048-4096-bit | Asymmetric | asymmetric.rs | Wrapper (ring)    | ring 0.17    |
| RSA-2048/3072/4096 verify  | 2048-4096-bit | Asymmetric | asymmetric.rs | Wrapper (ring)    | ring 0.17    |
| RSA-2048/3072/4096 encrypt | 2048-4096-bit | Asymmetric | asymmetric.rs | **Custom**        | None         |
| RSA-2048/3072/4096 decrypt | 2048-4096-bit | Asymmetric | asymmetric.rs | **Custom**        | None         |
| ECDSA P-256/SHA-256        | 256-bit       | Asymmetric | asymmetric.rs | Wrapper (ring)    | ring 0.17    |
| ECDSA P-384/SHA-384        | 384-bit       | Asymmetric | asymmetric.rs | Wrapper (ring)    | ring 0.17    |
| SHA-256                    | N/A           | Hash       | fips.rs       | Wrapper (ring)    | ring 0.17    |
| SHA-384                    | N/A           | Hash       | fips.rs       | Wrapper (ring)    | ring 0.17    |
| SHA-512                    | N/A           | Hash       | fips.rs       | Wrapper (ring)    | ring 0.17    |
| HMAC-SHA256                | 256-bit       | MAC        | fips.rs       | Wrapper (ring)    | ring 0.17    |
| HMAC-SHA512                | 512-bit       | MAC        | fips.rs       | Wrapper (ring)    | ring 0.17    |
| HKDF-SHA256                | Variable      | KDF        | fips.rs       | Wrapper (ring)    | ring 0.17    |
| TLS 1.2/1.3                | Various       | Protocol   | tls.rs        | Wrapper (rustls)  | rustls 0.23  |
| RNG (OS CSPRNG)            | N/A           | Random     | fips.rs       | Wrapper (rand)    | rand crate   |

### 8.2 Algorithms Defined but Not Truly Implemented (Placeholders)

| Algorithm                | Declared Standard | Actual Implementation              | File          |
| ------------------------ | ----------------- | ---------------------------------- | ------------- |
| ML-KEM-512/768/1024      | FIPS 203          | SHA-256 PRF (no lattice math)      | pqc.rs        |
| ML-DSA-44/65/87          | FIPS 204          | HMAC-SHA256 chain (no module-LWE)  | pqc.rs        |
| SLH-DSA variants         | FIPS 205          | HMAC-SHA256 chain (no Merkle tree) | pqc.rs        |
| Hybrid KEM schemes       | N/A               | Type definitions only              | pqc.rs        |
| Hybrid signature schemes | N/A               | Type definitions only              | pqc.rs        |
| RSA key generation       | FIPS 186-5        | Returns `NotImplemented`           | asymmetric.rs |
| ECDSA P-521              | FIPS 186-5        | Returns `UnsupportedAlgorithm`     | asymmetric.rs |

---

## 9. OVERALL CLASSIFICATION DETERMINATION

**Recommended ECCN: 5D002.c.1**

> "Information security" software not controlled by ECCN 5D002.a, that
> provides or performs "cryptographic activation" of commodities or
> software using "non-standard cryptography."

The software:
1. Implements encryption (AES-GCM, RSA) with key lengths >56-bit symmetric /
   >512-bit asymmetric
2. Provides TLS network encryption
3. Includes authentication services (vTPM, Secure Boot, digital signatures)

The project's position is that the open-source exception under EAR
§742.15(b) applies to source releases. Conditions:
1. Source code is publicly available — ✅ the canonical repository is
   public at <https://github.com/nervosys/HyperMachine>.
2. BIS notification is filed — tracked as roadmap item **R-1** in §7.1.
3. No restrictions on redistribution of source code — ✅ AGPL-3.0 satisfies
   this.

Binary distributions and commercial-license distributions are tracked
separately under roadmap item **R-2**.

---

*This document is the project's public technical self-assessment and does
not constitute legal advice. Downstream redistributors should consult
qualified export-control counsel for formal classification determinations
and compliance obligations applicable to their own jurisdiction.*
