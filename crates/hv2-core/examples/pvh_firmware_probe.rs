//! Enter a firmware image by the PVH boot protocol and report what it said.
//!
//! `linux_boot_probe` judges the Linux boot protocol with a real kernel; this
//! does the same for [`BootSource::Pvh`] with real firmware. Pass an ELF with
//! a PVH note, such as Rust Hypervisor Firmware's `hypervisor-fw`:
//!
//! ```sh
//! cargo run -p hv2-core --example pvh_firmware_probe -- /path/to/hypervisor-fw
//! ```
//!
//! With no disk attached the firmware has nothing to boot, and what it prints
//! on COM1 before giving up shows it was entered where its note said, found
//! `hvm_start_info` through `EBX`, and read the memory map from it.
//!
//! `HV2_DISK=<raw image>` attaches a disk over PCI, which the firmware boots
//! from if it holds an EFI system partition. `HV2_DISK_RO=1` makes it
//! read-only. `HV2_SETTLE_SECS` is how long to let the guest run, and
//! `HV2_UNTIL=<text>` stops as soon as the console contains that text.

use std::sync::Arc;
use std::time::Duration;

use hv2_core::machine::Machine;
use hv2_core::{BootSource, HypervisorPlatform, VMConfig, VM};

/// Long enough for firmware to print its banner and give up on booting.
const SETTLE: Duration = Duration::from_secs(3);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let Some(firmware) = std::env::args().nth(1) else {
        eprintln!("usage: pvh_firmware_probe <firmware.elf>");
        std::process::exit(2);
    };
    let firmware = std::path::PathBuf::from(firmware);
    println!("platform      : {:?}", HypervisorPlatform::detect());
    println!(
        "firmware      : {} ({} bytes)",
        firmware.display(),
        std::fs::metadata(&firmware).map(|m| m.len()).unwrap_or(0)
    );

    let memory_mib = std::env::var("HV2_MEMORY_MIB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(512);
    let config = VMConfig {
        name: "pvh-firmware-probe".to_string(),
        vcpu_count: 1,
        memory_size: memory_mib * 1024 * 1024,
        boot: Some(BootSource::pvh(&firmware)),
        ..Default::default()
    };
    let vm = match VM::new(config) {
        Ok(vm) => Arc::new(vm),
        Err(e) => {
            println!("VM::new       : FAILED — {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = Machine::legacy_pc_with_pci_root(vm.pci_root())
        .attach(&vm.devices())
        .await
    {
        println!("devices       : FAILED — {e}");
        std::process::exit(1);
    }
    // A disk over PCI, when given one: firmware finds its disk by walking the
    // bus, so this is what turns "it started" into "it found something".
    if let Ok(disk) = std::env::var("HV2_DISK") {
        match vm
            .attach_block_pci(
                std::path::Path::new(&disk),
                std::env::var("HV2_DISK_RO").is_ok(),
                "probe",
            )
            .await
        {
            Ok(_) => println!("disk          : {disk} (read-only, over PCI)"),
            Err(e) => {
                println!("disk          : FAILED — {e}");
                std::process::exit(1);
            }
        }
    }
    if let Err(e) = vm.provision().await {
        println!("provision     : FAILED — {e}");
        std::process::exit(1);
    }
    println!("provision     : OK");

    if let Ok(steps) = std::env::var("HV2_TRACE_STEPS") {
        let max = steps.parse::<u64>().unwrap_or(200_000);
        match vm.single_step_trace(max).await {
            Ok(trace) => {
                println!("trace         : {} instruction(s) stepped", trace.steps);
                match &trace.final_exit {
                    Some(exit) => println!("trace end     : {exit}"),
                    None => println!("trace end     : hit the {max}-step limit, still running"),
                }
                for rip in &trace.tail {
                    println!("  {rip:#x}");
                }
            }
            Err(e) => println!("trace         : FAILED — {e}"),
        }
        let _ = vm.stop().await;
        return;
    }

    if let Err(e) = vm.launch().await {
        println!("launch        : FAILED — {e}");
        std::process::exit(1);
    }
    let settle = std::env::var("HV2_SETTLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(SETTLE, Duration::from_secs);
    // Until the console says what the caller is waiting for, or the time is
    // up: a guest that reaches its login prompt in ten seconds need not be
    // watched for ninety.
    let until = std::env::var("HV2_UNTIL").ok();
    let deadline = tokio::time::Instant::now() + settle;
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let reached = match &until {
            Some(text) => vm.console_output().await.contains(text.as_str()),
            None => false,
        };
        if reached || tokio::time::Instant::now() >= deadline {
            break;
        }
    }
    if let Some(stats) = vm.vcpu_stats(0) {
        println!("exits         : {}", stats.exits());
    }
    println!("state         : {:?}", vm.state());
    let console = vm.console_output().await;
    let _ = vm.stop().await;
    if console.is_empty() {
        println!("console       : EMPTY — the firmware wrote nothing to COM1");
        std::process::exit(1);
    }
    println!("console       :\n{console}");
}
