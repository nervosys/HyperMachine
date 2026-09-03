//! Does `stop()` return for a guest that halts and produces nothing?
//!
//! A diagnostic, not a demonstration. `stop()` awaits every vCPU task, and a
//! vCPU task only ends when `run_vcpu` returns — which, with an in-kernel
//! irqchip, it does not do while the guest sits in `HLT`. KVM blocks inside
//! the `KVM_RUN` ioctl (`wchan` reads `kvm_vcpu_block`), and clearing the
//! running flag cannot help a thread that is not running.
//!
//! Whether that actually wedges shutdown was unclear, because a thousand
//! halting guests were stopped without trouble a few commits ago. This
//! narrows it: three guests differing only in what they do before halting.
//!
//! Each is timed with a bound, so a hang is a reported failure rather than a
//! hung terminal.

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Halt immediately. Nothing written, no exit to userspace first.
fn halt_only() -> Vec<u8> {
    vec![0xF4]
}

/// Write one byte, then halt — the shape every working example has used.
fn write_then_halt() -> Vec<u8> {
    vec![
        0xBA, 0xF8, 0x03, // mov dx, 0x3F8
        0xB0, 0x2E, // mov al, '.'
        0xEE, // out dx, al
        0xF4, // hlt
    ]
}

/// Never halt: spin forever, so the vCPU keeps exiting to userspace.
fn spin_forever() -> Vec<u8> {
    vec![0xEB, 0xFE] // jmp -2, to itself
}

async fn probe(label: &str, image: Vec<u8>) -> bool {
    let dir = std::env::temp_dir().join("hv2-unikernel");
    let path = dir.join(format!("{label}.bin"));
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, &image).is_err() {
        println!("{label:<16}: could not write the image");
        return false;
    }

    let config = VMConfig {
        name: format!("halt-probe-{label}"),
        vcpu_count: 1,
        memory_size: 16 * 1024 * 1024,
        boot: Some(BootSource::raw(&path)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        println!("{label:<16}: no hypervisor backend");
        return false;
    };
    let vm = Arc::new(vm);
    if vm.provision().await.is_err() || vm.launch().await.is_err() {
        println!("{label:<16}: could not start");
        return false;
    }

    // Let the guest reach whatever state it is going to reach.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let output = vm.console_output().await;

    let started = Instant::now();
    let stopped = tokio::time::timeout(Duration::from_secs(20), vm.stop()).await;
    match stopped {
        Ok(_) => {
            println!(
                "{label:<16}: stop() returned in {:>7.1} ms   console {output:?}",
                started.elapsed().as_secs_f64() * 1000.0
            );
            true
        }
        Err(_) => {
            println!("{label:<16}: stop() DID NOT RETURN within 20 s   console {output:?}");
            false
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut all_returned = true;
    all_returned &= probe("write-then-halt", write_then_halt()).await;
    all_returned &= probe("halt-only", halt_only()).await;
    all_returned &= probe("spin-forever", spin_forever()).await;

    if all_returned {
        println!("\nstop() returned in every case.");
        std::process::ExitCode::SUCCESS
    } else {
        println!("\nAt least one guest wedged stop(). The cases that differ say which.");
        std::process::ExitCode::FAILURE
    }
}
