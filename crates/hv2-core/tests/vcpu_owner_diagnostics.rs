//! Real KVM regression: halted/spinning vCPUs reply without concurrent ioctls.
#![cfg(target_os = "linux")]
use hv2_core::{BootSource, HypervisorPlatform, VMConfig, VMState, VM};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn singleton_guest_cpuid_reports_its_provisioned_processor_count() {
    if HypervisorPlatform::detect() != HypervisorPlatform::Kvm {
        eprintln!("skipping singleton topology: no usable /dev/kvm");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let image = directory.path().join("cpuid.bin");
    // Real mode: mov eax,1; cpuid; hlt. Preserve the CPUID result for capture.
    std::fs::write(&image, [0x66, 0xb8, 1, 0, 0, 0, 0x0f, 0xa2, 0xf4]).unwrap();
    let vm = Arc::new(
        VM::new(VMConfig {
            name: "singleton-cpuid".into(),
            vcpu_count: 1,
            memory_size: 16 * 1024 * 1024,
            boot: Some(BootSource::raw(&image)),
            ..Default::default()
        })
        .unwrap(),
    );
    vm.provision().await.unwrap();
    vm.launch().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let sampled = vm.diagnostic_vcpu_states().await;
    tokio::time::timeout(Duration::from_secs(10), vm.stop())
        .await
        .expect("CPUID guest must remain stoppable")
        .unwrap();
    let states = sampled.unwrap();
    assert_eq!(states[0].general.rip, 9);
    assert_eq!((states[0].general.rbx >> 16) & 0xff, 1);
    assert_eq!((states[0].general.rbx >> 24) & 0xff, 0);
    assert_eq!(states[0].general.rdx & (1 << 28), 0);
}

#[tokio::test]
async fn machine_interrupt_state_is_readable_before_kicking_a_halted_owner() {
    if HypervisorPlatform::detect() != HypervisorPlatform::Kvm {
        eprintln!("skipping machine diagnostics: no usable /dev/kvm");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let image = directory.path().join("guest.bin");
    std::fs::write(&image, [0xf4]).unwrap();
    let vm = Arc::new(
        VM::new(VMConfig {
            name: "pre-kick-machine-state".into(),
            vcpu_count: 1,
            memory_size: 16 * 1024 * 1024,
            boot: Some(BootSource::raw(&image)),
            ..Default::default()
        })
        .unwrap(),
    );
    vm.provision().await.unwrap();
    vm.launch().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Only VM-level ioctls: the vCPU remains in KVM_RUN with interrupts off.
    let machine = vm.backend().save_machine().await;
    let stopped = tokio::time::timeout(Duration::from_secs(10), vm.stop()).await;
    stopped
        .expect("machine sample must leave guest stoppable")
        .unwrap();
    let machine = machine.unwrap().expect("KVM must capture interrupt state");
    assert_eq!(machine.backend, "kvm");
    assert_eq!(machine.pic_master.len(), 512);
    assert_eq!(machine.pic_slave.len(), 512);
    assert_eq!(machine.ioapic.len(), 512);
    assert_eq!(machine.pit.len(), 112);
}

#[tokio::test]
async fn halted_and_spinning_guests_reply_on_the_owner_and_remain_stoppable() {
    if HypervisorPlatform::detect() != HypervisorPlatform::Kvm {
        eprintln!("skipping owner diagnostics: no usable /dev/kvm");
        return;
    }
    for (name, code, expected_rip) in [("halt", vec![0xf4], 1), ("spin", vec![0xeb, 0xfe], 0)] {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("guest.bin");
        std::fs::write(&image, code).unwrap();
        let vm = Arc::new(
            VM::new(VMConfig {
                name: format!("owner-diagnostic-{name}"),
                vcpu_count: 1,
                memory_size: 16 * 1024 * 1024,
                boot: Some(BootSource::raw(&image)),
                ..Default::default()
            })
            .unwrap(),
        );
        vm.provision().await.unwrap();
        assert!(vm.diagnostic_vcpu_states().await.is_err());
        vm.start().await.unwrap();
        assert!(
            vm.diagnostic_vcpu_states().await.is_err(),
            "unlaunched VM must refuse"
        );
        vm.stop().await.unwrap();
        vm.launch().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let sampled = async {
            let mut samples = Vec::new();
            for _ in 0..10 {
                samples.push(vm.diagnostic_vcpu_states().await?);
            }
            Ok::<_, hv2_core::Error>(samples)
        }
        .await;
        let state_after_samples = vm.state();
        let stopped = tokio::time::timeout(Duration::from_secs(10), vm.stop()).await;
        stopped
            .expect("diagnostic guest must remain stoppable")
            .unwrap();
        assert_eq!(state_after_samples, VMState::Running);
        for states in sampled.expect("owner must reply for halted and spinning guests") {
            assert_eq!(states.len(), 1);
            assert_eq!(states[0].id, 0);
            assert_eq!(states[0].general.rip, expected_rip, "{name} architecture");
            assert_eq!(
                states[0].general.rflags & (1 << 9),
                0,
                "interrupts remain disabled"
            );
        }
    }
}
