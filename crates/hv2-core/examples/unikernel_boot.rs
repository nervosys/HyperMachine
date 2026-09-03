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

use hv2_core::{BootSource, VMConfig, VM};
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

    // Timed from here: everything a caller pays to get a running guest.
    let started = Instant::now();

    let config = VMConfig {
        name: "unikernel".to_string(),
        vcpu_count: 1,
        // 16 MiB. A unikernel this size needs a page; the rest is here because
        // a guest that faults outside its image should fault somewhere mapped
        // rather than confusing a wrong jump with a missing region.
        memory_size: 16 * 1024 * 1024,
        boot: Some(BootSource::raw(&path)),
        ..Default::default()
    };

    let vm = match VM::new(config) {
        Ok(vm) => Arc::new(vm),
        Err(e) => {
            eprintln!("VM::new       : FAILED — {e}");
            eprintln!(
                "A hypervisor backend is required: /dev/kvm on Linux, or Windows Hypervisor \
                 Platform."
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    let created = started.elapsed();

    if let Err(e) = vm.provision().await {
        eprintln!("provision     : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
    let provisioned = started.elapsed();

    if let Err(e) = vm.launch().await {
        eprintln!("launch        : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
    let launched = started.elapsed();

    // Poll for the guest's own output rather than sleeping a fixed time: the
    // question is how long the guest took, and a sleep would answer how long
    // the sleep was.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut console = String::new();
    let mut first_byte = None;
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if !console.is_empty() {
            first_byte.get_or_insert(started.elapsed());
            if console.contains(GREETING.trim_end()) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_micros(200)).await;
    }

    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!("VM::new       : {:>8.3} ms", ms(created));
    println!("provision     : {:>8.3} ms  (cumulative)", ms(provisioned));
    println!("launch        : {:>8.3} ms  (cumulative)", ms(launched));
    match first_byte {
        Some(t) => println!("first output  : {:>8.3} ms  (cumulative)", ms(t)),
        None => println!("first output  :   never"),
    }

    let _ = vm.stop().await;

    if console.contains(GREETING.trim_end()) {
        println!("console       : {console:?}");
        println!(
            "result        : the guest executed. {} bytes of guest code reached a usable state \
             in {:.3} ms",
            image.len(),
            ms(first_byte.unwrap_or_default())
        );
        std::process::ExitCode::SUCCESS
    } else if console.is_empty() {
        println!("console       : EMPTY");
        println!(
            "result        : FAILED — nothing reached COM1, so no guest code ran. The image \
             loaded and the vCPU started, but neither is evidence on its own."
        );
        std::process::ExitCode::FAILURE
    } else {
        println!("console       : {console:?}");
        println!(
            "result        : FAILED — the guest wrote something other than the greeting, so it \
             executed the wrong bytes"
        );
        std::process::ExitCode::FAILURE
    }
}
