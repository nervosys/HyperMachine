//! Functional PCI checkpoint/restore gate using an owned Linux guest.
//! Set HV2_KERNEL and HV2_INITRD; no performance claim is made.
use hv2_agent::{AgentVM, Capability, CapabilitySet};
use std::{path::Path, time::Duration};

async fn build(kernel: &str, initrd: &str, name: &str) -> Result<AgentVM, String> {
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);
    let vm = AgentVM::builder()
        .name(name)
        .cpu_cores(1)
        .memory_gb(1)
        .capabilities(capabilities)
        .boot_linux(
            kernel,
            Some(initrd),
            "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0",
        )
        .build()
        .await
        .map_err(|e| e.to_string())?;
    vm.vm()
        .attach_vsock_pci(3)
        .await
        .map_err(|e| e.to_string())?;
    Ok(vm)
}

async fn exact(vm: &AgentVM, command: &str, expected: &str) -> Result<(), String> {
    let output = vm
        .exec_in_guest(
            "/bin/sh",
            &["-c".into(), command.into()],
            Duration::from_secs(15),
        )
        .await
        .map_err(|e| e.to_string())?;
    if output.exit_code != Some(0)
        || output.signal.is_some()
        || output.timed_out
        || output.stdout != expected
        || !output.stderr.is_empty()
    {
        return Err(format!("unexpected command response: {output:?}"));
    }
    Ok(())
}

async fn checkpoint(template: &AgentVM, path: &Path, marker: &str) -> Result<(), String> {
    template.launch().await.map_err(|e| e.to_string())?;
    template
        .ping_guest(Duration::from_secs(15))
        .await
        .map_err(|e| e.to_string())?;
    exact(
        template,
        &format!(
            "printf '%s' '{marker}' > /tmp/pci-checkpoint-marker; cat /tmp/pci-checkpoint-marker"
        ),
        marker,
    )
    .await?;
    println!("PCI_TEMPLATE_EXACT_COMMAND_PASS");
    template
        .snapshot_to(path)
        .await
        .map_err(|e| e.to_string())?;
    println!("PCI_SNAPSHOT_CAPTURE_PASS");
    Ok(())
}

async fn restored(vm: &AgentVM, path: &Path, marker: &str) -> Result<(), String> {
    vm.launch_from_snapshot(path)
        .await
        .map_err(|e| e.to_string())?;
    println!("PCI_SNAPSHOT_LAUNCH_PASS");
    for cycle in 0..16 {
        vm.ping_guest(Duration::from_secs(15))
            .await
            .map_err(|e| format!("restore cycle {cycle} ping: {e}"))?;
        exact(vm, "cat /tmp/pci-checkpoint-marker", marker).await?;
        let expected = format!("pci-restored-{cycle:04}");
        exact(vm, &format!("printf '%s' '{expected}'"), &expected).await?;
    }
    Ok(())
}

async fn run() -> Result<(), String> {
    let kernel = std::env::var("HV2_KERNEL").map_err(|e| e.to_string())?;
    let initrd = std::env::var("HV2_INITRD").map_err(|e| e.to_string())?;
    let path = std::env::temp_dir().join(format!("hm-pci-checkpoint-{}.snap", std::process::id()));
    let image = path.with_file_name(format!(
        "{}.mem",
        path.file_name().unwrap().to_string_lossy()
    ));
    if path.exists() || image.exists() {
        return Err("checkpoint output already exists".into());
    }
    let marker = format!("pci-checkpoint-persistent-{}", std::process::id());
    let template = build(&kernel, &initrd, "pci-checkpoint-template").await?;
    let capture = checkpoint(&template, &path, &marker).await;
    let stopped = template.stop().await.map_err(|e| e.to_string());
    let result = match capture.and(stopped) {
        Err(error) => Err(error),
        Ok(()) => match build(&kernel, &initrd, "pci-checkpoint-restored").await {
            Err(error) => Err(error),
            Ok(vm) => {
                let result = restored(&vm, &path, &marker).await;
                let stopped = vm.stop().await.map_err(|e| e.to_string());
                result.and(stopped)
            }
        },
    };
    for file in [&path, &image] {
        if file.exists() {
            std::fs::remove_file(file).map_err(|e| format!("cleanup {}: {e}", file.display()))?;
        }
    }
    println!("PCI_SNAPSHOT_OUTPUT_CLEANUP_PASS");
    result?;
    println!("{{\"pci_snapshot_probe\":true,\"restore_pings\":16,\"exact_restore_commands\":32,\"cleanup\":true}}");
    Ok(())
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PCI_SNAPSHOT_PROBE_FAILED: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
