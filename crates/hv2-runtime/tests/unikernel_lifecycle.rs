//! Unikernel Lifecycle Integration Tests
//!
//! End-to-end tests that verify the full unikernel lifecycle:
//! boot protocol → guest memory → VM pool → session → recycle.
//!
//! These tests prove that unikernels *work* as a coherent system —
//! not just individual components but the full path from kernel image
//! validation through warm-pool session management and teardown.

use std::time::Duration;

use hv2_core::boot::linux::{LinuxBootParams, LinuxBootProtocol};
use hv2_core::boot::multiboot::{MultibootInfo, MultibootModule, MultibootProtocol};
use hv2_core::memory::GuestMemory;
use hv2_core::security::{
    AdmissionDecision, EnforcementMode, ImageEntry, ImageKind, ImageRegistry, ImageSignature,
    RegistryConfig,
};

use hv2_runtime::{
    BillingTier, CapacityManager, GpuDevice, GpuInterconnect, GpuRequirements, GpuTopologyMap,
    PoolConfig, Runtime, RuntimeConfig, SlaTier, VmClass, VmSlotState,
};

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

fn create_valid_bzimage() -> Vec<u8> {
    let mut kernel = vec![0u8; 16384];
    kernel[0x01F1] = 4; // setup_sects
    kernel[0x01FE] = 0x55;
    kernel[0x01FF] = 0xAA;
    kernel[0x0202] = b'H';
    kernel[0x0203] = b'd';
    kernel[0x0204] = b'r';
    kernel[0x0205] = b'S';
    kernel[0x0206] = 0x0A; // version 2.10
    kernel[0x0207] = 0x02;
    kernel
}

fn create_valid_multiboot_kernel() -> Vec<u8> {
    let mut kernel = vec![0u8; 8192];
    let magic: u32 = 0x1BADB002;
    let flags: u32 = 0x00000003;
    let checksum: u32 = 0u32.wrapping_sub(magic).wrapping_sub(flags);
    kernel[0..4].copy_from_slice(&magic.to_le_bytes());
    kernel[4..8].copy_from_slice(&flags.to_le_bytes());
    kernel[8..12].copy_from_slice(&checksum.to_le_bytes());
    kernel
}

fn build_inference_topology() -> GpuTopologyMap {
    let mut topo = GpuTopologyMap::new();
    for i in 0..2u32 {
        let dev = GpuDevice::new(format!("gpu-{i}"), "infer-host", "T4-16GB")
            .numa(0)
            .vram(16 * 1024 * 1024 * 1024)
            .capability(75);
        topo.add_device(dev);
    }
    topo.add_link("gpu-0", "gpu-1", GpuInterconnect::PciePeer, 1);
    topo
}

fn runtime_with_warm_pool(n: usize) -> Runtime {
    let config = RuntimeConfig::builder()
        .pool(PoolConfig {
            min_warm: n,
            max_size: 64,
            ..Default::default()
        })
        .instance_id("unikernel-lifecycle-test")
        .gpu_topology(true)
        .capacity_reservations(true)
        .build();
    let rt = Runtime::new(config);
    for _ in 0..n {
        let vm_id = rt.pool().provision().unwrap();
        rt.pool().mark_warm(&vm_id).unwrap();
    }
    rt
}

fn trusted_registry() -> ImageRegistry {
    let config = RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: vec!["nervosys-key".to_string()],
    };
    ImageRegistry::new(config)
}

fn signed_kernel_entry(name: &str, digest: &str) -> ImageEntry {
    ImageEntry::new(name, ImageKind::Kernel, digest)
        .label("type", "unikernel")
        .signature(ImageSignature {
            signer: "nervosys-key".to_string(),
            algorithm: "ed25519".to_string(),
            signature_hex: "valid".to_string(),
            signed_at: std::time::SystemTime::now(),
            verified: true,
        })
}

// ═══════════════════════════════════════════════════════════════════
//  1. Boot Protocol Validation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn linux_bzimage_validates_and_prepares_memory_regions() {
    let params = LinuxBootParams {
        kernel_image: create_valid_bzimage(),
        initrd: Some(vec![0xCA; 4096]),
        cmdline: "console=ttyS0 iommu=pt isolcpus=1-7".to_string(),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
        // The e820 map the kernel reads is built from this.
        memory_size: 64 * 1024 * 1024,
    };

    LinuxBootProtocol::validate_params(&params).unwrap();
    let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();

    // Should have: initrd, boot_params, cmdline, kernel
    assert!(
        regions.len() >= 3,
        "Expected >=3 regions, got {}",
        regions.len()
    );

    // Verify boot_params at setup_addr
    let bp = regions.iter().find(|(a, _)| *a == 0x90000);
    assert!(bp.is_some(), "boot_params must be at setup_addr");
    assert_eq!(bp.unwrap().1.len(), 4096);

    // Verify cmdline at setup_addr + 0x1000
    let cl = regions.iter().find(|(a, _)| *a == 0x91000);
    assert!(cl.is_some(), "cmdline must be at setup_addr + 0x1000");
    let cmdline_bytes = &cl.unwrap().1;
    assert!(
        cmdline_bytes.ends_with(&[0]),
        "cmdline must be null-terminated"
    );

    // The initrd goes high in guest memory, not to a fixed 32 MB. A constant
    // here is what hid the collision that mattered: a compressed kernel
    // unpacks itself into init_size bytes from where it runs, and 32 MB sits
    // inside that region for any kernel of ordinary size -- decompression
    // overwrote the initrd and the kernel reported invalid magic about bytes
    // it had destroyed itself. What matters is that it is placed, page
    // aligned, and inside the guest's memory.
    let rd = regions
        .iter()
        .find(|(_, data)| data.len() == 4096)
        .expect("the initrd must be placed somewhere");
    assert_eq!(rd.0 % 4096, 0, "initrd placement is page aligned");
    assert!(
        rd.0 + 4096 <= params.memory_size,
        "initrd at {:#x} runs past the guest's memory",
        rd.0
    );
}

#[test]
fn multiboot_header_roundtrip() {
    let kernel = create_valid_multiboot_kernel();
    let header = MultibootProtocol::find_header(&kernel).unwrap();
    assert_eq!(header.offset, 0);

    // Verify checksum invariant
    let sum = 0x1BADB002u32
        .wrapping_add(header.flags)
        .wrapping_add(header.checksum);
    assert_eq!(sum, 0, "magic + flags + checksum must be 0");
}

#[test]
fn multiboot_with_model_weight_modules() {
    let info = MultibootInfo {
        kernel_image: create_valid_multiboot_kernel(),
        modules: vec![
            MultibootModule {
                data: vec![0xAB; 1024 * 1024], // 1 MB fake model weights
                cmdline: "model-weights-shard-0".to_string(),
            },
            MultibootModule {
                data: b"batch_size=32\nmax_seq_len=2048".to_vec(),
                cmdline: "inference-config".to_string(),
            },
        ],
        cmdline: "gpu=passthrough inference_engine=tensorrt".to_string(),
        memory_map: vec![(0, 640 * 1024), (1024 * 1024, 256 * 1024 * 1024)],
    };

    MultibootProtocol::validate_params(&info).unwrap();

    let mb_info =
        MultibootProtocol::create_multiboot_info(&info, 0x10000, 0x11000, Some(0x12000), 0x13000);
    // flags should have modules bit set
    let flags = u32::from_le_bytes(mb_info[0..4].try_into().unwrap());
    assert_ne!(
        flags & (1 << 3),
        0,
        "mods bit must be set when modules present"
    );
    assert_eq!(
        u32::from_le_bytes(mb_info[20..24].try_into().unwrap()),
        2,
        "mods_count should be 2"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  2. Guest Memory: Write Boot Structures & Read Back
// ═══════════════════════════════════════════════════════════════════

#[test]
fn guest_memory_holds_linux_boot_regions() {
    let params = LinuxBootParams {
        kernel_image: create_valid_bzimage(),
        initrd: None,
        cmdline: "console=ttyS0".to_string(),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
        // The e820 map the kernel reads is built from this.
        memory_size: 64 * 1024 * 1024,
    };

    let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();

    // Allocate guest memory large enough for all regions
    let mem = GuestMemory::new(16 * 1024 * 1024).unwrap();

    // We need a contiguous region that spans all addresses.
    // Allocate from 0 to 2MB to cover setup_addr + kernel_addr.
    let base = mem.allocate_region(2 * 1024 * 1024, false).unwrap();
    assert_eq!(base, 0);

    // Write all boot regions into guest memory
    for (addr, data) in &regions {
        mem.write_bytes(*addr, data).unwrap();
    }

    // Read back and verify boot_params
    let bp_readback = mem.read_bytes(0x90000, 4096).unwrap();
    assert_eq!(bp_readback.len(), 4096);

    // Read back cmdline and verify content
    let cl_data = mem.read_bytes(0x91000, 14).unwrap(); // "console=ttyS0\0"
    let cl_str = std::str::from_utf8(&cl_data[..13]).unwrap();
    assert_eq!(cl_str, "console=ttyS0");
}

#[test]
fn guest_memory_multiboot_info_roundtrip() {
    let info = MultibootInfo {
        kernel_image: create_valid_multiboot_kernel(),
        modules: Vec::new(),
        cmdline: "root=/dev/vda".to_string(),
        memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
    };

    let mb_data = MultibootProtocol::create_multiboot_info(&info, 0x10000, 0x11000, None, 0x12000);
    let mmap_data = MultibootProtocol::create_memory_map(&info.memory_map);

    let mem = GuestMemory::new(2 * 1024 * 1024).unwrap();
    mem.allocate_region(2 * 1024 * 1024, false).unwrap();

    mem.write_bytes(0x10000, &mb_data).unwrap();
    mem.write_bytes(0x12000, &mmap_data).unwrap();

    // Read back multiboot_info and check mem_lower
    let readback = mem.read_bytes(0x10000, 52).unwrap();
    let mem_lower = u32::from_le_bytes(readback[4..8].try_into().unwrap());
    assert_eq!(mem_lower, 640);

    // Read back memory map first entry base address
    let mmap_readback = mem.read_bytes(0x12000, 24).unwrap();
    let base = u64::from_le_bytes(mmap_readback[4..12].try_into().unwrap());
    assert_eq!(base, 0);
}

#[test]
fn guest_memory_isolation_between_regions() {
    let mem = GuestMemory::new(4 * 1024 * 1024).unwrap();
    let r1 = mem.allocate_region(4096, false).unwrap();
    let r2 = mem.allocate_region(4096, false).unwrap();
    assert_ne!(r1, r2);

    // Write distinct patterns
    mem.write_bytes(r1, &[0xAA; 4096]).unwrap();
    mem.write_bytes(r2, &[0xBB; 4096]).unwrap();

    // Verify no cross-contamination
    let d1 = mem.read_bytes(r1, 4096).unwrap();
    let d2 = mem.read_bytes(r2, 4096).unwrap();
    assert!(d1.iter().all(|&b| b == 0xAA));
    assert!(d2.iter().all(|&b| b == 0xBB));
}

// ═══════════════════════════════════════════════════════════════════
//  3. Image Admission → Pool → Session Lifecycle
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unikernel_admitted_then_served_via_pool() {
    // Step 1: admit the kernel image
    let registry = trusted_registry();
    registry
        .register(signed_kernel_entry(
            "registry.internal/unikernels/llm-serve:1.0",
            "sha256:deadbeef01",
        ))
        .unwrap();
    registry
        .approve("registry.internal/unikernels/llm-serve:1.0", "admin")
        .unwrap();
    assert_eq!(
        registry.check_admission("registry.internal/unikernels/llm-serve:1.0"),
        AdmissionDecision::Allowed
    );

    // Step 2: validate the kernel boots correctly
    let params = LinuxBootParams {
        kernel_image: create_valid_bzimage(),
        initrd: None,
        cmdline: "console=ttyS0 inference_mode=continuous".to_string(),
        // The e820 map the kernel reads is built from this.
        memory_size: 64 * 1024 * 1024,
        ..Default::default()
    };
    LinuxBootProtocol::validate_params(&params).unwrap();
    let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();
    assert!(!regions.is_empty());

    // Step 3: create runtime with warm pool, create session
    let rt = runtime_with_warm_pool(2);
    let session = rt
        .create_session("llm-session-1", BillingTier::Premium)
        .unwrap();
    assert!(!session.vm_id.is_empty());

    // Verify VM is assigned
    let vm = rt.pool().get(&session.vm_id).unwrap();
    assert_eq!(vm.state, VmSlotState::Assigned);

    // Step 4: record inference billing
    rt.billing_engine()
        .record("llm-session-1", "gpu_minutes", 120.0)
        .unwrap();

    // Step 5: destroy session, verify invoice and cleanup
    let invoice = rt.destroy_session("llm-session-1").unwrap();
    assert!(invoice.is_some());
    assert!(invoice.unwrap().total() > 0.0);
}

#[test]
fn pool_recycle_preserves_warm_capacity() {
    let rt = runtime_with_warm_pool(3);
    assert_eq!(rt.pool().warm_count(), 3);

    // Acquire, use, release, recycle
    let _s = rt.create_session("s1", BillingTier::Standard).unwrap();
    assert_eq!(rt.pool().warm_count(), 2);

    rt.destroy_session("s1").unwrap();
    // After destroy_session, the VM is released + recycled → back to warm
    assert_eq!(rt.pool().warm_count(), 3);

    // The recycled VM should be reusable
    let s2 = rt.create_session("s2", BillingTier::Standard).unwrap();
    assert!(!s2.vm_id.is_empty());
    assert_eq!(rt.pool().warm_count(), 2);
}

#[test]
fn concurrent_unikernel_sessions_are_independent() {
    let rt = runtime_with_warm_pool(4);

    let s1 = rt.create_session("inf-1", BillingTier::Premium).unwrap();
    let s2 = rt.create_session("inf-2", BillingTier::Premium).unwrap();
    let s3 = rt.create_session("inf-3", BillingTier::Standard).unwrap();

    // All get different VMs
    assert_ne!(s1.vm_id, s2.vm_id);
    assert_ne!(s2.vm_id, s3.vm_id);
    assert_ne!(s1.vm_id, s3.vm_id);

    // Record different billing for each
    rt.billing_engine()
        .record("inf-1", "gpu_minutes", 60.0)
        .unwrap();
    rt.billing_engine()
        .record("inf-2", "gpu_minutes", 120.0)
        .unwrap();
    rt.billing_engine()
        .record("inf-3", "cpu_seconds", 300.0)
        .unwrap();

    // Destroy in arbitrary order
    let inv2 = rt.destroy_session("inf-2").unwrap().unwrap();
    let inv1 = rt.destroy_session("inf-1").unwrap().unwrap();
    let inv3 = rt.destroy_session("inf-3").unwrap().unwrap();

    // Invoices should reflect respective usage
    assert!(inv2.total() >= inv1.total());
    assert!(inv1.total() > 0.0);
    assert!(inv3.total() > 0.0);
}

// ═══════════════════════════════════════════════════════════════════
//  4. GPU Topology + Boot Config Integration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn gpu_placement_feeds_into_boot_cmdline() {
    let mut topo = build_inference_topology();
    let req = GpuRequirements::default().count(1);
    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 1);

    let gpu_id = &placement.gpu_ids[0];
    topo.allocate(&placement.gpu_ids);

    // Build Linux boot with placed GPU in cmdline
    let params = LinuxBootParams {
        kernel_image: create_valid_bzimage(),
        initrd: None,
        cmdline: format!(
            "console=ttyS0 gpu_device={gpu_id} iommu=pt nvidia.NVreg_EnableGpuFirmware=1"
        ),
        // The e820 map the kernel reads is built from this.
        memory_size: 64 * 1024 * 1024,
        ..Default::default()
    };

    LinuxBootProtocol::validate_params(&params).unwrap();
    assert!(params.cmdline.contains(gpu_id));

    // Prepare memory and verify cmdline lands in regions
    let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();
    let cmdline_region = regions.iter().find(|(a, _)| *a == 0x91000).unwrap();
    let cmdline_str = String::from_utf8_lossy(&cmdline_region.1);
    assert!(cmdline_str.contains(gpu_id));

    topo.release(&placement.gpu_ids);
    assert_eq!(topo.available_count(), 2);
}

#[test]
fn multiboot_unikernel_with_gpu_config() {
    let mut topo = build_inference_topology();
    let placement = topo
        .find_placement(&GpuRequirements::default().count(1))
        .unwrap();
    topo.allocate(&placement.gpu_ids);

    let info = MultibootInfo {
        kernel_image: create_valid_multiboot_kernel(),
        modules: vec![MultibootModule {
            data: format!("gpu_device={}\nbatch_size=16", placement.gpu_ids[0]).into_bytes(),
            cmdline: "inference-config".to_string(),
        }],
        cmdline: format!("gpu=passthrough device={}", placement.gpu_ids[0]),
        memory_map: vec![(0, 640 * 1024), (1024 * 1024, 256 * 1024 * 1024)],
    };

    MultibootProtocol::validate_params(&info).unwrap();

    let mb_info =
        MultibootProtocol::create_multiboot_info(&info, 0x10000, 0x11000, Some(0x12000), 0x13000);
    assert!(!mb_info.is_empty());

    topo.release(&placement.gpu_ids);
}

// ═══════════════════════════════════════════════════════════════════
//  5. Capacity Management for Unikernel Fleets
// ═══════════════════════════════════════════════════════════════════

#[test]
fn capacity_reservation_for_inference_fleet() {
    let cm = CapacityManager::new();
    cm.register_class(
        VmClass::new("gpu-inference", SlaTier::Premium)
            .vcpus(4)
            .memory(16 * 1024 * 1024 * 1024)
            .gpus(1, "T4")
            .rate(0.50)
            .max(100),
    )
    .unwrap();

    // Reserve 10 instances for a tenant
    let rsv = cm
        .create_reservation("tenant-a", "gpu-inference", 10, Duration::from_secs(3600))
        .unwrap();

    // Consume one
    cm.consume_reservation(&rsv).unwrap();

    // Get status
    let info = cm.get_reservation(&rsv).unwrap();
    assert_eq!(info.instances_used, 1);

    cm.release_reservation(&rsv).unwrap();
}

// ═══════════════════════════════════════════════════════════════════
//  6. Maintenance & Auto-Scaling for Unikernel Pool
// ═══════════════════════════════════════════════════════════════════

#[test]
fn maintenance_tick_fills_warm_deficit() {
    let config = RuntimeConfig::builder()
        .pool(PoolConfig {
            min_warm: 4,
            max_size: 64,
            ..Default::default()
        })
        .instance_id("maintenance-test")
        .build();
    let rt = Runtime::new(config);

    assert_eq!(rt.pool().warm_count(), 0);
    let report = rt.maintenance_tick();
    assert!(report.vms_provisioned >= 4);
    assert!(rt.pool().warm_count() >= 4);
}

#[test]
fn maintenance_replaces_failed_vms() {
    let rt = runtime_with_warm_pool(3);
    assert_eq!(rt.pool().warm_count(), 3);

    // Simulate a VM failing (Warm → Assigned → Failed, since Warm → Failed is not valid)
    let victim = rt.pool().acquire("temp-session").unwrap();
    rt.pool().mark_failed(&victim, "GPU error").unwrap();

    // Before maintenance: 2 warm + 1 failed (one was acquired then failed)
    assert_eq!(rt.pool().warm_count(), 2);
    assert_eq!(rt.pool().stats().failed, 1);

    // Maintenance should terminate the failed VM and provision replacements
    let report = rt.maintenance_tick();
    assert!(report.vms_provisioned >= 1);
    assert!(rt.pool().warm_count() >= 3);
}

// ═══════════════════════════════════════════════════════════════════
//  7. Full E2E: Admit → Boot → Memory → Pool → GPU → Session → Recycle
// ═══════════════════════════════════════════════════════════════════

#[test]
fn full_unikernel_lifecycle_e2e() {
    // ── 1. Image Admission ──────────────────────────────────────
    let registry = trusted_registry();
    registry
        .register(signed_kernel_entry(
            "registry.internal/unikernels/llm-infer:2.0",
            "sha256:e2e0001",
        ))
        .unwrap();
    registry
        .approve("registry.internal/unikernels/llm-infer:2.0", "admin")
        .unwrap();
    assert_eq!(
        registry.check_admission("registry.internal/unikernels/llm-infer:2.0"),
        AdmissionDecision::Allowed
    );

    // ── 2. GPU Topology Placement ───────────────────────────────
    let mut topo = build_inference_topology();
    let placement = topo
        .find_placement(&GpuRequirements::default().count(1))
        .unwrap();
    topo.allocate(&placement.gpu_ids);
    let gpu_id = placement.gpu_ids[0].clone();

    // ── 3. Boot Protocol Configuration ──────────────────────────
    let params = LinuxBootParams {
        kernel_image: create_valid_bzimage(),
        initrd: Some(vec![0xDE; 2048]),
        cmdline: format!(
            "console=ttyS0 gpu_device={gpu_id} iommu=pt \
             inference_mode=continuous max_batch=32"
        ),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
        // The e820 map the kernel reads is built from this.
        memory_size: 64 * 1024 * 1024,
    };
    LinuxBootProtocol::validate_params(&params).unwrap();

    // ── 4. Guest Memory Setup ───────────────────────────────────
    let regions = LinuxBootProtocol::prepare_guest_memory(&params).unwrap();
    let mem = GuestMemory::new(64 * 1024 * 1024).unwrap();
    mem.allocate_region(64 * 1024 * 1024, false).unwrap(); // 0..64MB (must cover initrd at 32MB)

    for (addr, data) in &regions {
        mem.write_bytes(*addr, data).unwrap();
    }

    // Verify cmdline was written to guest memory
    let cmdline_region = regions.iter().find(|(a, _)| *a == 0x91000).unwrap();
    let readback = mem.read_bytes(0x91000, cmdline_region.1.len()).unwrap();
    assert_eq!(readback, cmdline_region.1);
    let cmdline = String::from_utf8_lossy(&readback);
    assert!(cmdline.contains(&gpu_id));
    assert!(cmdline.contains("inference_mode=continuous"));

    // Verify boot_params written
    let bp = mem.read_bytes(0x90000, 4096).unwrap();
    assert_eq!(bp.len(), 4096);

    // ── 5. Capacity Reservation ─────────────────────────────────
    let cm = CapacityManager::new();
    cm.register_class(
        VmClass::new("llm-inference", SlaTier::Premium)
            .vcpus(4)
            .memory(16 * 1024 * 1024 * 1024)
            .gpus(1, "T4")
            .rate(0.50)
            .max(50),
    )
    .unwrap();
    let rsv = cm
        .create_reservation("llm-tenant", "llm-inference", 5, Duration::from_secs(3600))
        .unwrap();
    cm.consume_reservation(&rsv).unwrap();

    // ── 6. Runtime Session ──────────────────────────────────────
    let rt = runtime_with_warm_pool(4);
    let session = rt
        .create_session("llm-e2e-session", BillingTier::Premium)
        .unwrap();
    assert!(!session.vm_id.is_empty());
    assert_eq!(session.tier, BillingTier::Premium);

    // Record GPU and CPU usage
    rt.billing_engine()
        .record("llm-e2e-session", "gpu_minutes", 60.0)
        .unwrap();
    rt.billing_engine()
        .record("llm-e2e-session", "cpu_seconds", 600.0)
        .unwrap();

    // GPU topology and capacity are available
    assert!(rt.gpu_topology().is_some());
    assert!(rt.capacity_manager().is_some());

    // ── 7. Session Teardown & Recycle ───────────────────────────
    let invoice = rt.destroy_session("llm-e2e-session").unwrap();
    assert!(invoice.is_some());
    let inv = invoice.unwrap();
    assert!(inv.total() > 0.0);

    // VM should be recycled back to warm
    assert_eq!(rt.pool().warm_count(), 4);

    // Pool can serve another session immediately
    let s2 = rt
        .create_session("llm-e2e-session-2", BillingTier::Standard)
        .unwrap();
    assert!(!s2.vm_id.is_empty());

    // ── 8. Cleanup ──────────────────────────────────────────────
    topo.release(&[gpu_id]);
    assert_eq!(topo.available_count(), 2);
    cm.release_reservation(&rsv).unwrap();
    rt.destroy_session("llm-e2e-session-2").unwrap();
}

// ═══════════════════════════════════════════════════════════════════
//  8. Error Paths
// ═══════════════════════════════════════════════════════════════════

#[test]
fn reject_invalid_kernel_image() {
    let params = LinuxBootParams {
        kernel_image: vec![0u8; 1024], // no valid header
        cmdline: "console=ttyS0".to_string(),
        // The e820 map the kernel reads is built from this.
        memory_size: 64 * 1024 * 1024,
        ..Default::default()
    };
    assert!(LinuxBootProtocol::validate_params(&params).is_err());

    let info = MultibootInfo {
        kernel_image: vec![0u8; 1024], // no multiboot header
        ..Default::default()
    };
    assert!(MultibootProtocol::validate_params(&info).is_err());
}

#[test]
fn reject_untrusted_kernel_from_registry() {
    let registry = trusted_registry();

    let bad_entry = ImageEntry::new(
        "registry.unknown/evil:latest",
        ImageKind::Kernel,
        "sha256:bad",
    )
    .signature(ImageSignature {
        signer: "untrusted-attacker".to_string(),
        algorithm: "ed25519".to_string(),
        signature_hex: "fake".to_string(),
        signed_at: std::time::SystemTime::now(),
        verified: true,
    });

    registry.register(bad_entry).unwrap();
    registry
        .approve("registry.unknown/evil:latest", "admin")
        .unwrap();

    let decision = registry.check_admission("registry.unknown/evil:latest");
    assert!(
        matches!(decision, AdmissionDecision::Denied(_)),
        "Untrusted signer must be denied"
    );
}

#[test]
fn session_fails_when_no_warm_vms() {
    let rt = Runtime::new(RuntimeConfig::default());
    let result = rt.create_session("orphan", BillingTier::Free);
    assert!(result.is_err());
}

#[test]
fn guest_memory_out_of_bounds_is_caught() {
    let mem = GuestMemory::new(1024 * 1024).unwrap();
    let addr = mem.allocate_region(64, false).unwrap();
    assert!(mem.write_bytes(addr + 64, &[1]).is_err());
    assert!(mem.read_bytes(addr + 64, 1).is_err());
}
