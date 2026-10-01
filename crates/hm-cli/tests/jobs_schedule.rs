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
    assert_eq!(
        success(invoke(&store, &["create", "test", spec])),
        json!({"id":"test"})
    );
    assert!(!invoke(&store, &["create", "test", spec]).status.success());
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
        json!({"first_ms":0,"every_ms":1,"job":{"command":["must-not-run"]}}).to_string(),
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
