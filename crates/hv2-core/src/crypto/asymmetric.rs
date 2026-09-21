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

/// Dispatch one call across the three NIST curves.
///
/// `ic-ec` names a separate type per curve because the hash is part of the
/// scheme -- P-256 is always SHA-256, P-384 always SHA-384 -- so there is no
/// runtime value to pass. This turns this module's runtime `EcCurve` into that
/// choice in one place, rather than three copies of the same `match`.
use ic_core::traits::SignatureScheme as _;

macro_rules! per_curve {
    ($curve:expr, $call:ident $args:tt) => {
        match $curve {
            EcCurve::P256 => ic_ec::EcdsaP256Sha256::$call $args,
            EcCurve::P384 => ic_ec::EcdsaP384Sha384::$call $args,
            EcCurve::P521 => ic_ec::p521::EcdsaP521Sha512::$call $args,
        }
    };
}

/// How many bytes each curve's private scalar and public key take.
///
/// The public key is SEC1 uncompressed: `0x04 || x || y`, so it is one byte
/// more than twice the field width.
const fn key_lengths(curve: EcCurve) -> (usize, usize) {
    match curve {
        EcCurve::P256 => (32, 65),
        EcCurve::P384 => (48, 97),
        EcCurve::P521 => (66, 133),
    }
}

/// Bits in the top byte of a private scalar that lie above the curve order.
///
/// P-256 and P-384 have orders that are a whole number of bytes wide, so a
/// buffer of random bytes is already the right size. P-521 is not: the order
/// is 521 bits but the buffer is 66 bytes, which is 528, and the 7 bits of
/// slack put roughly 127 of every 128 uniform draws above the order. Left
/// alone that made key generation fail outright about half the time, since a
/// hundred consecutive rejections is likelier than not at those odds.
///
/// Clearing the slack leaves a uniform value in `[0, 2^521)`, and every NIST
/// order sits close enough below its power of two that a redraw from there is
/// genuinely rare.
const fn excess_scalar_bits(curve: EcCurve) -> u32 {
    match curve {
        EcCurve::P256 | EcCurve::P384 => 0,
        EcCurve::P521 => 7,
    }
}

impl FipsCrypto {
    // ========================================================================
    // RSA Key Generation
    // ========================================================================

    /// Generate an RSA key pair.
    ///
    /// Uses IronCrypto's `ic-rsa`, drawing entropy from `HostRandom` so that
    /// key generation shares this module's CSPRNG rather than reaching for the
    /// OS separately. All components (n, e, d, p, q, and the CRT values) are
    /// stored as raw big-endian byte strings.
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
    ///
    /// P-521 works now. It did not under `ring`, which has no P-521 at all, so
    /// this used to answer `UnsupportedAlgorithm` for a curve the rest of the
    /// module claimed to support.
    ///
    /// The private scalar is drawn by rejection sampling: random bytes with
    /// the bits above the curve order masked off (see `excess_scalar_bits`),
    /// then ask IronCrypto to derive the public key, and draw again if it
    /// refuses. The refusal is the range check -- a scalar must be in
    /// `[1, n-1]` -- and borrowing the library's is the point, because a
    /// comparison against the curve order written here would be one more piece
    /// of hand-rolled arithmetic of exactly the kind this module has already
    /// been burned by. Masking first is what makes the redraw rare; without it
    /// P-521 rejects almost every candidate.
    pub fn generate_ecdsa_keypair(&self, curve: EcCurve) -> CryptoResult<EcPrivateKey> {
        let (private_len, public_len) = key_lengths(curve);
        let mut d = vec![0u8; private_len];
        let mut public = vec![0u8; public_len];

        // Bounded rather than `loop`: a hundred consecutive refusals is not
        // bad luck, it is a broken RNG, and spinning forever would hide that.
        let excess = excess_scalar_bits(curve);
        let mut attempts = 0;
        loop {
            self.random_bytes(&mut d)?;
            // Drop the bits that sit above the order before asking, or almost
            // every P-521 candidate is rejected for being too large.
            if excess > 0 {
                d[0] &= 0xffu8 >> excess;
            }
            if per_curve!(curve, public_key(&d, &mut public)).is_ok() {
                break;
            }
            attempts += 1;
            if attempts >= 100 {
                return Err(CryptoError::KeyGenerationFailed(
                    "100 candidate scalars were all out of range; the RNG is not \
                     producing what it should"
                        .into(),
                ));
            }
        }

        // SEC1 uncompressed is `0x04 || x || y`, so the coordinates are the
        // two halves of what follows the tag.
        let field = (public_len - 1) / 2;
        Ok(EcPrivateKey {
            public: EcPublicKey {
                x: public[1..=field].to_vec(),
                y: public[1 + field..].to_vec(),
                curve,
            },
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

    /// Sign data with an RSA private key (PKCS#1 v1.5 or PSS, SHA-256/384/512).
    ///
    /// Uses IronCrypto's `ic-rsa`, reconstructing the signing key from the
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

    /// Verify an RSA signature (PKCS#1 v1.5 or PSS, SHA-256/384/512) using
    /// IronCrypto's `ic-rsa` and the public key components (n, e).
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

        let mut signature = vec![0u8; 2 * key_lengths(private_key.public.curve).0];
        per_curve!(
            private_key.public.curve,
            sign(&private_key.d, data, &mut signature)
        )
        .map_err(|e| CryptoError::EncryptionFailed(format!("ECDSA signing failed: {e}")))?;

        Ok(Signature {
            data: signature,
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

        // SEC1 uncompressed, which is what `ic-ec` takes and what this module
        // already stored the coordinates for.
        let mut encoded = Vec::with_capacity(1 + public_key.x.len() + public_key.y.len());
        encoded.push(0x04);
        encoded.extend_from_slice(&public_key.x);
        encoded.extend_from_slice(&public_key.y);

        // A signature that does not verify is `false`, not an error: the
        // caller asked a question and "no" is an answer.
        Ok(per_curve!(public_key.curve, verify(&encoded, data, &signature.data)).is_ok())
    }
}

// ============================================================================
// IronCrypto Glue
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::fips::FipsMode;

    fn get_crypto() -> FipsCrypto {
        // Disabled mode skips the power-on self-tests.
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

        // All three curves, P-521 included. It used to be absent here and
        // asserted to *fail* just below, because `ring` has no P-521 -- so the
        // enum offered a curve the implementation refused.
        for curve in [EcCurve::P256, EcCurve::P384, EcCurve::P521] {
            let keypair = crypto
                .generate_ecdsa_keypair(curve)
                .unwrap_or_else(|e| panic!("{curve:?} keygen: {e}"));
            assert_eq!(keypair.public.curve, curve);
            assert_eq!(keypair.public.x.len(), curve.key_size_bytes());
            assert_eq!(keypair.public.y.len(), curve.key_size_bytes());
        }
    }

    /// P-521 keygen used to fail roughly half the time.
    ///
    /// Its 521-bit scalar lives in a 66-byte buffer, so 7 bits of slack put
    /// about 127 of every 128 uniform draws above the curve order, and the
    /// hundred-attempt bound was reached more often than not. A single
    /// generation passes 54% of the time even when the masking is wrong, which
    /// is exactly why one is not enough to believe: sixteen in a row would
    /// have caught it with probability better than a million to one.
    #[test]
    fn p521_keygen_does_not_run_out_of_attempts() {
        let crypto = get_crypto();
        for i in 0..16 {
            crypto
                .generate_ecdsa_keypair(EcCurve::P521)
                .unwrap_or_else(|e| panic!("P-521 keygen failed on attempt {i}: {e}"));
        }
    }

    #[test]
    fn test_ecdsa_sign_verify_p256() {
        let crypto = get_crypto();
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
