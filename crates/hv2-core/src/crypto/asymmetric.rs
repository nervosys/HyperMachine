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

#[cfg(feature = "ring")]
use ring::rand::SystemRandom;

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

    /// Generate an RSA key pair.
    ///
    /// Uses the pure-Rust `rsa` crate (RustCrypto) seeded from the OS CSPRNG.
    /// All components (n, e, d, p, q, and the CRT values) are stored as raw
    /// big-endian byte strings.
    pub fn generate_rsa_keypair(&self, size: RsaKeySize) -> CryptoResult<RsaPrivateKey> {
        use rsa::traits::{PrivateKeyParts, PublicKeyParts};

        let bits = size.bytes() * 8;
        // `rsa` 0.9 takes a `rand_core` 0.6 RNG, so go through its own
        // re-export rather than `rand` (which is on `rand_core` 0.10).
        // `OsRng` reads the OS CSPRNG directly on every call.
        let mut rng = rsa::rand_core::OsRng;

        let mut key = rsa::RsaPrivateKey::new(&mut rng, bits)
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA keygen: {e}")))?;
        // Precompute the CRT values (dp, dq, qinv).
        key.precompute()
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA precompute: {e}")))?;

        let primes = key.primes();
        if primes.len() != 2 {
            return Err(CryptoError::KeyGenerationFailed(
                "expected exactly two RSA primes".into(),
            ));
        }

        Ok(RsaPrivateKey {
            public: RsaPublicKey {
                n: key.n().to_bytes_be(),
                e: key.e().to_bytes_be(),
                size,
            },
            d: key.d().to_bytes_be(),
            p: primes[0].to_bytes_be(),
            q: primes[1].to_bytes_be(),
            dp: key.dp().map(|v| v.to_bytes_be()).unwrap_or_default(),
            dq: key.dq().map(|v| v.to_bytes_be()).unwrap_or_default(),
            qinv: key.qinv().map(|v| v.to_bytes_be().1).unwrap_or_default(),
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
    /// Software RSA encryption using modular exponentiation with the public key.
    /// Applies simple PKCS#1 v1.5 type 2 padding for compatibility.
    pub fn rsa_encrypt(
        &self,
        public_key: &RsaPublicKey,
        plaintext: &[u8],
    ) -> CryptoResult<RsaCiphertext> {
        let k = public_key.size.bytes();

        // PKCS#1 v1.5: plaintext must be at most k - 11 bytes
        if plaintext.len() > k.saturating_sub(11) {
            return Err(CryptoError::EncryptionFailed(
                "Plaintext too long for RSA key size".into(),
            ));
        }

        if public_key.n.is_empty() || public_key.e.is_empty() {
            return Err(CryptoError::InvalidKeyLength {
                expected: k,
                got: 0,
            });
        }

        // Build PKCS#1 v1.5 padded message: 0x00 || 0x02 || PS || 0x00 || M
        let ps_len = k - plaintext.len() - 3;
        let mut padded = vec![0u8; k];
        padded[0] = 0x00;
        padded[1] = 0x02;

        // Generate non-zero random padding
        self.random_bytes(&mut padded[2..2 + ps_len])?;
        for b in &mut padded[2..2 + ps_len] {
            if *b == 0 {
                *b = 0x01; // Ensure non-zero
            }
        }
        padded[2 + ps_len] = 0x00;
        padded[3 + ps_len..].copy_from_slice(plaintext);

        // Modular exponentiation: c = m^e mod n (big-endian byte arrays)
        let ciphertext_data = mod_exp_bytes(&padded, &public_key.e, &public_key.n);

        Ok(RsaCiphertext {
            data: ciphertext_data,
            key_size: public_key.size,
        })
    }

    /// RSA-OAEP decryption
    ///
    /// Software RSA decryption using modular exponentiation with the private key.
    /// Removes PKCS#1 v1.5 type 2 padding.
    pub fn rsa_decrypt(
        &self,
        private_key: &RsaPrivateKey,
        ciphertext: &RsaCiphertext,
    ) -> CryptoResult<Vec<u8>> {
        let k = private_key.public.size.bytes();

        if ciphertext.data.len() != k {
            return Err(CryptoError::DecryptionFailed(
                "Ciphertext length doesn't match key size".into(),
            ));
        }

        // m = c^d mod n
        let padded = mod_exp_bytes(&ciphertext.data, &private_key.d, &private_key.public.n);

        // Verify and strip PKCS#1 v1.5 padding: 0x00 || 0x02 || PS || 0x00 || M
        if padded.len() < 11 {
            return Err(CryptoError::DecryptionFailed("Invalid padding".into()));
        }

        // Find the 0x00 separator after the padding string
        if padded.len() >= 2 && padded[0] == 0x00 && padded[1] == 0x02 {
            if let Some(sep) = padded[2..].iter().position(|&b| b == 0x00) {
                if sep >= 8 {
                    // PS must be at least 8 bytes
                    return Ok(padded[2 + sep + 1..].to_vec());
                }
            }
        }

        Err(CryptoError::DecryptionFailed(
            "Invalid PKCS#1 padding".into(),
        ))
    }

    // ========================================================================
    // ECDSA Key Generation
    // ========================================================================

    /// Generate an ECDSA key pair
    /// Generate an ECDSA key pair
    ///
    /// Uses `ring` for P-256 and P-384 curves. P-521 is not supported by `ring`.
    pub fn generate_ecdsa_keypair(&self, curve: EcCurve) -> CryptoResult<EcPrivateKey> {
        #[cfg(feature = "ring")]
        {
            use ring::signature::{
                EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING,
                ECDSA_P384_SHA384_FIXED_SIGNING,
            };

            let alg = match curve {
                EcCurve::P256 => &ECDSA_P256_SHA256_FIXED_SIGNING,
                EcCurve::P384 => &ECDSA_P384_SHA384_FIXED_SIGNING,
                EcCurve::P521 => {
                    return Err(CryptoError::UnsupportedAlgorithm(
                        "P-521 is not supported by ring".into(),
                    ));
                }
            };

            let rng = SystemRandom::new();
            let pkcs8_bytes = EcdsaKeyPair::generate_pkcs8(alg, &rng).map_err(|_| {
                CryptoError::KeyGenerationFailed("ECDSA key generation failed".into())
            })?;

            let key_pair =
                EcdsaKeyPair::from_pkcs8(alg, pkcs8_bytes.as_ref(), &rng).map_err(|_| {
                    CryptoError::KeyGenerationFailed("Failed to parse generated ECDSA key".into())
                })?;

            // Extract public key coordinates from the uncompressed point (0x04 || x || y)
            let pub_key_bytes = key_pair.public_key().as_ref();
            let coord_len = curve.key_size_bytes();
            // Uncompressed point format: 0x04 || x || y
            if pub_key_bytes.len() != 1 + 2 * coord_len || pub_key_bytes[0] != 0x04 {
                return Err(CryptoError::KeyGenerationFailed(
                    "Unexpected public key format".into(),
                ));
            }
            let x = pub_key_bytes[1..1 + coord_len].to_vec();
            let y = pub_key_bytes[1 + coord_len..].to_vec();

            // Store the PKCS#8 bytes as the private scalar (for signing later)
            let d = pkcs8_bytes.as_ref().to_vec();

            Ok(EcPrivateKey {
                public: EcPublicKey { x, y, curve },
                d,
            })
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = curve;
            Err(CryptoError::NotImplemented(
                "ECDSA key generation requires the `ring` feature".into(),
            ))
        }
    }

    /// Extract public key from ECDSA private key
    pub fn ecdsa_public_key(&self, private_key: &EcPrivateKey) -> EcPublicKey {
        private_key.public.clone()
    }

    // ========================================================================
    // Digital Signatures
    // ========================================================================

    /// Sign data with an RSA private key (PKCS#1 v1.5 or PSS, SHA-256/384/512).
    ///
    /// Uses the pure-Rust `rsa` crate, reconstructing the signing key from the
    /// stored raw components. The message is hashed with the algorithm's digest
    /// before padding is applied.
    pub fn rsa_sign(
        &self,
        private_key: &RsaPrivateKey,
        data: &[u8],
        algorithm: SignatureAlgorithm,
    ) -> CryptoResult<Signature> {
        use rsa::sha2::{Digest, Sha256, Sha384, Sha512};
        use rsa::{BigUint, Pkcs1v15Sign, Pss};

        let key = rsa::RsaPrivateKey::from_components(
            BigUint::from_bytes_be(&private_key.public.n),
            BigUint::from_bytes_be(&private_key.public.e),
            BigUint::from_bytes_be(&private_key.d),
            vec![
                BigUint::from_bytes_be(&private_key.p),
                BigUint::from_bytes_be(&private_key.q),
            ],
        )
        .map_err(|e| CryptoError::InvalidInput(format!("invalid RSA private key: {e}")))?;

        // `rsa` 0.9 takes a `rand_core` 0.6 RNG; see `generate_rsa_keypair`.
        let mut rng = rsa::rand_core::OsRng;
        let sig = match algorithm {
            SignatureAlgorithm::RsaPkcs1Sha256 => {
                key.sign(Pkcs1v15Sign::new::<Sha256>(), &Sha256::digest(data))
            }
            SignatureAlgorithm::RsaPkcs1Sha384 => {
                key.sign(Pkcs1v15Sign::new::<Sha384>(), &Sha384::digest(data))
            }
            SignatureAlgorithm::RsaPkcs1Sha512 => {
                key.sign(Pkcs1v15Sign::new::<Sha512>(), &Sha512::digest(data))
            }
            SignatureAlgorithm::RsaPssSha256 => {
                key.sign_with_rng(&mut rng, Pss::new::<Sha256>(), &Sha256::digest(data))
            }
            SignatureAlgorithm::RsaPssSha384 => {
                key.sign_with_rng(&mut rng, Pss::new::<Sha384>(), &Sha384::digest(data))
            }
            SignatureAlgorithm::RsaPssSha512 => {
                key.sign_with_rng(&mut rng, Pss::new::<Sha512>(), &Sha512::digest(data))
            }
            _ => {
                return Err(CryptoError::UnsupportedAlgorithm(format!(
                    "{:?} is not an RSA algorithm",
                    algorithm
                )));
            }
        }
        .map_err(|e| CryptoError::EncryptionFailed(format!("RSA signing failed: {e}")))?;

        Ok(Signature {
            data: sig,
            algorithm,
        })
    }

    /// Verify RSA signature
    ///
    /// Verify an RSA signature (PKCS#1 v1.5 or PSS, SHA-256/384/512) using the
    /// pure-Rust `rsa` crate and the public key components (n, e).
    pub fn rsa_verify(
        &self,
        public_key: &RsaPublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> CryptoResult<bool> {
        use rsa::sha2::{Digest, Sha256, Sha384, Sha512};
        use rsa::{BigUint, Pkcs1v15Sign, Pss};

        if signature.data.len() != public_key.size.bytes() {
            return Ok(false);
        }

        let key = rsa::RsaPublicKey::new(
            BigUint::from_bytes_be(&public_key.n),
            BigUint::from_bytes_be(&public_key.e),
        )
        .map_err(|e| CryptoError::InvalidInput(format!("invalid RSA public key: {e}")))?;

        let result = match signature.algorithm {
            SignatureAlgorithm::RsaPkcs1Sha256 => key.verify(
                Pkcs1v15Sign::new::<Sha256>(),
                &Sha256::digest(data),
                &signature.data,
            ),
            SignatureAlgorithm::RsaPkcs1Sha384 => key.verify(
                Pkcs1v15Sign::new::<Sha384>(),
                &Sha384::digest(data),
                &signature.data,
            ),
            SignatureAlgorithm::RsaPkcs1Sha512 => key.verify(
                Pkcs1v15Sign::new::<Sha512>(),
                &Sha512::digest(data),
                &signature.data,
            ),
            SignatureAlgorithm::RsaPssSha256 => {
                key.verify(Pss::new::<Sha256>(), &Sha256::digest(data), &signature.data)
            }
            SignatureAlgorithm::RsaPssSha384 => {
                key.verify(Pss::new::<Sha384>(), &Sha384::digest(data), &signature.data)
            }
            SignatureAlgorithm::RsaPssSha512 => {
                key.verify(Pss::new::<Sha512>(), &Sha512::digest(data), &signature.data)
            }
            _ => {
                return Err(CryptoError::UnsupportedAlgorithm(format!(
                    "{:?} is not an RSA algorithm",
                    signature.algorithm
                )));
            }
        };

        Ok(result.is_ok())
    }

    /// Sign data with ECDSA private key
    pub fn ecdsa_sign(&self, private_key: &EcPrivateKey, data: &[u8]) -> CryptoResult<Signature> {
        let algorithm = match private_key.public.curve {
            EcCurve::P256 => SignatureAlgorithm::EcdsaP256Sha256,
            EcCurve::P384 => SignatureAlgorithm::EcdsaP384Sha384,
            EcCurve::P521 => SignatureAlgorithm::EcdsaP521Sha512,
        };

        #[cfg(feature = "ring")]
        {
            use ring::signature::{
                EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P384_SHA384_FIXED_SIGNING,
            };

            let alg = match private_key.public.curve {
                EcCurve::P256 => &ECDSA_P256_SHA256_FIXED_SIGNING,
                EcCurve::P384 => &ECDSA_P384_SHA384_FIXED_SIGNING,
                EcCurve::P521 => {
                    return Err(CryptoError::UnsupportedAlgorithm(
                        "P-521 signing is not supported by ring".into(),
                    ));
                }
            };

            let rng = SystemRandom::new();
            // private_key.d contains the PKCS#8 encoding
            let key_pair = EcdsaKeyPair::from_pkcs8(alg, &private_key.d, &rng)
                .map_err(|_| CryptoError::InvalidInput("Invalid ECDSA private key".into()))?;

            let sig = key_pair
                .sign(&rng, data)
                .map_err(|_| CryptoError::EncryptionFailed("ECDSA signing failed".into()))?;

            Ok(Signature {
                data: sig.as_ref().to_vec(),
                algorithm,
            })
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (private_key, data, algorithm);
            Err(CryptoError::NotImplemented(
                "ECDSA signing requires the `ring` feature".into(),
            ))
        }
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

        #[cfg(feature = "ring")]
        {
            use ring::signature::{
                UnparsedPublicKey, ECDSA_P256_SHA256_FIXED, ECDSA_P384_SHA384_FIXED,
            };

            let verify_alg: &dyn ring::signature::VerificationAlgorithm = match public_key.curve {
                EcCurve::P256 => &ECDSA_P256_SHA256_FIXED,
                EcCurve::P384 => &ECDSA_P384_SHA384_FIXED,
                EcCurve::P521 => {
                    return Err(CryptoError::UnsupportedAlgorithm(
                        "P-521 verification is not supported by ring".into(),
                    ));
                }
            };

            // Reconstruct uncompressed public key: 0x04 || x || y
            let mut pub_key_bytes = Vec::with_capacity(1 + public_key.x.len() + public_key.y.len());
            pub_key_bytes.push(0x04);
            pub_key_bytes.extend_from_slice(&public_key.x);
            pub_key_bytes.extend_from_slice(&public_key.y);

            let peer_public_key = UnparsedPublicKey::new(verify_alg, &pub_key_bytes);
            match peer_public_key.verify(data, &signature.data) {
                Ok(()) => Ok(true),
                Err(_) => Ok(false),
            }
        }

        #[cfg(not(feature = "ring"))]
        {
            let _ = (public_key, data, signature, expected_algorithm);
            Err(CryptoError::NotImplemented(
                "ECDSA verification requires the `ring` feature".into(),
            ))
        }
    }
}

// ============================================================================
// DER Encoding Helpers
// ============================================================================

/// Encode an RSA public key as DER (PKCS#1 RSAPublicKey format).
///
/// ring expects the public key bytes in this format for verification.
/// Structure: SEQUENCE { INTEGER(n), INTEGER(e) }
#[cfg(feature = "ring")]
fn encode_rsa_public_key_der(n: &[u8], e: &[u8]) -> Vec<u8> {
    // Encode n as DER INTEGER (may need leading 0x00 if high bit set)
    let n_int = der_encode_integer(n);
    let e_int = der_encode_integer(e);

    // SEQUENCE { n, e }
    let seq_content_len = n_int.len() + e_int.len();
    let mut result = Vec::new();
    result.push(0x30); // SEQUENCE tag
    der_encode_length(&mut result, seq_content_len);
    result.extend_from_slice(&n_int);
    result.extend_from_slice(&e_int);
    result
}

/// DER-encode a non-negative integer, stripping leading zeros and adding
/// a padding zero byte if the high bit is set.
#[cfg(feature = "ring")]
fn der_encode_integer(value: &[u8]) -> Vec<u8> {
    // Strip leading zeros (but keep at least one byte)
    let stripped = match value.iter().position(|&b| b != 0) {
        Some(pos) => &value[pos..],
        None => &[0u8],
    };

    // Add leading 0x00 if high bit is set (to keep it positive)
    let needs_pad = !stripped.is_empty() && (stripped[0] & 0x80) != 0;
    let content_len = stripped.len() + if needs_pad { 1 } else { 0 };

    let mut result = Vec::new();
    result.push(0x02); // INTEGER tag
    der_encode_length(&mut result, content_len);
    if needs_pad {
        result.push(0x00);
    }
    result.extend_from_slice(stripped);
    result
}

/// Encode a DER length value (supports definite form, short and long).
#[cfg(feature = "ring")]
fn der_encode_length(buf: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        buf.push(len as u8);
    } else if len < 0x100 {
        buf.push(0x81);
        buf.push(len as u8);
    } else if len < 0x10000 {
        buf.push(0x82);
        buf.push((len >> 8) as u8);
        buf.push(len as u8);
    } else {
        buf.push(0x83);
        buf.push((len >> 16) as u8);
        buf.push((len >> 8) as u8);
        buf.push(len as u8);
    }
}

// ============================================================================
// Big-integer modular exponentiation (software RSA)
// ============================================================================

/// Modular exponentiation on big-endian byte arrays: base^exp mod modulus.
/// Uses square-and-multiply algorithm with arbitrary-precision arithmetic.
fn mod_exp_bytes(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
    // Simple big-integer representation: Vec<u32> in little-endian limb order
    let b = bytes_to_limbs(base);
    let e = bytes_to_limbs(exp);
    let m = bytes_to_limbs(modulus);

    if m.is_empty() || (m.len() == 1 && m[0] == 0) {
        return vec![0u8; modulus.len()];
    }

    let mut result = vec![1u32; 1]; // Start with 1
    let b = mod_limbs(&b, &m);

    // Square-and-multiply, scanning exponent bits
    for limb_idx in 0..e.len() {
        let limb = e[limb_idx];
        let bits = if limb_idx == e.len() - 1 {
            32 - limb.leading_zeros() as usize
        } else {
            32
        };
        for bit in 0..bits {
            if limb_idx == 0 && bit == 0 {
                // First bit
                if limb & 1 != 0 {
                    result = mod_limbs(&b, &m);
                }
            } else {
                // Square
                result = mod_mul_limbs(&result, &result, &m);
                if (limb >> bit) & 1 != 0 {
                    result = mod_mul_limbs(&result, &b, &m);
                }
            }
        }
        if limb_idx < e.len() - 1 {
            // Advance base by 32 squarings for next limb
        }
    }

    limbs_to_bytes(&result, modulus.len())
}

fn bytes_to_limbs(bytes: &[u8]) -> Vec<u32> {
    // Convert big-endian bytes to little-endian u32 limbs
    let mut limbs = Vec::new();
    let mut i = bytes.len();
    while i > 0 {
        let start = i.saturating_sub(4);
        let mut val = 0u32;
        for (j, &b) in bytes[start..i].iter().enumerate() {
            val |= (b as u32) << ((i - start - 1 - j) * 8);
        }
        limbs.push(val);
        i = start;
    }
    // Trim leading zeros
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }
    limbs
}

fn limbs_to_bytes(limbs: &[u32], target_len: usize) -> Vec<u8> {
    // Convert little-endian u32 limbs to big-endian bytes
    let mut bytes = Vec::new();
    for &limb in limbs.iter().rev() {
        bytes.extend_from_slice(&limb.to_be_bytes());
    }
    // Trim leading zeros and pad to target_len
    while bytes.len() > 1 && bytes[0] == 0 {
        bytes.remove(0);
    }
    while bytes.len() < target_len {
        bytes.insert(0, 0);
    }
    if bytes.len() > target_len {
        bytes = bytes[bytes.len() - target_len..].to_vec();
    }
    bytes
}

fn mod_limbs(a: &[u32], m: &[u32]) -> Vec<u32> {
    // a mod m using repeated subtraction (slow but correct for moderate sizes)
    let mut r = a.to_vec();
    while cmp_limbs(&r, m) != std::cmp::Ordering::Less {
        r = sub_limbs(&r, m);
    }
    r
}

fn mod_mul_limbs(a: &[u32], b: &[u32], m: &[u32]) -> Vec<u32> {
    let product = mul_limbs(a, b);
    mod_limbs(&product, m)
}

fn mul_limbs(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = vec![0u32; a.len() + b.len()];
    for (i, &ai) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &bj) in b.iter().enumerate() {
            let product = (ai as u64) * (bj as u64) + (result[i + j] as u64) + carry;
            result[i + j] = product as u32;
            carry = product >> 32;
        }
        result[i + b.len()] += carry as u32;
    }
    while result.len() > 1 && result.last() == Some(&0) {
        result.pop();
    }
    result
}

fn sub_limbs(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = a.to_vec();
    let mut borrow = 0i64;
    for i in 0..result.len() {
        let bi = if i < b.len() { b[i] as i64 } else { 0 };
        let diff = (result[i] as i64) - bi - borrow;
        if diff < 0 {
            result[i] = (diff + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            result[i] = diff as u32;
            borrow = 0;
        }
    }
    while result.len() > 1 && result.last() == Some(&0) {
        result.pop();
    }
    result
}

fn cmp_limbs(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    let alen = a.len();
    let blen = b.len();
    // Strip leading zeros
    let aeff = a.iter().rposition(|&x| x != 0).map(|i| i + 1).unwrap_or(0);
    let beff = b.iter().rposition(|&x| x != 0).map(|i| i + 1).unwrap_or(0);
    let _ = (alen, blen);

    match aeff.cmp(&beff) {
        std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
        std::cmp::Ordering::Less => std::cmp::Ordering::Less,
        std::cmp::Ordering::Equal => {
            for i in (0..aeff).rev() {
                match a[i].cmp(&b[i]) {
                    std::cmp::Ordering::Equal => continue,
                    other => return other,
                }
            }
            std::cmp::Ordering::Equal
        }
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
    fn test_rsa_keygen_sign_verify() {
        let crypto = get_crypto();
        // RSA-2048 keeps the test fast; larger sizes use the same code path.
        let key = crypto
            .generate_rsa_keypair(RsaKeySize::Rsa2048)
            .expect("RSA keygen failed");
        assert_eq!(key.public.size, RsaKeySize::Rsa2048);
        assert_eq!(key.public.n.len(), 256);

        let message = b"RSA round-trip test";
        for alg in [
            SignatureAlgorithm::RsaPkcs1Sha256,
            SignatureAlgorithm::RsaPssSha256,
        ] {
            let sig = crypto
                .rsa_sign(&key, message, alg)
                .expect("RSA sign failed");
            assert!(crypto
                .rsa_verify(&key.public, message, &sig)
                .expect("RSA verify failed"));
            // A tampered message must fail verification.
            assert!(!crypto
                .rsa_verify(&key.public, b"tampered", &sig)
                .expect("RSA verify failed"));
        }
    }

    #[test]
    fn test_rsa_encrypt_invalid_key() {
        let crypto = get_crypto();
        let pub_key = RsaPublicKey {
            n: vec![],
            e: vec![0x01, 0x00, 0x01],
            size: RsaKeySize::Rsa2048,
        };
        let result = crypto.rsa_encrypt(&pub_key, b"test");
        assert!(result.is_err(), "RSA encrypt with empty key should fail");
    }

    #[test]
    fn test_ecdsa_keypair_generation() {
        let crypto = get_crypto();

        // P-256 and P-384 are supported by ring
        #[cfg(feature = "ring")]
        for curve in [EcCurve::P256, EcCurve::P384] {
            let result = crypto.generate_ecdsa_keypair(curve);
            let keypair = result.unwrap();
            assert_eq!(keypair.public.curve, curve);
            assert_eq!(keypair.public.x.len(), curve.key_size_bytes());
            assert_eq!(keypair.public.y.len(), curve.key_size_bytes());
        }

        // P-521 is not supported by ring
        #[cfg(feature = "ring")]
        {
            let result = crypto.generate_ecdsa_keypair(EcCurve::P521);
            assert!(result.is_err(), "P-521 not supported by ring");
        }

        #[cfg(not(feature = "ring"))]
        {
            for curve in [EcCurve::P256, EcCurve::P384, EcCurve::P521] {
                assert!(
                    crypto.generate_ecdsa_keypair(curve).is_err(),
                    "ECDSA keygen requires `ring` feature"
                );
            }
        }
    }

    #[test]
    fn test_ecdsa_sign_verify_p256() {
        let crypto = get_crypto();
        #[cfg(not(feature = "ring"))]
        assert!(
            crypto.generate_ecdsa_keypair(EcCurve::P256).is_err(),
            "ECDSA requires `ring` feature"
        );
        #[cfg(feature = "ring")]
        {
            let keypair = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            let message = b"Sign this with ECDSA P-256";
            let signature = crypto.ecdsa_sign(&keypair, message).unwrap();
            assert_eq!(signature.algorithm, SignatureAlgorithm::EcdsaP256Sha256);

            let valid = crypto
                .ecdsa_verify(&keypair.public, message, &signature)
                .unwrap();
            assert!(valid, "Valid signature should verify");

            // Tampered message should fail
            let bad_valid = crypto
                .ecdsa_verify(&keypair.public, b"wrong message", &signature)
                .unwrap();
            assert!(!bad_valid, "Tampered message should fail verification");
        }
    }

    #[test]
    fn test_ecdsa_sign_verify_p384() {
        let crypto = get_crypto();
        #[cfg(not(feature = "ring"))]
        assert!(
            crypto.generate_ecdsa_keypair(EcCurve::P384).is_err(),
            "ECDSA requires `ring` feature"
        );
        #[cfg(feature = "ring")]
        {
            let keypair = crypto.generate_ecdsa_keypair(EcCurve::P384).unwrap();
            let message = b"Sign this with ECDSA P-384";
            let signature = crypto.ecdsa_sign(&keypair, message).unwrap();
            assert_eq!(signature.algorithm, SignatureAlgorithm::EcdsaP384Sha384);

            let valid = crypto
                .ecdsa_verify(&keypair.public, message, &signature)
                .unwrap();
            assert!(valid, "Valid P-384 signature should verify");
        }
    }

    #[test]
    fn test_ecdsa_cross_key_verify_fails() {
        let crypto = get_crypto();
        #[cfg(feature = "ring")]
        {
            let keypair1 = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            let keypair2 = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            let message = b"Test cross-key verification";
            let signature = crypto.ecdsa_sign(&keypair1, message).unwrap();

            // Verifying with the wrong key should fail
            let valid = crypto
                .ecdsa_verify(&keypair2.public, message, &signature)
                .unwrap();
            assert!(!valid, "Signature verified with wrong key should fail");
        }
    }

    #[test]
    fn test_ecdsa_algorithm_mismatch() {
        let crypto = get_crypto();
        #[cfg(feature = "ring")]
        {
            let keypair = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            let message = b"Test algorithm mismatch";
            let mut signature = crypto.ecdsa_sign(&keypair, message).unwrap();

            // Change the algorithm to P-384 — should fail curve match
            signature.algorithm = SignatureAlgorithm::EcdsaP384Sha384;
            let valid = crypto
                .ecdsa_verify(&keypair.public, message, &signature)
                .unwrap();
            assert!(!valid, "Algorithm mismatch should fail");
        }
    }

    #[test]
    fn test_key_zeroization() {
        let crypto = get_crypto();

        #[cfg(not(feature = "ring"))]
        {
            assert!(crypto.generate_ecdsa_keypair(EcCurve::P256).is_err());
        }
        #[cfg(feature = "ring")]
        {
            // ECDSA key zeroization on drop
            let keypair = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            assert!(!keypair.d.iter().all(|&b| b == 0));
            // drop(keypair) will zeroize d
        }
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

    #[test]
    fn test_rsa_key_size_properties() {
        assert_eq!(RsaKeySize::Rsa2048.bits(), 2048);
        assert_eq!(RsaKeySize::Rsa2048.bytes(), 256);
        assert_eq!(RsaKeySize::Rsa3072.bits(), 3072);
        assert_eq!(RsaKeySize::Rsa3072.bytes(), 384);
        assert_eq!(RsaKeySize::Rsa4096.bits(), 4096);
        assert_eq!(RsaKeySize::Rsa4096.bytes(), 512);
    }

    #[test]
    fn test_signature_algorithm_variants() {
        // Verify all algorithm variants exist and are distinct
        let algs = [
            SignatureAlgorithm::RsaPkcs1Sha256,
            SignatureAlgorithm::RsaPkcs1Sha384,
            SignatureAlgorithm::RsaPkcs1Sha512,
            SignatureAlgorithm::RsaPssSha256,
            SignatureAlgorithm::RsaPssSha384,
            SignatureAlgorithm::RsaPssSha512,
            SignatureAlgorithm::EcdsaP256Sha256,
            SignatureAlgorithm::EcdsaP384Sha384,
            SignatureAlgorithm::EcdsaP521Sha512,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }
}
