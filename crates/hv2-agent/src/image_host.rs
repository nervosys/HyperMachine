//! The seam between the MCP tool surface and an image allowlist.
//!
//! Image admission is enforced at `VM::provision`: a VM refuses to start if the
//! registry does not admit its boot image. That is the right place for the
//! *decision*, but it leaves an agent finding out by failure. An agent that can
//! ask which images are approved — before composing a plan around one — makes
//! better choices than one that discovers the answer at boot time.
//!
//! [`ImageHost`] is that query surface. It is deliberately read-mostly: an
//! agent may list images and ask whether one would be admitted, but approving
//! an image is a human review step and is not exposed here.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use hv2_agent::mcp::McpServer;
//! use hv2_agent::image_host::RegistryImageHost;
//! use hv2_core::security::image_registry::{ImageRegistry, RegistryConfig};
//!
//! let registry = Arc::new(ImageRegistry::new(RegistryConfig::default()));
//! let server = McpServer::new();
//! server.set_image_host(Arc::new(RegistryImageHost::new(registry)));
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use hv2_core::security::image_registry::{AdmissionDecision, ApprovalStatus, ImageRegistry};

/// An image as the tool surface reports it to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageDescriptor {
    /// Registry reference, e.g. `registry.internal/kernels/ubuntu:6.8`.
    pub reference: String,
    /// What kind of artifact this is (`kernel`, `initrd`, `disk`, ...).
    pub kind: String,
    /// SHA-256 digest. This, not the reference, is what admission matches on.
    pub sha256: String,
    /// `approved`, `pending_review`, `denied`, `revoked`, or `deprecated`.
    pub status: String,
    /// Free-text review notes, e.g. the CVE a revocation cites.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// What the registry says about booting a particular image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionVerdict {
    /// Whether a VM would be allowed to boot this image.
    pub admitted: bool,
    /// Why, in both directions — a deprecation warning is an admission with a
    /// reason, and an agent should be able to surface that rather than treat
    /// approved and deprecated as identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Something that can answer which images are allowed to boot.
#[async_trait]
pub trait ImageHost: Send + Sync {
    /// Every image the registry knows about.
    async fn list(&self) -> Result<Vec<ImageDescriptor>, String>;

    /// One image by its registry reference.
    async fn get(&self, reference: &str) -> Result<ImageDescriptor, String>;

    /// Whether an image with this SHA-256 digest would be admitted.
    ///
    /// Keyed on digest rather than reference because that is what
    /// `VM::provision` checks: renaming a kernel must not change the answer,
    /// and an agent asking this question should get the same verdict the boot
    /// path would give it.
    async fn check_digest(&self, digest: &str) -> Result<AdmissionVerdict, String>;
}

// ═══════════════════════════════════════════════════════════════════
//  Registry-backed host
// ═══════════════════════════════════════════════════════════════════

/// An [`ImageHost`] over a real [`ImageRegistry`].
///
/// Share the same registry the API server and `VM::provision` use, so an
/// agent's answer and the boot path's decision cannot disagree.
pub struct RegistryImageHost {
    registry: Arc<ImageRegistry>,
}

impl RegistryImageHost {
    /// Wrap an existing registry.
    pub fn new(registry: Arc<ImageRegistry>) -> Self {
        Self { registry }
    }

    fn describe(entry: &hv2_core::security::image_registry::ImageEntry) -> ImageDescriptor {
        ImageDescriptor {
            reference: entry.reference.clone(),
            kind: format!("{:?}", entry.kind).to_lowercase(),
            sha256: entry.sha256.clone(),
            status: match entry.status {
                ApprovalStatus::Approved => "approved",
                ApprovalStatus::PendingReview => "pending_review",
                ApprovalStatus::Denied => "denied",
                ApprovalStatus::Revoked => "revoked",
                ApprovalStatus::Deprecated => "deprecated",
            }
            .to_string(),
            notes: entry.notes.clone(),
        }
    }
}

#[async_trait]
impl ImageHost for RegistryImageHost {
    async fn list(&self) -> Result<Vec<ImageDescriptor>, String> {
        let mut images: Vec<ImageDescriptor> = [
            ApprovalStatus::Approved,
            ApprovalStatus::PendingReview,
            ApprovalStatus::Denied,
            ApprovalStatus::Revoked,
            ApprovalStatus::Deprecated,
        ]
        .into_iter()
        .flat_map(|status| self.registry.list_by_status(status))
        .map(|entry| Self::describe(&entry))
        .collect();

        // Stable order: the registry is a HashMap, and an agent diffing two
        // listings should not see spurious changes from hash iteration.
        images.sort_by(|a, b| a.reference.cmp(&b.reference));
        Ok(images)
    }

    async fn get(&self, reference: &str) -> Result<ImageDescriptor, String> {
        self.registry
            .get(reference)
            .map(|entry| Self::describe(&entry))
            .ok_or_else(|| format!("Image not found: {reference}"))
    }

    async fn check_digest(&self, digest: &str) -> Result<AdmissionVerdict, String> {
        Ok(match self.registry.check_admission_by_digest(digest) {
            AdmissionDecision::Allowed => AdmissionVerdict {
                admitted: true,
                reason: None,
            },
            AdmissionDecision::AllowedWithWarning(warning) => AdmissionVerdict {
                admitted: true,
                reason: Some(warning),
            },
            AdmissionDecision::Denied(reason) => AdmissionVerdict {
                admitted: false,
                reason: Some(reason),
            },
        })
    }
}
