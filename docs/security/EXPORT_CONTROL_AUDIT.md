# HyperMachine — Export Control Posture (EAR / ITAR)

**Project:** HyperMachine (nervosys/HyperMachine)  
**Last reviewed:** 2026-09-28 (implementation facts in §1, §3, §5.4, §6-§9; the legal positions are unchanged)  
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

> **Revised 2026-09-28.** The version reviewed on 2026-03-25 described
> `ring` wrappers, a custom AES-CTR+HMAC fallback, a from-scratch RSA and
> placeholder post-quantum code. None of those remains. Since 93abff0 every
> classical primitive in `hv2-core` and `hv2-api` comes from **IronCrypto**
> (`ic-*` crates, AGPL-3.0-or-later, published on crates.io), which is
> checked here against published test vectors. The post-quantum algorithms
> come from the RustCrypto `ml-kem`, `ml-dsa` and `slh-dsa` crates (59ad48a);
> since 2026-09-29 ML-KEM and ML-DSA are IronCrypto's `ic-mlkem` and
> `ic-mldsa`, and only SLH-DSA remains RustCrypto's.
> `ring` remains in the build only beneath `rustls` (TLS), `rcgen`,
> `quinn-proto` and `x509-parser`. Where a classification below rested on a
> fact that has since changed, it is marked for re-review rather than
> reassigned here.

### 1.1 Symmetric Cryptography — AES-GCM

| Attribute                 | Detail                                                                                          |
| ------------------------- | ----------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](../../crates/hv2-core/src/crypto/fips.rs)                  |
| **Algorithm**             | AES-128-GCM, AES-256-GCM (NIST SP 800-38D)                                                      |
| **Key Lengths**           | 128-bit, 256-bit                                                                                |
| **Type**                  | Symmetric AEAD                                                                                  |
| **Implementation**        | **Wrapper** around IronCrypto `ic_cipher::Aes128Gcm` / `Aes256Gcm`; no software fallback exists |
| **Purpose**               | VM data encryption, secure communication, FIPS module                                           |
| **FIPS Mode**             | FIPS 140-3 architecture; not CMVP-certified (R-3)                                               |
| **Likely ECCN**           | 5D002.c.1 — "Information security" software using symmetric >56-bit                             |
| **Open-Source Exception** | Likely eligible under §742.15(b)                                                                |

**Detail:** Both key sizes are checked against the GCM specification's
published vectors (Test Cases 2 and 14 / SP 800-38D). The custom fallback the
2026-03-25 review described was deleted in 59ad48a. Despite its name it was not
AES: its keystream was `SHA256(key || nonce || counter)`. AES-128-GCM was
refused at runtime from 93abff0 until 2026-09-28, because the internals built
an AES-256 cipher for every key. It now works.

---

### 1.2 Hash Functions — SHA-2 Family

| Attribute                 | Detail                                                                         |
| ------------------------- | ------------------------------------------------------------------------------ |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](../../crates/hv2-core/src/crypto/fips.rs) |
| **Algorithms**            | SHA-256, SHA-384, SHA-512 (FIPS 180-4)                                         |
| **Implementation**        | **Wrapper** around IronCrypto `ic_hash`; unconditional (no feature gate)       |
| **Purpose**               | Integrity verification, KAT self-tests, key derivation, vTPM PCR chaining      |
| **Likely ECCN**           | EAR99 — Hash functions alone are generally not controlled                      |
| **Open-Source Exception** | N/A (not controlled)                                                           |

---

### 1.3 HMAC

| Attribute                 | Detail                                                                                                                                                      |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](../../crates/hv2-core/src/crypto/fips.rs), [crates/hv2-api/src/middleware.rs](../../crates/hv2-api/src/middleware.rs) |
| **Algorithms**            | HMAC-SHA256, HMAC-SHA512 (FIPS 198-1)                                                                                                                       |
| **Implementation**        | **Wrapper** around IronCrypto `ic_mac`; checked against RFC 4231                                                                                            |
| **Purpose**               | Message authentication, key derivation, API request/response signing (`X-Signature`, off by default)                                                       |
| **Likely ECCN**           | Part of 5D002 when used in encryption context                                                                                                               |
| **Open-Source Exception** | Likely eligible                                                                                                                                             |

---

### 1.4 Key Derivation — HKDF

| Attribute                 | Detail                                                                         |
| ------------------------- | ------------------------------------------------------------------------------ |
| **Files**                 | [crates/hv2-core/src/crypto/fips.rs](../../crates/hv2-core/src/crypto/fips.rs) |
| **Algorithm**             | HKDF-SHA256 (RFC 5869; NIST SP 800-56C)                                        |
| **Implementation**        | **Wrapper** around IronCrypto `ic_kdf::Hkdf`; checked against RFC 5869         |
| **Purpose**               | Key derivation for encryption keys                                             |
| **Likely ECCN**           | Part of 5D002 when used with controlled encryption                             |
| **Open-Source Exception** | Likely eligible                                                                |

---

### 1.5 RSA Asymmetric Cryptography

| Attribute                 | Detail                                                                                             |
| ------------------------- | -------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/asymmetric.rs](../../crates/hv2-core/src/crypto/asymmetric.rs)         |
| **Algorithms**            | RSA-2048, RSA-3072, RSA-4096                                                                       |
| **Operations**            | Key generation, signing, verification. **No encryption or decryption.** |
| **Key Lengths**           | 2048, 3072, 4096 bits                                                                              |
| **Type**                  | Asymmetric (digital signature)                                                                     |
| **Implementation**        | **Wrapper** around IronCrypto `ic_rsa` (`generate`, `RsaPrivateKey`, `RsaPublicKey`)               |
| **Purpose**               | Digital signatures                                                                                 |
| **Likely ECCN**           | **Re-review.** Recorded as 5D002.c.1 for "asymmetric encryption"; RSA now performs signatures only |
| **Open-Source Exception** | Likely eligible                                                                                    |

**Resolved (814d0c0):** the from-scratch `mod_exp_bytes()` RSA
encryption/decryption the 2026-03-25 review flagged as its critical finding
was deleted, not fixed. It computed wrong results (4^13 mod 497 gave 121, not
445) and had no callers. No ECDH is implemented either, so ML-KEM (§1.8.1) is
the only key-establishment mechanism.

---

### 1.6 ECDSA Elliptic Curve Cryptography

| Attribute                 | Detail                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------ |
| **Files**                 | [crates/hv2-core/src/crypto/asymmetric.rs](../../crates/hv2-core/src/crypto/asymmetric.rs) |
| **Algorithms**            | ECDSA P-256/SHA-256, P-384/SHA-384, P-521/SHA-512 (FIPS 186-5)                             |
| **Operations**            | Key generation, signing, verification. **No ECDH** (named in comments, not implemented)    |
| **Implementation**        | **Wrapper** around IronCrypto `ic_ec`                                                      |
| **Purpose**               | Digital signatures                                                                         |
| **Likely ECCN**           | **Re-review.** Recorded as 5D002.c.1; ECDSA here performs signatures only                  |
| **Open-Source Exception** | Likely eligible                                                                            |

**Note:** P-521, which the 2026-03-25 review recorded as unsupported, is
implemented. Its key generation masks the scalar's excess bits; before that
fix about 46% of P-521 key generations failed.

---

### 1.7 TLS

| Attribute                 | Detail                                                                                                                                                           |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-api/src/tls.rs](../../crates/hv2-api/src/tls.rs); `hv2-net`'s egress gateway ([gateway/mitm.rs](../../crates/hv2-net/src/gateway/mitm.rs)); `hm-cli` |
| **Protocol**              | TLS 1.2/1.3 via `rustls 0.23` with its `ring` backend                                                                                                            |
| **Implementation**        | **Wrapper** — `rustls`, `tokio-rustls`, `rustls-pki-types`; `rcgen` for certificate generation                                                                   |
| **Cipher Suites**         | rustls defaults (AES-128/256-GCM, ChaCha20-Poly1305, ECDHE key exchange)                                                                                         |
| **Purpose**               | HTTPS for the REST/gRPC API; the sandbox egress gateway's TLS interception                                                                                       |
| **Likely ECCN**           | 5D002.c.1                                                                                                                                                        |
| **Open-Source Exception** | Likely eligible (rustls is open-source)                                                                                                                          |

**TLS interception (new since 2026-03-25):** for a sandbox whose network
policy has header-injection rules, the `hv2-net` egress gateway terminates the
sandbox's outbound TLS with a per-sandbox CA it generates (ECDSA P-256, via
`rcgen`). It re-originates the connection upstream with ordinary rustls
verification. The guest trusts that CA only because the gateway installs it
in that guest. This lets an operator inject credentials the sandbox never
sees. It is described here because it is encryption functionality a reviewer
would otherwise not expect in a hypervisor.

---

### 1.8 Post-Quantum Cryptography (PQC)

> **Resolved (59ad48a):** the 2026-03-25 review found all three algorithms
> to be SHA-256/HMAC placeholders and classified two of them EAR99 on that
> basis. They are now real implementations from the RustCrypto project, on
> by default (`pqc` feature). **The ECCN entries below that rested on "no
> actual PQC" need re-review.**

#### 1.8.1 ML-KEM — FIPS 203

| Attribute                 | Detail                                                                       |
| ------------------------- | ---------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/crypto/pqc.rs](../../crates/hv2-core/src/crypto/pqc.rs) |
| **Parameter Sets**        | ML-KEM-512, ML-KEM-768, ML-KEM-1024                                          |
| **Type**                  | Key Encapsulation Mechanism (key establishment)                              |
| **Implementation**        | **Wrapper** around IronCrypto's `ic-mlkem` (RustCrypto `ml-kem` until 2026-09-29) |
| **Purpose**               | Quantum-resistant key establishment                                          |
| **Likely ECCN**           | 5D002.c.1 (key establishment) — re-review; previously EAR99 as a placeholder |
| **Open-Source Exception** | Likely eligible                                                              |

#### 1.8.2 ML-DSA — FIPS 204

| Attribute          | Detail                                                                               |
| ------------------ | ------------------------------------------------------------------------------------ |
| **Files**          | [crates/hv2-core/src/crypto/pqc.rs](../../crates/hv2-core/src/crypto/pqc.rs)         |
| **Parameter Sets** | ML-DSA-44, ML-DSA-65, ML-DSA-87                                                      |
| **Type**           | Digital signature                                                                    |
| **Implementation** | **Wrapper** around IronCrypto's `ic-mldsa` (RustCrypto `ml-dsa` until 2026-09-29)    |
| **Purpose**        | Quantum-resistant signatures                                                         |
| **Likely ECCN**    | **Re-review** — previously EAR99 because "no actual PQC"; that is no longer the case |

IronCrypto's `ic-mldsa` passes the NIST ACVP vectors for all three parameter
sets, and `hv2-core` uses it since 2026-09-29. No new cryptographic
function: the same three algorithms from a different implementation.

#### 1.8.3 SLH-DSA — FIPS 205

| Attribute          | Detail                                                                                        |
| ------------------ | --------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv2-core/src/crypto/pqc.rs](../../crates/hv2-core/src/crypto/pqc.rs)                  |
| **Parameter Sets** | SHA2-128f/s, SHA2-192f, SHA2-256f, SHAKE-128f, SHAKE-256f                                     |
| **Type**           | Hash-based digital signature                                                                  |
| **Implementation** | **Wrapper** around the RustCrypto `slh-dsa` crate (pinned `=0.2.0-rc.5`, a release candidate) |
| **Purpose**        | Stateless quantum-resistant signatures                                                        |
| **Likely ECCN**    | **Re-review** — previously EAR99 because "no actual PQC"; that is no longer the case          |

#### 1.8.4 Hybrid Schemes

| Attribute          | Detail                                                                                                                        |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| **Files**          | [crates/hv2-core/src/crypto/pqc.rs](../../crates/hv2-core/src/crypto/pqc.rs)                                                  |
| **Schemes**        | X25519+ML-KEM-768, ECDH-P256+ML-KEM-768, ECDH-P384+ML-KEM-1024, ECDSA-P256+ML-DSA-44, ECDSA-P384+ML-DSA-65, Ed25519+ML-DSA-65 |
| **Implementation** | **Type definitions only** — unchanged; no implementation code                                                                  |
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

**Note (revised 2026-09-28):** PCR extend is a SHA-2 hash chain,
`new = H(old || data)`, over IronCrypto, with property tests (922761f). Until
then it was XOR, which made every register forgeable. No guest can reach the
vTPM today: it is not wired to a device model, so it provides no service to a
running guest.

**Note (2026-09-29):** the "command processing" above is now real for seven
commands: `VirtualTpm::execute` parses TPM 2.0 wire-format buffers for
Startup, Shutdown, SelfTest, GetRandom, GetCapability, PCR_Read and
PCR_Extend. CreatePrimary, Sign, VerifySignature, Hash and Quote are not
dispatched and answer `TPM_RC_COMMAND_CODE`. No new cryptography: PCR
extension is the existing SHA-2 chain, and GetRandom is IronCrypto's
HMAC_DRBG.

### 3.2 Secure Boot

| Attribute                 | Detail                                                                                                                                                              |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Files**                 | [crates/hv2-core/src/security/secure_boot.rs](crates/hv2-core/src/security/secure_boot.rs) (~450+ lines)                                                            |
| **Features**              | UEFI Secure Boot chain (PK, KEK, db, dbx), X.509 certificate management, boot component verification, signature verification (RSA-SHA256/384/512, ECDSA-SHA256/384) |
| **Implementation**        | **From-scratch** — certificate and signature structures, verification chain logic                                                                                   |
| **Likely ECCN**           | Part of 5D002 (authentication)                                                                                                                                      |
| **Open-Source Exception** | Likely eligible                                                                                                                                                     |

**Note (revised 2026-09-28):** signatures are verified: RSA PKCS#1 v1.5
(SHA-256/384/512) and ECDSA P-256/SHA-256 and P-384/SHA-384, through the
IronCrypto primitives in §1.5-§1.6. Each is checked against the trusted
database entry's key, never one supplied with the signature. This is
authentication, not encryption. Until 2026-09-22 the path admitted any
claimed trusted name; then it refused everything until verification landed
on 2026-09-28.

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

**Revised 2026-09-28:** no hardware backend exists, so `EncryptionManager::enable`
refuses on every host, including one with SEV-SNP. Until 2026-09-22 it set a
flag and reported memory as encrypted. No memory encryption is managed or
performed today.

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
| `ic-cipher`, `ic-hash`, `ic-mac`, `ic-kdf`, `ic-rsa`, `ic-ec`, `ic-core` (IronCrypto) | AES-GCM, SHA-2, HMAC, HKDF, RSA signatures, ECDSA | AGPL-3.0-or-later | ✅ Yes (crates.io) |
| `slh-dsa` (RustCrypto) | SLH-DSA | MIT/Apache-2.0 | ✅ Yes (crates.io) |
| `rustls 0.23` (with its `ring 0.17` backend) | TLS protocol | Apache-2.0/ISC/MIT | ✅ Yes (GitHub) |
| `tokio-rustls 0.26` | Async TLS | MIT/Apache-2.0 | ✅ Yes |
| `rustls-pki-types` | PEM/DER parsing | MIT/Apache-2.0 | ✅ Yes |
| `rcgen` | Per-sandbox CA and leaf certificates for TLS interception | MIT/Apache-2.0 | ✅ Yes |
| `rand` | RNG (OS CSPRNG) | MIT/Apache-2.0 | ✅ Yes |

All cryptographic dependencies are publicly available open-source libraries.

---

## 6. ECCN CLASSIFICATION SUMMARY

| Component                                    | Likely ECCN    | Rationale                             | Exception              |
| -------------------------------------------- | -------------- | ------------------------------------- | ---------------------- |
| AES-128/256-GCM (IronCrypto wrapper)         | 5D002.c.1      | Symmetric encryption >56-bit          | §742.15(b) open-source |
| RSA sign/verify, keygen (IronCrypto wrapper) | Re-review      | Signatures only; no RSA encryption    | §742.15(b) open-source |
| ECDSA P-256/384/521 (IronCrypto wrapper)     | Re-review      | Signatures only; no ECDH              | §742.15(b) open-source |
| SHA-256/384/512 (IronCrypto wrapper)         | EAR99          | Hash functions                        | No license needed      |
| HMAC-SHA256/512 (IronCrypto wrapper)         | Part of 5D002  | Authentication in crypto context      | §742.15(b)             |
| HKDF-SHA256 (IronCrypto wrapper)             | Part of 5D002  | Key derivation                        | §742.15(b)             |
| TLS (rustls wrapper), incl. interception     | 5D002.c.1      | Network encryption                    | §742.15(b) open-source |
| PQC ML-KEM (IronCrypto wrapper)              | Re-review      | Real FIPS 203 key establishment       | §742.15(b) open-source |
| PQC ML-DSA (IronCrypto wrapper)              | Re-review      | Real FIPS 204 signatures              | §742.15(b) open-source |
| PQC SLH-DSA (RustCrypto wrapper)             | Re-review      | Real FIPS 205 signatures              | §742.15(b) open-source |
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
| **R-3** | **CMVP-validated FIPS 140-3 module.** The crypto module is FIPS-architected (NIST-approved algorithms via IronCrypto, and RustCrypto for SLH-DSA) but has not been submitted for CMVP validation.                               | Tracked in [FIPS_COMPLIANCE.md](FIPS_COMPLIANCE.md). All public docs and code comments use the phrase "FIPS 140-3 architecture; not yet CMVP-certified" to avoid misrepresentation.             |

### 7.2 Known Source-Code Limitations (K-series)

These are honest disclosures of the current implementation — they affect
production-readiness, not the export-control status of the source release.

| #       | Limitation                                                                                                                                                                                                        | Mitigation in current release                                                                                                                                                                                 |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **K-1** | ~~**Custom RSA encrypt/decrypt**: `mod_exp_bytes()` is a from-scratch big-integer modular exponentiation.~~ | **Resolved in 814d0c0.** Deleted rather than fixed: it computed wrong results (4^13 mod 497 gave 121, not 445) and had no callers. RSA is now IronCrypto `ic-rsa`, signatures only. |
| **K-2** | ~~**Custom AES-CTR+HMAC fallback** in `fips.rs`.~~ | **Resolved in 59ad48a.** Deleted. It was not AES: its keystream was `SHA256(key \|\| nonce \|\| counter)`. AES-GCM is IronCrypto, with no fallback. |
| **K-3** | ~~**PQC modules (`pqc.rs`) implement API stubs.**~~ | **Resolved in 59ad48a.** Real ML-KEM, ML-DSA and SLH-DSA via the RustCrypto crates, on by default. Their ECCN entries in §6 need re-review. SLH-DSA is pinned to a release candidate (`=0.2.0-rc.5`). |
| **K-4** | ~~**vTPM PCR extend uses XOR.**~~ | **Resolved in 922761f.** A SHA-2 hash chain, `H(old \|\| data)`, with property tests. |
| **K-5** | **SM3 hash algorithm** appears in the vTPM `HashAlgorithm` enum for TPM 2.0 specification completeness.                                                                                                           | SM3 is included for protocol parsing only; no HyperMachine security function uses SM3 to provide confidentiality, integrity, or authentication.                                                               |
| **K-6** | ~~The `ring` feature is optional; when disabled, most crypto APIs return `NotImplemented`.~~ | **Resolved in 93abff0.** The feature is gone. IronCrypto is pure Rust with no build script, so every classical primitive is unconditional. |
| **K-8** | ~~**RSA-4096 private-key operations are refused.**~~ `ic-rsa` 0.1.x/0.2.1 panicked deriving the private exponent at 4096 bits, in key generation and `from_primes`. | **Resolved 2026-09-29 in IronCrypto 0.2.2**, reported from here. HyperMachine refused 4096 from 2026-09-28 until then; the guard is gone and an RSA-4096 round trip is tested. |
| **K-7** | **AES-128-GCM was accepted and refused.** `AesKeySize::Aes128` generated 16-byte keys that validation accepted, and the implementation then built an AES-256 cipher for every key. | **Resolved 2026-09-28.** The cipher follows the key length, and AES-128 is checked against GCM-spec Test Case 2. Broken from 93abff0 until then. |

---

## 8. COMPLETE CRYPTOGRAPHIC INVENTORY

### 8.1 Algorithms with Active Implementations

| Algorithm                  | Key Length    | Sym/Asym   | File          | Wrapper vs Custom | External Dep |
| -------------------------- | ------------- | ---------- | ------------- | ----------------- | ------------ |
| AES-256-GCM                | 256-bit       | Symmetric  | fips.rs       | Wrapper           | ic-cipher (IronCrypto) |
| AES-128-GCM                | 128-bit       | Symmetric  | fips.rs       | Wrapper           | ic-cipher (IronCrypto) |
| RSA-2048/3072/4096 keygen  | 2048-4096-bit | Asymmetric | asymmetric.rs | Wrapper           | ic-rsa (IronCrypto)    |
| RSA-2048/3072/4096 sign    | 2048-4096-bit | Asymmetric | asymmetric.rs | Wrapper           | ic-rsa (IronCrypto)    |
| RSA-2048/3072/4096 verify  | 2048-4096-bit | Asymmetric | asymmetric.rs | Wrapper           | ic-rsa (IronCrypto)    |
| ECDSA P-256/SHA-256        | 256-bit       | Asymmetric | asymmetric.rs | Wrapper           | ic-ec (IronCrypto)     |
| ECDSA P-384/SHA-384        | 384-bit       | Asymmetric | asymmetric.rs | Wrapper           | ic-ec (IronCrypto)     |
| ECDSA P-521/SHA-512        | 521-bit       | Asymmetric | asymmetric.rs | Wrapper           | ic-ec (IronCrypto)     |
| ML-KEM-512/768/1024        | FIPS 203      | Asymmetric | pqc.rs        | Wrapper           | ic-mlkem (IronCrypto)  |
| ML-DSA-44/65/87            | FIPS 204      | Asymmetric | pqc.rs        | Wrapper           | ic-mldsa (IronCrypto)  |
| SLH-DSA (6 parameter sets) | FIPS 205      | Asymmetric | pqc.rs        | Wrapper           | slh-dsa (RustCrypto, rc) |
| SHA-256                    | N/A           | Hash       | fips.rs       | Wrapper           | ic-hash (IronCrypto)   |
| SHA-384                    | N/A           | Hash       | fips.rs       | Wrapper           | ic-hash (IronCrypto)   |
| SHA-512                    | N/A           | Hash       | fips.rs       | Wrapper           | ic-hash (IronCrypto)   |
| HMAC-SHA256                | 256-bit       | MAC        | fips.rs, hv2-api middleware.rs | Wrapper | ic-mac (IronCrypto) |
| HMAC-SHA512                | 512-bit       | MAC        | fips.rs       | Wrapper           | ic-mac (IronCrypto)    |
| HKDF-SHA256                | Variable      | KDF        | fips.rs       | Wrapper           | ic-kdf (IronCrypto)    |
| TLS 1.2/1.3                | Various       | Protocol   | hv2-api tls.rs, hv2-net gateway, hm-cli | Wrapper | rustls 0.23 (ring backend) |
| X.509 CA / leaf generation | ECDSA P-256   | Asymmetric | hv2-net gateway/mitm.rs | Wrapper | rcgen          |
| RNG (OS CSPRNG)            | N/A           | Random     | fips.rs       | Wrapper           | rand / OS              |

### 8.2 Algorithms Defined but Not Truly Implemented (Placeholders)

| Algorithm                | Declared Standard | Actual Implementation              | File          |
| ------------------------ | ----------------- | ---------------------------------- | ------------- |
| Hybrid KEM schemes       | N/A               | Type definitions only              | pqc.rs        |
| Hybrid signature schemes | N/A               | Type definitions only              | pqc.rs        |
| ECDH (P-256/P-384)       | SP 800-56A        | Named in comments; no code         | asymmetric.rs |

The PQC, RSA key generation and P-521 rows that the 2026-03-25 review listed
here are now implemented (§8.1).

---

## 9. OVERALL CLASSIFICATION DETERMINATION

**Recommended ECCN: 5D002.c.1**

> "Information security" software not controlled by ECCN 5D002.a, that
> provides or performs "cryptographic activation" of commodities or
> software using "non-standard cryptography."

The software:
1. Implements encryption (AES-GCM) with key lengths >56-bit symmetric, and
   key establishment (ML-KEM). RSA and ECDSA perform signatures only.
2. Provides TLS network encryption, including the egress gateway's TLS
   interception (§1.7)
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
