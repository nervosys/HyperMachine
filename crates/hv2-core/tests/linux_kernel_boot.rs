//! A real Linux kernel, booted on KVM, judged by what it says about itself.
//!
//! WHY THIS EXISTS: `examples/linux_boot_probe.rs` proved once, by hand, on one
//! machine, that this hypervisor can take a bzImage from cold to userspace
//! handoff. Nothing kept it true. Every other boot test in this crate builds a
//! synthetic bzImage and inspects the *host's* view -- registers, memory,
//! header fields -- which passes just as happily when `boot_params` is filled
//! in wrongly, because nothing ever reads it. A kernel reads all of it, field
//! by field, and complains on the serial console when it is wrong.
//!
//! The regressions this catches are the ones that survived every synthetic
//! test: an e820 table that describes memory the guest does not have, a
//! `cmdline_ptr` pointing somewhere the kernel cannot read, a 16550 that never
//! raises THR-empty so the real 8250 driver hangs where earlyprintk worked, an
//! absent CMOS or i8042 whose 0xff reads spin the guest forever, and any
//! breakage of the protected/long mode entry the Linux boot protocol requires.
//!
//! WHERE IT DOES NOT RUN, IT SKIPS AND SAYS SO. No CI runner here has
//! `/dev/kvm`, and a test that quietly passes where it cannot execute a guest
//! is worse than no test -- so this one refuses to claim anything unless it
//! actually booted something. It needs two things and names whichever is
//! missing:
//!
//!   * a host where `HypervisorPlatform::detect()` reports KVM (that is
//!     `/dev/kvm` opened read+write, not merely present), and
//!   * `HV2_TEST_KERNEL` pointing at a bzImage built with
//!     `CONFIG_SERIAL_8250_CONSOLE=y`.
//!
//! On a Windows host with WSL2, this runs it:
//!
//! ```sh
//! wsl -d Debian -u root -e bash -lc 'cd /mnt/c/…/HyperMachine && \
//!   HV2_TEST_KERNEL="/mnt/c/Program Files/WSL/tools/kernel" \
//!   cargo test --locked -p hv2-core --test linux_kernel_boot -- --nocapture'
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_core::devices::keyboard::KeyboardDevice;
use hv2_core::devices::rtc::RtcDevice;
use hv2_core::{BootSource, HypervisorPlatform, SerialDevice, VMConfig, VM};

/// COM1, where `console=ttyS0` sends everything.
const COM1: u64 = 0x3F8;

/// Guest RAM, in MiB. A kernel decompresses itself well above where it loads,
/// so this is not a knob to turn down; it is also the number the e820
/// assertion below is derived from, so the two cannot drift apart.
const MEMORY_MIB: u64 = 1024;

/// No `quiet`, and `earlyprintk` on purpose: everything that goes wrong in the
/// boot protocol goes wrong before the kernel has a console driver, and
/// earlyprintk is the only thing that reports it. `panic=1` keeps a failed
/// root mount from sitting there.
const CMDLINE: &str = "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1";

/// How long the guest gets to reach userspace handoff before the test calls it
/// a failure.
///
/// Observed on this developer host: ~2.6 s of guest time, ~860k vCPU exits,
/// nearly all of them the one-byte-at-a-time serial writes that make up the
/// boot log. That cost is dominated by the host, not the guest: a debug-profile
/// build with an async device dispatch behind every port write, on a machine
/// that may be running a parallel `cargo` at the same time. 120 s is roughly
/// 45x the observed figure -- deliberately far too much, because the failure
/// this test is for is "the kernel said the wrong thing", and a flake caused by
/// a busy machine would teach people to ignore it. It is bounded rather than
/// unbounded because a guest that faults early stops exiting entirely and would
/// otherwise hang the suite forever. Polling means a healthy boot still
/// finishes in seconds.
const BOOT_DEADLINE: Duration = Duration::from_secs(120);

/// How often the console is checked for the end-of-boot marker.
const POLL: Duration = Duration::from_millis(200);

/// The reason this host cannot run the test, or `None` if it can.
fn boot_unavailable() -> Option<String> {
    let platform = HypervisorPlatform::detect();
    if platform != HypervisorPlatform::Kvm {
        return Some(format!(
            "this host has no usable /dev/kvm (HypervisorPlatform::detect() reported {platform:?})"
        ));
    }
    match std::env::var("HV2_TEST_KERNEL") {
        Err(_) => Some(
            "HV2_TEST_KERNEL is not set, so there is no kernel image to boot (set it to a \
             bzImage built with CONFIG_SERIAL_8250_CONSOLE=y)"
                .to_string(),
        ),
        Ok(path) if !std::path::Path::new(&path).is_file() => Some(format!(
            "HV2_TEST_KERNEL points at {path}, which is not a file"
        )),
        Ok(_) => None,
    }
}

/// The last thing a kernel prints before it hands control to userspace, in
/// either of the two ways this boot can end.
///
/// With no initrd -- which is how this test runs, so that the endpoint is the
/// same on every machine -- the kernel reaches `prepare_namespace`, finds no
/// root filesystem, and panics. That panic is a *complete* boot: it happens in
/// PID 1, after every device probe, after initmem is freed. A kernel carrying
/// a built-in initramfs instead executes its `/init`. Either sentence proves
/// the same thing, which is that the guest got all the way to userspace
/// handoff, so both are accepted.
const REACHED_USERSPACE_HANDOFF: [&str; 2] = [
    "Kernel panic - not syncing: VFS: Unable to mount root fs",
    "Run /init as init process",
];

fn reached_userspace_handoff(console: &str) -> bool {
    REACHED_USERSPACE_HANDOFF
        .iter()
        .any(|marker| console.contains(marker))
}

#[tokio::test]
async fn a_real_linux_kernel_boots_on_kvm_and_reports_the_machine_it_was_given() {
    if let Some(why) = boot_unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");

    let config = VMConfig {
        name: "linux-kernel-boot-test".to_string(),
        vcpu_count: 1,
        memory_size: MEMORY_MIB * 1024 * 1024,
        boot: Some(BootSource::linux(&kernel).with_cmdline(CMDLINE.to_string())),
        ..Default::default()
    };
    let vm = Arc::new(VM::new(config).expect("a VM with a Linux boot source"));

    let serial = Arc::new(tokio::sync::RwLock::new(SerialDevice::new(
        "COM1".to_string(),
        COM1,
    )));
    vm.devices()
        .register_device("COM1", serial)
        .await
        .expect("a serial device");
    vm.devices()
        .register_io_port_range("COM1".to_string(), COM1 as u16, COM1 as u16 + 7)
        .await
        .expect("COM1 mapped");

    // A kernel asks the CMOS for the time and waits for the update-in-progress
    // bit to clear; with nothing at 0x70 an unhandled read returns 0xff, that
    // bit reads as permanently set, and the guest spins there forever having
    // printed most of a line. Same story for the i8042 status port at 0x64.
    // Both devices are here because their absence is a boot hang, which makes
    // them part of what this test covers.
    let rtc = Arc::new(tokio::sync::RwLock::new(RtcDevice::new()));
    vm.devices()
        .register_device("RTC", rtc)
        .await
        .expect("an RTC");
    vm.devices()
        .register_io_port_range("RTC".to_string(), 0x70, 0x71)
        .await
        .expect("CMOS mapped");

    let keyboard = Arc::new(tokio::sync::RwLock::new(KeyboardDevice::new()));
    vm.devices()
        .register_device("i8042", keyboard)
        .await
        .expect("a keyboard controller");
    vm.devices()
        .register_io_port_range("i8042".to_string(), 0x60, 0x64)
        .await
        .expect("i8042 mapped");

    vm.provision().await.expect("the VM provisions");

    // Spawned, not awaited. A guest that reaches userspace and idles sits
    // inside KVM_RUN waiting for an interrupt, which blocks its thread in a
    // syscall -- a timeout wrapped around the run loop could never fire, so the
    // test would hang exactly when the boot had gone furthest.
    vm.launch().await.expect("the vCPU loop starts");

    let started = Instant::now();
    let mut console = String::new();
    while started.elapsed() < BOOT_DEADLINE {
        console = vm.console_output().await;
        if reached_userspace_handoff(&console) {
            break;
        }
        tokio::time::sleep(POLL).await;
    }
    let elapsed = started.elapsed();
    let _ = vm.stop().await;

    assert!(
        !console.is_empty(),
        "the kernel wrote nothing to COM1 in {elapsed:?}: it either never reached its entry \
         point or rejected boot_params before earlyprintk came up"
    );

    // The memory map, which is the assertion this test is really for. These are
    // the two ranges the hypervisor builds into the e820 table in boot_params,
    // read back in the kernel's own words: conventional low memory below the
    // BIOS data area, then everything from 1 MiB to the top of guest RAM. The
    // upper bound is computed from MEMORY_MIB, so a table that describes memory
    // the guest was not given -- the failure mode that a synthetic bzImage test
    // can never see, because nothing in it reads the table -- fails here.
    let top_of_ram = MEMORY_MIB * 1024 * 1024 - 1;
    let low = "BIOS-e820: [mem 0x0000000000000000-0x000000000009fbff] usable";
    let high = format!("BIOS-e820: [mem 0x0000000000100000-{top_of_ram:#018x}] usable");
    assert!(
        console.contains(low),
        "the kernel did not see the low conventional-memory e820 range.\nconsole:\n{console}"
    );
    assert!(
        console.contains(&high),
        "the kernel did not see {MEMORY_MIB} MiB of RAM as one e820 range ending at \
         {top_of_ram:#x}.\nconsole:\n{console}"
    );

    // The command line, byte for byte. `cmdline_ptr` is a guest-physical
    // address written into boot_params; point it a page too high and the kernel
    // boots anyway, with an empty command line and none of the settings asked
    // for. Only the kernel echoing the string back proves the pointer landed.
    assert!(
        console.contains(&format!("Kernel command line: {CMDLINE}")),
        "the kernel did not read back the command line it was given.\nconsole:\n{console}"
    );

    // Past earlyprintk and onto the real driver. earlyprintk pokes the UART's
    // transmit register directly and ignores its status bits, so a 16550 that
    // never reports THR-empty produces a perfectly good early log and then
    // hangs the moment 8250_core takes over. This line is the handover.
    assert!(
        console.contains("printk: legacy console [ttyS0] enabled"),
        "the 8250 driver never took over from earlyprintk, so the emulated UART's status bits \
         are wrong.\nconsole:\n{console}"
    );

    // And the whole way to PID 1. Everything between the memory map and here --
    // the trap table, the timer, the CMOS and i8042 probes, freeing initmem --
    // had to work for this sentence to be printed.
    assert!(
        reached_userspace_handoff(&console),
        "the guest never reached userspace handoff within {BOOT_DEADLINE:?} (it stopped after \
         {elapsed:?} of console output).\nconsole:\n{console}"
    );
}
