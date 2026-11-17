//! The shipped control plane refuses to start with no key unless told to.

/// The shipped binary itself: no key, no flag, no start.
#[test]
fn the_binary_exits_without_a_key_unless_allowed() {
    let binary = env!("CARGO_BIN_EXE_hv2-control-plane");
    let run = |extra: &[&str], key: bool| {
        let mut command = std::process::Command::new(binary);
        command
            .args(["--port", "0", "--proxy-port", "0"])
            .args(extra)
            .env_remove("HV2_API_KEY")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        if key {
            command.env("HV2_API_KEY", "a-key-for-the-test");
        }
        command.spawn().unwrap()
    };
    // Bounded: a binary that wrongly starts must fail this test, not hang it.
    let mut child = run(&[], false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("started with no key and no --allow-unauthenticated");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--allow-unauthenticated"), "{stderr}");
    // With the flag, or with a key, it starts and keeps running.
    for (extra, key) in [(&["--allow-unauthenticated"][..], false), (&[][..], true)] {
        let mut child = run(extra, key);
        std::thread::sleep(std::time::Duration::from_secs(2));
        let running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let output = child.wait_with_output().unwrap();
        assert!(
            running,
            "exited: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
