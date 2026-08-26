//! Try to boot a real Linux kernel and report what it said.
//!
//! `boot_probe` runs a 512-byte real-mode image: enough to prove a guest
//! executes, and nothing at all about the Linux boot protocol, which is what
//! `BootSource::Linux` claims to implement. A kernel is a far harsher judge —
//! it reads `boot_params` field by field and stops early if any of it is
//! wrong, and it says so on the serial console.
//!
//! Pass a bzImage. On a Windows host with WSL, `/mnt/c/Program Files/WSL/tools/kernel`
//! is one, and it carries `CONFIG_SERIAL_8250_CONSOLE=y`.
//!
//! ```sh
//! cargo run -p hv2-core --example linux_boot_probe -- /path/to/bzImage
//! ```

use std::sync::Arc;
use std::time::Duration;

use hv2_core::{BootSource, HypervisorPlatform, SerialDevice, VMConfig, VM};

/// COM1, where `console=ttyS0` sends everything.
const COM1: u64 = 0x3F8;

/// Long enough for a kernel to reach its first console write, or to not.
const SETTLE: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() {
    // The vCPU loop reports every exit at debug level and nothing above it, so
    // without this a kernel that faults on its first instruction looks exactly
    // like one that never started. RUST_LOG=debug is the difference.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let Some(kernel) = std::env::args().nth(1) else {
        eprintln!("usage: linux_boot_probe <bzImage>");
        std::process::exit(2);
    };
    let kernel = std::path::PathBuf::from(kernel);
    if !kernel.exists() {
        println!("kernel        : MISSING {}", kernel.display());
        std::process::exit(2);
    }

    println!("platform      : {:?}", HypervisorPlatform::detect());
    println!(
        "kernel        : {} ({} bytes)",
        kernel.display(),
        std::fs::metadata(&kernel).map(|m| m.len()).unwrap_or(0)
    );

    // earlyprintk writes to the port directly, before the kernel has a console
    // driver. It is the only thing that reports a failure early enough to be
    // useful, because everything that goes wrong here goes wrong before then.
    let cmdline = std::env::var("HV2_CMDLINE")
        .unwrap_or_else(|_| "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1".into());
    println!("cmdline       : {cmdline}");

    let config = VMConfig {
        name: "linux-boot-probe".to_string(),
        vcpu_count: 1,
        // A kernel decompresses itself into space well above where it loads.
        memory_size: std::env::var("HV2_MEMORY_MIB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1024)
            * 1024
            * 1024,
        boot: Some(BootSource::linux(&kernel).with_cmdline(cmdline.clone())),
        ..Default::default()
    };

    let vm = match VM::new(config) {
        Ok(vm) => {
            println!("VM::new       : ok");
            Arc::new(vm)
        }
        Err(e) => {
            println!("VM::new       : FAILED — {e}");
            return;
        }
    };

    let serial = Arc::new(tokio::sync::RwLock::new(SerialDevice::new(
        "COM1".to_string(),
        COM1,
    )));
    if let Err(e) = vm.devices().register_device("COM1", serial).await {
        println!("serial        : FAILED — {e}");
        return;
    }
    if let Err(e) = vm
        .devices()
        .register_io_port_range("COM1".to_string(), COM1 as u16, COM1 as u16 + 7)
        .await
    {
        println!("serial        : FAILED to map ports — {e}");
        return;
    }
    println!("serial        : COM1 mapped at {COM1:#x}");

    match vm.provision().await {
        Ok(()) => println!("provision     : OK"),
        Err(e) => {
            println!("provision     : FAILED — {e}");
            return;
        }
    }

    // `launch()` spawns the run loop and drops whatever it returns, so a vCPU
    // that fails on its first KVM_RUN is indistinguishable from one that is
    // still going. Driving `run()` directly is the only way to see the error.
    if let Err(e) = vm.start().await {
        println!("start         : FAILED — {e}");
        return;
    }
    println!("start         : OK — driving the vCPU loop directly");

    match tokio::time::timeout(SETTLE, vm.run()).await {
        Ok(Ok(())) => println!("run           : the loop exited cleanly"),
        Ok(Err(e)) => println!("run           : FAILED — {e}"),
        Err(_) => println!("run           : still running after {SETTLE:?}"),
    }

    let console = vm.console_output().await;
    let _ = vm.stop().await;

    if console.is_empty() {
        println!(
            "console       : EMPTY — the kernel wrote nothing to COM1 in {SETTLE:?}. It either \
             never reached its entry point or rejected boot_params before earlyprintk was up."
        );
    } else {
        println!("console       :\n{console}");
    }
}
