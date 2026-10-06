//! Can a booted Linux sandbox guest be restored instead of booted -- and how
//! fast?
//!
//! Boots the sandbox guest once, waits for its agent, snapshots it, and then
//! creates fresh VMs from the snapshot, checking each is a working guest and
//! not just a VM that did not crash: the agent answers, the timer interrupt
//! is still advancing (the PIT and PICs came back), a command runs, and the
//! wall clock is right after `set_guest_clock`.
//!
//! ```text
//! HV2_KERNEL=/var/tmp/kbuild/bzImage HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run --release -p hv2-agent --example linux_snapshot_restore
//! ```

use std::time::{Duration, Instant};

use hv2_agent::{AgentVM, Capability, CapabilitySet};

const CID: u64 = 100;
const RESTORES: usize = 5;

async fn build(kernel: &str, initrd: &str, name: &str) -> AgentVM {
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);
    let vm = AgentVM::builder()
        .name(name)
        .cpu_cores(1)
        .memory_gb(1)
        .capabilities(capabilities)
        .boot_linux(
            kernel,
            Some(initrd),
            format!(
                "console=ttyS0,115200 nokaslr rdinit=/init {} {}",
                // HV2_VERBOSE_GUEST=1 shows the guest kernel's log on the
                // console, which is how a restored guest's panic is read.
                if std::env::var_os("HV2_VERBOSE_GUEST").is_some() {
                    "loglevel=7"
                } else {
                    "quiet loglevel=0"
                },
                hv2_core::BootSource::MICROVM_FAST_BOOT_ARGS
            ),
        )
        .build()
        .await
        .expect("building the VM");
    vm.attach_guest_channel(CID).await.expect("guest channel");
    vm
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

async fn run(vm: &AgentVM, cmd: &str) -> String {
    let exec = vm
        .exec_in_guest(
            "/bin/sh",
            &["-c".to_string(), cmd.to_string()],
            Duration::from_secs(10),
        )
        .await
        .expect("exec");
    format!("{}{}", exec.stdout, exec.stderr).trim().to_string()
}

fn timer_ticks(interrupts: &str) -> u64 {
    interrupts
        .lines()
        .find(|l| l.trim_start().starts_with("0:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let (Ok(kernel), Ok(initrd)) = (std::env::var("HV2_KERNEL"), std::env::var("HV2_INITRD"))
    else {
        println!("skipped: set HV2_KERNEL and HV2_INITRD to the sandbox guest");
        return std::process::ExitCode::SUCCESS;
    };

    // The template: booted once.
    let template = build(&kernel, &initrd, "template").await;
    let started = Instant::now();
    template.launch().await.expect("launch");
    template
        .ping_guest(Duration::from_secs(15))
        .await
        .expect("the template's agent never answered");
    let boot = started.elapsed();
    println!(
        "boot          : {:>7.1} ms to a guest agent that answers",
        ms(boot)
    );
    println!(
        "template      : {}",
        run(&template, "echo warm; uname -r")
            .await
            .replace('\n', " ")
    );

    let path = std::env::temp_dir().join(format!("hv2-linux-{}.snap", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let taken = Instant::now();
    template.snapshot_to(&path).await.expect("snapshot");
    let image = path.with_file_name(format!(
        "{}.mem",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    // What the image occupies, as opposed to its length: it is sparse.
    #[cfg(unix)]
    let on_disk = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&image)
            .map(|m| m.blocks() * 512)
            .unwrap_or(0)
    };
    #[cfg(not(unix))]
    let on_disk = std::fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
    println!(
        "snapshot      : {:>7.1} ms; memory image {:.1} MiB allocated of {:.0} MiB",
        ms(taken.elapsed()),
        on_disk as f64 / (1024.0 * 1024.0),
        std::fs::metadata(&image).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0)
    );
    let _ = template.stop().await;

    let mut ready = Vec::new();
    let mut before_reseed = Vec::new();
    let mut after_reseed = Vec::new();
    for i in 0..RESTORES {
        let vm = build(&kernel, &initrd, &format!("restored-{i}")).await;
        let t0 = Instant::now();
        if let Err(e) = vm.launch_from_snapshot(&path).await {
            eprintln!("restore {i}   : FAILED -- {e}");
            return std::process::ExitCode::FAILURE;
        }
        let restored = t0.elapsed();
        if let Err(e) = vm.ping_guest(Duration::from_secs(5)).await {
            eprintln!("restore {i}   : the restored agent never answered: {e}");
            eprintln!("console       : {:?}", vm.console_output().await);
            return std::process::ExitCode::FAILURE;
        }
        let answered = t0.elapsed();
        ready.push(answered);
        // A second ping separates what the first paid once -- faulting the
        // agent's pages in from the image -- from a round trip's own cost.
        let again = Instant::now();
        let _ = vm.ping_guest(Duration::from_secs(5)).await;
        let second = again.elapsed();

        // What the guest's RNG gives before anything reseeds it: the state it
        // shares with every other clone of the same snapshot.
        let draw = "head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \\n'";
        before_reseed.push(run(&vm, draw).await);
        if let Err(e) = vm.after_restore(Duration::from_secs(5)).await {
            eprintln!("restore {i}   : resynchronising failed: {e}");
            eprintln!("console       : {:?}", vm.console_output().await);
            return std::process::ExitCode::FAILURE;
        }
        after_reseed.push(run(&vm, draw).await);
        let before = timer_ticks(&run(&vm, "cat /proc/interrupts").await);
        let date = run(&vm, "sleep 0.3; date +%s").await;
        let after = timer_ticks(&run(&vm, "cat /proc/interrupts").await);
        let host_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let guest_now: u64 = date.parse().unwrap_or(0);
        let arithmetic = run(&vm, "echo $((6*7))").await;
        println!(
            "restore {i}     : {:>6.1} ms to running, {:>6.1} ms to an answering agent \
             (next ping {:.1} ms); timer {before}->{after}, clock skew {}s, 6*7={arithmetic}",
            ms(restored),
            ms(answered),
            ms(second),
            guest_now.abs_diff(host_now)
        );
        if after <= before || guest_now.abs_diff(host_now) > 2 || arithmetic != "42" {
            eprintln!("restore {i}   : the guest is not healthy");
            return std::process::ExitCode::FAILURE;
        }
        let _ = vm.stop().await;
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&image);

    let distinct = |draws: &[String]| {
        draws
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    println!(
        "randomness    : {} distinct of {RESTORES} before the reseed, {} distinct after",
        distinct(&before_reseed),
        distinct(&after_reseed)
    );
    if distinct(&after_reseed) != RESTORES {
        eprintln!("randomness    : clones share RNG output after the reseed");
        return std::process::ExitCode::FAILURE;
    }

    ready.sort();
    let median = ready[ready.len() / 2];
    println!(
        "result        : restore-to-agent median {:.1} ms vs boot {:.1} ms ({:.1}x)",
        ms(median),
        ms(boot),
        boot.as_secs_f64() / median.as_secs_f64()
    );
    std::process::ExitCode::SUCCESS
}
