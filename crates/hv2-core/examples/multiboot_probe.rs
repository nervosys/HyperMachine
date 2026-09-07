//! Can a Multiboot guest say anything, and does it depend on how it was built?
//!
//! `BootSource::Multiboot` is the entry a compiled unikernel needs: flat
//! 32-bit protected mode, paging off, `EAX` holding the bootloader magic and
//! `EBX` the info structure. `BootSource::Raw` only ever gives 16-bit real
//! mode, which is why every guest that has run in this repository so far has
//! been hand-assembled 16-bit code. Multiboot is the gate on a real `no_std`
//! Rust unikernel, and so on an actual agent payload.
//!
//! The reported symptom was that it launches and produces nothing. That is
//! true of some images and not others, and which ones is the whole point, so
//! this runs the same guest program built three ways:
//!
//! | Shape | How it says where it goes |
//! |---|---|
//! | flat | it does not — the loader's convention puts it at 1 MB |
//! | address fields | the header's own `load_addr` / `entry_addr` (flags bit 16) |
//! | ELF32 | its program headers and `e_entry` |
//!
//! Every one is assembled in-process, so there is no missing-asset failure
//! mode and the reader can check the encoding rather than trust the author.
//! The two that carry addresses deliberately do *not* load at 1 MB and do not
//! enter at their first byte, because an image that agrees with the
//! convention cannot tell you whether the convention was all that was
//! consulted.
//!
//! # What each guest does
//!
//! Eleven bytes of 32-bit code, the same in all three:
//!
//! ```text
//!   BA F8 03 00 00     mov edx, 0x3F8   COM1, the port Machine::legacy_pc maps
//!   B0 4D              mov al, 'M'
//!   EE                 out dx, al       -> the emulated 16550 in this process
//!   ...                                 one pair per character
//!   F4                 hlt
//! ```
//!
//! Every `out` leaves the guest, is decoded here, and lands in a device model,
//! so a byte on the console proves the image was loaded at the right address,
//! entered at the right instruction, in the right CPU mode. This reports the
//! console for that reason, and single-steps anything silent rather than
//! leaving you to guess between "faulted early" and "looping".
//!
//! ```text
//! cargo run --release -p hv2-core --example multiboot_probe
//! ```

use hv2_core::{BootSource, VMConfig, VM};
use std::sync::Arc;
use std::time::Duration;

/// COM1's data port, which `Machine::legacy_pc` maps to a 16550.
const COM1: u16 = 0x3F8;

/// The magic a Multiboot 1.0 header starts with.
const MULTIBOOT_MAGIC: u32 = 0x1BAD_B002;

/// Flags bit 16: the header carries its own load addresses.
const AOUT_KLUDGE: u32 = 1 << 16;

/// Where the two address-carrying images ask to be loaded. Not 1 MB, so that
/// honouring the convention instead of the image is a visible failure rather
/// than an invisible one.
const ELSEWHERE: u32 = 0x0030_0000;

/// The 32-bit payload: write `text` to COM1, then halt.
fn code(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0xBA); // mov edx, imm32
    out.extend_from_slice(&u32::from(COM1).to_le_bytes());
    for byte in text.bytes() {
        out.push(0xB0); // mov al, imm8
        out.push(byte);
        out.push(0xEE); // out dx, al
    }
    out.push(0xF4); // hlt
    out
}

/// The 12-byte header every Multiboot image carries, plus optional addresses.
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

/// A flat image: no addresses, so the loader's convention decides. Opens with
/// a jump over the header, because the entry is the first byte of the file.
fn flat(text: &str) -> Vec<u8> {
    let mut image = vec![0xEB, 0x0E, 0x90, 0x90]; // jmp +14 -> 0x10; align to 4
    image.extend_from_slice(&header(0, &[]));
    assert_eq!(
        image.len(),
        0x10,
        "the code must start where the jump lands"
    );
    image.extend_from_slice(&code(text));
    image
}

/// An image whose header says where it goes: loaded at `ELSEWHERE` and entered
/// after its own 32-byte header, so neither the load address nor the entry
/// matches what the flat convention would pick.
fn with_address_fields(text: &str) -> Vec<u8> {
    let mut image = header(
        AOUT_KLUDGE,
        &[
            ELSEWHERE,      // header_addr — the header is at file offset 0
            ELSEWHERE,      // load_addr
            0,              // load_end_addr — to the end of the file
            0,              // bss_end_addr — no .bss
            ELSEWHERE + 32, // entry_addr — immediately after the header
        ],
    );
    assert_eq!(image.len(), 32, "a header with addresses is 32 bytes");
    image.extend_from_slice(&code(text));
    image
}

/// An ELF32 with one `PT_LOAD` segment. This is the shape a linker produces,
/// and the shape that used to be written to 1 MB verbatim and entered at
/// `\x7fELF`.
fn elf32(text: &str) -> Vec<u8> {
    const EHSIZE: u32 = 52;
    const PHENTSIZE: u32 = 32;
    let payload_offset = EHSIZE + PHENTSIZE;

    // The segment: a bare Multiboot header, then the code. No jump over it,
    // because `e_entry` points past it — which is the thing under test.
    let mut payload = header(0, &[]);
    let entry_in_payload = payload.len() as u32;
    payload.extend_from_slice(&code(text));

    let mut image = Vec::new();
    image.extend_from_slice(&[0x7F, b'E', b'L', b'F']);
    image.extend_from_slice(&[1, 1, 1]); // ELFCLASS32, ELFDATA2LSB, version
    image.extend_from_slice(&[0u8; 9]); // OS ABI and padding
    image.extend_from_slice(&2u16.to_le_bytes()); // e_type    = ET_EXEC
    image.extend_from_slice(&3u16.to_le_bytes()); // e_machine = EM_386
    image.extend_from_slice(&1u32.to_le_bytes()); // e_version
    image.extend_from_slice(&(ELSEWHERE + entry_in_payload).to_le_bytes()); // e_entry
    image.extend_from_slice(&EHSIZE.to_le_bytes()); // e_phoff
    image.extend_from_slice(&0u32.to_le_bytes()); // e_shoff
    image.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    image.extend_from_slice(&(EHSIZE as u16).to_le_bytes()); // e_ehsize
    image.extend_from_slice(&(PHENTSIZE as u16).to_le_bytes()); // e_phentsize
    image.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
    image.extend_from_slice(&40u16.to_le_bytes()); // e_shentsize
    image.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    image.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx
    assert_eq!(image.len(), EHSIZE as usize);

    image.extend_from_slice(&1u32.to_le_bytes()); // p_type   = PT_LOAD
    image.extend_from_slice(&payload_offset.to_le_bytes()); // p_offset
    image.extend_from_slice(&ELSEWHERE.to_le_bytes()); // p_vaddr
    image.extend_from_slice(&ELSEWHERE.to_le_bytes()); // p_paddr
    image.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // p_filesz
    image.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // p_memsz
    image.extend_from_slice(&5u32.to_le_bytes()); // p_flags  = R+X
    image.extend_from_slice(&0x1000u32.to_le_bytes()); // p_align
    assert_eq!(image.len(), payload_offset as usize);

    image.extend_from_slice(&payload);
    image
}

fn vm_with(name: &str, path: &std::path::Path) -> Option<Arc<VM>> {
    let config = VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(path)),
        ..Default::default()
    };
    match VM::new(config) {
        Ok(vm) => Some(Arc::new(vm)),
        Err(e) => {
            eprintln!("VM::new: FAILED — {e}");
            None
        }
    }
}

/// Boot one image and report what came out of it. `true` if the guest spoke.
async fn probe(label: &str, expect: &str, image: Vec<u8>) -> bool {
    let dir = std::env::temp_dir().join("hv2-multiboot");
    let path = dir.join(format!("{label}.mb"));
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, &image).is_err() {
        println!("{label:<16}: could not write the image");
        return false;
    }

    let Some(vm) = vm_with(&format!("mb-{label}"), &path) else {
        return false;
    };
    if let Err(e) = vm.provision().await {
        println!("{label:<16}: provision failed — {e}");
        return false;
    }
    if let Err(e) = vm.launch().await {
        println!("{label:<16}: launch failed — {e}");
        return false;
    }

    // Poll rather than sleep a fixed time, so a guest that works is not
    // reported slowly and one that does not is not reported early.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut console = String::new();
    while std::time::Instant::now() < deadline {
        console = vm.console_output().await;
        if console.len() >= expect.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let _ = vm.stop().await;

    let ok = console == expect;
    println!(
        "{label:<16}: {:>3} bytes, console {console:?}{}",
        image.len(),
        if ok { "" } else { "   ← expected " },
    );
    if !ok {
        println!("{:<16}  {expect:?}", "");
    }

    if console.is_empty() {
        // A silent guest has either faulted before it could speak or is looping
        // without exiting, and neither says anything on its own. Step it on a
        // second VM, because tracing drives the vCPU directly.
        if let Some(vm) = vm_with(&format!("mb-{label}-trace"), &path) {
            if vm.provision().await.is_ok() {
                match vm.single_step_trace(16).await {
                    Ok(trace) => {
                        let tail: Vec<String> =
                            trace.tail.iter().map(|a| format!("{a:#x}")).collect();
                        println!(
                            "{:<16}  stepped {} to {:?}, via {}",
                            "",
                            trace.steps,
                            trace.final_exit,
                            tail.join(" ")
                        );
                    }
                    Err(e) => println!("{:<16}  trace unavailable — {e}", ""),
                }
            }
        }
    }

    ok
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let text = "MULTIBOOT\n";
    println!("guest         : writes {text:?} to COM1 and halts, built three ways");
    println!("addresses     : flat → 0x100000 by convention; the other two → {ELSEWHERE:#x}");
    println!();

    let mut all = true;
    all &= probe("flat", text, flat(text)).await;
    all &= probe("address-fields", text, with_address_fields(text)).await;
    all &= probe("elf32", text, elf32(text)).await;

    println!();
    if all {
        println!("every shape executed, including the two that load where the image asked.");
        std::process::ExitCode::SUCCESS
    } else {
        println!("at least one shape did not run. The console and the trace say which.");
        std::process::ExitCode::FAILURE
    }
}
