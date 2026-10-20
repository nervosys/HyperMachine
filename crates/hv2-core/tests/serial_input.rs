//! Bytes sent with `VM::send_serial` must reach a guest polling COM1.
//!
//! The guest below runs with interrupts off. It writes `R` to COM1 to say it
//! is ready, then polls the line status register (0x3FD, bit 0 = data ready),
//! reads each byte from 0x3F8 and writes it back plus one (a newline is
//! echoed unchanged). The host sends bytes in, reads the echo from the
//! console, and checks every byte came back in order, none lost, none
//! duplicated, and that a refused oversize send queued nothing.
//!
//! Backend independent: it skips, rather than fails, when the host has no
//! backend that executes guest code.

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// COM1's data port, which the legacy machine maps to a 16550.
const COM1: u32 = 0x3F8;
/// COM1's line status register; bit 0 is "received data ready".
const COM1_LSR: u32 = 0x3FD;
const MULTIBOOT_MAGIC: u32 = 0x1BAD_B002;
/// Flags bit 16: the header carries its own load addresses.
const AOUT_KLUDGE: u32 = 1 << 16;
/// Where the image loads (and where its absolute addresses point).
const BASE: u32 = 0x0030_0000;
/// Offsets of the data tables from the image start.
const GDTR: u32 = 0x200;
const GDT: u32 = 0x220;

fn le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// The 32-bit code, entered right after the Multiboot header.
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
    // Say `R` on COM1: ready.
    c.push(0xBA); // mov edx, COM1
    c.extend_from_slice(&le(COM1));
    c.extend_from_slice(&[0xB0, b'R']); // mov al, 'R'
    c.push(0xEE); // out dx, al

    // poll:
    c.push(0xBA); // mov edx, COM1_LSR
    c.extend_from_slice(&le(COM1_LSR));
    c.push(0xEC); // in al, dx        ; read the line status
    c.extend_from_slice(&[0xA8, 0x01]); // test al, 1     ; data ready?
    c.extend_from_slice(&[0x74, 0xF6]); // jz poll        ; no: back 10 bytes
    c.push(0xBA); // mov edx, COM1
    c.extend_from_slice(&le(COM1));
    c.push(0xEC); // in al, dx        ; the received byte
    c.extend_from_slice(&[0x3C, 0x0A]); // cmp al, '\n'
    c.extend_from_slice(&[0x74, 0x02]); // je emit          ; newline: skip the inc
    c.extend_from_slice(&[0xFE, 0xC0]); // inc al           ; any other byte + 1
                                        // emit:
    c.push(0xEE); // out dx, al       ; echo to COM1 (EDX is still COM1)
    c.extend_from_slice(&[0xEB, 0xE7]); // jmp poll         ; back 25 bytes
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

#[tokio::test(flavor = "multi_thread")]
async fn serial_input_round_trips_in_order() {
    let dir = std::env::temp_dir().join("hv2-serial-input-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("serial.mb");
    std::fs::write(&path, image()).expect("write image");

    let config = VMConfig {
        name: "serial-input".into(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&path)),
        ..Default::default()
    };
    let Ok(vm) = VM::new(config) else {
        eprintln!("serial input: VM could not be created here; skipped");
        return;
    };
    if !vm.executes_guest_code() {
        eprintln!("serial input: no hardware backend on this host; skipped");
        return;
    }
    let vm = Arc::new(vm);
    if let Err(e) = vm.provision().await {
        eprintln!("serial input: provision failed ({e}); skipped");
        return;
    }
    if let Err(e) = vm.launch().await {
        eprintln!("serial input: launch failed ({e}); skipped");
        return;
    }

    let (out, ready) = wait_for(&vm, |o| o.contains(&b'R')).await;
    if !ready {
        let _ = vm.stop().await;
        panic!("the guest never announced readiness; COM1: {out:?}");
    }

    // A short line first.
    let started = Instant::now();
    if let Err(e) = vm.send_serial(b"abc\n").await {
        let _ = vm.stop().await;
        panic!("send_serial failed: {e}");
    }
    let mut expected = b"Rbcd\n".to_vec();
    let (out, got) = wait_for(&vm, |o| o.len() >= expected.len()).await;
    if !got {
        let _ = vm.stop().await;
        panic!("the echo never completed; COM1: {out:?}");
    }
    assert_eq!(out, expected, "abc\\n must come back as bcd\\n");
    println!("serial input: short line echoed in {:?}", started.elapsed());

    // Three sends of 200 bytes, a repeating a..z pattern.
    let started = Instant::now();
    for round in 0..3usize {
        let chunk: Vec<u8> = (0..200usize)
            .map(|i| b'a' + ((round * 200 + i) % 26) as u8)
            .collect();
        expected.extend(chunk.iter().map(|b| b + 1));
        if let Err(e) = vm.send_serial(&chunk).await {
            let _ = vm.stop().await;
            panic!("send_serial of chunk {round} failed: {e}");
        }
    }
    let (out, got) = wait_for(&vm, |o| o.len() >= expected.len()).await;
    if !got {
        let _ = vm.stop().await;
        panic!(
            "only {} of {} bytes came back; COM1: {out:?}",
            out.len(),
            expected.len()
        );
    }
    // Give a stray duplicate a moment to show up before judging.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let out = com1(&vm).await;
    println!(
        "serial input: 600 bytes in 3 sends echoed in {:?}",
        started.elapsed()
    );
    assert_eq!(out, expected, "bytes must return in order, none lost");

    // More than the 4096-byte buffer in one call: refused whole.
    let refused = vm.send_serial(&[b'x'; 5000]).await;
    // Then a marker; 'z' echoes as '{'. Any 'y' (x + 1) means part was queued.
    let marker = vm.send_serial(b"z\n").await;
    expected.extend_from_slice(b"{\n");
    let (out, got) = wait_for(&vm, |o| o.len() >= expected.len()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = com1(&vm).await;
    let _ = vm.stop().await;
    assert!(refused.is_err(), "5000 bytes cannot fit a 4096-byte buffer");
    marker.expect("the marker send");
    assert!(
        got,
        "the marker never came back; COM1 tail: {:?}",
        &out[out.len().saturating_sub(16)..]
    );
    assert_eq!(
        after.len(),
        expected.len(),
        "the refused send must queue nothing; extra bytes: {:?}",
        after.get(expected.len() - 2..)
    );
    assert_eq!(after, expected, "the refused send must queue nothing");
}
