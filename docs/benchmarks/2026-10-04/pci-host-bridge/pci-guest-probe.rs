//! Verify exact commands through an owned Linux guest's PCI vsock transport.
//! Set HV2_KERNEL and HV2_INITRD. This is a functional gate, not a benchmark.
use hv2_agent::{AgentVM, Capability, CapabilitySet};
use std::time::Duration;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PCI_GUEST_PROBE_FAILED: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let kernel = std::env::var("HV2_KERNEL").map_err(|e| e.to_string())?;
    let initrd = std::env::var("HV2_INITRD").map_err(|e| e.to_string())?;
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);
    let diagnostic = std::env::var_os("HV2_PCI_DIAGNOSTICS").is_some();
    let cmdline = if diagnostic {
        "console=ttyS0,115200 nokaslr rdinit=/init loglevel=7"
    } else {
        "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0"
    };
    let vm = AgentVM::builder()
        .name("pci-guest-probe")
        .cpu_cores(1)
        .memory_gb(1)
        .capabilities(capabilities)
        .boot_linux(&kernel, Some(&initrd), cmdline)
        .build()
        .await
        .map_err(|e| e.to_string())?;
    let result = async {
        vm.vm()
            .attach_vsock_pci(3)
            .await
            .map_err(|e| e.to_string())?;
        vm.launch().await.map_err(|e| e.to_string())?;
        for cycle in 0..64 {
            vm.ping_guest(Duration::from_secs(15))
                .await
                .map_err(|e| e.to_string())?;
            let expected = format!("pci-guest-exact-{}-{cycle:04}", std::process::id());
            let output = vm
                .exec_in_guest(
                    "/bin/sh",
                    &["-c".into(), format!("printf '%s' '{expected}'")],
                    Duration::from_secs(15),
                )
                .await
                .map_err(|e| e.to_string())?;
            if output.exit_code != Some(0)
                || output.timed_out
                || output.signal.is_some()
                || output.stdout != expected
                || !output.stderr.is_empty()
            {
                return Err(format!("cycle {cycle} command response mismatch"));
            }
        }
        Ok(())
    }
    .await;
    if result.is_err() && diagnostic {
        let console = vm.vm().console_output().await;
        eprintln!("PCI_GUEST_CONSOLE_BEGIN\n{console}\nPCI_GUEST_CONSOLE_END");
    }
    // Always stop the launched guest, including an unanswered agent or command.
    let stopped = vm.stop().await.map_err(|e| e.to_string());
    result?;
    stopped?;
    println!("{{\"pci_guest_probe\":true,\"pings\":64,\"exact_commands\":64,\"vm_stopped\":true}}");
    Ok(())
}
