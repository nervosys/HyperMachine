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
}
