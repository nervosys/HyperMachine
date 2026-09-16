//! Phase 1 of `docs/CUBESANDBOX_PARITY_ROADMAP.md`, continued: a real gRPC
//! server against envd's actual `process.Process` service (from
//! `e2b-dev/runtime`'s `packages/envd/spec/process/process.proto`, copied
//! verbatim into `proto/process.proto`), not a guessed shape.
//!
//! The actual service implementation lives in the crate itself now
//! (`hv2_api::envd_process`), shared with `e2b_compat`'s per-sandbox gRPC
//! listener -- see that module's doc comment for what's real here and what
//! is not (output is polled at an interval rather than pushed; `Update`, and
//! PTYs generally, are `unimplemented`). This example is the standalone case: one VM, booted
//! at startup, served for the process of this binary's lifetime -- the
//! shape envd itself actually has (one daemon per sandbox, no sandbox ID
//! anywhere in the protocol).
//!
//! # Running it
//!
//! ```text
//! HV2_KERNEL=/var/tmp/kbuild/bzImage HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run --release -p hv2-api --example envd_process -- --port 8081
//! ```
//!
//! Then, with `grpcurl` (a real gRPC client, proving this is a real gRPC
//! service and not just an HTTP handler that looks like one):
//!
//! ```text
//! grpcurl -plaintext -import-path crates/hv2-api/proto -proto process.proto \
//!   -d '{"process":{"cmd":"/bin/sh","args":["-c","echo hello; uname -a"]}}' \
//!   localhost:8081 process.Process/Start
//! ```

use std::time::Duration;

use hv2_agent::{AgentVM, Capability, CapabilitySet};
use hv2_api::envd_process::serve_for_sandbox;

struct Options {
    port: u16,
    kernel: String,
    initrd: String,
}

fn parse_options() -> Result<Options, String> {
    let kernel =
        std::env::var("HV2_KERNEL").map_err(|_| "HV2_KERNEL must name a bzImage".to_string())?;
    let initrd = std::env::var("HV2_INITRD")
        .map_err(|_| "HV2_INITRD must name an initramfs running hv2-guest-agentd".to_string())?;
    let mut port = 8081u16;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            port = args
                .get(i)
                .ok_or("--port needs a value")?
                .parse()
                .map_err(|e| format!("{e}"))?;
        }
        i += 1;
    }
    Ok(Options {
        port,
        kernel,
        initrd,
    })
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let opts = match parse_options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("envd_process: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    println!("envd_process: booting this envd's one VM...");
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);
    let build = AgentVM::builder()
        .name("envd-process")
        .cpu_cores(1)
        .memory_gb(1)
        .capabilities(capabilities)
        .boot_linux(
            &opts.kernel,
            Some(&opts.initrd),
            format!(
                "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0 {}",
                hv2_core::BootSource::MICROVM_FAST_BOOT_ARGS
            ),
        )
        .build()
        .await;
    let vm = match build {
        Ok(vm) => vm,
        Err(e) => {
            eprintln!("envd_process: could not build the VM: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if let Err(e) = vm.attach_guest_channel(3).await {
        eprintln!("envd_process: attach_guest_channel: {e}");
        return std::process::ExitCode::FAILURE;
    }
    if let Err(e) = vm.launch().await {
        eprintln!("envd_process: launch: {e}");
        return std::process::ExitCode::FAILURE;
    }
    if let Err(e) = vm.ping_guest(Duration::from_secs(15)).await {
        eprintln!("envd_process: guest never became ready: {e}");
        let _ = vm.stop().await;
        return std::process::ExitCode::FAILURE;
    }
    println!("envd_process: this envd's VM is up and answering");

    let addr = format!("0.0.0.0:{}", opts.port).parse().unwrap();
    println!("envd_process: serving process.Process on {addr}");
    let (_tx, rx) = tokio::sync::oneshot::channel();
    if let Err(e) = serve_for_sandbox(std::sync::Arc::new(vm), addr, rx).await {
        eprintln!("envd_process: server error: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
