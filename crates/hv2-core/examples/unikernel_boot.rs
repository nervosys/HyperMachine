//! Boot a unikernel and time it.
//!
//! A unikernel is not a kernel plus an application; it is an application that
//! *is* the image. There is no scheduler, no init, no module loader, no
//! filesystem, and no syscall boundary, because there is nothing on the other
//! side of one. For an agent sandbox that is the security argument: a guest
//! with no kernel has no kernel attack surface, and the code the host is
//! isolating is the only code in the VM.
//!
//! It is also the performance argument. Booting Linux to a usable sandbox
//! measured 1,014 ms on this host, of which 988 ms was the kernel. A unikernel
//! has nothing to boot, so that second is not made faster — it does not exist.
//!
//! # What this runs
//!
//! The image is assembled here rather than shipped as a binary, so this
//! example has no missing-asset failure mode and anyone can read exactly what
//! the guest executes. It is thirteen instructions of 16-bit real mode:
//! point `dx` at COM1 and write a string one byte at a time, then halt.
//!
//! ```text
//!   mov dx, 0x3F8     BA F8 03     COM1, the port Machine::legacy_pc maps
//!   mov al, 'H'       B0 48
//!   out dx, al        EE           -> the emulated 16550 in this process
//!   ...                            one pair per character
//!   hlt               F4           done; the vCPU stops asking for time
//! ```
//!
//! Every `out` leaves the guest, is decoded by this process, and lands in a
//! device model. So a byte arriving on the host console proves the whole path:
//! the image was loaded at the right address, the vCPU started in the right
//! mode at the right instruction, the I/O exit was decoded, and the port was
//! routed to the device that claims it. An empty console proves none of it,
//! which is why this reports the console contents rather than a success line.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example unikernel_boot
//! ```
//!
//! Needs a hypervisor backend: `/dev/kvm` on Linux, or Windows Hypervisor
//! Platform. Without one it says so and exits non-zero rather than printing a
//! time it did not measure.
//!
//! # Nine boots, in one process
//!
//! It used to boot once. One boot is not a latency: this host's provision phase
//! alone varies by an order of magnitude run to run, so a single figure is a
//! measurement of whatever else the machine was doing. Worse, nine
//! *invocations* of the old version were once reported as a median of nine —
//! which folds process startup and image staging into every sample, and read
//! 3.4, 10.8, 12.4, 124 and 160 ms across five tries.
//!
//! So it boots nine times inside one process, as `rust_unikernel` does, and
//! prints the median with the range beside it. The range is the more honest
//! half: a median with no spread next to it invites exactly the mistake that
//! made the old number wrong.

use hv2_core::{BootSource, VMConfig, VM};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What the guest writes. Recognisable, and short enough to stay well inside
/// one page at the load address.
const GREETING: &str = "HYPERMACHINE UNIKERNEL\n";

/// COM1's data port, which `Machine::legacy_pc` maps to a 16550.
const COM1: u16 = 0x3F8;

/// Assemble the guest: write `text` to COM1, then halt.
///
/// Unrolled rather than looped. A loop is four bytes shorter and needs two
/// hand-computed jump displacements; at this size the straight-line version is
/// the one a reader can check against the encoding table without trusting the
/// author's arithmetic.
fn assemble(text: &str) -> Vec<u8> {
    let mut image = Vec::with_capacity(3 + text.len() * 3 + 1);

    // mov dx, imm16 — the port stays in dx for every `out` below.
    image.push(0xBA);
    image.extend_from_slice(&COM1.to_le_bytes());

    for byte in text.bytes() {
        image.push(0xB0); // mov al, imm8
        image.push(byte);
        image.push(0xEE); // out dx, al
    }

    image.push(0xF4); // hlt
    image
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

/// How many boots to time.
///
/// This example used to boot once and print that figure as the cold start. Nine
/// invocations of it were then reported as "median of nine", which is a
/// different measurement and a worse one: each invocation pays process startup,
/// image staging and whatever else the machine was doing, and five runs on this
/// host read 3.4, 10.8, 12.4, 124 and 160 ms. Nine boots *inside one process*
/// is what `rust_unikernel` does and what makes a median mean anything.
const RUNS: usize = 9;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// One boot, from `VM::new` to the guest's greeting.
async fn boot_once(path: &Path) -> Result<(Phases, String), String> {
    let started = Instant::now();

    let config = VMConfig {
        name: "unikernel".to_string(),
        vcpu_count: 1,
        // 16 MiB. A unikernel this size needs a page; the rest is here because
        // a guest that faults outside its image should fault somewhere mapped
        // rather than confusing a wrong jump with a missing region.
        memory_size: 16 * 1024 * 1024,
        boot: Some(BootSource::raw(path)),
        ..Default::default()
    };

    let vm = Arc::new(VM::new(config).map_err(|e| {
        format!(
            "VM::new failed — {e}\nA hypervisor backend is required: /dev/kvm on Linux, or \
             Windows Hypervisor Platform."
        )
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

    // Poll for the guest's own output rather than sleeping a fixed time: the
    // question is how long the guest took, and a sleep would answer how long
    // the sleep was.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut console = String::new();
    let mut first_output = None;
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if !console.is_empty() {
            first_output.get_or_insert(started.elapsed());
            if console.contains(GREETING.trim_end()) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_micros(200)).await;
    }

    let stop_started = Instant::now();
    let _ = vm.stop().await;

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

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let image = assemble(GREETING);
    println!(
        "image         : {} bytes, assembled in-process, entry {:#x}",
        image.len(),
        hv2_core::boot::source::BOOT_SECTOR_ADDR
    );

    let dir = std::env::temp_dir().join("hv2-unikernel");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("could not create {}: {e}", dir.display());
        return std::process::ExitCode::FAILURE;
    }
    let path = dir.join("greet.bin");
    if let Err(e) = std::fs::write(&path, &image) {
        eprintln!("could not write {}: {e}", path.display());
        return std::process::ExitCode::FAILURE;
    }
    println!("timing        : {RUNS} boots in this process — median, and the spread across them");
    println!();

    let mut runs = Vec::with_capacity(RUNS);
    let mut console = String::new();
    for _ in 0..RUNS {
        match boot_once(&path).await {
            Ok((phases, said)) => {
                if console.is_empty() {
                    console = said;
                }
                runs.push(phases);
            }
            Err(e) => {
                eprintln!("boot          : FAILED — {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }

    // Median and range, not median alone. A median hides the thing that made
    // the old figure wrong: on this host the provision phase varies by an order
    // of magnitude run to run, and a reader who cannot see that will treat one
    // number as repeatable when it is not.
    let report = |label: &str, pick: fn(&Phases) -> Duration, note: &str| {
        let values: Vec<f64> = runs.iter().map(|p| ms(pick(p))).collect();
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        println!(
            "{label:<14}: {:>8.3} ms   ({:.3} to {:.3})  {note}",
            median(values),
            low,
            high
        );
    };
    report("VM::new", |p| p.created, "");
    report("provision", |p| p.provisioned, "(cumulative)");
    report("launch", |p| p.launched, "(cumulative)");
    report("first output", |p| p.first_output, "(cumulative)");
    report("stop", |p| p.stopped, "");

    println!();
    println!("console       : {console:?}");
    println!();

    if console.contains(GREETING.trim_end()) {
        let first: Vec<f64> = runs.iter().map(|p| ms(p.first_output)).collect();
        println!(
            "result        : the guest executed on all {} boots. {} bytes of guest code reached \
             a usable state in {:.3} ms, median of {RUNS} boots in one process.",
            runs.len(),
            image.len(),
            median(first)
        );
        std::process::ExitCode::SUCCESS
    } else if console.is_empty() {
        println!(
            "result        : FAILED — nothing reached COM1, so no guest code ran. The image \
             loaded and the vCPU started, but neither is evidence on its own."
        );
        std::process::ExitCode::FAILURE
    } else {
        println!(
            "result        : FAILED — the guest wrote something other than the greeting, so it \
             executed the wrong bytes"
        );
        std::process::ExitCode::FAILURE
    }
}
