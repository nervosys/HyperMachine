//! HyperMachine Type-1 (HV1) bare-metal hypervisor kernel entry point
//!
//! This binary is only built when the `bootloader_api` feature is enabled.
//! It provides the bare-metal kernel entry point for the Type-1 hypervisor.

#![no_std]
#![no_main]

#[cfg(feature = "bootloader_api")]
use bootloader_api::{entry_point, BootInfo};

#[cfg(feature = "bootloader_api")]
entry_point!(kernel_main);

#[cfg(feature = "bootloader_api")]
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // Initialize the hypervisor (initialize() is the public entry point in lib.rs)
    let _ = hv1_core::initialize();
    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
