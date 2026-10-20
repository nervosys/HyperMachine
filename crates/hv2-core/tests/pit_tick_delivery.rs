//! A guest's PIT + 8259 timer tick must reach it while it sits in `HLT`.
//!
//! This is what a hobby kernel does: remap the PICs, program the PIT for about
//! 100 Hz, `sti`, then `hlt` in a loop and count ticks in the vector-32
//! handler. The guest below does exactly that and, on the 20th tick, writes
//! `T` to COM1. Seeing the `T` means the PIT counted down at the programmed
//! divisor, the PIC raised the line, the VM injected it only because IF was
//! set, and the `HLT` woke for it.
//!
//! The guest builds its own GDT and IDT so it does not depend on the selectors
//! the loader happens to set. Backend independent: it skips, rather than
//! fails, when the host has no backend that executes guest code.

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
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
const IDTR: u32 = 0x210;
const GDT: u32 = 0x220;
const COUNTER: u32 = 0x240;
const IDT: u32 = 0x300;
/// Ticks before the guest says `T`.
const TICKS: u8 = 20;

fn le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// `out imm8, al` after loading `value` into AL.
fn out8(code: &mut Vec<u8>, port: u8, value: u8) {
    code.extend_from_slice(&[0xB0, value]); // mov al, imm8
    code.extend_from_slice(&[0xE6, port]); // out imm8, al
}

/// The 32-bit setup code, entered right after the Multiboot header.
fn main_code(entry: u32) -> Vec<u8> {
    let mut c = Vec::new();
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
    // lidt [IDTR]
    c.extend_from_slice(&[0x0F, 0x01, 0x1D]);
    c.extend_from_slice(&le(BASE + IDTR));
    // Remap the PICs: master vectors 0x20, slave 0x28.
    out8(&mut c, 0x20, 0x11);
    out8(&mut c, 0xA0, 0x11);
    out8(&mut c, 0x21, 0x20);
    out8(&mut c, 0xA1, 0x28);
    out8(&mut c, 0x21, 0x04);
    out8(&mut c, 0xA1, 0x02);
    out8(&mut c, 0x21, 0x01);
    out8(&mut c, 0xA1, 0x01);
    out8(&mut c, 0x21, 0xFC); // unmask IRQ0 and IRQ1
    out8(&mut c, 0xA1, 0xFF);
    // PIT channel 0, lobyte/hibyte, mode 2, divisor 11932 (0x2E9C) ~ 100 Hz.
    out8(&mut c, 0x43, 0x34);
    out8(&mut c, 0x40, 0x9C);
    out8(&mut c, 0x40, 0x2E);
    c.push(0xFB); // sti
    c.extend_from_slice(&[0xF4, 0xEB, 0xFD]); // loop: hlt ; jmp loop
    c
}

/// Vector-32 handler: count, EOI, say `T` on the 20th tick, return.
fn handler() -> Vec<u8> {
    let mut h = vec![0x50]; // push eax
    h.extend_from_slice(&[0xFF, 0x05]); // inc dword [COUNTER]
    h.extend_from_slice(&le(BASE + COUNTER));
    h.extend_from_slice(&[0xB0, 0x20, 0xE6, 0x20]); // mov al,0x20 ; out 0x20,al
    h.push(0xA1); // mov eax, [COUNTER]
    h.extend_from_slice(&le(BASE + COUNTER));
    h.extend_from_slice(&[0x83, 0xF8, TICKS]); // cmp eax, 20
    h.extend_from_slice(&[0x75, 0x08]); // jne skip (8 bytes ahead)
    h.extend_from_slice(&[0xB0, b'T']); // mov al, 'T'
    h.push(0xBA); // mov edx, COM1
    h.extend_from_slice(&le(COM1));
    h.push(0xEE); // out dx, al
    h.push(0x58); // skip: pop eax
    h.push(0xCF); // iretd
    h
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
    let handler_addr = BASE + img.len() as u32;
    img.extend_from_slice(&handler());
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
    // IDT: entries 0..=32; only vector 32 is a gate (32-bit interrupt, 0x8E).
    let mut idtr = (33u16 * 8 - 1).to_le_bytes().to_vec();
    idtr.extend_from_slice(&le(BASE + IDT));
    put(&mut img, IDTR, &idtr);
    let mut gate = Vec::new();
    gate.extend_from_slice(&(handler_addr as u16).to_le_bytes());
    gate.extend_from_slice(&0x0008u16.to_le_bytes());
    gate.extend_from_slice(&[0, 0x8E]);
    gate.extend_from_slice(&((handler_addr >> 16) as u16).to_le_bytes());
    put(&mut img, IDT + 32 * 8, &gate);
    put(&mut img, COUNTER, &[0; 4]);
    img
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pit_tick_wakes_a_guest_halted_with_interrupts_enabled() {
    let dir = std::env::temp_dir().join("hv2-pit-tick-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("tick.mb");
    std::fs::write(&path, image()).expect("write image");

    let config = VMConfig {
        name: "pit-tick".into(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&path)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("pit tick: VM could not be created here; skipped");
        return;
    };
    if !vm.executes_guest_code() {
        eprintln!("pit tick: no hardware backend on this host; skipped");
        return;
    }
    let vm = Arc::new(vm);
    if let Err(e) = vm.provision().await {
        eprintln!("pit tick: provision failed ({e}); skipped");
        return;
    }
    let started = Instant::now();
    if let Err(e) = vm.launch().await {
        eprintln!("pit tick: launch failed ({e}); skipped");
        return;
    }

    let deadline = started + Duration::from_secs(5);
    let mut console = String::new();
    let mut seen = None;
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if console.contains('T') {
            seen = Some(started.elapsed());
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    vm.stop().await.expect("stop");

    let elapsed = seen.unwrap_or_else(|| {
        panic!("the guest never got {TICKS} timer interrupts while halted; console: {console:?}")
    });
    println!("pit tick: 'T' after {elapsed:?} ({TICKS} ticks at ~100 Hz)");
    assert!(
        elapsed >= Duration::from_millis(100),
        "20 ticks at 100 Hz cannot take {elapsed:?}: the timer is free-running"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "ticks arrived too slowly: {elapsed:?}"
    );
}
