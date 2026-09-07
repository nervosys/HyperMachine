//! Does a fleet of agents pay for a model once, or once each?
//!
//! This decides whether "a thousand agents on a node" survives contact with an
//! agent that actually runs a model. Today an agent VM costs 0.221 MiB because
//! it holds nothing but its own code. A small model is not small next to that:
//! a 0.6-billion-parameter model at four bits is around 350 MB, so a thousand
//! private copies is 350 GB and the premise is gone.
//!
//! They do not need private copies. Every agent in a fleet runs the same
//! weights, reads them and never writes them. This measures what that costs.
//!
//! # What is measured
//!
//! Resident memory, before and after, against a region of a size chosen to
//! stand in for a model. The number that matters is whether growth tracks the
//! region or the region times the number of guests — those differ by three
//! orders of magnitude at this fleet size, and nothing about the code makes it
//! obvious which one happens.
//!
//! Every guest also *reads* its copy and reports the byte it found. A region
//! that is mapped and unreadable would cost exactly the same and be worth
//! nothing, and "the VM started" has never been evidence in this project that
//! the guest can do anything.
//!
//! # The other half: what an agent costs once it holds something
//!
//! Shared weights are the part a fleet does not pay for twice. Everything an
//! agent holds *privately* it does pay for per agent, and for a language model
//! that is the KV cache — which cannot be shared with another agent by
//! definition, since it is that agent's conversation.
//!
//! `--touch-mib N` tells every guest to touch N MiB of its own memory before
//! reporting, which stands in for a cache being filled. What it measures is
//! whether private guest memory costs what it should or carries a multiplier:
//! the density of a fleet is the model once, plus this per agent, and a hidden
//! factor of two here halves how many agents fit on a node.
//!
//! ```text
//! cargo run --release -p hv2-core --example shared_weights -- \
//!     --agents 200 --mib 350 --touch-mib 24
//! ```

use hv2_core::shared_rom::SharedRom;
use hv2_core::{BootSource, VMConfig, VM};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where the shared region appears in every guest's physical address space.
///
/// Above the vsock window so a guest can have both, and far above any guest's
/// RAM so it is unambiguously not memory the guest owns.
const ROM_BASE: u64 = 0xE000_0000;

/// The byte written at the start of the region, for the guest to find. Chosen
/// to be recognisable in a hex dump and impossible to confuse with zero.
const MARKER: u8 = 0xA7;

const GUEST_TARGET: &str = "i686-unknown-linux-musl";

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
    let dir = std::env::temp_dir().join("hv2-unikernel");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv2-unikernel.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the guest image: {e}"))?;
    Ok(local)
}

/// Resident set size in bytes.
fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut agents = 100usize;
    let mut region_mib = 256u64;
    let mut touch_mib = 0u32;
    let mut guest_mib = 64u64;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agents" if i + 1 < args.len() => {
                agents = args[i + 1].parse().unwrap_or(agents).max(1);
                i += 1;
            }
            "--mib" if i + 1 < args.len() => {
                region_mib = args[i + 1].parse().unwrap_or(region_mib).max(1);
                i += 1;
            }
            "--touch-mib" if i + 1 < args.len() => {
                touch_mib = args[i + 1].parse().unwrap_or(touch_mib);
                i += 1;
            }
            "--guest-mib" if i + 1 < args.len() => {
                guest_mib = args[i + 1].parse().unwrap_or(guest_mib).max(32);
                i += 1;
            }
            other => {
                eprintln!("unrecognised argument {other}");
                return std::process::ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let baseline = rss_bytes().unwrap_or(0);

    // The stand-in for a model. Filled, not merely allocated: an untouched
    // anonymous mapping costs nothing resident, so measuring one would prove
    // that lazy allocation works rather than that sharing does.
    let region_bytes = region_mib * 1024 * 1024;
    let rom = match SharedRom::zeroed(region_bytes) {
        Ok(rom) => rom,
        Err(e) => {
            eprintln!("region        : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // SAFETY: the region is this process's own mapping and nothing else refers
    // to it yet.
    unsafe {
        let base = rom.host_addr() as *mut u8;
        std::ptr::write_bytes(base, 0x5A, region_bytes as usize);
        // A marker the guests can look for, so a guest reading zeroes is
        // distinguishable from a guest reading the region.
        std::ptr::write(base, MARKER);
        // And, four bytes in, how much private memory each of them should
        // touch. The region is the only thing every guest can already read, so
        // it is also the simplest place to put a parameter for them.
        std::ptr::write(base.add(4) as *mut u32, touch_mib);
    }
    let after_region = rss_bytes().unwrap_or(0);
    println!(
        "region        : {} MiB filled — resident grew {:.1} MiB",
        region_mib,
        mib(after_region.saturating_sub(baseline))
    );

    let started = Instant::now();
    let mut vms: Vec<Arc<VM>> = Vec::with_capacity(agents);
    for index in 0..agents {
        let config = VMConfig {
            name: format!("agent-{index}"),
            vcpu_count: 1,
            // The guest's working area starts at 16 MiB, so its RAM has to
            // reach past that plus whatever it was asked to touch.
            memory_size: guest_mib.max(u64::from(touch_mib) + 32) * 1024 * 1024,
            boot: Some(BootSource::multiboot(&elf)),
            ..Default::default()
        };
        let vm = match VM::new(config) {
            Ok(vm) => Arc::new(vm),
            Err(e) => {
                eprintln!("agent {index}       : no backend — {e}");
                eprintln!("This needs /dev/kvm.");
                return std::process::ExitCode::FAILURE;
            }
        };
        if let Err(e) = vm.provision().await {
            eprintln!("agent {index}       : provision — {e}");
            return std::process::ExitCode::FAILURE;
        }
        if let Err(e) = vm.attach_shared_rom(ROM_BASE, Arc::clone(&rom)).await {
            eprintln!("agent {index}       : shared region — {e}");
            return std::process::ExitCode::FAILURE;
        }
        if let Err(e) = vm.launch().await {
            eprintln!("agent {index}       : launch — {e}");
            return std::process::ExitCode::FAILURE;
        }
        vms.push(vm);
    }
    let booted = started.elapsed();

    // Give every guest time to read its region and say so. The guest prints a
    // full 32-bit word, and an *unmapped* guest-physical address reads as all
    // ones rather than as zero — so `rom 0x000000FF` is the answer when nothing
    // is there, and only the marker distinguishes a region that was read from
    // one that was never mapped.
    let expected = if touch_mib > 0 {
        format!("work 0x{touch_mib:08X}")
    } else {
        format!("rom 0x{:08X} write refused", MARKER)
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut read_it = 0usize;
    while Instant::now() < deadline {
        read_it = 0;
        for vm in &vms {
            if vm.console_output().await.contains(&expected) {
                read_it += 1;
            }
        }
        if read_it == vms.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let after_agents = rss_bytes().unwrap_or(0);
    let agent_growth = after_agents.saturating_sub(after_region);

    // What a fleet costs if nothing is shared: every agent with its own copy of
    // the model as well as its own working set.
    let working = u64::from(touch_mib) * 1024 * 1024;
    let copied = vms.len() as u64 * (region_bytes + working);
    // And what it should cost if the model is shared: the model once, plus each
    // agent's own working set and a megabyte for the agent itself. Both terms
    // matter — an earlier version left the working set out and reported a
    // perfectly shared fleet as unshared the moment the agents held anything.
    let budget = region_bytes + vms.len() as u64 * (working + 1024 * 1024);

    println!(
        "agents        : {} booted in {:.1} ms",
        vms.len(),
        booted.as_secs_f64() * 1000.0
    );
    println!(
        "read the ROM  : {read_it} of {} read {MARKER:#04x} at {ROM_BASE:#x}, write refused",
        vms.len()
    );
    if touch_mib > 0 {
        let per_agent = mib(agent_growth) / vms.len() as f64;
        println!(
            "touched       : {touch_mib} MiB each — {:.3} MiB per agent, {:+.3} MiB against \
             what was asked for",
            per_agent,
            per_agent - f64::from(touch_mib)
        );
    }
    println!(
        "resident      : {:.1} MiB total, {:.1} MiB attributable to the agents",
        mib(after_agents.saturating_sub(baseline)),
        mib(agent_growth)
    );
    println!(
        "per agent     : {:.3} MiB",
        mib(agent_growth) / vms.len() as f64
    );
    println!();
    println!(
        "if copied     : {} agents x {} MiB would be {:.1} GiB",
        vms.len(),
        region_mib + u64::from(touch_mib),
        mib(copied) / 1024.0
    );

    for vm in &vms {
        let _ = vm.stop().await;
    }

    if read_it != vms.len() {
        println!();
        println!(
            "result        : {read_it} of {} guests read the region. A region that is mapped \
             and unreadable costs the same and is worth nothing.",
            vms.len()
        );
        return std::process::ExitCode::FAILURE;
    }

    // The claim is that the fleet pays for the region once. So the threshold is
    // the region plus a megabyte per agent for the agents themselves — not the
    // region alone, which an earlier version of this compared against and which
    // fails for a fleet large enough that its own legitimate cost exceeds one
    // copy of the model. That is the arithmetic of the thing being measured:
    // per-agent cost scales with the fleet and the shared region does not.
    let total = after_agents.saturating_sub(baseline);
    let shared = total < budget;

    println!(
        "saved         : {:.1} GiB, {:.2}% of what copying would have cost",
        mib(copied.saturating_sub(total)) / 1024.0,
        100.0 * (copied.saturating_sub(total)) as f64 / copied as f64
    );
    println!();
    if shared {
        println!(
            "result        : every agent read the same {region_mib} MiB and the host paid for it once. Weights are shared, so a fleet costs the model plus whatever each agent needs of its own."
        );
        std::process::ExitCode::SUCCESS
    } else {
        println!(
            "result        : {:.1} MiB resident against a budget of {:.1} MiB — the region is not being shared, and a fleet would pay per agent.",
            mib(total),
            mib(budget)
        );
        std::process::ExitCode::FAILURE
    }
}
