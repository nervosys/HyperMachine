//! Reading a guest's console from the outside.
//!
//! Nothing registers a serial device automatically, so a VM's console is empty
//! until a caller attaches one. These tests pin the two things that matters
//! for: the output reaches the host through the device manager, and reading it
//! does not drain the buffer out from under whatever else is watching.

use std::sync::Arc;

use hv2_core::{Device, SerialDevice, VMConfig, VM};
use tokio::sync::RwLock;

/// Build a VM, or skip when this machine has no usable hypervisor backend.
async fn vm_or_skip() -> Option<Arc<VM>> {
    match VM::new(VMConfig::default()) {
        Ok(vm) => Some(Arc::new(vm)),
        Err(e) => {
            eprintln!("skipping: no hypervisor backend available ({e})");
            None
        }
    }
}

/// Attach a console to `vm` and have the "guest" write `text` through it.
async fn attach_console(vm: &VM, name: &str, text: &str) {
    let device = Arc::new(RwLock::new(SerialDevice::new(name.to_string(), 0x3F8)));
    {
        let mut guard = device.write().await;
        for byte in text.as_bytes() {
            guard.write(0, &[*byte]).await.unwrap();
        }
    }
    vm.devices().register_device(name, device).await.unwrap();
}

#[tokio::test]
async fn a_vm_with_no_console_device_reports_no_output() {
    let Some(vm) = vm_or_skip().await else {
        return;
    };

    assert!(vm.console_output().await.is_empty());
    assert!(
        vm.console_output_by_device().await.is_empty(),
        "an empty device list is how a caller learns nothing is attached"
    );
}

#[tokio::test]
async fn console_output_reaches_the_host() {
    let Some(vm) = vm_or_skip().await else {
        return;
    };
    attach_console(&vm, "COM1", "booting...\n").await;

    assert_eq!(vm.console_output().await, "booting...\n");
}

#[tokio::test]
async fn polling_the_console_does_not_drain_it() {
    let Some(vm) = vm_or_skip().await else {
        return;
    };
    attach_console(&vm, "COM1", "line one\n").await;

    let first = vm.console_output().await;
    let second = vm.console_output().await;

    assert_eq!(first, "line one\n");
    assert_eq!(
        second, first,
        "a status poll must not eat the boot log it is reporting"
    );
}

#[tokio::test]
async fn two_consoles_are_reported_separately_and_in_order() {
    let Some(vm) = vm_or_skip().await else {
        return;
    };
    attach_console(&vm, "COM2", "aux").await;
    attach_console(&vm, "COM1", "main").await;

    let per_device = vm.console_output_by_device().await;
    assert_eq!(
        per_device
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["COM1", "COM2"]
    );

    // The concatenation follows the same order, so it is stable across calls.
    assert_eq!(vm.console_output().await, "mainaux");
}

#[tokio::test]
async fn non_utf8_guest_output_is_decoded_lossily_rather_than_lost() {
    let Some(vm) = vm_or_skip().await else {
        return;
    };
    let device = Arc::new(RwLock::new(SerialDevice::new("COM1".to_string(), 0x3F8)));
    {
        let mut guard = device.write().await;
        for byte in [b'o', b'k', 0xFF] {
            guard.write(0, &[byte]).await.unwrap();
        }
    }
    vm.devices().register_device("COM1", device).await.unwrap();

    // A guest is free to write bytes that are not text. The raw form keeps
    // them; the string form must still return something rather than failing.
    assert_eq!(
        vm.console_output_by_device().await[0].1,
        vec![b'o', b'k', 0xFF]
    );
    assert!(vm.console_output().await.starts_with("ok"));
}
