//! Durable calendar publication diagnostic; no guest or competitor timing.
use hv2_jobs::{schedule::IntervalSchedule, Store};
use serde_json::json;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let first = 1793520000000_u64;
    let through = first + 2 * 86400000;
    let mut rows = Vec::new();
    for (zone, limit) in [("UTC", 1), ("UTC", 1000), ("America/Los_Angeles", 1), ("America/Los_Angeles", 1000)] {
        let schedule: IntervalSchedule = serde_json::from_value(json!({
            "first_ms": first, "cron": {"expression":"* * * * *","timezone":zone},
            "job":{"command":["true"]}}))?;
        let expected = schedule.due_occurrences(None, through, limit)?;
        let mut samples = Vec::new();
        for _ in 0..5 {
            let directory = tempfile::tempdir()?;
            let store = Store::open(directory.path())?;
            store.create_interval_schedule("publication", &schedule)?;
            let begin = Instant::now();
            let records = store.materialize_interval("publication", through, limit)?;
            samples.push(begin.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(records.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(), expected);
            let reopened = Store::open(directory.path())?;
            assert_eq!(reopened.interval_progress("publication")?, expected.last().copied());
            assert_eq!(reopened.committed_interval_occurrences("publication", None, 1024)?, records);
        }
        rows.push(json!({"timezone":zone,"limit":limit,"scheduled_utc_ms":expected,"publication_ms":samples}));
    }
    println!("{}", serde_json::to_string_pretty(&json!({"purpose":"durable calendar batch publication including file sync and progress commit; excludes guest execution and competitor measurement", "first_ms":first,"through_ms":through,"samples_per_case":5,"rows":rows}))?);
    Ok(())
}
