//! Image admission control on the provisioning path.
//!
//! The image registry has always been able to *answer* whether an image is
//! approved; nothing on the boot path asked it, so denying or revoking an image
//! did not stop a VM from booting it. These tests pin the enforcement point:
//! `VM::provision` refuses a boot image the installed registry rejects, and
//! identifies it by the digest of the bytes about to be loaded rather than by
//! the path they came from.

use std::sync::Arc;

use async_trait::async_trait;
use hv2_core::boot::source::BootSource;
use hv2_core::hypervisor::{
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm,
};
use hv2_core::security::image_registry::{
    EnforcementMode, ImageEntry, ImageKind, ImageRegistry, RegistryConfig,
};
use hv2_core::{Result, VCpu, VMConfig, VmExit, VM};

// ═══════════════════════════════════════════════════════════════════
//  A backend that does nothing but succeed
// ═══════════════════════════════════════════════════════════════════

struct StubBackend;

#[async_trait]
impl HypervisorBackend for StubBackend {
    fn platform(&self) -> HypervisorPlatform {
        HypervisorPlatform::Tcg
    }

    fn capabilities(&self) -> HypervisorCapabilities {
        HypervisorCapabilities {
            max_vcpus: 64,
            max_memory: 16 * 1024 * 1024 * 1024,
            supports_nested_virt: false,
            supports_apic: true,
            supports_x2apic: false,
            supports_iommu: false,
            supports_gpu_passthrough: false,
        }
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn create_vm(&self, vcpu_count: u32, memory_size: u64) -> Result<HypervisorVm> {
        Ok(HypervisorVm::new(
            HypervisorPlatform::Tcg,
            vcpu_count,
            memory_size,
        ))
    }

    async fn load_boot(
        &self,
        _vcpu: &VCpu,
        _boot: &hv2_core::boot::source::LoadedBoot,
    ) -> Result<()> {
        Ok(())
    }

    async fn run_vcpu(&self, _vcpu: &VCpu) -> Result<VmExit> {
        Ok(VmExit::Shutdown)
    }

    async fn inject_interrupt(&self, _vcpu: &VCpu, _vector: u8) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

/// A bzImage header the Linux boot protocol accepts, salted so different tests
/// produce different digests.
fn bzimage(salt: u8) -> Vec<u8> {
    let mut image = vec![0u8; 8192];
    image[0x1F1] = 4; // setup_sects
    image[0x1FE] = 0x55;
    image[0x1FF] = 0xAA;
    image[0x202..0x206].copy_from_slice(b"HdrS");
    image[0x206] = 0x0C; // protocol 2.12
    image[0x207] = 0x02;
    image[0x1000] = salt;
    image
}

fn temp_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("hv2-admission-{name}"));
    std::fs::write(&path, bytes).expect("write temp boot image");
    path
}

fn vm_booting(name: &str, kernel: std::path::PathBuf) -> Arc<VM> {
    let config = VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        parallel_vcpu: false,
        boot: Some(BootSource::linux(kernel)),
        ..VMConfig::default()
    };
    Arc::new(VM::new_with_backend(config, Arc::new(StubBackend)).expect("VM::new_with_backend"))
}

/// The digest `provision` will compute for this kernel — the same value the
/// registry has to hold for the image to be admitted.
fn digest_of(kernel: &std::path::Path) -> String {
    BootSource::linux(kernel)
        .load()
        .expect("load")
        .primary_image_digest()
        .expect("digest")
}

fn registry(mode: EnforcementMode) -> ImageRegistry {
    ImageRegistry::new(RegistryConfig {
        mode,
        require_signature: false,
        trusted_signers: Vec::new(),
    })
}

fn approved(reference: &str, digest: &str) -> ImageEntry {
    ImageEntry::new(reference, ImageKind::Kernel, digest)
}

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn without_a_registry_any_image_boots() {
    // The default has to stay what it was: no registry, no gate.
    let kernel = temp_image("ungated", &bzimage(1));
    let vm = vm_booting("ungated", kernel);

    assert!(vm.image_registry().is_none());
    vm.provision().await.expect("no registry installed");
}

#[tokio::test]
async fn an_approved_digest_is_admitted() {
    let kernel = temp_image("approved", &bzimage(2));
    let digest = digest_of(&kernel);

    let reg = registry(EnforcementMode::Enforce);
    reg.register(approved("internal/kernels/good:1", &digest))
        .unwrap();
    reg.approve("internal/kernels/good:1", "reviewer").unwrap();

    let vm = vm_booting("approved", kernel);
    vm.set_image_registry(Arc::new(reg));

    vm.provision().await.expect("an approved image must boot");
}

#[tokio::test]
async fn a_revoked_image_cannot_provision() {
    // The point of the whole exercise: revoking an image has to stop it
    // booting, not merely be reportable.
    let kernel = temp_image("revoked", &bzimage(3));
    let digest = digest_of(&kernel);

    let reg = registry(EnforcementMode::Enforce);
    reg.register(approved("internal/kernels/bad:1", &digest))
        .unwrap();
    reg.approve("internal/kernels/bad:1", "reviewer").unwrap();
    reg.revoke("internal/kernels/bad:1", "reviewer", "CVE-2026-0001")
        .unwrap();

    let vm = vm_booting("revoked", kernel);
    vm.set_image_registry(Arc::new(reg));

    let err = vm
        .provision()
        .await
        .expect_err("a revoked image must not boot");
    let message = err.to_string();
    assert!(message.contains("revoked"), "got: {message}");
    assert!(
        !vm.is_provisioned(),
        "a refused VM must not be left provisioned"
    );
}

#[tokio::test]
async fn an_unregistered_image_is_denied_under_enforce() {
    let kernel = temp_image("stranger", &bzimage(4));
    let vm = vm_booting("stranger", kernel);
    vm.set_image_registry(Arc::new(registry(EnforcementMode::Enforce)));

    let err = vm
        .provision()
        .await
        .expect_err("an unknown image is denied");
    assert!(err.to_string().contains("digest"), "got: {err}");
}

#[tokio::test]
async fn audit_mode_permits_an_unregistered_image() {
    // Audit mode exists so an operator can see what *would* be blocked before
    // turning enforcement on.
    let kernel = temp_image("audited", &bzimage(5));
    let vm = vm_booting("audited", kernel);
    vm.set_image_registry(Arc::new(registry(EnforcementMode::Audit)));

    vm.provision().await.expect("audit mode does not block");
}

#[tokio::test]
async fn admission_follows_the_bytes_not_the_path() {
    // Approving one kernel must not admit a different kernel, however it is
    // named — which is exactly what a path-keyed allowlist would get wrong.
    let approved_kernel = temp_image("bytes-approved", &bzimage(6));
    let digest = digest_of(&approved_kernel);

    let reg = Arc::new(registry(EnforcementMode::Enforce));
    reg.register(approved("internal/kernels/only:1", &digest))
        .unwrap();
    reg.approve("internal/kernels/only:1", "reviewer").unwrap();

    // Same filename, different contents.
    let impostor = temp_image("bytes-approved", &bzimage(7));
    let vm = vm_booting("impostor", impostor);
    vm.set_image_registry(Arc::clone(&reg));

    assert!(
        vm.provision().await.is_err(),
        "different bytes at an approved path must still be refused"
    );
}

#[tokio::test]
async fn a_missing_signature_is_refused_when_required() {
    let kernel = temp_image("unsigned", &bzimage(8));
    let digest = digest_of(&kernel);

    let reg = ImageRegistry::new(RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: Vec::new(),
    });
    reg.register(approved("internal/kernels/unsigned:1", &digest))
        .unwrap();

    // The registry refuses to approve it in the first place, so it never
    // leaves PendingReview — signature policy is enforced at review time, not
    // only at admission.
    let approval = reg.approve("internal/kernels/unsigned:1", "reviewer");
    assert!(
        approval.is_err(),
        "an unsigned image must not be approvable"
    );

    let vm = vm_booting("unsigned", kernel);
    vm.set_image_registry(Arc::new(reg));

    let err = vm
        .provision()
        .await
        .expect_err("an unapproved image must not boot");
    assert!(err.to_string().contains("pending review"), "got: {err}");
}

#[test]
fn the_digest_covers_the_kernel_alone() {
    // Initrds are separate artifacts with their own registry entries, so
    // attaching one must not change the kernel's identity.
    let kernel = temp_image("digest-kernel", &bzimage(9));
    let initrd = temp_image("digest-initrd", &[0xAB; 512]);

    let bare = digest_of(&kernel);
    let with_initrd = BootSource::linux(&kernel)
        .with_initrd(&initrd)
        .load()
        .expect("load")
        .primary_image_digest()
        .expect("digest");

    assert_eq!(bare, with_initrd);
}
