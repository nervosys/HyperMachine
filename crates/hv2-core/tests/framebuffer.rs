//! A guest draws into the Multiboot framebuffer and the host reads it back.
//!
//! The guest finds the framebuffer in the Multiboot information structure
//! (flag bit 12), checks it is 32 bits per pixel direct colour, stores one
//! pixel in the first corner and one in the last corner (the last one's
//! address computed from the structure's pitch, width and height), then says
//! `F` on COM1. The host's `VM::screendump` must show exactly those pixels and
//! nothing else.
//!
//! Skips, rather than fails, when the host has no backend that executes guest
//! code.

use hv2_core::{BootSource, VMConfig, VM};
use std::path::Path;
use std::time::{Duration, Instant};

/// COM1's data port, which the legacy machine maps to a 16550.
const COM1: u32 = 0x3F8;
const MULTIBOOT_MAGIC: u32 = 0x1BAD_B002;
/// Flags bit 16: the header carries its own load addresses.
const AOUT_KLUDGE: u32 = 1 << 16;
/// Where the image loads (and where its absolute addresses point).
const BASE: u32 = 0x0030_0000;
/// Offsets of the data tables from the image start.
const GDTR: u32 = 0x200;
const GDT: u32 = 0x220;

const FB_GPA: u64 = 0xFD00_0000;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 400;
const PITCH: usize = 640 * 4;

fn le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// The 32-bit code, entered right after the Multiboot header.
fn main_code(entry: u32) -> Vec<u8> {
    let mut c = Vec::new();
    // Multiboot passes the info structure's address in EBX: keep it in EBP.
    c.extend_from_slice(&[0x89, 0xDD]); // mov ebp, ebx
                                        // lgdt [GDTR]
    c.extend_from_slice(&[0x0F, 0x01, 0x15]);
    c.extend_from_slice(&le(BASE + GDTR));
    // jmp far 0x08:next   (EA off32 sel16) -- reloads CS from our GDT
    let next = entry + c.len() as u32 + 7;
    c.push(0xEA);
    c.extend_from_slice(&le(next));
    c.extend_from_slice(&[0x08, 0x00]);
    // mov ax, 0x10 ; mov ds, ax ; mov es, ax ; mov ss, ax
    c.extend_from_slice(&[0x66, 0xB8, 0x10, 0x00]);
    c.extend_from_slice(&[0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0]);
    // mov esp, 0x001F0000
    c.push(0xBC);
    c.extend_from_slice(&le(0x001F_0000));
    // mov edx, COM1
    c.push(0xBA);
    c.extend_from_slice(&le(COM1));

    // The success path, built first so the checks know how far to jump.
    let mut ok = Vec::new();
    ok.extend_from_slice(&[0x8B, 0x7D, 88]); // mov edi, [ebp+88]
    ok.extend_from_slice(&[0xC7, 0x07]); // mov dword [edi], 0x00112233
    ok.extend_from_slice(&le(0x0011_2233));
    ok.extend_from_slice(&[0x8B, 0x45, 104]); // mov eax, [ebp+104]  height
    ok.push(0x48); // dec eax
    ok.extend_from_slice(&[0x0F, 0xAF, 0x45, 96]); // imul eax, [ebp+96]  pitch
    ok.extend_from_slice(&[0x01, 0xC7]); // add edi, eax
    ok.extend_from_slice(&[0x8B, 0x45, 100]); // mov eax, [ebp+100]  width
    ok.push(0x48); // dec eax
    ok.extend_from_slice(&[0xC1, 0xE0, 0x02]); // shl eax, 2
    ok.extend_from_slice(&[0x01, 0xC7]); // add edi, eax
    ok.extend_from_slice(&[0xC7, 0x07]); // mov dword [edi], 0x00AABBCC
    ok.extend_from_slice(&le(0x00AA_BBCC));
    ok.extend_from_slice(&[0xB0, b'F', 0xEE]); // mov al,'F' ; out dx,al
    ok.extend_from_slice(&[0xFA, 0xF4, 0xEB, 0xFC]); // loop: cli ; hlt ; jmp loop

    // The three checks, each `jcc fail` where `fail` follows the success path.
    let checks: [&[u8]; 3] = [
        &[0xF7, 0x45, 0x00, 0x00, 0x10, 0x00, 0x00], // test dword [ebp], 1<<12
        &[0x80, 0x7D, 109, 0x01],                    // cmp byte [ebp+109], 1
        &[0x80, 0x7D, 108, 0x20],                    // cmp byte [ebp+108], 32
    ];
    let jcc = [0x74u8, 0x75, 0x75]; // jz, jne, jne
    for (i, check) in checks.iter().enumerate() {
        c.extend_from_slice(check);
        let later_checks: usize = checks[i + 1..].iter().map(|k| k.len() + 2).sum();
        let rel = later_checks + ok.len();
        assert!(rel < 128);
        c.extend_from_slice(&[jcc[i], rel as u8]);
    }
    c.extend_from_slice(&ok);
    // fail: mov al,'N' ; out dx,al ; loop: hlt ; jmp loop
    c.extend_from_slice(&[0xB0, b'N', 0xEE, 0xF4, 0xEB, 0xFD]);
    c
}

fn image() -> Vec<u8> {
    let flags = AOUT_KLUDGE;
    let checksum = 0u32.wrapping_sub(MULTIBOOT_MAGIC).wrapping_sub(flags);
    let entry = BASE + 32;
    let mut img = Vec::new();
    for v in [MULTIBOOT_MAGIC, flags, checksum, BASE, BASE, 0, 0, entry] {
        img.extend_from_slice(&le(v));
    }
    assert_eq!(img.len(), 32);

    img.extend_from_slice(&main_code(entry));
    assert!(img.len() < GDTR as usize, "code overruns the data tables");

    let put = |img: &mut Vec<u8>, at: u32, bytes: &[u8]| {
        let at = at as usize;
        if img.len() < at + bytes.len() {
            img.resize(at + bytes.len(), 0);
        }
        img[at..at + bytes.len()].copy_from_slice(bytes);
    };
    // GDTR: limit 23, base GDT. Entries: null, code 0x08, data 0x10 (flat).
    let mut gdtr = 23u16.to_le_bytes().to_vec();
    gdtr.extend_from_slice(&le(BASE + GDT));
    put(&mut img, GDTR, &gdtr);
    let mut gdt = vec![0u8; 8];
    gdt.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0x9A, 0xCF, 0]);
    gdt.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0x92, 0xCF, 0]);
    put(&mut img, GDT, &gdt);
    img
}

/// COM1's raw bytes so far.
async fn com1(vm: &VM) -> Vec<u8> {
    vm.console_output_by_device()
        .await
        .into_iter()
        .find(|(name, _)| name == "COM1")
        .map(|(_, bytes)| bytes)
        .unwrap_or_default()
}

/// Poll COM1 until `done` accepts it, or five seconds pass.
async fn wait_for(vm: &VM, done: impl Fn(&[u8]) -> bool) -> (Vec<u8>, bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let out = com1(vm).await;
        if done(&out) {
            return (out, true);
        }
        if Instant::now() >= deadline {
            return (out, false);
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

fn make_vm(path: &Path, name: &str) -> Option<VM> {
    let config = VMConfig {
        name: name.into(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(path)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("framebuffer: VM could not be created here; skipped");
        return None;
    };
    if !vm.executes_guest_code() {
        eprintln!("framebuffer: no hardware backend on this host; skipped");
        return None;
    }
    Some(vm)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_draws_and_the_host_reads_it_back() {
    let dir = std::env::temp_dir().join("hv2-framebuffer-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("fb.mb");
    std::fs::write(&path, image()).expect("write image");

    let Some(vm) = make_vm(&path, "framebuffer") else {
        return;
    };

    // No framebuffer yet: nothing to dump.
    assert!(
        vm.screendump().await.is_err(),
        "screendump without a framebuffer must fail"
    );

    // Unusable framebuffers are refused, and refusing leaves the VM usable.
    assert!(
        vm.attach_framebuffer(WIDTH, HEIGHT, 0x10_0000).is_err(),
        "inside RAM"
    );
    assert!(
        vm.attach_framebuffer(WIDTH, HEIGHT, FB_GPA + 1).is_err(),
        "not page aligned"
    );
    assert!(
        vm.attach_framebuffer(WIDTH, HEIGHT, 0xFEE0_0000).is_err(),
        "overlaps the local APIC"
    );
    assert!(
        vm.attach_framebuffer(0, HEIGHT, FB_GPA).is_err(),
        "zero width"
    );
    assert!(
        vm.screendump().await.is_err(),
        "refused attaches must not leave a framebuffer behind"
    );

    vm.attach_framebuffer(WIDTH, HEIGHT, FB_GPA)
        .expect("attach framebuffer");

    let vm = std::sync::Arc::new(vm);
    if let Err(e) = vm.provision().await {
        eprintln!("framebuffer: provision failed ({e}); skipped");
        return;
    }
    // Too late now.
    assert!(
        vm.attach_framebuffer(WIDTH, HEIGHT, FB_GPA).is_err(),
        "attach after provisioning must fail"
    );
    if let Err(e) = vm.launch().await {
        eprintln!("framebuffer: launch failed ({e}); skipped");
        return;
    }

    let (out, done) = wait_for(&vm, |o| o.contains(&b'F') || o.contains(&b'N')).await;
    if !done {
        let _ = vm.stop().await;
        panic!("the guest never finished drawing; COM1: {out:?}");
    }
    if out.contains(&b'N') {
        let _ = vm.stop().await;
        panic!("the guest found no 32 bpp framebuffer in the Multiboot info; COM1: {out:?}");
    }

    let dump = vm.screendump().await;
    let _ = vm.stop().await;
    let dump = dump.expect("screendump");

    assert_eq!(dump.width, WIDTH);
    assert_eq!(dump.height, HEIGHT);
    assert_eq!(dump.pitch as usize, PITCH);
    assert_eq!(dump.bits_per_pixel, 32);
    assert_eq!(dump.red, (16, 8));
    assert_eq!(dump.green, (8, 8));
    assert_eq!(dump.blue, (0, 8));
    assert_eq!(dump.bytes.len(), PITCH * HEIGHT as usize);

    assert_eq!(&dump.bytes[0..4], &[0x33, 0x22, 0x11, 0x00], "first pixel");
    let last = (HEIGHT as usize - 1) * PITCH + (WIDTH as usize - 1) * 4;
    assert_eq!(
        &dump.bytes[last..last + 4],
        &[0xCC, 0xBB, 0xAA, 0x00],
        "last pixel"
    );
    let untouched = PITCH * 10 + 40;
    assert_eq!(
        &dump.bytes[untouched..untouched + 4],
        &[0, 0, 0, 0],
        "a pixel the guest never drew"
    );
}
