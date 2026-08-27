//! Print what this host can confine, and prove it by trying to break out.
//!
//! Run it on the machine you care about: the answer differs per kernel, per
//! container, and per cgroup delegation, which is the whole reason
//! `Sandbox::controls` is probed rather than assumed.

use hv2_sandbox::{Control, ProcessSandbox, Sandbox, SandboxCommand, SandboxSpec};

fn main() {
    let sandbox = ProcessSandbox::new();
    let controls = sandbox.controls();

    println!("backend: {}", sandbox.name());
    for control in Control::ALL {
        if controls.enforces(control) {
            println!("  ENFORCED    {control}");
        } else {
            println!(
                "  unavailable {control} -- {}",
                controls.reason(control).unwrap_or("no reason reported")
            );
        }
    }

    // Ask the workload itself what it sees. A sandbox that reports isolation
    // and hands over the host's view would pass every test that only reads
    // `controls()`.
    let mut spec = SandboxSpec::untrusted(64 * 1024 * 1024, std::time::Duration::from_secs(10));
    spec.best_effort = true;

    let checks: [(&str, &str, &[&str]); 3] = [
        ("pid namespace", "/bin/sh", &["-c", "echo pid=$$"]),
        (
            "network namespace",
            "/bin/sh",
            &["-c", "ip -o link 2>/dev/null | wc -l"],
        ),
        (
            "process table",
            "/bin/sh",
            &["-c", "ls /proc | grep -c '^[0-9]'"],
        ),
    ];

    for (what, program, args) in checks {
        let command = SandboxCommand::new(program).args(args.iter().copied());
        match sandbox.run(&command, &spec) {
            Ok(out) => println!("  {what}: {}", String::from_utf8_lossy(&out.stdout).trim()),
            Err(e) => println!("  {what}: FAILED {e}"),
        }
    }

    filesystem_check(&sandbox);
}

/// Ask a confined workload whether it can still reach a file outside its root.
///
/// Needs a root to pivot into, which no default can choose for the caller, so
/// it makes a throwaway one rather than being left out of the report.
#[cfg(target_os = "linux")]
fn filesystem_check(sandbox: &ProcessSandbox) {
    use hv2_sandbox::{FilesystemPolicy, NetworkPolicy};

    let base = std::env::temp_dir().join(format!("hv2-sandbox-probe-{}", std::process::id()));
    let root = base.join("root");
    let outside = base.join("host-only");
    if std::fs::create_dir_all(&root)
        .and_then(|()| std::fs::write(&outside, b"host\n"))
        .is_err()
    {
        println!("  filesystem: FAILED could not make a scratch root");
        return;
    }

    let spec = SandboxSpec {
        filesystem: FilesystemPolicy::Isolated {
            root: root.clone(),
            read_only: ["/bin", "/usr", "/lib", "/lib64", "/sbin"]
                .iter()
                .map(std::path::PathBuf::from)
                .filter(|path| path.exists())
                .collect(),
        },
        network: NetworkPolicy::Host,
        wall_clock: Some(std::time::Duration::from_secs(20)),
        ..SandboxSpec::default()
    };
    let script = format!(
        "if [ -e '{}' ]; then echo 'STILL VISIBLE'; else echo 'gone'; fi",
        outside.display()
    );
    let command = SandboxCommand::new("/bin/sh").args(["-c", &script]);

    match sandbox.run(&command, &spec) {
        Ok(out) => println!(
            "  a host file outside the root: {}",
            String::from_utf8_lossy(&out.stdout).trim()
        ),
        Err(e) => println!("  a host file outside the root: FAILED {e}"),
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// Nothing to report where no backend isolates a filesystem.
#[cfg(not(target_os = "linux"))]
fn filesystem_check(_sandbox: &ProcessSandbox) {}
