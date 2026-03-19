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
use hv1_core::boot;
use hv1_core::device::DeviceManager;
use hv1_core::vm::{DefaultExitHandler, Vm, VmConfig, VmExitAction};
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
        }
        Err(e) => {
            serial_println!("Failed to initialize hypervisor: {}", e);
            panic!("Hypervisor initialization failed");
        }
    }

    // Convert bootloader boot info → hv1-core BootInfo and do late init
    let our_boot_info = convert_boot_info(boot_info);
    match boot::late_init(&our_boot_info) {
        Ok(()) => serial_println!("Late init complete (APIC, ACPI, SMP)."),
        Err(e) => serial_println!("Late init warning: {}", e),
    }

    // Report detected APs
    let ap_count = unsafe { boot::ap_count() };
    serial_println!("Detected {} application processor(s)", ap_count);

    // Enter hypervisor mode — create and run the first guest VM
    enter_hypervisor_mode(caps.vendor, &our_boot_info);

    // Should never reach here
    hlt_loop();
}

/// Convert bootloader_api BootInfo → hv1_core::boot::BootInfo
fn convert_boot_info(bi: &BootInfo) -> hv1_core::boot::BootInfo {
    let mut memory_map = hv1_core::boot::MemoryMap::new();

    for region in bi.memory_regions.iter() {
        let kind = match region.kind {
            bootloader_api::info::MemoryRegionKind::Usable => {
                hv1_core::boot::MemoryRegionType::Usable
            }
            bootloader_api::info::MemoryRegionKind::Bootloader => {
                hv1_core::boot::MemoryRegionType::BootloaderReclaimable
            }
            _ => hv1_core::boot::MemoryRegionType::Reserved,
        };
        let _ = memory_map.add_region(hv1_core::boot::MemoryRegion {
            start: region.start,
            size: region.end - region.start,
            kind,
        });
    }

    let framebuffer = bi
        .framebuffer
        .as_ref()
        .map(|fb| hv1_core::boot::FramebufferInfo {
            address: fb.buffer().as_ptr() as u64,
            width: fb.info().width as u32,
            height: fb.info().height as u32,
            pitch: fb.info().stride as u32 * fb.info().bytes_per_pixel as u32,
            bpp: (fb.info().bytes_per_pixel * 8) as u8,
            format: match fb.info().pixel_format {
                bootloader_api::info::PixelFormat::Rgb => hv1_core::boot::PixelFormat::Rgb,
                bootloader_api::info::PixelFormat::Bgr => hv1_core::boot::PixelFormat::Bgr,
                _ => hv1_core::boot::PixelFormat::Unknown,
            },
        });

    hv1_core::boot::BootInfo {
        memory_map,
        framebuffer,
        rsdp_addr: bi.rsdp_addr.into_option(),
        kernel_addr: 0,
        kernel_size: 0,
    }
}

/// Enter hypervisor mode: create a guest VM with identity-mapped memory,
/// register standard platform devices, and enter the VM-exit handling loop.
fn enter_hypervisor_mode(vendor: CpuVendor, boot_info: &hv1_core::boot::BootInfo) {
    serial_println!("\nEntering hypervisor mode...");

    match vendor {
        CpuVendor::Intel => serial_println!("Using Intel VMX"),
        CpuVendor::Amd => serial_println!("Using AMD-V SVM"),
        CpuVendor::Unknown => {
            serial_println!("Unknown CPU vendor, cannot enter hypervisor mode");
            panic!("Unknown CPU vendor");
        }
    }

    // Create VM configuration (1 vCPU, use all usable memory)
    let guest_mem_size = boot_info.memory_map.total_usable_memory();
    let config = VmConfig::new(1, guest_mem_size);
    serial_println!(
        "Creating VM: 1 vCPU, {} MB guest memory",
        guest_mem_size / 1024 / 1024
    );

    let mut vm = match Vm::new(vendor, config) {
        Ok(vm) => vm,
        Err(e) => {
            serial_println!("Failed to create VM: {}", e);
            return;
        }
    };

    // Initialise the VM's frame allocator from boot-time region
    let (fa_start, fa_end) = unsafe { boot::boot_frame_allocator_region() };
    if fa_start != 0 && fa_end > fa_start {
        vm.init_frame_allocator(fa_start, fa_end);
        serial_println!(
            "Frame allocator: {:#x} - {:#x} ({} MB)",
            fa_start,
            fa_end,
            (fa_end - fa_start) / 1024 / 1024
        );
    }

    // Identity-map all usable memory regions into the guest
    for region in boot_info.memory_map.iter() {
        if region.kind == hv1_core::boot::MemoryRegionType::Usable {
            let _ = vm.map_memory(region.start, region.start, region.size);
        }
    }

    // Register standard platform devices
    *vm.device_manager_mut() = DeviceManager::with_default_devices();
    serial_println!(
        "Registered {} platform devices",
        vm.device_manager().device_count()
    );

    // Initialise (creates vCPUs, builds EPT/NPT, configures VMCS/VMCB)
    match vm.initialize() {
        Ok(()) => serial_println!("VM initialized successfully"),
        Err(e) => {
            serial_println!("VM initialization failed: {}", e);
            return;
        }
    }

    // Set up BSP vCPU in real-mode (reset vector)
    if let Some(vcpu) = vm.vcpu_mut(0) {
        vcpu.setup_real_mode();
        serial_println!("vCPU 0 configured for real-mode boot");
    }

    // Start the VM
    match vm.start() {
        Ok(()) => serial_println!("VM started"),
        Err(e) => {
            serial_println!("Failed to start VM: {}", e);
            return;
        }
    }

    serial_println!("\n===========================================");
    serial_println!("HyperMachine Type-1 Hypervisor Active");
    serial_println!("Entering VM run loop...");
    serial_println!("===========================================");

    // Enter the run loop
    let mut handler = DefaultExitHandler;
    let result = unsafe { vm.run_vcpu(0, &mut handler) };

    match result {
        Ok(VmExitAction::Halt) => serial_println!("VM halted normally"),
        Ok(VmExitAction::Shutdown) => serial_println!("VM shutdown requested"),
        Ok(VmExitAction::Error) => serial_println!("VM exited with unhandled exit"),
        Ok(VmExitAction::Continue) => serial_println!("VM run loop ended unexpectedly"),
        Err(e) => serial_println!("VM run error: {}", e),
    }

    serial_println!(
        "Exit count: {}",
        vm.vcpu(0).map(|v| v.exit_count()).unwrap_or(0)
    );
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
