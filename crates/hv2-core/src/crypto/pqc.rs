//! Post-Quantum Cryptography Module
//!
//! Provides quantum-resistant cryptographic algorithms standardized by NIST,
//! backed by the pure-Rust [RustCrypto] implementations `ml-kem`, `ml-dsa`, and
//! `slh-dsa`. These are real lattice-based (ML-KEM/ML-DSA) and hash-based
//! (SLH-DSA) schemes — not placeholders.
//!
//! The implementations are compiled when the `pqc` feature is enabled (on by
//! default). With `--no-default-features` the operations return
//! [`CryptoError::NotImplemented`].
//!
//! ## Algorithms
//!
//! - **ML-KEM** (FIPS 203): Module-Lattice Key-Encapsulation Mechanism
//!   - ML-KEM-512, ML-KEM-768, ML-KEM-1024
//! - **ML-DSA** (FIPS 204): Module-Lattice Digital Signature Algorithm
//!   - ML-DSA-44, ML-DSA-65, ML-DSA-87
//! - **SLH-DSA** (FIPS 205): Stateless Hash-Based Digital Signature Algorithm
//!   - SLH-DSA-SHA2/SHAKE, 128/192/256, fast & small variants
//!
//! ## Key serialization
//!
//! The `data` field of each key/ciphertext/signature stores the canonical
//! byte encoding from the underlying crate. ML-KEM decapsulation keys are
//! stored as their 64-byte FIPS 203 seed (the preferred serialization), from
//! which the full key is deterministically reconstructed.
//!
//! ## Hybrid Mode
//!
//! For transitional security, use hybrid schemes combining classical (ECDH/ECDSA)
//! with post-quantum algorithms.
//!
//! [RustCrypto]: https://github.com/RustCrypto

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
// FipsCrypto Implementation (real, RustCrypto-backed)
// ============================================================================

/// A `rand_core` 0.10 CSPRNG adapter sourcing entropy from the OS via `rand`'s
/// `SysRng` (BCryptGenRandom on Windows, getrandom(2)/`/dev/urandom` on Linux).
///
/// `slh-dsa` requires a `CryptoRng` from `rand_core` 0.10; the `Rng`/`CryptoRng`
/// traits are blanket-derived from the infallible `TryRng`/`TryCryptoRng` impls.
/// `SysRng` is itself fallible, so a failure of the OS random source panics
/// here, matching the behaviour of the infallible `fill_bytes` it replaces.
#[cfg(feature = "pqc")]
struct PqcOsRng;

#[cfg(feature = "pqc")]
impl PqcOsRng {
    fn fill(dst: &mut [u8]) {
        use rand::TryRng as _;
        rand::rngs::SysRng
            .try_fill_bytes(dst)
            .expect("OS random source failure");
    }
}

#[cfg(feature = "pqc")]
impl slh_dsa::signature::rand_core::TryRng for PqcOsRng {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        Self::fill(&mut b);
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        Self::fill(&mut b);
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        Self::fill(dst);
        Ok(())
    }
}

#[cfg(feature = "pqc")]
impl slh_dsa::signature::rand_core::TryCryptoRng for PqcOsRng {}

/// Dispatch an ML-KEM operation over the concrete parameter type, binding it to
/// the type alias `$alias` inside `$body`.
#[cfg(feature = "pqc")]
macro_rules! mlkem_with {
    ($params:expr, $alias:ident, $body:block) => {
        match $params {
            MlKemParameterSet::MlKem512 => {
                type $alias = ml_kem::MlKem512;
                $body
            }
            MlKemParameterSet::MlKem768 => {
                type $alias = ml_kem::MlKem768;
                $body
            }
            MlKemParameterSet::MlKem1024 => {
                type $alias = ml_kem::MlKem1024;
                $body
            }
        }
    };
}

/// Dispatch an ML-DSA operation over the concrete parameter type.
#[cfg(feature = "pqc")]
macro_rules! mldsa_with {
    ($params:expr, $alias:ident, $body:block) => {
        match $params {
            MlDsaParameterSet::MlDsa44 => {
                type $alias = ml_dsa::MlDsa44;
                $body
            }
            MlDsaParameterSet::MlDsa65 => {
                type $alias = ml_dsa::MlDsa65;
                $body
            }
            MlDsaParameterSet::MlDsa87 => {
                type $alias = ml_dsa::MlDsa87;
                $body
            }
        }
    };
}

/// Dispatch an SLH-DSA operation over the concrete parameter type.
#[cfg(feature = "pqc")]
macro_rules! slhdsa_with {
    ($params:expr, $alias:ident, $body:block) => {
        match $params {
            SlhDsaParameterSet::Sha2_128f => {
                type $alias = slh_dsa::Sha2_128f;
                $body
            }
            SlhDsaParameterSet::Sha2_128s => {
                type $alias = slh_dsa::Sha2_128s;
                $body
            }
            SlhDsaParameterSet::Sha2_192f => {
                type $alias = slh_dsa::Sha2_192f;
                $body
            }
            SlhDsaParameterSet::Sha2_256f => {
                type $alias = slh_dsa::Sha2_256f;
                $body
            }
            SlhDsaParameterSet::Shake128f => {
                type $alias = slh_dsa::Shake128f;
                $body
            }
            SlhDsaParameterSet::Shake256f => {
                type $alias = slh_dsa::Shake256f;
                $body
            }
        }
    };
}

#[cfg(feature = "pqc")]
impl FipsCrypto {
    // ========================================================================
    // ML-KEM Operations (FIPS 203, via `ml-kem`)
    // ========================================================================

    /// Generate an ML-KEM key pair. The secret key stores the 64-byte FIPS 203
    /// seed; the public (encapsulation) key stores its canonical encoding.
    pub fn ml_kem_keygen(&self, params: MlKemParameterSet) -> CryptoResult<MlKemSecretKey> {
        use ml_kem::kem::{Kem, KeyExport};

        let (secret_data, public_data) = mlkem_with!(params, P, {
            let (dk, ek) = P::generate_keypair();
            let seed = dk.to_seed().ok_or_else(|| {
                CryptoError::KeyDerivationFailed("ML-KEM seed unavailable".into())
            })?;
            (seed.to_vec(), ek.to_bytes().to_vec())
        });

        Ok(MlKemSecretKey {
            public: MlKemPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: secret_data,
        })
    }

    /// ML-KEM encapsulation - generate ciphertext and shared secret.
    pub fn ml_kem_encaps(
        &self,
        public_key: &MlKemPublicKey,
    ) -> CryptoResult<(MlKemCiphertext, Vec<u8>)> {
        use ml_kem::kem::{Encapsulate, Key};
        use ml_kem::EncapsulationKey;

        let params = public_key.parameter_set;
        let (ciphertext_data, shared_secret) = mlkem_with!(params, P, {
            let arr = Key::<EncapsulationKey<P>>::try_from(&public_key.data[..])
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM public key".into()))?;
            let ek = EncapsulationKey::<P>::new(&arr)
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM public key".into()))?;
            let (ct, ss) = ek.encapsulate();
            (ct.to_vec(), ss.to_vec())
        });

        Ok((
            MlKemCiphertext {
                data: ciphertext_data,
                parameter_set: params,
            },
            shared_secret,
        ))
    }

    /// ML-KEM decapsulation - derive shared secret from ciphertext.
    pub fn ml_kem_decaps(
        &self,
        secret_key: &MlKemSecretKey,
        ciphertext: &MlKemCiphertext,
    ) -> CryptoResult<Vec<u8>> {
        use ml_kem::kem::{Ciphertext, Decapsulate};
        use ml_kem::DecapsulationKey;

        if ciphertext.parameter_set != secret_key.public.parameter_set {
            return Err(CryptoError::InvalidInput("Parameter set mismatch".into()));
        }

        let params = secret_key.public.parameter_set;
        let shared_secret = mlkem_with!(params, P, {
            let seed = ml_kem::Seed::try_from(&secret_key.data[..])
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM secret key".into()))?;
            let dk = DecapsulationKey::<P>::from_seed(seed);
            let ct = Ciphertext::<P>::try_from(&ciphertext.data[..])
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM ciphertext".into()))?;
            dk.decapsulate(&ct).to_vec()
        });

        Ok(shared_secret)
    }

    // ========================================================================
    // ML-DSA Operations (FIPS 204, via `ml-dsa`)
    // ========================================================================

    /// Generate an ML-DSA key pair.
    pub fn ml_dsa_keygen(&self, params: MlDsaParameterSet) -> CryptoResult<MlDsaSecretKey> {
        use ml_dsa::signature::Keypair;
        use ml_dsa::{Generate, KeyExport, SigningKey};

        let (secret_data, public_data) = mldsa_with!(params, P, {
            let sk = SigningKey::<P>::generate();
            let vk = sk.verifying_key();
            (sk.to_bytes().to_vec(), vk.to_bytes().to_vec())
        });

        Ok(MlDsaSecretKey {
            public: MlDsaPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: secret_data,
        })
    }

    /// ML-DSA sign.
    pub fn ml_dsa_sign(
        &self,
        secret_key: &MlDsaSecretKey,
        message: &[u8],
    ) -> CryptoResult<MlDsaSignature> {
        use ml_dsa::signature::Signer;
        use ml_dsa::{KeyInit, SigningKey};

        let params = secret_key.public.parameter_set;
        let sig_data = mldsa_with!(params, P, {
            let arr = ml_dsa::common::Key::<SigningKey<P>>::try_from(&secret_key.data[..])
                .map_err(|_| CryptoError::InvalidInput("invalid ML-DSA secret key".into()))?;
            let sk = SigningKey::<P>::new(&arr);
            let sig = sk
                .try_sign(message)
                .map_err(|e| CryptoError::EncryptionFailed(format!("ML-DSA sign: {e}")))?;
            sig.encode().to_vec()
        });

        Ok(MlDsaSignature {
            data: sig_data,
            parameter_set: params,
        })
    }

    /// ML-DSA verify.
    pub fn ml_dsa_verify(
        &self,
        public_key: &MlDsaPublicKey,
        message: &[u8],
        signature: &MlDsaSignature,
    ) -> CryptoResult<bool> {
        use ml_dsa::signature::Verifier;
        use ml_dsa::{KeyInit, Signature, VerifyingKey};

        if signature.parameter_set != public_key.parameter_set {
            return Ok(false);
        }

        let valid = mldsa_with!(public_key.parameter_set, P, {
            let arr = match ml_dsa::common::Key::<VerifyingKey<P>>::try_from(&public_key.data[..]) {
                Ok(a) => a,
                Err(_) => return Ok(false),
            };
            let vk = VerifyingKey::<P>::new(&arr);
            let sig = match Signature::<P>::try_from(&signature.data[..]) {
                Ok(s) => s,
                Err(_) => return Ok(false),
            };
            vk.verify(message, &sig).is_ok()
        });

        Ok(valid)
    }

    // ========================================================================
    // SLH-DSA Operations (FIPS 205, via `slh-dsa`)
    // ========================================================================

    /// Generate an SLH-DSA key pair.
    pub fn slh_dsa_keygen(&self, params: SlhDsaParameterSet) -> CryptoResult<SlhDsaSecretKey> {
        let (secret_data, public_data) = slhdsa_with!(params, P, {
            let sk = slh_dsa::SigningKey::<P>::new(&mut PqcOsRng);
            let vk: &slh_dsa::VerifyingKey<P> = sk.as_ref();
            (sk.to_bytes().to_vec(), vk.to_bytes().to_vec())
        });

        Ok(SlhDsaSecretKey {
            public: SlhDsaPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: secret_data,
        })
    }

    /// SLH-DSA sign (deterministic).
    pub fn slh_dsa_sign(
        &self,
        secret_key: &SlhDsaSecretKey,
        message: &[u8],
    ) -> CryptoResult<SlhDsaSignature> {
        use slh_dsa::signature::Signer;

        let params = secret_key.public.parameter_set;
        let sig_data = slhdsa_with!(params, P, {
            let sk = slh_dsa::SigningKey::<P>::try_from(&secret_key.data[..])
                .map_err(|_| CryptoError::InvalidInput("invalid SLH-DSA secret key".into()))?;
            let sig = sk
                .try_sign(message)
                .map_err(|e| CryptoError::EncryptionFailed(format!("SLH-DSA sign: {e}")))?;
            sig.to_vec()
        });

        Ok(SlhDsaSignature {
            data: sig_data,
            parameter_set: params,
        })
    }

    /// SLH-DSA verify.
    pub fn slh_dsa_verify(
        &self,
        public_key: &SlhDsaPublicKey,
        message: &[u8],
        signature: &SlhDsaSignature,
    ) -> CryptoResult<bool> {
        use slh_dsa::signature::Verifier;

        if signature.parameter_set != public_key.parameter_set {
            return Ok(false);
        }

        let valid = slhdsa_with!(public_key.parameter_set, P, {
            let vk = match slh_dsa::VerifyingKey::<P>::try_from(&public_key.data[..]) {
                Ok(v) => v,
                Err(_) => return Ok(false),
            };
            let sig = match slh_dsa::Signature::<P>::try_from(&signature.data[..]) {
                Ok(s) => s,
                Err(_) => return Ok(false),
            };
            vk.verify(message, &sig).is_ok()
        });

        Ok(valid)
    }
}

// ============================================================================
// Fallback when the `pqc` feature is disabled
// ============================================================================

#[cfg(not(feature = "pqc"))]
impl FipsCrypto {
    fn pqc_disabled<T>() -> CryptoResult<T> {
        Err(CryptoError::NotImplemented(
            "post-quantum cryptography requires the `pqc` feature".into(),
        ))
    }

    /// ML-KEM key generation (requires the `pqc` feature).
    pub fn ml_kem_keygen(&self, _params: MlKemParameterSet) -> CryptoResult<MlKemSecretKey> {
        Self::pqc_disabled()
    }
    /// ML-KEM encapsulation (requires the `pqc` feature).
    pub fn ml_kem_encaps(
        &self,
        _public_key: &MlKemPublicKey,
    ) -> CryptoResult<(MlKemCiphertext, Vec<u8>)> {
        Self::pqc_disabled()
    }
    /// ML-KEM decapsulation (requires the `pqc` feature).
    pub fn ml_kem_decaps(
        &self,
        _secret_key: &MlKemSecretKey,
        _ciphertext: &MlKemCiphertext,
    ) -> CryptoResult<Vec<u8>> {
        Self::pqc_disabled()
    }
    /// ML-DSA key generation (requires the `pqc` feature).
    pub fn ml_dsa_keygen(&self, _params: MlDsaParameterSet) -> CryptoResult<MlDsaSecretKey> {
        Self::pqc_disabled()
    }
    /// ML-DSA sign (requires the `pqc` feature).
    pub fn ml_dsa_sign(
        &self,
        _secret_key: &MlDsaSecretKey,
        _message: &[u8],
    ) -> CryptoResult<MlDsaSignature> {
        Self::pqc_disabled()
    }
    /// ML-DSA verify (requires the `pqc` feature).
    pub fn ml_dsa_verify(
        &self,
        _public_key: &MlDsaPublicKey,
        _message: &[u8],
        _signature: &MlDsaSignature,
    ) -> CryptoResult<bool> {
        Self::pqc_disabled()
    }
    /// SLH-DSA key generation (requires the `pqc` feature).
    pub fn slh_dsa_keygen(&self, _params: SlhDsaParameterSet) -> CryptoResult<SlhDsaSecretKey> {
        Self::pqc_disabled()
    }
    /// SLH-DSA sign (requires the `pqc` feature).
    pub fn slh_dsa_sign(
        &self,
        _secret_key: &SlhDsaSecretKey,
        _message: &[u8],
    ) -> CryptoResult<SlhDsaSignature> {
        Self::pqc_disabled()
    }
    /// SLH-DSA verify (requires the `pqc` feature).
    pub fn slh_dsa_verify(
        &self,
        _public_key: &SlhDsaPublicKey,
        _message: &[u8],
        _signature: &SlhDsaSignature,
    ) -> CryptoResult<bool> {
        Self::pqc_disabled()
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
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_ml_kem_keygen() {
        let crypto = get_crypto();

        for params in [
            MlKemParameterSet::MlKem512,
            MlKemParameterSet::MlKem768,
            MlKemParameterSet::MlKem1024,
        ] {
            let sk = crypto.ml_kem_keygen(params).unwrap();
            // Public (encapsulation) key uses the canonical FIPS 203 encoding.
            assert_eq!(sk.public.data.len(), params.public_key_bytes());
            // Secret key is stored as the 64-byte seed.
            assert_eq!(sk.data.len(), 64);
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_ml_kem_encaps_decaps() {
        let crypto = get_crypto();
        for params in [
            MlKemParameterSet::MlKem512,
            MlKemParameterSet::MlKem768,
            MlKemParameterSet::MlKem1024,
        ] {
            let sk = crypto.ml_kem_keygen(params).unwrap();
            let (ct, ss_send) = crypto.ml_kem_encaps(&sk.public).unwrap();
            assert_eq!(ct.data.len(), params.ciphertext_bytes());
            assert_eq!(ss_send.len(), 32);

            // Decapsulation must recover the same shared secret.
            let ss_recv = crypto.ml_kem_decaps(&sk, &ct).unwrap();
            assert_eq!(ss_send, ss_recv);
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_ml_dsa_keygen() {
        let crypto = get_crypto();

        for params in [
            MlDsaParameterSet::MlDsa44,
            MlDsaParameterSet::MlDsa65,
            MlDsaParameterSet::MlDsa87,
        ] {
            let sk = crypto.ml_dsa_keygen(params).unwrap();
            assert_eq!(sk.public.data.len(), params.public_key_bytes());
            assert!(!sk.data.is_empty());
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_ml_dsa_sign_verify() {
        let crypto = get_crypto();
        let sk = crypto.ml_dsa_keygen(MlDsaParameterSet::MlDsa65).unwrap();
        let message = b"Post-quantum signature test";

        let sig = crypto.ml_dsa_sign(&sk, message).unwrap();
        assert_eq!(sig.data.len(), MlDsaParameterSet::MlDsa65.signature_bytes());
        assert!(crypto.ml_dsa_verify(&sk.public, message, &sig).unwrap());

        // A tampered message must fail verification.
        assert!(!crypto
            .ml_dsa_verify(&sk.public, b"different message", &sig)
            .unwrap());
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_slh_dsa_keygen() {
        let crypto = get_crypto();

        for params in [SlhDsaParameterSet::Sha2_128f, SlhDsaParameterSet::Shake128f] {
            let sk = crypto.slh_dsa_keygen(params).unwrap();
            assert_eq!(sk.public.data.len(), params.public_key_bytes());
            assert_eq!(sk.data.len(), params.secret_key_bytes());
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_slh_dsa_sign_verify() {
        let crypto = get_crypto();
        // Use the fast 128-bit variant to keep the test quick.
        let sk = crypto
            .slh_dsa_keygen(SlhDsaParameterSet::Sha2_128f)
            .unwrap();
        let message = b"Hash-based signature test";

        let sig = crypto.slh_dsa_sign(&sk, message).unwrap();
        assert_eq!(
            sig.data.len(),
            SlhDsaParameterSet::Sha2_128f.signature_bytes()
        );
        assert!(crypto.slh_dsa_verify(&sk.public, message, &sig).unwrap());

        // A tampered message must fail verification.
        assert!(!crypto
            .slh_dsa_verify(&sk.public, b"tampered", &sig)
            .unwrap());
    }

    #[cfg(not(feature = "pqc"))]
    #[test]
    fn test_pqc_disabled_returns_error() {
        let crypto = get_crypto();
        assert!(crypto.ml_kem_keygen(MlKemParameterSet::MlKem768).is_err());
        assert!(crypto.ml_dsa_keygen(MlDsaParameterSet::MlDsa65).is_err());
        assert!(crypto
            .slh_dsa_keygen(SlhDsaParameterSet::Sha2_128f)
            .is_err());
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
