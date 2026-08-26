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

use hv2_core::devices::keyboard::KeyboardDevice;
use hv2_core::devices::rtc::RtcDevice;
use hv2_core::{BootSource, HypervisorPlatform, SerialDevice, VMConfig, VM};

/// COM1, where `console=ttyS0` sends everything.
const COM1: u64 = 0x3F8;

/// Long enough for a kernel to reach its first console write, or to not.
const SETTLE: Duration = Duration::from_secs(5);

/// Environment override for how long to let the guest run.

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

    // A kernel asks the CMOS for the time before it can finish setting up, and
    // waits for the update-in-progress bit to clear. With nothing at 0x70 an
    // unhandled read returns 0xff, that bit reads as permanently set, and the
    // guest spins there forever having printed most of a line.
    let rtc = Arc::new(tokio::sync::RwLock::new(RtcDevice::new()));
    if let Err(e) = vm.devices().register_device("RTC", rtc).await {
        println!("rtc           : FAILED — {e}");
        return;
    }
    if let Err(e) = vm
        .devices()
        .register_io_port_range("RTC".to_string(), 0x70, 0x71)
        .await
    {
        println!("rtc           : FAILED to map ports — {e}");
        return;
    }
    println!("rtc           : CMOS mapped at 0x70-0x71");

    // Same again for the keyboard controller: a kernel probing i8042 polls
    // 0x64 until the status bits settle, and an absent port reads 0xff, which
    // says "busy" forever.
    let keyboard = Arc::new(tokio::sync::RwLock::new(KeyboardDevice::new()));
    if let Err(e) = vm.devices().register_device("i8042", keyboard).await {
        println!("keyboard      : FAILED — {e}");
        return;
    }
    if let Err(e) = vm
        .devices()
        .register_io_port_range("i8042".to_string(), 0x60, 0x64)
        .await
    {
        println!("keyboard      : FAILED to map ports — {e}");
        return;
    }
    println!("keyboard      : i8042 mapped at 0x60-0x64");

    match vm.provision().await {
        Ok(()) => println!("provision     : OK"),
        Err(e) => {
            println!("provision     : FAILED — {e}");
            return;
        }
    }

    // Single-stepping first, when asked: a guest that produces nothing has
    // either faulted before it could speak or is looping without exiting, and
    // the run loop cannot tell those apart.
    if let Ok(steps) = std::env::var("HV2_TRACE_STEPS") {
        let max = steps.parse::<u64>().unwrap_or(200_000);
        match vm.single_step_trace(max).await {
            Ok(trace) => {
                println!("trace         : {} instruction(s) stepped", trace.steps);
                match &trace.final_exit {
                    Some(exit) => println!("trace end     : {exit}"),
                    None => println!("trace end     : hit the {max}-step limit, still running"),
                }
                println!("last addresses:");
                for rip in &trace.tail {
                    println!("  {rip:#x}");
                }
            }
            Err(e) => println!("trace         : FAILED — {e}"),
        }
        let _ = vm.stop().await;
        return;
    }

    // `launch()` spawns the run loop and drops whatever it returns, so a vCPU
    // that fails on its first KVM_RUN is indistinguishable from one that is
    // still going. Driving `run()` directly is the only way to see the error.
    if let Err(e) = vm.start().await {
        println!("start         : FAILED — {e}");
        return;
    }
    println!("start         : OK — driving the vCPU loop directly");

    let settle = std::env::var("HV2_SETTLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(SETTLE, Duration::from_secs);

    match tokio::time::timeout(settle, vm.run()).await {
        Ok(Ok(())) => println!("run           : the loop exited cleanly"),
        Ok(Err(e)) => println!("run           : FAILED — {e}"),
        Err(_) => println!("run           : still running after {settle:?}"),
    }

    // Exits distinguish the two ways a guest goes quiet: a spinning guest
    // keeps exiting, and one blocked in KVM_RUN waiting for an interrupt that
    // never arrives stops exiting entirely. Both look identical from outside.
    if let Some(stats) = vm.vcpu_stats(0) {
        println!("exits         : {}", stats.exits());
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
