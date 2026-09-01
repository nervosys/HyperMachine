//! Ask this machine whether a VM can actually be provisioned on it.
//!
//! `VM::new` only selects a backend; the partition or VM file descriptor is
//! created later, by `provision()`. Those are different questions, and a host
//! can answer yes to the first and no to the second -- which is exactly what
//! Windows does here when Hyper-V owns VT-x. Run this on any candidate host
//! before trusting a boot-path claim on it.

use hv2_core::{HypervisorPlatform, VMConfig, VM};

#[tokio::main]
async fn main() {
    println!("detected platform : {:?}", HypervisorPlatform::detect());

    let config = VMConfig {
        name: "kvm-probe".to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        ..Default::default()
    };

    let vm = match VM::new(config) {
        Ok(vm) => {
            println!("VM::new           : ok (state {:?})", vm.state());
            vm
        }
        Err(e) => {
            println!("VM::new           : FAILED -- {e}");
            return;
        }
    };

    // The step that has never run outside a type-checker on this project.
    match vm.provision().await {
        Ok(()) => println!("VM::provision     : OK -- the backend created a VM and its vCPUs"),
        Err(e) => println!("VM::provision     : FAILED -- {e}"),
    }
}
