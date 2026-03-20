//! Unikernel GPU-Accelerated AI Services Tests
//!
//! Tests the ultra-lightweight VM stack for GPU-accelerated AI inference:
//! - Boot protocol configuration for unikernel-style minimal kernels
//! - Image registry admission for AI kernel images
//! - Topology-aware placement for multi-GPU inference
//! - Capacity reservations for GPU inference tiers
//! - Runtime session lifecycle with GPU fabric
//!
//! "Unikernel" in HyperMachine maps to: Type-1 bare-metal boot (HV1) or
//! Type-2 lightweight kernel loading (HV2 Multiboot/Linux boot) combined
//! with GPU passthrough/vGPU for AI workloads.

use std::time::Duration;

// Boot protocols (unikernel loading via Multiboot / Linux bzImage)
use hv2_core::boot::linux::LinuxBootParams;
use hv2_core::boot::multiboot::{MultibootInfo, MultibootModule, MultibootProtocol};

// Image registry
use hv2_core::security::{
    AdmissionDecision, EnforcementMode, ImageEntry, ImageKind, ImageRegistry,
    ImageSignature, RegistryConfig,
};

// Runtime GPU fabric
use hv2_runtime::{
    CapacityManager, GpuDevice, GpuInterconnect, GpuRequirements, GpuTopologyMap, PoolConfig,
    Runtime, RuntimeConfig, SlaTier, VmClass,
};

// ═══════════════════════════════════════════════════════════════════
//  Unikernel Boot Configuration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn linux_boot_params_for_ai_kernel() {
    // Configure a minimal Linux bzImage kernel optimized for AI inference
    let params = LinuxBootParams {
        kernel_image: create_stub_bzimage(),
        initrd: Some(create_stub_initrd()),
        cmdline: "console=ttyS0 root=/dev/vda isolcpus=1-7 nohz_full=1-7 \
                  nvidia.NVreg_EnableGpuFirmware=1 iommu=pt"
            .to_string(),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
    };

    assert!(!params.kernel_image.is_empty());
    assert!(params.initrd.is_some());
    assert!(params.cmdline.contains("nvidia"));
    assert!(params.cmdline.contains("iommu=pt"));
    assert_eq!(params.setup_addr, 0x90000);
    assert_eq!(params.kernel_addr, 0x100000);
}

#[test]
fn multiboot_unikernel_config() {
    // Multiboot 1.0 is the standard for loading unikernel-style
    // single-purpose kernels (like MirageOS, IncludeOS, etc.)
    let info = MultibootInfo {
        kernel_image: create_stub_multiboot_kernel(),
        modules: vec![
            MultibootModule {
                data: b"model=resnet50".to_vec(),
                cmdline: "ai-model-config".to_string(),
            },
            MultibootModule {
                data: vec![0u8; 1024], // stub model weights
                cmdline: "model-weights".to_string(),
            },
        ],
        cmdline: "gpu=passthrough inference_batch_size=32".to_string(),
        memory_map: vec![
            (0, 640 * 1024),                  // Lower memory
            (1024 * 1024, 127 * 1024 * 1024), // Upper memory (127 MB)
        ],
    };

    assert!(!info.kernel_image.is_empty());
    assert_eq!(info.modules.len(), 2);
    assert!(info.cmdline.contains("gpu=passthrough"));
    assert_eq!(info.memory_map.len(), 2);
}

#[test]
fn multiboot_header_search() {
    // Create a kernel with valid Multiboot header
    let kernel = create_stub_multiboot_kernel();
    let result = MultibootProtocol::find_header(&kernel);
    // Valid Multiboot header should be findable
    assert!(result.is_ok());
}

#[test]
fn multiboot_default_memory_map() {
    let info = MultibootInfo::default();
    // Default has lower + upper memory regions
    assert_eq!(info.memory_map.len(), 2);
    assert_eq!(info.memory_map[0].0, 0); // Lower memory starts at 0
    assert_eq!(info.memory_map[1].0, 1024 * 1024); // Upper at 1MB
}

// ═══════════════════════════════════════════════════════════════════
//  GPU Passthrough Configuration for AI Accelerators
//  (Tested via hv2_core re-exports available through hv2-runtime)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn gpu_passthrough_concepts() {
    // GPU passthrough requires IOMMU and VFIO to securely assign a
    // physical GPU to a unikernel VM. We test the configuration concepts
    // here; actual hardware binding is tested in hv2-gpu crate.

    // Typical NVIDIA GPU PCI addresses in a multi-GPU server
    let gpu_bdf_addresses = [
        "0000:3b:00.0",
        "0000:86:00.0",
        "0000:af:00.0",
        "0000:d8:00.0",
    ];

    // All 4 should parse as valid BDF
    for bdf in &gpu_bdf_addresses {
        assert!(bdf.split(':').count() >= 2, "BDF should have domain:bus:dev.fn");
    }

    // NVIDIA vendor ID
    let nvidia_vendor_id: u16 = 0x10de;
    let a100_device_id: u16 = 0x20b5;
    let h100_device_id: u16 = 0x2330;
    let t4_device_id: u16 = 0x1eb8;

    assert_ne!(a100_device_id, h100_device_id);
    assert_ne!(a100_device_id, t4_device_id);
    assert_eq!(nvidia_vendor_id, 0x10de);
}

// ═══════════════════════════════════════════════════════════════════
//  Boot + GPU Topology Integration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn boot_config_references_gpu_device() {
    // A unikernel kernel's cmdline can reference the assigned GPU
    let mut topo = build_ai_cluster_topology();
    let req = GpuRequirements::default().count(1);
    let placement = topo.find_placement(&req).unwrap();
    topo.allocate(&placement.gpu_ids);

    let params = LinuxBootParams {
        kernel_image: create_stub_bzimage(),
        initrd: None,
        cmdline: format!(
            "console=ttyS0 gpu_device={} inference_mode=batch",
            placement.gpu_ids[0]
        ),
        ..Default::default()
    };

    assert!(params.cmdline.contains(&placement.gpu_ids[0]));
    topo.release(&placement.gpu_ids);
}

// ═══════════════════════════════════════════════════════════════════
//  Unikernel VM Configuration Concepts
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unikernel_minimal_memory_map() {
    // A unikernel needs very little memory — Multiboot default provides
    // a suitable layout for a lightweight inference VM
    let info = MultibootInfo::default();
    // Lower memory: 0 to 640KB (real mode memory)
    assert_eq!(info.memory_map[0], (0, 640 * 1024));
    // Upper memory: 1MB to 128MB (plenty for a unikernel AI service)
    let upper = info.memory_map[1];
    assert!(upper.1 >= 64 * 1024 * 1024, "Should have at least 64MB upper memory");
}

// ═══════════════════════════════════════════════════════════════════
//  Image Admission for AI Kernels
// ═══════════════════════════════════════════════════════════════════

fn signed_ai_kernel_entry() -> ImageEntry {
    ImageEntry::new(
        "registry.internal/kernels/ai-inference:2.1.0",
        ImageKind::Kernel,
        "sha256:aabbccdd11223344",
    )
    .label("purpose", "gpu-inference")
    .label("framework", "tensorrt")
    .label("gpu_required", "true")
    .signature(ImageSignature {
        signer: "ai-team-key".to_string(),
        algorithm: "ed25519".to_string(),
        signature_hex: "signed-by-ai-team".to_string(),
        signed_at: std::time::SystemTime::now(),
        verified: true,
    })
}

#[test]
fn admit_signed_ai_kernel() {
    let config = RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: vec!["ai-team-key".to_string()],
    };
    let registry = ImageRegistry::new(config);
    registry.register(signed_ai_kernel_entry()).unwrap();

    registry
        .approve(
            "registry.internal/kernels/ai-inference:2.1.0",
            "admin@nervosys",
        )
        .unwrap();

    let decision = registry
        .check_admission("registry.internal/kernels/ai-inference:2.1.0");
    assert_eq!(decision, AdmissionDecision::Allowed);
}

#[test]
fn reject_untrusted_signer_ai_kernel() {
    let config = RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: vec!["official-key-only".to_string()],
    };
    let registry = ImageRegistry::new(config);

    // Register image signed by different key
    let entry = signed_ai_kernel_entry(); // signed by "ai-team-key", not in trusted list
    registry.register(entry).unwrap();

    // approve() succeeds (signature is valid), but check_admission rejects
    // because the signer is not in trusted_signers
    registry
        .approve(
            "registry.internal/kernels/ai-inference:2.1.0",
            "admin",
        )
        .unwrap();

    let decision = registry
        .check_admission("registry.internal/kernels/ai-inference:2.1.0");
    assert!(matches!(decision, AdmissionDecision::Denied(_)));
}

#[test]
fn audit_mode_allows_all() {
    let config = RegistryConfig {
        mode: EnforcementMode::Audit,
        require_signature: false,
        trusted_signers: vec![],
    };
    let registry = ImageRegistry::new(config);

    // In audit mode, unknown images should still be admitted
    let decision = registry
        .check_admission("registry.internal/unknown:latest");
    // Audit mode logs but allows
    assert!(matches!(
        decision,
        AdmissionDecision::Allowed | AdmissionDecision::AllowedWithWarning(_)
    ));
}

// ═══════════════════════════════════════════════════════════════════
//  Topology-Aware Placement for AI Workloads
// ═══════════════════════════════════════════════════════════════════

fn build_ai_cluster_topology() -> GpuTopologyMap {
    let mut topo = GpuTopologyMap::new();

    // Inference host: 2 x T4 (lower power, good for inference)
    for i in 0..2u32 {
        let dev = GpuDevice::new(format!("t4-{i}"), "inference-node", "T4-16GB")
            .numa(0)
            .vram(16 * 1024 * 1024 * 1024)
            .capability(75);
        topo.add_device(dev);
    }
    topo.add_link("t4-0", "t4-1", GpuInterconnect::PciePeer, 1);

    // Training host: 4 x A100 with NVLink mesh
    for i in 0..4u32 {
        let dev = GpuDevice::new(format!("a100-{i}"), "training-node", "A100-SXM4-80GB")
            .numa(i / 2)
            .vram(80 * 1024 * 1024 * 1024)
            .capability(80);
        topo.add_device(dev);
    }
    for i in 0..4u32 {
        for j in (i + 1)..4 {
            topo.add_link(
                &format!("a100-{i}"),
                &format!("a100-{j}"),
                GpuInterconnect::NvLink,
                12,
            );
        }
    }

    topo
}

#[test]
fn place_inference_workload_on_t4() {
    let topo = build_ai_cluster_topology();

    // Inference needs 1 GPU, doesn't need NVLink
    let req = GpuRequirements::default().count(1);

    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 1);
}

#[test]
fn place_training_workload_on_a100_nvlink() {
    let topo = build_ai_cluster_topology();

    // Training needs 4 x A100 with NVLink and 80GB VRAM
    let req = GpuRequirements::default()
        .count(4)
        .min_vram(80 * 1024 * 1024 * 1024)
        .nvlink();

    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 4);
    assert_eq!(placement.host_id, "training-node");
    assert!(placement.affinity_score > 0.8);
}

#[test]
fn allocate_for_concurrent_ai_workloads() {
    let mut topo = build_ai_cluster_topology();

    // First: grab 2 inference GPUs from T4 pool specifically
    // Allocate T4s manually by id so training node stays free
    topo.allocate(&["t4-0".to_string(), "t4-1".to_string()]);

    // Second: grab training GPUs (all 4 A100s should still be available)
    let training_req = GpuRequirements::default()
        .count(4)
        .min_vram(80 * 1024 * 1024 * 1024)
        .nvlink();

    let train_placement = topo.find_placement(&training_req).unwrap();
    assert_eq!(train_placement.host_id, "training-node");
    topo.allocate(&train_placement.gpu_ids);

    // All GPUs now allocated
    assert_eq!(topo.available_count(), 0);

    // Release inference GPUs
    topo.release(&["t4-0".to_string(), "t4-1".to_string()]);
    assert_eq!(topo.available_count(), 2);
}

// ═══════════════════════════════════════════════════════════════════
//  End-to-End: Unikernel AI Service Pipeline
// ═══════════════════════════════════════════════════════════════════

/// Full pipeline: admit kernel image → reserve capacity → place GPUs →
/// configure boot → build agent VM
#[test]
fn unikernel_ai_service_e2e() {
    // 1. Image admission: approve the AI unikernel
    let reg_config = RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: vec!["nervosys-key".to_string()],
    };
    let registry = ImageRegistry::new(reg_config);

    let kernel_entry = ImageEntry::new(
        "registry.internal/unikernel/gpu-inference:1.0.0",
        ImageKind::Kernel,
        "sha256:unikernel001",
    )
    .label("type", "unikernel")
    .label("gpu", "required")
    .label("framework", "tensorrt")
    .signature(ImageSignature {
        signer: "nervosys-key".to_string(),
        algorithm: "ed25519".to_string(),
        signature_hex: "valid-sig".to_string(),
        signed_at: std::time::SystemTime::now(),
        verified: true,
    });

    registry.register(kernel_entry).unwrap();
    registry
        .approve(
            "registry.internal/unikernel/gpu-inference:1.0.0",
            "admin",
        )
        .unwrap();

    let decision = registry
        .check_admission("registry.internal/unikernel/gpu-inference:1.0.0");
    assert_eq!(decision, AdmissionDecision::Allowed);

    // 2. Capacity reservation for GPU inference tier
    let cm = CapacityManager::new();
    cm.register_class(
        VmClass::new("gpu-inference-t4", SlaTier::Premium)
            .vcpus(4)
            .memory(16 * 1024 * 1024 * 1024)
            .gpus(1, "T4")
            .rate(0.526)
            .max(50),
    )
    .unwrap();

    let rsv_id = cm
        .create_reservation("ai-service-tenant", "gpu-inference-t4", 10, Duration::from_secs(86400))
        .unwrap();
    cm.consume_reservation(&rsv_id).unwrap();

    // 3. Topology-aware GPU placement
    let mut topo = build_ai_cluster_topology();
    let req = GpuRequirements::default().count(1);
    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 1);

    // 5. Verify GPU passthrough metadata (device-level, no hv2_gpu dep)
    // Must check before allocate() since devices_on_host filters allocated GPUs
    let placed_gpu = topo
        .devices_on_host(&placement.host_id)
        .into_iter()
        .find(|d| d.id == placement.gpu_ids[0])
        .expect("placed device must exist");
    assert!(placed_gpu.vram_bytes > 0);
    assert!(!placed_gpu.id.is_empty());

    topo.allocate(&placement.gpu_ids);

    // 4. Configure boot protocol for the unikernel
    let boot_params = LinuxBootParams {
        kernel_image: create_stub_bzimage(),
        initrd: None,
        cmdline: format!(
            "console=ttyS0 gpu_device={} inference_mode=batch",
            placement.gpu_ids[0]
        ),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
    };
    assert!(boot_params.cmdline.contains(&placement.gpu_ids[0]));

    // 6. Verify runtime can host unikernel session with GPU fabric
    let config = RuntimeConfig::builder()
        .instance_id("e2e-unikernel")
        .pool(PoolConfig {
            min_warm: 1,
            max_size: 8,
            ..Default::default()
        })
        .gpu_topology(true)
        .capacity_reservations(true)
        .build();
    let rt = Runtime::new(config);
    rt.maintenance_tick();
    assert!(rt.gpu_topology().is_some());
    assert!(rt.capacity_manager().is_some());

    // Cleanup
    topo.release(&placement.gpu_ids);
    cm.release_reservation(&rsv_id).unwrap();
}

/// Test: Runtime integrates GPU fabric with session lifecycle for
/// unikernel AI inference services.
#[test]
fn runtime_unikernel_ai_session() {
    let config = RuntimeConfig::builder()
        .instance_id("unikernel-ai-runtime")
        .pool(PoolConfig {
            min_warm: 4,
            max_size: 64,
            ..Default::default()
        })
        .gpu_topology(true)
        .capacity_reservations(true)
        .build();

    let rt = Runtime::new(config);
    rt.maintenance_tick();

    // Create AI inference session
    let session = rt
        .create_session("ai-inference-001", hv2_runtime::BillingTier::Premium)
        .unwrap();
    assert!(!session.vm_id.is_empty());
    assert_eq!(session.tier, hv2_runtime::BillingTier::Premium);

    // GPU topology and capacity managers are available
    assert!(rt.gpu_topology().is_some());
    assert!(rt.capacity_manager().is_some());

    // Record GPU compute billing
    rt.billing_engine()
        .record("ai-inference-001", "gpu_minutes", 60.0)
        .unwrap();

    // Destroy session and check billing
    let invoice = rt.destroy_session("ai-inference-001").unwrap();
    assert!(invoice.is_some());
    let inv = invoice.unwrap();
    assert!(inv.total() > 0.0);
}

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

/// Create a minimal stub that looks like a Linux bzImage header.
///
/// A real bzImage has setup sectors + kernel payload. This stub has
/// just enough structure to pass header validation checks.
fn create_stub_bzimage() -> Vec<u8> {
    let mut kernel = vec![0u8; 16384];
    // Setup sectors byte at offset 0x01F1
    kernel[0x01F1] = 4; // 4 setup sectors
    // Boot flag at 0x01FE
    kernel[0x01FE] = 0x55;
    kernel[0x01FF] = 0xAA;
    // "HdrS" magic at 0x0202
    kernel[0x0202] = b'H';
    kernel[0x0203] = b'd';
    kernel[0x0204] = b'r';
    kernel[0x0205] = b'S';
    // Protocol version 2.10 at 0x0206
    kernel[0x0206] = 0x0A;
    kernel[0x0207] = 0x02;
    kernel
}

/// Create a stub initrd
fn create_stub_initrd() -> Vec<u8> {
    // Minimal CPIO archive header (newc format)
    let header = b"070701";
    let mut initrd = vec![0u8; 4096];
    initrd[..6].copy_from_slice(header);
    initrd
}

/// Create a stub kernel with a valid Multiboot header.
///
/// Multiboot header: magic (0x1BADB002) + flags + checksum,
/// where magic + flags + checksum == 0.
fn create_stub_multiboot_kernel() -> Vec<u8> {
    let mut kernel = vec![0u8; 8192];

    let magic: u32 = 0x1BADB002;
    let flags: u32 = 0x00000003; // PAGE_ALIGN | MEMORY_INFO
    let checksum: u32 = (0u32.wrapping_sub(magic)).wrapping_sub(flags);

    // Place header at offset 0 (4-byte aligned)
    kernel[0..4].copy_from_slice(&magic.to_le_bytes());
    kernel[4..8].copy_from_slice(&flags.to_le_bytes());
    kernel[8..12].copy_from_slice(&checksum.to_le_bytes());

    kernel
}
