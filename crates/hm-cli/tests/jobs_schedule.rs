//! Test schedule persistence through separate invocations of the shipped CLI.
use serde_json::{json, Value};
use std::process::{Command, Output};

fn invoke(store: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hm"))
        .arg("jobs")
        .arg("--store")
        .arg(store)
        .arg("schedule")
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn schedule_cli_persists_and_pages_without_executing_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let spec = dir.path().join("schedule.json");
    std::fs::write(
        &spec,
        json!({"first_ms":100,"every_ms":10,"job":{"command":["program-that-must-not-be-run"]}})
            .to_string(),
    )
    .unwrap();
    let spec = spec.to_str().unwrap();
    assert_eq!(success(invoke(&store, &["list"])), json!([]));
    assert_eq!(
        success(invoke(&store, &["create", "test", spec])),
        json!({"id":"test"})
    );
    assert!(!invoke(&store, &["create", "test", spec]).status.success());
    assert_eq!(
        success(invoke(&store, &["list", "--limit", "1"])),
        json!(["test"])
    );
    assert_eq!(
        success(invoke(&store, &["list", "--after", "test"])),
        json!([])
    );
    assert_eq!(
        success(invoke(&store, &["status", "test"]))["publication_through_ms"],
        Value::Null
    );
    let batch = success(invoke(
        &store,
        &["publish", "test", "--now-ms", "135", "--limit", "2"],
    ));
    assert_eq!(batch.as_array().unwrap().len(), 2);
    assert_eq!(batch[1]["scheduled_ms"], 110);
    success(invoke(
        &store,
        &["publish", "test", "--now-ms", "135", "--limit", "2"],
    ));
    assert_eq!(
        success(invoke(&store, &["status", "test"]))["publication_through_ms"],
        130
    );
    let page = success(invoke(
        &store,
        &["occurrences", "test", "--after-ms", "110", "--limit", "1"],
    ));
    assert_eq!(page.as_array().unwrap().len(), 1);
    assert_eq!(page[0]["scheduled_ms"], 120);
    assert!(!invoke(
        &store,
        &["publish", "test", "--now-ms", "135", "--limit", "0"]
    )
    .status
    .success());
    assert!(!invoke(&store, &["status", "../escape"]).status.success());
    assert_eq!(std::fs::read_dir(store.join("queue")).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(store.join("jobs")).unwrap().count(), 0);
    assert_eq!(
        success(invoke(&store, &["cancel", "test"]))["cancelled"],
        true
    );
    assert_eq!(
        success(invoke(&store, &["status", "test"]))["cancelled"],
        true
    );
    assert!(!invoke(&store, &["publish", "test", "--now-ms", "140"])
        .status
        .success());
    assert!(invoke(&store, &["watch", "test", "--ticks", "1"])
        .status
        .success());
}

#[test]
fn automatic_publication_is_bounded_and_recovers_on_a_second_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let spec = dir.path().join("interval.json");
    std::fs::write(
        &spec,
        json!({"first_ms":0,"every_ms":1,"vm":{"sandbox_id":"guest-1","connection_profile":"local","timeout_secs":30},"job":{"command":["must-not-run"]}}).to_string(),
    )
    .unwrap();
    success(invoke(&store, &["create", "watch", spec.to_str().unwrap()]));
    for expected in [3, 7] {
        let output = invoke(
            &store,
            &[
                "watch",
                "watch",
                "--limit",
                "2",
                "--poll-ms",
                "1",
                "--ticks",
                "2",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let lines: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.as_array().unwrap().len() == 2));
        assert!(lines
            .iter()
            .all(|line| line[0]["vm"]["connection_profile"] == "local"));
        assert_eq!(
            success(invoke(&store, &["status", "watch"]))["publication_through_ms"],
            expected
        );
    }
    assert!(!invoke(&store, &["watch", "watch", "--ticks", "0"])
        .status
        .success());
    assert!(!invoke(
        &store,
        &["watch", "watch", "--poll-ms", "0", "--ticks", "1"]
    )
    .status
    .success());
    assert_eq!(std::fs::read_dir(store.join("queue")).unwrap().count(), 0);
}

async fn running_publisher_shutdown(signal: bool) {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let spec = dir.path().join("live.json");
    std::fs::write(
        &spec,
        json!({"first_ms":0,"every_ms":1,"job":{"command":["must-not-run"]}}).to_string(),
    )
    .unwrap();
    success(invoke(&store, &["create", "live", spec.to_str().unwrap()]));
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_hm"))
        .args(["jobs", "--store"])
        .arg(&store)
        .args([
            "schedule",
            "watch",
            "live",
            "--limit",
            "2",
            "--poll-ms",
            "10",
        ])
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), output.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Keep draining so a healthy publisher cannot block on a full stdout pipe.
    let drain =
        tokio::spawn(async move { tokio::io::copy(&mut output, &mut tokio::io::sink()).await });
    if signal {
        let status = Command::new("/bin/kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
    } else {
        success(invoke(&store, &["cancel", "live"]));
    }
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success(), "publisher did not exit cleanly: {status}");
    drain.await.unwrap().unwrap();
    let persisted = success(invoke(&store, &["status", "live"]));
    assert_eq!(persisted["cancelled"], !signal);
    let records = success(invoke(&store, &["occurrences", "live", "--limit", "1024"]));
    assert_eq!(
        records.as_array().unwrap().last().unwrap()["scheduled_ms"],
        persisted["publication_through_ms"]
    );
    assert_eq!(std::fs::read_dir(store.join("queue")).unwrap().count(), 0);
}

#[tokio::test]
async fn running_publisher_observes_external_schedule_cancellation() {
    running_publisher_shutdown(false).await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn running_publisher_handles_sigint_and_preserves_committed_records() {
    running_publisher_shutdown(true).await;
}
