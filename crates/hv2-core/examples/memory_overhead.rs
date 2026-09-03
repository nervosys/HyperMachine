//! Measure what a running VM actually costs the host, in resident memory.
//!
//! The other number a sandbox is judged on. CubeSandbox publishes "less than
//! 5MB of memory overhead" per sandbox, at guest sizes up to 32GB, and rests
//! its "thousands of instances per server" claim on it.
//!
//! That figure is only meaningful if guest RAM is faulted in lazily. Until
//! recently this project allocated it with `alloc_zeroed` at page alignment,
//! which memsets — so a 1 GiB guest cost 1 GiB of host RAM before executing an
//! instruction, and any density claim was arithmetic about memory the host had
//! already committed. This measures whether that is actually fixed.
//!
//! # What it measures
//!
//! Resident set size of this process, sampled before any VM exists and again
//! after each one is created and launched. The VMs are kept alive, because the
//! question is what N concurrent sandboxes cost, not what one costs
//! transiently.
//!
//! RSS rather than virtual size. A lazily mapped 1 GiB guest reserves 1 GiB of
//! address space and occupies almost none of it; reporting the reservation
//! would give the flattering number for density and the wrong one.
//!
//! # What it does not measure
//!
//! What the guest goes on to touch. A unikernel that writes one page costs one
//! page; a Linux guest that boots touches tens of megabytes and this reports
//! that honestly. The figure here is the floor — the cost of the VM itself —
//! which is what "overhead per sandbox" means and what a density estimate
//! needs.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example memory_overhead -- --count 20 --memory-gb 1
//! ```
//!
//! Linux only: it reads `/proc/self/statm`. Needs `/dev/kvm`.

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;

/// Resident set size of this process, in bytes.
///
/// From `/proc/self/statm`, whose second field is resident pages. `/proc` is
/// the only place this is available without a dependency, and the alternative
/// — asking the allocator — would report what this process requested rather
/// than what the kernel has actually given it, which is the whole distinction
/// being measured.
#[cfg(target_os = "linux")]
fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    Some(resident_pages * page_size)
}

#[cfg(not(target_os = "linux"))]
fn rss_bytes() -> Option<u64> {
    None
}

/// The guest: write one byte to COM1 so the VM has demonstrably run, then
/// halt. Deliberately the smallest guest that proves it executed, so the
/// number is the VM's cost and not the workload's.
fn unikernel_image() -> Vec<u8> {
    vec![
        0xBA, 0xF8, 0x03, // mov dx, 0x3F8
        0xB0, 0x2E, // mov al, '.'
        0xEE, // out dx, al
        0xF4, // hlt
    ]
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut count = 10usize;
    let mut memory_gb = 1u64;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--count" if i + 1 < args.len() => {
                count = args[i + 1].parse().unwrap_or(count);
                i += 1;
            }
            "--memory-gb" if i + 1 < args.len() => {
                memory_gb = args[i + 1].parse().unwrap_or(memory_gb);
                i += 1;
            }
            other => {
                eprintln!("unrecognised argument {other}");
                return std::process::ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let Some(baseline) = rss_bytes() else {
        eprintln!("memory_overhead: this measurement reads /proc/self/statm and needs Linux.");
        return std::process::ExitCode::FAILURE;
    };

    let dir = std::env::temp_dir().join("hv2-unikernel");
    let path = dir.join("tick.bin");
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, unikernel_image()).is_err() {
        eprintln!("could not write the guest image to {}", path.display());
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "measuring {count} concurrent unikernel VMs, {memory_gb} GiB guest each\n\
         baseline RSS  : {:.2} MiB",
        mib(baseline)
    );

    // Held, not dropped: the question is what N concurrent sandboxes cost.
    let mut vms: Vec<Arc<VM>> = Vec::with_capacity(count);
    let mut samples = Vec::with_capacity(count);

    for index in 0..count {
        let config = VMConfig {
            name: format!("overhead-{index}"),
            vcpu_count: 1,
            memory_size: memory_gb * 1024 * 1024 * 1024,
            boot: Some(BootSource::raw(&path)),
            ..Default::default()
        };

        // Progress on stderr: a run that blocks should say which VM it
        // blocked on rather than going quiet after the baseline.
        eprint!("creating VM {}/{count}... ", index + 1);
        let vm = match VM::new(config) {
            Ok(vm) => Arc::new(vm),
            Err(e) => {
                eprintln!("VM {index}: could not be created — {e}");
                eprintln!("A hypervisor backend is required: /dev/kvm on Linux.");
                return std::process::ExitCode::FAILURE;
            }
        };
        if let Err(e) = vm.provision().await {
            eprintln!("VM {index}: provision failed — {e}");
            return std::process::ExitCode::FAILURE;
        }
        if let Err(e) = vm.launch().await {
            eprintln!("VM {index}: launch failed — {e}");
            return std::process::ExitCode::FAILURE;
        }
        vms.push(vm);

        if let Some(rss) = rss_bytes() {
            samples.push(rss);
        }
    }

    eprintln!();

    // Give the guests a moment to run and touch what they touch, so the number
    // includes a VM that has executed rather than one that has only been
    // configured.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let settled = rss_bytes().unwrap_or(baseline);

    let ran = {
        let mut ran = 0usize;
        for vm in &vms {
            if !vm.console_output().await.is_empty() {
                ran += 1;
            }
        }
        ran
    };

    println!("after {count} VMs : {:.2} MiB", mib(settled));
    println!(
        "growth        : {:.2} MiB total, {:.3} MiB per VM",
        mib(settled.saturating_sub(baseline)),
        mib(settled.saturating_sub(baseline)) / count as f64
    );
    println!(
        "guests that ran: {ran} of {count}  (a VM that never executed is not a sandbox, and its \
         cost is not an overhead figure)"
    );

    if samples.len() >= 2 {
        // The first VM carries one-off costs — backend handles, lazily
        // initialised statics — that the tenth does not. The marginal figure is
        // the one a density estimate needs.
        let marginal = samples[samples.len() - 1].saturating_sub(samples[0]) as f64
            / (samples.len() - 1) as f64;
        println!(
            "marginal      : {:.3} MiB per additional VM",
            marginal / (1024.0 * 1024.0)
        );
    }

    println!(
        "\nreserved      : {:.0} GiB of address space across {count} guests, almost none of it \
         resident — which is the point",
        (memory_gb * count as u64) as f64
    );

    for vm in &vms {
        let _ = vm.stop().await;
    }

    if ran == 0 {
        eprintln!("\nNo guest produced output, so this measured configured VMs, not sandboxes.");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
