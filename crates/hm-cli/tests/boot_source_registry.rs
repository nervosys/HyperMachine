//! `hm t2 create --kernel …` through to a persisted, restartable VM record.
//!
//! The boot source has to survive the registry round-trip: `hm t2 create`
//! writes it, the process exits, and a later `hm t2 start` reads it back and
//! boots from it. These tests cover that path and the validation that keeps an
//! unbootable VM from being registered in the first place.

use hm_cli::vm_manager::VmManager;
use hv2_core::BootSource;

/// A bzImage header the Linux boot protocol accepts.
fn valid_bzimage() -> Vec<u8> {
    let mut image = vec![0u8; 8192];
    image[0x1F1] = 4; // setup_sects
    image[0x1FE] = 0x55;
    image[0x1FF] = 0xAA;
    image[0x202..0x206].copy_from_slice(b"HdrS");
    image[0x206] = 0x0C; // protocol 2.12
    image[0x207] = 0x02;
    image
}

#[tokio::test]
async fn a_boot_source_survives_the_registry_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("vmlinuz");
    std::fs::write(&kernel, valid_bzimage()).unwrap();

    let source = BootSource::linux(&kernel).with_cmdline("console=ttyS0 root=/dev/vda");

    {
        let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();
        manager
            .create_bootable_vm("booted", 2, 4, false, false, Some(source.clone()))
            .await
            .expect("create a bootable VM");
    }

    // A fresh manager, as a later `hm` invocation would build.
    let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();
    let record = manager.get_vm("booted").await.expect("VM should persist");

    assert_eq!(
        record.boot,
        Some(source),
        "the boot source must come back exactly as it went in, or `hm t2 start` \
         would boot something different from what `hm t2 create` was told"
    );
}

#[tokio::test]
async fn creating_a_vm_with_an_unusable_kernel_fails_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();

    let err = manager
        .create_bootable_vm(
            "doomed",
            2,
            4,
            false,
            false,
            Some(BootSource::linux(dir.path().join("no-such-kernel"))),
        )
        .await
        .expect_err("a missing kernel must be rejected at create time");

    assert!(
        err.to_string().contains("doomed"),
        "the error should name the VM: {err}"
    );

    assert!(
        manager.get_vm("doomed").await.is_err(),
        "a VM that can never boot must not be left in the registry"
    );
}

#[tokio::test]
async fn creating_a_vm_with_a_malformed_kernel_fails_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("not-a-kernel");
    std::fs::write(&kernel, vec![0u8; 8192]).unwrap();

    let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();
    let err = manager
        .create_bootable_vm(
            "bad-header",
            2,
            4,
            false,
            false,
            Some(BootSource::linux(&kernel)),
        )
        .await
        .expect_err("a kernel with no HdrS signature must be rejected");

    assert!(err.to_string().contains("bad-header"), "got: {err}");
}

#[tokio::test]
async fn a_vm_created_without_a_boot_source_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();

    let record = manager
        .create_vm("plain", 2, 4, false, false)
        .await
        .expect("create");

    assert!(record.boot.is_none());
}

#[tokio::test]
async fn a_registry_written_before_boot_sources_existed_still_loads() {
    // Forward compatibility matters here: upgrading `hm` must not orphan the
    // VMs a user already has.
    let dir = tempfile::tempdir().unwrap();
    let legacy = r#"{
        "vms": {
            "legacy-vm": {
                "name": "legacy-vm",
                "cpu_cores": 4,
                "memory_gb": 8,
                "gpu_enabled": false,
                "network_enabled": true,
                "state": "Stopped",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            }
        },
        "version": 1
    }"#;
    std::fs::write(dir.path().join("registry.json"), legacy).unwrap();

    let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();
    let record = manager
        .get_vm("legacy-vm")
        .await
        .expect("a pre-boot-source registry must still load");

    assert_eq!(record.cpu_cores, 4);
    assert!(record.boot.is_none());
}

#[tokio::test]
async fn a_raw_image_boot_source_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("boot.img");
    std::fs::write(&image, [0xF4, 0xEB, 0xFD]).unwrap();

    let source = BootSource::raw(&image);
    let manager = VmManager::with_state_dir(dir.path().to_path_buf()).unwrap();
    manager
        .create_bootable_vm("raw-vm", 1, 1, false, false, Some(source.clone()))
        .await
        .unwrap();

    let record = manager.get_vm("raw-vm").await.unwrap();
    assert_eq!(record.boot, Some(source));
    assert_eq!(record.boot.unwrap().protocol(), "raw");
}
