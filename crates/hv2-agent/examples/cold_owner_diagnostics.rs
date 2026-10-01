//! Investigate cold boot stalls using the daemon's AgentVM path, not an API benchmark.
use anyhow::{bail, Context, Result};
use hv2_agent::{AgentVM, Capability, CapabilitySet};
use hv2_core::BootSource;
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        bail!("usage: cold_owner_diagnostics KERNEL INITRD SAMPLES");
    }
    let samples: u32 = args[3].parse().context("invalid sample count")?;
    if !(1..=1000).contains(&samples) {
        bail!("samples must be 1-1000");
    }
    let fast_boot_args = BootSource::MICROVM_FAST_BOOT_ARGS;
    let cmdline =
        format!("console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=3 {fast_boot_args}");
    let mut failures = 0;
    for index in 0..samples {
        let mut capabilities = CapabilitySet::default();
        capabilities.add(Capability::GuestExec);
        let vm = AgentVM::builder()
            .name(format!("cold-owner-{index}"))
            .cpu_cores(1)
            .memory_mb(1024)
            .capabilities(capabilities)
            .boot_linux(&args[1], Some(&args[2]), &cmdline)
            .build()
            .await?;
        vm.attach_guest_channel(0x7800_0000 + u64::from(index))
            .await?;
        vm.launch().await?;
        let ready = vm.ping_guest(Duration::from_secs(15)).await;
        let mut diagnostic = json!(null);
        if ready.is_err() {
            failures += 1;
            diagnostic = match vm.vm().diagnostic_vcpu_states().await {
                Ok(states) => json!(states
                    .iter()
                    .map(|state| json!({
                        "id":state.id,"rip":format!("{:#x}",state.general.rip),
                        "rflags":format!("{:#x}",state.general.rflags),
                        "cr3":format!("{:#x}",state.system.cr3),"run_state":state.run_state
                    }))
                    .collect::<Vec<_>>()),
                Err(error) => json!({"error":error.to_string()}),
            };
        }
        // Cleanup even when readiness or diagnostic sampling failed.
        let cleanup = vm.stop().await;
        println!(
            "{}",
            json!({"sample":index,"ready":ready.is_ok(),
            "readiness_error":ready.err().map(|error|error.to_string()),
            "owner_diagnostic":diagnostic,"cleanup":cleanup.is_ok(),
            "cleanup_error":cleanup.as_ref().err().map(|error|error.to_string())})
        );
        cleanup?;
    }
    println!(
        "{}",
        json!({"summary":true,"samples":samples,"readiness_failures":failures,
        "diagnostic_only":true,"cpu_count":1,"memory_mb":1024,"cmdline":cmdline})
    );
    Ok(())
}
