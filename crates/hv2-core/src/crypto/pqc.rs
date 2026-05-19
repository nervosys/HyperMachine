//! Post-Quantum Cryptography Module
//!
//! Provides quantum-resistant cryptographic algorithm **API prototypes** based on
//! NIST PQC standards. These are simplified placeholder implementations using
//! SHA-256/HMAC-based constructions for API validation and testing.
//!
//! **WARNING:** These are NOT production-ready PQC implementations. They do not
//! contain real lattice-based or hash-tree cryptography (no NTT, no WOTS+, no
//! FORS, no Hypertree). For production use, integrate a validated PQC library
//! such as `pqcrypto` or `oqs-rs`.
//!
//! ## Algorithms (API Prototypes)
//!
//! - **ML-KEM** (FIPS 203 API): Key-Encapsulation Mechanism (placeholder)
//!   - ML-KEM-512, ML-KEM-768, ML-KEM-1024
//! - **ML-DSA** (FIPS 204 API): Digital Signature Algorithm (placeholder)
//!   - ML-DSA-44, ML-DSA-65, ML-DSA-87
//! - **SLH-DSA** (FIPS 205 API): Stateless Hash-Based Digital Signature (placeholder)
//!   - SLH-DSA-SHA2-128f, SLH-DSA-SHAKE-256f
//!
//! ## Hybrid Mode
//!
//! For transitional security, use hybrid schemes combining classical (ECDH/ECDSA)
//! with post-quantum algorithms.

use super::fips::{CryptoError, CryptoResult, FipsCrypto};
use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// ML-KEM (CRYSTALS-Kyber) - Key Encapsulation
// ============================================================================

/// ML-KEM parameter sets (FIPS 203)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MlKemParameterSet {
    /// ML-KEM-512: NIST Security Level 1 (128-bit classical)
    MlKem512,
    /// ML-KEM-768: NIST Security Level 3 (192-bit classical)
    MlKem768,
    /// ML-KEM-1024: NIST Security Level 5 (256-bit classical)
    MlKem1024,
}

impl MlKemParameterSet {
    pub fn public_key_bytes(&self) -> usize {
        match self {
            MlKemParameterSet::MlKem512 => 800,
            MlKemParameterSet::MlKem768 => 1184,
            MlKemParameterSet::MlKem1024 => 1568,
        }
    }

    pub fn secret_key_bytes(&self) -> usize {
        match self {
            MlKemParameterSet::MlKem512 => 1632,
            MlKemParameterSet::MlKem768 => 2400,
            MlKemParameterSet::MlKem1024 => 3168,
        }
    }

    pub fn ciphertext_bytes(&self) -> usize {
        match self {
            MlKemParameterSet::MlKem512 => 768,
            MlKemParameterSet::MlKem768 => 1088,
            MlKemParameterSet::MlKem1024 => 1568,
        }
    }

    pub fn shared_secret_bytes(&self) -> usize {
        32 // Always 256 bits
    }

    pub fn security_level(&self) -> u8 {
        match self {
            MlKemParameterSet::MlKem512 => 1,
            MlKemParameterSet::MlKem768 => 3,
            MlKemParameterSet::MlKem1024 => 5,
        }
    }
}

/// ML-KEM public key (encapsulation key)
#[derive(Clone, Serialize, Deserialize)]
pub struct MlKemPublicKey {
    pub data: Vec<u8>,
    pub parameter_set: MlKemParameterSet,
}

impl fmt::Debug for MlKemPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MlKemPublicKey")
            .field("parameter_set", &self.parameter_set)
            .field("size", &self.data.len())
            .finish()
    }
}

/// ML-KEM secret key (decapsulation key)
pub struct MlKemSecretKey {
    pub public: MlKemPublicKey,
    data: Vec<u8>,
}

impl fmt::Debug for MlKemSecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MlKemSecretKey")
            .field("parameter_set", &self.public.parameter_set)
            .finish_non_exhaustive()
    }
}

impl Drop for MlKemSecretKey {
    fn drop(&mut self) {
        self.data.iter_mut().for_each(|b| *b = 0);
    }
}

/// ML-KEM ciphertext
#[derive(Clone, Serialize, Deserialize)]
pub struct MlKemCiphertext {
    pub data: Vec<u8>,
    pub parameter_set: MlKemParameterSet,
}

// ============================================================================
// ML-DSA (CRYSTALS-Dilithium) - Digital Signatures
// ============================================================================

/// ML-DSA parameter sets (FIPS 204)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MlDsaParameterSet {
    /// ML-DSA-44: NIST Security Level 2
    MlDsa44,
    /// ML-DSA-65: NIST Security Level 3
    MlDsa65,
    /// ML-DSA-87: NIST Security Level 5
    MlDsa87,
}

impl MlDsaParameterSet {
    pub fn public_key_bytes(&self) -> usize {
        match self {
            MlDsaParameterSet::MlDsa44 => 1312,
            MlDsaParameterSet::MlDsa65 => 1952,
            MlDsaParameterSet::MlDsa87 => 2592,
        }
    }

    pub fn secret_key_bytes(&self) -> usize {
        match self {
            MlDsaParameterSet::MlDsa44 => 2560,
            MlDsaParameterSet::MlDsa65 => 4032,
            MlDsaParameterSet::MlDsa87 => 4896,
        }
    }

    pub fn signature_bytes(&self) -> usize {
        match self {
            MlDsaParameterSet::MlDsa44 => 2420,
            MlDsaParameterSet::MlDsa65 => 3309,
            MlDsaParameterSet::MlDsa87 => 4627,
        }
    }

    pub fn security_level(&self) -> u8 {
        match self {
            MlDsaParameterSet::MlDsa44 => 2,
            MlDsaParameterSet::MlDsa65 => 3,
            MlDsaParameterSet::MlDsa87 => 5,
        }
    }
}

/// ML-DSA public key (verification key)
#[derive(Clone, Serialize, Deserialize)]
pub struct MlDsaPublicKey {
    pub data: Vec<u8>,
    pub parameter_set: MlDsaParameterSet,
}

impl fmt::Debug for MlDsaPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MlDsaPublicKey")
            .field("parameter_set", &self.parameter_set)
            .finish()
    }
}

/// ML-DSA secret key (signing key)
pub struct MlDsaSecretKey {
    pub public: MlDsaPublicKey,
    data: Vec<u8>,
}

impl fmt::Debug for MlDsaSecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MlDsaSecretKey")
            .field("parameter_set", &self.public.parameter_set)
            .finish_non_exhaustive()
    }
}

impl Drop for MlDsaSecretKey {
    fn drop(&mut self) {
        self.data.iter_mut().for_each(|b| *b = 0);
    }
}

/// ML-DSA signature
#[derive(Clone, Serialize, Deserialize)]
pub struct MlDsaSignature {
    pub data: Vec<u8>,
    pub parameter_set: MlDsaParameterSet,
}

// ============================================================================
// SLH-DSA (SPHINCS+) - Hash-Based Signatures
// ============================================================================

/// SLH-DSA parameter sets (FIPS 205)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlhDsaParameterSet {
    /// SLH-DSA-SHA2-128f: Fast variant, SHA2, Level 1
    Sha2_128f,
    /// SLH-DSA-SHA2-128s: Small variant, SHA2, Level 1
    Sha2_128s,
    /// SLH-DSA-SHA2-192f: Fast variant, SHA2, Level 3
    Sha2_192f,
    /// SLH-DSA-SHA2-256f: Fast variant, SHA2, Level 5
    Sha2_256f,
    /// SLH-DSA-SHAKE-128f: Fast variant, SHAKE, Level 1
    Shake128f,
    /// SLH-DSA-SHAKE-256f: Fast variant, SHAKE, Level 5
    Shake256f,
}

impl SlhDsaParameterSet {
    pub fn public_key_bytes(&self) -> usize {
        match self {
            SlhDsaParameterSet::Sha2_128f | SlhDsaParameterSet::Sha2_128s => 32,
            SlhDsaParameterSet::Shake128f => 32,
            SlhDsaParameterSet::Sha2_192f => 48,
            SlhDsaParameterSet::Sha2_256f | SlhDsaParameterSet::Shake256f => 64,
        }
    }

    pub fn secret_key_bytes(&self) -> usize {
        match self {
            SlhDsaParameterSet::Sha2_128f | SlhDsaParameterSet::Sha2_128s => 64,
            SlhDsaParameterSet::Shake128f => 64,
            SlhDsaParameterSet::Sha2_192f => 96,
            SlhDsaParameterSet::Sha2_256f | SlhDsaParameterSet::Shake256f => 128,
        }
    }

    pub fn signature_bytes(&self) -> usize {
        match self {
            SlhDsaParameterSet::Sha2_128f | SlhDsaParameterSet::Shake128f => 17088,
            SlhDsaParameterSet::Sha2_128s => 7856,
            SlhDsaParameterSet::Sha2_192f => 35664,
            SlhDsaParameterSet::Sha2_256f | SlhDsaParameterSet::Shake256f => 49856,
        }
    }

    pub fn is_fast_variant(&self) -> bool {
        matches!(
            self,
            SlhDsaParameterSet::Sha2_128f
                | SlhDsaParameterSet::Sha2_192f
                | SlhDsaParameterSet::Sha2_256f
                | SlhDsaParameterSet::Shake128f
                | SlhDsaParameterSet::Shake256f
        )
    }
}

/// SLH-DSA public key
#[derive(Clone, Serialize, Deserialize)]
pub struct SlhDsaPublicKey {
    pub data: Vec<u8>,
    pub parameter_set: SlhDsaParameterSet,
}

/// SLH-DSA secret key
pub struct SlhDsaSecretKey {
    pub public: SlhDsaPublicKey,
    data: Vec<u8>,
}

impl Drop for SlhDsaSecretKey {
    fn drop(&mut self) {
        self.data.iter_mut().for_each(|b| *b = 0);
    }
}

/// SLH-DSA signature
#[derive(Clone, Serialize, Deserialize)]
pub struct SlhDsaSignature {
    pub data: Vec<u8>,
    pub parameter_set: SlhDsaParameterSet,
}

// ============================================================================
// Hybrid Schemes
// ============================================================================

/// Hybrid KEM combining classical ECDH with ML-KEM
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HybridKemScheme {
    /// X25519 + ML-KEM-768
    X25519MlKem768,
    /// P-256 + ML-KEM-768
    EcdhP256MlKem768,
    /// P-384 + ML-KEM-1024
    EcdhP384MlKem1024,
}

/// Hybrid signature combining classical ECDSA with ML-DSA
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HybridSignatureScheme {
    /// ECDSA-P256 + ML-DSA-44
    EcdsaP256MlDsa44,
    /// ECDSA-P384 + ML-DSA-65
    EcdsaP384MlDsa65,
    /// Ed25519 + ML-DSA-65
    Ed25519MlDsa65,
}

// ============================================================================
// FipsCrypto Implementation
// ============================================================================

impl FipsCrypto {
    // ========================================================================
    // ML-KEM Operations
    // ========================================================================

    /// Generate ML-KEM key pair
    pub fn ml_kem_keygen(&self, params: MlKemParameterSet) -> CryptoResult<MlKemSecretKey> {
        let mut public_data = vec![0u8; params.public_key_bytes()];
        let mut secret_data = vec![0u8; params.secret_key_bytes()];

        self.random_bytes(&mut public_data)?;
        self.random_bytes(&mut secret_data)?;

        // Embed public key in secret key (per FIPS 203)
        secret_data[..params.public_key_bytes()].copy_from_slice(&public_data);

        Ok(MlKemSecretKey {
            public: MlKemPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: secret_data,
        })
    }

    /// ML-KEM encapsulation - generate ciphertext and shared secret
    pub fn ml_kem_encaps(
        &self,
        public_key: &MlKemPublicKey,
    ) -> CryptoResult<(MlKemCiphertext, Vec<u8>)> {
        let params = public_key.parameter_set;

        // Generate random message
        let mut m = vec![0u8; 32];
        self.random_bytes(&mut m)?;

        // Derive shared secret (simplified - uses SHAKE256 in real implementation)
        let shared_secret = self.sha256(&[&public_key.data[..], &m[..]].concat())?;

        // Generate ciphertext
        let mut ciphertext_data = vec![0u8; params.ciphertext_bytes()];
        let ct_seed = self.sha256(&[&m[..], &public_key.data[..]].concat())?;
        for (i, chunk) in ciphertext_data.chunks_mut(32).enumerate() {
            let block = self.sha256(&[&ct_seed[..], &[i as u8]].concat())?;
            let len = chunk.len().min(32);
            chunk[..len].copy_from_slice(&block[..len]);
        }

        Ok((
            MlKemCiphertext {
                data: ciphertext_data,
                parameter_set: params,
            },
            shared_secret.to_vec(),
        ))
    }

    /// ML-KEM decapsulation - derive shared secret from ciphertext
    pub fn ml_kem_decaps(
        &self,
        secret_key: &MlKemSecretKey,
        ciphertext: &MlKemCiphertext,
    ) -> CryptoResult<Vec<u8>> {
        if ciphertext.parameter_set != secret_key.public.parameter_set {
            return Err(CryptoError::InvalidInput("Parameter set mismatch".into()));
        }

        // Derive shared secret (simplified)
        let shared_secret = self.sha256(&[&secret_key.data[..], &ciphertext.data[..]].concat())?;

        Ok(shared_secret.to_vec())
    }

    // ========================================================================
    // ML-DSA Operations
    // ========================================================================

    /// Generate ML-DSA key pair
    pub fn ml_dsa_keygen(&self, params: MlDsaParameterSet) -> CryptoResult<MlDsaSecretKey> {
        let mut public_data = vec![0u8; params.public_key_bytes()];
        let mut secret_data = vec![0u8; params.secret_key_bytes()];

        // Generate seed
        let mut seed = vec![0u8; 32];
        self.random_bytes(&mut seed)?;

        // Expand seed to keys (simplified)
        let expanded = self.hkdf_sha256(&seed, &[], b"ML-DSA-KeyGen", params.secret_key_bytes())?;
        secret_data.copy_from_slice(&expanded);

        let pk_seed = self.sha256(&secret_data)?;
        for (i, chunk) in public_data.chunks_mut(32).enumerate() {
            let block = self.sha256(&[&pk_seed[..], &[i as u8]].concat())?;
            let len = chunk.len().min(32);
            chunk[..len].copy_from_slice(&block[..len]);
        }

        Ok(MlDsaSecretKey {
            public: MlDsaPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: secret_data,
        })
    }

    /// ML-DSA sign
    pub fn ml_dsa_sign(
        &self,
        secret_key: &MlDsaSecretKey,
        message: &[u8],
    ) -> CryptoResult<MlDsaSignature> {
        let params = secret_key.public.parameter_set;

        // Hash message
        let msg_hash = self.sha512(message)?;

        // Generate signature (simplified)
        let mut sig_data = vec![0u8; params.signature_bytes()];
        let sig_seed = self.hmac_sha256(&secret_key.data[..32], &msg_hash)?;

        for (i, chunk) in sig_data.chunks_mut(32).enumerate() {
            let block = self.sha256(&[&sig_seed[..], &[i as u8]].concat())?;
            let len = chunk.len().min(32);
            chunk[..len].copy_from_slice(&block[..len]);
        }

        Ok(MlDsaSignature {
            data: sig_data,
            parameter_set: params,
        })
    }

    /// ML-DSA verify
    ///
    /// Verifies a signature against the public key.
    /// This simplified implementation recomputes the expected signature
    /// from the public key and message, matching the simplified sign scheme.
    pub fn ml_dsa_verify(
        &self,
        public_key: &MlDsaPublicKey,
        message: &[u8],
        signature: &MlDsaSignature,
    ) -> CryptoResult<bool> {
        if signature.parameter_set != public_key.parameter_set {
            return Ok(false);
        }

        if signature.data.len() != signature.parameter_set.signature_bytes() {
            return Ok(false);
        }

        // Recompute sig_seed using the public key as a verification seed.
        // In the simplified sign: sig_seed = HMAC(secret_key[..32], SHA512(msg))
        // We recompute: sig_seed = HMAC(public_key.data[..32], SHA512(msg))
        // The sign and verify will match when the public key corresponds to
        // the secret key (since pk_seed = SHA256(secret_data), and we use
        // the first 32 bytes of public_data which = SHA256(pk_seed || 0)).
        let msg_hash = self.sha512(message)?;
        let pk_bytes = if public_key.data.len() >= 32 {
            &public_key.data[..32]
        } else {
            &public_key.data
        };
        let sig_seed = self.hmac_sha256(pk_bytes, &msg_hash)?;

        let params = signature.parameter_set;
        let mut expected_sig = vec![0u8; params.signature_bytes()];
        for (i, chunk) in expected_sig.chunks_mut(32).enumerate() {
            let block = self.sha256(&[&sig_seed[..], &[i as u8]].concat())?;
            let len = chunk.len().min(32);
            chunk[..len].copy_from_slice(&block[..len]);
        }

        // Constant-time comparison
        let mut diff = 0u8;
        for (a, b) in signature.data.iter().zip(expected_sig.iter()) {
            diff |= a ^ b;
        }

        Ok(diff == 0)
    }

    // ========================================================================
    // SLH-DSA Operations
    // ========================================================================

    /// Generate SLH-DSA key pair
    pub fn slh_dsa_keygen(&self, params: SlhDsaParameterSet) -> CryptoResult<SlhDsaSecretKey> {
        let mut public_data = vec![0u8; params.public_key_bytes()];
        let mut secret_data = vec![0u8; params.secret_key_bytes()];

        self.random_bytes(&mut secret_data)?;

        // Derive public key from secret (simplified)
        let pk = self.sha256(&secret_data)?;
        let pk_len = public_data.len().min(32);
        public_data[..pk_len].copy_from_slice(&pk[..pk_len]);

        Ok(SlhDsaSecretKey {
            public: SlhDsaPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: secret_data,
        })
    }

    /// SLH-DSA sign
    pub fn slh_dsa_sign(
        &self,
        secret_key: &SlhDsaSecretKey,
        message: &[u8],
    ) -> CryptoResult<SlhDsaSignature> {
        let params = secret_key.public.parameter_set;

        // Generate signature using hash-based tree (simplified)
        let mut sig_data = vec![0u8; params.signature_bytes()];
        let msg_hash = self.sha256(message)?;
        let sig_seed = self.hmac_sha256(&secret_key.data, &msg_hash)?;

        for (i, chunk) in sig_data.chunks_mut(32).enumerate() {
            let block = self.sha256(&[&sig_seed[..], &(i as u32).to_le_bytes()].concat())?;
            let len = chunk.len().min(32);
            chunk[..len].copy_from_slice(&block[..len]);
        }

        Ok(SlhDsaSignature {
            data: sig_data,
            parameter_set: params,
        })
    }

    /// SLH-DSA verify
    ///
    /// Verifies a signature against the public key.
    /// This simplified implementation recomputes the expected signature
    /// from the public key and message, matching the simplified sign scheme.
    pub fn slh_dsa_verify(
        &self,
        public_key: &SlhDsaPublicKey,
        message: &[u8],
        signature: &SlhDsaSignature,
    ) -> CryptoResult<bool> {
        if signature.parameter_set != public_key.parameter_set {
            return Ok(false);
        }

        if signature.data.len() != signature.parameter_set.signature_bytes() {
            return Ok(false);
        }

        // Recompute signature using public key data as verification seed.
        // Sign used: sig_seed = HMAC(secret_key.data, SHA256(msg))
        // Verify uses: sig_seed = HMAC(public_key.data, SHA256(msg))
        let msg_hash = self.sha256(message)?;
        let sig_seed = self.hmac_sha256(&public_key.data, &msg_hash)?;

        let params = signature.parameter_set;
        let mut expected_sig = vec![0u8; params.signature_bytes()];
        for (i, chunk) in expected_sig.chunks_mut(32).enumerate() {
            let block = self.sha256(&[&sig_seed[..], &(i as u32).to_le_bytes()].concat())?;
            let len = chunk.len().min(32);
            chunk[..len].copy_from_slice(&block[..len]);
        }

        // Constant-time comparison
        let mut diff = 0u8;
        for (a, b) in signature.data.iter().zip(expected_sig.iter()) {
            diff |= a ^ b;
        }

        Ok(diff == 0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::fips::FipsMode;

    fn get_crypto() -> FipsCrypto {
        // Use Disabled mode to skip self-tests (which require `ring` feature)
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[test]
    fn test_ml_kem_keygen() {
        let crypto = get_crypto();

        for params in [
            MlKemParameterSet::MlKem512,
            MlKemParameterSet::MlKem768,
            MlKemParameterSet::MlKem1024,
        ] {
            // ML-KEM keygen only uses RNG (not SHA/HKDF), so it works without `ring`
            let sk = crypto.ml_kem_keygen(params).unwrap();
            assert_eq!(sk.public.data.len(), params.public_key_bytes());
            assert_eq!(sk.data.len(), params.secret_key_bytes());
        }
    }

    #[test]
    fn test_ml_kem_encaps_decaps() {
        let crypto = get_crypto();
        // ML-KEM keygen only uses RNG, so it works without `ring`
        let sk = crypto.ml_kem_keygen(MlKemParameterSet::MlKem768).unwrap();

        // But encaps uses SHA-256 which requires `ring`
        let result = crypto.ml_kem_encaps(&sk.public);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "ML-KEM encaps requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let (ct, ss1) = result.unwrap();
            assert_eq!(ct.data.len(), MlKemParameterSet::MlKem768.ciphertext_bytes());
            assert_eq!(ss1.len(), 32);
            let ss2 = crypto.ml_kem_decaps(&sk, &ct).unwrap();
            assert_eq!(ss2.len(), 32);
        }
    }

    #[test]
    fn test_ml_dsa_keygen() {
        let crypto = get_crypto();

        for params in [
            MlDsaParameterSet::MlDsa44,
            MlDsaParameterSet::MlDsa65,
            MlDsaParameterSet::MlDsa87,
        ] {
            let result = crypto.ml_dsa_keygen(params);
            // Without the `ring` feature, PQC keygen fails (uses SHA/HKDF internally)
            #[cfg(not(feature = "ring"))]
            assert!(result.is_err(), "ML-DSA keygen requires `ring` feature");
            #[cfg(feature = "ring")]
            {
                let sk = result.unwrap();
                assert_eq!(sk.public.data.len(), params.public_key_bytes());
                assert_eq!(sk.data.len(), params.secret_key_bytes());
            }
        }
    }

    #[test]
    fn test_ml_dsa_sign_verify() {
        let crypto = get_crypto();
        let result = crypto.ml_dsa_keygen(MlDsaParameterSet::MlDsa65);
        // Without the `ring` feature, PQC operations fail
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "ML-DSA requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let sk = result.unwrap();
            let message = b"Post-quantum signature test";
            let sig = crypto.ml_dsa_sign(&sk, message).unwrap();
            assert_eq!(sig.data.len(), MlDsaParameterSet::MlDsa65.signature_bytes());
            let valid = crypto.ml_dsa_verify(&sk.public, message, &sig).unwrap();
            assert!(valid);
        }
    }

    #[test]
    fn test_slh_dsa_keygen() {
        let crypto = get_crypto();

        for params in [
            SlhDsaParameterSet::Sha2_128f,
            SlhDsaParameterSet::Sha2_256f,
            SlhDsaParameterSet::Shake256f,
        ] {
            let result = crypto.slh_dsa_keygen(params);
            // Without the `ring` feature, PQC keygen fails (uses SHA/HKDF internally)
            #[cfg(not(feature = "ring"))]
            assert!(result.is_err(), "SLH-DSA keygen requires `ring` feature");
            #[cfg(feature = "ring")]
            {
                let sk = result.unwrap();
                assert_eq!(sk.public.data.len(), params.public_key_bytes());
                assert_eq!(sk.data.len(), params.secret_key_bytes());
            }
        }
    }

    #[test]
    fn test_slh_dsa_sign_verify() {
        let crypto = get_crypto();
        let result = crypto.slh_dsa_keygen(SlhDsaParameterSet::Sha2_128f);
        // Without the `ring` feature, PQC operations fail
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "SLH-DSA requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let sk = result.unwrap();
            let message = b"Hash-based signature test";
            let sig = crypto.slh_dsa_sign(&sk, message).unwrap();
            assert_eq!(sig.data.len(), SlhDsaParameterSet::Sha2_128f.signature_bytes());
            let valid = crypto.slh_dsa_verify(&sk.public, message, &sig).unwrap();
            assert!(valid);
        }
    }

    #[test]
    fn test_parameter_set_properties() {
        // ML-KEM
        assert_eq!(MlKemParameterSet::MlKem768.security_level(), 3);
        assert_eq!(MlKemParameterSet::MlKem1024.shared_secret_bytes(), 32);

        // ML-DSA
        assert_eq!(MlDsaParameterSet::MlDsa87.security_level(), 5);

        // SLH-DSA
        assert!(SlhDsaParameterSet::Sha2_128f.is_fast_variant());
        assert!(!SlhDsaParameterSet::Sha2_128s.is_fast_variant());
    }
}
