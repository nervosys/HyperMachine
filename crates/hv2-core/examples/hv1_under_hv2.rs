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
    // it writes is `hv1   done`, and anything short of that is a guest that
    // stopped somewhere it did not choose.
    //
    // The whole line, not the word. This matched on "done" until hv1 gained an
    // exit description ending "and is done" — after which it stopped reading
    // partway through the log and reported every later claim as absent. A
    // sentinel that can appear inside ordinary output is not a sentinel.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut console = String::new();
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if console.contains("hv1   done") {
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
    // A guest ran under hv1, exited for a reason it chose, was resumed past
    // the instruction that caused the exit, and exited again. One entry
    // proves a guest ran; two prove a loop, which is what makes it a
    // hypervisor rather than a launcher.
    let guest_ran = console.contains("VMEXIT_VMMCALL");
    let guest_resumed = console.contains("VMEXIT_HLT");

    // The three things that make the loop above a hypervisor rather than an
    // exit log, each asserted from the guest's side of the boundary: the
    // console line was written by the guest one intercepted `out` at a time,
    // the handler line can only be reached by the CPU taking a vector through
    // the guest's own table, and the third only by an `iret` returning from it.
    let emulated_a_device = console.contains("hello from a guest of hv1");
    let crossed_to_protected = console.contains("crossed into 32-bit mode");
    let drove_a_ring = console.contains("a request through a ring");
    let ring_returned = console.contains("read the reply back out of the buffer it named");
    let refused_out_of_range = console.contains("outside the guest's own memory was not followed");
    let second_processor = console.contains("the first processor saw its work in shared memory");
    // The bring-up itself, not just its result: two writes to the APIC page,
    // decoded out of the guest's own instruction stream. The second processor
    // could not have reached 32-bit code any other way — it was started with
    // CR0.PE clear at a page number, so the trampoline is the only thing that
    // could have set it.
    let bring_up = console.contains("INIT — the other processor is held at reset")
        && console.contains("STARTUP — the other processor begins in real mode");
    let half_a_start = console.contains("a startup that skipped the reset was refused");
    // The identity register, which is the only one whose answer depends on who
    // asked. One instruction, at one address, run by both processors: if they
    // print different numbers there is a per-processor APIC behind it and not a
    // register the hypervisor keeps one copy of.
    let told_apart = console.contains("> cpu 0") && console.contains("> cpu 1");
    let apic_used = console.contains("software-enabled by the guest, 4 end-of-interrupt");
    // A timer the guest programmed, not a vector the hypervisor chose to send.
    // The guest picks the vector, the period and the mode; three interrupts
    // arrive on that vector through its own gate, and the count it reads back
    // is smaller the second time — which is what separates a timer from a
    // number the hypervisor remembered.
    let its_own_timer = console.contains("3 ticks on the vector the guest chose");
    let delivered_an_interrupt = console.contains("the guest's own handler ran");
    let guest_carried_on = console.contains("the handler's iret returned");

    println!("long mode     : {}", yes_no(long_mode));
    println!("hv1 executed  : {}", yes_no(reached_hv1));
    println!("hv1 initialised: {}", yes_no(initialised));
    println!(
        "CPU changed   : {}  (EFER.SVME, which is what svm::initialize() does)",
        yes_no(svme)
    );
    println!(
        "ran a guest   : {}  (VMEXIT_VMMCALL, which only a guest can produce)",
        yes_no(guest_ran)
    );
    println!(
        "resumed it    : {}  (VMEXIT_HLT, after stepping past the vmmcall)",
        yes_no(guest_resumed)
    );
    println!(
        "emulated a device: {}  (the guest's line arrived one intercepted `out` at a time)",
        yes_no(emulated_a_device)
    );
    println!(
        "crossed modes : {}  (its own GDT, CR0.PE and a far jump — the RIP in the exit log goes from 0x21 to 0x1060, a linear address, so the segment is flat)",
        yes_no(crossed_to_protected)
    );
    println!(
        "drove a ring  : {}  (a descriptor the guest filled in, followed to a buffer the guest chose)",
        yes_no(drove_a_ring)
    );
    println!(
        "and got a reply: {}  (written where the guest asked, read back, and put on the console)",
        yes_no(ring_returned)
    );
    println!(
        "refused a bad one: {}  (a descriptor pointing outside the guest's own memory)",
        yes_no(refused_out_of_range)
    );
    println!(
        "second processor: {}  (the first saw its work in shared memory)",
        yes_no(second_processor)
    );
    println!(
        "brought up      : {}  (INIT then STARTUP written to the local APIC page, decoded from the guest's instruction stream)",
        yes_no(bring_up)
    );
    println!(
        "refused half of it: {}  (a STARTUP for a processor that had never been reset did not start one)",
        yes_no(half_a_start)
    );
    println!(
        "told apart      : {}  (both processors ran one instruction at one address and read different identities)",
        yes_no(told_apart)
    );
    println!(
        "an APIC, used   : {}  (the guest software-enabled it, and its handlers wrote end-of-interrupt)",
        yes_no(apic_used)
    );
    println!(
        "a timer of its own: {}  (the guest armed it, its count fell between two reads, and three interrupts arrived on the vector it picked)",
        yes_no(its_own_timer)
    );
    // Not a claim, a measurement, and the reason the timer's period is what it
    // is: the first version used a period of 200,000 ticks and the timer
    // sometimes expired inside the two exits it took to read the count back.
    if let Some(cost) = console
        .lines()
        .find_map(|line| line.split("exit cost : ").nth(1))
    {
        println!("a nested exit costs: {}", cost.trim());
    }
    println!(
        "delivered an interrupt: {}  (injected 0x20 into a halted guest; its own handler ran)",
        yes_no(delivered_an_interrupt)
    );
    println!(
        "and it carried on: {}  (the handler's `iret` returned and the guest kept going)",
        yes_no(guest_carried_on)
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

    if initialised && svme && !guest_ran {
        println!(
            "result        : hv1-core initialised and no guest ran. The exit codes above \
             say how far VMRUN got — VMEXIT_INVALID means the VMCB was refused before any \
             guest instruction executed, and VMEXIT_NPF means the nested page tables did \
             not translate the guest's first fetch."
        );
        return std::process::ExitCode::FAILURE;
    }

    if guest_ran && !guest_resumed {
        println!(
            "result        : a guest entered and exited once, and did not survive being \
             resumed. The exit loop is where to look: an intercepted instruction the \
             hypervisor does not step over is re-executed forever."
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

    if initialised
        && emulated_a_device
        && crossed_to_protected
        && drove_a_ring
        && ring_returned
        && refused_out_of_range
        && second_processor
        && bring_up
        && half_a_start
        && told_apart
        && apic_used
        && its_own_timer
        && delivered_an_interrupt
        && guest_carried_on
    {
        println!(
            "result        : hv1-core initialised on a real CPU and was a hypervisor to a guest. It emulated the serial port the guest wrote to, answered its hypercalls while the guest crossed from real mode into protected mode under its own GDT, took a request through a descriptor ring the guest filled in and left a reply where the guest asked for one, refused a descriptor pointing outside the guest's own memory, started a second processor the way hardware does — INIT and STARTUP written to the local APIC page, faulted out of the nested tables and decoded from the guest's own instruction stream, with the second processor beginning in real mode at the page the vector named — and scheduled the two of them onto the one it has, gave both of them an identity to read and a timer to arm, delivered three ticks on the vector the guest itself chose, and injected an interrupt the guest took through a gate in its own IDT and returned from. Still not bare metal: the layer underneath is KVM, and the APIC is this hypervisor's rather than a real one — nine registers of one, counting a timer whose tick is a timestamp-counter tick because there is no bus clock to read — so the firmware path, the real memory map and every real device remain untested."
        );
    } else if initialised {
        println!(
            "result        : hv1-core initialised and entered a guest, and at least one of the three things above did not happen. The exit log says which — an exit with no answer beside it is the one to read."
        );
        return std::process::ExitCode::FAILURE;
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
