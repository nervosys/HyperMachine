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

use hv2_core::machine::Machine;
use hv2_core::{BootSource, HypervisorPlatform, VMConfig, VM};

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
    let mut cmdline = std::env::var("HV2_CMDLINE")
        .unwrap_or_else(|_| "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1".into());

    // A vsock device, when asked for. virtio-mmio has no enumeration, so the
    // window has to be named on the command line before the kernel is loaded —
    // which is before a `VM` exists to be asked where it put one. The address
    // and interrupt line are constants for exactly this reason, and
    // `VM::vsock_kernel_args()` is checked against what was rendered here once
    // the device is actually attached.
    let vsock_cid = std::env::var("HV2_VSOCK_CID")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    if vsock_cid.is_some() {
        cmdline.push_str(&format!(
            " virtio_mmio.device=4K@{:#x}:{}",
            VM::VSOCK_MMIO_BASE,
            VM::VSOCK_IRQ
        ));
    }
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
        boot: Some({
            let mut source = BootSource::linux(&kernel).with_cmdline(cmdline.clone());
            // An initrd is what turns "the kernel booted" into "userspace
            // ran": without a root filesystem the kernel reaches
            // prepare_namespace and panics, which is a complete boot and not
            // a running system.
            if let Ok(initrd) = std::env::var("HV2_INITRD") {
                println!("initrd        : {initrd}");
                source = source.with_initrd(initrd);
            }
            source
        }),
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

    // The legacy PC set in one call: COM1 at 0x3F8 (without it the guest's
    // console goes nowhere), the CMOS at 0x70-0x71 (without it the kernel spins
    // on an update-in-progress bit that an absent port reads as permanently
    // set) and the i8042 at 0x60-0x64 (same failure, keyboard-controller
    // status port). Each of those was a separate hang before it was a line in
    // `Machine::legacy_pc`.
    if let Err(e) = Machine::legacy_pc().attach(&vm.devices()).await {
        println!("devices       : FAILED — {e}");
        return;
    }
    println!("devices       : legacy PC set attached (COM1, RTC, i8042)");

    if let Some(cid) = vsock_cid {
        match vm.attach_vsock(cid).await {
            Ok(_) => {
                let args = vm.vsock_kernel_args().unwrap_or_default();
                println!("vsock         : attached, guest CID {cid}");
                println!("vsock args    : {args}");
                if !cmdline.contains(&args) {
                    println!(
                        "vsock         : WARNING — the command line says something else, so the \
                         guest will look at the wrong address"
                    );
                }
            }
            Err(e) => {
                println!("vsock         : FAILED — {e}");
                return;
            }
        }
    }

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
    let settle = std::env::var("HV2_SETTLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(SETTLE, Duration::from_secs);

    // Spawned rather than awaited. A guest that reaches userspace and idles
    // sits in KVM_RUN waiting for an interrupt, and that blocks the thread
    // inside a poll -- a timeout around `run()` can never fire, so the probe
    // would hang exactly when the boot had gone furthest.
    if let Err(e) = vm.launch().await {
        println!("launch        : FAILED — {e}");
        return;
    }
    println!("launch        : OK — the loop is running in the background");
    // Type something at the guest, if asked. This is the direction that has
    // never worked: input arrives from the host while the guest sits idle
    // waiting for it, so it can only be delivered by an interrupt the device
    // raises on its own rather than one polled after a guest access.
    if let Ok(text) = std::env::var("HV2_INPUT") {
        // Let the guest finish booting and reach its prompt first.
        tokio::time::sleep(Duration::from_secs(6)).await;
        // A shell prompt needs a newline to act on the line, and an
        // environment variable cannot carry one portably, so a literal
        // backslash-n in the variable stands in for it.
        let typed = text.replace("\\n", "\n");
        println!("input         : sending {typed:?}");
        match vm.devices().find_io_device(COM1 as u16).await {
            Some(device) => {
                if let Err(e) = device.console_input(typed.as_bytes()).await {
                    println!("input         : FAILED — {e}");
                }
            }
            None => println!("input         : FAILED — COM1 is not registered"),
        }
    }

    // Try one host-to-guest round trip over vsock, when asked. This is the
    // pair of halves that have never spoken: the device has only ever been
    // driven by tests that lay out rings by hand, and the agent only ever over
    // the host kernel's own socket.
    if std::env::var("HV2_VSOCK_PING").is_ok() {
        tokio::time::sleep(Duration::from_secs(8)).await;
        match vm.vsock() {
            Some(device) => {
                // Each step takes the device lock and releases it before the
                // next await: holding it across one would block the vCPU loop,
                // which needs the same device to serve the guest.
                let opened = { device.lock().connect(50_000, 1024) };
                match opened {
                    Ok(id) => {
                        println!("vsock connect : opened {id:?} to guest port 1024");

                        // Queueing is not delivering: the request has to be
                        // moved into a buffer the driver posted, and the guest
                        // told to look.
                        match vm.notify_vsock().await {
                            Ok(published) => println!("vsock request : published={published}"),
                            Err(e) => println!("vsock request : FAILED — {e}"),
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;

                        let request = b"{\"id\":1,\"version\":1,\"op\":{\"kind\":\"ping\"}}";
                        let mut framed = (request.len() as u32).to_le_bytes().to_vec();
                        framed.extend_from_slice(request);
                        let sent = { device.lock().send(id, &framed) };
                        match sent {
                            Ok(n) => println!("vsock send    : {n} bytes queued"),
                            Err(e) => println!("vsock send    : FAILED — {e}"),
                        }
                        match vm.notify_vsock().await {
                            Ok(true) => println!("vsock notify  : published and signalled"),
                            Ok(false) => println!("vsock notify  : nothing to publish"),
                            Err(e) => println!("vsock notify  : FAILED — {e}"),
                        }
                    }
                    Err(e) => println!("vsock connect : FAILED — {e}"),
                }
            }
            None => println!("vsock         : no device attached"),
        }
    }

    tokio::time::sleep(settle).await;

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
