//! Security and isolation infrastructure
//!
//! This module provides security features for the hypervisor:
//! - Memory encryption (AMD SEV/TDX)
//! - Virtual TPM (vTPM 2.0)
//! - Secure boot chain verification

pub mod image_registry;
pub mod memory_encryption;
pub mod secure_boot;
pub mod vtpm;

pub use memory_encryption::{
    CbitPosition, EncryptionConfig, EncryptionError, EncryptionManager, EncryptionResult,
    EncryptionStats, EncryptionTechnology, KeyId, KeyMetadata, KeyState, PageEncryptionState,
    SevContext, SevLaunchState, TdxContext,
};

pub use secure_boot::{
    BootComponent, BootComponentType, Certificate, CertificateStatus, CertificateType,
    SecureBootError, SecureBootManager, SecureBootMode, SecureBootPolicy, SecureBootResult,
    SecureBootStats, Signature, SignatureAlgorithm, VerificationResult,
};

pub use vtpm::{
    HashAlgorithm, KeyHandle, KeyType, NvEntry, NvIndex, PcrBank, TpmCommandCode, TpmKey,
    TpmResponseCode, VirtualTpm,
};

pub use image_registry::{
    AdmissionDecision, ApprovalStatus, EnforcementMode, ImageEntry, ImageKind, ImageRegistry,
    ImageSignature, RegistryConfig, RegistryError, RegistryResult,
};
