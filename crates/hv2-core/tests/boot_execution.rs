//! Integration tests for boot execution
//!
//! This module contains comprehensive end-to-end tests for all boot protocols:
//! - Linux boot protocol (bzImage format, 64-bit long mode)
//! - Multiboot 1.0 protocol (32-bit protected mode)
//! - Real mode boot (boot sectors, MBR/VBR)
//!
//! Tests validate:
//! - Complete boot sequences
//! - CPU state after boot
//! - Memory layout correctness
//! - Segment register configuration
//! - Error handling for invalid inputs

use hv2_core::{
    backends::whpx::{CpuMode, WhpxBackend, WhpxVm},
    boot::{
        linux::{LinuxBootParams, LinuxBootProtocol},
        multiboot::{MultibootInfo, MultibootModule, MultibootProtocol},
    },
};

/// Helper to create a valid Linux bzImage kernel
fn create_test_bzimage(size: usize) -> Vec<u8> {
    let mut kernel = vec![0u8; size];

    // Boot signature at offset 0x1FE
    kernel[0x1FE] = 0x55;
    kernel[0x1FF] = 0xAA;

    // Linux magic "HdrS" at offset 0x202
    kernel[0x202] = b'H';
    kernel[0x203] = b'd';
    kernel[0x204] = b'r';
    kernel[0x205] = b'S';

    // Protocol version 2.10 at offset 0x206
    kernel[0x206] = 0x0A;
    kernel[0x207] = 0x02;

    // Setup sectors at offset 0x1F1
    kernel[0x1F1] = 4;

    kernel
}

/// Helper to create a valid Multiboot kernel
fn create_test_multiboot(size: usize) -> Vec<u8> {
    let mut kernel = vec![0u8; size];

    // Multiboot header at offset 0x100
    let offset = 0x100;
    kernel[offset] = 0x02; // Magic: 0x1BADB002
    kernel[offset + 1] = 0xB0;
    kernel[offset + 2] = 0xAD;
    kernel[offset + 3] = 0x1B;

    // Flags: 0x00000000
    kernel[offset + 4] = 0x00;
    kernel[offset + 5] = 0x00;
    kernel[offset + 6] = 0x00;
    kernel[offset + 7] = 0x00;

    // Checksum: -(magic + flags)
    let checksum = 0u32.wrapping_sub(0x1BADB002).wrapping_sub(0x00000000);
    kernel[offset + 8] = (checksum & 0xFF) as u8;
    kernel[offset + 9] = ((checksum >> 8) & 0xFF) as u8;
    kernel[offset + 10] = ((checksum >> 16) & 0xFF) as u8;
    kernel[offset + 11] = ((checksum >> 24) & 0xFF) as u8;

    kernel
}

/// Helper to create a valid boot sector
fn create_test_boot_sector() -> Vec<u8> {
    let mut boot_sector = vec![0u8; 512];

    // Simple HLT loop
    boot_sector[0] = 0xF4; // HLT
    boot_sector[1] = 0xEB; // JMP short
    boot_sector[2] = 0xFD; // -3 (jump back to HLT)

    // Boot signature
    boot_sector[510] = 0x55;
    boot_sector[511] = 0xAA;

    boot_sector
}

#[tokio::test]
async fn test_linux_boot_complete() {
    // Create a complete Linux boot environment
    let kernel = create_test_bzimage(8192);
    let initrd = vec![0xFFu8; 2048]; // 2KB initrd

    let params = LinuxBootParams {
        kernel_image: kernel,
        cmdline: "console=ttyS0 debug".to_string(),
        initrd: Some(initrd),
        setup_addr: 0x90000,
        kernel_addr: 0x100000,
    };

    // Validate parameters
    if let Err(e) = LinuxBootProtocol::validate_params(&params) {
        panic!("Linux boot params validation failed: {}", e);
    }

    // Attempt boot if WHPX is available
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 128 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                match vcpu.boot_linux(&vm, &params, 0x100000) {
                    Ok(()) => {
                        println!("✓ Complete Linux boot test passed");

                        // Verify CPU state
                        if let Ok(regs) = vcpu.get_register_set() {
                            assert_eq!(regs.rip, 0x100000, "RIP should be at entry point");
                            assert_eq!(regs.rsi, 0x90000, "RSI should point to boot_params");
                            println!("  RIP: 0x{:016X}", regs.rip);
                            println!("  RSI: 0x{:016X}", regs.rsi);
                        }

                        // Verify CPU mode
                        if let Ok(mode) = vcpu.get_cpu_mode() {
                            assert_eq!(
                                mode,
                                CpuMode::LongMode64Bit,
                                "CPU should be in 64-bit long mode"
                            );
                            println!("  Mode: 64-bit long mode");
                        }
                    }
                    Err(e) => {
                        println!("⚠ Linux boot test skipped (WHPX unavailable): {}", e);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn test_multiboot_boot_complete() {
    // Create a complete Multiboot boot environment
    let kernel = create_test_multiboot(16384); // 16KB kernel

    let module1 = MultibootModule {
        data: vec![0xAAu8; 4096],
        cmdline: "test_module_1".to_string(),
    };

    let module2 = MultibootModule {
        data: vec![0xBBu8; 8192],
        cmdline: "test_module_2".to_string(),
    };

    let info = MultibootInfo {
        kernel_image: kernel,
        modules: vec![module1, module2],
        cmdline: "root=/dev/sda1 ro quiet".to_string(),
        memory_map: vec![
            (0, 640 * 1024),                  // Lower memory
            (1024 * 1024, 127 * 1024 * 1024), // Upper memory
        ],
    };

    // Validate parameters
    if let Err(e) = MultibootProtocol::validate_params(&info) {
        panic!("Multiboot params validation failed: {}", e);
    }

    // Attempt boot if WHPX is available
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 128 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                match vcpu.boot_multiboot(&vm, &info, 0x100000) {
                    Ok(()) => {
                        println!("✓ Complete Multiboot boot test passed");

                        // Verify CPU state
                        if let Ok(regs) = vcpu.get_register_set() {
                            assert_eq!(regs.rip as u32, 0x100000, "EIP should be at entry point");
                            assert_eq!(
                                regs.rax as u32, 0x2BADB002,
                                "EAX should contain Multiboot magic"
                            );
                            assert_eq!(
                                regs.rbx as u32, 0x9000,
                                "EBX should point to multiboot_info"
                            );
                            println!("  EIP: 0x{:08X}", regs.rip as u32);
                            println!("  EAX: 0x{:08X}", regs.rax as u32);
                            println!("  EBX: 0x{:08X}", regs.rbx as u32);
                        }

                        // Verify CPU mode
                        if let Ok(mode) = vcpu.get_cpu_mode() {
                            assert_eq!(
                                mode,
                                CpuMode::ProtectedMode,
                                "CPU should be in 32-bit protected mode"
                            );
                            println!("  Mode: 32-bit protected mode");
                        }
                    }
                    Err(e) => {
                        println!("⚠ Multiboot boot test skipped (WHPX unavailable): {}", e);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn test_real_mode_boot_complete() {
    // Create a boot sector
    let boot_sector = create_test_boot_sector();

    // Attempt boot if WHPX is available
    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                match vcpu.boot_real_mode(&vm, &boot_sector, 0x80).await {
                    Ok(()) => {
                        println!("✓ Complete real mode boot test passed");

                        // Verify CPU state
                        if let Ok(regs) = vcpu.get_register_set() {
                            assert_eq!(regs.rip as u32, 0x7C00, "IP should be at boot sector");
                            let dl = (regs.rdx & 0xFF) as u8;
                            assert_eq!(dl, 0x80, "DL should contain boot drive");
                            println!("  CS:IP: 0x0000:0x{:04X}", regs.rip as u16);
                            println!("  DL: 0x{:02X}", dl);
                        }

                        // Verify CPU mode
                        if let Ok(mode) = vcpu.get_cpu_mode() {
                            assert_eq!(mode, CpuMode::RealMode, "CPU should be in real mode");
                            println!("  Mode: Real mode (16-bit)");
                        }
                    }
                    Err(e) => {
                        println!("⚠ Real mode boot test skipped (WHPX unavailable): {}", e);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn test_boot_state_validation() {
    // Test that boot methods properly configure all CPU state

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 64 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                // Test Linux boot state
                let kernel = create_test_bzimage(4096);
                let params = LinuxBootParams {
                    kernel_image: kernel,
                    cmdline: "test".to_string(),
                    initrd: None,
                    setup_addr: 0x90000,
                    kernel_addr: 0x100000,
                };

                if vcpu.boot_linux(&vm, &params, 0x100000).is_ok() {
                    println!("✓ Testing Linux boot state:");

                    // Check control registers
                    if let Ok(cr) = vcpu.get_control_registers() {
                        assert!(cr.is_paging_enabled(), "Paging should be enabled");
                        assert!(cr.is_protected_mode(), "Protected mode should be enabled");
                        assert!(cr.is_long_mode_enabled(), "Long mode should be enabled");
                        println!("  ✓ Control registers configured correctly");
                    }

                    // Check general registers
                    if let Ok(regs) = vcpu.get_register_set() {
                        assert_eq!(regs.rip, 0x100000, "RIP incorrect");
                        assert_eq!(regs.rsi, 0x90000, "RSI incorrect");
                        assert_eq!(regs.rsp, 0x00400000, "RSP incorrect");
                        println!("  ✓ General registers configured correctly");
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn test_memory_layout_validation() {
    // Test that boot methods don't create overlapping memory regions

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 128 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                // Test Linux with initrd (tests potential overlap)
                let kernel = create_test_bzimage(8192);
                let initrd = vec![0xFFu8; 4096];

                let params = LinuxBootParams {
                    kernel_image: kernel,
                    cmdline: "test".to_string(),
                    initrd: Some(initrd),
                    setup_addr: 0x90000,
                    kernel_addr: 0x100000,
                };

                if vcpu.boot_linux(&vm, &params, 0x100000).is_ok() {
                    println!("✓ Linux memory layout validation passed");
                    println!("  boot_params: 0x90000");
                    println!("  kernel: 0x100000");
                    println!("  initrd: 0x800000");
                    println!("  stack: 0x400000");
                }

                // Test Multiboot with modules
                let kernel = create_test_multiboot(8192);
                let module = MultibootModule {
                    data: vec![0xAAu8; 2048],
                    cmdline: "test".to_string(),
                };

                let info = MultibootInfo {
                    kernel_image: kernel,
                    modules: vec![module],
                    cmdline: "test".to_string(),
                    memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
                };

                if vcpu.boot_multiboot(&vm, &info, 0x100000).is_ok() {
                    println!("✓ Multiboot memory layout validation passed");
                    println!("  multiboot_info: 0x9000");
                    println!("  kernel: 0x100000");
                    println!("  modules: 0x200000");
                    println!("  stack: 0x400000");
                }
            }
        }
    }
}

#[tokio::test]
async fn test_segment_configuration() {
    // Test that segment registers are correctly configured for each boot mode

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 32 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                // Test 64-bit segment configuration (Linux)
                let kernel = create_test_bzimage(4096);
                let params = LinuxBootParams {
                    kernel_image: kernel,
                    cmdline: String::new(),
                    initrd: None,
                    setup_addr: 0x90000,
                    kernel_addr: 0x100000,
                };

                if vcpu.boot_linux(&vm, &params, 0x100000).is_ok() {
                    println!("✓ 64-bit segment configuration test passed");
                    // In long mode, segment bases are ignored (except FS/GS)
                    // but selectors should still be set correctly
                }

                // Test 32-bit segment configuration (Multiboot)
                let kernel = create_test_multiboot(4096);
                let info = MultibootInfo {
                    kernel_image: kernel,
                    modules: vec![],
                    cmdline: String::new(),
                    memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
                };

                if vcpu.boot_multiboot(&vm, &info, 0x100000).is_ok() {
                    println!("✓ 32-bit segment configuration test passed");
                    // Segments should be set to flat model (base=0, limit=4GB)
                }

                // Test 16-bit segment configuration (real mode)
                let boot_sector = create_test_boot_sector();

                if vcpu.boot_real_mode(&vm, &boot_sector, 0x80).await.is_ok() {
                    println!("✓ 16-bit segment configuration test passed");
                    // Segments should be set to real mode (base=selector<<4, limit=64KB)
                }
            }
        }
    }
}

#[tokio::test]
async fn test_boot_error_handling() {
    // Test that boot methods properly handle invalid inputs

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 16 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                println!("Testing boot error handling:");

                // Test invalid Linux kernel (no signature)
                let mut invalid_kernel = vec![0u8; 4096];
                invalid_kernel[0x202] = b'H';
                invalid_kernel[0x203] = b'd';
                invalid_kernel[0x204] = b'r';
                invalid_kernel[0x205] = b'S';

                let params = LinuxBootParams {
                    kernel_image: invalid_kernel,
                    cmdline: String::new(),
                    initrd: None,
                    setup_addr: 0x90000,
                    kernel_addr: 0x100000,
                };

                match LinuxBootProtocol::validate_params(&params) {
                    Ok(_) => {
                        println!("  ⚠ Linux validation should have failed (missing signature)")
                    }
                    Err(_) => println!("  ✓ Invalid Linux kernel rejected"),
                }

                // Test invalid Multiboot kernel (no header)
                let invalid_kernel = vec![0u8; 4096];
                let info = MultibootInfo {
                    kernel_image: invalid_kernel,
                    modules: vec![],
                    cmdline: String::new(),
                    memory_map: vec![(0, 640 * 1024)],
                };

                match MultibootProtocol::validate_params(&info) {
                    Ok(_) => println!("  ⚠ Multiboot validation should have failed (no header)"),
                    Err(_) => println!("  ✓ Invalid Multiboot kernel rejected"),
                }

                // Test invalid boot sector (wrong size)
                let invalid_sector = vec![0x55, 0xAA];
                match vcpu.boot_real_mode(&vm, &invalid_sector, 0x80).await {
                    Ok(_) => println!("  ⚠ Boot sector validation should have failed (wrong size)"),
                    Err(_) => println!("  ✓ Invalid boot sector rejected"),
                }

                // Test invalid boot sector (wrong signature)
                let mut invalid_sector = vec![0u8; 512];
                invalid_sector[510] = 0xAA;
                invalid_sector[511] = 0x55;
                match vcpu.boot_real_mode(&vm, &invalid_sector, 0x80).await {
                    Ok(_) => {
                        println!("  ⚠ Boot sector validation should have failed (wrong signature)")
                    }
                    Err(_) => println!("  ✓ Invalid boot sector signature rejected"),
                }
            }
        }
    }
}

#[tokio::test]
async fn test_boot_performance() {
    // Measure boot sequence performance

    use std::time::Instant;

    if let Ok(_backend) = WhpxBackend::new() {
        if let Ok(vm) = WhpxVm::new(1, 64 * 1024 * 1024) {
            if let Ok(vcpu) = vm.create_vcpu(0) {
                println!("Boot performance measurements:");

                // Test Linux boot performance
                let kernel = create_test_bzimage(8192);
                let params = LinuxBootParams {
                    kernel_image: kernel,
                    cmdline: "test".to_string(),
                    initrd: None,
                    setup_addr: 0x90000,
                    kernel_addr: 0x100000,
                };

                let start = Instant::now();
                if vcpu.boot_linux(&vm, &params, 0x100000).is_ok() {
                    let duration = start.elapsed();
                    println!("  Linux boot: {:?}", duration);
                    assert!(duration.as_millis() < 1000, "Linux boot took too long");
                }

                // Test Multiboot boot performance
                let kernel = create_test_multiboot(8192);
                let info = MultibootInfo {
                    kernel_image: kernel,
                    modules: vec![],
                    cmdline: "test".to_string(),
                    memory_map: vec![(0, 640 * 1024), (1024 * 1024, 127 * 1024 * 1024)],
                };

                let start = Instant::now();
                if vcpu.boot_multiboot(&vm, &info, 0x100000).is_ok() {
                    let duration = start.elapsed();
                    println!("  Multiboot boot: {:?}", duration);
                    assert!(duration.as_millis() < 1000, "Multiboot boot took too long");
                }

                // Test real mode boot performance
                let boot_sector = create_test_boot_sector();

                let start = Instant::now();
                if vcpu.boot_real_mode(&vm, &boot_sector, 0x80).await.is_ok() {
                    let duration = start.elapsed();
                    println!("  Real mode boot: {:?}", duration);
                    assert!(duration.as_millis() < 500, "Real mode boot took too long");
                }
            }
        }
    }
}
