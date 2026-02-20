//! HyperMachine Type-1 Hypervisor Entry Point
//!
//! This is the bare-metal entry point for the Type-1 hypervisor.
//! It is loaded by the bootloader and initializes the hypervisor
//! on bare hardware.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

use bootloader_api::{BootInfo, BootloaderConfig, entry_point};
use core::panic::PanicInfo;
use hv1_core::{CpuVendor, HypervisorCapabilities, serial_println};
use linked_list_allocator::LockedHeap;

/// Global allocator for heap allocations
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Heap start address (16 MB)
const HEAP_START: usize = 0x1000000;
/// Heap size (16 MB)  
const HEAP_SIZE: usize = 0x1000000;

/// Bootloader configuration
static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

/// Kernel entry point called by bootloader
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // Initialize serial port for early debug output
    // SAFETY: Called once at boot before any other code uses the serial port.
    // No concurrent access is possible at this point.
    unsafe {
        hv1_core::serial::init_global_serial();
    }

    serial_println!(
        "HyperMachine Type-1 Hypervisor v{}",
        env!("CARGO_PKG_VERSION")
    );
    serial_println!("===========================================");

    // Initialize heap allocator
    // SAFETY: Called once at boot with a valid heap region. HEAP_START points
    // to an unused memory range and HEAP_SIZE does not overlap other mappings.
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }
    serial_println!(
        "Heap initialized: {} MB at {:#x}",
        HEAP_SIZE / 1024 / 1024,
        HEAP_START
    );

    // Print memory map
    serial_println!("\nMemory Map:");
    let memory_regions = &boot_info.memory_regions;
    let mut total_usable: u64 = 0;
    for region in memory_regions.iter() {
        let kind = match region.kind {
            bootloader_api::info::MemoryRegionKind::Usable => {
                total_usable += region.end - region.start;
                "Usable"
            }
            bootloader_api::info::MemoryRegionKind::Bootloader => "Bootloader",
            bootloader_api::info::MemoryRegionKind::UnknownUefi(_) => "UEFI Reserved",
            bootloader_api::info::MemoryRegionKind::UnknownBios(_) => "BIOS Reserved",
            _ => "Other",
        };
        serial_println!("  {:#016x} - {:#016x} ({})", region.start, region.end, kind);
    }
    serial_println!("Total usable memory: {} MB", total_usable / 1024 / 1024);

    // Print RSDP address for ACPI
    if let Some(rsdp_addr) = boot_info.rsdp_addr.into_option() {
        serial_println!("\nRSDP Address: {:#x}", rsdp_addr);
    }

    // Detect CPU capabilities
    serial_println!("\nDetecting CPU capabilities...");
    let caps = HypervisorCapabilities::detect();
    serial_println!("  CPU Vendor: {:?}", caps.vendor);
    serial_println!("  VMX Support: {}", caps.vmx_supported);
    serial_println!("  SVM Support: {}", caps.svm_supported);
    serial_println!("  EPT Support: {}", caps.ept_supported);
    serial_println!("  NPT Support: {}", caps.npt_supported);

    // Initialize the hypervisor
    serial_println!("\nInitializing hypervisor...");

    match hv1_core::initialize() {
        Ok(()) => {
            serial_println!("Hypervisor initialized successfully!");

            // Enter hypervisor mode
            enter_hypervisor_mode(caps.vendor);
        }
        Err(e) => {
            serial_println!("Failed to initialize hypervisor: {}", e);
            panic!("Hypervisor initialization failed");
        }
    }

    // Should never reach here
    hlt_loop();
}

/// Enter hypervisor mode based on CPU vendor
fn enter_hypervisor_mode(vendor: CpuVendor) {
    serial_println!("\nEntering hypervisor mode...");

    match vendor {
        CpuVendor::Intel => {
            serial_println!("Using Intel VMX");
            #[cfg(feature = "intel")]
            {
                // VMX initialization is done in hv1_core::initialize()
                serial_println!("VMX enabled, hypervisor is now active");
            }
        }
        CpuVendor::Amd => {
            serial_println!("Using AMD-V SVM");
            #[cfg(feature = "amd")]
            {
                // SVM initialization is done in hv1_core::initialize()
                serial_println!("SVM enabled, hypervisor is now active");
            }
        }
        CpuVendor::Unknown => {
            serial_println!("Unknown CPU vendor, cannot enter hypervisor mode");
            panic!("Unknown CPU vendor");
        }
    }

    serial_println!("\n===========================================");
    serial_println!("HyperMachine Type-1 Hypervisor Active");
    serial_println!("Ready to create and manage virtual machines");
    serial_println!("===========================================");
}

/// Halt loop for when we have nothing else to do
fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

/// Panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("\n*** KERNEL PANIC ***");
    serial_println!("{}", info);
    hlt_loop();
}
