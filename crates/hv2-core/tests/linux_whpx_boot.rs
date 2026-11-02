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
//! `HV2_TEST_DATA` names a blank, writable disk image: the guest writes a
//! line to the last `/dev/vd?`, syncs, and the test reads it back from the
//! file once the VM is stopped.

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
const DEFAULT_CMDLINE: &str =
    "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1 nolapic noapic";

/// The kernel command line: `HV2_TEST_CMDLINE`, or [`DEFAULT_CMDLINE`].
fn cmdline_base() -> String {
    std::env::var("HV2_TEST_CMDLINE").unwrap_or_else(|_| DEFAULT_CMDLINE.to_string())
}

/// Virtual processors: `HV2_TEST_VCPUS`, or one.
fn vcpus() -> u32 {
    std::env::var("HV2_TEST_VCPUS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

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
    let mut source = BootSource::linux(&kernel).with_cmdline(cmdline_base());
    if let Ok(initrd) = std::env::var("HV2_TEST_INITRD") {
        source = source.with_initrd(initrd);
    }
    let config = VMConfig {
        name: "linux-whpx-boot-test".to_string(),
        vcpu_count: vcpus(),
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
    let data = std::env::var("HV2_TEST_DATA").ok();
    if let Some(data) = &data {
        vm.attach_data_pci(std::path::Path::new(data), "data0")
            .await
            .expect("the data disk attaches");
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
    // A writable disk: the guest puts a computed line on the last disk and
    // syncs; the host reads it from the image after the VM is stopped.
    if let (true, Some(_)) = (sleeper && marker.is_some(), &data) {
        vm.send_serial(
            b"d=$(ls /dev/vd? | tail -1); printf 'hv2-data-%s\\n' $((6*7)) > $d; sync; echo hv2-synced-$((6*7))
",
        )
        .await
        .expect("the serial input is accepted");
        while started.elapsed() < BOOT_DEADLINE && !console.contains("hv2-synced-42") {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
    }
    let elapsed = started.elapsed();
    let _ = vm.stop().await;
    println!("linux whpx boot: userspace handoff after {elapsed:?}");
    if let Some(data) = &data {
        if sleeper && marker.is_some() {
            let image = std::fs::read(data).expect("the data image is readable");
            assert!(
                image.starts_with(b"hv2-data-42\n"),
                "the guest's write never reached the data image (first bytes {:?}).\nconsole:\n{console}",
                &image[..image.len().min(16)]
            );
        }
    }

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
        format!("Kernel command line: {}", cmdline_base()),
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
    let cmdline = format!("{} root=/dev/vda1 ro", cmdline_base());
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
    // The guest's own account of its clocks, and of when it mounted: the
    // stamp on that line against `elapsed` is the guest's clock against the
    // host's.
    for line in console.lines().filter(|l| {
        l.contains("tsc") || l.contains("clocksource") || l.contains("EXT4-fs (vda1): mounted")
    }) {
        println!("linux whpx root: guest: {line}");
    }

    assert!(
        console.contains("virtio_blk virtio0: [vda]"),
        "the guest never found the disk.\nconsole:\n{console}"
    );
    assert!(
        console.contains("EXT4-fs (vda1): mounted filesystem"),
        "the root filesystem never mounted within {BOOT_DEADLINE:?}.\nconsole:\n{console}"
    );
}

/// A paused WHPX guest's vCPU state read, written back unchanged and read
/// again, then resumed: the registers must match, and the guest must still run.
///
/// Needs the same `HV2_TEST_KERNEL`, `HV2_TEST_INITRD`, `HV2_TEST_INITRD_MARKER`
/// and `HV2_TEST_INITRD_SHELL` as the first test (a shell on the console).
///
/// The TSC is left out of the comparison: it counts on between the write and
/// the next read. So are `RIP`-independent things that cannot differ.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_vcpus_state_survives_a_write_back() {
    if let Some(why) = unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let (Ok(initrd), Ok(marker), Ok(_)) = (
        std::env::var("HV2_TEST_INITRD"),
        std::env::var("HV2_TEST_INITRD_MARKER"),
        std::env::var("HV2_TEST_INITRD_SHELL"),
    ) else {
        eprintln!("skipping: needs HV2_TEST_INITRD, _MARKER and _SHELL");
        return;
    };
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");
    let config = VMConfig {
        name: "linux-whpx-state-test".to_string(),
        vcpu_count: 1,
        memory_size: memory_mib() * 1024 * 1024,
        boot: Some(
            BootSource::linux(&kernel)
                .with_cmdline(cmdline_base())
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
    if let Err(e) = vm.provision().await {
        eprintln!("skipping: provision failed ({e})");
        return;
    }
    vm.launch().await.expect("the vCPU loop starts");
    let started = Instant::now();
    let mut console = String::new();
    while started.elapsed() < BOOT_DEADLINE && !console.contains(marker.as_str()) {
        tokio::time::sleep(POLL).await;
        console = vm.console_output().await;
    }
    assert!(
        console.contains(marker.as_str()),
        "userspace never came up:\n{console}"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;

    vm.pause().await.expect("the VM pauses");
    let before = vm.save_vcpu_states().await.expect("the vCPU state is read");
    vm.restore_vcpu_states(&before)
        .await
        .expect("the same state is written back");
    let after = vm.save_vcpu_states().await.expect("it is read again");

    let strip = |mut s: hv2_core::snapshot::vcpu::VCpuSnapshot| {
        s.msrs.retain(|m| m.index != 0x10);
        s
    };
    for (b, a) in before.iter().cloned().zip(after.iter().cloned()) {
        assert_eq!(strip(b), strip(a), "a write-back changed the vCPU's state");
    }
    println!(
        "whpx vcpu state: {} vCPU(s) round-tripped; rip {:#x}, cr3 {:#x}, {} MSRs",
        before.len(),
        before[0].general.rip,
        before[0].system.cr3,
        before[0].msrs.len()
    );

    vm.resume().await.expect("the VM resumes");
    vm.send_serial(b"echo hv2-after-restore-$((6*7))\n")
        .await
        .expect("the serial input is accepted");
    while started.elapsed() < BOOT_DEADLINE && !console.contains("hv2-after-restore-42") {
        tokio::time::sleep(POLL).await;
        console = vm.console_output().await;
    }
    let _ = vm.stop().await;
    assert!(
        console.contains("hv2-after-restore-42"),
        "the guest did not run after the write-back.\nconsole:\n{console}"
    );
}

/// A whole-VM snapshot of a running WHPX guest, taken, diverged from and
/// restored into the same VM: the guest must be back where the snapshot was
/// (a shell variable it set, in its memory) and its timer must still tick
/// (`sleep 1` returns), which needs the PIC and PIT put back too.
///
/// Same environment as the write-back test.
#[tokio::test(flavor = "multi_thread")]
async fn a_whpx_guest_returns_to_its_snapshot() {
    if let Some(why) = unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let (Ok(initrd), Ok(marker), Ok(_)) = (
        std::env::var("HV2_TEST_INITRD"),
        std::env::var("HV2_TEST_INITRD_MARKER"),
        std::env::var("HV2_TEST_INITRD_SHELL"),
    ) else {
        eprintln!("skipping: needs HV2_TEST_INITRD, _MARKER and _SHELL");
        return;
    };
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");
    let config = VMConfig {
        name: "linux-whpx-snapshot-test".to_string(),
        vcpu_count: 1,
        memory_size: memory_mib() * 1024 * 1024,
        boot: Some(
            BootSource::linux(&kernel)
                .with_cmdline(cmdline_base())
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
    if let Err(e) = vm.provision().await {
        eprintln!("skipping: provision failed ({e})");
        return;
    }
    vm.launch().await.expect("the vCPU loop starts");
    let started = Instant::now();

    async fn wait_for(vm: &VM, started: Instant, what: &str) -> String {
        let mut console = vm.console_output().await;
        while started.elapsed() < BOOT_DEADLINE && !console.contains(what) {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
        console
    }
    let console = wait_for(&vm, started, marker.as_str()).await;
    assert!(
        console.contains(marker.as_str()),
        "userspace never came up:\n{console}"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;

    vm.send_serial(b"X=hv2-state-$((6*7)); echo set-$X\n")
        .await
        .unwrap();
    let console = wait_for(&vm, started, "set-hv2-state-42").await;
    assert!(
        console.contains("set-hv2-state-42"),
        "the variable never got set:\n{console}"
    );

    let path = std::env::temp_dir().join("hv2-whpx-snapshot-test.snap");
    vm.pause().await.expect("the VM pauses");
    vm.snapshot(&path).await.expect("the VM snapshots");
    vm.resume().await.expect("the VM resumes");

    vm.send_serial(b"X=changed; echo now-$X\n").await.unwrap();
    let console = wait_for(&vm, started, "now-changed").await;
    assert!(
        console.contains("now-changed"),
        "the guest did not run on:\n{console}"
    );

    vm.pause().await.expect("the VM pauses again");
    vm.restore(&path).await.expect("the VM restores");
    vm.resume().await.expect("the VM resumes after the restore");

    let sent = Instant::now();
    vm.send_serial(b"sleep 1; echo restored-$X-$((6*7))\n")
        .await
        .unwrap();
    // The typed line echoes "restored-$X-$((6*7))" literally, so wait for the
    // answer: whatever X was, the arithmetic ends it in "-42".
    let mut console = vm.console_output().await;
    while started.elapsed() < BOOT_DEADLINE
        && !console
            .lines()
            .any(|l| l.starts_with("restored-") && l.trim_end().ends_with("-42"))
    {
        tokio::time::sleep(POLL).await;
        console = vm.console_output().await;
    }
    let after = console
        .lines()
        .find(|l| l.starts_with("restored-") && l.trim_end().ends_with("-42"))
        .unwrap_or("(no answer)")
        .trim_end()
        .to_string();
    let console_full = console.clone();
    let _ = vm.stop().await;
    let _ = std::fs::remove_file(&path);
    println!(
        "whpx snapshot: restored guest answered after {:?}",
        sent.elapsed()
    );
    assert!(
        after == "restored-hv2-state-42-42",
        "after the restore the variable was not back (got {after:?}).\nconsole:\n{console_full}"
    );
}

/// A snapshot taken from one WHPX VM, restored into a second, fresh one with
/// `launch_from_snapshot`: the fork path. The first VM is stopped and dropped
/// before the second is created, because WHP here cannot map two guests'
/// memory at once (`0xC0370008`).
///
/// Same environment as the write-back test.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_whpx_vm_continues_from_a_snapshot() {
    if let Some(why) = unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let (Ok(initrd), Ok(marker), Ok(_)) = (
        std::env::var("HV2_TEST_INITRD"),
        std::env::var("HV2_TEST_INITRD_MARKER"),
        std::env::var("HV2_TEST_INITRD_SHELL"),
    ) else {
        eprintln!("skipping: needs HV2_TEST_INITRD, _MARKER and _SHELL");
        return;
    };
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");
    let make = |name: &str| VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: memory_mib() * 1024 * 1024,
        boot: Some(
            BootSource::linux(&kernel)
                .with_cmdline(cmdline_base())
                .with_initrd(initrd.clone()),
        ),
        ..Default::default()
    };
    let path = std::env::temp_dir().join("hv2-whpx-fork-test.snap");
    let started = Instant::now();

    {
        let Ok(vm) = VM::new(make("linux-whpx-fork-source")) else {
            eprintln!("skipping: a VM could not be created here");
            return;
        };
        if !vm.executes_guest_code() {
            eprintln!("skipping: no backend on this host executes guest code");
            return;
        }
        let vm = Arc::new(vm);
        if let Err(e) = vm.provision().await {
            eprintln!("skipping: provision failed ({e})");
            return;
        }
        vm.launch().await.expect("the vCPU loop starts");
        let mut console = String::new();
        while started.elapsed() < BOOT_DEADLINE && !console.contains(marker.as_str()) {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
        assert!(
            console.contains(marker.as_str()),
            "userspace never came up:\n{console}"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        vm.send_serial(b"X=hv2-fork-$((6*7)); echo set-$X\n")
            .await
            .unwrap();
        while started.elapsed() < BOOT_DEADLINE && !console.contains("set-hv2-fork-42") {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
        assert!(
            console.contains("set-hv2-fork-42"),
            "the variable never got set:\n{console}"
        );
        vm.pause().await.expect("the source pauses");
        vm.snapshot(&path).await.expect("the source snapshots");
        let _ = vm.stop().await;
    }
    // the source is out of scope: its partition is released

    let vm = Arc::new(VM::new(make("linux-whpx-fork-copy")).expect("the copy is created"));
    // HV2_TEST_CLOCK=continue restores the TSC with the rest, as a sandbox does.
    let clock = if std::env::var("HV2_TEST_CLOCK").as_deref() == Ok("continue") {
        hv2_core::snapshot::machine::ClockOnRestore::Continue
    } else {
        hv2_core::snapshot::machine::ClockOnRestore::Fresh
    };
    vm.launch_from_snapshot(&path, clock)
        .await
        .expect("the copy launches from the snapshot");
    let sent = Instant::now();
    vm.send_serial(b"sleep 1; echo forked-$X-$((6*7))\n")
        .await
        .unwrap();
    let mut console = vm.console_output().await;
    let answer = |c: &str| {
        c.lines()
            .find(|l| l.starts_with("forked-") && l.trim_end().ends_with("-42"))
            .map(|l| l.trim_end().to_string())
    };
    while started.elapsed() < BOOT_DEADLINE && answer(&console).is_none() {
        tokio::time::sleep(POLL).await;
        console = vm.console_output().await;
    }
    let got = answer(&console);
    let _ = vm.stop().await;
    let _ = std::fs::remove_file(&path);
    println!("whpx fork: the copy answered after {:?}", sent.elapsed());
    assert_eq!(
        got.as_deref(),
        Some("forked-hv2-fork-42-42"),
        "the copy did not continue from the snapshot.\nconsole:\n{console}"
    );
}

/// A guest with a virtio-blk disk on PCI is snapshotted and restored into a
/// second VM: the restored device must serve the disk the guest wrote.
#[tokio::test(flavor = "multi_thread")]
async fn a_restored_whpx_guest_reads_its_pci_data_disk() {
    if let Some(why) = unavailable() {
        eprintln!("skipping: {why}");
        return;
    }
    let (Ok(initrd), Ok(marker), Ok(_)) = (
        std::env::var("HV2_TEST_INITRD"),
        std::env::var("HV2_TEST_INITRD_MARKER"),
        std::env::var("HV2_TEST_INITRD_SHELL"),
    ) else {
        eprintln!("skipping: needs HV2_TEST_INITRD, _MARKER and _SHELL");
        return;
    };
    let kernel = std::env::var("HV2_TEST_KERNEL").expect("checked just above");
    let make = |name: &str| VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: memory_mib() * 1024 * 1024,
        boot: Some(
            BootSource::linux(&kernel)
                .with_cmdline(cmdline_base())
                .with_initrd(initrd.clone()),
        ),
        ..Default::default()
    };
    let dir = std::env::temp_dir();
    let image = dir.join("hv2-whpx-data-disk.img");
    let path = dir.join("hv2-whpx-data-disk.snap");
    std::fs::write(&image, vec![0u8; 1024 * 1024]).expect("the image is written");
    let started = Instant::now();
    let wait_for = |needle: &'static str| needle;

    {
        let Ok(vm) = VM::new(make("linux-whpx-disk-source")) else {
            eprintln!("skipping: a VM could not be created here");
            return;
        };
        if !vm.executes_guest_code() {
            eprintln!("skipping: no backend on this host executes guest code");
            return;
        }
        let vm = Arc::new(vm);
        vm.attach_data_pci(&image, "data0")
            .await
            .expect("the data disk attaches");
        if let Err(e) = vm.provision().await {
            eprintln!("skipping: provision failed ({e})");
            return;
        }
        vm.launch().await.expect("the vCPU loop starts");
        let mut console = String::new();
        while started.elapsed() < BOOT_DEADLINE && !console.contains(marker.as_str()) {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
        assert!(
            console.contains(marker.as_str()),
            "no userspace:\n{console}"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        vm.send_serial(
            b"echo -n DISKMARK-$((6*7)) | dd of=/dev/vda bs=512 count=1 conv=sync 2>/dev/null; sync; echo wrote-$?\n",
        )
        .await
        .unwrap();
        let done = wait_for("wrote-0");
        while started.elapsed() < BOOT_DEADLINE && !console.contains(done) {
            tokio::time::sleep(POLL).await;
            console = vm.console_output().await;
        }
        assert!(
            console.lines().any(|l| l.trim_end() == "wrote-0"),
            "the guest could not write /dev/vda:\n{console}"
        );
        vm.pause().await.expect("the source pauses");
        vm.snapshot(&path).await.expect("the source snapshots");
        let _ = vm.stop().await;
    }

    let vm = Arc::new(VM::new(make("linux-whpx-disk-copy")).expect("the copy is created"));
    vm.attach_data_pci(&image, "data0")
        .await
        .expect("the copy attaches the same disk");
    vm.launch_from_snapshot(&path, hv2_core::snapshot::machine::ClockOnRestore::Continue)
        .await
        .expect("the copy launches from the snapshot");
    // Drop the page cache so the read goes through the restored device.
    vm.send_serial(
        b"echo 3 > /proc/sys/vm/drop_caches; echo -n NEW-$((6*7)) | dd of=/dev/vda bs=512 seek=1 count=1 conv=sync 2>/dev/null; sync; echo read-$(dd if=/dev/vda bs=512 count=1 2>/dev/null | head -c 11)-$(dd if=/dev/vda bs=512 skip=1 count=1 2>/dev/null | head -c 6)-end\n",
    )
    .await
    .unwrap();
    let mut console = vm.console_output().await;
    let answer = |c: &str| {
        c.lines()
            .find(|l| l.starts_with("read-") && l.trim_end().ends_with("-end"))
            .map(|l| l.trim_end().to_string())
    };
    while started.elapsed() < BOOT_DEADLINE && answer(&console).is_none() {
        tokio::time::sleep(POLL).await;
        console = vm.console_output().await;
    }
    let got = answer(&console);
    let _ = vm.stop().await;
    let on_host = std::fs::read(&image).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&image);
    assert_eq!(
        got.as_deref(),
        Some("read-DISKMARK-42-NEW-42-end"),
        "the restored disk did not serve what the guest wrote.\nconsole:\n{console}"
    );
    assert!(
        on_host.get(512..518) == Some(b"NEW-42".as_slice()),
        "the write after restore never reached the image file"
    );
}
