//! Try to actually execute a guest on this machine, and report how far it got.
//!
//! The boot path has four steps that can each fail for different reasons, and
//! a host can pass one and fail the next: `VM::new` only selects a backend,
//! `provision()` creates the backend's VM and vCPUs, `load_boot` writes the
//! image into guest physical memory, and `launch()` runs it. Reporting them
//! separately is the point — "the tests ran" has never meant "a guest
//! executed", and this is the program that tells the difference.
//!
//! It boots `examples/guest_code/hello.bin`, a 512-byte real-mode image that
//! writes to COM1 and halts, so the evidence of execution is the guest's own
//! output rather than an exit code.
//!
//! ```sh
//! cargo run -p hv2-core --example boot_probe
//! ```

use std::sync::Arc;
use std::time::Duration;

use hv2_core::{BootSource, HypervisorPlatform, SerialDevice, VMConfig, VM};

/// COM1. The image writes here and nowhere else.
const COM1: u64 = 0x3F8;

#[tokio::main]
async fn main() {
    println!("platform      : {:?}", HypervisorPlatform::detect());

    let image = std::path::Path::new("examples/guest_code/hello.bin");
    if !image.exists() {
        println!(
            "image         : MISSING {} — run from the repo root",
            image.display()
        );
        return;
    }
    println!(
        "image         : {} bytes",
        std::fs::metadata(image).map(|m| m.len()).unwrap_or(0)
    );

    let config = VMConfig {
        name: "boot-probe".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::raw(image)),
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

    // Give the guest somewhere to write. Nothing attaches a console
    // automatically, so without this the image runs and says nothing.
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
        Ok(()) => println!("provision     : OK — backend VM and vCPUs exist"),
        Err(e) => {
            println!("provision     : FAILED — {e}");
            return;
        }
    }

    match vm.launch().await {
        Ok(()) => println!("launch        : OK — the guest is executing"),
        Err(e) => {
            println!("launch        : FAILED — {e}");
            return;
        }
    }

    // The image halts almost immediately; give it room and then read what it
    // said. An empty console here means the guest never ran, which is a
    // different failure from any of the ones above.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let console = vm.console_output().await;
    let _ = vm.stop().await;

    if console.is_empty() {
        println!("console       : EMPTY — nothing reached COM1, so no guest code ran");
    } else {
        println!("console       : {console:?}");
        if console.contains("Hello") {
            println!("\nA guest executed and printed. That is the gate.");
        }
    }
}
