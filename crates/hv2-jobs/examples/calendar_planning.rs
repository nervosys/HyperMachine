//! Local planner diagnostic: no storage, guest, network or competitor timing.
use hv2_jobs::schedule::IntervalSchedule;
use serde_json::json;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let first =
        chrono::DateTime::parse_from_rfc3339("2026-11-01T08:00:00Z")?.timestamp_millis() as u64;
    let mut rows = Vec::new();
    for (expression, zone, limit, horizon_days) in [
        ("* * * * *", "UTC", 1, 2),
        ("* * * * *", "UTC", 1000, 2),
        ("* * * * *", "America/Los_Angeles", 1, 2),
        ("* * * * *", "America/Los_Angeles", 1000, 2),
        ("30 1 * * *", "America/Los_Angeles", 1000, 366),
        ("0 0 29 2 *", "America/Los_Angeles", 1, 8 * 366),
    ] {
        let schedule: IntervalSchedule = serde_json::from_value(json!({
            "first_ms":first,"cron":{"expression":expression,"timezone":zone},
            "job":{"command":["true"]}}))?;
        let through = first + horizon_days * 86_400_000;
        let expected = schedule.due_occurrences(None, through, limit)?;
        assert!(!expected.is_empty() && expected.len() <= limit);
        assert!(expected.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(expected
            .iter()
            .all(|time| *time >= first && *time <= through));
        if expression == "* * * * *" {
            assert_eq!(expected.len(), limit);
        }
        let mut samples = Vec::new();
        for _ in 0..9 {
            let begin = Instant::now();
            let actual = schedule.due_occurrences(None, through, limit)?;
            samples.push(begin.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(actual, expected);
        }
        rows.push(json!({"expression":expression,"timezone":zone,"limit":limit,
            "horizon_days":horizon_days,"occurrences":expected.len(),
            "first_occurrence_ms":expected[0],"last_occurrence_ms":expected.last(),"scheduled_utc_ms":expected,"planning_ms":samples}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
        "purpose":"local calendar planner diagnostic; no store, guest, network or competitor measurement",
        "first_ms":first,"timezone_database_version":chrono_tz::IANA_TZDB_VERSION,
        "samples_per_case":9,"rows":rows}))?
    );
    Ok(())
}
