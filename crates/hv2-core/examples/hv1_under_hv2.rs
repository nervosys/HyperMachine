//! Run the Type-1 hypervisor under the Type-2 one, and report exactly what
//! that does and does not prove.
//!
//! `hv1-core` has been graded "typed" on every honest accounting this project
//! has made of itself — EL2, stage-2, VMX, SVM, all compiling and none of it
//! ever executed. The reason was always the same: it is a bare-metal
//! hypervisor and nobody had bare metal to put it on. That is still true. What
//! changed is that `hv2-core` can now boot a Multiboot ELF, and this host's CPU
//! exposes AMD-V to guests, so there is somewhere else to run it.
//!
//! # What a pass here means
//!
//! - **The code runs.** `hv1_core::initialize()` executes on a real CPU, reads
//!   real `CPUID` and real MSRs, and reports what it decided. Every previous
//!   claim about this crate was a claim about `rustc`.
//! - **It is not hardware.** The layer underneath is KVM, and a guest sees the
//!   CPU its hypervisor chose to show it. Nested SVM is a real feature and this
//!   is a real path through it, but bare metal is still untested.
//! - **It is not a working hypervisor.** Initialising is the first step. A
//!   hypervisor that initialises and cannot run a guest is a hypervisor that
//!   initialises, and this says nothing about `VMRUN`.
//!
//! Those bounds are the point. The alternative was not running it at all, and
//! "it compiles" was being carried as though it were a fourth supported model.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example hv1_under_hv2
//! ```
//!
//! Needs `/dev/kvm`, the `x86_64-unknown-none` target, and a host whose CPU
//! offers virtualisation to its guests — on Linux, `kvm_amd`/`kvm_intel` with
//! `nested=1`. Without any of them it says which and exits non-zero.

use hv2_core::{BootSource, VMConfig, VM};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The freestanding 64-bit target. Unlike the 32-bit guest target, this one is
/// bare metal proper and needs no C runtime excluded by hand.
const GUEST_TARGET: &str = "x86_64-unknown-none";

/// Build `crates/hv1-multiboot` and stage the ELF on local storage.
fn build_image() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv1-multiboot");
    if !crate_dir.join("Cargo.toml").exists() {
        return Err(format!("no crate at {}", crate_dir.display()));
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        // Its own target directory, and its own `.cargo/config.toml`, which
        // cargo only reads because the working directory is the crate.
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "building hv1-multiboot failed:\n{}\n\nThe target may be missing:  rustup target \
             add {GUEST_TARGET}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let elf = crate_dir
        .join("target")
        .join(GUEST_TARGET)
        .join("release")
        .join("hv1-multiboot");
    if !elf.exists() {
        return Err(format!("built, but {} is missing", elf.display()));
    }

    // Boot from a copy on local storage: `provision` reads the image, and over
    // a 9p mount that read costs more than the whole boot.
    let dir = std::env::temp_dir().join("hv1-multiboot");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv1-multiboot.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the image: {e}"))?;
    Ok(local)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let elf = match build_image() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("build         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let size = std::fs::metadata(&elf).map(|m| m.len()).unwrap_or(0);
    println!("image         : {size} bytes of ELF64, hv1-core linked into a Multiboot kernel");
    println!("               (an ELF64, so the header carries its own load addresses —");
    println!("                the specification's ELF path is ELF32 only)");
    println!();

    let config = VMConfig {
        name: "hv1-under-hv2".to_string(),
        vcpu_count: 1,
        // Room for the image, its 1 MiB heap, and the 1 GiB the trampoline
        // identity-maps having somewhere to point.
        memory_size: 256 * 1024 * 1024,
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

    let started = Instant::now();
    if let Err(e) = vm.provision().await {
        eprintln!("provision     : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
    if let Err(e) = vm.launch().await {
        eprintln!("launch        : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }

    // Wait for the guest to finish rather than for a fixed time: the last line
    // it writes is "done", and anything short of that is a guest that stopped
    // somewhere it did not choose.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut console = String::new();
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if console.contains("done") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let elapsed = started.elapsed();
    let _ = vm.stop().await;

    if console.is_empty() {
        println!("console       : nothing");
        println!();
        println!(
            "result        : the image produced no output at all. It never reached 64-bit \
             mode, or never reached its first `out`."
        );
        return std::process::ExitCode::FAILURE;
    }

    for line in console.lines() {
        println!("hv1 says      : {line}");
    }
    println!();
    println!(
        "elapsed       : {:.1} ms to the last line",
        elapsed.as_secs_f64() * 1000.0
    );
    println!();

    // Three separate claims, checked separately, because they fail in different
    // places and a single pass/fail would hide which.
    let long_mode = console.contains("LMA");
    let reached_hv1 = console.contains("hv1   version");
    let initialised = console.contains("initialize() OK");
    // The effect rather than the verdict: `EFER.SVME` is what
    // `svm::initialize()` actually does, and an `Ok` without it is the one
    // outcome here that reads as a pass and is not one.
    let svme = console.contains("SVME set");

    println!("long mode     : {}", yes_no(long_mode));
    println!("hv1 executed  : {}", yes_no(reached_hv1));
    println!("hv1 initialised: {}", yes_no(initialised));
    println!(
        "CPU changed   : {}  (EFER.SVME, which is what svm::initialize() does)",
        yes_no(svme)
    );
    println!();

    if !long_mode {
        println!(
            "result        : the guest spoke but never reached long mode, so the 32-bit \
             trampoline is where to look."
        );
        return std::process::ExitCode::FAILURE;
    }
    if !reached_hv1 {
        println!(
            "result        : long mode reached, but hv1-core's own code never ran. Something \
             between the far jump and the first call."
        );
        return std::process::ExitCode::FAILURE;
    }

    if initialised && !svme {
        println!(
            "result        : hv1-core reported success and left EFER.SVME clear. That is the \
             worst outcome available here, because it reads as a pass and would fail at the \
             first VMRUN."
        );
        return std::process::ExitCode::FAILURE;
    }

    if initialised {
        println!(
            "result        : hv1-core initialised on a real CPU and left EFER.SVME set, so \
             it did the architectural work rather than only returning Ok. That is execution, \
             not a \
             compilation result — and it is still not bare metal: the layer underneath is \
             KVM, and nothing here has run a guest."
        );
    } else {
        println!(
            "result        : hv1-core executed and declined to initialise. The line above \
             says which error, which is the useful half — it ran, and it decided."
        );
    }
    std::process::ExitCode::SUCCESS
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}
