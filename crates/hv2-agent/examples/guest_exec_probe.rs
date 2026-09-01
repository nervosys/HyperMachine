//! Run a command inside a guest through the published API.
//!
//! The boot probe in `hv2-core` drives the vsock device by hand: it connects,
//! publishes, signals, and reads, in the right order, because it was written
//! alongside the device. This does none of that. It uses
//! [`AgentVM::ping_guest`] and [`AgentVM::exec_in_guest`] — the surface an
//! embedder actually has — and so it answers a different question: not "can
//! these bytes reach the guest", but "can someone who did not write the device
//! get a command run in there".
//!
//! # Running it
//!
//! ```text
//! HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run -p hv2-agent --example guest_exec_probe -- /var/tmp/kbuild/bzImage
//! ```
//!
//! Needs `/dev/kvm`, a kernel built with `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`,
//! and an initramfs that starts `hv2-guest-agentd`. It prints what it did at
//! each step and exits non-zero if the guest never answers, so it is usable as a
//! check rather than only as a thing to read.

use hv2_agent::{AgentVM, Capability, CapabilitySet};
use std::time::Duration;

/// Context ID for the guest. 0 and 1 are reserved and 2 means the host.
const GUEST_CID: u64 = 3;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("off")),
        )
        .init();

    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            println!("FAILED        — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let kernel = std::env::args().nth(1).ok_or_else(|| {
        "usage: guest_exec_probe <bzImage>  (set HV2_INITRD to an initramfs \
         running hv2-guest-agentd)"
            .to_string()
    })?;
    let initrd = std::env::var("HV2_INITRD").ok();
    let settle = std::env::var("HV2_SETTLE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12u64);

    // GuestExec and nothing else that matters here: the point is to exercise
    // the gate as an embedder meets it, not to hand the VM every capability so
    // that a permission defect could not show up.
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);

    let vm = AgentVM::builder()
        .name("guest-exec-probe")
        .cpu_cores(1)
        .memory_gb(2)
        .capabilities(capabilities)
        .boot_linux(
            &kernel,
            initrd.as_ref(),
            "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0",
        )
        .build()
        .await
        .map_err(|e| format!("building the VM: {e}"))?;
    println!("build         : ok");

    // Before launch, and before the command line is fixed: the guest has to be
    // told where to look for the device, and a guest that boots without that
    // argument enumerates nothing and reports no device rather than failing.
    vm.attach_guest_channel(GUEST_CID)
        .await
        .map_err(|e| format!("attaching the guest channel: {e}"))?;
    let args = vm
        .guest_kernel_args()
        .ok_or_else(|| "the channel attached but reported no kernel arguments".to_string())?;
    println!("channel       : attached, guest CID {GUEST_CID}");
    println!("kernel args   : {args}");

    vm.launch().await.map_err(|e| format!("launching: {e}"))?;
    println!("launch        : ok, waiting {settle}s for the guest to come up");
    tokio::time::sleep(Duration::from_secs(settle)).await;

    // Ping first. A guest that is up with no agent in it looks exactly like a
    // guest that is up with one, and telling those apart before running a
    // command makes a failure mean something.
    let version = vm
        .ping_guest(Duration::from_secs(20))
        .await
        .map_err(|e| format!("pinging the guest agent: {e}"))?;
    println!("ping          : agent {version}");

    let exec = vm
        .exec_in_guest("/bin/uname", &["-a".to_string()], Duration::from_secs(20))
        .await
        .map_err(|e| format!("running uname in the guest: {e}"))?;
    println!(
        "exec status   : exit={:?} signal={:?} timed_out={}",
        exec.exit_code, exec.signal, exec.timed_out
    );
    println!("exec stdout   : {}", exec.stdout.trim_end());
    if !exec.stderr.is_empty() {
        println!("exec stderr   : {}", exec.stderr.trim_end());
    }

    if exec.stdout.trim().is_empty() {
        return Err("the guest ran the command and said nothing, which is not a pass".to_string());
    }
    println!("result        : a command ran in the guest through the published API");
    Ok(())
}
