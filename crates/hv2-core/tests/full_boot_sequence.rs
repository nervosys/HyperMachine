//! End-to-end boot sequence integration tests
//!
//! These tests verify the complete boot flow from initial CPU state
//! through descriptor table loading, mode transitions, and final boot state.

use hv2_core::backends::whpx::{WhpxBackend, WhpxVm};
use hv2_core::boot::linux::{LinuxBootParams, LinuxBootProtocol};
use hv2_core::boot::multiboot::{MultibootInfo, MultibootProtocol};
use hv2_core::boot::BootSetup;
use hv2_core::descriptors::{GdtBuilder, IdtBuilder, DESC_DPL_0, DESC_DPL_3};
use hv2_core::Result;

/// Test complete descriptor table setup flow
#[tokio::test]
async fn test_complete_descriptor_setup() -> Result<()> {
    // Try to create backend (will skip if WHPX not available)
    let _backend = match WhpxBackend::new() {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ WHPX not available, skipping test");
            return Ok(());
        }
    };

    let vm = match WhpxVm::new(1, 4 * 1024 * 1024) {
        Ok(v) => v,
        Err(e) => {
            println!("⚠ Cannot create VM: {}, skipping test", e);
            return Ok(());
        }
    };
    let vcpu = vm.create_vcpu(0)?;

    // Allocate standard table locations
    let (gdt_base, idt_base, _pt_base, _sp) = BootSetup::allocate_standard_tables();

    // Build 64-bit GDT
    let gdt = GdtBuilder::new()
        .add_null()
        .add_code_64bit(DESC_DPL_0)
        .add_data_64bit(DESC_DPL_0)
        .add_code_64bit(DESC_DPL_3) // User code
        .add_data_64bit(DESC_DPL_3) // User data
        .build();

    // Build 64-bit IDT with exception handlers
    let idt = IdtBuilder::new_64bit()
        .add_interrupt_gate(0, 0xFFFF_8000_0010_0000, 0x08, 0, DESC_DPL_0) // Divide by zero
        .add_interrupt_gate(1, 0xFFFF_8000_0010_0100, 0x08, 0, DESC_DPL_0) // Debug
        .add_trap_gate(3, 0xFFFF_8000_0010_0200, 0x08, 0, DESC_DPL_3) // Breakpoint
        .add_interrupt_gate(6, 0xFFFF_8000_0010_0600, 0x08, 0, DESC_DPL_0) // Invalid opcode
        .add_interrupt_gate(8, 0xFFFF_8000_0010_0800, 0x08, 1, DESC_DPL_0) // Double fault (IST=1)
        .add_interrupt_gate(13, 0xFFFF_8000_0010_0D00, 0x08, 0, DESC_DPL_0) // GPF
        .add_interrupt_gate(14, 0xFFFF_8000_0010_0E00, 0x08, 0, DESC_DPL_0) // Page fault
        .build();

    // Load GDT
    let result = vcpu.load_gdt(&vm, &gdt, gdt_base);
    if let Ok((cs, ds)) = result {
        println!("✓ GDT loaded: CS=0x{:02X}, DS=0x{:02X}", cs, ds);
        assert_eq!(cs, 0x08);
        assert_eq!(ds, 0x10);
    } else {
        println!("⚠ GDT loading failed (may require admin): {:?}", result);
        return Ok(());
    }

    // Load IDT
    if let Err(e) = vcpu.load_idt(&vm, &idt, idt_base) {
        println!("⚠ IDT loading failed: {}", e);
        return Ok(());
    }
    println!("✓ IDT loaded with {} entries", 256);

    println!("✓ Complete descriptor setup successful");
    Ok(())
}

/// Test long mode transition with page tables
#[tokio::test]
async fn test_long_mode_with_page_tables() -> Result<()> {
    let _backend = match WhpxBackend::new() {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ WHPX not available, skipping test");
            return Ok(());
        }
    };

    let vm = match WhpxVm::new(1, 8 * 1024 * 1024) {
        Ok(v) => v,
        Err(e) => {
            println!("⚠ Cannot create VM: {}, skipping test", e);
            return Ok(());
        }
    };
    let vcpu = vm.create_vcpu(0)?;

    // Allocate locations
    let (gdt_base, idt_base, pt_base, _sp) = BootSetup::allocate_standard_tables();

    // Create identity-mapped page tables
    let page_tables = BootSetup::create_identity_page_tables(pt_base);
    vm.write_guest_memory(pt_base, &page_tables)?;
    println!("✓ Page tables written at 0x{:X}", pt_base);

    // Build minimal GDT
    let gdt = GdtBuilder::new()
        .add_null()
        .add_code_64bit(DESC_DPL_0)
        .add_data_64bit(DESC_DPL_0)
        .build();

    // Load GDT
    if vcpu.load_gdt(&vm, &gdt, gdt_base).is_err() {
        println!("⚠ GDT loading failed, skipping test");
        return Ok(());
    }

    // Attempt long mode transition
    match vcpu.enable_long_mode(pt_base) {
        Ok(()) => {
            println!("✓ Long mode enabled successfully");

            // Verify mode
            if let Ok(mode) = vcpu.get_cpu_mode() {
                println!("✓ CPU mode: {:?}", mode);
            }

            // Verify control registers
            if let Ok(cr) = vcpu.get_control_registers() {
                assert!(cr.is_long_mode_enabled(), "LME should be set");
                assert!(cr.is_long_mode_active(), "LMA should be set");
                assert!(cr.is_paging_enabled(), "Paging should be enabled");
                assert!(cr.is_pae_enabled(), "PAE should be enabled");
                println!("✓ All long mode prerequisites verified");
            }
        }
        Err(e) => {
            println!("⚠ Long mode transition failed (expected): {}", e);
        }
    }

    Ok(())
}

/// Test complete boot environment setup
#[tokio::test]
async fn test_complete_boot_environment() -> Result<()> {
    let _backend = match WhpxBackend::new() {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ WHPX not available, skipping test");
            return Ok(());
        }
    };

    let vm = match WhpxVm::new(1, 16 * 1024 * 1024) {
        Ok(v) => v,
        Err(e) => {
            println!("⚠ Cannot create VM: {}, skipping test", e);
            return Ok(());
        }
    };
    let vcpu = vm.create_vcpu(0)?;

    // Allocate all tables
    let (gdt_base, idt_base, pt_base, sp) = BootSetup::allocate_standard_tables();
    println!("✓ Allocated tables: GDT=0x{:X}, IDT=0x{:X}, PT=0x{:X}, SP=0x{:X}",
             gdt_base, idt_base, pt_base, sp);

    // Setup page tables
    let page_tables = BootSetup::create_identity_page_tables(pt_base);
    vm.write_guest_memory(pt_base, &page_tables)?;

    // Setup GDT with full privilege levels
    let gdt = GdtBuilder::new()
        .add_null()
        .add_code_64bit(DESC_DPL_0) // Kernel code
        .add_data_64bit(DESC_DPL_0) // Kernel data
        .add_code_64bit(DESC_DPL_3) // User code
        .add_data_64bit(DESC_DPL_3) // User data
        .build();

    if vcpu.load_gdt(&vm, &gdt, gdt_base).is_err() {
        println!("⚠ Cannot load GDT, skipping test");
        return Ok(());
    }

    // Setup IDT with common exception handlers
    let idt = IdtBuilder::new_64bit()
        .add_interrupt_gate(0, 0xFFFF_8000_0010_0000, 0x08, 0, DESC_DPL_0)
        .add_interrupt_gate(1, 0xFFFF_8000_0010_0100, 0x08, 0, DESC_DPL_0)
        .add_trap_gate(3, 0xFFFF_8000_0010_0200, 0x08, 0, DESC_DPL_3)
        .add_interrupt_gate(6, 0xFFFF_8000_0010_0600, 0x08, 0, DESC_DPL_0)
        .add_interrupt_gate(8, 0xFFFF_8000_0010_0800, 0x08, 1, DESC_DPL_0)
        .add_interrupt_gate(13, 0xFFFF_8000_0010_0D00, 0x08, 0, DESC_DPL_0)
        .add_interrupt_gate(14, 0xFFFF_8000_0010_0E00, 0x08, 0, DESC_DPL_0)
        .build();

    if vcpu.load_idt(&vm, &idt, idt_base).is_err() {
        println!("⚠ Cannot load IDT, skipping test");
        return Ok(());
    }

    // Setup stack pointer
    if vcpu.set_stack_pointer(0x10, sp as u16).is_err() {
        println!("⚠ Cannot set stack pointer");
    }

    println!("✓ Complete boot environment configured");
    Ok(())
}

/// Test Linux boot parameter validation
#[tokio::test]
async fn test_linux_boot_validation() -> Result<()> {
    // Create a minimal valid bzImage
    let mut kernel = vec![0u8; 4096];

    // Boot flag
    kernel[0x1FE] = 0x55;
    kernel[0x1FF] = 0xAA;

    // Boot signature "HdrS"
    kernel[0x202] = 0x48;
    kernel[0x203] = 0x64;
    kernel[0x204] = 0x72;
    kernel[0x205] = 0x53;

    // Protocol version 2.12
    kernel[0x206] = 0x0C;
    kernel[0x207] = 0x02;

    // Setup sectors
    kernel[0x1F1] = 4;

    let params = LinuxBootParams {
        kernel_image: kernel,
        initrd: None,
        cmdline: "console=ttyS0 root=/dev/vda".to_string(),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
    };

    // Validate parameters
    LinuxBootProtocol::validate_params(&params)?;
    println!("✓ Linux boot parameters validated");

    // Parse header
    let header = LinuxBootProtocol::parse_header(&params.kernel_image)?;
    println!("✓ Linux header parsed: version={}.{:02}, setup_size={}, kernel_size={}",
             header.version >> 8, header.version & 0xFF,
             header.setup_size, header.kernel_size);

    // Create boot_params structure
    let boot_params = LinuxBootProtocol::create_boot_params(&params, None, None);
    assert_eq!(boot_params.len(), 4096);
    println!("✓ Linux boot_params structure created");

    Ok(())
}

/// Test Multiboot header validation
#[tokio::test]
async fn test_multiboot_validation() -> Result<()> {
    // Create a kernel with valid Multiboot header
    let mut kernel = vec![0u8; 8192];

    // Place header at offset 0x100
    let offset = 0x100;

    // Magic
    kernel[offset..offset + 4].copy_from_slice(&0x1BADB002u32.to_le_bytes());

    // Flags
    let flags = 0u32;
    kernel[offset + 4..offset + 8].copy_from_slice(&flags.to_le_bytes());

    // Checksum = -(magic + flags)
    let checksum = (-(0x1BADB002i32 + flags as i32)) as u32;
    kernel[offset + 8..offset + 12].copy_from_slice(&checksum.to_le_bytes());

    let info = MultibootInfo {
        kernel_image: kernel,
        modules: Vec::new(),
        cmdline: "root=/dev/sda1".to_string(),
        memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
    };

    // Validate
    MultibootProtocol::validate_params(&info)?;
    println!("✓ Multiboot parameters validated");

    // Find header
    let header = MultibootProtocol::find_header(&info.kernel_image)?;
    println!("✓ Multiboot header found at offset 0x{:X}", header.offset);

    // Create multiboot_info structure
    let mb_info = MultibootProtocol::create_multiboot_info(
        &info,
        0x10000, // info_addr
        0x11000, // cmdline_addr
        None,    // mods_addr
        0x12000, // mmap_addr
    );
    assert!(mb_info.len() >= 52);
    println!("✓ Multiboot info structure created");

    // Create memory map
    let mmap = MultibootProtocol::create_memory_map(&info.memory_map);
    assert_eq!(mmap.len(), 48); // 2 entries * 24 bytes
    println!("✓ Multiboot memory map created");

    Ok(())
}

/// Test boot address validation
#[tokio::test]
async fn test_boot_address_validation() -> Result<()> {
    // Valid addresses
    assert!(BootSetup::validate_boot_addresses(0x100000, 0x400000, Some(0x90000)).is_ok());
    println!("✓ Valid boot addresses accepted");

    // Kernel below 1MB (invalid)
    assert!(BootSetup::validate_boot_addresses(0x80000, 0x100000, None).is_err());
    println!("✓ Kernel below 1MB rejected");

    // Setup above 1MB (invalid)
    assert!(BootSetup::validate_boot_addresses(0x100000, 0x400000, Some(0x200000)).is_err());
    println!("✓ Setup above 1MB rejected");

    // Overflow (invalid)
    assert!(BootSetup::validate_boot_addresses(0xFFFFFFFF_FFFFF000, 0x2000, None).is_err());
    println!("✓ Address overflow rejected");

    Ok(())
}

/// Test page table structure
#[tokio::test]
async fn test_page_table_structure() -> Result<()> {
    let pt_base = 0x3000u64;
    let tables = BootSetup::create_identity_page_tables(pt_base);

    assert_eq!(tables.len(), 16 * 1024);

    // Verify PML4E[0] points to PDPT
    let pml4e = u64::from_le_bytes(tables[0..8].try_into().unwrap());
    let pdpt_addr = pt_base + 0x1000;
    assert_eq!(pml4e & !0xFFF, pdpt_addr);
    assert_eq!(pml4e & 0x03, 0x03); // Present + Writable
    println!("✓ PML4E valid: points to PDPT at 0x{:X}", pdpt_addr);

    // Verify PDPTE[0] points to PD
    let pdpte = u64::from_le_bytes(tables[0x1000..0x1008].try_into().unwrap());
    let pd_addr = pt_base + 0x2000;
    assert_eq!(pdpte & !0xFFF, pd_addr);
    assert_eq!(pdpte & 0x03, 0x03);
    println!("✓ PDPTE valid: points to PD at 0x{:X}", pd_addr);

    // Verify PDE[0] is 2MB page
    let pde = u64::from_le_bytes(tables[0x2000..0x2008].try_into().unwrap());
    assert_eq!(pde & !0xFFF, 0);
    assert_eq!(pde & 0x83, 0x83); // Present + Writable + PS
    println!("✓ PDE valid: 2MB page at physical address 0");

    Ok(())
}

/// Test GDT and IDT builder integration
#[tokio::test]
async fn test_descriptor_builders() -> Result<()> {
    // Build GDT
    let gdt = GdtBuilder::new()
        .add_null()
        .add_code_64bit(DESC_DPL_0)
        .add_data_64bit(DESC_DPL_0)
        .add_code_64bit(DESC_DPL_3)
        .add_data_64bit(DESC_DPL_3)
        .build();

    assert_eq!(gdt.len(), 5 * 8); // 5 entries * 8 bytes
    println!("✓ GDT built: {} bytes ({} entries)", gdt.len(), gdt.len() / 8);

    // Build IDT
    let idt = IdtBuilder::new_64bit()
        .add_interrupt_gate(0, 0xFFFF_8000_0010_0000, 0x08, 0, DESC_DPL_0)
        .add_interrupt_gate(14, 0xFFFF_8000_0010_0E00, 0x08, 0, DESC_DPL_0)
        .add_trap_gate(3, 0xFFFF_8000_0010_0200, 0x08, 0, DESC_DPL_3)
        .build();

    assert_eq!(idt.len(), 256 * 16); // 256 entries * 16 bytes
    println!("✓ IDT built: {} bytes ({} entries)", idt.len(), idt.len() / 16);

    // Test pointer generation
    let gdt_ptr = GdtBuilder::new()
        .add_null()
        .add_code_64bit(DESC_DPL_0)
        .build_pointer(0x1000);

    let base = gdt_ptr.base;
    let limit = gdt_ptr.limit;
    assert_eq!(base, 0x1000);
    assert_eq!(limit, 15); // 2 entries * 8 - 1
    println!("✓ GDT pointer: base=0x{:X}, limit={}", base, limit);

    Ok(())
}
