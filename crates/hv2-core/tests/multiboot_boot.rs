//! A Multiboot guest must run whichever way it was built.
//!
//! The unit tests in `boot::multiboot` check where an image is placed. These
//! check that a guest placed that way actually executes, which is a different
//! claim: it needs the load address, the entry, the CPU mode, the I/O exit and
//! the device routing all to be right at once, and a byte arriving on COM1 is
//! the only thing that says so.
//!
//! The three shapes exist because only one of them used to work. A flat image
//! was written to 1 MB and entered at its first byte, which is correct for a
//! flat image and wrong for everything a linker produces — an ELF was entered
//! at `\x7fELF`, decoded as `jns +0x45`, and jumped into its own header. The
//! two address-carrying shapes here deliberately load somewhere other than
//! 1 MB and enter somewhere other than their first byte, so that falling back
//! to the old convention fails the test rather than passing it by luck.
//!
//! Requires `/dev/kvm`; without it these skip rather than fail.

#![cfg(target_os = "linux")]

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// COM1's data port, which `Machine::legacy_pc` maps to a 16550.
const COM1: u16 = 0x3F8;
/// The magic a Multiboot 1.0 header starts with.
const MULTIBOOT_MAGIC: u32 = 0x1BAD_B002;
/// Flags bit 16: the header carries its own load addresses.
const AOUT_KLUDGE: u32 = 1 << 16;
/// Where the address-carrying images ask to go. Not 1 MB, on purpose.
const ELSEWHERE: u32 = 0x0030_0000;
/// What every guest writes.
const GREETING: &str = "MB\n";

/// Long enough that a slow host is not a failure, short enough that a guest
/// which never speaks is reported rather than waited on.
const BOUND: Duration = Duration::from_secs(10);

/// 32-bit code: write `GREETING` to COM1, then halt.
fn code() -> Vec<u8> {
    let mut out = vec![0xBA]; // mov edx, imm32
    out.extend_from_slice(&u32::from(COM1).to_le_bytes());
    for byte in GREETING.bytes() {
        out.push(0xB0); // mov al, imm8
        out.push(byte);
        out.push(0xEE); // out dx, al
    }
    out.push(0xF4); // hlt
    out
}

fn header(flags: u32, addresses: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let checksum = 0u32.wrapping_sub(MULTIBOOT_MAGIC).wrapping_sub(flags);
    out.extend_from_slice(&MULTIBOOT_MAGIC.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&checksum.to_le_bytes());
    for value in addresses {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// No addresses: the loader's convention decides, and the entry is byte zero,
/// so the image opens with a jump over its own header.
fn flat() -> Vec<u8> {
    let mut image = vec![0xEB, 0x0E, 0x90, 0x90];
    image.extend_from_slice(&header(0, &[]));
    assert_eq!(image.len(), 0x10);
    image.extend_from_slice(&code());
    image
}

/// The header's own address fields, loading at `ELSEWHERE` and entering after
/// the 32-byte header.
fn with_address_fields() -> Vec<u8> {
    let mut image = header(AOUT_KLUDGE, &[ELSEWHERE, ELSEWHERE, 0, 0, ELSEWHERE + 32]);
    assert_eq!(image.len(), 32);
    image.extend_from_slice(&code());
    image
}

/// An ELF32 with one `PT_LOAD` segment, entered past the Multiboot header that
/// sits at the start of the segment.
fn elf32() -> Vec<u8> {
    const EHSIZE: u32 = 52;
    const PHENTSIZE: u32 = 32;
    let payload_offset = EHSIZE + PHENTSIZE;

    let mut payload = header(0, &[]);
    let entry_in_payload = payload.len() as u32;
    payload.extend_from_slice(&code());

    let mut image = Vec::new();
    image.extend_from_slice(&[0x7F, b'E', b'L', b'F']);
    image.extend_from_slice(&[1, 1, 1]);
    image.extend_from_slice(&[0u8; 9]);
    image.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    image.extend_from_slice(&3u16.to_le_bytes()); // EM_386
    image.extend_from_slice(&1u32.to_le_bytes());
    image.extend_from_slice(&(ELSEWHERE + entry_in_payload).to_le_bytes());
    image.extend_from_slice(&EHSIZE.to_le_bytes());
    image.extend_from_slice(&0u32.to_le_bytes());
    image.extend_from_slice(&0u32.to_le_bytes());
    image.extend_from_slice(&(EHSIZE as u16).to_le_bytes());
    image.extend_from_slice(&(PHENTSIZE as u16).to_le_bytes());
    image.extend_from_slice(&1u16.to_le_bytes());
    image.extend_from_slice(&40u16.to_le_bytes());
    image.extend_from_slice(&0u16.to_le_bytes());
    image.extend_from_slice(&0u16.to_le_bytes());

    image.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    image.extend_from_slice(&payload_offset.to_le_bytes());
    image.extend_from_slice(&ELSEWHERE.to_le_bytes()); // p_vaddr
    image.extend_from_slice(&ELSEWHERE.to_le_bytes()); // p_paddr
    image.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    image.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    image.extend_from_slice(&5u32.to_le_bytes());
    image.extend_from_slice(&0x1000u32.to_le_bytes());

    image.extend_from_slice(&payload);
    image
}

/// Boot `image` and return what the guest wrote, or `None` if this host has no
/// backend that executes guest code.
async fn console_of(label: &str, image: Vec<u8>) -> Option<String> {
    let dir = std::env::temp_dir().join("hv2-multiboot-test");
    let path = dir.join(format!("{label}.mb"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(&path, &image).expect("write image");

    let config = VMConfig {
        name: format!("mb-{label}"),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(&path)),
        ..Default::default()
    };
    let vm = VM::new(config).ok()?;
    if !vm.executes_guest_code() {
        eprintln!("{label}: no hardware backend on this host; skipped");
        return None;
    }
    let vm = Arc::new(vm);

    vm.provision().await.expect("provision");
    vm.launch().await.expect("launch");

    let deadline = Instant::now() + BOUND;
    let mut console = String::new();
    while Instant::now() < deadline {
        console = vm.console_output().await;
        if console.len() >= GREETING.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    vm.stop().await.expect("stop");
    Some(console)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flat_multiboot_image_runs() {
    let Some(console) = console_of("flat", flat()).await else {
        return;
    };
    assert_eq!(console, GREETING);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_image_is_loaded_and_entered_where_its_header_says() {
    let Some(console) = console_of("address-fields", with_address_fields()).await else {
        return;
    };
    assert_eq!(
        console, GREETING,
        "the header asks to load at {ELSEWHERE:#x} and enter 32 bytes in; silence means \
         one or both were ignored"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_elf_kernel_runs_rather_than_executing_its_own_header() {
    let Some(console) = console_of("elf32", elf32()).await else {
        return;
    };
    assert_eq!(
        console, GREETING,
        "an ELF written verbatim to 1 MB is entered at 0x7f 'E' — `jns +0x45` — and \
         produces exactly this silence"
    );
}
