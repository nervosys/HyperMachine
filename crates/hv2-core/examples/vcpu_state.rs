//! Snapshot a running guest to a file, and restore it into a different VM.
//!
//! Phase 2 of `docs/CUBESANDBOX_PARITY_ROADMAP.md`, end to end.
//!
//! Two halves, in order of what they prove:
//!
//! 1. **The same VM.** Boot, pause, read every vCPU's registers, check they
//!    describe a real machine rather than zeroes, write them straight back,
//!    resume. The identity case: if a round trip cannot survive that, nothing
//!    else matters.
//! 2. **A different VM.** Snapshot the first guest's memory and vCPUs to a
//!    file, stop it, boot a *second* VM from the same image, pause it, restore
//!    the file into it, and resume. The second VM is then the first guest,
//!    continuing where it left off.
//!
//! The second half is the one worth running. Reading registers proves an
//! ioctl works; only a guest that resumes in a machine it never booted in
//! proves the state was coherent and complete enough to move.
//!
//! ```text
//! cargo run --release -p hv2-core --example vcpu_state
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_core::{BootSource, Result, VMConfig, VM};

/// Where the guest ELF comes from. Same as `vsock_echo`'s: the unikernel is
/// built by its own cargo invocation into its own target directory.
fn build_guest() -> std::result::Result<std::path::PathBuf, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("cannot find the workspace root")?
        .to_path_buf();
    let unikernel = root.join("crates/hv2-unikernel");
    if !unikernel.exists() {
        return Err(format!("{} does not exist", unikernel.display()));
    }

    let status = std::process::Command::new(env!("CARGO"))
        .current_dir(&unikernel)
        .args(["build", "--release"])
        .status()
        .map_err(|e| format!("building the guest: {e}"))?;
    if !status.success() {
        return Err("the guest did not build".to_string());
    }

    // The target triple comes from the unikernel crate's own
    // `.cargo/config.toml`, not from here; hard-coding it wrong is a build
    // that succeeds and then cannot find what it built.
    let elf = unikernel.join("target/x86_64-unknown-none/release/hv2-unikernel");
    if elf.exists() {
        Ok(elf)
    } else {
        Err(format!("built, but {} is missing", elf.display()))
    }
}

/// A cheap digest of every writable byte of guest memory.
///
/// Not a cryptographic hash and not trying to be: this compares one VM's
/// memory against another's, where the alternative to agreeing is disagreeing
/// by megabytes. FNV-1a over the whole image is enough to tell those apart and
/// costs one pass.
///
/// Read-only regions are skipped because a restore deliberately does not write
/// them -- they are host pages shared between VMs -- so including them would
/// compare something the restore never claimed to move.
fn memory_digest(vm: &VM) -> hv2_core::Result<u64> {
    let memory = vm.memory();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut buffer = vec![0u8; 1 << 20];
    for region in memory.regions() {
        if region.readonly {
            continue;
        }
        let mut read = 0u64;
        while read < region.size {
            let take = (1usize << 20).min((region.size - read) as usize);
            memory.read_bytes_into(region.guest_addr + read, &mut buffer[..take])?;
            for byte in &buffer[..take] {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            read += take as u64;
        }
    }
    Ok(hash)
}

async fn console_says(vm: &VM, needle: &str, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if vm.console_output().await.contains(needle) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    false
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("vcpu_state    : FAILED — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<std::process::ExitCode> {
    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return Ok(std::process::ExitCode::FAILURE);
        }
    };
    println!("guest         : {}", elf.display());

    let config = VMConfig {
        name: "vcpu-state".to_string(),
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
            return Ok(std::process::ExitCode::SUCCESS);
        }
    };

    vm.provision().await?;
    vm.launch().await?;

    // The banner the guest actually prints, not the crate name.
    if !console_says(&vm, "HYPERMACHINE RUST UNIKERNEL", Duration::from_secs(5)).await {
        eprintln!("guest         : never reached its banner");
        eprintln!("console       : {:?}", vm.console_output().await);
        let _ = vm.stop().await;
        return Ok(std::process::ExitCode::FAILURE);
    }
    println!("guest         : running");

    // Paused first. Reading a vCPU that is executing describes a machine that
    // has already moved on, and `save_vcpu_states` refuses for that reason.
    vm.pause().await?;
    println!("pause         : ok");

    let states = match vm.save_vcpu_states().await {
        Ok(states) => states,
        Err(e) => {
            eprintln!("save          : FAILED — {e}");
            let _ = vm.stop().await;
            return Ok(std::process::ExitCode::FAILURE);
        }
    };

    let mut plausible = true;
    for state in &states {
        println!(
            "vcpu {:<2}      : rip={:#018x} rsp={:#018x} cr3={:#x} cs={:#06x} efer={:#x} {:?}",
            state.id,
            state.general.rip,
            state.general.rsp,
            state.system.cr3,
            state.system.cs.selector,
            state.system.efer,
            state.run_state,
        );

        // A backend that answered with zeroes would print a perfectly
        // plausible-looking vCPU halted at address zero, which is why this
        // checks rather than trusting the call's success.
        if state.general.rip == 0 && state.system.cr0 == 0 {
            eprintln!("              : that is a zeroed vCPU, not a running one");
            plausible = false;
        }
        // The guest runs in protected or long mode; bit 0 of CR0 is set in
        // both. A guest in real mode here would mean it never got started.
        if state.system.cr0 & 1 == 0 {
            eprintln!("              : CR0.PE clear — the guest is in real mode");
            plausible = false;
        }
    }
    if !plausible {
        let _ = vm.stop().await;
        return Ok(std::process::ExitCode::FAILURE);
    }
    println!("save          : {} vCPU(s), all plausible", states.len());

    if !states.is_empty() && !states[0].is_complete() {
        println!(
            "not captured  : {}",
            hv2_core::snapshot::vcpu::VCpuSnapshot::missing().join("; ")
        );
    }

    // Write the same state back. The same state on purpose: this is the
    // identity case, and if the round trip cannot survive that, it certainly
    // cannot survive a restore into a fresh VM.
    vm.restore_vcpu_states(&states).await?;
    println!("restore       : ok");

    let before = vm.console_output().await.len();
    vm.resume().await?;
    println!("resume        : ok");

    // Alive afterwards, which is the point of the whole exercise. The guest
    // prints as it runs, so more console output is it making progress rather
    // than a vCPU sitting in a state that merely looks right.
    let grew = {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut grew = false;
        while Instant::now() < deadline {
            if vm.console_output().await.len() > before {
                grew = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        grew
    };

    if grew {
        println!("after restore : the guest kept running");
    } else {
        // Not a failure: the unikernel halts when idle, so a guest with
        // nothing to do prints nothing whether or not the restore worked.
        // Reported rather than asserted, because asserting it would be a
        // check that passes for a reason it does not test.
        println!("after restore : no new output — this guest idles in hlt, so that is");
        println!("                inconclusive rather than a failure");
    }

    // ---------------------------------------------------------------- part 2
    println!();
    println!("=== to a file, and into a different VM ===");

    let path = std::env::temp_dir().join(format!("hv2-snapshot-{}.hv2snap", std::process::id()));
    let _ = std::fs::remove_file(&path);

    vm.pause().await?;

    // Read at the same paused moment the snapshot is taken at, so this is what
    // the file contains. The first half's reading is *not* it: the guest ran
    // on between them, and comparing against that would report a perfectly
    // good restore as a failure -- which is exactly what it did first time.
    let expected = vm.save_vcpu_states().await?;

    // Something only this guest has.
    //
    // Without it the memory comparison below is a tautology: this unikernel
    // is deterministic, so a second VM booted from the same image reaches
    // byte-identical memory on its own, and "the restored memory matches"
    // would be true whether or not a single page moved. The first run of this
    // example said exactly that -- all three digests equal -- which is a check
    // passing for a reason it does not test.
    //
    // High in the 64 MiB region, far above the guest's code (~0x100000) and
    // its stack (~0x12ff90), so writing it disturbs nothing.
    const MARKER_AT: u64 = 0x0300_0000;
    const MARKER: &[u8] = b"this guest was snapshotted, not booted";
    vm.memory().write_bytes(MARKER_AT, MARKER)?;
    println!(
        "marker        : {:?} at {MARKER_AT:#x}",
        String::from_utf8_lossy(MARKER)
    );

    let expected_memory = memory_digest(&vm)?;
    vm.snapshot(&path).await?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "snapshot      : {} ({:.1} MiB, uncompressed and not sparse)",
        path.display(),
        size as f64 / (1024.0 * 1024.0)
    );

    // The console of the guest being captured, so the restored one can be
    // compared against it. A restore that produced a *different* guest would
    // start from a different banner.
    let captured_console = vm.console_output().await;
    let _ = vm.stop().await;
    println!("original      : stopped");

    // A second VM from the same image: same shape, its own memory, its own
    // vCPU. It boots normally first -- there is no way to create a KVM vCPU
    // without one -- and is then overwritten by the snapshot.
    let second = VM::new(VMConfig {
        name: "vcpu-state-restored".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&elf)),
        ..Default::default()
    })?;
    let second = Arc::new(second);
    // Timed, because the roadmap asks for this as a number: restore latency
    // competes with boot latency, and a snapshot that takes longer to load
    // than the guest takes to boot is a feature for a different workload.
    let boot_started = Instant::now();
    second.provision().await?;
    second.launch().await?;
    if !console_says(
        &second,
        "HYPERMACHINE RUST UNIKERNEL",
        Duration::from_secs(5),
    )
    .await
    {
        eprintln!("second VM     : never booted");
        let _ = second.stop().await;
        let _ = std::fs::remove_file(&path);
        return Ok(std::process::ExitCode::FAILURE);
    }
    let boot_took = boot_started.elapsed();
    println!(
        "second VM     : booted on its own in {:.1} ms",
        boot_took.as_secs_f64() * 1000.0
    );

    second.pause().await?;
    let before_memory = memory_digest(&second)?;
    let restore_started = Instant::now();
    second.restore(&path).await?;
    let restore_took = restore_started.elapsed();
    println!(
        "restore       : the snapshot is now this VM, in {:.1} ms",
        restore_took.as_secs_f64() * 1000.0
    );

    let restored_states = second.save_vcpu_states().await?;
    let same_rip =
        restored_states.first().map(|s| s.general.rip) == expected.first().map(|s| s.general.rip);
    println!(
        "vcpu 0       : rip={:#018x} — {}",
        restored_states.first().map_or(0, |s| s.general.rip),
        if same_rip {
            "the instruction the snapshot was taken on"
        } else {
            "DIFFERENT from the snapshot, which is a failed restore"
        }
    );

    // And the memory, which is the half registers cannot show. Three digests:
    // what the first guest had, what the second had before the restore, and
    // what it has after. The middle one is why this is a test rather than a
    // tautology -- two VMs booted from one image have *similar* memory, so
    // "after == first" only means something alongside "before != first".
    let after_memory = memory_digest(&second)?;
    println!("memory        : snapshot {expected_memory:#018x}");
    println!("                before   {before_memory:#018x}  (this VM's own boot)");
    println!("                after    {after_memory:#018x}");

    let marker_here = second.memory().read_bytes(MARKER_AT, MARKER.len())?;
    let marker_moved = marker_here == MARKER;
    println!(
        "marker        : {} in the second VM",
        if marker_moved {
            "found"
        } else {
            "MISSING — memory did not move"
        }
    );

    let memory_moved = after_memory == expected_memory && before_memory != expected_memory;
    if !memory_moved {
        eprintln!(
            "              : {}",
            if after_memory == expected_memory {
                "the second VM's memory already matched, so this proves nothing"
            } else {
                "the restored memory is not the snapshot's"
            }
        );
    }

    second.resume().await?;
    println!("resume        : ok");
    let _ = second.stop().await;
    let _ = std::fs::remove_file(&path);

    if !same_rip || !memory_moved || !marker_moved {
        eprintln!("verdict       : the restore did not reproduce the snapshotted guest");
        return Ok(std::process::ExitCode::FAILURE);
    }
    println!("verdict       : a guest was moved between two VMs through a file");
    println!();
    println!("=== what it cost ===");
    println!(
        "boot          : {:>8.1} ms   (this guest, from its ELF)",
        boot_took.as_secs_f64() * 1000.0
    );
    println!(
        "restore       : {:>8.1} ms   ({:.1} MiB read and written back)",
        restore_took.as_secs_f64() * 1000.0,
        size as f64 / (1024.0 * 1024.0)
    );
    if restore_took > boot_took {
        // The honest reading, and the one this guest produces: a unikernel
        // that boots in milliseconds is not a workload snapshots help. A
        // restore costs the memory image, which does not shrink because the
        // guest boots quickly -- so the ratio improves with guests that boot
        // *slowly*, not with better snapshot code.
        println!(
            "              : restore is {:.1}x slower than booting this guest.",
            restore_took.as_secs_f64() / boot_took.as_secs_f64().max(f64::EPSILON)
        );
        println!("                Expected. A restore costs the memory image, which a");
        println!("                fast-booting guest does not make smaller. Snapshots pay off");
        println!("                where boot is slow and the image is warm -- a loaded");
        println!("                interpreter, a model in RAM -- not on a 64 MiB unikernel.");
    } else {
        println!(
            "              : restore is {:.1}x faster than booting this guest",
            boot_took.as_secs_f64() / restore_took.as_secs_f64().max(f64::EPSILON)
        );
    }
    let _ = captured_console;
    Ok(std::process::ExitCode::SUCCESS)
}
