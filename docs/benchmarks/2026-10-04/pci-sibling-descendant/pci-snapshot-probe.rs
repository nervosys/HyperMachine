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

async fn fork_gate(kernel: &str, initrd: &str, path: &Path, marker: &str) -> Result<(), String> {
    let nested = path.with_file_name(format!("hm-pci-descendant-{}.snap", std::process::id()));
    let nested_image = nested.with_file_name(format!(
        "{}.mem",
        nested.file_name().unwrap().to_string_lossy()
    ));
    if nested.exists() || nested_image.exists() {
        return Err("descendant checkpoint output already exists".into());
    }
    let mut guests = Vec::new();
    let result = async {
        let mut commands = 0;
        let mut pings = 0;
        macro_rules! checked {
            ($vm:expr, $command:expr, $expected:expr) => {{
                exact($vm, $command, $expected).await?;
                commands += 1;
            }};
        }
        for name in ["pci-fork-first", "pci-fork-second"] {
            guests.push(build(kernel, initrd, name).await?);
            restored(guests.last().unwrap(), path, marker).await?;
            pings += 16; commands += 32;
        }
        let first = &guests[0]; let second = &guests[1];
        let first_marker = format!("{marker}-first");
        let second_marker = format!("{marker}-second");
        checked!(first, &format!("printf '%s' '{first_marker}' > /tmp/pci-checkpoint-marker; cat /tmp/pci-checkpoint-marker"), &first_marker);
        checked!(second, "cat /tmp/pci-checkpoint-marker", marker);
        checked!(second, &format!("printf '%s' '{second_marker}' > /tmp/pci-checkpoint-marker; cat /tmp/pci-checkpoint-marker"), &second_marker);
        checked!(first, "cat /tmp/pci-checkpoint-marker", &first_marker);
        println!("PCI_FORK_INITIAL_ISOLATION_PASS");
        for cycle in 0..5 {
            first.pause().await.map_err(|e| e.to_string())?;
            checked!(second, "cat /tmp/pci-checkpoint-marker", &second_marker);
            first.resume().await.map_err(|e| e.to_string())?;
            first.ping_guest(Duration::from_secs(15)).await.map_err(|e| e.to_string())?;
            pings += 1;
            checked!(first, "cat /tmp/pci-checkpoint-marker", &first_marker);
            second.pause().await.map_err(|e| e.to_string())?;
            checked!(first, "cat /tmp/pci-checkpoint-marker", &first_marker);
            second.resume().await.map_err(|e| e.to_string())?;
            second.ping_guest(Duration::from_secs(15)).await.map_err(|e| e.to_string())?;
            pings += 1;
            checked!(second, "cat /tmp/pci-checkpoint-marker", &second_marker);
            println!("PCI_FORK_PAUSE_CYCLE_PASS {cycle}");
        }
        first.snapshot_to(&nested).await.map_err(|e| e.to_string())?;
        checked!(first, "cat /tmp/pci-checkpoint-marker", &first_marker);
        checked!(second, "cat /tmp/pci-checkpoint-marker", &second_marker);
        let changed_marker = format!("{first_marker}-changed-after-checkpoint");
        checked!(first, &format!("printf '%s' '{changed_marker}' > /tmp/pci-checkpoint-marker; cat /tmp/pci-checkpoint-marker"), &changed_marker);
        guests.push(build(kernel, initrd, "pci-fork-grandchild").await?);
        restored(&guests[2], &nested, &first_marker).await?;
        pings += 16; commands += 32;
        checked!(&guests[2], "cat /tmp/pci-checkpoint-marker", &first_marker);
        checked!(&guests[0], "cat /tmp/pci-checkpoint-marker", &changed_marker);
        checked!(&guests[1], "cat /tmp/pci-checkpoint-marker", &second_marker);
        println!("PCI_FORK_SECOND_GENERATION_ISOLATION_PASS");
        println!("PCI_FORK_OPERATIONS pings={pings} exact_commands={commands}");
        Ok::<_, String>(())
    }.await;
    let mut cleanup = Ok(());
    for vm in &guests {
        if let Err(error) = vm.stop().await {
            cleanup = Err(error.to_string());
        }
    }
    for file in [&nested, &nested_image] {
        if file.exists() {
            if let Err(error) = std::fs::remove_file(file) {
                cleanup = Err(format!("cleanup {}: {error}", file.display()));
            }
        }
    }
    result.and(cleanup)?;
    println!("{{\"pci_fork_probe\":true,\"restored_guests\":3,\"explicit_pause_resume_pairs\":10,\"cleanup\":true}}");
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
        Ok(()) if std::env::var_os("HV2_PCI_FORK_GATE").is_some() => {
            fork_gate(&kernel, &initrd, &path, &marker).await
        }
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
    if std::env::var_os("HV2_PCI_FORK_GATE").is_none() {
        println!("{{\"pci_snapshot_probe\":true,\"restore_pings\":16,\"exact_restore_commands\":32,\"cleanup\":true}}");
    }
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
