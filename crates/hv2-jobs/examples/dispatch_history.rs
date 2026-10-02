//! Diagnostic selection cost over completed durable history; no VM execution.
use hv2_jobs::{
    dispatch::{DispatchClaim, DispatchCompletion},
    Store,
};
use serde_json::json;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = Vec::new();
    for (completed, pending) in [
        (0_u64, 1_u64),
        (100, 1),
        (1000, 1),
        (5000, 1),
        (0, 1000),
        (0, 5000),
    ] {
        let through = completed + pending - 1;
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        let schedule = serde_json::from_value(json!({"first_ms":0,"every_ms":1,
            "vm":{"sandbox_id":"fixture","connection_profile":"local","timeout_secs":30},
            "job":{"command":["true"]}}))?;
        store.create_interval_schedule("history", &schedule)?;
        while store.interval_progress("history")? != Some(through) {
            store.materialize_interval("history", through, 1024)?;
        }
        // Fixture-only preparation excludes sequential claim scans from timing.
        // These records use the real serialized claim/completion types.
        std::fs::create_dir_all(store.root().join("dispatch-claims"))?;
        std::fs::create_dir_all(store.root().join("dispatch-results"))?;
        let mut cursor = None;
        loop {
            let records = store.committed_interval_occurrences("history", cursor, 1024)?;
            if records.is_empty() {
                break;
            }
            for record in records {
                cursor = Some(record.scheduled_ms);
                if record.scheduled_ms >= completed {
                    continue;
                }
                let key = format!("history--{}", record.scheduled_ms);
                let token = format!("fixture-{}", record.scheduled_ms);
                let claim = DispatchClaim {
                    occurrence: record,
                    worker: "fixture".into(),
                    token: token.clone(),
                };
                let result = DispatchCompletion {
                    origin: hv2_jobs::dispatch::CompletionOrigin::Unknown,
                    claim_token: token,
                    exit_code: Some(0),
                    timed_out: false,
                    stdout: Some(String::new()),
                    stderr: Some(String::new()),
                    stdout_truncated: false,
                    stderr_truncated: false,
                };
                std::fs::write(
                    store.root().join("dispatch-claims").join(&key),
                    serde_json::to_vec(&claim)?,
                )?;
                std::fs::write(
                    store.root().join("dispatch-results").join(&key),
                    serde_json::to_vec(&result)?,
                )?;
            }
        }
        let mut samples = Vec::new();
        // Warm filesystem caches first; each measured scan reopens the store.
        assert_eq!(
            store.next_vm_occurrence("history")?.unwrap().scheduled_ms,
            completed
        );
        for _ in 0..9 {
            let reopened = Store::open(directory.path())?;
            let begin = Instant::now();
            let next = reopened.next_vm_occurrence("history")?.unwrap();
            let elapsed = begin.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(next.scheduled_ms, completed);
            samples.push(elapsed);
        }
        rows.push(json!({"completed_occurrences":completed,"pending_occurrences":pending,"selection_ms":samples}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"purpose":"warm-cache durable history selection diagnostic; no guest or competitor measurement","samples_per_size":9,"rows":rows})
        )?
    );
    Ok(())
}
