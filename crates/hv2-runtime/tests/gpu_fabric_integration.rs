//! GPU VM Fabric Integration Tests
//!
//! End-to-end tests that exercise the four new GPU fabric subsystems
//! (topology, fleet, capacity, image registry) both individually and
//! in cross-module workflows. These validate the full fabric stack
//! needed for GPU-accelerated AI workloads.

use std::time::Duration;

use hv2_core::security::{
    AdmissionDecision, EnforcementMode, ImageEntry, ImageKind, ImageRegistry, ImageSignature,
    RegistryConfig,
};
use hv2_runtime::{
    // Fleet
    ArtifactKind,
    // Capacity
    CapacityManager,
    FleetHost,
    FleetManager,
    // Topology
    GpuDevice,
    GpuInterconnect,
    GpuRequirements,
    GpuTopologyMap,
    PoolConfig,
    RolloutStrategy,
    // Core runtime
    Runtime,
    RuntimeConfig,
    SlaTier,
    VmClass,
};

// ═══════════════════════════════════════════════════════════════════
//  Runtime Integration with GPU Fabric
// ═══════════════════════════════════════════════════════════════════

#[test]
fn runtime_with_gpu_fabric_enabled() {
    let config = RuntimeConfig::builder()
        .instance_id("gpu-fabric-test")
        .pool(PoolConfig {
            min_warm: 2,
            max_size: 16,
            ..Default::default()
        })
        .gpu_topology(true)
        .fleet_management(true)
        .capacity_reservations(true)
        .build();

    let rt = Runtime::new(config);

    assert!(rt.gpu_topology().is_some());
    assert!(rt.fleet_manager().is_some());
    assert!(rt.capacity_manager().is_some());
    assert_eq!(rt.instance_id(), "gpu-fabric-test");
}

#[test]
fn runtime_with_gpu_fabric_disabled() {
    let config = RuntimeConfig::builder().instance_id("no-gpu-test").build();

    let rt = Runtime::new(config);

    assert!(rt.gpu_topology().is_none());
    assert!(rt.fleet_manager().is_none());
    assert!(rt.capacity_manager().is_none());
}

#[test]
fn runtime_partial_gpu_fabric() {
    let config = RuntimeConfig::builder()
        .gpu_topology(true)
        .fleet_management(false)
        .capacity_reservations(true)
        .build();

    let rt = Runtime::new(config);

    assert!(rt.gpu_topology().is_some());
    assert!(rt.fleet_manager().is_none());
    assert!(rt.capacity_manager().is_some());
}

// ═══════════════════════════════════════════════════════════════════
//  GPU Topology: DGX-style Cluster
// ═══════════════════════════════════════════════════════════════════

/// Build a DGX A100-style topology: 8 GPUs on one host with NVSwitch
fn build_dgx_topology() -> GpuTopologyMap {
    let mut topo = GpuTopologyMap::new();

    // 8 x A100 80GB on node-0, two NUMA domains
    for i in 0..8u32 {
        let dev = GpuDevice::new(format!("gpu-{i}"), "dgx-node-0", "A100-SXM4-80GB")
            .numa(i / 4)
            .pci(format!("0000:{:02x}:00.0", i))
            .vram(80 * 1024 * 1024 * 1024)
            .capability(80);
        topo.add_device(dev);
    }

    // Full NVSwitch mesh among all 8 GPUs
    for i in 0..8u32 {
        for j in (i + 1)..8 {
            topo.add_link(
                &format!("gpu-{i}"),
                &format!("gpu-{j}"),
                GpuInterconnect::NvSwitch,
                12,
            );
        }
    }

    topo
}

#[test]
fn topology_dgx_device_registration() {
    let topo = build_dgx_topology();
    assert_eq!(topo.device_count(), 8);
    assert_eq!(topo.available_count(), 8);
    assert_eq!(topo.hosts(), vec!["dgx-node-0"]);
}

#[test]
fn topology_find_4gpu_nvswitch_placement() {
    let topo = build_dgx_topology();

    let req = GpuRequirements::default()
        .count(4)
        .min_vram(80 * 1024 * 1024 * 1024)
        .nvlink();

    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 4);
    assert_eq!(placement.host_id, "dgx-node-0");
    assert!(
        placement.affinity_score > 0.8,
        "NVSwitch should yield high affinity"
    );
    assert!(placement.aggregate_bandwidth_gbps > 100.0);
}

#[test]
fn topology_allocate_and_release() {
    let mut topo = build_dgx_topology();

    // Allocate 4 GPUs
    let req = GpuRequirements::default().count(4);
    let placement = topo.find_placement(&req).unwrap();
    let gpu_ids = placement.gpu_ids.clone();
    topo.allocate(&gpu_ids);

    assert_eq!(topo.available_count(), 4);

    // Second 4-GPU request should still succeed
    let placement2 = topo.find_placement(&req).unwrap();
    assert_eq!(placement2.gpu_ids.len(), 4);
    // No overlap
    for id in &placement2.gpu_ids {
        assert!(!gpu_ids.contains(id));
    }

    // Release first batch
    topo.release(&gpu_ids);
    assert_eq!(topo.available_count(), 8);
}

#[test]
fn topology_insufficient_gpus() {
    let topo = build_dgx_topology();

    let req = GpuRequirements::default().count(16);
    let err = topo.find_placement(&req).unwrap_err();
    assert!(format!("{err}").contains("Insufficient GPUs"));
}

#[test]
fn topology_same_numa_constraint() {
    let topo = build_dgx_topology();

    // Require all GPUs on the same NUMA node (4 GPUs each on NUMA 0 and 1)
    let req = GpuRequirements::default().count(4).same_numa_node();

    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 4);
    assert!(placement.same_numa);
}

// ═══════════════════════════════════════════════════════════════════
//  Multi-Host Topology
// ═══════════════════════════════════════════════════════════════════

fn build_multi_host_topology() -> GpuTopologyMap {
    let mut topo = GpuTopologyMap::new();

    // Host A: 4 x A100 with NVLink pairs
    for i in 0..4u32 {
        let dev = GpuDevice::new(format!("a-gpu-{i}"), "host-a", "A100-PCIe-40GB")
            .numa(0)
            .vram(40 * 1024 * 1024 * 1024)
            .capability(80);
        topo.add_device(dev);
    }
    // NVLink pairs on host A: (0,1) and (2,3)
    topo.add_link("a-gpu-0", "a-gpu-1", GpuInterconnect::NvLink, 6);
    topo.add_link("a-gpu-2", "a-gpu-3", GpuInterconnect::NvLink, 6);
    topo.add_link("a-gpu-0", "a-gpu-2", GpuInterconnect::PciePeer, 1);
    topo.add_link("a-gpu-1", "a-gpu-3", GpuInterconnect::PciePeer, 1);

    // Host B: 2 x H100 with NVSwitch
    for i in 0..2u32 {
        let dev = GpuDevice::new(format!("b-gpu-{i}"), "host-b", "H100-SXM5-80GB")
            .numa(0)
            .vram(80 * 1024 * 1024 * 1024)
            .capability(90);
        topo.add_device(dev);
    }
    topo.add_link("b-gpu-0", "b-gpu-1", GpuInterconnect::NvSwitch, 18);

    topo
}

#[test]
fn multi_host_prefers_best_interconnect() {
    let topo = build_multi_host_topology();

    // Request 2 GPUs with NVLink — host B has NVSwitch which satisfies NVLink req
    let req = GpuRequirements::default().count(2).nvlink();

    let placement = topo.find_placement(&req).unwrap();
    // Should prefer host-b's NVSwitch pair (higher affinity)
    assert_eq!(placement.gpu_ids.len(), 2);
    assert!(placement.affinity_score >= 0.9);
}

// ═══════════════════════════════════════════════════════════════════
//  Fleet Lifecycle Management
// ═══════════════════════════════════════════════════════════════════

fn build_test_fleet() -> FleetManager {
    let fm = FleetManager::new();

    // Register 3 hosts with GPU tags
    fm.register_host(
        FleetHost::new("host-a")
            .tag("gpu", "a100")
            .tag("region", "us-west-2")
            .installed(ArtifactKind::GpuDriver, "550.90.07"),
    );
    fm.register_host(
        FleetHost::new("host-b")
            .tag("gpu", "h100")
            .tag("region", "us-west-2")
            .installed(ArtifactKind::GpuDriver, "550.90.07"),
    );
    fm.register_host(
        FleetHost::new("host-c")
            .tag("gpu", "a100")
            .tag("region", "eu-west-1")
            .installed(ArtifactKind::GpuDriver, "550.90.07"),
    );

    fm
}

#[test]
fn fleet_host_registration() {
    let fm = build_test_fleet();
    assert_eq!(fm.host_count(), 3);
    assert!(fm.get_host("host-a").is_some());
    assert!(fm.get_host("host-d").is_none());
}

#[test]
fn fleet_host_tag_filtering() {
    let fm = build_test_fleet();

    let a100_hosts = fm.hosts_with_tag("gpu", "a100");
    assert_eq!(a100_hosts.len(), 2);

    let eu_hosts = fm.hosts_with_tag("region", "eu-west-1");
    assert_eq!(eu_hosts.len(), 1);
    assert_eq!(eu_hosts[0].id, "host-c");
}

#[test]
fn fleet_artifact_publish_and_lookup() {
    let fm = build_test_fleet();

    use hv2_runtime::fleet::ArtifactVersion;

    let artifact = ArtifactVersion::new(ArtifactKind::GpuDriver, "555.42.02", "abc123def456");
    fm.publish_artifact(artifact);

    assert!(fm
        .get_artifact(&ArtifactKind::GpuDriver, "555.42.02")
        .is_some());
    assert!(fm
        .get_artifact(&ArtifactKind::GpuDriver, "999.99.99")
        .is_none());
}

#[test]
fn fleet_rolling_rollout() {
    let fm = build_test_fleet();

    // Publish target version
    use hv2_runtime::fleet::{ArtifactVersion, RolloutConfig};

    fm.publish_artifact(ArtifactVersion::new(
        ArtifactKind::GpuDriver,
        "555.42.02",
        "abc123",
    ));

    // Create a rolling rollout targeting all hosts
    let host_ids: Vec<String> = fm.list_hosts().iter().map(|h| h.id.clone()).collect();
    let rollout_config = RolloutConfig {
        strategy: RolloutStrategy::Rolling,
        max_concurrent: 2,
        ..Default::default()
    };

    let rollout_id = fm
        .create_rollout(
            ArtifactKind::GpuDriver,
            "555.42.02",
            Some(host_ids),
            rollout_config,
        )
        .unwrap();

    // Advance the rollout
    let result = fm.advance_rollout(&rollout_id);
    assert!(result.is_ok());

    // Should be able to get rollout status
    let rollout = fm.get_rollout(&rollout_id).unwrap();
    assert_eq!(rollout.target_version, "555.42.02");
    assert_eq!(rollout.hosts.len(), 3);
}

#[test]
fn fleet_canary_rollout() {
    let fm = build_test_fleet();

    use hv2_runtime::fleet::{ArtifactVersion, RolloutConfig};

    fm.publish_artifact(ArtifactVersion::new(
        ArtifactKind::CudaToolkit,
        "12.4.0",
        "def456",
    ));

    let host_ids: Vec<String> = fm.list_hosts().iter().map(|h| h.id.clone()).collect();
    let rollout_config = RolloutConfig {
        strategy: RolloutStrategy::Canary,
        canary_count: 1,
        max_concurrent: 1,
        ..Default::default()
    };

    let rollout_id = fm
        .create_rollout(
            ArtifactKind::CudaToolkit,
            "12.4.0",
            Some(host_ids),
            rollout_config,
        )
        .unwrap();

    let rollout = fm.get_rollout(&rollout_id).unwrap();
    assert_eq!(rollout.target_version, "12.4.0");
}

#[test]
fn fleet_host_unregister() {
    let fm = build_test_fleet();
    assert_eq!(fm.host_count(), 3);

    fm.unregister_host("host-b");
    assert_eq!(fm.host_count(), 2);
    assert!(fm.get_host("host-b").is_none());
}

// ═══════════════════════════════════════════════════════════════════
//  Capacity Reservations
// ═══════════════════════════════════════════════════════════════════

fn build_capacity_manager() -> CapacityManager {
    let cm = CapacityManager::new();

    // GPU VM classes
    cm.register_class(
        VmClass::new("gpu-a100-1x", SlaTier::Standard)
            .vcpus(8)
            .memory(32 * 1024 * 1024 * 1024)
            .gpus(1, "A100")
            .rate(3.06)
            .max(100),
    )
    .unwrap();

    cm.register_class(
        VmClass::new("gpu-a100-4x-premium", SlaTier::Premium)
            .vcpus(48)
            .memory(192 * 1024 * 1024 * 1024)
            .gpus(4, "A100")
            .rate(12.24)
            .max(25)
            .dedicated(),
    )
    .unwrap();

    cm.register_class(
        VmClass::new("gpu-h100-8x-dedicated", SlaTier::Dedicated)
            .vcpus(192)
            .memory(2048_u64 * 1024 * 1024 * 1024)
            .gpus(8, "H100")
            .rate(65.98)
            .max(10)
            .dedicated(),
    )
    .unwrap();

    cm
}

#[test]
fn capacity_class_registration() {
    let cm = build_capacity_manager();
    let classes = cm.list_classes();
    assert_eq!(classes.len(), 3);

    let a100 = cm.get_class("gpu-a100-1x").unwrap();
    assert_eq!(a100.gpu_count, 1);
    assert_eq!(a100.sla_tier, SlaTier::Standard);
    assert!(!a100.dedicated_host);
}

#[test]
fn capacity_duplicate_class_rejected() {
    let cm = build_capacity_manager();
    let result = cm.register_class(VmClass::new("gpu-a100-1x", SlaTier::BestEffort));
    assert!(result.is_err());
}

#[test]
fn capacity_reservation_lifecycle() {
    let cm = build_capacity_manager();

    // Create a reservation for 5 x gpu-a100-1x instances for 1 hour
    let rsv_id = cm
        .create_reservation("tenant-acme", "gpu-a100-1x", 5, Duration::from_secs(3600))
        .unwrap();

    let rsv = cm.get_reservation(&rsv_id).unwrap();
    assert_eq!(rsv.tenant_id, "tenant-acme");
    assert_eq!(rsv.vm_class, "gpu-a100-1x");
    assert_eq!(rsv.instance_count, 5);
    assert_eq!(rsv.instances_used, 0);
    assert!(rsv.has_available());

    // Consume 3 instances (one at a time — API consumes a single slot)
    cm.consume_reservation(&rsv_id).unwrap();
    cm.consume_reservation(&rsv_id).unwrap();
    cm.consume_reservation(&rsv_id).unwrap();
    let rsv = cm.get_reservation(&rsv_id).unwrap();
    assert_eq!(rsv.instances_used, 3);
    assert_eq!(rsv.available(), 2);

    // Release 1 instance
    cm.release_reservation(&rsv_id).unwrap();
    let rsv = cm.get_reservation(&rsv_id).unwrap();
    assert_eq!(rsv.instances_used, 2);

    // Cancel the reservation
    cm.cancel_reservation(&rsv_id).unwrap();
    let rsv = cm.get_reservation(&rsv_id).unwrap();
    assert_eq!(rsv.state, hv2_runtime::ReservationState::Cancelled);
}

#[test]
fn capacity_reservation_for_invalid_class() {
    let cm = build_capacity_manager();
    let result = cm.create_reservation("tenant-x", "nonexistent-class", 1, Duration::from_secs(60));
    assert!(result.is_err());
}

#[test]
fn capacity_sla_tier_properties() {
    assert!(SlaTier::BestEffort.preemptible());
    assert!(!SlaTier::Standard.preemptible());
    assert!(!SlaTier::Premium.preemptible());
    assert!(!SlaTier::Dedicated.preemptible());

    assert!(SlaTier::Dedicated.priority_boost() > SlaTier::Premium.priority_boost());
    assert!(SlaTier::Premium.priority_boost() > SlaTier::Standard.priority_boost());
    assert!(SlaTier::Standard.priority_boost() > SlaTier::BestEffort.priority_boost());

    assert!(SlaTier::Dedicated.target_availability() > SlaTier::Premium.target_availability());
}

#[test]
fn capacity_class_instance_tracking() {
    let cm = build_capacity_manager();

    // Should start with zero active instances
    let cls = cm.get_class("gpu-a100-1x").unwrap();
    assert_eq!(cls.active_instances, 0);
    assert!(cls.has_capacity());

    // Increment usage
    cm.increment_class_usage("gpu-a100-1x").unwrap();
    cm.increment_class_usage("gpu-a100-1x").unwrap();

    let cls = cm.get_class("gpu-a100-1x").unwrap();
    assert_eq!(cls.active_instances, 2);

    // Decrement
    cm.decrement_class_usage("gpu-a100-1x").unwrap();
    let cls = cm.get_class("gpu-a100-1x").unwrap();
    assert_eq!(cls.active_instances, 1);
}

// ═══════════════════════════════════════════════════════════════════
//  Image Allowlist Registry
// ═══════════════════════════════════════════════════════════════════

fn build_image_registry() -> ImageRegistry {
    let config = RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: vec!["nervosys-release-key".to_string()],
    };
    let registry = ImageRegistry::new(config);

    // Register a GPU driver image (pending review)
    let entry = ImageEntry::new(
        "registry.internal/drivers/nvidia:550.90.07",
        ImageKind::Custom("gpu-driver".into()),
        "sha256:abcdef0123456789",
    )
    .label("gpu", "nvidia")
    .label("driver_version", "550.90.07")
    .signature(ImageSignature {
        signer: "nervosys-release-key".to_string(),
        algorithm: "ed25519".to_string(),
        signature_hex: "deadbeef".to_string(),
        signed_at: std::time::SystemTime::now(),
        verified: true,
    });

    registry.register(entry).unwrap();

    // Register a kernel image
    let kernel = ImageEntry::new(
        "registry.internal/kernels/unikernel-ai:1.2.0",
        ImageKind::Kernel,
        "sha256:kernelhash123",
    )
    .label("purpose", "ai-inference")
    .signature(ImageSignature {
        signer: "nervosys-release-key".to_string(),
        algorithm: "ed25519".to_string(),
        signature_hex: "cafebabe".to_string(),
        signed_at: std::time::SystemTime::now(),
        verified: true,
    });

    registry.register(kernel).unwrap();

    registry
}

#[test]
fn image_registry_register_and_approve() {
    let registry = build_image_registry();

    // Approve the GPU driver image
    registry
        .approve(
            "registry.internal/drivers/nvidia:550.90.07",
            "admin@nervosys",
        )
        .unwrap();

    let decision = registry.check_admission("registry.internal/drivers/nvidia:550.90.07");
    assert_eq!(decision, AdmissionDecision::Allowed);
}

#[test]
fn image_registry_deny_unsigned() {
    let config = RegistryConfig {
        mode: EnforcementMode::Enforce,
        require_signature: true,
        trusted_signers: vec![],
    };
    let registry = ImageRegistry::new(config);

    // Register an unsigned image
    let entry = ImageEntry::new(
        "registry.internal/untrusted:latest",
        ImageKind::VmImage,
        "sha256:unsigned123",
    );
    registry.register(entry).unwrap();

    // Attempt to approve should fail (no verified signature)
    let result = registry.approve("registry.internal/untrusted:latest", "admin");
    assert!(result.is_err());
}

#[test]
fn image_registry_deny_revoked() {
    let registry = build_image_registry();

    // Approve then revoke
    registry
        .approve("registry.internal/drivers/nvidia:550.90.07", "admin")
        .unwrap();
    registry
        .revoke(
            "registry.internal/drivers/nvidia:550.90.07",
            "security-team",
            "CVE found",
        )
        .unwrap();

    let decision = registry.check_admission("registry.internal/drivers/nvidia:550.90.07");
    assert!(matches!(decision, AdmissionDecision::Denied(_)));
}

#[test]
fn image_registry_deprecated_warning() {
    let registry = build_image_registry();

    registry
        .approve("registry.internal/drivers/nvidia:550.90.07", "admin")
        .unwrap();
    registry
        .deprecate(
            "registry.internal/drivers/nvidia:550.90.07",
            "admin",
            "use v555 instead",
        )
        .unwrap();

    let decision = registry.check_admission("registry.internal/drivers/nvidia:550.90.07");
    assert!(matches!(decision, AdmissionDecision::AllowedWithWarning(_)));
}

#[test]
fn image_registry_enforcement_disabled() {
    let config = RegistryConfig {
        mode: EnforcementMode::Disabled,
        require_signature: false,
        trusted_signers: vec![],
    };
    let registry = ImageRegistry::new(config);

    // Unknown images should be allowed when enforcement is disabled
    let decision = registry.check_admission("anything:latest");
    assert_eq!(decision, AdmissionDecision::Allowed);
}

#[test]
fn image_registry_list_by_kind() {
    let registry = build_image_registry();

    let kernels = registry.list_by_kind(&ImageKind::Kernel);
    assert_eq!(kernels.len(), 1);
    assert!(kernels[0].reference.contains("unikernel-ai"));
}

// ═══════════════════════════════════════════════════════════════════
//  Cross-Module: GPU Fabric Pipeline
// ═══════════════════════════════════════════════════════════════════

/// End-to-end test: image admission → capacity reservation → topology
/// placement → fleet rollout prerequisite check.
#[test]
fn full_gpu_fabric_pipeline() {
    // 1. Image admission: approve the AI kernel
    let registry = build_image_registry();
    registry
        .approve(
            "registry.internal/kernels/unikernel-ai:1.2.0",
            "admin@nervosys",
        )
        .unwrap();
    let decision = registry.check_admission("registry.internal/kernels/unikernel-ai:1.2.0");
    assert_eq!(decision, AdmissionDecision::Allowed);

    // 2. Capacity: reserve GPU resources
    let cm = build_capacity_manager();
    let rsv_id = cm
        .create_reservation(
            "tenant-ml-training",
            "gpu-a100-4x-premium",
            2,
            Duration::from_secs(7200),
        )
        .unwrap();
    cm.consume_reservation(&rsv_id).unwrap();

    let rsv = cm.get_reservation(&rsv_id).unwrap();
    assert_eq!(rsv.instances_used, 1);
    assert!(rsv.has_available());

    // 3. Topology: find GPUs for the workload
    let mut topo = build_dgx_topology();
    let req = GpuRequirements::default()
        .count(4)
        .min_vram(80 * 1024 * 1024 * 1024)
        .nvlink();

    let placement = topo.find_placement(&req).unwrap();
    assert_eq!(placement.gpu_ids.len(), 4);
    topo.allocate(&placement.gpu_ids);

    // 4. Fleet: verify hosts have correct driver version
    let fm = build_test_fleet();
    let host = fm.get_host("host-a").unwrap();
    let driver_version = host
        .installed_versions
        .get(&ArtifactKind::GpuDriver)
        .unwrap();
    assert_eq!(driver_version, "550.90.07");

    // 5. Cleanup: release GPUs and reservation
    topo.release(&placement.gpu_ids);
    assert_eq!(topo.available_count(), 8);

    cm.release_reservation(&rsv_id).unwrap();
    cm.cancel_reservation(&rsv_id).unwrap();
}

/// Test: Runtime with all fabric features enabled can exercise
/// session lifecycle alongside GPU management.
#[test]
fn runtime_session_with_gpu_fabric() {
    let config = RuntimeConfig::builder()
        .instance_id("fabric-session-test")
        .pool(PoolConfig {
            min_warm: 4,
            max_size: 32,
            ..Default::default()
        })
        .gpu_topology(true)
        .fleet_management(true)
        .capacity_reservations(true)
        .build();

    let rt = Runtime::new(config);

    // Ensure pool is warmed up
    rt.maintenance_tick();

    // Create a session (standard runtime path)
    let session = rt
        .create_session("gpu-agent-1", hv2_runtime::BillingTier::Premium)
        .unwrap();
    assert!(!session.vm_id.is_empty());

    // GPU topology is available for querying
    let topo = rt.gpu_topology().unwrap();
    assert_eq!(topo.device_count(), 0); // Empty until devices registered

    // Fleet manager is available
    let fm = rt.fleet_manager().unwrap();
    assert_eq!(fm.host_count(), 0); // Empty until hosts registered

    // Capacity manager is available
    let cm = rt.capacity_manager().unwrap();
    assert_eq!(cm.list_classes().len(), 0);

    // Destroy session
    let invoice = rt.destroy_session("gpu-agent-1").unwrap();
    assert!(invoice.is_some());
}
