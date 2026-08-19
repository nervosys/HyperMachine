//! End-to-end tests for booting a VM from a [`BootSource`].
//!
//! These cover the path a CLI or API handler actually takes: configure a VM
//! with a boot source, `provision()` it onto a backend, and `launch()` it into
//! a running execution loop. A recording backend stands in for the hardware so
//! the test asserts on *what the backend was asked to do* — which images landed
//! at which guest physical addresses, and where vCPU 0 was left — rather than
//! on a hypervisor being present.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use hv2_core::boot::source::{BootSource, LoadedBoot, DEFAULT_KERNEL_ADDR, DEFAULT_SETUP_ADDR};
use hv2_core::hypervisor::{
    HypervisorBackend, HypervisorCapabilities, HypervisorPlatform, HypervisorVm,
};
use hv2_core::{Result, VCpu, VMConfig, VMState, VmExit, VM};

// ═══════════════════════════════════════════════════════════════════
//  A backend that records what it was asked to boot
// ═══════════════════════════════════════════════════════════════════

#[derive(Default)]
struct BootRecord {
    /// One entry per `create_vm` call: (vcpu_count, memory_size).
    created: Vec<(u32, u64)>,
    /// Regions the boot source asked to have written into guest memory.
    regions: Vec<(u64, usize)>,
    /// Protocol of each `load_boot` call.
    protocols: Vec<String>,
}

struct RecordingBackend {
    record: Arc<RwLock<BootRecord>>,
    /// When set, `load_boot` fails with this message.
    boot_failure: Option<String>,
    /// Exits handed to the run loop, oldest first; `Shutdown` once drained.
    exits: Arc<RwLock<Vec<VmExit>>>,
}

impl RecordingBackend {
    fn new(record: Arc<RwLock<BootRecord>>) -> Self {
        Self {
            record,
            boot_failure: None,
            exits: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn failing(record: Arc<RwLock<BootRecord>>, message: &str) -> Self {
        Self {
            boot_failure: Some(message.to_string()),
            ..Self::new(record)
        }
    }
}

#[async_trait]
impl HypervisorBackend for RecordingBackend {
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
        self.record
            .write()
            .unwrap()
            .created
            .push((vcpu_count, memory_size));
        Ok(HypervisorVm::new(
            HypervisorPlatform::Tcg,
            vcpu_count,
            memory_size,
        ))
    }

    async fn load_boot(&self, _vcpu: &VCpu, boot: &LoadedBoot) -> Result<()> {
        if let Some(message) = &self.boot_failure {
            return Err(hv2_core::Error::VM(message.clone()));
        }
        let mut record = self.record.write().unwrap();
        record.protocols.push(boot.protocol().to_string());
        for (addr, data) in boot.memory_regions()? {
            record.regions.push((addr, data.len()));
        }
        Ok(())
    }

    async fn run_vcpu(&self, _vcpu: &VCpu) -> Result<VmExit> {
        let mut exits = self.exits.write().unwrap();
        if exits.is_empty() {
            Ok(VmExit::Shutdown)
        } else {
            Ok(exits.remove(0))
        }
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

fn temp_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("hv2-vmboot-{name}"));
    std::fs::write(&path, bytes).expect("write temp boot image");
    path
}

fn config_with(name: &str, boot: Option<BootSource>) -> VMConfig {
    VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        parallel_vcpu: false,
        boot,
        ..VMConfig::default()
    }
}

fn vm_with(config: VMConfig, backend: RecordingBackend) -> Arc<VM> {
    Arc::new(VM::new_with_backend(config, Arc::new(backend)).expect("VM::new_with_backend"))
}

// ═══════════════════════════════════════════════════════════════════
//  Provisioning
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn provision_creates_the_backend_vm() {
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("no-boot", None),
        RecordingBackend::new(record.clone()),
    );

    assert!(
        !vm.is_provisioned(),
        "a freshly constructed VM has no backend partition yet"
    );

    vm.provision().await.expect("provision");

    assert!(vm.is_provisioned());
    assert_eq!(
        record.read().unwrap().created,
        vec![(1, 64 * 1024 * 1024)],
        "the backend should be asked for exactly the configured shape"
    );
}

#[tokio::test]
async fn provision_is_idempotent() {
    // start() and launch() both provision; calling it twice must not create a
    // second partition.
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("idempotent", None),
        RecordingBackend::new(record.clone()),
    );

    vm.provision().await.unwrap();
    vm.provision().await.unwrap();
    vm.provision().await.unwrap();

    assert_eq!(record.read().unwrap().created.len(), 1);
}

#[tokio::test]
async fn provision_without_a_boot_source_loads_nothing() {
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("empty", None),
        RecordingBackend::new(record.clone()),
    );

    vm.provision().await.unwrap();

    assert!(
        record.read().unwrap().protocols.is_empty(),
        "a VM with no boot source must not ask the backend to boot anything"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  Loading a boot source
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn provision_loads_a_linux_kernel_and_positions_vcpu0() {
    let kernel = temp_image("linux-kernel.bin", &valid_bzimage());
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with(
            "linux",
            Some(BootSource::linux(&kernel).with_cmdline("console=ttyS0")),
        ),
        RecordingBackend::new(record.clone()),
    );

    vm.provision().await.expect("provision a Linux VM");

    let record = record.read().unwrap();
    assert_eq!(record.protocols, vec!["linux"]);

    let addrs: Vec<u64> = record.regions.iter().map(|(a, _)| *a).collect();
    assert!(
        addrs.contains(&DEFAULT_KERNEL_ADDR),
        "kernel should be written at 1 MB, got {addrs:#x?}"
    );
    assert!(
        addrs.contains(&DEFAULT_SETUP_ADDR),
        "boot_params should be written at 0x90000, got {addrs:#x?}"
    );

    assert_eq!(
        vm.vcpu(0).unwrap().registers().rip,
        DEFAULT_KERNEL_ADDR,
        "vCPU 0 should be left at the kernel entry point"
    );

    let _ = std::fs::remove_file(kernel);
}

#[tokio::test]
async fn provision_loads_a_raw_image_at_its_entry_point() {
    let image = temp_image("raw-boot.bin", &[0xF4, 0xEB, 0xFD]);
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("raw", Some(BootSource::raw(&image).at(0x7C00, 0x7C00))),
        RecordingBackend::new(record.clone()),
    );

    vm.provision().await.expect("provision a raw-image VM");

    assert_eq!(record.read().unwrap().regions, vec![(0x7C00, 3)]);
    assert_eq!(vm.vcpu(0).unwrap().registers().rip, 0x7C00);

    let _ = std::fs::remove_file(image);
}

// ═══════════════════════════════════════════════════════════════════
//  Failure paths
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn provision_fails_before_touching_the_backend_when_an_image_is_missing() {
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with(
            "missing",
            Some(BootSource::linux("/nonexistent/vmlinuz-does-not-exist")),
        ),
        RecordingBackend::new(record.clone()),
    );

    let err = vm
        .provision()
        .await
        .expect_err("a missing kernel must fail provisioning");
    assert!(
        err.to_string().contains("vmlinuz-does-not-exist"),
        "the error should name the image: {err}"
    );

    assert!(
        record.read().unwrap().protocols.is_empty(),
        "the backend must not be asked to boot an image that could not be read"
    );
    assert!(
        !vm.is_provisioned(),
        "a VM whose boot source failed must not be marked provisioned, so a \
         retry after fixing the path still works"
    );
}

#[tokio::test]
async fn provision_rejects_a_boot_image_larger_than_guest_memory() {
    // 1 MB of RAM cannot hold a kernel that loads *at* 1 MB.
    let kernel = temp_image("too-big-kernel.bin", &valid_bzimage());
    let mut config = config_with("cramped", Some(BootSource::linux(&kernel)));
    config.memory_size = 1024 * 1024;

    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(config, RecordingBackend::new(record.clone()));

    let err = vm
        .provision()
        .await
        .expect_err("images that do not fit must be rejected");
    let message = err.to_string();
    assert!(
        message.contains("guest memory") && message.contains("cramped"),
        "the error should explain the shortfall and name the VM: {message}"
    );

    let _ = std::fs::remove_file(kernel);
}

#[tokio::test]
async fn a_backend_boot_failure_leaves_the_vm_unprovisioned() {
    let image = temp_image("backend-fail.bin", &[0x90]);
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("backend-fail", Some(BootSource::raw(&image))),
        RecordingBackend::failing(record, "partition rejected the image"),
    );

    let err = vm
        .provision()
        .await
        .expect_err("backend failure propagates");
    assert!(err.to_string().contains("partition rejected the image"));
    assert!(!vm.is_provisioned());

    let _ = std::fs::remove_file(image);
}

#[tokio::test]
async fn a_backend_without_boot_support_says_so() {
    // The default `load_boot` reports NotSupported rather than silently
    // starting a VM that will never execute.
    struct NoBootBackend;

    #[async_trait]
    impl HypervisorBackend for NoBootBackend {
        fn platform(&self) -> HypervisorPlatform {
            HypervisorPlatform::Hvf
        }
        fn capabilities(&self) -> HypervisorCapabilities {
            HypervisorCapabilities {
                max_vcpus: 8,
                max_memory: 1 << 40,
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
                HypervisorPlatform::Hvf,
                vcpu_count,
                memory_size,
            ))
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

    let image = temp_image("unsupported.bin", &[0x90]);
    let vm = Arc::new(
        VM::new_with_backend(
            config_with("unsupported", Some(BootSource::raw(&image))),
            Arc::new(NoBootBackend),
        )
        .unwrap(),
    );

    let err = vm
        .provision()
        .await
        .expect_err("should report NotSupported");
    let message = err.to_string();
    assert!(
        message.contains("cannot boot") && message.contains("raw"),
        "the error should name the backend and the protocol: {message}"
    );

    let _ = std::fs::remove_file(image);
}

// ═══════════════════════════════════════════════════════════════════
//  Launching
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn launch_provisions_starts_and_runs() {
    let image = temp_image("launch.bin", &[0xF4]);
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("launched", Some(BootSource::raw(&image))),
        RecordingBackend::new(record.clone()),
    );

    vm.launch().await.expect("launch");

    assert!(vm.is_provisioned(), "launch provisions the backend VM");
    assert_eq!(vm.state(), VMState::Running);
    assert_eq!(record.read().unwrap().protocols, vec!["raw"]);

    vm.stop().await.expect("stop");
    assert_eq!(vm.state(), VMState::Stopped);

    let _ = std::fs::remove_file(image);
}

#[tokio::test]
async fn stop_reaps_the_launched_execution_loop() {
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("reaped", None),
        RecordingBackend::new(record.clone()),
    );

    vm.launch().await.expect("launch");

    // stop() must return rather than hang: it notifies the loop before
    // awaiting it, and bounds the wait either way.
    tokio::time::timeout(std::time::Duration::from_secs(10), vm.stop())
        .await
        .expect("stop should not hang on the execution loop")
        .expect("stop");

    assert_eq!(vm.state(), VMState::Stopped);
}

#[tokio::test]
async fn launch_refuses_a_vm_that_cannot_boot() {
    // The failure must surface from launch() itself — not silently, and not
    // from a background task the caller never awaits.
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with(
            "bad-launch",
            Some(BootSource::linux("/nonexistent/vmlinuz")),
        ),
        RecordingBackend::new(record),
    );

    let err = vm.launch().await.expect_err("launch should fail");
    assert!(err.to_string().contains("vmlinuz"), "got: {err}");
    assert_eq!(
        vm.state(),
        VMState::Created,
        "a VM that failed to boot must not be left looking Running"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  Configuration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn vm_config_round_trips_its_boot_source_through_toml() {
    // The boot source has to survive `hm t2 create` writing a config and a
    // later `hm t2 start` reading it back.
    let config = config_with(
        "persisted",
        Some(
            BootSource::linux("/boot/vmlinuz")
                .with_initrd("/boot/initrd.img")
                .with_cmdline("root=/dev/vda ro"),
        ),
    );

    let text = toml::to_string(&config).expect("serialize");
    let back: VMConfig = toml::from_str(&text).expect("deserialize");

    assert_eq!(back.boot, config.boot);
}

#[test]
fn a_config_with_no_boot_key_deserializes_to_no_boot_source() {
    let config: VMConfig = toml::from_str(
        r#"
        name = "legacy"
        vcpu_count = 2
        memory_size = 1073741824
        enable_gpu = false
        enable_networking = false
        enable_tracing = false
        "#,
    )
    .expect("a config written before boot sources existed must still load");

    assert!(config.boot.is_none());
}

// ═══════════════════════════════════════════════════════════════════
//  Backends that cannot execute
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_backend_reports_whether_it_executes_guest_code() {
    // The recording backend stands in for hardware, so it claims execution;
    // TCG is the one that does not, and a caller should be able to ask rather
    // than infer it from a Running VM that produces nothing.
    let record = Arc::new(RwLock::new(BootRecord::default()));
    let vm = vm_with(
        config_with("asks", None),
        RecordingBackend::new(record.clone()),
    );

    assert!(
        vm.executes_guest_code(),
        "the default is true, so only a backend that cannot execute has to say so"
    );
}
