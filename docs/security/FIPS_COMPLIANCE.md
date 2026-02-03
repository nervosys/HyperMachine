# NIST FIPS 140-3 Compliance Roadmap

**Document Version:** 1.0.0  
**Target Standard:** FIPS 140-3 Level 1 (Software) / Level 2 (HSM)  
**Last Updated:** 2026-02-02  
**Classification:** PUBLIC  

---

## Executive Summary

This document outlines HyperMachine's path to FIPS 140-3 cryptographic module validation, required for:
- Federal government sales (FedRAMP prerequisite)
- DoD IL4/IL5 environments
- Financial services (PCI-DSS supplement)
- Healthcare (HIPAA security rule)

### Compliance Timeline

| Milestone | Target Date | Status |
|-----------|-------------|--------|
| FIPS-ready architecture | Q1 2026 | ✅ Complete |
| Cryptographic module design | Q2 2026 | 🔄 In Progress |
| CAVP testing submission | Q3 2026 | 📋 Planned |
| CMVP validation | Q4 2026 | 📋 Planned |
| FIPS 140-3 Certificate | Q1 2027 | 📋 Planned |

---

## 1. FIPS 140-3 Overview

### 1.1 Security Levels

| Level | Physical Security | Use Case |
|-------|-------------------|----------|
| **Level 1** | None required | Software-only modules |
| **Level 2** | Tamper-evident seals | Cloud deployments |
| **Level 3** | Tamper-resistant | On-premises high-security |
| **Level 4** | Environmental protection | National security |

**HyperMachine Target:** Level 1 (Software), Level 2 (with HSM integration)

### 1.2 Applicable Standards

| Standard | Description | Status |
|----------|-------------|--------|
| FIPS 140-3 | Cryptographic module requirements | Primary target |
| FIPS 186-5 | Digital signatures (ECDSA, EdDSA) | ✅ Compliant |
| FIPS 180-4 | Secure Hash Standard (SHA-2/3) | ✅ Compliant |
| FIPS 197 | AES encryption | ✅ Compliant |
| FIPS 198-1 | HMAC | ✅ Compliant |
| SP 800-38A-D | Block cipher modes | ✅ Compliant |
| SP 800-56A/B | Key establishment | 🔄 Implementing |
| SP 800-90A | DRBG (random numbers) | ✅ Compliant |
| SP 800-108 | Key derivation | ✅ Compliant |
| SP 800-132 | Password-based key derivation | ✅ Compliant |

---

## 2. Cryptographic Inventory

### 2.1 Approved Algorithms

| Function | Algorithm | Key Size | FIPS Status |
|----------|-----------|----------|-------------|
| Symmetric Encryption | AES-256-GCM | 256-bit | ✅ Approved |
| Symmetric Encryption | AES-256-XTS | 512-bit | ✅ Approved |
| Digital Signatures | Ed25519 | 256-bit | ✅ Approved (FIPS 186-5) |
| Digital Signatures | ECDSA P-384 | 384-bit | ✅ Approved |
| Hashing | SHA-256 | N/A | ✅ Approved |
| Hashing | SHA-384 | N/A | ✅ Approved |
| Hashing | SHA3-256 | N/A | ✅ Approved |
| MAC | HMAC-SHA256 | 256-bit | ✅ Approved |
| Key Derivation | HKDF | Variable | ✅ Approved |
| Key Derivation | Argon2id | N/A | ⚠️ Not FIPS (use PBKDF2) |
| Random | DRBG (CTR_DRBG) | 256-bit | ✅ Approved |

### 2.2 Non-Approved Algorithms (To Migrate)

| Current | Replacement | Migration Date |
|---------|-------------|----------------|
| Argon2id (password hashing) | PBKDF2-SHA256 | Q2 2026 |
| ChaCha20-Poly1305 (optional TLS) | AES-256-GCM | Q2 2026 |

### 2.3 Implementation Status

```rust
// Current cryptographic configuration (hv2-api/src/crypto.rs)
pub struct FipsConfig {
    // FIPS-approved algorithms only
    pub symmetric: SymmetricAlgorithm::Aes256Gcm,
    pub signature: SignatureAlgorithm::Ed25519,
    pub hash: HashAlgorithm::Sha256,
    pub kdf: KdfAlgorithm::Hkdf,
    pub drbg: DrbgAlgorithm::CtrDrbg,
    
    // FIPS mode enforcement
    pub fips_mode: bool,
}

impl FipsConfig {
    pub fn fips_only() -> Self {
        Self {
            fips_mode: true,
            // Only FIPS-approved algorithms enabled
            ..Default::default()
        }
    }
}
```

---

## 3. Cryptographic Module Boundary

### 3.1 Module Identification

| Attribute | Value |
|-----------|-------|
| Module Name | HyperMachine Cryptographic Module |
| Module Version | 1.0.0 |
| Module Type | Software |
| Operational Environment | General Purpose Computing |
| Target Platform | x86_64, aarch64 |

### 3.2 Security Boundary

```
┌─────────────────────────────────────────────────────────────┐
│                   HyperMachine Process                       │
│  ┌─────────────────────────────────────────────────────┐    │
│  │         Cryptographic Module Boundary                │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │    │
│  │  │   AES-GCM   │  │   Ed25519   │  │   SHA-256   │  │    │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │    │
│  │  │    HKDF     │  │  CTR_DRBG   │  │  HMAC-SHA   │  │    │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  │    │
│  │                                                      │    │
│  │  ┌─────────────────────────────────────────────┐    │    │
│  │  │            Key Storage (Protected)           │    │    │
│  │  └─────────────────────────────────────────────┘    │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌──────────────────┐  ┌──────────────────┐                 │
│  │   REST API       │  │   VM Manager     │                 │
│  │   (Uses crypto)  │  │   (Uses crypto)  │                 │
│  └──────────────────┘  └──────────────────┘                 │
└─────────────────────────────────────────────────────────────┘
```

### 3.3 Interfaces

| Interface | Type | Data In | Data Out |
|-----------|------|---------|----------|
| Encrypt | Data | Plaintext, Key, IV | Ciphertext, Tag |
| Decrypt | Data | Ciphertext, Key, IV, Tag | Plaintext |
| Sign | Data | Message, Private Key | Signature |
| Verify | Data | Message, Signature, Public Key | Boolean |
| Hash | Data | Message | Digest |
| Random | Status | Request | Random bytes |
| KeyGen | Control | Algorithm params | Key pair |
| KeyDerive | Control | Master key, context | Derived key |

---

## 4. Self-Tests

### 4.1 Power-On Self-Tests (POST)

```rust
// Executed at module initialization
pub fn fips_power_on_self_tests() -> Result<(), FipsError> {
    // Algorithm Known Answer Tests (KAT)
    aes_256_gcm_kat()?;
    ed25519_kat()?;
    sha256_kat()?;
    hmac_sha256_kat()?;
    hkdf_sha256_kat()?;
    ctr_drbg_kat()?;
    
    // Integrity check
    module_integrity_check()?;
    
    Ok(())
}
```

### 4.2 Conditional Self-Tests

| Trigger | Test | Algorithm |
|---------|------|-----------|
| Key generation | Pairwise consistency | Ed25519 |
| DRBG reseed | Health test | CTR_DRBG |
| DRBG generate | Continuous RNG test | CTR_DRBG |
| Firmware update | Integrity verification | SHA-256 |

### 4.3 Error States

| Error | Action |
|-------|--------|
| KAT failure | Enter error state, no crypto operations |
| Integrity failure | Enter error state, shutdown |
| DRBG failure | Reseed, retry, or error state |

---

## 5. Key Management

### 5.1 Key Hierarchy

```
┌─────────────────────────────────────────┐
│           Root Key (HSM/KMS)            │
│          [Hardware Protected]           │
└─────────────────┬───────────────────────┘
                  │
        ┌─────────┴─────────┐
        │                   │
┌───────▼───────┐   ┌───────▼───────┐
│   API KEK     │   │   VM KEK      │
│  (Key Wrap)   │   │  (Key Wrap)   │
└───────┬───────┘   └───────┬───────┘
        │                   │
┌───────▼───────┐   ┌───────▼───────┐
│ Session Keys  │   │ VM Disk Keys  │
│ (AES-256-GCM) │   │ (AES-256-XTS) │
└───────────────┘   └───────────────┘
```

### 5.2 Key Protection

| Key Type | Storage | Protection |
|----------|---------|------------|
| Root keys | HSM/KMS | Hardware |
| KEKs | Encrypted file | AES-256-GCM wrapped |
| Session keys | Memory only | Zeroized on destruction |
| API tokens | Memory + encrypted store | AES-256-GCM |

### 5.3 Key Zeroization

```rust
// Secure key destruction
impl Drop for CryptoKey {
    fn drop(&mut self) {
        // Zeroize key material
        self.bytes.zeroize();
        
        // Memory barrier to prevent optimization
        std::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}
```

---

## 6. Physical Security (Level 2+)

### 6.1 HSM Integration

For Level 2+ compliance, HyperMachine supports HSM integration:

| HSM | Integration | Status |
|-----|-------------|--------|
| AWS CloudHSM | PKCS#11 | 📋 Planned |
| Azure Dedicated HSM | PKCS#11 | 📋 Planned |
| Thales Luna | PKCS#11 | 📋 Planned |
| YubiHSM 2 | Native SDK | 📋 Planned |

### 6.2 Configuration

```rust
// HSM configuration
pub enum KeyStorage {
    Software,           // Level 1
    Hsm(HsmConfig),     // Level 2+
}

pub struct HsmConfig {
    pub provider: HsmProvider,
    pub slot: u64,
    pub pin: SecureString,
    pub key_label: String,
}
```

---

## 7. CAVP Testing Plan

### 7.1 Algorithm Validation

| Algorithm | Test Vectors | CAVP Status |
|-----------|--------------|-------------|
| AES-256-GCM | NIST CAVP | 📋 Pending |
| AES-256-XTS | NIST CAVP | 📋 Pending |
| SHA-256 | NIST CAVP | 📋 Pending |
| SHA-384 | NIST CAVP | 📋 Pending |
| HMAC-SHA256 | NIST CAVP | 📋 Pending |
| Ed25519 | NIST CAVP | 📋 Pending |
| ECDSA P-384 | NIST CAVP | 📋 Pending |
| CTR_DRBG | NIST CAVP | 📋 Pending |
| HKDF | NIST CAVP | 📋 Pending |

### 7.2 Validation Lab

**Selected Lab:** [TBD - Accredited NVLAP lab]  
**Estimated Cost:** $50,000 - $100,000  
**Timeline:** 6-12 months  

---

## 8. Operational Environment

### 8.1 Supported Platforms

| Platform | OS | FIPS Provider |
|----------|----|--------------| 
| x86_64 | Linux 6.x | OpenSSL 3.x FIPS provider |
| x86_64 | Windows Server 2022 | Windows CNG |
| aarch64 | Linux 6.x | OpenSSL 3.x FIPS provider |

### 8.2 FIPS Mode Configuration

```bash
# Enable FIPS mode at runtime
export HYPERMACHINE_FIPS_MODE=1

# Verify FIPS mode
hm config show | grep fips
# fips_mode: enabled
# fips_provider: openssl-fips
```

---

## 9. Documentation Requirements

### 9.1 Required Documents for CMVP

| Document | Status |
|----------|--------|
| Security Policy | 🔄 Draft |
| Finite State Model | 📋 Planned |
| Design documentation | 📋 Planned |
| Source code | ✅ Available |
| Vendor evidence | 📋 Planned |

### 9.2 User Guidance

| Document | Status |
|----------|--------|
| Crypto Officer guidance | 📋 Planned |
| User guidance | 📋 Planned |
| Approved mode of operation | 📋 Planned |

---

## 10. Compliance Checklist

### FIPS 140-3 Section Compliance

| Section | Requirement | Status |
|---------|-------------|--------|
| §4.1 | Cryptographic module specification | 🔄 In Progress |
| §4.2 | Cryptographic module interfaces | 🔄 In Progress |
| §4.3 | Roles, services, authentication | 🔄 In Progress |
| §4.4 | Software/firmware security | ✅ Complete |
| §4.5 | Operational environment | ✅ Complete |
| §4.6 | Physical security | N/A (Level 1) |
| §4.7 | Non-invasive security | N/A (Level 1) |
| §4.8 | Sensitive security parameters | ✅ Complete |
| §4.9 | Self-tests | 🔄 In Progress |
| §4.10 | Life-cycle assurance | 🔄 In Progress |
| §4.11 | Mitigation of other attacks | 🔄 In Progress |

---

## References

- [FIPS 140-3 Standard](https://csrc.nist.gov/publications/detail/fips/140/3/final)
- [CMVP Management Manual](https://csrc.nist.gov/projects/cryptographic-module-validation-program)
- [CAVP Testing](https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program)
- [SP 800-140 Series](https://csrc.nist.gov/publications/sp800)

---

**Document Control:**
- Author: HyperMachine Security Team
- Reviewers: Security Architect, Compliance Officer
- Next Review: 2026-05-02
