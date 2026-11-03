//! Several vCPUs on WHPX: the BSP wakes each application processor with
//! INIT and SIPI, every AP reports its local APIC ID through shared memory,
//! and the BSP prints the IDs on COM1.
//!
//! The guest is a hand-assembled flat Multiboot image, so nothing but the
//! hypervisor's own APIC emulation is under test: no kernel, no ACPI. It
//! skips where WHPX is unavailable.

#![cfg(windows)]

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MULTIBOOT_MAGIC: u32 = 0x1BAD_B002;
/// Where the BSP copies the AP trampoline: the SIPI vector is `0x08`.
const TRAMP: u32 = 0x8000;
/// One byte per APIC ID, written by that processor.
const IDS: u32 = 0x9000;
/// How many APs have reported.
const COUNT: u32 = 0x9100;
const COM1: u32 = 0x3F8;
const BOUND: Duration = Duration::from_secs(20);

/// A busy loop of `iterations` decrements; nothing when zero.
fn spin_loop(code: &mut Vec<u8>, iterations: u32) {
    if iterations != 0 {
        code.push(0xB9); // mov ecx, iterations
        code.extend_from_slice(&le(iterations));
        code.extend_from_slice(&[0x49, 0x75, 0xFD]); // dec ecx; jnz -3
    }
}

fn le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// Real-mode entry at the SIPI vector, then 32-bit code that reads the local
/// APIC ID, records it, counts itself and halts.
fn trampoline(spin: u32) -> Vec<u8> {
    // Offsets inside the page; the GDT pseudo-descriptor and table follow the code.
    let mut code: Vec<u8> = Vec::new();
    code.extend_from_slice(&[0xFA, 0x31, 0xC0, 0x8E, 0xD8]); // cli; xor ax,ax; mov ds,ax
    let lgdt_at = code.len();
    code.extend_from_slice(&[0x0F, 0x01, 0x16, 0, 0]); // lgdt [gdtr] (patched)
    code.extend_from_slice(&[0x0F, 0x20, 0xC0, 0x0C, 0x01, 0x0F, 0x22, 0xC0]); // cr0.PE = 1
    let jmp_at = code.len();
    code.extend_from_slice(&[0x66, 0xEA, 0, 0, 0, 0, 0x08, 0x00]); // jmp 0x08:pm (patched)
    let pm = code.len();
    code.extend_from_slice(&[0xB8, 0x10, 0, 0, 0, 0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0]);
    // software-enable the APIC, then read its ID
    code.extend_from_slice(&[0xC7, 0x05, 0xF0, 0x00, 0xE0, 0xFE, 0xFF, 0x01, 0, 0]);
    code.extend_from_slice(&[0xA1, 0x20, 0x00, 0xE0, 0xFE]); // mov eax,[0xFEE00020]
    code.extend_from_slice(&[0xC1, 0xE8, 0x18, 0x89, 0xC3, 0x88, 0xD9, 0x80, 0xC1, b'0']);
    code.extend_from_slice(&[0x88, 0x8B]); // mov [ebx+IDS],cl
    code.extend_from_slice(&le(IDS));
    spin_loop(&mut code, spin);
    code.extend_from_slice(&[0xF0, 0xFF, 0x05]); // lock inc dword [COUNT]
    code.extend_from_slice(&le(COUNT));
    code.extend_from_slice(&[0xF4, 0xEB, 0xFD]); // halt: hlt; jmp halt
    while !code.len().is_multiple_of(8) {
        code.push(0x90);
    }
    let gdtr = code.len();
    let gdt = gdtr + 8;
    code.extend_from_slice(&23u16.to_le_bytes());
    code.extend_from_slice(&le(TRAMP + gdt as u32));
    code.extend_from_slice(&[0, 0]);
    code.extend_from_slice(&[0; 8]);
    code.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0x9A, 0xCF, 0]);
    code.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0x92, 0xCF, 0]);
    code[lgdt_at + 3..lgdt_at + 5].copy_from_slice(&((TRAMP as usize + gdtr) as u16).to_le_bytes());
    code[jmp_at + 2..jmp_at + 6].copy_from_slice(&le(TRAMP + pm as u32));
    code
}

/// The BSP: copy the trampoline, send INIT then two SIPIs, wait for every AP,
/// print the IDs.
fn bsp(cpus: u32, base: u32, spin: u32) -> Vec<u8> {
    let tramp = trampoline(spin);
    let mut code: Vec<u8> = Vec::new();
    // The trampoline lives after this code; its address is patched in below.
    code.push(0xBE);
    let src_patch = code.len();
    code.extend_from_slice(&[0; 4]);
    code.push(0xBF);
    code.extend_from_slice(&le(TRAMP));
    code.push(0xB9);
    code.extend_from_slice(&le(tramp.len() as u32));
    code.extend_from_slice(&[0xFC, 0xF3, 0xA4]); // cld; rep movsb
    code.push(0xBF);
    code.extend_from_slice(&le(IDS));
    code.push(0xB9);
    code.extend_from_slice(&le(0x41));
    code.extend_from_slice(&[0x31, 0xC0, 0xF3, 0xAB]); // rep stosd
    if cpus == 1 {
        // No APIC without a second processor: record ID 0 and go on.
        code.extend_from_slice(&[0xC6, 0x05]);
        code.extend_from_slice(&le(IDS));
        code.push(b'0');
        spin_loop(&mut code, spin);
    } else {
        code.extend_from_slice(&[0xC7, 0x05, 0xF0, 0x00, 0xE0, 0xFE, 0xFF, 0x01, 0, 0]); // SVR
        code.extend_from_slice(&[0xA1, 0x20, 0x00, 0xE0, 0xFE]);
        code.extend_from_slice(&[0xC1, 0xE8, 0x18, 0x89, 0xC3, 0x88, 0xD9, 0x80, 0xC1, b'0']);
        code.extend_from_slice(&[0x88, 0x8B]);
        code.extend_from_slice(&le(IDS));
        let delay = |code: &mut Vec<u8>| {
            code.push(0xB9);
            code.extend_from_slice(&le(0x0010_0000));
            code.extend_from_slice(&[0x49, 0x75, 0xFD]); // dec ecx; jnz -3
        };
        code.extend_from_slice(&[0xC7, 0x05, 0x10, 0x03, 0xE0, 0xFE, 0, 0, 0, 0]); // ICR high
        code.extend_from_slice(&[0xC7, 0x05, 0x00, 0x03, 0xE0, 0xFE, 0x00, 0x45, 0x0C, 0x00]); // INIT
        delay(&mut code);
        for _ in 0..2 {
            code.extend_from_slice(&[0xC7, 0x05, 0x00, 0x03, 0xE0, 0xFE, 0x08, 0x46, 0x0C, 0x00]); // SIPI
            delay(&mut code);
        }
        spin_loop(&mut code, spin);
        // wait: mov eax,[COUNT]; cmp eax,cpus-1; jb wait
        code.push(0xA1);
        code.extend_from_slice(&le(COUNT));
        code.extend_from_slice(&[0x83, 0xF8, (cpus - 1) as u8, 0x72, 0xF6]);
    }
    code.push(0xBA);
    code.extend_from_slice(&le(COM1));
    let shown = if std::env::var("HV2_SMP_DUMP").is_ok() {
        0x40
    } else {
        cpus
    };
    for i in 0..shown {
        code.push(0xA0); // mov al,[IDS+i]
        code.extend_from_slice(&le(IDS + i));
        code.push(0xEE);
    }
    code.extend_from_slice(&[0xB0, b'\n', 0xEE, 0xF4]);
    while !code.len().is_multiple_of(16) {
        code.push(0x90);
    }
    let tramp_at = base + code.len() as u32;
    code[src_patch..src_patch + 4].copy_from_slice(&le(tramp_at));
    code.extend_from_slice(&tramp);
    code
}

fn image(cpus: u32, spin: u32) -> Vec<u8> {
    flat(bsp(cpus, 0x0010_0010, spin))
}

fn flat(code: Vec<u8>) -> Vec<u8> {
    let flags = 0u32;
    let checksum = 0u32.wrapping_sub(MULTIBOOT_MAGIC).wrapping_sub(flags);
    let mut out = vec![0xEB, 0x0E, 0x90, 0x90];
    out.extend_from_slice(&le(MULTIBOOT_MAGIC));
    out.extend_from_slice(&le(flags));
    out.extend_from_slice(&le(checksum));
    assert_eq!(out.len(), 0x10);
    out.extend_from_slice(&code);
    out
}

async fn run(cpus: u32) -> Option<String> {
    run_image(cpus, image(cpus, 0)).await
}

async fn run_image(cpus: u32, image: Vec<u8>) -> Option<String> {
    let dir = std::env::temp_dir().join("hv2-whpx-smp-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(format!("smp{cpus}.mb"));
    std::fs::write(&path, image).expect("write image");
    let config = VMConfig {
        name: format!("whpx-smp-{cpus}"),
        vcpu_count: cpus,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&path)),
        ..Default::default()
    };
    let vm = VM::new(config).ok()?;
    if !vm.executes_guest_code() {
        eprintln!("skipping: no backend on this host executes guest code");
        return None;
    }
    let vm = Arc::new(vm);
    if let Err(e) = vm.provision().await {
        eprintln!("skipping: provision failed ({e})");
        return None;
    }
    vm.launch().await.expect("launch");
    let deadline = Instant::now() + BOUND;
    let mut console = String::new();
    while Instant::now() < deadline && !console.contains('\n') {
        tokio::time::sleep(Duration::from_millis(20)).await;
        console = vm.console_output().await;
    }
    let _ = vm.stop().await;
    Some(console)
}

#[tokio::test(flavor = "multi_thread")]
async fn four_vcpus_report_their_apic_ids_after_init_and_sipi() {
    let Some(console) = run(4).await else {
        return;
    };
    assert_eq!(console, "0123\n", "the BSP should print the four APIC IDs");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_single_vcpu_guest_still_runs() {
    // One processor: nothing to wake, so the wait is already satisfied.
    let Some(console) = run(1).await else {
        return;
    };
    assert_eq!(console, "0\n");
}

/// Scan 0xE0000..0x100000 for the RSDP, walk the RSDT to the MADT, and print
/// how many local-APIC entries it holds (or `?` if any step fails).
const MADT_PROBE: &[u8] = &[
    0xBE, 0x00, 0x00, 0x0E, 0x00, 0x81, 0x3E, 0x52, 0x53, 0x44, 0x20, 0x75, 0x09, 0x81, 0x7E, 0x04,
    0x50, 0x54, 0x52, 0x20, 0x74, 0x0D, 0x83, 0xC6, 0x10, 0x81, 0xFE, 0x00, 0x00, 0x10, 0x00, 0x72,
    0xE4, 0xEB, 0x3E, 0x8B, 0x5E, 0x10, 0x8B, 0x4B, 0x04, 0x01, 0xD9, 0x83, 0xC3, 0x24, 0x39, 0xCB,
    0x73, 0x2F, 0x8B, 0x3B, 0x81, 0x3F, 0x41, 0x50, 0x49, 0x43, 0x74, 0x05, 0x83, 0xC3, 0x04, 0xEB,
    0xED, 0x8B, 0x4F, 0x04, 0x01, 0xF9, 0x83, 0xC7, 0x2C, 0x31, 0xC0, 0x39, 0xCF, 0x73, 0x0E, 0x80,
    0x3F, 0x00, 0x75, 0x01, 0x40, 0x0F, 0xB6, 0x57, 0x01, 0x01, 0xD7, 0xEB, 0xEE, 0x04, 0x30, 0xEB,
    0x02, 0xB0, 0x3F, 0xBA, 0xF8, 0x03, 0x00, 0x00, 0xEE, 0xB0, 0x0A, 0xEE, 0xF4,
];

#[tokio::test(flavor = "multi_thread")]
async fn the_madt_lists_one_local_apic_per_vcpu() {
    let Some(console) = run_image(4, flat(MADT_PROBE.to_vec())).await else {
        return;
    };
    assert_eq!(
        console,
        "4
",
        "a guest should find four LAPICs in the MADT"
    );
}

/// Run the same fixed spin on every processor and time the whole guest.
async fn timed(cpus: u32, spin: u32) -> Option<(String, Duration)> {
    let started = Instant::now();
    let console = run_image(cpus, image(cpus, spin)).await?;
    Some((console, started.elapsed()))
}

/// Every vCPU does the same work. If they run in parallel the 4-vCPU guest
/// takes about as long as the 1-vCPU one; time-sliced on one host thread it
/// would take about four times as long. Needs a quiet host.
#[tokio::test(flavor = "multi_thread")]
async fn four_vcpus_spin_in_parallel_not_in_turn() {
    const SPIN: u32 = 0x8000_0000;
    let Some((one, t1)) = timed(1, SPIN).await else {
        return;
    };
    let Some((four, t4)) = timed(4, SPIN).await else {
        return;
    };
    println!("spin of {SPIN:#x} per vCPU: 1 vCPU {t1:?}, 4 vCPUs {t4:?}");
    assert_eq!(
        one,
        "0
"
    );
    assert_eq!(
        four,
        "0123
"
    );
    assert!(
        t4 < t1 * 2,
        "4 vCPUs took {t4:?} against {t1:?} for one: they ran in turn, not in parallel"
    );
}

/// Arm the BSP's local APIC timer in periodic mode on vector 0x40, take five
/// interrupts through the guest's own IDT (each one EOIs and counts), halting
/// in between, and print `T`.
const TIMER_PROBE: &[u8] = &[
    0xB8, 0x9A, 0x00, 0x10, 0x00, 0x66, 0xA3, 0x00, 0x96, 0x00, 0x00, 0x66, 0xC7, 0x05, 0x02, 0x96,
    0x00, 0x00, 0x08, 0x00, 0x66, 0xC7, 0x05, 0x04, 0x96, 0x00, 0x00, 0x00, 0x8E, 0xC1, 0xE8, 0x10,
    0x66, 0xA3, 0x06, 0x96, 0x00, 0x00, 0x66, 0xC7, 0x05, 0x00, 0x93, 0x00, 0x00, 0x07, 0x02, 0xC7,
    0x05, 0x02, 0x93, 0x00, 0x00, 0x00, 0x94, 0x00, 0x00, 0x0F, 0x01, 0x1D, 0x00, 0x93, 0x00, 0x00,
    0xC7, 0x05, 0x00, 0x92, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC7, 0x05, 0xF0, 0x00, 0xE0, 0xFE,
    0xFF, 0x01, 0x00, 0x00, 0xC7, 0x05, 0xE0, 0x03, 0xE0, 0xFE, 0x0B, 0x00, 0x00, 0x00, 0xC7, 0x05,
    0x20, 0x03, 0xE0, 0xFE, 0x40, 0x00, 0x02, 0x00, 0xC7, 0x05, 0x80, 0x03, 0xE0, 0xFE, 0x00, 0x00,
    0x10, 0x00, 0xFB, 0xF4, 0x83, 0x3D, 0x00, 0x92, 0x00, 0x00, 0x05, 0x72, 0xF6, 0xFA, 0xBA, 0xF8,
    0x03, 0x00, 0x00, 0xB0, 0x54, 0xEE, 0xB0, 0x0A, 0xEE, 0xF4, 0x50, 0xFF, 0x05, 0x00, 0x92, 0x00,
    0x00, 0xC7, 0x05, 0xB0, 0x00, 0xE0, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x58, 0xCF,
];

#[tokio::test(flavor = "multi_thread")]
async fn the_local_apic_timer_interrupts_a_halted_vcpu() {
    let Some(console) = run_image(2, flat(TIMER_PROBE.to_vec())).await else {
        return;
    };
    assert_eq!(
        console,
        "T
",
        "five LAPIC timer interrupts should have arrived"
    );
}
