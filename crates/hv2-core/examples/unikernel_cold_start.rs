//! Phase 0 of `docs/CUBESANDBOX_PARITY_ROADMAP.md`: an honest boot-time
//! number for `hv2-unikernel`, next to CubeSandbox's published cold start
//! (<60ms at single concurrency; 67ms avg / P95 90ms / P99 137ms at 50
//! concurrent creations).
//!
//! `hv2-agent/examples/cold_start.rs` already does this for the Linux-guest
//! path, but needs a real bzImage kernel and an initramfs running
//! `hv2-guest-agentd` — building one is its own project, and `hv2-unikernel`
//! is the guest this repository has actually boot-verified (see
//! `net_echo.rs`, and `docs/CUBESANDBOX_PARITY_ROADMAP.md`'s account of it).
//! This measures that guest instead, reusing `cold_start.rs`'s phase
//! breakdown and percentile reporting so the two numbers are comparable in
//! shape even though they measure different boot paths.
//!
//! # What "ready" means here
//!
//! `hv2-unikernel` has no guest agent to ping. Its console output is the
//! only signal available, so "ready" is the guest printing `vsock cid ` —
//! the point at which its vsock driver has come up and it could accept a
//! host->guest exec the way `AgentVM::exec_in_guest` would use. An earlier
//! checkpoint, "alive" (the guest's first line, `HYPERMACHINE RUST
//! UNIKERNEL`), is also reported, since it separates "the vCPU is executing
//! guest code at all" from "the guest has finished bringing up its one
//! control channel" — collapsing those into one number would hide which
//! phase actually costs the time.
//!
//! # What this does not measure
//!
//! Memory overhead per sandbox. CubeSandbox's <5MB figure is a real,
//! specific measurement methodology this example does not attempt to
//! reproduce — printing a guessed number here would be exactly the failure
//! mode `cold_start.rs`'s own doc comment warns against. The guest's
//! configured memory (`--memory-mb`) is reported as context, not as an
//! overhead figure.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-core --example unikernel_cold_start -- --iterations 20
//! cargo run --release -p hv2-core --example unikernel_cold_start -- --iterations 20 --concurrency 50
//! ```
//!
//! Needs `/dev/kvm` and the `x86_64-unknown-none` target (builds the guest
//! itself, once, before timing anything).

use hv2_core::{BootSource, VMConfig, VM};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

const GUEST_TARGET: &str = "x86_64-unknown-none";
const GUEST_CID: u64 = 3;

struct Options {
    iterations: usize,
    concurrency: usize,
    memory_mb: u64,
    ready_timeout: Duration,
}

fn parse_options() -> Result<Options, String> {
    let mut opts = Options {
        iterations: 10,
        concurrency: 1,
        memory_mb: 64,
        ready_timeout: Duration::from_secs(10),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        let value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match args[i].as_str() {
            "--iterations" => {
                opts.iterations = value(&mut i)?.parse().map_err(|e| format!("{e}"))?;
            }
            "--concurrency" => {
                opts.concurrency = value(&mut i)?.parse().map_err(|e| format!("{e}"))?;
            }
            "--memory-mb" => opts.memory_mb = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--ready-timeout-secs" => {
                opts.ready_timeout =
                    Duration::from_secs(value(&mut i)?.parse().map_err(|e| format!("{e}"))?);
            }
            "--help" | "-h" => {
                println!(
                    "usage: unikernel_cold_start [--iterations N] [--concurrency N] \
                     [--memory-mb N] [--ready-timeout-secs N]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unrecognised argument {other}")),
        }
        i += 1;
    }
    if opts.iterations == 0 || opts.concurrency == 0 {
        return Err("iterations and concurrency must both be at least 1".to_string());
    }
    Ok(opts)
}

/// Build the guest crate once and stage its ELF on local storage. Not part
/// of any iteration's timing -- a real deployment boots a pre-built image,
/// and timing a `cargo build` here would measure this machine's compiler,
/// not this guest's boot.
fn build_guest() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv2-unikernel");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "building the guest failed:\n{}\n\nThe target may be missing:  rustup target add \
             {GUEST_TARGET}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let elf = crate_dir
        .join("target")
        .join(GUEST_TARGET)
        .join("release")
        .join("hv2-unikernel");
    if !elf.exists() {
        return Err(format!("the guest built but {} is missing", elf.display()));
    }
    let dir = std::env::temp_dir().join("hv2-unikernel-bench");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv2-unikernel.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the guest image: {e}"))?;
    Ok(local)
}

#[derive(Debug, Default, Clone)]
struct Sample {
    /// `VM::new` -- in-process struct allocation, no hypervisor syscalls.
    new_vm: Option<Duration>,
    /// `VM::provision` -- this is where `backend.create_vm()` issues the
    /// actual `KVM_CREATE_VM` ioctl. Split from `new_vm` specifically to find
    /// out whether a concurrency-50 slowdown is in-process contention or a
    /// kernel/KVM-level one.
    provision: Option<Duration>,
    channel: Option<Duration>,
    launch: Option<Duration>,
    alive: Option<Duration>,
    ready: Option<Duration>,
}

impl Sample {
    fn build(&self) -> Option<Duration> {
        Some(self.new_vm? + self.provision?)
    }
    fn to_running(&self) -> Option<Duration> {
        Some(self.build()? + self.channel? + self.launch?)
    }
    fn to_ready(&self) -> Option<Duration> {
        Some(self.to_running()? + self.ready?)
    }
}

/// Poll console output until `needle` appears, or the deadline passes.
/// Returns the elapsed time from `start` to the first appearance.
async fn wait_for(
    vm: &Arc<VM>,
    start: Instant,
    needle: &str,
    deadline: Instant,
) -> Option<Duration> {
    while Instant::now() < deadline {
        if vm.console_output().await.contains(needle) {
            return Some(start.elapsed());
        }
        tokio::time::sleep(Duration::from_micros(200)).await;
    }
    None
}

async fn one(elf: Arc<PathBuf>, opts: Arc<Options>, index: usize) -> Result<Sample, String> {
    let mut sample = Sample::default();

    let started = Instant::now();
    let config = VMConfig {
        name: format!("unikernel-cold-start-{index}"),
        vcpu_count: 1,
        memory_size: opts.memory_mb * 1024 * 1024,
        boot: Some(BootSource::multiboot(&*elf)),
        ..Default::default()
    };
    let vm = VM::new(config).map_err(|e| format!("VM::new: {e}"))?;
    let vm = Arc::new(vm);
    sample.new_vm = Some(started.elapsed());

    let started = Instant::now();
    vm.provision()
        .await
        .map_err(|e| format!("provision: {e}"))?;
    sample.provision = Some(started.elapsed());

    let started = Instant::now();
    vm.attach_vsock(GUEST_CID)
        .await
        .map_err(|e| format!("attach_vsock: {e}"))?;
    sample.channel = Some(started.elapsed());

    let started = Instant::now();
    vm.launch().await.map_err(|e| format!("launch: {e}"))?;
    sample.launch = Some(started.elapsed());
    let launched_at = Instant::now();

    let deadline = Instant::now() + opts.ready_timeout;
    sample.alive = wait_for(&vm, launched_at, "HYPERMACHINE RUST UNIKERNEL", deadline).await;
    // The console timestamp is cumulative, so the time to reach the driver is
    // what is left after the time to reach the banner.
    sample.ready = wait_for(&vm, launched_at, "vsock cid ", deadline)
        .await
        .map(|total| total - sample.alive.unwrap_or(Duration::ZERO));

    let ready_err = if sample.ready.is_none() {
        Some(format!(
            "guest did not print \"vsock cid \" within {:?}. console: {:?}",
            opts.ready_timeout,
            vm.console_output().await
        ))
    } else {
        None
    };

    if let Err(e) = vm.stop().await {
        tracing::warn!("could not stop the VM after measuring: {e}");
    }

    if let Some(e) = ready_err {
        return Err(e);
    }
    Ok(sample)
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn report(label: &str, mut values: Vec<Duration>) {
    if values.is_empty() {
        println!("  {label:<10} not measured");
        return;
    }
    values.sort();
    let total: Duration = values.iter().sum();
    let mean = total / values.len() as u32;
    println!(
        "  {label:<10} n={:<4} avg {:>8.2}ms  min {:>8.2}ms  P50 {:>8.2}ms  P95 {:>8.2}ms  \
         P99 {:>8.2}ms  max {:>8.2}ms",
        values.len(),
        ms(mean),
        ms(values[0]),
        ms(percentile(&values, 50.0)),
        ms(percentile(&values, 95.0)),
        ms(percentile(&values, 99.0)),
        ms(values[values.len() - 1]),
    );
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("off")),
        )
        .init();

    let opts = match parse_options().map(Arc::new) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("unikernel_cold_start: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    if cfg!(debug_assertions) {
        eprintln!(
            "unikernel_cold_start: this is a debug build of the harness (the guest is always \
             built --release). Rebuild the harness with --release too before trusting the number."
        );
    }

    let elf = match build_guest() {
        Ok(elf) => Arc::new(elf),
        Err(e) => {
            eprintln!("unikernel_cold_start: guest build failed -- {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    println!(
        "unikernel_cold_start: {} iteration(s) at concurrency {}, {} MiB guest memory",
        opts.iterations, opts.concurrency, opts.memory_mb
    );
    println!("  guest {}", elf.display());
    println!(
        "  comparing against CubeSandbox's published cold start: <60ms @ concurrency 1; \
         67ms avg / P95 90ms / P99 137ms @ concurrency 50"
    );

    match one(Arc::clone(&elf), Arc::clone(&opts), 0).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("\nunikernel_cold_start: could not boot the guest at all: {e}");
            eprintln!("A hypervisor backend is required: /dev/kvm on Linux.");
            return std::process::ExitCode::FAILURE;
        }
    }

    let mut samples = Vec::new();
    let mut failures = Vec::new();

    for batch in 0..opts.iterations {
        let mut set = tokio::task::JoinSet::new();
        for slot in 0..opts.concurrency {
            let index = batch * opts.concurrency + slot;
            let elf = Arc::clone(&elf);
            let opts = Arc::clone(&opts);
            set.spawn(async move { one(elf, opts, index).await });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(sample)) => samples.push(sample),
                Ok(Err(e)) => failures.push(e),
                Err(e) => failures.push(format!("the boot task itself failed: {e}")),
            }
        }
    }

    println!("\nphases");
    report("new_vm", samples.iter().filter_map(|s| s.new_vm).collect());
    report(
        "provision",
        samples.iter().filter_map(|s| s.provision).collect(),
    );
    report(
        "channel",
        samples.iter().filter_map(|s| s.channel).collect(),
    );
    report("launch", samples.iter().filter_map(|s| s.launch).collect());
    report("alive", samples.iter().filter_map(|s| s.alive).collect());
    report("vsock-up", samples.iter().filter_map(|s| s.ready).collect());

    println!("\ntotals");
    report(
        "running",
        samples.iter().filter_map(|s| s.to_running()).collect(),
    );
    let ready: Vec<Duration> = samples.iter().filter_map(|s| s.to_ready()).collect();
    report("ready", ready.clone());

    println!(
        "\n'ready' (build+channel+launch+time-to-\"vsock cid\") is the figure comparable to \
         CubeSandbox's published cold start -- both are 'time until the sandbox can be given \
         work,' not just 'time until the vCPU is running.'"
    );
    println!(
        "Memory overhead was not measured here -- see this example's own doc comment for why \
         printing one would not be honest."
    );

    if !failures.is_empty() {
        println!(
            "\n{} of {} boot(s) failed:",
            failures.len(),
            samples.len() + failures.len()
        );
        let mut seen = std::collections::BTreeSet::new();
        for failure in &failures {
            if seen.insert(failure.clone()) {
                println!("  {failure}");
            }
        }
        return std::process::ExitCode::FAILURE;
    }

    std::process::ExitCode::SUCCESS
}
