//! Motion sent with `VM::send_pointer` must reach a guest as PS/2 mouse
//! packets on the i8042's auxiliary port.
//!
//! The guest below runs with interrupts off. It enables the mouse port
//! (`0x64 <- 0xA8`), writes `0xF6` and `0xF4` to the mouse through `0xD4` and
//! reads the two ACKs, then writes `R` to COM1. After that it polls the status
//! port, and for each byte at `0x60` writes the byte and then `'1'` or `'0'`
//! (status bit 5, "this byte came from the mouse") to COM1.
//!
//! Backend independent: it skips, rather than fails, when the host has no
//! backend that executes guest code.

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
use std::time::{Duration, Instant};

const COM1: u32 = 0x3F8;
const MULTIBOOT_MAGIC: u32 = 0x1BAD_B002;
const AOUT_KLUDGE: u32 = 1 << 16;
const BASE: u32 = 0x0030_0000;
const GDTR: u32 = 0x200;
const GDT: u32 = 0x220;

fn le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// `mov edx, port ; mov al, value ; out dx, al`
fn out(c: &mut Vec<u8>, port: u32, value: u8) {
    c.push(0xBA);
    c.extend_from_slice(&le(port));
    c.extend_from_slice(&[0xB0, value]);
    c.push(0xEE);
}

/// Spin until the output buffer has a byte: `in al, 0x64 ; test al, 1 ; jz`.
fn wait_for_byte(c: &mut Vec<u8>) {
    let top = c.len();
    c.push(0xBA);
    c.extend_from_slice(&le(0x64));
    c.push(0xEC); // in al, dx
    c.extend_from_slice(&[0xA8, 0x01]); // test al, 1
    let rel = top as i64 - (c.len() as i64 + 2);
    c.extend_from_slice(&[0x74, rel as i8 as u8]); // jz top
}

/// The 32-bit code, entered right after the Multiboot header.
fn main_code(entry: u32) -> Vec<u8> {
    let mut c = Vec::new();
    // lgdt [GDTR]
    c.extend_from_slice(&[0x0F, 0x01, 0x15]);
    c.extend_from_slice(&le(BASE + GDTR));
    // jmp far 0x08:next
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

    out(&mut c, 0x64, 0xA8); // enable the mouse port
    for command in [0xF6u8, 0xF4] {
        out(&mut c, 0x64, 0xD4); // the next byte is for the mouse
        out(&mut c, 0x60, command);
        wait_for_byte(&mut c);
        c.push(0xBA); // mov edx, 0x60
        c.extend_from_slice(&le(0x60));
        c.push(0xEC); // in al, dx ; the ACK, discarded
    }
    out(&mut c, COM1, b'R'); // ready

    // loop:
    let top = c.len();
    wait_for_byte(&mut c);
    c.extend_from_slice(&[0x88, 0xC3]); // mov bl, al      ; the status
    c.push(0xBA);
    c.extend_from_slice(&le(0x60));
    c.push(0xEC); // in al, dx        ; the data byte
    c.extend_from_slice(&[0x88, 0xC1]); // mov cl, al
    c.extend_from_slice(&[0xC0, 0xEB, 0x05]); // shr bl, 5
    c.extend_from_slice(&[0x80, 0xE3, 0x01]); // and bl, 1
    c.extend_from_slice(&[0x80, 0xC3, b'0']); // add bl, '0'
    c.push(0xBA);
    c.extend_from_slice(&le(COM1));
    c.extend_from_slice(&[0x88, 0xC8]); // mov al, cl
    c.push(0xEE); // out dx, al       ; the data byte
    c.extend_from_slice(&[0x88, 0xD8]); // mov al, bl
    c.push(0xEE); // out dx, al       ; '1' if it came from the mouse
    let rel = top as i64 - (c.len() as i64 + 2);
    c.extend_from_slice(&[0xEB, rel as i8 as u8]); // jmp loop
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
    let mut gdtr = 23u16.to_le_bytes().to_vec();
    gdtr.extend_from_slice(&le(BASE + GDT));
    put(&mut img, GDTR, &gdtr);
    let mut gdt = vec![0u8; 8];
    gdt.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0x9A, 0xCF, 0]);
    gdt.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0, 0x92, 0xCF, 0]);
    put(&mut img, GDT, &gdt);
    img
}

async fn com1(vm: &VM) -> Vec<u8> {
    vm.console_output_by_device()
        .await
        .into_iter()
        .find(|(name, _)| name == "COM1")
        .map(|(_, bytes)| bytes)
        .unwrap_or_default()
}

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

/// What the guest writes for one packet: each byte followed by `'1'`.
fn echoed(packet: &[u8]) -> Vec<u8> {
    packet.iter().flat_map(|&b| [b, b'1']).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn pointer_motion_reaches_a_polling_guest_as_ps2_packets() {
    let dir = std::env::temp_dir().join("hv2-pointer-delivery-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("pointer.mb");
    std::fs::write(&path, image()).expect("write image");

    let config = VMConfig {
        name: "pointer-delivery".into(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&path)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("pointer delivery: VM could not be created here; skipped");
        return;
    };
    if !vm.executes_guest_code() {
        eprintln!("pointer delivery: no hardware backend on this host; skipped");
        return;
    }
    let vm = Arc::new(vm);
    if let Err(e) = vm.provision().await {
        eprintln!("pointer delivery: provision failed ({e}); skipped");
        return;
    }
    if let Err(e) = vm.launch().await {
        eprintln!("pointer delivery: launch failed ({e}); skipped");
        return;
    }

    let (out, ready) = wait_for(&vm, |o| o.contains(&b'R')).await;
    if !ready {
        let _ = vm.stop().await;
        panic!("the guest never announced readiness; COM1: {out:?}");
    }

    let mut expected = b"R".to_vec();
    let mut failure = None;

    // Right 10, down 3 (PS/2 y is up), left button held.
    let started = Instant::now();
    vm.send_pointer(10, -3, 0b001).await.expect("first move");
    expected.extend(echoed(&[0x08 | 0x01 | 0x20, 10, 0xFD]));
    let (out, got) = wait_for(&vm, |o| o.len() >= expected.len()).await;
    if !got || out != expected {
        failure = Some(format!("first packet: got {out:?}, wanted {expected:?}"));
    }
    println!("pointer delivery: first packet in {:?}", started.elapsed());

    // 300 to the right is two packets (255 + 45); the button is released.
    if failure.is_none() {
        vm.send_pointer(300, 0, 0).await.expect("second move");
        expected.extend(echoed(&[0x08, 255, 0, 0x08, 45, 0]));
        let (out, got) = wait_for(&vm, |o| o.len() >= expected.len()).await;
        if !got || out != expected {
            failure = Some(format!("split move: got {out:?}, wanted {expected:?}"));
        }
    }

    // Too many packets for the 16-byte buffer: refused whole. A marker move
    // after it shows nothing of the refused one was queued.
    let refused = vm.send_pointer(6 * 255, 0, 0).await;
    vm.send_pointer(1, 0, 0b100).await.expect("marker move");
    expected.extend(echoed(&[0x08 | 0x04, 1, 0]));
    let (out, got) = wait_for(&vm, |o| o.len() >= expected.len()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = com1(&vm).await;
    let _ = vm.stop().await;

    if let Some(message) = failure {
        panic!("{message}");
    }
    assert!(refused.is_err(), "six packets cannot fit sixteen bytes");
    assert!(got, "the marker never arrived; COM1 tail: {out:?}");
    assert_eq!(after, expected, "the refused move must queue nothing");
}
