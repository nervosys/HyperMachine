//! Secure boot chain verification
//!
//! This module provides secure boot infrastructure including UEFI Secure Boot
//! verification, certificate management, and boot policy enforcement.
//!
//! # What a boot signature is
//!
//! A signature covers [`BootComponent::signed_message`]: the domain separator
//! [`SIGNED_MESSAGE_PREFIX`], one byte naming the component type, then the
//! component's image digest. The prefix keeps a signature made for anything
//! else from being replayed here, and the type byte keeps a kernel's signature
//! from admitting a module with the same digest.
//!
//! - RSA signatures are PKCS#1 v1.5 with the hash the algorithm names.
//! - ECDSA signatures are fixed-width `r || s`, P-256 with SHA-256 and P-384
//!   with SHA-384.
//! - A [`Certificate`]'s `public_key` is a DER `SubjectPublicKeyInfo`. EC
//!   points must be uncompressed.
//!
//! This is HyperMachine's own format. It does not parse or interoperate with
//! UEFI Authenticode signatures embedded in PE images.
//!
//! A signature is checked against the public key of the **trusted database
//! entry** its claimed signer names, never against the certificate travelling
//! with it. That certificate is chosen by whoever built the component.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::RwLock;

/// Signature algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
    /// RSA with SHA-256
    RsaSha256,
    /// RSA with SHA-384
    RsaSha384,
    /// RSA with SHA-512
    RsaSha512,
    /// ECDSA with SHA-256
    EcdsaSha256,
    /// ECDSA with SHA-384
    EcdsaSha384,
}

impl SignatureAlgorithm {
    /// Get signature size in bytes
    pub fn signature_size(&self) -> usize {
        match self {
            Self::RsaSha256 | Self::RsaSha384 | Self::RsaSha512 => 256, // RSA-2048
            Self::EcdsaSha256 => 64,
            Self::EcdsaSha384 => 96,
        }
    }

    /// Get hash size in bytes
    pub fn hash_size(&self) -> usize {
        match self {
            Self::RsaSha256 | Self::EcdsaSha256 => 32,
            Self::RsaSha384 | Self::EcdsaSha384 => 48,
            Self::RsaSha512 => 64,
        }
    }
}

/// Certificate type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateType {
    /// Platform Key (PK)
    PlatformKey,
    /// Key Exchange Key (KEK)
    KeyExchangeKey,
    /// Database (db) - allowed signatures
    Database,
    /// Forbidden Database (dbx) - revoked signatures
    ForbiddenDatabase,
    /// OEM certificate
    Oem,
    /// User certificate
    User,
}

/// Certificate status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateStatus {
    /// Certificate is valid
    Valid,
    /// Certificate has expired
    Expired,
    /// Certificate has been revoked
    Revoked,
    /// Certificate is not yet valid
    NotYetValid,
    /// Certificate is untrusted
    Untrusted,
}

/// X.509 certificate (simplified)
#[derive(Debug, Clone)]
pub struct Certificate {
    /// Certificate type
    pub cert_type: CertificateType,
    /// Subject name
    pub subject: String,
    /// Issuer name
    pub issuer: String,
    /// Serial number
    pub serial: Vec<u8>,
    /// Not valid before (Unix timestamp)
    pub not_before: u64,
    /// Not valid after (Unix timestamp)
    pub not_after: u64,
    /// Public key
    pub public_key: Vec<u8>,
    /// Signature algorithm
    pub algorithm: SignatureAlgorithm,
    /// Raw certificate data
    pub raw: Vec<u8>,
    /// Status
    pub status: CertificateStatus,
}

impl Certificate {
    /// Create a new certificate
    pub fn new(
        cert_type: CertificateType,
        subject: impl Into<String>,
        issuer: impl Into<String>,
        public_key: Vec<u8>,
        algorithm: SignatureAlgorithm,
    ) -> Self {
        Self {
            cert_type,
            subject: subject.into(),
            issuer: issuer.into(),
            serial: vec![0x01],
            not_before: 0,
            not_after: u64::MAX,
            public_key,
            algorithm,
            raw: Vec::new(),
            status: CertificateStatus::Valid,
        }
    }

    /// Check if certificate is valid at given time
    pub fn is_valid_at(&self, timestamp: u64) -> bool {
        timestamp >= self.not_before && timestamp <= self.not_after
    }

    /// Check if certificate is self-signed
    pub fn is_self_signed(&self) -> bool {
        self.subject == self.issuer
    }
}

/// Signature for verification
#[derive(Debug, Clone)]
pub struct Signature {
    /// Algorithm used
    pub algorithm: SignatureAlgorithm,
    /// Signature data
    pub data: Vec<u8>,
    /// Signer certificate (if available)
    pub signer: Option<Certificate>,
}

impl Signature {
    /// Create new signature
    pub fn new(algorithm: SignatureAlgorithm, data: Vec<u8>) -> Self {
        Self {
            algorithm,
            data,
            signer: None,
        }
    }

    /// Attach signer certificate
    pub fn with_signer(mut self, cert: Certificate) -> Self {
        self.signer = Some(cert);
        self
    }
}

/// Boot component type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootComponentType {
    /// UEFI firmware
    Firmware,
    /// Option ROM
    OptionRom,
    /// Boot loader
    BootLoader,
    /// Operating system kernel
    Kernel,
    /// Kernel module / driver
    KernelModule,
    /// Init ramdisk
    InitRd,
    /// Configuration file
    Config,
}

/// Boot component for verification
#[derive(Debug, Clone)]
pub struct BootComponent {
    /// Component type
    pub component_type: BootComponentType,
    /// Name / path
    pub name: String,
    /// Hash of component
    pub hash: Vec<u8>,
    /// Hash algorithm
    pub hash_algorithm: SignatureAlgorithm,
    /// Signature
    pub signature: Option<Signature>,
    /// Size in bytes
    pub size: u64,
    /// Load address
    pub load_address: u64,
}

/// The domain separator every boot signature's message starts with.
pub const SIGNED_MESSAGE_PREFIX: &[u8] = b"HyperMachine secure boot v1\0";

impl BootComponentType {
    /// The byte that names this type in a signed message.
    ///
    /// Written out rather than `as u8`, so reordering the enum cannot change
    /// what an existing signature covers.
    fn wire_id(self) -> u8 {
        match self {
            Self::Firmware => 1,
            Self::OptionRom => 2,
            Self::BootLoader => 3,
            Self::Kernel => 4,
            Self::KernelModule => 5,
            Self::InitRd => 6,
            Self::Config => 7,
        }
    }
}

impl BootComponent {
    /// The exact bytes a signature over this component covers: the prefix,
    /// the type byte, then the image digest. A signing tool must sign these.
    pub fn signed_message(&self) -> Vec<u8> {
        let mut message = Vec::with_capacity(SIGNED_MESSAGE_PREFIX.len() + 1 + self.hash.len());
        message.extend_from_slice(SIGNED_MESSAGE_PREFIX);
        message.push(self.component_type.wire_id());
        message.extend_from_slice(&self.hash);
        message
    }

    /// Create new boot component
    pub fn new(component_type: BootComponentType, name: impl Into<String>, hash: Vec<u8>) -> Self {
        Self {
            component_type,
            name: name.into(),
            hash,
            hash_algorithm: SignatureAlgorithm::RsaSha256,
            signature: None,
            size: 0,
            load_address: 0,
        }
    }

    /// Set signature
    pub fn with_signature(mut self, signature: Signature) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Set size
    pub fn with_size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }

    /// Set load address
    pub fn with_load_address(mut self, address: u64) -> Self {
        self.load_address = address;
        self
    }
}

/// Verification result
///
/// `#[non_exhaustive]` because this grows: `VerificationUnavailable` was added
/// when the signature path stopped pretending to verify, and real signature
/// checking will distinguish more outcomes than it does today. A caller that
/// matched every variant exhaustively would break on each of those, which is
/// the wrong pressure to put on a security decision -- the safe default for a
/// verdict a caller does not recognise is to refuse, and a wildcard arm makes
/// that expressible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VerificationResult {
    /// Verification succeeded
    Success,
    /// Signature is invalid
    InvalidSignature,
    /// Certificate not trusted
    UntrustedCertificate,
    /// Certificate expired
    CertificateExpired,
    /// Certificate revoked
    CertificateRevoked,
    /// Hash mismatch
    HashMismatch,
    /// Component in forbidden list
    Forbidden,
    /// No signature present
    NoSignature,
    /// Unknown algorithm
    UnknownAlgorithm,
    /// No signature verification is implemented, so nothing was verified.
    ///
    /// Distinct from [`Self::InvalidSignature`], which is a verdict. This is
    /// the absence of one: the signature was neither accepted nor rejected,
    /// because this build cannot check it. Admission must treat it as a
    /// refusal -- see [`SecureBootManager::verify`].
    VerificationUnavailable,
}

/// Secure boot mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureBootMode {
    /// Secure boot disabled
    Disabled,
    /// Setup mode (PK not enrolled).
    ///
    /// Every mode but [`Self::Disabled`] verifies and refuses identically;
    /// the others are recorded, not acted on. In particular, key-database
    /// updates are not authenticated in any mode (UEFI requires that in
    /// User mode). That is contained only because this manager is a host-side
    /// API no guest can reach. `tools/find-unread-controls.py` lists these
    /// variants, with this reason.
    Setup,
    /// User mode. Enforces exactly as [`Self::Setup`] does.
    User,
    /// Deployed mode. Enforces exactly as [`Self::User`] does; nothing makes
    /// it more restrictive.
    Deployed,
    /// Audit mode. **Enforces, like every mode but `Disabled`**: an
    /// unverified component is refused, not logged and admitted. Audit-only
    /// admission would loosen secure boot, and is deliberately not
    /// implemented here.
    Audit,
}

/// Secure boot policy
#[derive(Debug, Clone)]
pub struct SecureBootPolicy {
    /// Boot mode
    pub mode: SecureBootMode,
    /// Allow unsigned drivers
    pub allow_unsigned_drivers: bool,
    /// Allow unsigned option ROMs
    pub allow_unsigned_option_roms: bool,
    /// Require measured boot
    pub require_measured_boot: bool,
    /// Maximum hash age (0 = no limit)
    pub max_hash_age_secs: u64,
}

impl Default for SecureBootPolicy {
    fn default() -> Self {
        Self {
            mode: SecureBootMode::Disabled,
            allow_unsigned_drivers: true,
            allow_unsigned_option_roms: true,
            require_measured_boot: false,
            max_hash_age_secs: 0,
        }
    }
}

/// Secure boot error
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecureBootError {
    /// Not enabled
    #[error("Secure boot not enabled")]
    NotEnabled,
    /// Already enrolled
    #[error("Key already enrolled")]
    AlreadyEnrolled,
    /// Not enrolled
    #[error("Key not enrolled")]
    NotEnrolled,
    /// Invalid certificate
    #[error("Invalid certificate: {0}")]
    InvalidCertificate(String),
    /// Verification failed
    #[error("Verification failed: {0:?}")]
    VerificationFailed(VerificationResult),
    /// Policy violation
    #[error("Policy violation: {0}")]
    PolicyViolation(String),
}

/// Result type for secure boot operations
pub type SecureBootResult<T> = Result<T, SecureBootError>;

/// Secure boot manager
#[derive(Debug)]
pub struct SecureBootManager {
    /// Policy
    policy: RwLock<SecureBootPolicy>,
    /// Platform Key (only one allowed)
    platform_key: RwLock<Option<Certificate>>,
    /// Key Exchange Keys
    keks: RwLock<Vec<Certificate>>,
    /// Database (allowed)
    db: RwLock<Vec<Certificate>>,
    /// Forbidden database (revoked)
    dbx: RwLock<Vec<Certificate>>,
    /// Hash allowlist
    hash_allowlist: RwLock<HashMap<Vec<u8>, String>>,
    /// Hash blocklist
    hash_blocklist: RwLock<HashMap<Vec<u8>, String>>,
    /// Verification count
    verification_count: AtomicU64,
    /// Verification failures
    verification_failures: AtomicU64,
    /// Enabled flag
    enabled: AtomicBool,
}

impl SecureBootManager {
    /// Create new secure boot manager
    pub fn new() -> Self {
        Self {
            policy: RwLock::new(SecureBootPolicy::default()),
            platform_key: RwLock::new(None),
            keks: RwLock::new(Vec::new()),
            db: RwLock::new(Vec::new()),
            dbx: RwLock::new(Vec::new()),
            hash_allowlist: RwLock::new(HashMap::new()),
            hash_blocklist: RwLock::new(HashMap::new()),
            verification_count: AtomicU64::new(0),
            verification_failures: AtomicU64::new(0),
            enabled: AtomicBool::new(false),
        }
    }

    /// Get policy
    pub fn policy(&self) -> SecureBootPolicy {
        self.policy.read().clone()
    }

    /// Set policy
    pub fn set_policy(&self, policy: SecureBootPolicy) {
        *self.policy.write() = policy;
    }

    /// Get current mode
    pub fn mode(&self) -> SecureBootMode {
        self.policy.read().mode
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Enable secure boot
    pub fn enable(&self) -> SecureBootResult<()> {
        let pk = self.platform_key.read();
        if pk.is_none() {
            // No PK = setup mode
            let mut policy = self.policy.write();
            policy.mode = SecureBootMode::Setup;
        } else {
            let mut policy = self.policy.write();
            policy.mode = SecureBootMode::User;
        }
        self.enabled.store(true, Ordering::Release);
        Ok(())
    }

    /// Disable secure boot
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
        self.policy.write().mode = SecureBootMode::Disabled;
    }

    /// Enroll platform key
    pub fn enroll_pk(&self, cert: Certificate) -> SecureBootResult<()> {
        let mut pk = self.platform_key.write();
        if pk.is_some() {
            return Err(SecureBootError::AlreadyEnrolled);
        }

        if cert.cert_type != CertificateType::PlatformKey {
            return Err(SecureBootError::InvalidCertificate(
                "Not a platform key".to_string(),
            ));
        }

        *pk = Some(cert);

        // Transition from setup to user mode
        if self.is_enabled() {
            self.policy.write().mode = SecureBootMode::User;
        }

        Ok(())
    }

    /// Get platform key
    pub fn get_pk(&self) -> Option<Certificate> {
        self.platform_key.read().clone()
    }

    /// Add key exchange key
    pub fn add_kek(&self, cert: Certificate) -> SecureBootResult<()> {
        if cert.cert_type != CertificateType::KeyExchangeKey {
            return Err(SecureBootError::InvalidCertificate("Not a KEK".to_string()));
        }
        self.keks.write().push(cert);
        Ok(())
    }

    /// Add to database (db)
    pub fn add_db(&self, cert: Certificate) -> SecureBootResult<()> {
        if cert.cert_type != CertificateType::Database {
            return Err(SecureBootError::InvalidCertificate(
                "Not a db certificate".to_string(),
            ));
        }
        self.db.write().push(cert);
        Ok(())
    }

    /// Add to forbidden database (dbx)
    pub fn add_dbx(&self, cert: Certificate) -> SecureBootResult<()> {
        if cert.cert_type != CertificateType::ForbiddenDatabase {
            return Err(SecureBootError::InvalidCertificate(
                "Not a dbx certificate".to_string(),
            ));
        }
        self.dbx.write().push(cert);
        Ok(())
    }

    /// Add hash to allowlist
    pub fn allow_hash(&self, hash: Vec<u8>, description: impl Into<String>) {
        self.hash_allowlist.write().insert(hash, description.into());
    }

    /// Add hash to blocklist
    pub fn block_hash(&self, hash: Vec<u8>, description: impl Into<String>) {
        self.hash_blocklist.write().insert(hash, description.into());
    }

    /// Check if hash is allowed
    pub fn is_hash_allowed(&self, hash: &[u8]) -> bool {
        self.hash_allowlist.read().contains_key(hash)
    }

    /// Check if hash is blocked
    pub fn is_hash_blocked(&self, hash: &[u8]) -> bool {
        self.hash_blocklist.read().contains_key(hash)
    }

    /// Verify a boot component
    pub fn verify(&self, component: &BootComponent) -> VerificationResult {
        self.verification_count.fetch_add(1, Ordering::Relaxed);

        // Check blocklist first
        if self.is_hash_blocked(&component.hash) {
            self.verification_failures.fetch_add(1, Ordering::Relaxed);
            return VerificationResult::Forbidden;
        }

        // If in allowlist, allow immediately
        if self.is_hash_allowed(&component.hash) {
            return VerificationResult::Success;
        }

        // Only a disabled manager passes components through. Audit mode is not
        // an exception: it enforces (see `SecureBootMode::Audit`).
        let policy = self.policy.read();
        if !self.is_enabled() || policy.mode == SecureBootMode::Disabled {
            return VerificationResult::Success;
        }

        // Check signature
        let signature = match &component.signature {
            Some(sig) => sig,
            None => {
                // Check policy for unsigned components
                let allow_unsigned = match component.component_type {
                    BootComponentType::KernelModule => policy.allow_unsigned_drivers,
                    BootComponentType::OptionRom => policy.allow_unsigned_option_roms,
                    _ => false,
                };

                if allow_unsigned {
                    return VerificationResult::Success;
                }

                self.verification_failures.fetch_add(1, Ordering::Relaxed);
                return VerificationResult::NoSignature;
            }
        };

        // Verify signature against db
        let db = self.db.read();
        let dbx = self.dbx.read();

        // Check if signer is in dbx (revoked)
        if let Some(ref signer) = signature.signer {
            for revoked in dbx.iter() {
                if revoked.subject == signer.subject {
                    self.verification_failures.fetch_add(1, Ordering::Relaxed);
                    return VerificationResult::CertificateRevoked;
                }
            }
        }

        // Is the claimed signer's subject one this platform trusts?
        //
        // Note what this is and is not. Matching a subject says the component
        // *claims* to come from a trusted signer. It says nothing about
        // whether the signature is that signer's, because the subject is a
        // string on a certificate the caller supplied.
        //
        // Until 2026-09-22 this arm returned `Success`, under a comment
        // reading "Would verify actual signature here". So secure boot
        // admitted any component whose signature carried a `signer` whose
        // `subject` string matched a trusted certificate -- a forgery needed
        // no cryptography, only the right string. It now refuses.
        let claims_a_trusted_signer = signature
            .signer
            .as_ref()
            .is_some_and(|signer| db.iter().any(|trusted| trusted.subject == signer.subject));

        if !claims_a_trusted_signer {
            self.verification_failures.fetch_add(1, Ordering::Relaxed);
            return VerificationResult::UntrustedCertificate;
        }
        let Some(subject) = signature.signer.as_ref().map(|s| &s.subject) else {
            // Unreachable: `claims_a_trusted_signer` needs a signer.
            self.verification_failures.fetch_add(1, Ordering::Relaxed);
            return VerificationResult::UntrustedCertificate;
        };

        // Claimed; now shown, or not. The claim only selects which trusted
        // entries to try. The key comes from the trusted entry, never from the
        // certificate attached to the signature, which whoever built the
        // component chose. Until 2026-09-28 this refused every signed
        // component with `VerificationUnavailable`; before 2026-09-22 it
        // admitted any whose claimed subject matched.
        let message = component.signed_message();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut saw_expired = false;
        for trusted in db.iter().filter(|c| &c.subject == subject) {
            match trusted.status {
                CertificateStatus::Valid => {}
                CertificateStatus::Revoked => {
                    self.verification_failures.fetch_add(1, Ordering::Relaxed);
                    return VerificationResult::CertificateRevoked;
                }
                CertificateStatus::Expired => {
                    saw_expired = true;
                    continue;
                }
                CertificateStatus::NotYetValid | CertificateStatus::Untrusted => continue,
            }
            // A key in dbx is revoked under whatever subject it is enrolled.
            if dbx.iter().any(|revoked| {
                !revoked.public_key.is_empty() && revoked.public_key == trusted.public_key
            }) {
                self.verification_failures.fetch_add(1, Ordering::Relaxed);
                return VerificationResult::CertificateRevoked;
            }
            if !trusted.is_valid_at(now) {
                saw_expired = true;
                continue;
            }
            if verify_boot_signature(
                &trusted.public_key,
                signature.algorithm,
                &message,
                &signature.data,
            ) {
                return VerificationResult::Success;
            }
        }

        self.verification_failures.fetch_add(1, Ordering::Relaxed);
        if saw_expired {
            VerificationResult::CertificateExpired
        } else {
            VerificationResult::InvalidSignature
        }
    }

    /// Get verification statistics
    pub fn stats(&self) -> SecureBootStats {
        SecureBootStats {
            mode: self.mode(),
            enabled: self.is_enabled(),
            verification_count: self.verification_count.load(Ordering::Relaxed),
            verification_failures: self.verification_failures.load(Ordering::Relaxed),
            pk_enrolled: self.platform_key.read().is_some(),
            kek_count: self.keks.read().len(),
            db_count: self.db.read().len(),
            dbx_count: self.dbx.read().len(),
            allowlist_count: self.hash_allowlist.read().len(),
            blocklist_count: self.hash_blocklist.read().len(),
        }
    }

    /// Get KEK count
    pub fn kek_count(&self) -> usize {
        self.keks.read().len()
    }

    /// Get db count
    pub fn db_count(&self) -> usize {
        self.db.read().len()
    }

    /// Get dbx count
    pub fn dbx_count(&self) -> usize {
        self.dbx.read().len()
    }
}

impl Default for SecureBootManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Secure boot statistics
#[derive(Debug, Clone)]
pub struct SecureBootStats {
    /// Current mode
    pub mode: SecureBootMode,
    /// Whether secure boot is enabled
    pub enabled: bool,
    /// Total verification attempts
    pub verification_count: u64,
    /// Failed verifications
    pub verification_failures: u64,
    /// PK enrolled
    pub pk_enrolled: bool,
    /// Number of KEKs
    pub kek_count: usize,
    /// Number of db entries
    pub db_count: usize,
    /// Number of dbx entries
    pub dbx_count: usize,
    /// Number of allowlist entries
    pub allowlist_count: usize,
    /// Number of blocklist entries
    pub blocklist_count: usize,
}

/// Whether `signature` over `message` verifies under the DER
/// `SubjectPublicKeyInfo` `spki`, with `algorithm`.
///
/// `false` for anything that does not verify, including a key that does not
/// parse, a key of the wrong type for `algorithm`, or a compressed EC point.
/// None of those is a reason to admit a component, and the caller reports all
/// of them as an invalid signature.
fn verify_boot_signature(
    spki: &[u8],
    algorithm: SignatureAlgorithm,
    message: &[u8],
    signature: &[u8],
) -> bool {
    use crate::crypto::asymmetric::{
        EcCurve, EcPublicKey, RsaKeySize, RsaPublicKey, Signature as CryptoSignature,
        SignatureAlgorithm as Scheme,
    };
    use crate::crypto::{FipsCrypto, FipsMode};
    use ic_pkix::{KeyAlgorithm, PublicKeyInfo};

    let Ok(key) = PublicKeyInfo::from_der(spki) else {
        return false;
    };
    let Ok(crypto) = FipsCrypto::new(FipsMode::Disabled) else {
        return false;
    };
    let as_signature = |scheme| CryptoSignature {
        data: signature.to_vec(),
        algorithm: scheme,
    };

    match key {
        PublicKeyInfo::Rsa { modulus, exponent } => {
            let scheme = match algorithm {
                SignatureAlgorithm::RsaSha256 => Scheme::RsaPkcs1Sha256,
                SignatureAlgorithm::RsaSha384 => Scheme::RsaPkcs1Sha384,
                SignatureAlgorithm::RsaSha512 => Scheme::RsaPkcs1Sha512,
                _ => return false,
            };
            let size = match modulus.len() {
                256 => RsaKeySize::Rsa2048,
                384 => RsaKeySize::Rsa3072,
                512 => RsaKeySize::Rsa4096,
                _ => return false,
            };
            let public = RsaPublicKey {
                n: modulus.to_vec(),
                e: exponent.to_be_bytes().to_vec(),
                size,
            };
            crypto
                .rsa_verify(&public, message, &as_signature(scheme))
                .unwrap_or(false)
        }
        PublicKeyInfo::Ec {
            algorithm: curve_id,
            point,
        } => {
            let (curve, scheme, width) = match (curve_id, algorithm) {
                (KeyAlgorithm::EcP256, SignatureAlgorithm::EcdsaSha256) => {
                    (EcCurve::P256, Scheme::EcdsaP256Sha256, 32)
                }
                (KeyAlgorithm::EcP384, SignatureAlgorithm::EcdsaSha384) => {
                    (EcCurve::P384, Scheme::EcdsaP384Sha384, 48)
                }
                _ => return false,
            };
            if point.len() != 1 + 2 * width || point[0] != 0x04 {
                return false;
            }
            let public = EcPublicKey {
                x: point[1..=width].to_vec(),
                y: point[1 + width..].to_vec(),
                curve,
            };
            crypto
                .ecdsa_verify(&public, message, &as_signature(scheme))
                .unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pk() -> Certificate {
        Certificate::new(
            CertificateType::PlatformKey,
            "Test PK",
            "Test PK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        )
    }

    fn make_kek() -> Certificate {
        Certificate::new(
            CertificateType::KeyExchangeKey,
            "Test KEK",
            "Test PK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        )
    }

    fn make_db_cert() -> Certificate {
        Certificate::new(
            CertificateType::Database,
            "Test DB",
            "Test KEK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        )
    }

    fn make_dbx_cert() -> Certificate {
        Certificate::new(
            CertificateType::ForbiddenDatabase,
            "Revoked Cert",
            "Test KEK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        )
    }

    #[test]
    fn test_signature_algorithm() {
        assert_eq!(SignatureAlgorithm::RsaSha256.hash_size(), 32);
        assert_eq!(SignatureAlgorithm::RsaSha384.hash_size(), 48);
        assert_eq!(SignatureAlgorithm::EcdsaSha256.signature_size(), 64);
    }

    #[test]
    fn test_certificate_creation() {
        let cert = make_pk();
        assert_eq!(cert.cert_type, CertificateType::PlatformKey);
        assert!(cert.is_self_signed());
        assert!(cert.is_valid_at(1000));
    }

    #[test]
    fn test_certificate_validity() {
        let mut cert = make_pk();
        cert.not_before = 100;
        cert.not_after = 200;

        assert!(!cert.is_valid_at(50));
        assert!(cert.is_valid_at(150));
        assert!(!cert.is_valid_at(250));
    }

    #[test]
    fn test_secure_boot_manager_creation() {
        let manager = SecureBootManager::new();
        assert!(!manager.is_enabled());
        assert_eq!(manager.mode(), SecureBootMode::Disabled);
    }

    #[test]
    fn test_enable_without_pk() {
        let manager = SecureBootManager::new();
        manager.enable().unwrap();

        assert!(manager.is_enabled());
        assert_eq!(manager.mode(), SecureBootMode::Setup);
    }

    #[test]
    fn test_enable_with_pk() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.enable().unwrap();

        assert!(manager.is_enabled());
        assert_eq!(manager.mode(), SecureBootMode::User);
    }

    #[test]
    fn test_enroll_pk_twice() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();

        let result = manager.enroll_pk(make_pk());
        assert!(matches!(result, Err(SecureBootError::AlreadyEnrolled)));
    }

    #[test]
    fn test_enroll_wrong_cert_type() {
        let manager = SecureBootManager::new();
        let kek = make_kek();

        let result = manager.enroll_pk(kek);
        assert!(matches!(
            result,
            Err(SecureBootError::InvalidCertificate(_))
        ));
    }

    #[test]
    fn test_add_kek() {
        let manager = SecureBootManager::new();
        manager.add_kek(make_kek()).unwrap();
        assert_eq!(manager.kek_count(), 1);
    }

    #[test]
    fn test_add_db_dbx() {
        let manager = SecureBootManager::new();

        manager.add_db(make_db_cert()).unwrap();
        assert_eq!(manager.db_count(), 1);

        manager.add_dbx(make_dbx_cert()).unwrap();
        assert_eq!(manager.dbx_count(), 1);
    }

    #[test]
    fn test_hash_allowlist() {
        let manager = SecureBootManager::new();

        let hash = vec![0xAA; 32];
        manager.allow_hash(hash.clone(), "Test hash");

        assert!(manager.is_hash_allowed(&hash));
        assert!(!manager.is_hash_allowed(&[0xBB; 32]));
    }

    #[test]
    fn test_hash_blocklist() {
        let manager = SecureBootManager::new();

        let hash = vec![0xCC; 32];
        manager.block_hash(hash.clone(), "Malware hash");

        assert!(manager.is_hash_blocked(&hash));
        assert!(!manager.is_hash_blocked(&[0xDD; 32]));
    }

    #[test]
    fn test_verify_disabled() {
        let manager = SecureBootManager::new();

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32]);

        assert_eq!(manager.verify(&component), VerificationResult::Success);
    }

    #[test]
    fn test_verify_blocked_hash() {
        let manager = SecureBootManager::new();
        manager.enable().unwrap();

        let hash = vec![0xEE; 32];
        manager.block_hash(hash.clone(), "Bad kernel");

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", hash);

        assert_eq!(manager.verify(&component), VerificationResult::Forbidden);
    }

    #[test]
    fn test_verify_allowed_hash() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.enable().unwrap();

        let hash = vec![0xFF; 32];
        manager.allow_hash(hash.clone(), "Known good kernel");

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", hash);

        assert_eq!(manager.verify(&component), VerificationResult::Success);
    }

    #[test]
    fn test_verify_no_signature() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.enable().unwrap();

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32]);

        assert_eq!(manager.verify(&component), VerificationResult::NoSignature);
    }

    #[test]
    fn test_verify_unsigned_driver_allowed() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.enable().unwrap();

        // Default policy allows unsigned drivers
        let component =
            BootComponent::new(BootComponentType::KernelModule, "test.ko", vec![0u8; 32]);

        assert_eq!(manager.verify(&component), VerificationResult::Success);
    }

    #[test]
    fn test_verify_unsigned_driver_not_allowed() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.enable().unwrap();

        let mut policy = manager.policy();
        policy.allow_unsigned_drivers = false;
        manager.set_policy(policy);

        let component =
            BootComponent::new(BootComponentType::KernelModule, "test.ko", vec![0u8; 32]);

        assert_eq!(manager.verify(&component), VerificationResult::NoSignature);
    }

    #[test]
    fn test_verify_revoked_signer() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.add_dbx(make_dbx_cert()).unwrap();
        manager.enable().unwrap();

        let signer = Certificate::new(
            CertificateType::Database,
            "Revoked Cert",
            "Test KEK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        );

        let signature =
            Signature::new(SignatureAlgorithm::RsaSha256, vec![0u8; 256]).with_signer(signer);

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32])
            .with_signature(signature);

        assert_eq!(
            manager.verify(&component),
            VerificationResult::CertificateRevoked
        );
    }

    #[test]
    fn test_verify_trusted_signer() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.add_db(make_db_cert()).unwrap();
        manager.enable().unwrap();

        let signer = Certificate::new(
            CertificateType::Database,
            "Test DB",
            "Test KEK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        );

        // Note what this signature is: 256 zero bytes, over a component whose
        // hash is also zeros, from a signer whose public key is zeros too.
        let signature =
            Signature::new(SignatureAlgorithm::RsaSha256, vec![0u8; 256]).with_signer(signer);

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32])
            .with_signature(signature);

        // This asserted `Success` until 2026-09-22, and it passed, because
        // `verify` matched the signer's subject string against the trusted
        // database and returned `Success` under a comment reading "Would
        // verify actual signature here". So the test did not merely miss the
        // bypass -- it pinned it as intended behaviour, and a all-zero
        // signature admitting a kernel was the documented expectation.
        //
        // Nothing about this component should admit it. Matching a subject
        // means it *claims* a trusted signer; the claim is a string the caller
        // wrote. Since 2026-09-28 the signature is checked, and 256 zero
        // bytes under a trusted entry whose key does not even parse are
        // `InvalidSignature` -- a verdict, where before there was none.
        assert_eq!(
            manager.verify(&component),
            VerificationResult::InvalidSignature,
            "zero bytes under a zero key must not be a pass"
        );
    }

    // ---- Real signatures, 2026-09-28 -------------------------------------

    use crate::crypto::asymmetric::{EcCurve, EcPrivateKey, RsaKeySize, RsaPrivateKey};
    use crate::crypto::{FipsCrypto, FipsMode};

    fn crypto() -> FipsCrypto {
        FipsCrypto::new(FipsMode::Disabled).unwrap()
    }

    fn rsa_spki(key: &RsaPrivateKey) -> Vec<u8> {
        let n = &key.public.n;
        let first = n.iter().position(|&b| b != 0).unwrap_or(n.len());
        let exponent = key
            .public
            .e
            .iter()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        let info = ic_pkix::PublicKeyInfo::Rsa {
            modulus: &n[first..],
            exponent,
        };
        let mut out = [0u8; 1024];
        let len = info.to_der(&mut out).unwrap();
        out[..len].to_vec()
    }

    fn ec_spki(key: &EcPrivateKey) -> Vec<u8> {
        let mut point = vec![0x04];
        point.extend_from_slice(&key.public.x);
        point.extend_from_slice(&key.public.y);
        let algorithm = match key.public.curve {
            EcCurve::P256 => ic_pkix::KeyAlgorithm::EcP256,
            EcCurve::P384 => ic_pkix::KeyAlgorithm::EcP384,
            EcCurve::P521 => unreachable!("secure boot offers P-256 and P-384"),
        };
        let info = ic_pkix::PublicKeyInfo::Ec {
            algorithm,
            point: &point,
        };
        let mut out = [0u8; 256];
        let len = info.to_der(&mut out).unwrap();
        out[..len].to_vec()
    }

    fn trusting(subject: &str, spki: Vec<u8>, algorithm: SignatureAlgorithm) -> SecureBootManager {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager
            .add_db(Certificate::new(
                CertificateType::Database,
                subject,
                "Test KEK",
                spki,
                algorithm,
            ))
            .unwrap();
        manager.enable().unwrap();
        manager
    }

    /// A claim of `subject`, carrying whatever key the component's builder
    /// chose. Verification must never use that key.
    fn claiming(
        subject: &str,
        key_it_carries: Vec<u8>,
        algorithm: SignatureAlgorithm,
    ) -> Certificate {
        Certificate::new(
            CertificateType::Database,
            subject,
            "?",
            key_it_carries,
            algorithm,
        )
    }

    fn kernel(hash: u8) -> BootComponent {
        BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![hash; 32])
    }

    fn rsa_sign(key: &RsaPrivateKey, component: &BootComponent) -> Vec<u8> {
        crypto()
            .rsa_sign(
                key,
                &component.signed_message(),
                crate::crypto::asymmetric::SignatureAlgorithm::RsaPkcs1Sha256,
            )
            .unwrap()
            .data
    }

    #[test]
    fn a_valid_rsa_signature_from_a_trusted_key_is_admitted() {
        let key = crypto().generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        let manager = trusting("Vendor", rsa_spki(&key), SignatureAlgorithm::RsaSha256);
        let component = kernel(7);
        let sig = Signature::new(SignatureAlgorithm::RsaSha256, rsa_sign(&key, &component))
            .with_signer(claiming("Vendor", vec![], SignatureAlgorithm::RsaSha256));
        assert_eq!(
            manager.verify(&component.with_signature(sig)),
            VerificationResult::Success
        );
    }

    #[test]
    fn valid_ecdsa_signatures_on_p256_and_p384_are_admitted() {
        for (curve, algorithm, scheme) in [
            (
                EcCurve::P256,
                SignatureAlgorithm::EcdsaSha256,
                crate::crypto::asymmetric::SignatureAlgorithm::EcdsaP256Sha256,
            ),
            (
                EcCurve::P384,
                SignatureAlgorithm::EcdsaSha384,
                crate::crypto::asymmetric::SignatureAlgorithm::EcdsaP384Sha384,
            ),
        ] {
            let key = crypto().generate_ecdsa_keypair(curve).unwrap();
            let manager = trusting("Vendor", ec_spki(&key), algorithm);
            let component = kernel(9);
            let signed = crypto()
                .ecdsa_sign(&key, &component.signed_message())
                .unwrap();
            assert_eq!(signed.algorithm, scheme);
            let sig = Signature::new(algorithm, signed.data).with_signer(claiming(
                "Vendor",
                vec![],
                algorithm,
            ));
            assert_eq!(
                manager.verify(&component.with_signature(sig)),
                VerificationResult::Success,
                "{curve:?}"
            );
        }
    }

    /// The plan's exit criterion: a forgery whose claimed signer matches a
    /// trusted subject is refused. The forger signs with their own key and
    /// attaches a certificate carrying *that* key under the trusted name,
    /// which a verifier that used the attached key would accept.
    #[test]
    fn a_forgery_under_a_trusted_name_carrying_its_own_key_is_refused() {
        let vendor = crypto().generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        let forger = crypto().generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        let manager = trusting("Vendor", rsa_spki(&vendor), SignatureAlgorithm::RsaSha256);
        let component = kernel(7);
        let sig = Signature::new(SignatureAlgorithm::RsaSha256, rsa_sign(&forger, &component))
            .with_signer(claiming(
                "Vendor",
                rsa_spki(&forger),
                SignatureAlgorithm::RsaSha256,
            ));
        assert_eq!(
            manager.verify(&component.with_signature(sig)),
            VerificationResult::InvalidSignature
        );
    }

    #[test]
    fn a_signature_does_not_transfer_to_another_digest_or_component_type() {
        let key = crypto().generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        let manager = trusting("Vendor", rsa_spki(&key), SignatureAlgorithm::RsaSha256);
        let signed_for = kernel(7);
        let sig = Signature::new(SignatureAlgorithm::RsaSha256, rsa_sign(&key, &signed_for))
            .with_signer(claiming("Vendor", vec![], SignatureAlgorithm::RsaSha256));

        // A different image.
        assert_eq!(
            manager.verify(&kernel(8).with_signature(sig.clone())),
            VerificationResult::InvalidSignature
        );
        // The same image, presented as a module: the type byte differs.
        let module = BootComponent::new(BootComponentType::KernelModule, "m.ko", vec![7; 32]);
        assert_eq!(
            manager.verify(&module.with_signature(sig)),
            VerificationResult::InvalidSignature
        );
    }

    #[test]
    fn an_expired_trusted_certificate_does_not_verify() {
        let key = crypto().generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        let mut cert = Certificate::new(
            CertificateType::Database,
            "Vendor",
            "Test KEK",
            rsa_spki(&key),
            SignatureAlgorithm::RsaSha256,
        );
        cert.not_after = 1; // 1970.
        manager.add_db(cert).unwrap();
        manager.enable().unwrap();
        let component = kernel(7);
        let sig = Signature::new(SignatureAlgorithm::RsaSha256, rsa_sign(&key, &component))
            .with_signer(claiming("Vendor", vec![], SignatureAlgorithm::RsaSha256));
        assert_eq!(
            manager.verify(&component.with_signature(sig)),
            VerificationResult::CertificateExpired
        );
    }

    #[test]
    fn a_trusted_key_listed_in_dbx_under_another_name_is_revoked() {
        let key = crypto().generate_rsa_keypair(RsaKeySize::Rsa2048).unwrap();
        let manager = trusting("Vendor", rsa_spki(&key), SignatureAlgorithm::RsaSha256);
        manager
            .add_dbx(Certificate::new(
                CertificateType::ForbiddenDatabase,
                "Leaked key",
                "Test KEK",
                rsa_spki(&key),
                SignatureAlgorithm::RsaSha256,
            ))
            .unwrap();
        let component = kernel(7);
        let sig = Signature::new(SignatureAlgorithm::RsaSha256, rsa_sign(&key, &component))
            .with_signer(claiming("Vendor", vec![], SignatureAlgorithm::RsaSha256));
        assert_eq!(
            manager.verify(&component.with_signature(sig)),
            VerificationResult::CertificateRevoked
        );
    }

    /// A signer nobody trusts is still refused, and for the earlier reason.
    ///
    /// Worth separating from the case above so the two refusals cannot be
    /// confused: this one is a verdict about trust, that one is the absence of
    /// a verdict about the signature.
    #[test]
    fn an_untrusted_signer_is_refused_before_the_signature_matters() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.add_db(make_db_cert()).unwrap();
        manager.enable().unwrap();

        let stranger = Certificate::new(
            CertificateType::Database,
            "Someone Else",
            "Unknown KEK",
            vec![0u8; 256],
            SignatureAlgorithm::RsaSha256,
        );
        let signature =
            Signature::new(SignatureAlgorithm::RsaSha256, vec![0u8; 256]).with_signer(stranger);
        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32])
            .with_signature(signature);

        assert_eq!(
            manager.verify(&component),
            VerificationResult::UntrustedCertificate
        );
    }

    #[test]
    fn test_stats() {
        let manager = SecureBootManager::new();
        manager.enroll_pk(make_pk()).unwrap();
        manager.add_kek(make_kek()).unwrap();
        manager.add_db(make_db_cert()).unwrap();
        manager.enable().unwrap();

        let stats = manager.stats();
        assert!(stats.enabled);
        assert!(stats.pk_enrolled);
        assert_eq!(stats.kek_count, 1);
        assert_eq!(stats.db_count, 1);
    }

    #[test]
    fn test_verification_count() {
        let manager = SecureBootManager::new();

        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32]);

        manager.verify(&component);
        manager.verify(&component);

        let stats = manager.stats();
        assert_eq!(stats.verification_count, 2);
    }

    #[test]
    fn test_boot_component_builder() {
        let component = BootComponent::new(BootComponentType::Kernel, "vmlinuz", vec![0u8; 32])
            .with_size(1024 * 1024)
            .with_load_address(0x100000);

        assert_eq!(component.size, 1024 * 1024);
        assert_eq!(component.load_address, 0x100000);
    }

    #[test]
    fn test_disable_secure_boot() {
        let manager = SecureBootManager::new();
        manager.enable().unwrap();
        assert!(manager.is_enabled());

        manager.disable();
        assert!(!manager.is_enabled());
        assert_eq!(manager.mode(), SecureBootMode::Disabled);
    }

    #[test]
    fn test_default_policy() {
        let policy = SecureBootPolicy::default();
        assert_eq!(policy.mode, SecureBootMode::Disabled);
        assert!(policy.allow_unsigned_drivers);
        assert!(policy.allow_unsigned_option_roms);
    }
}
