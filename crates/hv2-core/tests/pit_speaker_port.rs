//! A real guest must see the PIT speaker-port stub rather than unmapped I/O.
#![cfg(target_os = "linux")]
use hv2_core::{BootSource, HypervisorPlatform, VMConfig, VM};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn pit_speaker_port_is_serviced_by_kvm() {
    if HypervisorPlatform::detect() != HypervisorPlatform::Kvm {
        eprintln!("skipping PIT speaker-port regression: no usable /dev/kvm");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let image = directory.path().join("guest.bin");
    // Real mode: mov dx, 0x61; in al, dx; hlt. Interrupts remain disabled.
    std::fs::write(&image, [0xba, 0x61, 0x00, 0xec, 0xf4]).unwrap();
    let vm = Arc::new(
        VM::new(VMConfig {
            name: "pit-speaker-port".into(),
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
    let sample = vm.diagnostic_vcpu_states().await;
    let stopped = tokio::time::timeout(Duration::from_secs(10), vm.stop()).await;
    stopped.expect("guest must remain stoppable").unwrap();
    let states = sample.expect("vCPU owner must reply");
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].general.rip, 5, "guest completed the port read");
    assert_eq!(
        states[0].general.rax & 0xc0,
        0,
        "PIT speaker stub has reserved bits clear; unmapped I/O returns 0xff"
    );
}
