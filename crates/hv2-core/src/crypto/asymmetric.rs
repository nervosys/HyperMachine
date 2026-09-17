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
        let bits = size.bytes() * 8;
        let mut rng = HostRandom(self);

        let key = ic_rsa::generate(bits, &mut rng)
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA keygen: {e}")))?;

        // Every component comes back through a fixed-size buffer rather than a
        // growable one: `ic-rsa` writes big-endian into exactly the width the
        // modulus implies, which is what the rest of this file stores and what
        // a PKCS#1 encoder expects. A shorter value here would be a different
        // number once it is parsed back.
        let k = size.bytes();
        let half = k / 2;

        let mut n = vec![0u8; k];
        key.public_key()
            .modulus_bytes(&mut n)
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA modulus: {e}")))?;

        let mut d = vec![0u8; k];
        key.exponent_bytes(&mut d)
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA exponent: {e}")))?;

        let (mut p, mut q) = (vec![0u8; half], vec![0u8; half]);
        key.prime_bytes(&mut p, &mut q)
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA primes: {e}")))?;

        let (mut dp, mut dq, mut qinv) = (vec![0u8; half], vec![0u8; half], vec![0u8; half]);
        key.crt_exponent_bytes(&mut dp, &mut dq, &mut qinv)
            .map_err(|e| CryptoError::KeyGenerationFailed(format!("RSA CRT values: {e}")))?;

        Ok(RsaPrivateKey {
            public: RsaPublicKey {
                n,
                e: key.public_key().exponent().to_be_bytes().to_vec(),
                size,
            },
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

    // ========================================================================
    // RSA encryption is deliberately absent
    //
    // `rsa_encrypt`/`rsa_decrypt` used to live here, over a hand-written
    // `mod_exp_bytes`. They were removed rather than fixed, for three reasons
    // in increasing order of importance.
    //
    // They did not work. `mod_exp_bytes` returned 121 for 4^13 mod 497 (445)
    // and 7 for 5^117 mod 19 (1): it squared the accumulator and multiplied by
    // the base, which is the left-to-right square-and-multiply, while scanning
    // the exponent's bits least-significant first, which that form cannot do.
    // A comment reading "advance base by 32 squarings for next limb" sat above
    // an empty `if`. Nothing caught it because the only test asserted that an
    // empty key is refused.
    //
    // Nothing called them -- no caller anywhere in the workspace.
    //
    // And the primitive is the wrong one. Both doc comments said "RSA-OAEP"
    // while the code did PKCS#1 v1.5, whose decryption path is a
    // Bleichenbacher oracle by construction; the padding check here returned
    // early on each distinct failure, which is that oracle. IronCrypto omits
    // RSA encryption for exactly this reason, and key transport belongs to
    // ECDH or ML-KEM. RSA's remaining job here is signatures.
    // ========================================================================

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
        use ic_rsa::{Pkcs1Sha256, Pkcs1Sha384, Pkcs1Sha512, PssSha256, PssSha384, PssSha512};

        // Rebuilt from the primes rather than from (n, e, d), so signing uses
        // the Chinese remainder theorem. Without them every signature is a
        // full-width exponentiation: correct, and about four times slower.
        let key = ic_rsa::RsaPrivateKey::from_primes(
            &private_key.p,
            &private_key.q,
            be_exponent(&private_key.public.e)?,
        )
        .map_err(|e| CryptoError::InvalidInput(format!("invalid RSA private key: {e}")))?;

        let mut signature = vec![0u8; key.size()];
        let mut rng = HostRandom(self);
        match algorithm {
            SignatureAlgorithm::RsaPkcs1Sha256 => Pkcs1Sha256::sign(&key, data, &mut signature),
            SignatureAlgorithm::RsaPkcs1Sha384 => Pkcs1Sha384::sign(&key, data, &mut signature),
            SignatureAlgorithm::RsaPkcs1Sha512 => Pkcs1Sha512::sign(&key, data, &mut signature),
            SignatureAlgorithm::RsaPssSha256 => {
                PssSha256::sign(&key, data, &mut rng, &mut signature)
            }
            SignatureAlgorithm::RsaPssSha384 => {
                PssSha384::sign(&key, data, &mut rng, &mut signature)
            }
            SignatureAlgorithm::RsaPssSha512 => {
                PssSha512::sign(&key, data, &mut rng, &mut signature)
            }
            _ => {
                return Err(CryptoError::UnsupportedAlgorithm(format!(
                    "{algorithm:?} is not an RSA algorithm"
                )));
            }
        }
        .map_err(|e| CryptoError::EncryptionFailed(format!("RSA signing failed: {e}")))?;

        Ok(Signature {
            data: signature,
            algorithm,
        })
    }

    /// Verify an RSA signature (PKCS#1 v1.5 or PSS, SHA-256/384/512) using the
    /// pure-Rust `rsa` crate and the public key components (n, e).
    pub fn rsa_verify(
        &self,
        public_key: &RsaPublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> CryptoResult<bool> {
        use ic_rsa::{Pkcs1Sha256, Pkcs1Sha384, Pkcs1Sha512, PssSha256, PssSha384, PssSha512};

        let key = ic_rsa::RsaPublicKey::from_components(&public_key.n, be_exponent(&public_key.e)?)
            .map_err(|e| CryptoError::InvalidInput(format!("invalid RSA public key: {e}")))?;

        let outcome = match signature.algorithm {
            SignatureAlgorithm::RsaPkcs1Sha256 => Pkcs1Sha256::verify(&key, data, &signature.data),
            SignatureAlgorithm::RsaPkcs1Sha384 => Pkcs1Sha384::verify(&key, data, &signature.data),
            SignatureAlgorithm::RsaPkcs1Sha512 => Pkcs1Sha512::verify(&key, data, &signature.data),
            SignatureAlgorithm::RsaPssSha256 => PssSha256::verify(&key, data, &signature.data),
            SignatureAlgorithm::RsaPssSha384 => PssSha384::verify(&key, data, &signature.data),
            SignatureAlgorithm::RsaPssSha512 => PssSha512::verify(&key, data, &signature.data),
            other => {
                return Err(CryptoError::UnsupportedAlgorithm(format!(
                    "{other:?} is not an RSA algorithm"
                )));
            }
        };

        // A bad signature is `false`, not an error: the caller asked whether
        // this signature is valid, and "no" is an answer rather than a
        // failure. Errors are reserved for a key or algorithm that could never
        // verify anything.
        Ok(outcome.is_ok())
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

/// The host's CSPRNG, as IronCrypto's random source.
///
/// `ic-rsa` takes any `RandomSource` rather than reaching for the OS itself --
/// it is `no_std` and has no opinion about where entropy comes from. This
/// hands it whatever this module already uses, so key generation and PSS salts
/// draw from the same place as everything else here rather than from a second,
/// separately-configured source.
struct HostRandom<'a>(&'a FipsCrypto);

impl ic_core::traits::RandomSource for HostRandom<'_> {
    fn fill(&mut self, out: &mut [u8]) -> ic_core::Result<()> {
        self.0
            .random_bytes(out)
            .map_err(|_| ic_core::err!(Internal, "the host RNG failed"))
    }
}

/// A big-endian public exponent as the `u64` IronCrypto takes.
///
/// Stored here as bytes because that is how it arrives in a certificate. It is
/// `e`, so it is public and small -- 65537 in almost every key ever issued --
/// and a value that does not fit in 64 bits is a malformed key rather than an
/// exotic one.
fn be_exponent(bytes: &[u8]) -> CryptoResult<u64> {
    let trimmed = bytes
        .iter()
        .position(|b| *b != 0)
        .map_or(&[][..], |at| &bytes[at..]);
    if trimmed.is_empty() || trimmed.len() > 8 {
        return Err(CryptoError::InvalidInput(format!(
            "an RSA public exponent of {} bytes is not a usable one",
            trimmed.len()
        )));
    }
    Ok(trimmed
        .iter()
        .fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
}

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
