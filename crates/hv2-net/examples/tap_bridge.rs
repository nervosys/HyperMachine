//! Does a frame the guest sends reach a real network interface?
//!
//! `net_echo` in `hv2-core` boots the guest driver and puts a frame through it
//! in both directions, which proves everything up to the host's own edge. This
//! is the last link: the [`Bridge`] carrying what the guest sent onto a TAP
//! device the host kernel owns.
//!
//! # The privilege, and how little of it is needed
//!
//! Creating a TAP interface needs `CAP_NET_ADMIN`. *Opening* one that already
//! exists and is owned by the calling user needs nothing at all — which means
//! the privileged part is one command, run once, and every run after it is
//! ordinary:
//!
//! ```text
//! sudo ip tuntap add dev hm0 mode tap user "$USER"
//! sudo ip link set hm0 up
//! ```
//!
//! Without that this example reports what is missing and exits successfully,
//! because "this machine has no TAP device" is a fact about the machine and
//! not a failure of the code. It is the same shape as the examples here that
//! need `/dev/kvm`.
//!
//! # What it proves, and what it does not
//!
//! It proves the guest-to-world direction end to end: a frame crosses a
//! virtqueue the guest walks itself, is drained by the device, carried by the
//! bridge, and written to an interface the kernel owns.
//!
//! It does not prove the return direction against a real peer. Reading from a
//! TAP fd yields frames the *kernel* transmits out that interface, and a
//! kernel with no address and no route on it transmits nothing. Watching one
//! arrive means giving the interface an address and arranging for traffic —
//! more host configuration, and a different claim from this one. The bridge's
//! own tests cover that direction against an in-memory link, and `net_echo`
//! covers it against a real guest.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-net --example tap_bridge
//! ```

use hv2_core::{BootSource, VMConfig, VM};
use hv2_net::bridge::{Bridge, TapLink};
use hv2_net::tap::TapConfig;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The target the guest crate is built for.
const GUEST_TARGET: &str = "x86_64-unknown-none";

/// The interface this looks for. Overridable, because the name is the one
/// thing about it that is somebody else's choice.
const DEFAULT_TAP: &str = "hm0";

/// The guest's context ID. The guest reaches its network driver only after its
/// vsock driver comes up, so this VM carries both.
const GUEST_CID: u64 = 3;

/// The MAC given to the guest's device.
const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xab, 0xcd, 0xef];

/// Where the injected frame claims to come from.
const PEER: [u8; 6] = [0x02, 0x00, 0x00, 0x11, 0x22, 0x33];

fn build_guest() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv2-unikernel");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
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

/// An Ethernet frame addressed to the guest.
fn frame_to_guest() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&MAC);
    frame.extend_from_slice(&PEER);
    frame.extend_from_slice(&[0x08, 0x00]);
    frame.extend_from_slice(b"hv2-net: out through a real interface");
    frame
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let name = std::env::args().nth(1).unwrap_or(DEFAULT_TAP.to_string());

    // The TAP device first, because it is the thing most likely to be absent
    // and there is no reason to boot a guest to find that out.
    let link = match TapLink::open(TapConfig::new(&name).with_vnet_hdr(false)).await {
        Ok(link) => link,
        Err(e) => {
            println!("tap           : not available — {e}");
            println!();
            println!("This machine has no TAP device called '{name}'. Creating one needs");
            println!("CAP_NET_ADMIN; opening one that already exists and is owned by you");
            println!("needs nothing, so the privileged part is these two lines, once:");
            println!();
            println!("    sudo ip tuntap add dev {name} mode tap user \"$USER\"");
            println!("    sudo ip link set {name} up");
            println!();
            println!("skipped       : a fact about this host, not about the code");
            return std::process::ExitCode::SUCCESS;
        }
    };
    println!("tap           : {} open, no vnet header", link.name());

    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let config = VMConfig {
        name: "tap-bridge".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&elf)),
        ..Default::default()
    };
    let vm = match VM::new(config) {
        Ok(vm) => Arc::new(vm),
        Err(e) => {
            println!("VM::new       : not available — {e}");
            println!("skipped       : a hypervisor backend is required (/dev/kvm on Linux)");
            return std::process::ExitCode::SUCCESS;
        }
    };

    if let Err(e) = vm.provision().await {
        eprintln!("provision     : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
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
    if let Err(e) = vm.launch().await {
        eprintln!("launch        : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }

    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = false;
    while Instant::now() < deadline {
        if vm.console_output().await.contains("net mac ") {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    if !ready {
        eprintln!("guest driver  : the guest never reported a network device");
        eprintln!("console       : {:?}", vm.console_output().await);
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }
    println!("guest driver  : up in {:>6.1} ms", ms(started.elapsed()));

    // No NAT. It translates IPv4 and this frame is not addressed to anywhere,
    // so a NAT in the path would correctly refuse it and the test would be
    // about NAT rather than about the bridge.
    // `allow_all`, and deliberately: this carries one hand-built ethernet
    // frame that is not addressed to anywhere routable, so any policy that
    // read its destination would refuse it and the run would demonstrate the
    // policy instead of the bridge. A sandbox is the other case entirely --
    // there the default `EgressPolicy::deny_all` applies and an allowlist is
    // written for what the workload actually needs.
    let mut bridge = Bridge::new(
        Arc::clone(&device),
        link,
        None,
        hv2_net::egress::EgressPolicy::allow_all(),
    );

    // Hand the guest a frame. It echoes with the addresses swapped, the device
    // drains it on the guest's own kick, and the bridge is what carries it out.
    let sent = frame_to_guest();
    device.lock().queue_received(sent.clone());
    println!("frame sent    : {} bytes, into the guest", sent.len());

    let offered = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut carried = 0;
    while Instant::now() < deadline {
        match bridge.pump().await {
            Ok(n) => carried += n,
            Err(e) => {
                eprintln!("bridge        : FAILED — {e}");
                let _ = vm.stop().await;
                return std::process::ExitCode::FAILURE;
            }
        }
        if bridge.stats().out_frames > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let elapsed = offered.elapsed();
    let _ = vm.stop().await;

    let stats = bridge.stats();
    if stats.out_frames == 0 {
        eprintln!("bridge        : FAILED — the guest's frame never reached the interface");
        eprintln!("  carried {carried}, stats {stats:?}");
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "bridge        : {} frame(s) out to {} in {:>6.1} ms",
        stats.out_frames,
        name,
        ms(elapsed)
    );
    println!(
        "result        : a frame the guest wrote into its own virtqueue was written \
         to a network interface the host kernel owns"
    );
    println!(
        "note          : the return direction needs an addressed, routed interface \
         and is not claimed here"
    );
    std::process::ExitCode::SUCCESS
}
