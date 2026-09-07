//! Compile a unikernel in Rust, boot it, and time it.
//!
//! Every guest that has run in this repository until now was hand-assembled
//! bytes. That proves the hypervisor works and proves nothing about whether
//! anyone can *write* a guest for it — which is the only question that matters
//! for an agent payload, because an agent is not going to be eleven bytes of
//! hand-encoded `out dx, al`.
//!
//! This builds `crates/hv2-unikernel` with the ordinary Rust toolchain and
//! boots the ELF that comes out. Nothing is pre-built and checked in, so there
//! is no asset to rot; and nothing is assembled in-process, because the point
//! is precisely that a compiler produced it.
//!
//! # What the output proves
//!
//! The guest prints a greeting and then the contents of `EAX` and `EBX`. The
//! greeting proves the ELF was loaded by its program headers and entered at
//! `e_entry` — an ELF written verbatim to 1 MB and entered at its first byte
//! executes `\x7fELF` as `jns +0x45` and says nothing at all.
//!
//! `EAX` proves more. The Multiboot specification says a bootloader leaves
//! `0x2BADB002` there and its info structure address in `EBX`, and nothing but
//! the loader can put them there. A guest that reports the magic was booted
//! *by the protocol*, not merely jumped to.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example rust_unikernel
//! ```
//!
//! Needs `/dev/kvm` and the `i686-unknown-linux-musl` target
//! (`rustup target add i686-unknown-linux-musl`). Without either it says which
//! and exits non-zero rather than printing a time it did not measure.

use hv2_core::{BootSource, VMConfig, VM};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The target the guest crate is built for. Not because anything here is
/// Linux, but because it is the 32-bit x86 target stable Rust ships a prebuilt
/// `core` for; the guest links with `rust-lld` and no C runtime.
const GUEST_TARGET: &str = "i686-unknown-linux-musl";

/// What the guest writes first. Checked, so a guest that boots into something
/// else is a failure rather than a surprise.
const GREETING: &str = "HYPERMACHINE RUST UNIKERNEL";

/// Build the guest crate and return the ELF it produced.
fn build_guest() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv2-unikernel");
    if !crate_dir.join("Cargo.toml").exists() {
        return Err(format!(
            "no guest crate at {} — this example builds it from source",
            crate_dir.display()
        ));
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        // The guest gets its own target directory. Inheriting this process's
        // would share a build lock with the cargo invocation that is running
        // this example, which is a deadlock waiting for a slow enough host.
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let hint = if stderr.contains("target may not be installed")
            || stderr.contains("can't find crate for `core`")
        {
            format!("\n\nInstall the target:  rustup target add {GUEST_TARGET}")
        } else {
            String::new()
        };
        return Err(format!("building the guest failed:\n{stderr}{hint}"));
    }

    let elf = crate_dir
        .join("target")
        .join(GUEST_TARGET)
        .join("release")
        .join("hv2-unikernel");
    if !elf.exists() {
        return Err(format!("the guest built but {} is missing", elf.display()));
    }

    // Boot from a copy on local storage, because `provision` reads the image
    // and where the file lives is therefore part of anything timed around it.
    // Measured here: the first read of this 8.7 KB ELF from the repository
    // took 9.0 ms, against 0.02 ms for everything else provision does with it,
    // because the repository is on a 9p mount. That number is a fact about the
    // mount and not about the hypervisor, and leaving it inside the boot
    // measurement would make this guest look six times slower to start than the
    // hand-assembled one for reasons that have nothing to do with either.
    // `unikernel_boot` writes its image to the temp directory for the same
    // reason; this copies for it.
    let dir = std::env::temp_dir().join("hv2-unikernel");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv2-unikernel.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the guest image: {e}"))?;
    Ok(local)
}

/// One boot, from `VM::new` to the guest's third line of output.
///
/// Returns the phase timings and what the guest said, or an error naming the
/// phase that failed.
async fn boot_once(elf: &Path) -> Result<(Phases, String), String> {
    let started = Instant::now();

    let config = VMConfig {
        name: "rust-unikernel".to_string(),
        vcpu_count: 1,
        memory_size: 16 * 1024 * 1024,
        boot: Some(BootSource::multiboot(elf)),
        ..Default::default()
    };

    let vm = Arc::new(VM::new(config).map_err(|e| {
        format!("VM::new failed — {e}\nA hypervisor backend is required: /dev/kvm on Linux.")
    })?);
    let created = started.elapsed();

    vm.provision()
        .await
        .map_err(|e| format!("provision failed — {e}"))?;
    let provisioned = started.elapsed();

    vm.launch()
        .await
        .map_err(|e| format!("launch failed — {e}"))?;
    let launched = started.elapsed();

    // Poll for the guest's own output rather than sleeping: the question is how
    // long the guest took, and a sleep answers how long the sleep was.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut console = String::new();
    let mut first_output = None;
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if !console.is_empty() && first_output.is_none() {
            first_output = Some(started.elapsed());
        }
        // Three lines: the greeting, the magic, and the info address.
        if console.lines().count() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_micros(200)).await;
    }

    let stop_started = Instant::now();
    vm.stop().await.map_err(|e| format!("stop failed — {e}"))?;

    Ok((
        Phases {
            created,
            provisioned,
            launched,
            first_output: first_output.ok_or("the guest produced nothing within 5 s")?,
            stopped: stop_started.elapsed(),
        },
        console,
    ))
}

/// Cumulative timings for one boot, except `stopped`, which is its own.
#[derive(Clone, Copy)]
struct Phases {
    created: Duration,
    provisioned: Duration,
    launched: Duration,
    first_output: Duration,
    stopped: Duration,
}

/// How many boots to time. The first measurement taken here was 12 ms and the
/// second 31 ms for a *smaller* guest, which is the shape of a number that
/// means nothing on its own: one run measures whatever else the machine was
/// doing. Nine is enough for a median to stop moving between invocations.
const RUNS: usize = 9;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let build_started = Instant::now();
    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build    : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let built = build_started.elapsed();
    let size = std::fs::metadata(&elf).map(|m| m.len()).unwrap_or(0);
    println!(
        "guest          : {size} bytes of ELF32, compiled in {:.2} s",
        built.as_secs_f64()
    );
    println!("timing         : {RUNS} boots, median. The compile is not part of it — a sandbox");
    println!("                 boots an image someone already built.");
    println!();

    let mut runs = Vec::with_capacity(RUNS);
    let mut console = String::new();
    for _ in 0..RUNS {
        match boot_once(&elf).await {
            Ok((phases, said)) => {
                if console.is_empty() {
                    console = said;
                }
                runs.push(phases);
            }
            Err(e) => {
                eprintln!("boot           : FAILED — {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }

    let report = |label: &str, pick: fn(&Phases) -> Duration, note: &str| {
        let values: Vec<f64> = runs.iter().map(|p| ms(pick(p))).collect();
        println!("{label:<15}: {:>8.3} ms  {note}", median(values));
    };
    report("VM::new", |p| p.created, "");
    report("provision", |p| p.provisioned, "(cumulative)");
    report("launch", |p| p.launched, "(cumulative)");
    report("first output", |p| p.first_output, "(cumulative)");
    report("stop", |p| p.stopped, "");

    println!();
    for line in console.lines() {
        println!("guest says     : {line}");
    }
    println!();

    // The two claims, checked rather than eyeballed.
    let ran = console.contains(GREETING);
    let booted = console.contains("OK");

    if ran && booted {
        println!(
            "result         : a compiled Rust unikernel executed, and was entered by the \
             Multiboot protocol — the magic in EAX is the loader's, not the guest's."
        );
        std::process::ExitCode::SUCCESS
    } else if ran {
        println!(
            "result         : the guest ran but EAX did not hold the bootloader magic, so it \
             was jumped to rather than booted."
        );
        std::process::ExitCode::FAILURE
    } else {
        println!(
            "result         : nothing recognisable on the console. An ELF entered at its \
             first byte executes its own header and produces exactly this."
        );
        std::process::ExitCode::FAILURE
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
