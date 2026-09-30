//! Boot a Linux guest and hand its serial console to this terminal: a REPL
//! inside a HyperMachine VM.
//!
//! Lines typed here are delivered to COM1 through `console_input`; what the
//! guest writes to COM1 is printed as it arrives. It is line-buffered (the
//! terminal sends a line on Enter), so it suits a shell, not a full-screen
//! program.
//!
//! ```sh
//! HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run -p hv2-core --example serial_repl -- /var/tmp/kbuild/bzImage
//! ```
//!
//! Needs `/dev/kvm` (or WHPX/HVF), a kernel with `CONFIG_SERIAL_8250_CONSOLE=y`,
//! and an initramfs whose init leaves a shell on `/dev/console`. End of input
//! (Ctrl-D) stops the VM. `HV2_BOOT_LOG=1` shows the kernel's boot messages.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

use hv2_core::machine::Machine;
use hv2_core::{BootSource, VMConfig, VM};

/// COM1, where `console=ttyS0` sends everything.
const COM1: u16 = 0x3F8;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("serial_repl: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let kernel = std::env::args()
        .nth(1)
        .ok_or("usage: serial_repl <bzImage>  (set HV2_INITRD to an initramfs)")?;
    let boot_log = std::env::var("HV2_BOOT_LOG").is_ok();
    let cmdline = std::env::var("HV2_CMDLINE").unwrap_or_else(|_| {
        let level = if boot_log { "" } else { " quiet loglevel=0" };
        format!("console=ttyS0,115200 panic=1{level}")
    });

    let mut source = BootSource::linux(&kernel).with_cmdline(cmdline);
    if let Ok(initrd) = std::env::var("HV2_INITRD") {
        source = source.with_initrd(initrd);
    }
    let config = VMConfig {
        name: "serial-repl".to_string(),
        vcpu_count: 1,
        memory_size: std::env::var("HV2_MEMORY_MIB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1024)
            * 1024
            * 1024,
        boot: Some(source),
        ..Default::default()
    };

    let vm = Arc::new(VM::new(config).map_err(|e| format!("VM::new: {e}"))?);
    Machine::legacy_pc()
        .attach(&vm.devices())
        .await
        .map_err(|e| format!("devices: {e}"))?;
    vm.provision()
        .await
        .map_err(|e| format!("provision: {e}"))?;
    vm.launch().await.map_err(|e| format!("launch: {e}"))?;
    eprintln!("[hypermachine] guest launched; its serial console follows (Ctrl-D stops the VM)");

    // Stdin is blocking, so it is read on its own thread and handed over.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(Some(line)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(None);
    });

    // `console_output` returns everything written so far without consuming
    // it, so print only what is new since the last look.
    let mut shown = 0usize;
    let mut out = std::io::stdout();
    loop {
        let console = vm.console_output().await;
        if console.len() > shown {
            let fresh = &console[shown..];
            // Terminal status queries the shell sends (ESC[6n) would print as
            // noise; they have no terminal to answer them here.
            let _ = out.write_all(fresh.replace("\x1b[6n", "").as_bytes());
            let _ = out.flush();
            shown = console.len();
        }
        match rx.try_recv() {
            Ok(Some(line)) => {
                let mut typed = line.into_bytes();
                typed.push(b'\n');
                let device = vm
                    .devices()
                    .find_io_device(COM1)
                    .await
                    .ok_or("COM1 is not registered")?;
                device
                    .console_input(&typed)
                    .await
                    .map_err(|e| format!("console input: {e}"))?;
            }
            Ok(None) | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Let the last answer drain before stopping.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let console = vm.console_output().await;
    if console.len() > shown {
        let _ = out.write_all(console[shown..].replace("\x1b[6n", "").as_bytes());
        let _ = out.flush();
    }
    let _ = vm.stop().await;
    eprintln!("\n[hypermachine] VM stopped");
    Ok(())
}
