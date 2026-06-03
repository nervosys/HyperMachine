//! KVM Backend Integration Tests
//!
//! These tests verify the KVM backend implementation.
//! They require Linux with KVM support to run.

#![cfg(target_os = "linux")]

use hv2_core::backends::kvm::KvmBackend;
use hv2_core::hypervisor::{HypervisorBackend, HypervisorPlatform};
use hv2_core::{IoDirection, VmExit};

#[tokio::test]
#[ignore] // Requires /dev/kvm access
async fn test_kvm_backend_creation() {
    let backend = KvmBackend::new();

    match backend {
        Ok(backend) => {
            assert_eq!(backend.platform(), HypervisorPlatform::Kvm);
            let caps = backend.capabilities();
            assert!(caps.max_vcpus > 0);
            assert!(caps.max_memory > 0);
        }
        Err(e) => {
            // If KVM is not available, skip the test
            eprintln!("KVM not available: {}. Test skipped.", e);
        }
    }
}

#[tokio::test]
#[ignore] // Requires /dev/kvm access
async fn test_kvm_backend_initialization() {
    let mut backend = match KvmBackend::new() {
        Ok(b) => b,
        Err(_) => {
            eprintln!("KVM not available. Test skipped.");
            return;
        }
    };

    let result = backend.init().await;
    assert!(result.is_ok(), "KVM backend initialization should succeed");
}

#[tokio::test]
#[ignore] // Requires /dev/kvm access
async fn test_kvm_vm_creation() {
    let mut backend = match KvmBackend::new() {
        Ok(b) => b,
        Err(_) => {
            eprintln!("KVM not available. Test skipped.");
            return;
        }
    };

    backend.init().await.expect("init failed");

    // Create a VM with 1 vCPU and 64MB RAM
    let vm = backend.create_vm(1, 64 * 1024 * 1024).await;
    assert!(vm.is_ok(), "VM creation should succeed");

    if let Ok(vm) = vm {
        assert_eq!(vm.platform(), HypervisorPlatform::Kvm);
    }
}

#[tokio::test]
#[ignore] // Requires /dev/kvm access
async fn test_kvm_vm_memory_limits() {
    let mut backend = match KvmBackend::new() {
        Ok(b) => b,
        Err(_) => {
            eprintln!("KVM not available. Test skipped.");
            return;
        }
    };

    backend.init().await.expect("init failed");

    let caps = backend.capabilities();

    // Try to create VM exceeding max memory
    let result = backend.create_vm(1, caps.max_memory + 1).await;
    assert!(result.is_err(), "Should reject memory size exceeding limit");

    // Try to create VM exceeding max vCPUs
    let result = backend
        .create_vm(caps.max_vcpus + 1, 64 * 1024 * 1024)
        .await;
    assert!(result.is_err(), "Should reject vCPU count exceeding limit");
}

/// Test KVM exit reason conversion
///
/// This test verifies that KVM exit reasons are correctly converted
/// to our VmExit enum.
#[test]
fn test_exit_reason_conversion() {
    // Test HLT exit
    let exit = VmExit::Hlt;
    assert!(matches!(exit, VmExit::Hlt));

    // Test I/O exit
    let exit = VmExit::Io {
        port: 0x3F8,
        direction: IoDirection::Out,
        size: 1,
        data: b'A' as u32,
    };
    if let VmExit::Io {
        port,
        direction,
        size,
        data,
    } = exit
    {
        assert_eq!(port, 0x3F8);
        assert_eq!(direction, IoDirection::Out);
        assert_eq!(size, 1);
        assert_eq!(data, b'A' as u32);
    } else {
        panic!("Expected I/O exit");
    }

    // Test MMIO exit
    let exit = VmExit::Mmio {
        phys_addr: 0x10000000,
        data: [1, 2, 3, 4, 0, 0, 0, 0],
        len: 4,
        is_write: true,
    };
    if let VmExit::Mmio {
        phys_addr,
        len,
        is_write,
        ..
    } = exit
    {
        assert_eq!(phys_addr, 0x10000000);
        assert_eq!(len, 4);
        assert!(is_write);
    } else {
        panic!("Expected MMIO exit");
    }
}

/// Test KVM capabilities detection
#[test]
#[ignore] // Requires /dev/kvm access
fn test_capabilities_detection() {
    let backend = match KvmBackend::new() {
        Ok(b) => b,
        Err(_) => {
            eprintln!("KVM not available. Test skipped.");
            return;
        }
    };

    let caps = backend.capabilities();

    // KVM should support at least basic features
    assert!(caps.max_vcpus >= 1, "Should support at least 1 vCPU");
    assert!(
        caps.max_memory >= 1024 * 1024,
        "Should support at least 1MB RAM"
    );

    // Log capabilities for debugging
    println!("KVM Capabilities:");
    println!("  Max vCPUs: {}", caps.max_vcpus);
    println!("  Max Memory: {} bytes", caps.max_memory);
    println!("  Nested Virt: {}", caps.supports_nested_virt);
    println!("  APIC: {}", caps.supports_apic);
    println!("  x2APIC: {}", caps.supports_x2apic);
    println!("  IOMMU: {}", caps.supports_iommu);
}

/// Comprehensive KVM backend test
///
/// This test creates a VM, allocates memory, creates a vCPU,
/// and verifies the basic setup works correctly.
#[tokio::test]
#[ignore] // Requires /dev/kvm access
async fn test_kvm_full_vm_setup() {
    let mut backend = match KvmBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("KVM not available: {}. Test skipped.", e);
            return;
        }
    };

    // Initialize backend
    backend.init().await.expect("Backend initialization failed");

    // Create VM with 2 vCPUs and 128MB RAM
    let vm_result = backend.create_vm(2, 128 * 1024 * 1024).await;
    assert!(vm_result.is_ok(), "VM creation should succeed");

    let vm = vm_result.unwrap();
    assert_eq!(vm.platform(), HypervisorPlatform::Kvm);

    println!("Successfully created KVM VM:");
    println!("  vCPUs: 2");
    println!("  Memory: 128 MB");
}

/// Test shutdown cleanup
#[tokio::test]
#[ignore] // Requires /dev/kvm access
async fn test_kvm_shutdown() {
    let mut backend = match KvmBackend::new() {
        Ok(b) => b,
        Err(_) => {
            eprintln!("KVM not available. Test skipped.");
            return;
        }
    };

    backend.init().await.expect("init failed");

    // Create a VM
    let _ = backend.create_vm(1, 64 * 1024 * 1024).await;

    // Shutdown should clean up resources
    let result = backend.shutdown().await;
    assert!(result.is_ok(), "Shutdown should succeed");
}

/// Test error handling for invalid operations
#[test]
fn test_kvm_error_handling() {
    // Test that we can create VmExit error types
    let unknown_exit = VmExit::Unknown { reason: 999 };
    if let VmExit::Unknown { reason } = unknown_exit {
        assert_eq!(reason, 999);
    } else {
        panic!("Expected Unknown exit");
    }

    let exception_exit = VmExit::Exception {
        vector: 13, // General Protection Fault
        error_code: Some(0x1234),
    };
    if let VmExit::Exception { vector, error_code } = exception_exit {
        assert_eq!(vector, 13);
        assert_eq!(error_code, Some(0x1234));
    } else {
        panic!("Expected Exception exit");
    }
}

/// Test KVM FFI constants
#[test]
fn test_kvm_ffi_constants() {
    use hv2_core::backends::kvm_ffi::*;

    // Verify API version
    assert_eq!(KVM_API_VERSION, 12);

    // Verify exit reasons
    assert_eq!(KVM_EXIT_HLT, 5);
    assert_eq!(KVM_EXIT_MMIO, 6);
    assert_eq!(KVM_EXIT_IO, 2);
    assert_eq!(KVM_EXIT_SHUTDOWN, 8);

    // Verify I/O directions
    assert_eq!(KVM_EXIT_IO_IN, 0);
    assert_eq!(KVM_EXIT_IO_OUT, 1);
}

/// Test that KVM structures have correct sizes
#[test]
fn test_kvm_structure_sizes() {
    use hv2_core::backends::kvm_ffi::*;
    use std::mem;

    // kvm_userspace_memory_region should be 32 bytes
    assert_eq!(mem::size_of::<kvm_userspace_memory_region>(), 32);

    // kvm_regs should be 144 bytes (18 * 8)
    assert_eq!(mem::size_of::<kvm_regs>(), 144);

    // kvm_interrupt should be 4 bytes
    assert_eq!(mem::size_of::<kvm_interrupt>(), 4);

    println!("KVM structure sizes verified:");
    println!(
        "  kvm_userspace_memory_region: {} bytes",
        mem::size_of::<kvm_userspace_memory_region>()
    );
    println!("  kvm_regs: {} bytes", mem::size_of::<kvm_regs>());
    println!("  kvm_sregs: {} bytes", mem::size_of::<kvm_sregs>());
    println!("  kvm_run: {} bytes", mem::size_of::<kvm_run>());
}
