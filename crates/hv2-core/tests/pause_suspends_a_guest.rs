//! Does `pause()` actually stop a guest executing?
//!
//! `VM::pause` could not succeed at all until now: it required a vCPU state
//! nothing ever wrote, so it failed on the first vCPU of every VM. Asserting
//! that it returns `Ok` would therefore be a weaker test than it looks -- the
//! old one failed, the new one succeeds, and neither says whether the guest
//! stopped.
//!
//! So this watches the guest instead. The unikernel prints continuously; a
//! paused one stops adding to its console, and a resumed one starts again.
//! Needs `/dev/kvm` and the `x86_64-unknown-none` target, and skips cleanly
//! without them, as the other guest-booting tests here do.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use hv2_core::{BootSource, VMConfig, VMState, VM};

const GUEST_TARGET: &str = "x86_64-unknown-none";

/// Build the unikernel, or `None` if this host cannot.
fn build_guest() -> Option<PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv2-unikernel");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let elf = crate_dir
        .join("target")
        .join(GUEST_TARGET)
        .join("release")
        .join("hv2-unikernel");
    elf.exists().then_some(elf)
}

/// How much console the guest has produced.
async fn console_len(vm: &Arc<VM>) -> usize {
    vm.console_output().await.len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_guest_cannot_answer_and_a_resumed_one_can() {
    let Some(elf) = build_guest() else {
        eprintln!("skipping: the guest could not be built on this host");
        return;
    };

    let config = VMConfig {
        name: "pause-test".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&elf)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("skipping: no hypervisor backend (needs /dev/kvm)");
        return;
    };
    let vm = Arc::new(vm);

    if vm.provision().await.is_err() {
        eprintln!("skipping: could not provision");
        return;
    }
    let Ok(vsock) = vm.attach_vsock(3).await else {
        eprintln!("skipping: could not attach vsock");
        return;
    };
    if vm.launch().await.is_err() {
        eprintln!("skipping: could not launch");
        return;
    }

    // Wait until the guest has finished booting and is idle in `hlt`.
    //
    // Idle is the point. An earlier version of this test watched the console
    // for growth and asserted a paused guest stops adding to it -- but this
    // guest halts between messages, so a *running* idle guest also adds
    // nothing. That test passed its pause assertion for the wrong reason and
    // then failed its resume assertion for the same one.
    //
    // What distinguishes suspended from idle is whether the guest can be
    // *woken*. A vsock connection raises an interrupt; a running guest answers
    // it and prints, a suspended one cannot.
    let mut booted = false;
    for _ in 0..100 {
        if vm.console_output().await.contains("vsock cid") {
            booted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if !booted {
        eprintln!("skipping: the guest never reported its vsock driver");
        let _ = vm.stop().await;
        return;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    vm.pause().await.expect("pause should now succeed");
    assert_eq!(vm.state(), VMState::Paused);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let while_paused = console_len(&vm).await;

    // Knock on the door. A running guest prints "vsock connect"; this one is
    // suspended and cannot.
    let opened = vsock.lock().connect(1024, 5000);
    assert!(opened.is_ok(), "the host side should queue the request");

    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        console_len(&vm).await,
        while_paused,
        "a suspended guest answered a vsock connection: it is not suspended, \
         only marked as such"
    );

    vm.resume().await.expect("resume");
    assert_eq!(vm.state(), VMState::Running);

    // And now it can. The request is still queued, so it is answered as soon
    // as the vCPU runs again.
    let mut answered = while_paused;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        answered = console_len(&vm).await;
        if answered > while_paused {
            break;
        }
    }
    assert!(
        answered > while_paused,
        "a resumed guest never answered the connection it was sent while \
         suspended ({while_paused} bytes of console before, {answered} after): \
         pause stopped it permanently rather than suspending it"
    );

    let _ = vm.stop().await;
}

/// The state machine still refuses what it should.
#[tokio::test]
async fn pausing_a_vm_that_is_not_running_is_refused() {
    let config = VMConfig {
        name: "pause-state-test".to_string(),
        vcpu_count: 1,
        memory_size: 16 * 1024 * 1024,
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        return;
    };
    let vm = Arc::new(vm);

    // Created, never started.
    assert!(vm.pause().await.is_err(), "a VM that is not running");
    assert!(vm.resume().await.is_err(), "and one that is not paused");
}

/// A VM that was `start()`ed but never launched has no vCPU tasks. Pausing it
/// must say so rather than report success over nothing -- that shape is what
/// the old implementation's failure was mistaken for.
#[tokio::test]
async fn pausing_a_started_but_unlaunched_vm_says_there_is_nothing_to_suspend() {
    let config = VMConfig {
        name: "pause-unlaunched".to_string(),
        vcpu_count: 1,
        memory_size: 16 * 1024 * 1024,
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        return;
    };
    let vm = Arc::new(vm);
    if vm.start().await.is_err() {
        return;
    }

    let err = vm.pause().await.expect_err("nothing is running to suspend");
    assert!(
        err.to_string().contains("no running vCPU tasks"),
        "the error should name the cause, not a vCPU's state: {err}"
    );
}
