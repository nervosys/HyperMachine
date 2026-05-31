//! Example: reserving GPU capacity with SLA tiers (the GPU fabric).
//!
//! Operators publish *VM classes* (GPU SKUs); tenants reserve guaranteed blocks
//! of capacity at an SLA tier. Reservations protect premium/training workloads
//! from best-effort contention and drive topology-aware placement. This is the
//! engine behind the GPU-fabric REST API in `hv2-api`.
//!
//! Run with:
//! ```bash
//! cargo run -p hv2-runtime --example gpu_fabric_reservation
//! ```

use hv2_runtime::{CapacityManager, SlaTier, VmClass};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🛰️  HyperMachine — GPU-fabric capacity reservation\n{}",
        "=".repeat(60)
    );

    let mgr = CapacityManager::new();

    // 1. Operator publishes a GPU SKU: 8× A100, premium SLA, fleet-capped.
    println!("\n[publish class]");
    let class = VmClass::new("gpu-a100-8x-premium", SlaTier::Premium)
        .description("8x NVIDIA A100 80GB, NVLink")
        .vcpus(96)
        .memory(640 * 1024 * 1024 * 1024)
        .gpus(8, "A100-80GB")
        .rate(32.77)
        .max(16);
    mgr.register_class(class)?;
    println!("  gpu-a100-8x-premium — 8×A100, premium SLA, max 16 instances fleet-wide");

    // 2. A tenant reserves 4 instances for a 7-day training run.
    println!("\n[reserve]");
    let rsv = mgr.create_reservation(
        "tenant-acme",
        "gpu-a100-8x-premium",
        4,
        Duration::from_secs(7 * 24 * 3600),
    )?;
    println!("  {rsv}: 4 instances reserved for tenant-acme (7 days)");

    // 3. Launch two jobs against the reservation.
    println!("\n[consume]");
    mgr.consume_reservation(&rsv)?;
    mgr.consume_reservation(&rsv)?;
    let r = mgr.get_reservation(&rsv).expect("reservation exists");
    println!(
        "  {} — used {}/{}, {} still available",
        r.id,
        r.instances_used,
        r.instance_count,
        r.available()
    );

    // 4. Reserving an unknown class is rejected (capacity safety).
    println!("\n[guardrail]");
    match mgr.create_reservation("tenant-acme", "nonexistent-sku", 1, Duration::from_secs(60)) {
        Err(e) => println!("  unknown-class reservation correctly rejected: {e}"),
        Ok(_) => panic!("expected rejection for unknown class"),
    }

    // 5. Jobs finish — release their slots back to the reservation, then the
    //    tenant cancels the reservation entirely.
    println!("\n[release slots + cancel]");
    mgr.release_reservation(&rsv)?;
    mgr.release_reservation(&rsv)?;
    let r = mgr.get_reservation(&rsv).expect("reservation exists");
    println!(
        "  jobs done — used {}/{}",
        r.instances_used, r.instance_count
    );
    println!(
        "  active reservations before cancel: {}",
        mgr.active_reservations().len()
    );
    mgr.cancel_reservation(&rsv)?;
    println!(
        "  active reservations after cancel:  {}",
        mgr.active_reservations().len()
    );

    // SLA tiers drive scheduling priority and preemption.
    println!("\n[sla tiers]");
    for tier in [
        SlaTier::BestEffort,
        SlaTier::Standard,
        SlaTier::Premium,
        SlaTier::Dedicated,
    ] {
        println!(
            "  {:<10?} {:>6.2}% avail  +{:<3} priority  preemptible={}",
            tier,
            tier.target_availability(),
            tier.priority_boost(),
            tier.preemptible()
        );
    }

    println!("\n✅ GPU-fabric reservation complete.");
    Ok(())
}
