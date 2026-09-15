//! Does a frame reach a network driver running inside the guest?
//!
//! Four pieces of networking landed together and none of them had carried a
//! packet: a device a guest can drive, `VM::attach_net` to give a VM one, a
//! guest driver, and a bridge to a host link. Each was verified against the
//! piece on either side of it, and the chain as a whole was verified nowhere.
//!
//! This is the chain, minus the host link. A frame is handed to the device,
//! the guest's driver receives it, swaps the two addresses and sends it back,
//! and the host compares what returns against what it sent.
//!
//! # Why an echo, and why no TAP device
//!
//! The echo is for the reason `vsock_echo` gives: a guest that receives and
//! prints proves one direction and pretends to the other, and an echo of a
//! payload chosen here is the smallest thing that can fail for the right
//! reason.
//!
//! No TAP device, because one needs `CAP_NET_ADMIN` and an interface somebody
//! configured, and bundling that in would mean a failure here could be either
//! the guest or the host's network configuration. This half needs only
//! `/dev/kvm`. The bridge to a real interface is the separate step, and it is
//! worth doing second: an echo that never comes back localises the fault to
//! the guest, and doing both at once does not.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example net_echo
//! ```
//!
//! Needs `/dev/kvm` and the `x86_64-unknown-none` target.

use hv2_core::{BootSource, VMConfig, VM};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The target the guest crate is built for.
const GUEST_TARGET: &str = "x86_64-unknown-none";

/// The guest's context ID. The guest only reaches its network driver after its
/// vsock driver comes up, so this VM carries both devices — which is also the
/// arrangement an agent VM wants, and a second check that the two windows and
/// the two interrupt lines do not collide in a running machine.
const GUEST_CID: u64 = 3;

/// The MAC given to the guest's device.
const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xab, 0xcd, 0xef];

/// Where the frame claims to come from: a different address from the guest's,
/// so that a correct echo is one where the two have visibly changed places
/// rather than one where nothing happened.
const PEER: [u8; 6] = [0x02, 0x00, 0x00, 0x11, 0x22, 0x33];

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

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// An Ethernet frame addressed to the guest: destination, source, ethertype,
/// and a payload distinctive enough that an echo cannot be confused with
/// anything else that might be in a buffer.
fn frame_to_guest() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&MAC);
    frame.extend_from_slice(&PEER);
    frame.extend_from_slice(&[0x08, 0x00]); // IPv4, though nothing here parses it
    frame.extend_from_slice(b"hv2-net: a frame for the guest to send back");
    frame
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
        name: "net-echo".to_string(),
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

    // Both devices before launch. A guest that probes an empty window reports
    // no device and carries on, which is a different failure from one that
    // never ran — and a harder one to tell apart afterwards.
    if let Err(e) = vm.attach_vsock(GUEST_CID).await {
        eprintln!("attach_vsock  : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
    let device = match vm.attach_net(MAC).await {
        Ok(device) => device,
        Err(e) => {
            eprintln!("attach_net    : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!(
        "net           : window at {:#x}, IRQ {}",
        VM::NET_MMIO_BASE,
        VM::NET_IRQ
    );

    if let Err(e) = vm.launch().await {
        eprintln!("launch        : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
    let started = Instant::now();

    // Wait for the guest to say it found the device, so a frame that never
    // comes back is not blamed on a guest that had not got there yet.
    let ready = {
        let vm = Arc::clone(&vm);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while Instant::now() < deadline {
            if vm.console_output().await.contains("net mac ") {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        found
    };
    if !ready {
        eprintln!("guest driver  : the guest never reported a network device");
        eprintln!("console       : {:?}", vm.console_output().await);
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }
    println!("guest driver  : up in {:>6.1} ms", ms(started.elapsed()));

    // Hand the guest a frame. `queue_received` runs the wake hook `attach_net`
    // installed, so this also publishes it and raises the interrupt — there is
    // nothing else to call.
    let sent = frame_to_guest();
    let offered = Instant::now();
    if !device.lock().queue_received(sent.clone()) {
        eprintln!("queue frame   : the device refused it");
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }
    println!("frame sent    : {} bytes", sent.len());

    // And wait for it to come back. The guard is dropped inside the closure
    // rather than held across a sleep: this is a `parking_lot` mutex the
    // delivery thread also takes.
    let echoed = until(Duration::from_secs(5), || device.lock().take_transmitted()).await;
    let Some(echoed) = echoed else {
        eprintln!("echo          : nothing came back within 5 s");
        eprintln!("console       : {:?}", vm.console_output().await);
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    };
    let round_trip = offered.elapsed();

    let _ = vm.stop().await;

    // What a correct echo looks like: the same bytes, with the first twelve
    // exchanged. Checked rather than merely counted, because a frame of the
    // right length that is not the frame sent would otherwise pass.
    let mut want = sent.clone();
    want[..6].copy_from_slice(&PEER);
    want[6..12].copy_from_slice(&MAC);

    if echoed != want {
        eprintln!("echo          : FAILED — {} bytes came back", echoed.len());
        eprintln!("  sent  {:02x?}", &sent[..sent.len().min(20)]);
        eprintln!("  got   {:02x?}", &echoed[..echoed.len().min(20)]);
        eprintln!("  want  {:02x?}", &want[..want.len().min(20)]);
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "echo          : {} bytes back, addresses swapped",
        echoed.len()
    );
    println!("round trip    : {:>6.1} ms", ms(round_trip));
    println!(
        "result        : a frame crossed into a guest, through a driver with no \
         allocator under it, and came back — both directions of a virtqueue the \
         guest walks itself"
    );
    std::process::ExitCode::SUCCESS
}
