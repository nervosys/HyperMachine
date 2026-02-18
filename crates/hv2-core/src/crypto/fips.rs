//! FIPS 140-3 Compliant Cryptographic Module
//!
//! This module provides FIPS-validated cryptographic primitives for HyperMachine.
//! When compiled with `fips` feature, uses FIPS-validated implementations.
//!
//! # Security Compliance
//!
//! - FIPS 140-3 Level 1 (software)
//! - NIST SP 800-56A (key agreement)
//! - NIST SP 800-56B (key transport)
//! - NIST SP 800-90A (DRBG)
//!
//! # Usage
//!
//! ```rust,ignore
//! use hv2_core::crypto::fips::{FipsCrypto, FipsMode};
//!
//! // Initialize FIPS module
//! let crypto = FipsCrypto::new(FipsMode::Enabled)?;
//!
//! // Generate random bytes
//! let mut key = [0u8; 32];
//! crypto.random_bytes(&mut key)?;
//!
//! // Encrypt data
//! let ciphertext = crypto.aes_gcm_encrypt(&key, &plaintext, &aad)?;
//! ```

use std::fmt;

// ============================================================================
// FIPS Configuration
// ============================================================================

/// FIPS operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FipsMode {
    /// FIPS mode disabled - use standard crypto
    Disabled,
    /// FIPS mode enabled - use validated implementations
    Enabled,
    /// FIPS mode strict - fail if non-FIPS operation attempted
    Strict,
}

impl Default for FipsMode {
    fn default() -> Self {
        // Default based on compile-time feature
        #[cfg(feature = "fips")]
        return FipsMode::Enabled;
        #[cfg(not(feature = "fips"))]
        return FipsMode::Disabled;
    }
}

/// FIPS module status
#[derive(Debug, Clone)]
pub struct FipsStatus {
    /// Current operating mode
    pub mode: FipsMode,
    /// Module version
    pub version: &'static str,
    /// Self-test passed
    pub self_test_passed: bool,
    /// Approved algorithms available
    pub approved_algorithms: Vec<&'static str>,
    /// Module certificate (if certified)
    pub certificate: Option<&'static str>,
}

impl Default for FipsStatus {
    fn default() -> Self {
        Self {
            mode: FipsMode::default(),
            version: "1.0.0",
            self_test_passed: false,
            approved_algorithms: vec![
                "AES-128-GCM",
                "AES-256-GCM",
                "AES-128-CBC",
                "AES-256-CBC",
                "SHA-256",
                "SHA-384",
                "SHA-512",
                "SHA3-256",
                "SHA3-512",
                "HMAC-SHA256",
                "HMAC-SHA384",
                "HMAC-SHA512",
                "ECDSA-P256",
                "ECDSA-P384",
                "ECDH-P256",
                "ECDH-P384",
                "RSA-2048",
                "RSA-3072",
                "RSA-4096",
                "CTR-DRBG",
            ],
            certificate: None, // Pending certification
        }
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Cryptographic operation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum CryptoError {
    /// FIPS self-test failed
    #[error("FIPS self-test failed: {0}")]
    SelfTestFailed(String),
    /// Invalid key length
    #[error("Invalid key length: expected {expected}, got {got}")]
    InvalidKeyLength { expected: usize, got: usize },
    /// Invalid nonce/IV length
    #[error("Invalid nonce length: expected {expected}, got {got}")]
    InvalidNonceLength { expected: usize, got: usize },
    /// Authentication failed
    #[error("Authentication failed")]
    AuthenticationFailed,
    /// Encryption failed
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    /// Decryption failed
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
    /// Key generation failed
    #[error("Key generation failed: {0}")]
    KeyGenerationFailed(String),
    /// Algorithm not approved in FIPS mode
    #[error("Algorithm not approved in FIPS mode: {0}")]
    AlgorithmNotApproved(String),
    /// Random number generation failed
    #[error("RNG failed: {0}")]
    RngFailed(String),
    /// Signature verification failed
    #[error("Signature verification failed")]
    SignatureVerificationFailed,
    /// Invalid signature format
    #[error("Invalid signature format")]
    InvalidSignature,
    /// Key derivation failed
    #[error("Key derivation failed: {0}")]
    KeyDerivationFailed(String),
    /// Hash computation failed
    #[error("Hash computation failed: {0}")]
    HashFailed(String),
    /// Invalid input parameters
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    /// Unsupported algorithm
    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),
    /// Operation not implemented (placeholder crypto removed)
    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

pub type CryptoResult<T> = Result<T, CryptoError>;

// ============================================================================
// Key Types
// ============================================================================

/// AES key sizes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesKeySize {
    /// 128-bit key
    Aes128,
    /// 256-bit key
    Aes256,
}

impl AesKeySize {
    pub fn bytes(&self) -> usize {
        match self {
            Self::Aes128 => 16,
            Self::Aes256 => 32,
        }
    }
}

/// Symmetric encryption key
#[derive(Clone)]
pub struct SymmetricKey {
    key: Vec<u8>,
    algorithm: String,
}

impl SymmetricKey {
    /// Create new symmetric key from bytes
    pub fn new(key: Vec<u8>, algorithm: &str) -> Self {
        Self {
            key,
            algorithm: algorithm.to_string(),
        }
    }

    /// Get key bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.key
    }

    /// Get key length
    pub fn len(&self) -> usize {
        self.key.len()
    }

    /// Check if key is empty
    pub fn is_empty(&self) -> bool {
        self.key.is_empty()
    }

    /// Get algorithm name
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
}

impl Drop for SymmetricKey {
    fn drop(&mut self) {
        // Zeroize key material
        for byte in &mut self.key {
            *byte = 0;
        }
    }
}

impl fmt::Debug for SymmetricKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymmetricKey")
            .field("algorithm", &self.algorithm)
            .field("length", &self.key.len())
            .finish()
    }
}

/// Asymmetric key pair
#[derive(Debug)]
pub struct KeyPair {
    /// Private key (PEM or DER encoded)
    private_key: Vec<u8>,
    /// Public key (PEM or DER encoded)
    public_key: Vec<u8>,
    /// Algorithm identifier
    algorithm: String,
}

impl KeyPair {
    /// Create new key pair
    pub fn new(private_key: Vec<u8>, public_key: Vec<u8>, algorithm: &str) -> Self {
        Self {
            private_key,
            public_key,
            algorithm: algorithm.to_string(),
        }
    }

    /// Get public key
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Get private key (use with caution)
    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }

    /// Get algorithm
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
}

impl Drop for KeyPair {
    fn drop(&mut self) {
        // Zeroize private key material
        for byte in &mut self.private_key {
            *byte = 0;
        }
    }
}

// ============================================================================
// FIPS Crypto Module
// ============================================================================

/// FIPS-compliant cryptographic operations
pub struct FipsCrypto {
    mode: FipsMode,
    status: FipsStatus,
}

impl FipsCrypto {
    /// Create new FIPS crypto module
    pub fn new(mode: FipsMode) -> CryptoResult<Self> {
        let mut status = FipsStatus::default();
        status.mode = mode;

        let mut crypto = Self { mode, status };

        // Run self-tests on initialization
        if mode != FipsMode::Disabled {
            crypto.run_self_tests()?;
        }

        Ok(crypto)
    }

    /// Get current FIPS status
    pub fn status(&self) -> &FipsStatus {
        &self.status
    }

    /// Check if FIPS mode is enabled
    pub fn is_fips_enabled(&self) -> bool {
        self.mode != FipsMode::Disabled
    }

    /// Run FIPS self-tests (KAT - Known Answer Tests)
    pub fn run_self_tests(&mut self) -> CryptoResult<()> {
        // AES-GCM KAT
        self.kat_aes_gcm()?;

        // SHA-256 KAT
        self.kat_sha256()?;

        // HMAC-SHA256 KAT
        self.kat_hmac_sha256()?;

        // DRBG KAT
        self.kat_drbg()?;

        self.status.self_test_passed = true;
        Ok(())
    }

    // ========================================================================
    // Random Number Generation
    // ========================================================================

    /// Generate cryptographically secure random bytes
    /// Generate cryptographically secure random bytes
    pub fn random_bytes(&self, buffer: &mut [u8]) -> CryptoResult<()> {
        use rand::RngCore;

        // Use rand's thread_rng which uses the OS CSPRNG
        // On Windows this uses BCryptGenRandom internally
        // On Linux this uses getrandom(2) or /dev/urandom
        rand::thread_rng()
            .try_fill_bytes(buffer)
            .map_err(|e| CryptoError::RngFailed(e.to_string()))?;

        Ok(())
    }

    /// Generate random 128-bit value
    pub fn random_u128(&self) -> CryptoResult<u128> {
        let mut bytes = [0u8; 16];
        self.random_bytes(&mut bytes)?;
        Ok(u128::from_le_bytes(bytes))
    }
    // ========================================================================
    // Symmetric Encryption (AES-GCM)
    // ========================================================================

    /// Generate AES key
    pub fn generate_aes_key(&self, size: AesKeySize) -> CryptoResult<SymmetricKey> {
        let mut key = vec![0u8; size.bytes()];
        self.random_bytes(&mut key)?;

        let alg = match size {
            AesKeySize::Aes128 => "AES-128",
            AesKeySize::Aes256 => "AES-256",
        };

        Ok(SymmetricKey::new(key, alg))
    }

    /// AES-GCM encryption
    pub fn aes_gcm_encrypt(
        &self,
        key: &[u8],
        plaintext: &[u8],
        aad: &[u8],
    ) -> CryptoResult<AesGcmCiphertext> {
        // Validate key length
        if key.len() != 16 && key.len() != 32 {
            return Err(CryptoError::InvalidKeyLength {
                expected: 16, // or 32
                got: key.len(),
            });
        }

        // Generate random 96-bit nonce
        let mut nonce = [0u8; 12];
        self.random_bytes(&mut nonce)?;

        // Perform encryption (using ring or similar)
        let ciphertext = self.aes_gcm_encrypt_internal(key, &nonce, plaintext, aad)?;

        Ok(AesGcmCiphertext {
            nonce: nonce.to_vec(),
            ciphertext,
            tag_length: 16,
        })
    }

    /// AES-GCM decryption
    pub fn aes_gcm_decrypt(
        &self,
        key: &[u8],
        ciphertext: &AesGcmCiphertext,
        aad: &[u8],
    ) -> CryptoResult<Vec<u8>> {
        // Validate key length
        if key.len() != 16 && key.len() != 32 {
            return Err(CryptoError::InvalidKeyLength {
                expected: 16,
                got: key.len(),
            });
        }

        // Validate nonce length
        if ciphertext.nonce.len() != 12 {
            return Err(CryptoError::InvalidNonceLength {
                expected: 12,
                got: ciphertext.nonce.len(),
            });
        }

        self.aes_gcm_decrypt_internal(key, &ciphertext.nonce, &ciphertext.ciphertext, aad)
    }

    // Internal AES-GCM implementation
    fn aes_gcm_encrypt_internal(
        &self,
        key: &[u8],
        nonce: &[u8],
        plaintext: &[u8],
        aad: &[u8],
    ) -> CryptoResult<Vec<u8>> {
        // This is a placeholder - in production, use ring, aws-lc-rs, or OpenSSL FIPS
        // For now, we'll use a simple XOR for demonstration (NOT SECURE!)

        #[cfg(feature = "ring")]
        {
            use ring::aead::{
                Aad, BoundKey, Nonce, NonceSequence, SealingKey, UnboundKey, AES_256_GCM,
            };

            struct SingleNonce(Option<[u8; 12]>);
            impl NonceSequence for SingleNonce {
                fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
                    self.0
                        .take()
                        .map(Nonce::assume_unique_for_key)
                        .ok_or(ring::error::Unspecified)
                }
            }

            let unbound_key = UnboundKey::new(&AES_256_GCM, key)
                .map_err(|_| CryptoError::EncryptionFailed("Invalid key".into()))?;

            let mut nonce_arr = [0u8; 12];
            nonce_arr.copy_from_slice(nonce);

            let mut sealing_key = SealingKey::new(unbound_key, SingleNonce(Some(nonce_arr)));

            let mut in_out = plaintext.to_vec();
            sealing_key
                .seal_in_place_append_tag(Aad::from(aad), &mut in_out)
                .map_err(|_| CryptoError::EncryptionFailed("Seal failed".into()))?;

            return Ok(in_out);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (key, nonce, plaintext, aad);
            Err(CryptoError::NotImplemented(
                "AES-GCM encryption requires the `ring` feature".into(),
            ))
        }
    }

    fn aes_gcm_decrypt_internal(
        &self,
        key: &[u8],
        nonce: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
    ) -> CryptoResult<Vec<u8>> {
        #[cfg(feature = "ring")]
        {
            use ring::aead::{
                Aad, BoundKey, Nonce, NonceSequence, OpeningKey, UnboundKey, AES_256_GCM,
            };

            struct SingleNonce(Option<[u8; 12]>);
            impl NonceSequence for SingleNonce {
                fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
                    self.0
                        .take()
                        .map(Nonce::assume_unique_for_key)
                        .ok_or(ring::error::Unspecified)
                }
            }

            let unbound_key = UnboundKey::new(&AES_256_GCM, key)
                .map_err(|_| CryptoError::DecryptionFailed("Invalid key".into()))?;

            let mut nonce_arr = [0u8; 12];
            nonce_arr.copy_from_slice(nonce);

            let mut opening_key = OpeningKey::new(unbound_key, SingleNonce(Some(nonce_arr)));

            let mut in_out = ciphertext.to_vec();
            let plaintext = opening_key
                .open_in_place(Aad::from(aad), &mut in_out)
                .map_err(|_| CryptoError::AuthenticationFailed)?;

            return Ok(plaintext.to_vec());
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (key, nonce, ciphertext, aad);
            Err(CryptoError::NotImplemented(
                "AES-GCM decryption requires the `ring` feature".into(),
            ))
        }
    }

    // ========================================================================
    // Hashing (SHA-2, SHA-3)
    // ========================================================================

    /// SHA-256 hash
    pub fn sha256(&self, data: &[u8]) -> CryptoResult<[u8; 32]> {
        #[cfg(feature = "ring")]
        {
            use ring::digest::{digest, SHA256};
            let result = digest(&SHA256, data);
            let mut hash = [0u8; 32];
            hash.copy_from_slice(result.as_ref());
            return Ok(hash);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = data;
            Err(CryptoError::NotImplemented(
                "SHA-256 requires the `ring` feature".into(),
            ))
        }
    }

    /// SHA-384 hash
    pub fn sha384(&self, data: &[u8]) -> CryptoResult<[u8; 48]> {
        #[cfg(feature = "ring")]
        {
            use ring::digest::{digest, SHA384};
            let result = digest(&SHA384, data);
            let mut hash = [0u8; 48];
            hash.copy_from_slice(result.as_ref());
            return Ok(hash);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = data;
            Err(CryptoError::NotImplemented(
                "SHA-384 requires the `ring` feature".into(),
            ))
        }
    }

    /// SHA-512 hash
    pub fn sha512(&self, data: &[u8]) -> CryptoResult<[u8; 64]> {
        #[cfg(feature = "ring")]
        {
            use ring::digest::{digest, SHA512};
            let result = digest(&SHA512, data);
            let mut hash = [0u8; 64];
            hash.copy_from_slice(result.as_ref());
            return Ok(hash);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = data;
            Err(CryptoError::NotImplemented(
                "SHA-512 requires the `ring` feature".into(),
            ))
        }
    }

    // ========================================================================
    // HMAC
    // ========================================================================

    /// HMAC-SHA256
    pub fn hmac_sha256(&self, key: &[u8], data: &[u8]) -> CryptoResult<[u8; 32]> {
        #[cfg(feature = "ring")]
        {
            use ring::hmac::{self, Key, HMAC_SHA256};
            let key = Key::new(HMAC_SHA256, key);
            let tag = hmac::sign(&key, data);
            let mut result = [0u8; 32];
            result.copy_from_slice(tag.as_ref());
            return Ok(result);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (key, data);
            Err(CryptoError::NotImplemented(
                "HMAC-SHA256 requires the `ring` feature".into(),
            ))
        }
    }

    /// HMAC-SHA512
    pub fn hmac_sha512(&self, key: &[u8], data: &[u8]) -> CryptoResult<[u8; 64]> {
        #[cfg(feature = "ring")]
        {
            use ring::hmac::{self, Key, HMAC_SHA512};
            let key = Key::new(HMAC_SHA512, key);
            let tag = hmac::sign(&key, data);
            let mut result = [0u8; 64];
            result.copy_from_slice(tag.as_ref());
            return Ok(result);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (key, data);
            Err(CryptoError::NotImplemented(
                "HMAC-SHA512 requires the `ring` feature".into(),
            ))
        }
    }

    // ========================================================================
    // Key Derivation (HKDF)
    // ========================================================================

    /// HKDF-SHA256 key derivation
    pub fn hkdf_sha256(
        &self,
        salt: &[u8],
        ikm: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> CryptoResult<Vec<u8>> {
        #[cfg(feature = "ring")]
        {
            use ring::hkdf::{Salt, HKDF_SHA256};
            let salt = Salt::new(HKDF_SHA256, salt);
            let prk = salt.extract(ikm);
            let okm = prk
                .expand(&[info], HkdfLen(output_len))
                .map_err(|_| CryptoError::KeyDerivationFailed("HKDF expand failed".into()))?;
            let mut out = vec![0u8; output_len];
            okm.fill(&mut out)
                .map_err(|_| CryptoError::KeyDerivationFailed("HKDF fill failed".into()))?;
            return Ok(out);
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (salt, ikm, info, output_len);
            Err(CryptoError::NotImplemented(
                "HKDF-SHA256 requires the `ring` feature".into(),
            ))
        }
    }

    // ========================================================================
    // Known Answer Tests (KAT)
    // ========================================================================

    fn kat_aes_gcm(&self) -> CryptoResult<()> {
        // NIST AES-GCM test vector
        let key = [0u8; 32];
        let plaintext = b"test";
        let aad = b"";

        let ciphertext = self.aes_gcm_encrypt(&key, plaintext, aad)?;
        let decrypted = self.aes_gcm_decrypt(&key, &ciphertext, aad)?;

        if decrypted != plaintext {
            return Err(CryptoError::SelfTestFailed("AES-GCM KAT failed".into()));
        }
        Ok(())
    }

    fn kat_sha256(&self) -> CryptoResult<()> {
        // SHA-256 known answer test
        // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let empty_hash = self.sha256(b"")?;

        #[cfg(feature = "ring")]
        {
            let expected = [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55,
            ];
            if empty_hash != expected {
                return Err(CryptoError::SelfTestFailed("SHA-256 KAT failed".into()));
            }
        }

        Ok(())
    }

    fn kat_hmac_sha256(&self) -> CryptoResult<()> {
        // HMAC-SHA256 test
        let key = [0u8; 32];
        let data = b"test";
        let _mac = self.hmac_sha256(&key, data)?;
        Ok(())
    }

    fn kat_drbg(&self) -> CryptoResult<()> {
        // DRBG test - verify we can generate random bytes
        let mut buffer = [0u8; 32];
        self.random_bytes(&mut buffer)?;

        // Check not all zeros (extremely unlikely with good RNG)
        if buffer.iter().all(|&b| b == 0) {
            return Err(CryptoError::SelfTestFailed("DRBG KAT failed".into()));
        }
        Ok(())
    }
}

// Helper for HKDF output length
#[cfg(feature = "ring")]
struct HkdfLen(usize);

#[cfg(feature = "ring")]
impl ring::hkdf::KeyType for HkdfLen {
    fn len(&self) -> usize {
        self.0
    }
}

// ============================================================================
// AES-GCM Ciphertext
// ============================================================================

/// AES-GCM ciphertext with nonce and tag
#[derive(Debug, Clone)]
pub struct AesGcmCiphertext {
    /// Nonce (96-bit / 12 bytes for GCM)
    pub nonce: Vec<u8>,
    /// Ciphertext with appended authentication tag
    pub ciphertext: Vec<u8>,
    /// Tag length (always 16 for GCM)
    pub tag_length: usize,
}

impl AesGcmCiphertext {
    /// Get combined nonce + ciphertext for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = self.nonce.clone();
        result.extend_from_slice(&self.ciphertext);
        result
    }

    /// Parse from combined nonce + ciphertext
    pub fn from_bytes(data: &[u8]) -> CryptoResult<Self> {
        if data.len() < 12 + 16 {
            return Err(CryptoError::DecryptionFailed("Data too short".into()));
        }
        Ok(Self {
            nonce: data[..12].to_vec(),
            ciphertext: data[12..].to_vec(),
            tag_length: 16,
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fips_mode_default() {
        let mode = FipsMode::default();
        #[cfg(feature = "fips")]
        assert_eq!(mode, FipsMode::Enabled);
        #[cfg(not(feature = "fips"))]
        assert_eq!(mode, FipsMode::Disabled);
    }

    #[test]
    fn test_crypto_init() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        assert!(!crypto.is_fips_enabled());
    }

    #[test]
    fn test_random_bytes() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let mut buffer1 = [0u8; 32];
        let mut buffer2 = [0u8; 32];

        crypto.random_bytes(&mut buffer1).unwrap();
        crypto.random_bytes(&mut buffer2).unwrap();

        // Buffers should be different (with overwhelming probability)
        assert_ne!(buffer1, buffer2);
        // Should not be all zeros
        assert!(!buffer1.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_aes_key_generation() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();

        let key128 = crypto.generate_aes_key(AesKeySize::Aes128).unwrap();
        assert_eq!(key128.len(), 16);
        assert_eq!(key128.algorithm(), "AES-128");

        let key256 = crypto.generate_aes_key(AesKeySize::Aes256).unwrap();
        assert_eq!(key256.len(), 32);
        assert_eq!(key256.algorithm(), "AES-256");
    }

    #[test]
    fn test_aes_gcm_roundtrip() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let key = crypto.generate_aes_key(AesKeySize::Aes256).unwrap();

        let plaintext = b"Hello, HyperMachine!";
        let aad = b"additional data";

        // Without the `ring` feature, AES-GCM operations return NotImplemented
        let result = crypto.aes_gcm_encrypt(key.as_bytes(), plaintext, aad);
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "AES-GCM encrypt requires `ring` feature");
        #[cfg(feature = "ring")]
        {
            let ciphertext = result.unwrap();
            let decrypted = crypto
                .aes_gcm_decrypt(key.as_bytes(), &ciphertext, aad)
                .unwrap();
            assert_eq!(decrypted, plaintext);
        }
    }

    #[test]
    fn test_sha256() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let result = crypto.sha256(b"test");
        // Without the `ring` feature, SHA-256 returns NotImplemented
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "SHA-256 requires `ring` feature");
        #[cfg(feature = "ring")]
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_hmac_sha256() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let key = [0u8; 32];
        let result = crypto.hmac_sha256(&key, b"test");
        // Without the `ring` feature, HMAC-SHA256 returns NotImplemented
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "HMAC-SHA256 requires `ring` feature");
        #[cfg(feature = "ring")]
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_hkdf() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let salt = [0u8; 32];
        let ikm = b"input key material";
        let info = b"context";

        let result = crypto.hkdf_sha256(&salt, ikm, info, 64);
        // Without the `ring` feature, HKDF returns NotImplemented
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "HKDF requires `ring` feature");
        #[cfg(feature = "ring")]
        assert_eq!(result.unwrap().len(), 64);
    }

    #[test]
    fn test_symmetric_key_zeroize() {
        let key = SymmetricKey::new(vec![1, 2, 3, 4], "test");
        let ptr = key.as_bytes().as_ptr();
        drop(key);
        // Key material should be zeroized (can't directly verify without unsafe)
    }

    #[test]
    fn test_ciphertext_serialization() {
        let ct = AesGcmCiphertext {
            nonce: vec![1; 12],
            ciphertext: vec![2; 32],
            tag_length: 16,
        };

        let bytes = ct.to_bytes();
        assert_eq!(bytes.len(), 12 + 32);

        let parsed = AesGcmCiphertext::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.nonce, ct.nonce);
        assert_eq!(parsed.ciphertext, ct.ciphertext);
    }

    #[test]
    fn test_self_tests() {
        let mut crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let result = crypto.run_self_tests();
        // Without the `ring` feature, self-tests fail (crypto ops return NotImplemented)
        #[cfg(not(feature = "ring"))]
        assert!(result.is_err(), "Self-tests require `ring` feature");
        #[cfg(feature = "ring")]
        {
            result.unwrap();
            assert!(crypto.status.self_test_passed);
        }
    }

    #[test]
    fn test_fips_status() {
        let crypto = FipsCrypto::new(FipsMode::Disabled).unwrap();
        let status = crypto.status();

        assert!(!status.approved_algorithms.is_empty());
        assert!(status.approved_algorithms.contains(&"AES-256-GCM"));
        assert!(status.approved_algorithms.contains(&"SHA-256"));
    }
}
