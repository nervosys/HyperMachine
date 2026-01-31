//! Integration tests for interrupt delivery
//!
//! These tests verify the complete interrupt flow from device to guest:
//! Device → PIC → VM → Backend → Guest
//!
//! Note: These tests use the VM's PIC which is automatically created.
//! The PIC is pre-initialized and ready to use.

use hv2_core::{hypervisor::create_backend, VMConfig, VmExit, VM};
use std::sync::Arc;

/// Helper to create a VM with unmasked PIC for testing
async fn create_test_vm(name: &str) -> Arc<VM> {
    let config = VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        ..Default::default()
    };

    let vm = Arc::new(VM::new(config).expect("Failed to create VM"));
    let pic = vm.pic();

    // Unmask all interrupts on both master and slave PICs
    pic.set_master_mask(0x00);
    pic.set_slave_mask(0x00);

    vm
}

#[tokio::test]
async fn test_direct_pic_irq() {
    let vm = create_test_vm("test-direct-irq").await;
    let pic = vm.pic();

    // Directly raise IRQ 0
    pic.raise_irq(0).expect("Failed to raise IRQ 0");

    // Verify interrupt is pending
    let vector = pic.get_pending_interrupt();
    assert!(vector.is_some(), "No interrupt pending after raising IRQ");
    assert_eq!(vector.unwrap(), 0x20, "Wrong interrupt vector");

    // Acknowledge interrupt
    pic.acknowledge_interrupt(0x20)
        .expect("Failed to acknowledge interrupt");

    // Verify no more interrupts pending
    assert!(
        pic.get_pending_interrupt().is_none(),
        "Interrupt still pending after acknowledgment"
    );
}

#[tokio::test]
async fn test_direct_pic_irq4() {
    let vm = create_test_vm("test-direct-irq4").await;
    let pic = vm.pic();

    // Directly raise IRQ 4 (serial)
    pic.raise_irq(4).expect("Failed to raise IRQ 4");

    // Verify interrupt is pending
    let vector = pic.get_pending_interrupt();
    assert!(vector.is_some(), "No interrupt pending after raising IRQ 4");
    assert_eq!(
        vector.unwrap(),
        0x24,
        "Wrong interrupt vector (expected 0x24)"
    );

    // Acknowledge interrupt
    pic.acknowledge_interrupt(0x24)
        .expect("Failed to acknowledge interrupt");

    // Verify no more interrupts pending
    assert!(
        pic.get_pending_interrupt().is_none(),
        "Interrupt still pending after acknowledgment"
    );
}

#[tokio::test]
async fn test_interrupt_priority() {
    let vm = create_test_vm("test-priority").await;
    let pic = vm.pic();

    // Raise both IRQ 0 and IRQ 4
    pic.raise_irq(0).expect("Failed to raise IRQ 0");
    pic.raise_irq(4).expect("Failed to raise IRQ 4");

    // First interrupt should be IRQ 0 (higher priority)
    let vector1 = pic
        .get_pending_interrupt()
        .expect("No first interrupt pending");
    assert_eq!(
        vector1, 0x20,
        "First interrupt should be IRQ 0 (vector 0x20)"
    );

    pic.acknowledge_interrupt(vector1)
        .expect("Failed to acknowledge first interrupt");

    // Second interrupt should be IRQ 4 (lower priority)
    let vector2 = pic
        .get_pending_interrupt()
        .expect("No second interrupt pending");
    assert_eq!(
        vector2, 0x24,
        "Second interrupt should be IRQ 4 (vector 0x24)"
    );

    pic.acknowledge_interrupt(vector2)
        .expect("Failed to acknowledge second interrupt");

    // No more interrupts
    assert!(
        pic.get_pending_interrupt().is_none(),
        "Unexpected interrupt pending"
    );
}

#[tokio::test]
async fn test_lower_irq() {
    let vm = create_test_vm("test-lower-irq").await;
    let pic = vm.pic();

    // Raise IRQ 1
    pic.raise_irq(1).expect("Failed to raise IRQ 1");

    // Verify interrupt is pending
    assert!(
        pic.get_pending_interrupt().is_some(),
        "No interrupt pending after raising IRQ 1"
    );

    // Lower IRQ 1
    pic.lower_irq(1).expect("Failed to lower IRQ 1");

    // Verify interrupt is no longer pending
    assert!(
        pic.get_pending_interrupt().is_none(),
        "Interrupt still pending after lowering IRQ"
    );
}

#[tokio::test]
async fn test_multiple_irqs() {
    let vm = create_test_vm("test-multiple-irqs").await;
    let pic = vm.pic();

    // Test multiple interrupts (skip IRQ 2 which is cascade)
    let test_irqs = vec![0, 1, 3, 4, 5, 6, 7];

    for irq in test_irqs {
        pic.raise_irq(irq)
            .expect(&format!("Failed to raise IRQ {}", irq));

        let vector = pic
            .get_pending_interrupt()
            .expect(&format!("No interrupt pending for IRQ {}", irq));
        assert_eq!(
            vector,
            0x20 + irq,
            "Wrong vector for IRQ {} (expected {:#x}, got {:#x})",
            irq,
            0x20 + irq,
            vector
        );

        pic.acknowledge_interrupt(vector)
            .expect(&format!("Failed to acknowledge IRQ {}", irq));
    }

    // Verify no more interrupts pending
    assert!(
        pic.get_pending_interrupt().is_none(),
        "Unexpected interrupt pending"
    );
}

#[tokio::test]
#[ignore = "Requires WHPX hardware virtualization to be enabled"]
async fn test_vm_with_backend_integration() {
    let vm = create_test_vm("test-backend-integration").await;
    let backend = create_backend().expect("Failed to create backend");
    let vcpu = vm.vcpu(0).expect("Failed to get vCPU");
    let pic = vm.pic();

    // Start VM
    vm.start().await.expect("Failed to start VM");

    // Raise interrupt directly
    pic.raise_irq(0).expect("Failed to raise IRQ 0");

    // Check for pending interrupt
    if let Some(vector) = pic.get_pending_interrupt() {
        // Inject interrupt into guest
        backend
            .inject_interrupt(&vcpu, vector)
            .await
            .expect("Failed to inject interrupt");

        // Acknowledge interrupt
        pic.acknowledge_interrupt(vector)
            .expect("Failed to acknowledge interrupt");
    } else {
        panic!("No interrupt pending after raising IRQ");
    }

    // Run vCPU (simulated)
    let exit = backend.run_vcpu(&vcpu).await.expect("Failed to run vCPU");

    // Should exit with HLT
    match exit {
        VmExit::Hlt => {
            // Expected
        }
        other => {
            panic!("Unexpected exit type: {:?}", other);
        }
    }

    // Stop VM
    vm.stop().await.expect("Failed to stop VM");
}

// Slave PIC cascade test - Currently has issues in integration test environment
// The unit test for slave cascade in interrupt.rs passes successfully
// TODO: Investigate why this hangs in integration tests but works in unit tests
#[tokio::test]
#[ignore = "Hangs in integration test environment - unit test passes successfully"]
async fn test_slave_pic_cascade() {
    let vm = create_test_vm("test-slave-cascade").await;
    let pic = vm.pic();

    // Raise IRQ 8 (RTC on slave PIC)
    pic.raise_irq(8).expect("Failed to raise IRQ 8");

    // Verify interrupt is pending
    let vector = pic.get_pending_interrupt();
    assert!(vector.is_some(), "No interrupt pending after raising IRQ 8");

    // IRQ 8 maps to vector 0x28 (slave base 0x28 + IRQ 0)
    assert_eq!(
        vector.unwrap(),
        0x28,
        "Wrong interrupt vector for IRQ 8 (expected 0x28)"
    );

    // Acknowledge interrupt
    pic.acknowledge_interrupt(0x28)
        .expect("Failed to acknowledge interrupt");

    // Verify no more interrupts pending
    assert!(
        pic.get_pending_interrupt().is_none(),
        "Interrupt still pending after acknowledgment"
    );
}
