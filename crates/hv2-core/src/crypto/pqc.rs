//! Post-Quantum Cryptography Module
//!
//! Provides quantum-resistant cryptographic algorithms standardized by NIST.
//! ML-KEM and ML-DSA are IronCrypto's `ic-mlkem` and `ic-mldsa`, each
//! parameter set checked against NIST's ACVP vectors and self-tested by
//! `ic-fips`, so they sit inside the module boundary `FipsMode::Strict` holds
//! to. SLH-DSA is still the pure-Rust [RustCrypto] `slh-dsa`, which Strict
//! refuses: IronCrypto has no SLH-DSA yet.
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
//! FIPS byte encoding. ML-KEM decapsulation keys are stored as their 64-byte
//! FIPS 203 seed and ML-DSA signing keys as their 32-byte FIPS 204 seed, from
//! which the full keys are deterministically reconstructed. Those are the
//! encodings RustCrypto's crates used before, so stored keys carry over.
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
#[cfg(feature = "pqc")]
use zeroize::Zeroizing;

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

    /// The name ic-fips self-tests and indicates this parameter set under.
    pub fn fips_id(&self) -> &'static str {
        match self {
            MlKemParameterSet::MlKem512 => "ml-kem-512",
            MlKemParameterSet::MlKem768 => "ml-kem-768",
            MlKemParameterSet::MlKem1024 => "ml-kem-1024",
        }
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

    /// The name ic-fips self-tests and indicates this parameter set under.
    pub fn fips_id(&self) -> &'static str {
        match self {
            MlDsaParameterSet::MlDsa44 => "ml-dsa-44",
            MlDsaParameterSet::MlDsa65 => "ml-dsa-65",
            MlDsaParameterSet::MlDsa87 => "ml-dsa-87",
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
// FipsCrypto Implementation (IronCrypto for ML-KEM/ML-DSA, RustCrypto for SLH-DSA)
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

/// Dispatch an ML-KEM operation over the parameter set, binding the scheme
/// type to `$kem` and its module (for the size constants) to `$m`.
#[cfg(feature = "pqc")]
macro_rules! mlkem_with {
    ($params:expr, $kem:ident, $m:ident, $body:block) => {
        match $params {
            MlKemParameterSet::MlKem512 => {
                use ic_mlkem::kem512 as $m;
                type $kem = ic_mlkem::MlKem512;
                $body
            }
            MlKemParameterSet::MlKem768 => {
                use ic_mlkem::kem as $m;
                type $kem = ic_mlkem::MlKem768;
                $body
            }
            MlKemParameterSet::MlKem1024 => {
                use ic_mlkem::kem1024 as $m;
                type $kem = ic_mlkem::MlKem1024;
                $body
            }
        }
    };
}

/// Dispatch an ML-DSA operation over the parameter set, binding its module --
/// functions and size constants alike -- to `$m`.
#[cfg(feature = "pqc")]
macro_rules! mldsa_with {
    ($params:expr, $m:ident, $body:block) => {
        match $params {
            MlDsaParameterSet::MlDsa44 => {
                use ic_mldsa::sign44 as $m;
                $body
            }
            MlDsaParameterSet::MlDsa65 => {
                use ic_mldsa::sign as $m;
                $body
            }
            MlDsaParameterSet::MlDsa87 => {
                use ic_mldsa::sign87 as $m;
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

/// `d` and `z` from a 64-byte FIPS 203 seed.
#[cfg(feature = "pqc")]
fn split_seed(seed: &[u8; 64]) -> (&[u8; 32], &[u8; 32]) {
    let (d, z) = seed.split_at(32);
    (
        d.try_into().expect("32 of 64"),
        z.try_into().expect("32 of 64"),
    )
}

#[cfg(feature = "pqc")]
impl FipsCrypto {
    // ========================================================================
    // ML-KEM Operations (FIPS 203, via IronCrypto's `ic-mlkem`)
    // ========================================================================

    /// Generate an ML-KEM key pair. The secret key stores the 64-byte FIPS 203
    /// seed `d || z`; the public (encapsulation) key stores its encoding.
    ///
    /// The seed is the same one RustCrypto's `ml-kem` stored before this
    /// module moved to IronCrypto, so keys generated then still decapsulate.
    pub fn ml_kem_keygen(&self, params: MlKemParameterSet) -> CryptoResult<MlKemSecretKey> {
        self.require_approved(params.fips_id())?;
        let mut seed = Zeroizing::new([0u8; 64]);
        self.random_bytes(&mut seed[..])?;

        let public_data = mlkem_with!(params, Kem, m, {
            let mut ek = [0u8; m::ENCAPS_KEY_LEN];
            let mut dk = Zeroizing::new([0u8; m::DECAPS_KEY_LEN]);
            let (d, z) = split_seed(&seed);
            Kem::keygen_deterministic(d, z, &mut ek, &mut dk);

            // FIPS 140-3's pairwise consistency test: the two halves must
            // agree on a secret before the pair is handed to anyone.
            let mut msg = Zeroizing::new([0u8; 32]);
            self.random_bytes(&mut msg[..])?;
            let mut ct = [0u8; m::CIPHERTEXT_LEN];
            let mut sent = Zeroizing::new([0u8; m::SHARED_SECRET_LEN]);
            let mut received = Zeroizing::new([0u8; m::SHARED_SECRET_LEN]);
            Kem::encapsulate_deterministic(&msg, &ek, &mut ct, &mut sent);
            Kem::decapsulate(&dk, &ct, &mut received)
                .map_err(|e| CryptoError::KeyDerivationFailed(format!("ML-KEM: {e}")))?;
            if *sent != *received {
                return Err(CryptoError::KeyDerivationFailed(
                    "ML-KEM key pair failed its pairwise consistency test".into(),
                ));
            }
            ek.to_vec()
        });

        Ok(MlKemSecretKey {
            public: MlKemPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: seed.to_vec(),
        })
    }

    /// ML-KEM encapsulation - generate ciphertext and shared secret.
    pub fn ml_kem_encaps(
        &self,
        public_key: &MlKemPublicKey,
    ) -> CryptoResult<(MlKemCiphertext, Vec<u8>)> {
        let params = public_key.parameter_set;
        self.require_approved(params.fips_id())?;
        let mut msg = Zeroizing::new([0u8; 32]);
        self.random_bytes(&mut msg[..])?;

        let (ciphertext_data, shared_secret) = mlkem_with!(params, Kem, m, {
            let ek: &[u8; m::ENCAPS_KEY_LEN] = public_key.data[..]
                .try_into()
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM public key".into()))?;
            // FIPS 203's encapsulation-key check: every coefficient reduced.
            Kem::validate_encapsulation_key(ek)
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM public key".into()))?;
            let mut ct = [0u8; m::CIPHERTEXT_LEN];
            let mut ss = [0u8; m::SHARED_SECRET_LEN];
            Kem::encapsulate_deterministic(&msg, ek, &mut ct, &mut ss);
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
        let params = secret_key.public.parameter_set;
        self.require_approved(params.fips_id())?;

        if ciphertext.parameter_set != params {
            return Err(CryptoError::InvalidInput("Parameter set mismatch".into()));
        }
        let seed: &[u8; 64] = secret_key.data[..]
            .try_into()
            .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM secret key".into()))?;

        let shared_secret = mlkem_with!(params, Kem, m, {
            let mut ek = [0u8; m::ENCAPS_KEY_LEN];
            let mut dk = Zeroizing::new([0u8; m::DECAPS_KEY_LEN]);
            let (d, z) = split_seed(seed);
            Kem::keygen_deterministic(d, z, &mut ek, &mut dk);
            let ct: &[u8; m::CIPHERTEXT_LEN] = ciphertext.data[..]
                .try_into()
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM ciphertext".into()))?;
            let mut ss = [0u8; m::SHARED_SECRET_LEN];
            Kem::decapsulate(&dk, ct, &mut ss)
                .map_err(|_| CryptoError::InvalidInput("invalid ML-KEM ciphertext".into()))?;
            ss.to_vec()
        });

        Ok(shared_secret)
    }

    // ========================================================================
    // ML-DSA Operations (FIPS 204, via IronCrypto's `ic-mldsa`)
    // ========================================================================

    /// Generate an ML-DSA key pair. The secret key stores the 32-byte FIPS 204
    /// seed `xi`, as RustCrypto's `ml-dsa` did, so earlier keys still sign.
    ///
    /// `keygen` runs FIPS 140-3's pairwise consistency test itself and
    /// reports it: a failure is an error here, never a key.
    pub fn ml_dsa_keygen(&self, params: MlDsaParameterSet) -> CryptoResult<MlDsaSecretKey> {
        self.require_approved(params.fips_id())?;
        let mut xi = Zeroizing::new([0u8; 32]);
        self.random_bytes(&mut xi[..])?;

        let public_data = mldsa_with!(params, m, {
            let mut pk = [0u8; m::PUBLIC_KEY_LEN];
            let mut sk = Zeroizing::new([0u8; m::SECRET_KEY_LEN]);
            if !m::keygen(&xi, &mut pk, &mut sk) {
                return Err(CryptoError::KeyDerivationFailed(
                    "ML-DSA key pair failed its pairwise consistency test".into(),
                ));
            }
            pk.to_vec()
        });

        Ok(MlDsaSecretKey {
            public: MlDsaPublicKey {
                data: public_data,
                parameter_set: params,
            },
            data: xi.to_vec(),
        })
    }

    /// ML-DSA sign: pure ML-DSA, empty context, hedged with fresh randomness
    /// (FIPS 204's default). RustCrypto's `ml-dsa` signed deterministically;
    /// both verify identically.
    pub fn ml_dsa_sign(
        &self,
        secret_key: &MlDsaSecretKey,
        message: &[u8],
    ) -> CryptoResult<MlDsaSignature> {
        let params = secret_key.public.parameter_set;
        self.require_approved(params.fips_id())?;
        let xi: &[u8; 32] = secret_key.data[..]
            .try_into()
            .map_err(|_| CryptoError::InvalidInput("invalid ML-DSA secret key".into()))?;
        let mut rnd = Zeroizing::new([0u8; 32]);
        self.random_bytes(&mut rnd[..])?;

        let sig_data = mldsa_with!(params, m, {
            let mut pk = [0u8; m::PUBLIC_KEY_LEN];
            let mut sk = Zeroizing::new([0u8; m::SECRET_KEY_LEN]);
            if !m::keygen(xi, &mut pk, &mut sk) {
                return Err(CryptoError::InvalidInput(
                    "invalid ML-DSA secret key".into(),
                ));
            }
            let mut sig = vec![0u8; m::SIGNATURE_LEN];
            let sig_arr: &mut [u8; m::SIGNATURE_LEN] =
                (&mut sig[..]).try_into().expect("sized above");
            if !m::sign(&sk, message, &[], &rnd, sig_arr) {
                return Err(CryptoError::EncryptionFailed("ML-DSA sign failed".into()));
            }
            sig
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
        self.require_approved(public_key.parameter_set.fips_id())?;

        if signature.parameter_set != public_key.parameter_set {
            return Ok(false);
        }

        let valid = mldsa_with!(public_key.parameter_set, m, {
            let Ok(pk) = <&[u8; m::PUBLIC_KEY_LEN]>::try_from(&public_key.data[..]) else {
                return Ok(false);
            };
            let Ok(sig) = <&[u8; m::SIGNATURE_LEN]>::try_from(&signature.data[..]) else {
                return Ok(false);
            };
            m::verify(pk, message, &[], sig)
        });

        Ok(valid)
    }

    // ========================================================================
    // SLH-DSA Operations (FIPS 205, via `slh-dsa`)
    // ========================================================================

    /// Generate an SLH-DSA key pair.
    pub fn slh_dsa_keygen(&self, params: SlhDsaParameterSet) -> CryptoResult<SlhDsaSecretKey> {
        self.require_inside_boundary("SLH-DSA")?;
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
        self.require_inside_boundary("SLH-DSA")?;
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
        self.require_inside_boundary("SLH-DSA")?;
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
        self.require_approved(_params.fips_id())?;
        Self::pqc_disabled()
    }
    /// ML-KEM encapsulation (requires the `pqc` feature).
    pub fn ml_kem_encaps(
        &self,
        _public_key: &MlKemPublicKey,
    ) -> CryptoResult<(MlKemCiphertext, Vec<u8>)> {
        self.require_approved(_public_key.parameter_set.fips_id())?;
        Self::pqc_disabled()
    }
    /// ML-KEM decapsulation (requires the `pqc` feature).
    pub fn ml_kem_decaps(
        &self,
        _secret_key: &MlKemSecretKey,
        _ciphertext: &MlKemCiphertext,
    ) -> CryptoResult<Vec<u8>> {
        self.require_approved(_secret_key.public.parameter_set.fips_id())?;
        Self::pqc_disabled()
    }
    /// ML-DSA key generation (requires the `pqc` feature).
    pub fn ml_dsa_keygen(&self, _params: MlDsaParameterSet) -> CryptoResult<MlDsaSecretKey> {
        self.require_approved(_params.fips_id())?;
        Self::pqc_disabled()
    }
    /// ML-DSA sign (requires the `pqc` feature).
    pub fn ml_dsa_sign(
        &self,
        _secret_key: &MlDsaSecretKey,
        _message: &[u8],
    ) -> CryptoResult<MlDsaSignature> {
        self.require_approved(_secret_key.public.parameter_set.fips_id())?;
        Self::pqc_disabled()
    }
    /// ML-DSA verify (requires the `pqc` feature).
    pub fn ml_dsa_verify(
        &self,
        _public_key: &MlDsaPublicKey,
        _message: &[u8],
        _signature: &MlDsaSignature,
    ) -> CryptoResult<bool> {
        self.require_approved(_public_key.parameter_set.fips_id())?;
        Self::pqc_disabled()
    }
    /// SLH-DSA key generation (requires the `pqc` feature).
    pub fn slh_dsa_keygen(&self, _params: SlhDsaParameterSet) -> CryptoResult<SlhDsaSecretKey> {
        self.require_inside_boundary("SLH-DSA")?;
        Self::pqc_disabled()
    }
    /// SLH-DSA sign (requires the `pqc` feature).
    pub fn slh_dsa_sign(
        &self,
        _secret_key: &SlhDsaSecretKey,
        _message: &[u8],
    ) -> CryptoResult<SlhDsaSignature> {
        self.require_inside_boundary("SLH-DSA")?;
        Self::pqc_disabled()
    }
    /// SLH-DSA verify (requires the `pqc` feature).
    pub fn slh_dsa_verify(
        &self,
        _public_key: &SlhDsaPublicKey,
        _message: &[u8],
        _signature: &SlhDsaSignature,
    ) -> CryptoResult<bool> {
        self.require_inside_boundary("SLH-DSA")?;
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

    /// `Strict` admits ML-KEM and ML-DSA now that they are IronCrypto's and
    /// `ic-fips` self-tests every parameter set, and still refuses SLH-DSA,
    /// which is RustCrypto's and outside that boundary.
    #[cfg(feature = "pqc")]
    #[test]
    fn strict_mode_admits_ironcrypto_pqc_and_refuses_the_rest() {
        let strict = FipsCrypto::new(FipsMode::Strict).unwrap();
        let refused = |r: CryptoResult<()>| matches!(r, Err(CryptoError::AlgorithmNotApproved(_)));

        let kem = strict.ml_kem_keygen(MlKemParameterSet::MlKem768).unwrap();
        let (ct, sent) = strict.ml_kem_encaps(&kem.public).unwrap();
        assert_eq!(strict.ml_kem_decaps(&kem, &ct).unwrap(), sent);

        let sk = strict.ml_dsa_keygen(MlDsaParameterSet::MlDsa65).unwrap();
        let sig = strict.ml_dsa_sign(&sk, b"m").unwrap();
        assert!(strict.ml_dsa_verify(&sk.public, b"m", &sig).unwrap());

        assert!(refused(
            strict
                .slh_dsa_keygen(SlhDsaParameterSet::Sha2_128f)
                .map(|_| ())
        ));
    }

    /// Keys stored before the move to IronCrypto were RustCrypto's seeds.
    /// The same ML-KEM seed must give the same encapsulation key, and each
    /// side must decapsulate what the other encapsulated.
    #[cfg(feature = "pqc")]
    #[test]
    fn ml_kem_interoperates_with_rustcrypto_for_every_parameter_set() {
        use ml_kem::kem::{Ciphertext, Decapsulate, Encapsulate, Key, KeyExport};
        use ml_kem::{DecapsulationKey, EncapsulationKey};

        // A macro, not a generic fn: ml-kem does not export its parameter trait.
        macro_rules! check {
            ($p:ty, $params:expr) => {{
                let crypto = get_crypto();
                let params = $params;
                let ours = crypto.ml_kem_keygen(params).unwrap();
                let seed = ml_kem::Seed::try_from(&ours.data[..]).unwrap();
                let theirs = DecapsulationKey::<$p>::from_seed(seed);
                let ek = theirs.encapsulation_key();
                assert_eq!(ek.to_bytes().to_vec(), ours.public.data, "{params:?}");

                // Theirs encapsulates, ours decapsulates.
                let (ct, sent) = ek.encapsulate();
                let ct_ours = MlKemCiphertext {
                    data: ct.to_vec(),
                    parameter_set: params,
                };
                assert_eq!(
                    crypto.ml_kem_decaps(&ours, &ct_ours).unwrap(),
                    sent.to_vec()
                );

                // Ours encapsulates, theirs decapsulates.
                let (ct, sent) = crypto.ml_kem_encaps(&ours.public).unwrap();
                let ct = Ciphertext::<$p>::try_from(&ct.data[..]).unwrap();
                assert_eq!(theirs.decapsulate(&ct).to_vec(), sent, "{params:?}");

                let _ = Key::<EncapsulationKey<$p>>::try_from(&ours.public.data[..]).unwrap();
            }};
        }

        check!(ml_kem::MlKem512, MlKemParameterSet::MlKem512);
        check!(ml_kem::MlKem768, MlKemParameterSet::MlKem768);
        check!(ml_kem::MlKem1024, MlKemParameterSet::MlKem1024);
    }

    /// The same for ML-DSA: one seed, one verifying key, and each side's
    /// signatures verify under the other.
    #[cfg(feature = "pqc")]
    #[test]
    fn ml_dsa_interoperates_with_rustcrypto_for_every_parameter_set() {
        use ml_dsa::signature::{Keypair, Signer, Verifier};
        use ml_dsa::{KeyExport, KeyInit, Signature, SigningKey, VerifyingKey};

        macro_rules! check {
            ($p:ty, $params:expr) => {{
                let crypto = get_crypto();
                let params = $params;
                let ours = crypto.ml_dsa_keygen(params).unwrap();
                let seed = ml_dsa::common::Key::<SigningKey<$p>>::try_from(&ours.data[..]).unwrap();
                let theirs = SigningKey::<$p>::new(&seed);
                let vk = theirs.verifying_key();
                assert_eq!(vk.to_bytes().to_vec(), ours.public.data, "{params:?}");

                let message = b"signed on one side, verified on the other";
                let sig = crypto.ml_dsa_sign(&ours, message).unwrap();
                let parsed = Signature::<$p>::try_from(&sig.data[..]).unwrap();
                assert!(vk.verify(message, &parsed).is_ok(), "{params:?}");

                let theirs_sig = MlDsaSignature {
                    data: theirs.try_sign(message).unwrap().encode().to_vec(),
                    parameter_set: params,
                };
                assert!(crypto
                    .ml_dsa_verify(&ours.public, message, &theirs_sig)
                    .unwrap());
                assert!(!crypto
                    .ml_dsa_verify(&ours.public, b"other", &theirs_sig)
                    .unwrap());

                let vk_bytes =
                    ml_dsa::common::Key::<VerifyingKey<$p>>::try_from(&ours.public.data[..])
                        .unwrap();
                let _ = VerifyingKey::<$p>::new(&vk_bytes);
            }};
        }

        check!(ml_dsa::MlDsa44, MlDsaParameterSet::MlDsa44);
        check!(ml_dsa::MlDsa65, MlDsaParameterSet::MlDsa65);
        check!(ml_dsa::MlDsa87, MlDsaParameterSet::MlDsa87);
    }

    /// Every ML-DSA parameter set signs and verifies, not only ML-DSA-65:
    /// generating a key of the right length says nothing about signing with it.
    #[cfg(feature = "pqc")]
    #[test]
    fn every_ml_dsa_parameter_set_signs_and_verifies() {
        let crypto = get_crypto();
        for params in [
            MlDsaParameterSet::MlDsa44,
            MlDsaParameterSet::MlDsa65,
            MlDsaParameterSet::MlDsa87,
        ] {
            let sk = crypto.ml_dsa_keygen(params).unwrap();
            let sig = crypto
                .ml_dsa_sign(&sk, b"every parameter set")
                .unwrap_or_else(|e| panic!("{params:?} sign: {e}"));
            assert_eq!(sig.data.len(), params.signature_bytes(), "{params:?}");
            assert!(
                crypto
                    .ml_dsa_verify(&sk.public, b"every parameter set", &sig)
                    .unwrap(),
                "{params:?} did not verify its own signature"
            );
            assert!(
                !crypto.ml_dsa_verify(&sk.public, b"tampered", &sig).unwrap(),
                "{params:?} verified a tampered message"
            );
        }
    }

    /// Every SLH-DSA parameter set this module offers. Four of the six had
    /// never been generated in a test, let alone signed with.
    #[cfg(feature = "pqc")]
    #[test]
    fn every_slh_dsa_parameter_set_signs_and_verifies() {
        let crypto = get_crypto();
        for params in [
            SlhDsaParameterSet::Sha2_128f,
            SlhDsaParameterSet::Sha2_128s,
            SlhDsaParameterSet::Sha2_192f,
            SlhDsaParameterSet::Sha2_256f,
            SlhDsaParameterSet::Shake128f,
            SlhDsaParameterSet::Shake256f,
        ] {
            let sk = crypto
                .slh_dsa_keygen(params)
                .unwrap_or_else(|e| panic!("{params:?} keygen: {e}"));
            assert_eq!(
                sk.public.data.len(),
                params.public_key_bytes(),
                "{params:?}"
            );
            let sig = crypto
                .slh_dsa_sign(&sk, b"every parameter set")
                .unwrap_or_else(|e| panic!("{params:?} sign: {e}"));
            assert_eq!(sig.data.len(), params.signature_bytes(), "{params:?}");
            assert!(
                crypto
                    .slh_dsa_verify(&sk.public, b"every parameter set", &sig)
                    .unwrap(),
                "{params:?} did not verify its own signature"
            );
            assert!(
                !crypto
                    .slh_dsa_verify(&sk.public, b"tampered", &sig)
                    .unwrap(),
                "{params:?} verified a tampered message"
            );
        }
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
