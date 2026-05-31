//! Image Allowlist Registry
//!
//! Fleet-level registry for approved VM images, container images, and
//! runtime artifacts. Enforces allowlists and denylists so that only
//! verified, signed images can be used to launch workloads. Complements
//! Secure Boot (which validates the boot chain) with policy-level
//! control over what images are permitted in the fleet.

use std::collections::HashMap;
use std::time::SystemTime;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Image registry result
pub type RegistryResult<T> = Result<T, RegistryError>;

/// Image registry errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// Image not found
    #[error("Image not found: {0}")]
    NotFound(String),

    /// Image is on the denylist
    #[error("Image denied by policy: {0}")]
    Denied(String),

    /// Image not on the allowlist
    #[error("Image not on allowlist: {0}")]
    NotAllowed(String),

    /// Signature verification failed
    #[error("Signature verification failed for {image}: {reason}")]
    SignatureInvalid { image: String, reason: String },

    /// Duplicate entry
    #[error("Duplicate image: {0}")]
    Duplicate(String),

    /// Invalid configuration
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
}

/// Kind of image being registered
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageKind {
    /// VM base image (rootfs / disk image)
    VmImage,
    /// Container image (OCI)
    Container,
    /// Kernel image
    Kernel,
    /// Initramfs / initrd
    Initramfs,
    /// UEFI firmware
    Firmware,
    /// Custom artifact type
    Custom(String),
}

/// Approval status of an image
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApprovalStatus {
    /// Approved for use
    Approved,
    /// Pending review
    PendingReview,
    /// Denied (on denylist)
    Denied,
    /// Deprecated — allowed but warns
    Deprecated,
    /// Revoked — was approved, now blocked
    Revoked,
}

/// Signature information for an image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSignature {
    /// Signer identity (e.g., key ID, certificate CN)
    pub signer: String,
    /// Signature algorithm
    pub algorithm: String,
    /// Signature bytes (hex-encoded)
    pub signature_hex: String,
    /// When the signature was created
    pub signed_at: SystemTime,
    /// Whether the signature has been verified
    pub verified: bool,
}

/// A registered image in the fleet registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEntry {
    /// Image reference (e.g., "registry.internal/vm/ubuntu-22.04:v3")
    pub reference: String,
    /// Image kind
    pub kind: ImageKind,
    /// SHA-256 digest
    pub sha256: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Approval status
    pub status: ApprovalStatus,
    /// Signatures
    pub signatures: Vec<ImageSignature>,
    /// Tags / labels (e.g., "os=ubuntu", "gpu=cuda-12.4")
    pub labels: HashMap<String, String>,
    /// Who approved/denied this image
    pub reviewed_by: Option<String>,
    /// When the image was registered
    pub registered_at: SystemTime,
    /// When the status was last updated
    pub status_updated_at: SystemTime,
    /// Free-text notes (e.g., CVE remediation)
    pub notes: String,
}

impl ImageEntry {
    /// Create a new image entry in PendingReview status
    pub fn new(reference: impl Into<String>, kind: ImageKind, sha256: impl Into<String>) -> Self {
        let now = SystemTime::now();
        Self {
            reference: reference.into(),
            kind,
            sha256: sha256.into(),
            size_bytes: 0,
            status: ApprovalStatus::PendingReview,
            signatures: Vec::new(),
            labels: HashMap::new(),
            reviewed_by: None,
            registered_at: now,
            status_updated_at: now,
            notes: String::new(),
        }
    }

    /// Builder: set size
    pub fn size(mut self, bytes: u64) -> Self {
        self.size_bytes = bytes;
        self
    }

    /// Builder: add a label
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Builder: add a signature
    pub fn signature(mut self, sig: ImageSignature) -> Self {
        self.signatures.push(sig);
        self
    }

    /// Whether the image has at least one verified signature
    pub fn has_verified_signature(&self) -> bool {
        self.signatures.iter().any(|s| s.verified)
    }
}

/// Enforcement mode for the registry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnforcementMode {
    /// Log violations but don't block (audit mode)
    Audit,
    /// Block images not on the allowlist
    Enforce,
    /// Disabled — all images permitted
    Disabled,
}

/// Registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Enforcement mode
    pub mode: EnforcementMode,
    /// Require at least one verified signature for approval
    pub require_signature: bool,
    /// Trusted signer identities (if empty, any signer is trusted)
    pub trusted_signers: Vec<String>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            mode: EnforcementMode::Enforce,
            require_signature: true,
            trusted_signers: Vec::new(),
        }
    }
}

/// Admission decision returned by `check_admission`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionDecision {
    /// Image is allowed
    Allowed,
    /// Image is allowed but deprecated (warning)
    AllowedWithWarning(String),
    /// Image is denied
    Denied(String),
}

/// Image allowlist registry
///
/// Central registry for the fleet that tracks which images are approved
/// for use. Provides admission checks that the scheduler and VM
/// provisioning path call before launching workloads.
pub struct ImageRegistry {
    /// Configuration
    config: RegistryConfig,
    /// Image catalog keyed by reference
    images: RwLock<HashMap<String, ImageEntry>>,
}

impl ImageRegistry {
    /// Create a new image registry
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            config,
            images: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new image
    pub fn register(&self, entry: ImageEntry) -> RegistryResult<()> {
        let mut images = self.images.write();
        if images.contains_key(&entry.reference) {
            return Err(RegistryError::Duplicate(entry.reference.clone()));
        }
        images.insert(entry.reference.clone(), entry);
        Ok(())
    }

    /// Approve an image (set status to Approved)
    pub fn approve(&self, reference: &str, reviewer: &str) -> RegistryResult<()> {
        let mut images = self.images.write();
        let entry = images
            .get_mut(reference)
            .ok_or_else(|| RegistryError::NotFound(reference.to_string()))?;

        if self.config.require_signature && !entry.has_verified_signature() {
            return Err(RegistryError::SignatureInvalid {
                image: reference.to_string(),
                reason: "No verified signature".to_string(),
            });
        }

        entry.status = ApprovalStatus::Approved;
        entry.reviewed_by = Some(reviewer.to_string());
        entry.status_updated_at = SystemTime::now();
        Ok(())
    }

    /// Deny an image (add to denylist)
    pub fn deny(&self, reference: &str, reviewer: &str, reason: &str) -> RegistryResult<()> {
        let mut images = self.images.write();
        let entry = images
            .get_mut(reference)
            .ok_or_else(|| RegistryError::NotFound(reference.to_string()))?;

        entry.status = ApprovalStatus::Denied;
        entry.reviewed_by = Some(reviewer.to_string());
        entry.notes = reason.to_string();
        entry.status_updated_at = SystemTime::now();
        Ok(())
    }

    /// Revoke a previously approved image
    pub fn revoke(&self, reference: &str, reviewer: &str, reason: &str) -> RegistryResult<()> {
        let mut images = self.images.write();
        let entry = images
            .get_mut(reference)
            .ok_or_else(|| RegistryError::NotFound(reference.to_string()))?;

        entry.status = ApprovalStatus::Revoked;
        entry.reviewed_by = Some(reviewer.to_string());
        entry.notes = reason.to_string();
        entry.status_updated_at = SystemTime::now();
        Ok(())
    }

    /// Deprecate an image
    pub fn deprecate(&self, reference: &str, reviewer: &str, reason: &str) -> RegistryResult<()> {
        let mut images = self.images.write();
        let entry = images
            .get_mut(reference)
            .ok_or_else(|| RegistryError::NotFound(reference.to_string()))?;

        entry.status = ApprovalStatus::Deprecated;
        entry.reviewed_by = Some(reviewer.to_string());
        entry.notes = reason.to_string();
        entry.status_updated_at = SystemTime::now();
        Ok(())
    }

    /// Check whether an image is admitted for use
    ///
    /// This is the primary admission gate — called before launching
    /// a VM or scheduling a workload.
    pub fn check_admission(&self, reference: &str) -> AdmissionDecision {
        if self.config.mode == EnforcementMode::Disabled {
            return AdmissionDecision::Allowed;
        }

        let images = self.images.read();
        let entry = match images.get(reference) {
            Some(e) => e,
            None => {
                return if self.config.mode == EnforcementMode::Enforce {
                    AdmissionDecision::Denied(format!("Image not in registry: {reference}"))
                } else {
                    AdmissionDecision::Allowed
                };
            }
        };

        match entry.status {
            ApprovalStatus::Approved => {
                // Verify signature trust if configured
                if self.config.require_signature && !entry.has_verified_signature() {
                    return AdmissionDecision::Denied(format!(
                        "No verified signature for {reference}"
                    ));
                }
                if !self.config.trusted_signers.is_empty() {
                    let trusted = entry
                        .signatures
                        .iter()
                        .any(|s| s.verified && self.config.trusted_signers.contains(&s.signer));
                    if !trusted {
                        return AdmissionDecision::Denied(format!(
                            "No trusted signer for {reference}"
                        ));
                    }
                }
                AdmissionDecision::Allowed
            }
            ApprovalStatus::Deprecated => AdmissionDecision::AllowedWithWarning(format!(
                "Image {reference} is deprecated: {}",
                entry.notes
            )),
            ApprovalStatus::PendingReview => {
                if self.config.mode == EnforcementMode::Enforce {
                    AdmissionDecision::Denied(format!("Image pending review: {reference}"))
                } else {
                    AdmissionDecision::Allowed
                }
            }
            ApprovalStatus::Denied => {
                AdmissionDecision::Denied(format!("Image denied: {reference} — {}", entry.notes))
            }
            ApprovalStatus::Revoked => {
                AdmissionDecision::Denied(format!("Image revoked: {reference} — {}", entry.notes))
            }
        }
    }

    /// Get an image entry
    pub fn get(&self, reference: &str) -> Option<ImageEntry> {
        self.images.read().get(reference).cloned()
    }

    /// List all images with a given status
    pub fn list_by_status(&self, status: ApprovalStatus) -> Vec<ImageEntry> {
        self.images
            .read()
            .values()
            .filter(|i| i.status == status)
            .cloned()
            .collect()
    }

    /// List all images of a given kind
    pub fn list_by_kind(&self, kind: &ImageKind) -> Vec<ImageEntry> {
        self.images
            .read()
            .values()
            .filter(|i| i.kind == *kind)
            .cloned()
            .collect()
    }

    /// Total image count
    pub fn image_count(&self) -> usize {
        self.images.read().len()
    }

    /// Get registry configuration
    pub fn config(&self) -> &RegistryConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_sig() -> ImageSignature {
        ImageSignature {
            signer: "build-system".to_string(),
            algorithm: "ecdsa-sha256".to_string(),
            signature_hex: "deadbeef".to_string(),
            signed_at: SystemTime::now(),
            verified: true,
        }
    }

    fn test_registry() -> ImageRegistry {
        ImageRegistry::new(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: true,
            trusted_signers: vec![],
        })
    }

    #[test]
    fn test_register_and_get() {
        let reg = test_registry();
        let entry = ImageEntry::new("ubuntu:22.04", ImageKind::VmImage, "sha256:abc123")
            .label("os", "ubuntu")
            .signature(verified_sig());
        reg.register(entry).unwrap();

        let got = reg.get("ubuntu:22.04").unwrap();
        assert_eq!(got.sha256, "sha256:abc123");
        assert_eq!(got.status, ApprovalStatus::PendingReview);
    }

    #[test]
    fn test_duplicate_rejected() {
        let reg = test_registry();
        reg.register(ImageEntry::new("img:1", ImageKind::VmImage, "sha1"))
            .unwrap();
        let err = reg
            .register(ImageEntry::new("img:1", ImageKind::VmImage, "sha2"))
            .unwrap_err();
        assert!(matches!(err, RegistryError::Duplicate(_)));
    }

    #[test]
    fn test_approve_and_admit() {
        let reg = test_registry();
        reg.register(
            ImageEntry::new("img:1", ImageKind::VmImage, "sha1").signature(verified_sig()),
        )
        .unwrap();

        reg.approve("img:1", "admin").unwrap();
        assert_eq!(reg.check_admission("img:1"), AdmissionDecision::Allowed);
    }

    #[test]
    fn test_approve_without_signature_fails() {
        let reg = test_registry();
        reg.register(ImageEntry::new("img:1", ImageKind::VmImage, "sha1"))
            .unwrap();

        let err = reg.approve("img:1", "admin").unwrap_err();
        assert!(matches!(err, RegistryError::SignatureInvalid { .. }));
    }

    #[test]
    fn test_deny_blocks_admission() {
        let reg = test_registry();
        reg.register(ImageEntry::new("bad:1", ImageKind::Container, "sha1"))
            .unwrap();
        reg.deny("bad:1", "admin", "Contains CVE-2024-1234")
            .unwrap();

        match reg.check_admission("bad:1") {
            AdmissionDecision::Denied(msg) => assert!(msg.contains("denied")),
            other => panic!("Expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn test_revoke_blocks_admission() {
        let reg = test_registry();
        reg.register(
            ImageEntry::new("img:1", ImageKind::VmImage, "sha1").signature(verified_sig()),
        )
        .unwrap();
        reg.approve("img:1", "admin").unwrap();
        reg.revoke("img:1", "admin", "Supply chain compromise")
            .unwrap();

        match reg.check_admission("img:1") {
            AdmissionDecision::Denied(msg) => assert!(msg.contains("revoked")),
            other => panic!("Expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn test_deprecated_warns() {
        let reg = test_registry();
        reg.register(
            ImageEntry::new("old:1", ImageKind::VmImage, "sha1").signature(verified_sig()),
        )
        .unwrap();
        reg.approve("old:1", "admin").unwrap();
        reg.deprecate("old:1", "admin", "Use new:1 instead")
            .unwrap();

        match reg.check_admission("old:1") {
            AdmissionDecision::AllowedWithWarning(msg) => {
                assert!(msg.contains("deprecated"));
            }
            other => panic!("Expected AllowedWithWarning, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_image_denied_in_enforce_mode() {
        let reg = test_registry();
        match reg.check_admission("unknown:latest") {
            AdmissionDecision::Denied(msg) => assert!(msg.contains("not in registry")),
            other => panic!("Expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_image_allowed_in_audit_mode() {
        let reg = ImageRegistry::new(RegistryConfig {
            mode: EnforcementMode::Audit,
            require_signature: false,
            trusted_signers: vec![],
        });
        assert_eq!(
            reg.check_admission("unknown:latest"),
            AdmissionDecision::Allowed
        );
    }

    #[test]
    fn test_disabled_mode() {
        let reg = ImageRegistry::new(RegistryConfig {
            mode: EnforcementMode::Disabled,
            require_signature: true,
            trusted_signers: vec![],
        });
        assert_eq!(reg.check_admission("anything"), AdmissionDecision::Allowed);
    }

    #[test]
    fn test_trusted_signer_enforcement() {
        let reg = ImageRegistry::new(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: true,
            trusted_signers: vec!["release-key".to_string()],
        });

        // Image signed by untrusted signer
        reg.register(
            ImageEntry::new("img:1", ImageKind::VmImage, "sha1").signature(verified_sig()), // signer = "build-system"
        )
        .unwrap();
        reg.approve("img:1", "admin").unwrap();

        match reg.check_admission("img:1") {
            AdmissionDecision::Denied(msg) => assert!(msg.contains("trusted signer")),
            other => panic!("Expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn test_trusted_signer_allowed() {
        let reg = ImageRegistry::new(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: true,
            trusted_signers: vec!["build-system".to_string()],
        });

        reg.register(
            ImageEntry::new("img:1", ImageKind::VmImage, "sha1").signature(verified_sig()),
        )
        .unwrap();
        reg.approve("img:1", "admin").unwrap();

        assert_eq!(reg.check_admission("img:1"), AdmissionDecision::Allowed);
    }

    #[test]
    fn test_list_by_status() {
        let reg = ImageRegistry::new(RegistryConfig {
            mode: EnforcementMode::Enforce,
            require_signature: false,
            trusted_signers: vec![],
        });
        reg.register(ImageEntry::new("a:1", ImageKind::VmImage, "s1"))
            .unwrap();
        reg.register(ImageEntry::new("b:1", ImageKind::VmImage, "s2"))
            .unwrap();
        reg.approve("a:1", "admin").unwrap();

        assert_eq!(reg.list_by_status(ApprovalStatus::Approved).len(), 1);
        assert_eq!(reg.list_by_status(ApprovalStatus::PendingReview).len(), 1);
    }

    #[test]
    fn test_list_by_kind() {
        let reg = test_registry();
        reg.register(ImageEntry::new("vm:1", ImageKind::VmImage, "s1"))
            .unwrap();
        reg.register(ImageEntry::new("c:1", ImageKind::Container, "s2"))
            .unwrap();

        assert_eq!(reg.list_by_kind(&ImageKind::VmImage).len(), 1);
        assert_eq!(reg.list_by_kind(&ImageKind::Container).len(), 1);
        assert_eq!(reg.list_by_kind(&ImageKind::Kernel).len(), 0);
    }

    #[test]
    fn test_image_count() {
        let reg = test_registry();
        assert_eq!(reg.image_count(), 0);
        reg.register(ImageEntry::new("a:1", ImageKind::VmImage, "s1"))
            .unwrap();
        assert_eq!(reg.image_count(), 1);
    }

    #[test]
    fn test_pending_review_denied_in_enforce() {
        let reg = test_registry();
        reg.register(ImageEntry::new("new:1", ImageKind::VmImage, "sha1"))
            .unwrap();

        match reg.check_admission("new:1") {
            AdmissionDecision::Denied(msg) => assert!(msg.contains("pending review")),
            other => panic!("Expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn test_not_found_errors() {
        let reg = test_registry();
        assert!(matches!(
            reg.approve("nope", "admin").unwrap_err(),
            RegistryError::NotFound(_)
        ));
        assert!(matches!(
            reg.deny("nope", "admin", "bad").unwrap_err(),
            RegistryError::NotFound(_)
        ));
        assert!(matches!(
            reg.revoke("nope", "admin", "bad").unwrap_err(),
            RegistryError::NotFound(_)
        ));
    }
}
