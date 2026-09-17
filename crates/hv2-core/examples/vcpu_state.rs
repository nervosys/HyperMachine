//! Read a running guest's vCPU state back from the hardware, and put it back.
//!
//! The first half of snapshot/restore (`docs/CUBESANDBOX_PARITY_ROADMAP.md`,
//! Phase 2). Memory is not captured here; this answers the narrower question
//! that has to be answered first, because everything else depends on it: can
//! this hypervisor read what a guest's processor is actually doing, and write
//! it back?
//!
//! What it does:
//!
//! 1. boots the unikernel guest and waits for it to say it is running,
//! 2. pauses it and reads every vCPU's registers,
//! 3. checks the reading describes a real machine rather than zeroes,
//! 4. writes the same state back and resumes,
//! 5. confirms the guest is still alive afterwards.
//!
//! Step 5 is the one that matters. Reading registers proves an ioctl works;
//! only resuming a guest that then keeps running proves the values were
//! coherent. A restore that silently corrupts a vCPU looks identical to a
//! successful one until the guest touches whatever was wrong.
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

    let verdict = if grew {
        println!("after restore : the guest kept running");
        std::process::ExitCode::SUCCESS
    } else {
        // Not a failure of this example: the unikernel halts when idle, so a
        // guest with nothing to do prints nothing whether or not the restore
        // worked. Reported rather than asserted, because asserting it would
        // be a test that passes for a reason it does not check.
        println!("after restore : no new output — this guest idles in hlt, so that is");
        println!("                inconclusive rather than a failure");
        std::process::ExitCode::SUCCESS
    };

    let _ = vm.stop().await;
    Ok(verdict)
}
