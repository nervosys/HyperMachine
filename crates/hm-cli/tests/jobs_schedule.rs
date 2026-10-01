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
}
