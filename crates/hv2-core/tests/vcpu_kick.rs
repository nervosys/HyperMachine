//! `stop()` must return for a guest that never exits to userspace.
//!
//! The defect these tests exist for: `KVM_RUN` blocks, and `VM::stop()` waits
//! on the vCPU threads. A halted guest sits in `kvm_vcpu_block` and a spinning
//! one never leaves the guest at all, so those threads never read the running
//! flag or the stop message — and `stop()` never returned, for any guest, for
//! the whole life of the crate.
//!
//! Each test asserts on the effect rather than on the mechanism: `stop()`
//! completes. A regression does not hang the suite, because every wait is
//! bounded and a bound that elapses is a failure.
//!
//! Requires `/dev/kvm`. Without it these skip rather than fail, so they are
//! not `#[ignore]`d: on a machine that can run them, the ordinary suite does.

#![cfg(target_os = "linux")]

use hv2_core::{BootSource, VMConfig, VmExit, VM};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Generous next to the microseconds a kicked vCPU actually takes. It is here
/// so a regression reports rather than wedges the suite.
const BOUND: Duration = Duration::from_secs(10);

/// Spin forever: `jmp` to itself. Takes no VM exits at all, so nothing the
/// VMM does outside the guest is observable to it.
const SPIN_FOREVER: &[u8] = &[0xEB, 0xFE];

/// Halt immediately. Exits once, then blocks in the kernel waiting for an
/// interrupt that is never coming.
const HALT_ONLY: &[u8] = &[0xF4];

/// Write one byte to COM1, then halt — a guest that has demonstrably run.
const WRITE_THEN_HALT: &[u8] = &[
    0xBA, 0xF8, 0x03, // mov dx, 0x3F8
    0xB0, 0x2E, // mov al, '.'
    0xEE, // out dx, al
    0xF4, // hlt
];

/// Build a VM around `image`, or `None` if this host has no hypervisor that
/// executes guest code — in which case there is no ioctl to be stuck in and
/// nothing for these tests to say.
fn vm_for(name: &str, image: &[u8]) -> Option<Arc<VM>> {
    let dir = std::env::temp_dir().join("hv2-vcpu-kick");
    let path = dir.join(format!("{name}.bin"));
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(&path, image).ok()?;

    let config = VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 16 * 1024 * 1024,
        boot: Some(BootSource::raw(&path)),
        ..Default::default()
    };

    let vm = VM::new(config).ok()?;
    if !vm.executes_guest_code() {
        eprintln!("{name}: no hardware backend on this host; skipped");
        return None;
    }
    Some(Arc::new(vm))
}

/// Launch, let the guest reach whatever state it is going to reach, then stop
/// it and report how long that took.
async fn launch_then_stop(vm: &Arc<VM>) -> Duration {
    vm.provision().await.expect("provision");
    vm.launch().await.expect("launch");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let started = Instant::now();
    tokio::time::timeout(BOUND, vm.stop())
        .await
        .unwrap_or_else(|_| panic!("stop() did not return within {BOUND:?}"))
        .expect("stop");
    started.elapsed()
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_returns_for_a_spinning_guest() {
    let Some(vm) = vm_for("spin-forever", SPIN_FOREVER) else {
        return;
    };
    // The hard case. This guest takes no exits, so the only thing that can
    // reach its vCPU thread is a signal delivered to it directly.
    let took = launch_then_stop(&vm).await;
    eprintln!("spin-forever: stop() returned in {took:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_returns_for_a_halted_guest() {
    let Some(vm) = vm_for("halt-only", HALT_ONLY) else {
        return;
    };
    let took = launch_then_stop(&vm).await;
    eprintln!("halt-only: stop() returned in {took:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_returns_for_a_guest_that_ran_first() {
    let Some(vm) = vm_for("write-then-halt", WRITE_THEN_HALT) else {
        return;
    };
    let took = launch_then_stop(&vm).await;

    // Proof the guest executed, so this is not a VM that stopped easily by
    // never having started.
    assert_eq!(
        vm.console_output().await,
        ".",
        "the guest should have written to COM1 before halting"
    );
    eprintln!("write-then-halt: stop() returned in {took:?}");
}

/// A kick that arrives before the vCPU enters the guest must be honoured at
/// the next entry, not dropped.
///
/// This is the race that would otherwise make shutdown hang intermittently
/// instead of always: `stop()` kicks a vCPU thread that is between `KVM_RUN`
/// calls, the signal lands on a thread that is not in the ioctl, and the vCPU
/// then walks into a guest it can never be recalled from. Asserting the exit
/// is `Interrupted` says the request was latched and that no guest
/// instructions ran under it.
#[tokio::test(flavor = "multi_thread")]
async fn a_kick_before_entry_is_not_lost() {
    use hv2_core::backends::kvm::KvmBackend;
    use hv2_core::hypervisor::HypervisorBackend;

    let Ok(mut backend) = KvmBackend::new() else {
        eprintln!("no /dev/kvm on this host; skipped");
        return;
    };
    backend.init().await.expect("init");
    backend.create_vm(1, 16 * 1024 * 1024).await.expect("vm");

    let vcpu = hv2_core::VCpu::new(0);

    // Nothing is running: the vCPU has never been entered.
    backend.kick_vcpu(&vcpu).await.expect("kick");

    let exit = tokio::time::timeout(BOUND, backend.run_vcpu(&vcpu))
        .await
        .expect("run_vcpu should return immediately on a pending kick")
        .expect("run_vcpu");

    assert!(
        matches!(exit, VmExit::Interrupted),
        "a pending kick should end the run before the guest is entered, got {exit}"
    );

    // And the kick must be consumed rather than left latched. A vCPU that
    // reports `Interrupted` on every entry looks exactly like one making no
    // progress, which is what `immediate_exit` left set would produce.
    //
    // What the second entry *does* is deliberately not asserted: this vCPU has
    // no guest loaded, so it runs whatever a reset vector over zeroed memory
    // means and may exit or fault. Either is fine. Reporting `Interrupted` a
    // second time is not.
    let exit = tokio::time::timeout(BOUND, backend.run_vcpu(&vcpu))
        .await
        .expect("run_vcpu should not hang after the kick was consumed");

    assert!(
        !matches!(exit, Ok(VmExit::Interrupted)),
        "the kick should have been consumed by the first run, not latched forever"
    );
}
