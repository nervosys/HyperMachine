//! Keystrokes sent with `VM::send_key` must reach a guest halted in `HLT`.
//!
//! The guest below remaps the PICs, opens only IRQ 1, writes `R` to COM1 to
//! say it is ready, then `sti` and `hlt` in a loop. Its vector-33 handler
//! reads port 0x60, echoes the byte to COM1 and sends the EOI. The host queues
//! several scancodes at once, so the controller must raise IRQ 1 again as each
//! byte is drained: every byte must arrive, in order, exactly once.
//!
//! Backend independent: it skips, rather than fails, when the host has no
//! backend that executes guest code.

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
const IDT: u32 = 0x300;
/// IRQ 1 with the master PIC based at 0x20.
const KEY_VECTOR: u32 = 33;

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
    out8(&mut c, 0x21, 0xFD); // only IRQ1 open
    out8(&mut c, 0xA1, 0xFF);
    // Say `R` on COM1: ready.
    c.push(0xBA); // mov edx, COM1
    c.extend_from_slice(&le(COM1));
    c.extend_from_slice(&[0xB0, b'R']); // mov al, 'R'
    c.push(0xEE); // out dx, al
    c.push(0xFB); // sti
    c.extend_from_slice(&[0xF4, 0xEB, 0xFD]); // loop: hlt ; jmp loop
    c
}

/// Vector-33 handler: read the scancode, echo it to COM1, EOI, return.
fn handler() -> Vec<u8> {
    let mut h = vec![0x50, 0x52]; // push eax ; push edx
    h.extend_from_slice(&[0xE4, 0x60]); // in al, 0x60
    h.push(0xBA); // mov edx, COM1
    h.extend_from_slice(&le(COM1));
    h.push(0xEE); // out dx, al
    h.extend_from_slice(&[0xB0, 0x20, 0xE6, 0x20]); // mov al,0x20 ; out 0x20,al
    h.extend_from_slice(&[0x5A, 0x58]); // pop edx ; pop eax
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
    // IDT: entries 0..=33; only vector 33 is a gate (32-bit interrupt, 0x8E).
    let mut idtr = ((KEY_VECTOR as u16 + 1) * 8 - 1).to_le_bytes().to_vec();
    idtr.extend_from_slice(&le(BASE + IDT));
    put(&mut img, IDTR, &idtr);
    let mut gate = Vec::new();
    gate.extend_from_slice(&(handler_addr as u16).to_le_bytes());
    gate.extend_from_slice(&0x0008u16.to_le_bytes());
    gate.extend_from_slice(&[0, 0x8E]);
    gate.extend_from_slice(&((handler_addr >> 16) as u16).to_le_bytes());
    put(&mut img, IDT + KEY_VECTOR * 8, &gate);
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

#[tokio::test(flavor = "multi_thread")]
async fn queued_keystrokes_reach_a_guest_halted_with_interrupts_enabled() {
    let dir = std::env::temp_dir().join("hv2-key-delivery-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("key.mb");
    std::fs::write(&path, image()).expect("write image");

    let config = VMConfig {
        name: "key-delivery".into(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&path)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("key delivery: VM could not be created here; skipped");
        return;
    };
    if !vm.executes_guest_code() {
        eprintln!("key delivery: no hardware backend on this host; skipped");
        return;
    }
    let vm = Arc::new(vm);
    if let Err(e) = vm.provision().await {
        eprintln!("key delivery: provision failed ({e}); skipped");
        return;
    }
    if let Err(e) = vm.launch().await {
        eprintln!("key delivery: launch failed ({e}); skipped");
        return;
    }

    let (out, ready) = wait_for(&vm, |o| o.contains(&b'R')).await;
    if !ready {
        let _ = vm.stop().await;
        panic!("the guest never announced readiness; COM1: {out:?}");
    }

    // Left Super make, Return make, Return break, Super break.
    let keys = [0xE0, 0x5B, 0x1C, 0x9C, 0xE0, 0xDB];
    let started = Instant::now();
    if let Err(e) = vm.send_key(&keys).await {
        let _ = vm.stop().await;
        panic!("send_key failed: {e}");
    }
    let (out, got) = wait_for(&vm, |o| o.len() > keys.len()).await;
    let elapsed = started.elapsed();
    if !got {
        let _ = vm.stop().await;
        panic!("not every scancode reached the guest; COM1: {out:02X?}");
    }
    // Give a stray duplicate a moment to show up before judging.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let out = com1(&vm).await;
    println!(
        "key delivery: {} scancodes delivered in {elapsed:?}; COM1: {out:02X?}",
        keys.len()
    );
    assert_eq!(out[0], b'R');
    assert_eq!(
        &out[1..],
        &keys[..],
        "scancodes must arrive in order, none lost, none duplicated"
    );

    // Too many bytes for the 16-byte buffer: refused whole, nothing queued.
    assert!(
        vm.send_key(&[0x1E; 17]).await.is_err(),
        "17 bytes cannot fit a 16-byte buffer"
    );
    vm.send_key(&[0x30]).await.expect("one more key");
    let (out, got) = wait_for(&vm, |o| o.len() > 1 + keys.len()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = com1(&vm).await;
    let _ = vm.stop().await;
    assert!(got, "the follow-up key never arrived; COM1: {out:02X?}");
    assert_eq!(
        &after[1 + keys.len()..],
        &[0x30],
        "the refused send must queue nothing; COM1: {after:02X?}"
    );
}
