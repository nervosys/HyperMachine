//! Does a message reach a program running inside the guest?
//!
//! The host has had a complete virtio-vsock device for some time: split
//! virtqueues walked out of guest memory, connection state, credit accounting.
//! What it never had was a guest that could talk to it, so every "delivery" in
//! this repository has been a byte written to a serial port — real, but
//! one-directional and not a socket.
//!
//! This connects to a `no_std` Rust guest over vsock and asks it to answer.
//! The guest is `crates/hv2-unikernel`, built here from source; its driver is
//! `src/vsock.rs`.
//!
//! # Why an echo
//!
//! Because the claim is that bytes crossed the boundary in *both* directions.
//! A guest that receives a message and prints it proves the first half; a
//! guest that receives a message and answers with a fixed string proves the
//! first half and pretends to the second. Echoing a payload chosen here, and
//! comparing it, is the smallest test that can fail for the right reason.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example vsock_echo
//! ```
//!
//! Needs `/dev/kvm` and the `i686-unknown-linux-musl` target.

use hv2_core::devices::virtio_vsock::VsockConnectionState;
use hv2_core::{BootSource, VMConfig, VM};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The target the guest crate is built for: the 32-bit x86 target stable Rust
/// ships a prebuilt `core` for.
const GUEST_TARGET: &str = "i686-unknown-linux-musl";

/// The guest's context ID. 0, 1 and 2 are reserved; 3 is the first a guest may
/// have.
const GUEST_CID: u64 = 3;

/// The port the host listens on and the guest answers.
const HOST_PORT: u32 = 1024;
/// The guest's port. Nothing in the guest binds it — the driver answers
/// whatever it is asked on — but the host has to name one.
const GUEST_PORT: u32 = 5000;

/// What gets sent. Long enough that a truncation is visible and distinctive
/// enough that an echo cannot be confused with anything else in the buffer.
const MESSAGE: &str = "hv2-swarm: command from root to worker-7";

/// Build the guest crate and stage its ELF on local storage.
fn build_guest() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv2-unikernel");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        // Its own target directory: inheriting this process's would share a
        // build lock with the cargo invocation running this example.
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "building the guest failed:\n{}\n\nThe target may be missing:  rustup target add \
             {GUEST_TARGET}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let elf = crate_dir
        .join("target")
        .join(GUEST_TARGET)
        .join("release")
        .join("hv2-unikernel");
    if !elf.exists() {
        return Err(format!("the guest built but {} is missing", elf.display()));
    }

    // Boot from a copy on local storage: `provision` reads the image, and the
    // first read of it from a 9p mount costs more than the whole boot.
    let dir = std::env::temp_dir().join("hv2-unikernel");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv2-unikernel.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the guest image: {e}"))?;
    Ok(local)
}

/// Poll `f` until it returns `Some`, or give up after `bound`.
async fn until<T>(bound: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + bound;
    while Instant::now() < deadline {
        if let Some(value) = f() {
            return Some(value);
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    None
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("guest         : {}", elf.display());

    let config = VMConfig {
        name: "vsock-echo".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&elf)),
        ..Default::default()
    };
    let vm = match VM::new(config) {
        Ok(vm) => Arc::new(vm),
        Err(e) => {
            eprintln!("VM::new       : FAILED — {e}");
            eprintln!("A hypervisor backend is required: /dev/kvm on Linux.");
            return std::process::ExitCode::FAILURE;
        }
    };

    if let Err(e) = vm.provision().await {
        eprintln!("provision     : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }

    // The device has to exist before the guest looks for it. A guest that
    // probes an empty window reports "no virtio device" and carries on, which
    // is a different failure from one that never ran.
    let device = match vm.attach_vsock(GUEST_CID).await {
        Ok(device) => device,
        Err(e) => {
            eprintln!("attach_vsock  : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!(
        "vsock         : guest CID {GUEST_CID}, window at {:#x}",
        VM::VSOCK_MMIO_BASE
    );

    if let Err(e) = vm.launch().await {
        eprintln!("launch        : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }

    let started = Instant::now();

    // Wait for the guest to say it found the device, so a connection failure
    // below is not blamed on a guest that had not got there yet.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = false;
    while Instant::now() < deadline {
        if vm.console_output().await.contains("vsock cid") {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    if !ready {
        eprintln!("guest driver  : the guest never reported a vsock device");
        eprintln!("console       : {:?}", vm.console_output().await);
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }
    println!("guest driver  : up in {:>6.1} ms", ms(started.elapsed()));

    // Connect. The device sends REQUEST to the guest and the guest answers
    // RESPONSE; `state` becomes Established when that answer arrives.
    // The guard is dropped before the match, not held across the awaits in its
     // arms: this is a `parking_lot` mutex the vsock device is also taken under
     // from the delivery thread, and holding one across an await is how an
     // executor deadlocks against itself.
    let opened = device.lock().connect(HOST_PORT, GUEST_PORT);
    let id = match opened {
        Ok(id) => id,
        Err(e) => {
            eprintln!("connect       : FAILED — {e}");
            let _ = vm.stop().await;
            return std::process::ExitCode::FAILURE;
        }
    };

    let connected = until(Duration::from_secs(5), || {
        (device.lock().state(id) == Some(VsockConnectionState::Established)).then_some(())
    })
    .await;
    if connected.is_none() {
        eprintln!("connect       : the guest never answered the connection request");
        eprintln!("console       : {:?}", vm.console_output().await);
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }
    println!(
        "connect       : established in {:>6.1} ms",
        ms(started.elapsed())
    );

    // Send, and wait for the whole message to come back.
    let sent_at = Instant::now();
    let sent = device.lock().send(id, MESSAGE.as_bytes());
    if let Err(e) = sent {
        eprintln!("send          : FAILED — {e}");
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }

    let echo = until(Duration::from_secs(5), || {
        let got = device.lock().peek(id).unwrap_or_default();
        (got.len() >= MESSAGE.len()).then_some(got)
    })
    .await;
    let round_trip = sent_at.elapsed();

    let console = vm.console_output().await;
    let _ = vm.stop().await;

    println!();
    for line in console.lines() {
        println!("guest says    : {line}");
    }
    println!();

    match echo {
        Some(bytes) if bytes == MESSAGE.as_bytes() => {
            println!("sent          : {MESSAGE:?}");
            println!("echoed        : {:?}", String::from_utf8_lossy(&bytes));
            println!("round trip    : {:>6.3} ms", ms(round_trip));
            println!();
            println!(
                "result        : a message reached a program inside the guest and came back. \
                 Not a serial port — a vsock connection, established by the guest's own reply."
            );
            std::process::ExitCode::SUCCESS
        }
        Some(bytes) => {
            println!("sent          : {MESSAGE:?}");
            println!("echoed        : {:?}", String::from_utf8_lossy(&bytes));
            println!("result        : the guest answered, but not with what it was sent.");
            std::process::ExitCode::FAILURE
        }
        None => {
            println!("result        : the connection was established but nothing came back.");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
