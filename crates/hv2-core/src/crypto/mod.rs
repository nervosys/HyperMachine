//! Cryptographic Module
//!
//! FIPS 140-3 compliant cryptographic primitives for HyperMachine.
//!
//! # Features
//!
//! - **fips**: Enable FIPS-validated implementations (default: disabled)
//! - **pqc**: Post-quantum algorithms (default: enabled)
//!
//! The primitives themselves are unconditional. They come from IronCrypto
//! (`ic-*`), which is pure Rust and `no_std`, so there is no longer a feature
//! that decides whether cryptography is present -- the `ring` feature that used
//! to make that choice is gone, along with the stub paths it selected between.
//!
//! # Modules
//!
//! - `fips`: FIPS 140-3 compliant symmetric crypto operations
//! - `asymmetric`: RSA and ECDSA operations
//! - `pqc`: Post-quantum cryptography

pub mod asymmetric;
pub mod fips;
pub mod pqc;

pub use fips::{
    AesGcmCiphertext, AesKeySize, CryptoError, CryptoResult, FipsCrypto, FipsMode, FipsStatus,
    KeyPair, SymmetricKey,
};
