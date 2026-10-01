//! Real KVM regression: halted/spinning vCPUs reply without concurrent ioctls.
#![cfg(target_os = "linux")]
use hv2_core::{BootSource, HypervisorPlatform, VMConfig, VMState, VM};
use std::{sync::Arc, time::Duration};

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
