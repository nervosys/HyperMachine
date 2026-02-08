//! Cryptographic Module
//!
//! FIPS 140-3 compliant cryptographic primitives for HyperMachine.
//!
//! # Features
//!
//! - **fips**: Enable FIPS-validated implementations (default: disabled)
//! - **ring**: Use ring crypto library (recommended)
//!
//! # Modules
//!
//! - `fips`: FIPS 140-3 compliant symmetric crypto operations
//! - `asymmetric`: RSA and ECDSA operations
//! - `pqc`: Post-quantum cryptography

pub mod fips;
pub mod asymmetric;
pub mod pqc;

pub use fips::{
    AesGcmCiphertext, AesKeySize, CryptoError, CryptoResult, FipsCrypto, FipsMode, FipsStatus,
    KeyPair, SymmetricKey,
};
