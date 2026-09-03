//! Measure how long it takes to get a usable sandbox, and where the time goes.
//!
//! CubeSandbox publishes a cold start of 60ms at single concurrency and, at 50
//! concurrent creations, 67ms average with P95 90ms and P99 137ms. Those are
//! the numbers to be measured against, and this repository had no comparable
//! figure of its own -- `vm_bench` measures guest-memory allocation and
//! snapshot serialisation, neither of which is a boot.
//!
//! So this exists to produce the number, in the same shape, before any claim
//! is made about it. It reports percentiles at whatever concurrency it is
//! given, and it decomposes the total, because "we are slower than 60ms" is
//! not actionable and "provisioning is 80% of it" is.
//!
//! # What it measures
//!
//! | phase | from | to |
//! | --- | --- | --- |
//! | `build` | nothing | a configured VM with a backend handle |
//! | `channel` | that | a vsock device attached |
//! | `launch` | that | the vCPU running guest code |
//! | `ready` | that | the guest agent answering a ping |
//!
//! `build + channel + launch` is what a caller waits for before the guest is
//! executing. `ready` is what a caller waits for before the sandbox is usable,
//! and it is the one comparable to a published cold-start figure -- a VM whose
//! vCPU is running but whose guest has not finished booting is not a sandbox
//! anyone can use.
//!
//! # What it will not do
//!
//! Print a number it did not measure. Without a hypervisor backend it says so
//! and exits non-zero; without a guest image it measures the phases it can and
//! says which one is missing. An absent measurement is reported as absent,
//! because a benchmark that silently degrades to timing less work is how a
//! project comes to believe it is fast.
//!
//! # Running it
//!
//! ```text
//! # Phases up to a running vCPU, no guest image needed.
//! cargo run --release -p hv2-agent --example cold_start -- --iterations 20
//!
//! # To a usable sandbox, which needs a kernel and an initramfs running
//! # hv2-guest-agentd.
//! HV2_KERNEL=/var/tmp/kbuild/bzImage HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run --release -p hv2-agent --example cold_start -- --iterations 20 --concurrency 50
//! ```
//!
//! Build with `--release`. A debug build measures the debug build, which is
//! not the thing anyone deploys, and reporting it as a cold start would
//! overstate the cost by a large and unstated factor.

use hv2_agent::{AgentVM, Capability, CapabilitySet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One creation's timings. `None` means the phase did not run.
#[derive(Debug, Default, Clone)]
struct Sample {
    build: Option<Duration>,
    channel: Option<Duration>,
    launch: Option<Duration>,
    ready: Option<Duration>,
}

impl Sample {
    /// Everything a caller waits for before the guest is executing.
    fn to_running(&self) -> Option<Duration> {
        Some(self.build? + self.channel? + self.launch?)
    }

    /// Everything a caller waits for before the sandbox is usable.
    fn to_usable(&self) -> Option<Duration> {
        Some(self.to_running()? + self.ready?)
    }
}

/// Context ID for the guest. 0 and 1 are reserved, 2 is the host.
const GUEST_CID: u64 = 3;

struct Options {
    iterations: usize,
    concurrency: usize,
    memory_gb: u64,
    cpu_cores: u32,
    kernel: Option<String>,
    initrd: Option<String>,
    ready_timeout: Duration,
}

fn parse_options() -> Result<Options, String> {
    let mut opts = Options {
        iterations: 10,
        concurrency: 1,
        memory_gb: 1,
        cpu_cores: 1,
        kernel: std::env::var("HV2_KERNEL").ok(),
        initrd: std::env::var("HV2_INITRD").ok(),
        ready_timeout: Duration::from_secs(30),
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        // The value of the current flag, consuming it. Taken by index rather
        // than through a closure so the loop counter is not borrowed across
        // the match arms.
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
            "--memory-gb" => opts.memory_gb = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--cpu-cores" => opts.cpu_cores = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--kernel" => opts.kernel = Some(value(&mut i)?),
            "--initrd" => opts.initrd = Some(value(&mut i)?),
            "--help" | "-h" => {
                println!(
                    "usage: cold_start [--iterations N] [--concurrency N] [--memory-gb N] \
                     [--cpu-cores N] [--kernel PATH] [--initrd PATH]"
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

/// Create one sandbox, timing each phase.
async fn one(opts: Arc<Options>, index: usize) -> Result<Sample, String> {
    let mut sample = Sample::default();

    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);

    let started = Instant::now();
    let builder = AgentVM::builder()
        .name(format!("cold-start-{index}"))
        .cpu_cores(opts.cpu_cores)
        .memory_gb(opts.memory_gb)
        .capabilities(capabilities);

    let builder = match (&opts.kernel, &opts.initrd) {
        (Some(kernel), initrd) => builder.boot_linux(
            kernel,
            initrd.as_ref(),
            "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0",
        ),
        (None, _) => builder,
    };

    let vm = builder
        .build()
        .await
        .map_err(|e| format!("building: {e}"))?;
    sample.build = Some(started.elapsed());

    let started = Instant::now();
    vm.attach_guest_channel(GUEST_CID)
        .await
        .map_err(|e| format!("attaching the guest channel: {e}"))?;
    sample.channel = Some(started.elapsed());

    let started = Instant::now();
    vm.launch().await.map_err(|e| format!("launching: {e}"))?;
    sample.launch = Some(started.elapsed());

    // Only meaningful with a guest that has an agent in it. Without one this
    // would measure the timeout, not the boot.
    if opts.kernel.is_some() {
        let started = Instant::now();
        vm.ping_guest(opts.ready_timeout)
            .await
            .map_err(|e| format!("waiting for the guest agent: {e}"))?;
        sample.ready = Some(started.elapsed());
    }

    Ok(sample)
}

/// Percentile by nearest rank, which is what a small sample supports.
///
/// Interpolating between two of twenty measurements invents precision the
/// sample does not have.
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
            eprintln!("cold_start: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    if cfg!(debug_assertions) {
        eprintln!(
            "cold_start: this is a debug build. The number below is not a cold start anyone \
             would deploy -- rebuild with --release."
        );
    }

    println!(
        "cold_start: {} iteration(s) at concurrency {}, {} vCPU, {} GiB",
        opts.iterations, opts.concurrency, opts.cpu_cores, opts.memory_gb
    );
    match (&opts.kernel, &opts.initrd) {
        (Some(k), Some(i)) => println!("  kernel {k}\n  initrd {i}"),
        (Some(k), None) => println!("  kernel {k}, no initramfs"),
        (None, _) => println!(
            "  no kernel: measuring up to a running vCPU only. Set HV2_KERNEL and HV2_INITRD \
             for the figure comparable to a published cold start."
        ),
    }

    // One creation first, so a missing backend is reported as itself rather
    // than as every iteration failing.
    match one(Arc::clone(&opts), 0).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("\ncold_start: could not create a sandbox at all: {e}");
            eprintln!(
                "No number is printed, because there is nothing to report. A hypervisor backend \
                 is required: /dev/kvm on Linux, or Windows Hypervisor Platform."
            );
            return std::process::ExitCode::FAILURE;
        }
    }

    let mut samples = Vec::new();
    let mut failures = Vec::new();

    for batch in 0..opts.iterations {
        // A JoinSet rather than a joined future: each creation runs as its own
        // task on the runtime, so the concurrency is real rather than a loop
        // that happens to be async and interleaves at await points.
        let mut set = tokio::task::JoinSet::new();
        for slot in 0..opts.concurrency {
            let index = batch * opts.concurrency + slot;
            let opts = Arc::clone(&opts);
            set.spawn(async move { one(opts, index).await });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(sample)) => samples.push(sample),
                Ok(Err(e)) => failures.push(e),
                Err(e) => failures.push(format!("the creation task itself failed: {e}")),
            }
        }
    }

    println!("\nphases");
    report(
        "build",
        samples.iter().filter_map(|s| s.build).collect::<Vec<_>>(),
    );
    report(
        "channel",
        samples.iter().filter_map(|s| s.channel).collect::<Vec<_>>(),
    );
    report(
        "launch",
        samples.iter().filter_map(|s| s.launch).collect::<Vec<_>>(),
    );
    report(
        "ready",
        samples.iter().filter_map(|s| s.ready).collect::<Vec<_>>(),
    );

    println!("\ntotals");
    report(
        "running",
        samples
            .iter()
            .filter_map(|s| s.to_running())
            .collect::<Vec<_>>(),
    );
    let usable: Vec<Duration> = samples.iter().filter_map(|s| s.to_usable()).collect();
    report("usable", usable.clone());

    if usable.is_empty() {
        println!(
            "\n'usable' is the figure comparable to a published cold start, and it was not \
             measured. Supply a kernel and an initramfs running hv2-guest-agentd."
        );
    }

    if !failures.is_empty() {
        println!(
            "\n{} of {} creation(s) failed:",
            failures.len(),
            samples.len() + failures.len()
        );
        // Distinct reasons only: fifty identical messages say no more than one.
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
