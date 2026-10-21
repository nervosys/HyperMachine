//! A real Linux kernel, booted on WHPX, judged by what it says about itself.
//!
//! `linux_kernel_boot.rs` does this on KVM, where the hypervisor describes the
//! interrupt controller to the guest (an MP table, an in-kernel I/O APIC and
//! PIT). WHPX here has none of that: the guest has a 8259 PIC and an 8254 PIT
//! that this crate emulates, no local APIC, and no firmware tables. So this
//! test boots with `nolapic noapic` and asserts what holds there instead.
//!
//! What it catches, by the line of the boot log that goes missing:
//!
//!   * `Kernel command line:` / the e820 ranges -- `boot_params` filled in
//!     wrongly, as in the KVM test.
//!   * `Calibrating delay loop` -- the kernel spins waiting for timer ticks,
//!     never executing an I/O instruction while it does. An interrupt that is
//!     only injected at the next VM exit never arrives, and the boot stops
//!     here forever.
//!   * the handoff to userspace.
//!
//! It skips, and says so, unless the host runs guests through a non-KVM
//! backend and `HV2_TEST_KERNEL` names a bzImage with
//! `CONFIG_SERIAL_8250_CONSOLE=y`. `HV2_TEST_INITRD` adds an initramfs, in
//! which case `/init` running is the end of the boot instead of the panic for
//! want of a root filesystem. `HV2_TEST_INITRD_MARKER` names a string that init
//! prints, which proves output from userspace (not just the kernel) arrives;
//! `HV2_TEST_INITRD_SHELL` says init leaves a shell on the console, and the test
//! then has it `sleep 1` to prove a halted guest is woken by the timer.
//! `HV2_TEST_SEED` names a second, read-only disk (a tar stream) and
//! `HV2_TEST_SEED_MARKER` a string in it that reading the disk as a tar stream must print (it is `/dev/vda` here, with no root disk).

use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_core::{BootSource, HypervisorPlatform, VMConfig, VM};

const MEMORY_MIB: u64 = 1024;

/// Guest RAM for the first test: `HV2_TEST_MEMORY_MIB`, or [`MEMORY_MIB`].
/// Above 3 GiB the RAM is in two ranges either side of the device hole.
fn memory_mib() -> u64 {
    std::env::var("HV2_TEST_MEMORY_MIB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(MEMORY_MIB)
}

/// No local APIC or I/O APIC exists on this backend; saying so is what stops
/// the kernel probing 0xFEE00000 for ever.
const CMDLINE: &str = "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1 nolapic noapic";

const BOOT_DEADLINE: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(200);

const REACHED_USERSPACE_HANDOFF: [&str; 2] = [
    "Kernel panic - not syncing: VFS: Unable to mount root fs",
    "Run /init as init process",
];

fn reached_userspace_handoff(console: &str) -> bool {
    REACHED_USERSPACE_HANDOFF
        .iter()
        .any(|marker| console.contains(marker))
}

fn unavailable() -> Option<String> {
    let platform = HypervisorPlatform::detect();
    if platform == HypervisorPlatform::Kvm {
        return Some("this host uses KVM; linux_kernel_boot.rs covers it".to_string());
    }
    match std::env::var("HV2_TEST_KERNEL") {
        Err(_) => Some("HV2_TEST_KERNEL is not set".to_string()),
        Ok(path) if !std::path::Path::new(&path).is_file() => {
            Some(format!("HV2_TEST_KERNEL points at {path}, not a file"))
        }
        Ok(_) => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_linux_kernel_boots_on_a_userspace_pic_backend() {
    if let Some(why) = unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");
    let mut source = BootSource::linux(&kernel).with_cmdline(CMDLINE.to_string());
    if let Ok(initrd) = std::env::var("HV2_TEST_INITRD") {
        source = source.with_initrd(initrd);
    }
    let config = VMConfig {
        name: "linux-whpx-boot-test".to_string(),
        vcpu_count: 1,
        memory_size: memory_mib() * 1024 * 1024,
        boot: Some(source),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("skipping: a VM could not be created here");
        return;
    };
    if !vm.executes_guest_code() {
        eprintln!("skipping: no backend on this host executes guest code");
        return;
    }
    let vm = Arc::new(vm);
    let seed = std::env::var("HV2_TEST_SEED").ok();
    if let Some(seed) = &seed {
        vm.attach_seed_pci(std::path::Path::new(seed), "seed0")
            .await
            .expect("the seed disk attaches");
    }
    if let Err(e) = vm.provision().await {
        eprintln!("skipping: provision failed ({e})");
        return;
    }
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
    // Whatever the initramfs prints once it is running reaches the console
    // through the tty layer, which waits on the UART's transmit interrupt
    // (IRQ 4) after every byte. Kernel messages do not prove that works --
    // printk polls the line-status register -- so the test asks for a marker
    // only userspace can have written.
    let marker = std::env::var("HV2_TEST_INITRD_MARKER").ok();
    if let Some(marker) = &marker {
        while started.elapsed() < BOOT_DEADLINE && !console.contains(marker.as_str()) {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
    }
    // A shell that is asked to sleep puts the guest into HLT with nothing
    // runnable, and only the PIT's tick can wake it. The reply is computed
    // (`$((6*7))`) so the echo of the typed line cannot satisfy the check.
    // Needs a shell on the console, which `HV2_TEST_INITRD_SHELL` promises.
    let sleeper = std::env::var("HV2_TEST_INITRD_SHELL").is_ok();
    if sleeper && marker.is_some() {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let sent = Instant::now();
        vm.send_serial(
            b"sleep 1; echo hv2-woke-$((6*7))
",
        )
        .await
        .expect("the serial input is accepted");
        while started.elapsed() < BOOT_DEADLINE && !console.contains("hv2-woke-42") {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
        let slept = sent.elapsed();
        println!("linux whpx boot: `sleep 1` took {slept:?} of host time");
        // The guest's clock follows the host's: it was 14.5 s while the tick
        // was its only clock. The bound is loose for a loaded host.
        assert!(
            slept < Duration::from_secs(6),
            "`sleep 1` took {slept:?} of host time: the guest's clock runs slow"
        );
    }
    // A second disk, read by the guest: a tar stream with no filesystem on it,
    // as the host would write one. `HV2_TEST_SEED_MARKER` is a string in it.
    let seed_marker = std::env::var("HV2_TEST_SEED_MARKER").ok();
    if let (true, Some(marker), true) = (sleeper, &seed_marker, seed.is_some()) {
        vm.send_serial(
            b"for d in /dev/vd?; do tar -xOf $d; done
",
        )
        .await
        .expect("the serial input is accepted");
        while started.elapsed() < BOOT_DEADLINE && !console.contains(marker.as_str()) {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
    }
    let elapsed = started.elapsed();
    let _ = vm.stop().await;
    println!("linux whpx boot: userspace handoff after {elapsed:?}");

    assert!(
        !console.is_empty(),
        "the kernel wrote nothing to COM1 in {elapsed:?}: it never ran its first instruction"
    );
    let ram = memory_mib() * 1024 * 1024;
    const HOLE: u64 = 0xC000_0000;
    const ABOVE_4G: u64 = 0x1_0000_0000;
    // One range up to the top of RAM, or two either side of the device hole.
    let ranges: Vec<String> = if ram <= HOLE {
        vec![format!(
            "BIOS-e820: [mem 0x0000000000100000-{:#018x}] usable",
            ram - 1
        )]
    } else {
        vec![
            format!(
                "BIOS-e820: [mem 0x0000000000100000-{:#018x}] usable",
                HOLE - 1
            ),
            format!(
                "BIOS-e820: [mem {ABOVE_4G:#018x}-{:#018x}] usable",
                ABOVE_4G + (ram - HOLE) - 1
            ),
        ]
    };
    for expected in ranges.into_iter().chain([
        "BIOS-e820: [mem 0x0000000000000000-0x000000000009fbff] usable".to_string(),
        format!("Kernel command line: {CMDLINE}"),
    ]) {
        assert!(
            console.contains(&expected),
            "the console lacks {expected:?}.\nconsole:\n{console}"
        );
    }
    assert!(
        console.contains("printk: console [ttyS0] enabled")
            || console.contains("printk: legacy console [ttyS0] enabled"),
        "the 8250 driver never took over from earlyprintk.\nconsole:\n{console}"
    );
    assert!(
        console.contains("Calibrating delay loop"),
        "the kernel never finished calibrating its delay loop: timer interrupts do not reach a \
         guest that is spinning.\nconsole:\n{console}"
    );
    if let Some(marker) = &marker {
        assert!(
            console.contains(marker.as_str()),
            "userspace printed nothing ({marker:?} never appeared): the UART's transmit              interrupt does not reach the guest.
console:
{console}"
        );
    }
    if let (true, Some(marker), true) = (sleeper, &seed_marker, seed.is_some()) {
        assert!(
            console.contains(marker.as_str()),
            "the guest could not read the seed disk.
console:
{console}"
        );
    }
    if sleeper && marker.is_some() {
        assert!(
            console.contains("hv2-woke-42"),
            "a guest that slept never woke: the timer tick never reached it: an interruption injected over a still-queued one is lost, and its in-service bit then masks the line for good.
console:
{console}"
        );
    }
    assert!(
        reached_userspace_handoff(&console),
        "the guest never reached userspace handoff within {BOOT_DEADLINE:?}.\nconsole:\n{console}"
    );
}

/// A root filesystem on a virtio-blk disk behind PCI, mounted by the guest.
///
/// What it takes, each a separate way to see nothing: the guest enumerating a
/// bus at all (the VM's own PCI root, not another `Machine`'s), the device's
/// registers being readable and writable through the instruction behind each
/// MMIO exit, and the device seeing the rings the guest wrote (it reads the
/// pages the partition runs the guest in). Needs `HV2_TEST_DISK`, a raw image
/// whose first partition is the root, and an `HV2_TEST_INITRD` that loads
/// `virtio_blk` (a distribution initramfs does); the disk is attached
/// read-only.
#[tokio::test(flavor = "multi_thread")]
async fn a_root_filesystem_mounts_from_a_virtio_blk_disk_on_pci() {
    if let Some(why) = unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let (Ok(disk), Ok(initrd)) = (
        std::env::var("HV2_TEST_DISK"),
        std::env::var("HV2_TEST_INITRD"),
    ) else {
        eprintln!("skipping: HV2_TEST_DISK and HV2_TEST_INITRD are not both set");
        return;
    };
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");
    let cmdline = format!("{CMDLINE} root=/dev/vda1 ro");
    let config = VMConfig {
        name: "linux-whpx-root-test".to_string(),
        vcpu_count: 1,
        memory_size: MEMORY_MIB * 1024 * 1024,
        boot: Some(
            BootSource::linux(&kernel)
                .with_cmdline(cmdline)
                .with_initrd(initrd),
        ),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("skipping: a VM could not be created here");
        return;
    };
    if !vm.executes_guest_code() {
        eprintln!("skipping: no backend on this host executes guest code");
        return;
    }
    let vm = Arc::new(vm);
    vm.attach_block_pci(std::path::Path::new(&disk), true, "root0")
        .await
        .expect("the disk attaches");
    if let Err(e) = vm.provision().await {
        eprintln!("skipping: provision failed ({e})");
        return;
    }
    vm.launch().await.expect("the vCPU loop starts");

    let started = Instant::now();
    let mut console = String::new();
    while started.elapsed() < BOOT_DEADLINE {
        console = vm.console_output().await;
        if console.contains("EXT4-fs (vda1): mounted filesystem") {
            break;
        }
        tokio::time::sleep(POLL).await;
    }
    let elapsed = started.elapsed();
    let _ = vm.stop().await;
    println!("linux whpx root: mounted after {elapsed:?}");

    assert!(
        console.contains("virtio_blk virtio0: [vda]"),
        "the guest never found the disk.\nconsole:\n{console}"
    );
    assert!(
        console.contains("EXT4-fs (vda1): mounted filesystem"),
        "the root filesystem never mounted within {BOOT_DEADLINE:?}.\nconsole:\n{console}"
    );
}
