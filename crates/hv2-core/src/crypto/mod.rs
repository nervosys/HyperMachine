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
//! - `fips`: FIPS 140-3 compliant operations

pub mod fips;

pub use fips::{
    AesGcmCiphertext, AesKeySize, CryptoError, CryptoResult, FipsCrypto, FipsMode, FipsStatus,
    KeyPair, SymmetricKey,
};
