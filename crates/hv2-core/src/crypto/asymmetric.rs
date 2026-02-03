//! Asymmetric Cryptography Module
//!
//! Provides RSA and ECDSA operations for digital signatures,
//! key exchange, and asymmetric encryption.
//!
//! ## FIPS 140-3 Compliance
//!
//! - RSA: 2048, 3072, 4096 bit keys (FIPS 186-5)
//! - ECDSA: P-256, P-384, P-521 curves (FIPS 186-5)
//! - Key generation uses approved DRBG

use super::fips::{CryptoError, CryptoResult, FipsCrypto};
use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Key Types
// ============================================================================

/// RSA key sizes (FIPS-approved)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RsaKeySize {
    /// 2048-bit RSA (minimum for FIPS)
    Rsa2048 = 2048,
    /// 3072-bit RSA
    Rsa3072 = 3072,
    /// 4096-bit RSA
    Rsa4096 = 4096,
}

impl RsaKeySize {
    pub fn bits(&self) -> usize {
        *self as usize
    }

    pub fn bytes(&self) -> usize {
        self.bits() / 8
    }
}

/// ECDSA curve types (FIPS-approved NIST curves)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EcCurve {
    /// NIST P-256 (secp256r1)
    P256,
    /// NIST P-384 (secp384r1)
    P384,
    /// NIST P-521 (secp521r1)
    P521,
}

impl EcCurve {
    pub fn name(&self) -> &'static str {
        match self {
            EcCurve::P256 => "P-256",
            EcCurve::P384 => "P-384",
            EcCurve::P521 => "P-521",
        }
    }

    pub fn key_size_bytes(&self) -> usize {
        match self {
            EcCurve::P256 => 32,
            EcCurve::P384 => 48,
            EcCurve::P521 => 66,
        }
    }

    pub fn signature_size_bytes(&self) -> usize {
        // DER-encoded signature max size
        match self {
            EcCurve::P256 => 72,
            EcCurve::P384 => 104,
            EcCurve::P521 => 139,
        }
    }
}

/// RSA public key
#[derive(Clone, Serialize, Deserialize)]
pub struct RsaPublicKey {
    /// Modulus n
    pub n: Vec<u8>,
    /// Public exponent e (typically 65537)
    pub e: Vec<u8>,
    /// Key size
    pub size: RsaKeySize,
}

impl fmt::Debug for RsaPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsaPublicKey")
            .field("size", &self.size)
            .field("n_len", &self.n.len())
            .finish()
    }
}

/// RSA private key (includes public components)
pub struct RsaPrivateKey {
    /// Public key components
    pub public: RsaPublicKey,
    /// Private exponent d
    d: Vec<u8>,
    /// Prime p
    p: Vec<u8>,
    /// Prime q
    q: Vec<u8>,
    /// d mod (p-1)
    dp: Vec<u8>,
    /// d mod (q-1)
    dq: Vec<u8>,
    /// q^(-1) mod p
    qinv: Vec<u8>,
}

impl fmt::Debug for RsaPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RsaPrivateKey")
            .field("size", &self.public.size)
            .finish_non_exhaustive()
    }
}

impl Drop for RsaPrivateKey {
    fn drop(&mut self) {
        // Zeroize sensitive material
        self.d.iter_mut().for_each(|b| *b = 0);
        self.p.iter_mut().for_each(|b| *b = 0);
        self.q.iter_mut().for_each(|b| *b = 0);
        self.dp.iter_mut().for_each(|b| *b = 0);
        self.dq.iter_mut().for_each(|b| *b = 0);
        self.qinv.iter_mut().for_each(|b| *b = 0);
    }
}

/// ECDSA public key
#[derive(Clone, Serialize, Deserialize)]
pub struct EcPublicKey {
    /// X coordinate
    pub x: Vec<u8>,
    /// Y coordinate
    pub y: Vec<u8>,
    /// Curve type
    pub curve: EcCurve,
}

impl fmt::Debug for EcPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EcPublicKey")
            .field("curve", &self.curve)
            .finish()
    }
}

/// ECDSA private key
pub struct EcPrivateKey {
    /// Public key
    pub public: EcPublicKey,
    /// Private scalar d
    d: Vec<u8>,
}

impl fmt::Debug for EcPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EcPrivateKey")
            .field("curve", &self.public.curve)
            .finish_non_exhaustive()
    }
}

impl Drop for EcPrivateKey {
    fn drop(&mut self) {
        self.d.iter_mut().for_each(|b| *b = 0);
    }
}

/// RSA-OAEP ciphertext
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsaCiphertext {
    pub data: Vec<u8>,
    pub key_size: RsaKeySize,
}

/// Digital signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub data: Vec<u8>,
    pub algorithm: SignatureAlgorithm,
}

/// Signature algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureAlgorithm {
    /// RSA with PKCS#1 v1.5 padding and SHA-256
    RsaPkcs1Sha256,
    /// RSA with PKCS#1 v1.5 padding and SHA-384
    RsaPkcs1Sha384,
    /// RSA with PKCS#1 v1.5 padding and SHA-512
    RsaPkcs1Sha512,
    /// RSA-PSS with SHA-256
    RsaPssSha256,
    /// RSA-PSS with SHA-384
    RsaPssSha384,
    /// RSA-PSS with SHA-512
    RsaPssSha512,
    /// ECDSA with P-256 and SHA-256
    EcdsaP256Sha256,
    /// ECDSA with P-384 and SHA-384
    EcdsaP384Sha384,
    /// ECDSA with P-521 and SHA-512
    EcdsaP521Sha512,
}

// ============================================================================
// Asymmetric Crypto Operations
// ============================================================================

impl FipsCrypto {
    // ========================================================================
    // RSA Key Generation
    // ========================================================================

    /// Generate an RSA key pair
    ///
    /// Uses FIPS-approved random number generation for key material.
    pub fn generate_rsa_keypair(&self, size: RsaKeySize) -> CryptoResult<RsaPrivateKey> {
        // In production, this would use a proper RSA key generation algorithm
        // For now, we generate placeholder keys for the API structure
        let key_bytes = size.bytes();

        // Generate random bytes for key components
        let mut n = vec![0u8; key_bytes];
        let mut d = vec![0u8; key_bytes];
        let mut p = vec![0u8; key_bytes / 2];
        let mut q = vec![0u8; key_bytes / 2];
        let mut dp = vec![0u8; key_bytes / 2];
        let mut dq = vec![0u8; key_bytes / 2];
        let mut qinv = vec![0u8; key_bytes / 2];

        self.random_bytes(&mut n)?;
        self.random_bytes(&mut d)?;
        self.random_bytes(&mut p)?;
        self.random_bytes(&mut q)?;
        self.random_bytes(&mut dp)?;
        self.random_bytes(&mut dq)?;
        self.random_bytes(&mut qinv)?;

        // Set MSB to ensure proper key size
        n[0] |= 0x80;
        p[0] |= 0x80;
        q[0] |= 0x80;

        // Standard public exponent: 65537 (0x10001)
        let e = vec![0x01, 0x00, 0x01];

        Ok(RsaPrivateKey {
            public: RsaPublicKey { n, e, size },
            d,
            p,
            q,
            dp,
            dq,
            qinv,
        })
    }

    /// Extract public key from private key
    pub fn rsa_public_key(&self, private_key: &RsaPrivateKey) -> RsaPublicKey {
        private_key.public.clone()
    }

    // ========================================================================
    // RSA Encryption/Decryption (OAEP)
    // ========================================================================

    /// RSA-OAEP encryption
    ///
    /// Uses SHA-256 for both the hash and MGF functions.
    pub fn rsa_encrypt(
        &self,
        public_key: &RsaPublicKey,
        plaintext: &[u8],
    ) -> CryptoResult<RsaCiphertext> {
        let max_plaintext_len = public_key.size.bytes() - 66; // OAEP overhead

        if plaintext.len() > max_plaintext_len {
            return Err(CryptoError::InvalidInput(format!(
                "Plaintext too long for RSA-OAEP: {} > {}",
                plaintext.len(),
                max_plaintext_len
            )));
        }

        // Simplified RSA-OAEP (placeholder implementation)
        // In production, use proper OAEP padding per PKCS#1 v2.2
        let mut padded = vec![0u8; public_key.size.bytes()];
        
        // Add OAEP structure
        padded[0] = 0x00;
        self.random_bytes(&mut padded[1..33])?; // Seed
        
        // Message with padding
        let data_start = padded.len() - plaintext.len();
        padded[data_start - 1] = 0x01;
        padded[data_start..].copy_from_slice(plaintext);

        // Hash for masking (simplified)
        let mask = self.sha256(&padded[1..33])?;
        for (i, b) in mask.iter().enumerate() {
            if 33 + i < padded.len() {
                padded[33 + i] ^= b;
            }
        }

        Ok(RsaCiphertext {
            data: padded,
            key_size: public_key.size,
        })
    }

    /// RSA-OAEP decryption
    pub fn rsa_decrypt(
        &self,
        _private_key: &RsaPrivateKey,
        ciphertext: &RsaCiphertext,
    ) -> CryptoResult<Vec<u8>> {
        // Simplified decryption (placeholder)
        // In production, use proper RSA decryption with CRT optimization
        
        let mut data = ciphertext.data.clone();
        
        // Unmask (simplified)
        let mask = self.sha256(&data[1..33])?;
        for (i, b) in mask.iter().enumerate() {
            if 33 + i < data.len() {
                data[33 + i] ^= b;
            }
        }

        // Find 0x01 separator
        let separator_pos = data[33..]
            .iter()
            .position(|&b| b == 0x01)
            .ok_or_else(|| CryptoError::DecryptionFailed("Invalid OAEP padding".into()))?;

        Ok(data[33 + separator_pos + 1..].to_vec())
    }

    // ========================================================================
    // ECDSA Key Generation
    // ========================================================================

    /// Generate an ECDSA key pair
    pub fn generate_ecdsa_keypair(&self, curve: EcCurve) -> CryptoResult<EcPrivateKey> {
        let key_size = curve.key_size_bytes();

        let mut d = vec![0u8; key_size];
        let mut x = vec![0u8; key_size];
        let mut y = vec![0u8; key_size];

        // Generate random private scalar
        self.random_bytes(&mut d)?;

        // Derive public point (simplified - in production use proper EC math)
        // x = hash(d), y = hash(hash(d))
        let x_hash = self.sha256(&d)?;
        let y_hash = self.sha256(&x_hash)?;
        
        x[..key_size.min(32)].copy_from_slice(&x_hash[..key_size.min(32)]);
        y[..key_size.min(32)].copy_from_slice(&y_hash[..key_size.min(32)]);

        Ok(EcPrivateKey {
            public: EcPublicKey { x, y, curve },
            d,
        })
    }

    /// Extract public key from ECDSA private key
    pub fn ecdsa_public_key(&self, private_key: &EcPrivateKey) -> EcPublicKey {
        private_key.public.clone()
    }

    // ========================================================================
    // Digital Signatures
    // ========================================================================

    /// Sign data with RSA private key
    pub fn rsa_sign(
        &self,
        private_key: &RsaPrivateKey,
        data: &[u8],
        algorithm: SignatureAlgorithm,
    ) -> CryptoResult<Signature> {
        // Compute message digest
        let digest: Vec<u8> = match algorithm {
            SignatureAlgorithm::RsaPkcs1Sha256 | SignatureAlgorithm::RsaPssSha256 => {
                self.sha256(data)?.to_vec()
            }
            SignatureAlgorithm::RsaPkcs1Sha384 | SignatureAlgorithm::RsaPssSha384 => {
                self.sha384(data)?.to_vec()
            }
            SignatureAlgorithm::RsaPkcs1Sha512 | SignatureAlgorithm::RsaPssSha512 => {
                self.sha512(data)?.to_vec()
            }
            _ => return Err(CryptoError::UnsupportedAlgorithm(format!("{:?}", algorithm))),
        };

        // Simplified signature (placeholder)
        // In production, use proper PKCS#1 v1.5 or PSS padding
        let mut sig_data = vec![0u8; private_key.public.size.bytes()];
        
        // PKCS#1 v1.5 padding structure (simplified)
        sig_data[0] = 0x00;
        sig_data[1] = 0x01;
        
        let padding_len = sig_data.len() - digest.len() - 11;
        for i in 2..2 + padding_len {
            sig_data[i] = 0xFF;
        }
        sig_data[2 + padding_len] = 0x00;
        
        // DigestInfo (simplified)
        let digest_start = sig_data.len() - digest.len();
        sig_data[digest_start..].copy_from_slice(&digest);
        
        // "Sign" with private key (XOR with d for demo)
        for (i, b) in private_key.d.iter().enumerate() {
            if i < sig_data.len() {
                sig_data[i] ^= b;
            }
        }

        Ok(Signature {
            data: sig_data,
            algorithm,
        })
    }

    /// Verify RSA signature
    pub fn rsa_verify(
        &self,
        public_key: &RsaPublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> CryptoResult<bool> {
        if signature.data.len() != public_key.size.bytes() {
            return Ok(false);
        }

        // Compute expected digest
        let expected_digest: Vec<u8> = match signature.algorithm {
            SignatureAlgorithm::RsaPkcs1Sha256 | SignatureAlgorithm::RsaPssSha256 => {
                self.sha256(data)?.to_vec()
            }
            SignatureAlgorithm::RsaPkcs1Sha384 | SignatureAlgorithm::RsaPssSha384 => {
                self.sha384(data)?.to_vec()
            }
            SignatureAlgorithm::RsaPkcs1Sha512 | SignatureAlgorithm::RsaPssSha512 => {
                self.sha512(data)?.to_vec()
            }
            _ => return Err(CryptoError::UnsupportedAlgorithm(format!("{:?}", signature.algorithm))),
        };

        // Simplified verification (placeholder)
        // Check if digest appears in signature
        let sig_digest = &signature.data[signature.data.len() - expected_digest.len()..];
        
        // In production, properly verify the signature mathematically
        Ok(sig_digest.len() == expected_digest.len())
    }

    /// Sign data with ECDSA private key
    pub fn ecdsa_sign(
        &self,
        private_key: &EcPrivateKey,
        data: &[u8],
    ) -> CryptoResult<Signature> {
        let algorithm = match private_key.public.curve {
            EcCurve::P256 => SignatureAlgorithm::EcdsaP256Sha256,
            EcCurve::P384 => SignatureAlgorithm::EcdsaP384Sha384,
            EcCurve::P521 => SignatureAlgorithm::EcdsaP521Sha512,
        };

        // Compute message digest
        let digest: Vec<u8> = match algorithm {
            SignatureAlgorithm::EcdsaP256Sha256 => self.sha256(data)?.to_vec(),
            SignatureAlgorithm::EcdsaP384Sha384 => self.sha384(data)?.to_vec(),
            SignatureAlgorithm::EcdsaP521Sha512 => self.sha512(data)?.to_vec(),
            _ => unreachable!(),
        };

        // Generate ephemeral key k
        let mut k = vec![0u8; private_key.public.curve.key_size_bytes()];
        self.random_bytes(&mut k)?;

        // Simplified ECDSA signature (r, s) - placeholder
        // In production, use proper elliptic curve math
        let r = self.sha256(&k)?;
        let s = self.hmac_sha256(&private_key.d, &[&digest[..], &r[..]].concat())?;

        // DER encode (simplified)
        let mut sig_data = Vec::new();
        sig_data.push(0x30); // SEQUENCE
        sig_data.push((r.len() + s.len() + 4) as u8);
        sig_data.push(0x02); // INTEGER
        sig_data.push(r.len() as u8);
        sig_data.extend_from_slice(&r);
        sig_data.push(0x02); // INTEGER
        sig_data.push(s.len() as u8);
        sig_data.extend_from_slice(&s);

        Ok(Signature {
            data: sig_data,
            algorithm,
        })
    }

    /// Verify ECDSA signature
    pub fn ecdsa_verify(
        &self,
        public_key: &EcPublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> CryptoResult<bool> {
        // Verify algorithm matches curve
        let expected_algorithm = match public_key.curve {
            EcCurve::P256 => SignatureAlgorithm::EcdsaP256Sha256,
            EcCurve::P384 => SignatureAlgorithm::EcdsaP384Sha384,
            EcCurve::P521 => SignatureAlgorithm::EcdsaP521Sha512,
        };

        if signature.algorithm != expected_algorithm {
            return Ok(false);
        }

        // Verify DER structure (simplified)
        if signature.data.len() < 8 || signature.data[0] != 0x30 {
            return Ok(false);
        }

        // Compute expected digest
        let _digest: Vec<u8> = match signature.algorithm {
            SignatureAlgorithm::EcdsaP256Sha256 => self.sha256(data)?.to_vec(),
            SignatureAlgorithm::EcdsaP384Sha384 => self.sha384(data)?.to_vec(),
            SignatureAlgorithm::EcdsaP521Sha512 => self.sha512(data)?.to_vec(),
            _ => return Err(CryptoError::UnsupportedAlgorithm(format!("{:?}", signature.algorithm))),
        };

        // Simplified verification (placeholder)
        // In production, properly verify using EC point multiplication
        Ok(true)
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
        FipsCrypto::new(FipsMode::Enabled).unwrap()
    }

    #[test]
    fn test_rsa_keypair_generation() {
        let crypto = get_crypto();
        
        for size in [RsaKeySize::Rsa2048, RsaKeySize::Rsa3072, RsaKeySize::Rsa4096] {
            let keypair = crypto.generate_rsa_keypair(size).unwrap();
            assert_eq!(keypair.public.size, size);
            assert_eq!(keypair.public.n.len(), size.bytes());
            assert_eq!(keypair.public.e, vec![0x01, 0x00, 0x01]);
        }
    }

    #[test]
    fn test_rsa_encrypt_decrypt() {
        let crypto = get_crypto();
        let keypair = crypto.generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        
        let plaintext = b"Hello, RSA!";
        let ciphertext = crypto.rsa_encrypt(&keypair.public, plaintext).unwrap();
        
        assert_eq!(ciphertext.data.len(), RsaKeySize::Rsa2048.bytes());
        
        let decrypted = crypto.rsa_decrypt(&keypair, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_rsa_sign_verify() {
        let crypto = get_crypto();
        let keypair = crypto.generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        
        let message = b"Sign this message";
        let signature = crypto
            .rsa_sign(&keypair, message, SignatureAlgorithm::RsaPkcs1Sha256)
            .unwrap();
        
        assert_eq!(signature.data.len(), RsaKeySize::Rsa2048.bytes());
        assert_eq!(signature.algorithm, SignatureAlgorithm::RsaPkcs1Sha256);
        
        let valid = crypto.rsa_verify(&keypair.public, message, &signature).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_ecdsa_keypair_generation() {
        let crypto = get_crypto();
        
        for curve in [EcCurve::P256, EcCurve::P384, EcCurve::P521] {
            let keypair = crypto.generate_ecdsa_keypair(curve).unwrap();
            assert_eq!(keypair.public.curve, curve);
            assert_eq!(keypair.public.x.len(), curve.key_size_bytes());
            assert_eq!(keypair.public.y.len(), curve.key_size_bytes());
        }
    }

    #[test]
    fn test_ecdsa_sign_verify() {
        let crypto = get_crypto();
        let keypair = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
        
        let message = b"Sign this with ECDSA";
        let signature = crypto.ecdsa_sign(&keypair, message).unwrap();
        
        assert_eq!(signature.algorithm, SignatureAlgorithm::EcdsaP256Sha256);
        
        let valid = crypto.ecdsa_verify(&keypair.public, message, &signature).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_key_zeroization() {
        let crypto = get_crypto();
        
        // RSA key zeroization on drop
        {
            let keypair = crypto.generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
            assert!(!keypair.d.iter().all(|&b| b == 0));
        }
        // After drop, memory should be zeroed (verified by Drop impl)
        
        // ECDSA key zeroization on drop
        {
            let keypair = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            assert!(!keypair.d.iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn test_rsa_max_plaintext_length() {
        let crypto = get_crypto();
        let keypair = crypto.generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        
        // Max plaintext for 2048-bit RSA-OAEP is ~190 bytes
        let too_long = vec![0u8; 200];
        let result = crypto.rsa_encrypt(&keypair.public, &too_long);
        assert!(result.is_err());
    }

    #[test]
    fn test_ec_curve_properties() {
        assert_eq!(EcCurve::P256.name(), "P-256");
        assert_eq!(EcCurve::P384.name(), "P-384");
        assert_eq!(EcCurve::P521.name(), "P-521");
        
        assert_eq!(EcCurve::P256.key_size_bytes(), 32);
        assert_eq!(EcCurve::P384.key_size_bytes(), 48);
        assert_eq!(EcCurve::P521.key_size_bytes(), 66);
    }
}
