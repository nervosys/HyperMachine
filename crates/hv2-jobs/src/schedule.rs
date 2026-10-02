//! Immutable schedule and occurrence records. These do not dispatch jobs yet.
//! Publication uses an exclusive hard link to a completely written file, so
//! racing publishers cannot expose partial JSON or replace the winning record.

use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::{JobError, JobSpec, Result, Store};

/// Immutable recurrence, retaining the historical type name for compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalSchedule {
    pub first_ms: u64,
    #[serde(default)]
    pub every_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<CronCalendar>,
    #[serde(default)]
    pub missed_policy: MissedOccurrencePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm: Option<VmScheduleTarget>,
    pub job: JobSpec,
}

/// Calendar recurrence pinned to the timezone rules used to create its history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronCalendar {
    pub expression: String,
    pub timezone: String,
    #[serde(default)]
    pub tzdb_version: Option<String>,
}

/// Guest destination. Connection profiles are resolved by the operator's
/// dispatcher; credentials and arbitrary endpoint URLs are not stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmScheduleTarget {
    pub sandbox_id: String,
    pub connection_profile: String,
    pub timeout_secs: u64,
}

/// How a scheduler selects overdue occurrences after downtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedOccurrencePolicy {
    /// Return oldest occurrences first, in bounded batches.
    #[default]
    CatchUp,
    /// Select only the latest due occurrence, explicitly skipping older ones.
    Coalesce,
}

impl IntervalSchedule {
    /// Plan due occurrences after an exclusive processed-through watermark.
    /// The caller must persist the work represented by its watermark before
    /// advancing it. This method neither publishes records nor dispatches jobs.
    /// Backward clock movement produces no occurrences already behind it.
    pub fn due_occurrences(
        &self,
        after_ms: Option<u64>,
        now_ms: u64,
        limit: usize,
    ) -> Result<Vec<u64>> {
        self.validate()?;
        if !(1..=1024).contains(&limit) {
            return Err(JobError::InvalidSpec(
                "occurrence batch limit must be 1-1024".into(),
            ));
        }
        let start = match after_ms {
            Some(after) => match after.checked_add(1) {
                Some(start) => start,
                None => return Ok(Vec::new()),
            },
            None => self.first_ms,
        };
        let Some(first) = self.at_or_after(start)? else {
            return Ok(Vec::new());
        };
        if first > now_ms {
            return Ok(Vec::new());
        }
        if self.missed_policy == MissedOccurrencePolicy::Coalesce {
            if let Some(cron) = &self.cron {
                let expression: crate::cron::CronExpression = cron.expression.parse()?;
                return Ok(expression
                    .at_or_before_in_timezone(now_ms, &cron.timezone)?
                    .filter(|last| *last >= first)
                    .into_iter()
                    .collect());
            }
            let offset = (now_ms - self.first_ms) / self.every_ms * self.every_ms;
            return Ok(vec![self.first_ms + offset]);
        }
        if limit == 1 {
            return Ok(vec![first]);
        }
        if let Some(cron) = &self.cron {
            let expression: crate::cron::CronExpression = cron.expression.parse()?;
            return expression.bounded_occurrences(first, now_ms, &cron.timezone, limit);
        }
        let mut due = Vec::with_capacity(limit);
        let mut current = first;
        loop {
            due.push(current);
            if due.len() == limit {
                break;
            }
            let next = current.checked_add(self.every_ms);
            match next {
                Some(next) if next <= now_ms => current = next,
                _ => break,
            }
        }
        Ok(due)
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(cron) = &self.cron {
            if self.every_ms != 0 {
                return Err(JobError::InvalidSpec(
                    "choose cron or every_ms, not both".into(),
                ));
            }
            let _: crate::cron::CronExpression = cron.expression.parse()?;
            cron.timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|_| JobError::InvalidSpec("unknown cron timezone".into()))?;
            if cron
                .tzdb_version
                .as_deref()
                .is_some_and(|version| version != chrono_tz::IANA_TZDB_VERSION)
            {
                return Err(JobError::InvalidSpec(
                    "cron timezone database version differs from this build".into(),
                ));
            }
            use chrono::Datelike;
            let first = i64::try_from(self.first_ms)
                .ok()
                .and_then(chrono::DateTime::from_timestamp_millis);
            if first.is_none_or(|value| !(1970..=9999).contains(&value.year())) {
                return Err(JobError::InvalidSpec(
                    "cron first_ms is outside supported calendar years".into(),
                ));
            }
        } else if self.every_ms == 0 {
            return Err(JobError::InvalidSpec(
                "schedule interval must be positive".into(),
            ));
        }
        if let Some(vm) = &self.vm {
            check_schedule_id(&vm.connection_profile)?;
            if vm.sandbox_id.is_empty()
                || vm.sandbox_id.len() > 128
                || !vm
                    .sandbox_id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
                || !(1..=86_400).contains(&vm.timeout_secs)
            {
                return Err(JobError::InvalidSpec("VM target requires a sandbox ID of 1-128 ASCII letters, digits, hyphens or underscores and timeout 1-86400 seconds".into()));
            }
            if self.job.sandbox != crate::SandboxSettings::default()
                || self.job.graceful_stop.is_some()
            {
                return Err(JobError::InvalidSpec(
                    "VM schedules cannot use host sandbox limits or host graceful-stop files"
                        .into(),
                ));
            }
            if self.job.command.iter().any(|arg| arg.contains('\0')) {
                return Err(JobError::InvalidSpec(
                    "VM command arguments must not contain NUL".into(),
                ));
            }
            if self.job.workdir.as_ref().is_some_and(|path| {
                path.to_str()
                    .is_none_or(|path| !path.starts_with('/') || path.contains('\0'))
            }) {
                return Err(JobError::InvalidSpec(
                    "VM workdir must be an absolute UTF-8 guest path".into(),
                ));
            }
        }
        self.job.validate()
    }

    /// The first occurrence at or after `time_ms`; None means overflow.
    pub fn at_or_after(&self, time_ms: u64) -> Result<Option<u64>> {
        self.validate()?;
        if let Some(cron) = &self.cron {
            let expression: crate::cron::CronExpression = cron.expression.parse()?;
            return expression.at_or_after_in_timezone(time_ms.max(self.first_ms), &cron.timezone);
        }
        let delta = time_ms.saturating_sub(self.first_ms);
        let intervals = delta / self.every_ms;
        let intervals = if delta.is_multiple_of(self.every_ms) {
            intervals
        } else {
            match intervals.checked_add(1) {
                Some(n) => n,
                None => return Ok(None),
            }
        };
        Ok(intervals
            .checked_mul(self.every_ms)
            .and_then(|offset| self.first_ms.checked_add(offset)))
    }
}

/// A stable occurrence identity and the immutable job configuration it uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub schedule_id: String,
    pub scheduled_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm: Option<VmScheduleTarget>,
    pub job: JobSpec,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgressStep {
    through_ms: u64,
    #[serde(default)]
    cancelled: bool,
}

fn progress_key(id: &str, after_ms: Option<u64>) -> String {
    format!(
        "{id}--{}",
        after_ms.map_or_else(|| "start".into(), |n| n.to_string())
    )
}

fn check_schedule_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(JobError::InvalidSpec(
            "schedule ID requires 1-64 ASCII letters, digits or hyphens".into(),
        ));
    }
    Ok(())
}

impl Store {
    /// List a bounded page of immutable schedule names in lexical order.
    /// Interrupted publication temporaries are excluded. The scan visits the
    /// directory but retains at most `limit` names in memory.
    pub fn interval_schedule_ids(&self, after: Option<&str>, limit: usize) -> Result<Vec<String>> {
        if !(1..=1024).contains(&limit) {
            return Err(JobError::InvalidSpec(
                "schedule page limit must be 1-1024".into(),
            ));
        }
        if let Some(after) = after {
            check_schedule_id(after)?;
        }
        let entries = match std::fs::read_dir(self.root().join("schedules")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut names = std::collections::BTreeSet::new();
        for entry in entries {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| JobError::Corrupt("non-UTF8 schedule filename".into()))?;
            if name
                .strip_prefix('.')
                .and_then(|n| n.strip_suffix(".tmp"))
                .is_some_and(|n| uuid::Uuid::parse_str(n).is_ok())
            {
                continue;
            }
            if check_schedule_id(&name).is_err() || !entry.file_type()?.is_file() {
                return Err(JobError::Corrupt(format!("invalid schedule entry {name}")));
            }
            if after.is_some_and(|after| name.as_str() <= after) {
                continue;
            }
            names.insert(name);
            if names.len() > limit {
                names.pop_last();
            }
        }
        Ok(names.into_iter().collect())
    }

    /// Stop future publication without erasing committed occurrence history.
    /// Cancellation competes for the same exclusive edge as publication.
    /// If a publication wins first, reload and cancel its successor edge.
    pub fn cancel_interval_schedule(&self, id: &str) -> Result<()> {
        for _ in 0..32 {
            let (after, cancelled) = self.interval_progress_state(id)?;
            if cancelled {
                return Ok(());
            }
            match self.publish_schedule_record(
                "schedule-progress",
                &progress_key(id, after),
                &ProgressStep {
                    through_ms: after.unwrap_or(0),
                    cancelled: true,
                },
            ) {
                Ok(()) => return Ok(()),
                Err(JobError::Conflict(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(JobError::Conflict(
            "schedule progress kept changing; retry cancellation".into(),
        ))
    }

    pub fn interval_schedule_cancelled(&self, id: &str) -> Result<bool> {
        Ok(self.interval_progress_state(id)?.1)
    }

    /// Recover a bounded page from the committed publication chain. The
    /// exclusive timestamp cursor is the last record consumed by the caller.
    /// Uncommitted records, including losing coalesced batches, are excluded.
    /// Consumption/dispatch acknowledgements are not persisted by this read.
    pub fn committed_interval_occurrences(
        &self,
        id: &str,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<Vec<Occurrence>> {
        if !(1..=1024).contains(&limit) {
            return Err(JobError::InvalidSpec(
                "occurrence page limit must be 1-1024".into(),
            ));
        }
        let schedule = self.interval_schedule(id)?;
        let mut after = None;
        let mut records = Vec::with_capacity(limit);
        loop {
            let key = progress_key(id, after);
            let step: ProgressStep =
                match crate::read_json(&self.root().join("schedule-progress").join(&key), &key) {
                    Ok(step) => step,
                    Err(JobError::NotFound(_)) => return Ok(records),
                    Err(error) => return Err(error),
                };
            if step.cancelled {
                return Ok(records);
            }
            let selected = schedule.due_occurrences(after, step.through_ms, 1024)?;
            if selected.last().copied() != Some(step.through_ms) {
                return Err(JobError::Corrupt(format!("invalid progress step {key}")));
            }
            for scheduled_ms in selected {
                if cursor.is_some_and(|cursor| scheduled_ms <= cursor) {
                    continue;
                }
                let key = format!("{id}--{scheduled_ms}");
                let record: Occurrence =
                    crate::read_json(&self.root().join("occurrences").join(&key), &key)?;
                let mut job = schedule.job.clone();
                job.not_before_ms = Some(scheduled_ms);
                if record
                    != (Occurrence {
                        schedule_id: id.into(),
                        scheduled_ms,
                        vm: schedule.vm.clone(),
                        job,
                    })
                {
                    return Err(JobError::Corrupt(format!(
                        "occurrence {key} disagrees with schedule"
                    )));
                }
                records.push(record);
                if records.len() == limit {
                    return Ok(records);
                }
            }
            after = Some(step.through_ms);
        }
    }

    /// Publish one bounded batch of due occurrences and commit its progress.
    /// Existing records from an interrupted publication are reconciled.
    /// Concurrent progress changes return Conflict: reload and retry rather
    /// than dispatching records from a failed batch. This does not run jobs.
    pub fn materialize_interval(
        &self,
        id: &str,
        now_ms: u64,
        limit: usize,
    ) -> Result<Vec<Occurrence>> {
        let schedule = self.interval_schedule(id)?;
        let (expected, cancelled) = self.interval_progress_state(id)?;
        if cancelled {
            return Err(JobError::Conflict("schedule is cancelled".into()));
        }
        let selected = schedule.due_occurrences(expected, now_ms, limit)?;
        let mut records = Vec::with_capacity(selected.len());
        for scheduled_ms in selected {
            records.push(self.record_selected_occurrence(id, scheduled_ms, &schedule)?);
        }
        if let Some(last) = records.last() {
            self.advance_interval_progress(id, expected, last.scheduled_ms)?;
        }
        Ok(records)
    }

    /// Recover the last committed occurrence-publication watermark. This is
    /// not a guest execution or dispatch acknowledgement. Immutable steps
    /// form a chain; a racing publisher cannot replace an existing edge.
    pub fn interval_progress(&self, id: &str) -> Result<Option<u64>> {
        Ok(self.interval_progress_state(id)?.0)
    }

    fn interval_progress_state(&self, id: &str) -> Result<(Option<u64>, bool)> {
        let schedule = self.interval_schedule(id)?;
        let mut after = None;
        loop {
            let key = progress_key(id, after);
            let step: ProgressStep =
                match crate::read_json(&self.root().join("schedule-progress").join(&key), &key) {
                    Ok(step) => step,
                    Err(JobError::NotFound(_)) => return Ok((after, false)),
                    Err(error) => return Err(error),
                };
            if step.cancelled {
                return Ok((after, true));
            }
            if after.is_some_and(|previous| step.through_ms <= previous)
                || schedule.at_or_after(step.through_ms)? != Some(step.through_ms)
            {
                return Err(JobError::Corrupt(format!("invalid progress step {key}")));
            }
            after = Some(step.through_ms);
        }
    }

    /// Commit a bounded publication batch with compare-and-set semantics.
    /// Every selected occurrence must already have its complete record. A
    /// failed or interrupted commit can be retried after reading progress.
    /// Coalescing intentionally omits older times; catch-up cannot skip them.
    pub fn advance_interval_progress(
        &self,
        id: &str,
        expected: Option<u64>,
        through_ms: u64,
    ) -> Result<()> {
        let schedule = self.interval_schedule(id)?;
        let (current, cancelled) = self.interval_progress_state(id)?;
        if cancelled || current != expected {
            return Err(JobError::Conflict(
                "schedule progress changed; reload before advancing".into(),
            ));
        }
        let selected = schedule.due_occurrences(expected, through_ms, 1024)?;
        if selected.last().copied() != Some(through_ms) {
            return Err(JobError::InvalidSpec(
                "progress must end at a selected occurrence within one bounded batch".into(),
            ));
        }
        for scheduled_ms in selected {
            let key = format!("{id}--{scheduled_ms}");
            let record: Occurrence =
                crate::read_json(&self.root().join("occurrences").join(&key), &key)?;
            let mut job = schedule.job.clone();
            job.not_before_ms = Some(scheduled_ms);
            if record
                != (Occurrence {
                    schedule_id: id.into(),
                    scheduled_ms,
                    vm: schedule.vm.clone(),
                    job,
                })
            {
                return Err(JobError::Corrupt(format!(
                    "occurrence {key} disagrees with schedule"
                )));
            }
        }
        self.publish_schedule_record(
            "schedule-progress",
            &progress_key(id, expected),
            &ProgressStep {
                through_ms,
                cancelled: false,
            },
        )
    }

    /// Create an immutable interval schedule. Existing names are conflicts.
    pub fn create_interval_schedule(&self, id: &str, schedule: &IntervalSchedule) -> Result<()> {
        check_schedule_id(id)?;
        schedule.validate()?;
        let mut schedule = schedule.clone();
        if let Some(cron) = &mut schedule.cron {
            cron.tzdb_version = Some(chrono_tz::IANA_TZDB_VERSION.into());
        }
        self.publish_schedule_record("schedules", id, &schedule)
    }

    pub fn interval_schedule(&self, id: &str) -> Result<IntervalSchedule> {
        check_schedule_id(id)?;
        let schedule: IntervalSchedule =
            crate::read_json(&self.root().join("schedules").join(id), id)?;
        if schedule
            .cron
            .as_ref()
            .is_some_and(|cron| cron.tzdb_version.is_none())
        {
            return Err(JobError::Corrupt(
                "persisted cron is missing its timezone database version".into(),
            ));
        }
        schedule.validate()?;
        Ok(schedule)
    }

    /// Publish exactly one record for this schedule/time pair. Repeating the
    /// operation returns the original record; it does not submit a queue job.
    pub fn record_interval_occurrence(&self, id: &str, scheduled_ms: u64) -> Result<Occurrence> {
        let schedule = self.interval_schedule(id)?;
        if schedule.at_or_after(scheduled_ms)? != Some(scheduled_ms) {
            return Err(JobError::InvalidSpec(
                "time is not an occurrence of this schedule".into(),
            ));
        }
        self.record_selected_occurrence(id, scheduled_ms, &schedule)
    }

    // Called only with timestamps selected or individually validated against
    // the loaded immutable schedule. Existing records still require equality.
    fn record_selected_occurrence(
        &self,
        id: &str,
        scheduled_ms: u64,
        schedule: &IntervalSchedule,
    ) -> Result<Occurrence> {
        let mut job = schedule.job.clone();
        job.not_before_ms = Some(scheduled_ms);
        let occurrence = Occurrence {
            schedule_id: id.into(),
            scheduled_ms,
            vm: schedule.vm.clone(),
            job,
        };
        let key = format!("{id}--{scheduled_ms}");
        match self.publish_schedule_record("occurrences", &key, &occurrence) {
            Ok(()) => Ok(occurrence),
            Err(JobError::Conflict(_)) => {
                let existing: Occurrence =
                    crate::read_json(&self.root().join("occurrences").join(&key), &key)?;
                if existing != occurrence {
                    return Err(JobError::Corrupt(format!(
                        "occurrence {key} disagrees with schedule"
                    )));
                }
                Ok(existing)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn publish_schedule_record(
        &self,
        directory: &str,
        key: &str,
        value: &impl Serialize,
    ) -> Result<()> {
        self.publish_schedule_record_with_sync(directory, key, value, sync_schedule_directory)
    }

    // Per-call injection keeps failure tests independent of other publishers.
    fn publish_schedule_record_with_sync(
        &self,
        directory: &str,
        key: &str,
        value: &impl Serialize,
        sync_directory: impl Fn(&std::path::Path) -> Result<()>,
    ) -> Result<()> {
        let dir = self.root().join(directory);
        std::fs::create_dir_all(&dir)?;
        // Persist the schedule subdirectory before acknowledging records in it.
        sync_directory(self.root())?;
        let temp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            let bytes = serde_json::to_vec(value).map_err(|e| JobError::Corrupt(e.to_string()))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            let published = std::fs::hard_link(&temp, dir.join(key));
            if published.is_ok()
                || published
                    .as_ref()
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists)
            {
                // Also flush on replay: a previous publisher may have linked
                // the file but failed or exited before syncing the directory.
                sync_directory(&dir)?;
            }
            match published {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(JobError::Conflict(
                    format!("{directory}/{key} already exists"),
                )),
                Err(e) => Err(e.into()),
            }
        })();
        let _ = std::fs::remove_file(temp);
        result
    }
}

// Rust exposes directory handles suitable for fsync on Unix. Other platforms
// retain the existing file-sync contract; no directory durability is claimed.
fn sync_schedule_directory(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_sync_failures_do_not_acknowledge_or_replace_records() {
        for directory in [
            "schedules",
            "occurrences",
            "schedule-progress",
            "dispatch-claims",
            "dispatch-results",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(dir.path()).unwrap();
            let value = serde_json::json!({"immutable":"original"});
            let final_path = dir.path().join(directory).join("record");
            let fault = || JobError::Io(std::io::Error::other("injected directory sync failure"));
            // Failure before linking must leave no published record.
            assert!(matches!(
                store.publish_schedule_record_with_sync(directory, "record", &value, |_| Err(
                    fault()
                )),
                Err(JobError::Io(_))
            ));
            assert!(!final_path.exists());
            let calls = std::cell::RefCell::new(Vec::new());
            // Failure after linking is ambiguous to the caller, with complete
            // immutable bytes present and no success acknowledgement.
            assert!(matches!(
                store.publish_schedule_record_with_sync(directory, "record", &value, |path| {
                    calls.borrow_mut().push(path.to_path_buf());
                    if path == store.root() {
                        Ok(())
                    } else {
                        Err(fault())
                    }
                }),
                Err(JobError::Io(_))
            ));
            assert_eq!(
                *calls.borrow(),
                vec![store.root().to_path_buf(), store.root().join(directory)]
            );
            let original = std::fs::read(&final_path).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&original).unwrap(),
                value
            );
            let changed = serde_json::json!({"immutable":"replacement"});
            // A replay still flushes, and propagates a second sync failure.
            assert!(matches!(
                store.publish_schedule_record_with_sync(directory, "record", &changed, |path| {
                    if path == store.root() {
                        Ok(())
                    } else {
                        Err(fault())
                    }
                }),
                Err(JobError::Io(_))
            ));
            let replay_calls = std::cell::RefCell::new(Vec::new());
            assert!(matches!(
                store.publish_schedule_record_with_sync(directory, "record", &changed, |path| {
                    replay_calls.borrow_mut().push(path.to_path_buf());
                    sync_schedule_directory(path)
                }),
                Err(JobError::Conflict(_))
            ));
            assert_eq!(
                *replay_calls.borrow(),
                vec![store.root().to_path_buf(), store.root().join(directory)]
            );
            assert_eq!(std::fs::read(final_path).unwrap(), original);
            assert_eq!(
                std::fs::read_dir(store.root().join(directory))
                    .unwrap()
                    .count(),
                1
            );
        }
    }

    fn schedule() -> IntervalSchedule {
        IntervalSchedule {
            first_ms: 100,
            every_ms: 10,
            cron: None,
            missed_policy: MissedOccurrencePolicy::CatchUp,
            vm: None,
            job: serde_json::from_value(serde_json::json!({"command":["echo","ok"]})).unwrap(),
        }
    }

    #[test]
    fn calendar_publication_recovers_folds_coalesces_and_pins_rules() {
        let utc = |value: &str| {
            chrono::DateTime::parse_from_rfc3339(value)
                .unwrap()
                .timestamp_millis() as u64
        };
        let first = utc("2026-11-01T08:00:00Z");
        let a = utc("2026-11-01T08:30:00Z");
        let b = utc("2026-11-01T09:30:00Z");
        let now = utc("2026-11-01T10:00:00Z");
        let mut calendar: IntervalSchedule = serde_json::from_value(serde_json::json!({
            "first_ms":first,"cron":{"expression":"30 1 * * *","timezone":"America/Los_Angeles"},
            "vm":{"sandbox_id":"guest","connection_profile":"local","timeout_secs":30},
            "job":{"command":["true"]}}))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("calendar", &calendar)
            .unwrap();
        assert_eq!(
            store
                .interval_schedule("calendar")
                .unwrap()
                .cron
                .unwrap()
                .tzdb_version
                .as_deref(),
            Some(chrono_tz::IANA_TZDB_VERSION)
        );
        store.record_interval_occurrence("calendar", a).unwrap(); // interrupted publication
        assert_eq!(
            store.materialize_interval("calendar", now, 1).unwrap()[0].scheduled_ms,
            a
        );
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(
            reopened.materialize_interval("calendar", now, 1).unwrap()[0].scheduled_ms,
            b
        );
        assert!(reopened
            .materialize_interval("calendar", a, 1)
            .unwrap()
            .is_empty());
        let times = |name| {
            reopened
                .committed_interval_occurrences(name, None, 10)
                .unwrap()
                .into_iter()
                .map(|r| r.scheduled_ms)
                .collect::<Vec<_>>()
        };
        assert_eq!(times("calendar"), vec![a, b]);
        reopened.cancel_interval_schedule("calendar").unwrap();
        assert!(reopened.materialize_interval("calendar", now, 1).is_err());
        assert_eq!(times("calendar"), vec![a, b]);
        calendar.missed_policy = MissedOccurrencePolicy::Coalesce;
        assert_eq!(calendar.due_occurrences(None, b - 1, 10).unwrap(), vec![a]);
        assert!(calendar
            .due_occurrences(Some(a), b - 1, 10)
            .unwrap()
            .is_empty());
        store
            .create_interval_schedule("coalesce", &calendar)
            .unwrap();
        assert_eq!(
            store.materialize_interval("coalesce", now, 10).unwrap()[0].scheduled_ms,
            b
        );
        assert_eq!(times("coalesce"), vec![b]);
        calendar.every_ms = 1;
        assert!(store.create_interval_schedule("both", &calendar).is_err());
        calendar.every_ms = 0;
        calendar.cron.as_mut().unwrap().tzdb_version = Some("different".into());
        assert!(store
            .create_interval_schedule("wrong-version", &calendar)
            .is_err());
        let mut persisted = store.interval_schedule("coalesce").unwrap();
        persisted.cron.as_mut().unwrap().tzdb_version = None;
        std::fs::write(
            store.root().join("schedules/coalesce"),
            serde_json::to_vec(&persisted).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.interval_schedule("coalesce"),
            Err(JobError::Corrupt(_))
        ));
    }

    #[test]
    fn racing_calendar_publishers_commit_one_fold_history() {
        let first = chrono::DateTime::parse_from_rfc3339("2026-11-01T08:00:00Z")
            .unwrap()
            .timestamp_millis() as u64;
        let calendar = serde_json::from_value(serde_json::json!({"first_ms":first,
            "cron":{"expression":"30 1 * * *","timezone":"America/Los_Angeles"},
            "job":{"command":["true"]}}))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("race-calendar", &calendar)
            .unwrap();
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        store.materialize_interval("race-calendar", first + 2 * 3_600_000, 2)
                    })
                })
                .collect();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert!(results
                .iter()
                .any(|result| result.as_ref().is_ok_and(|rows| rows.len() == 2)));
            assert!(results
                .iter()
                .all(|result| result.is_ok() || matches!(result, Err(JobError::Conflict(_)))));
        });
        let rows = Store::open(dir.path())
            .unwrap()
            .committed_interval_occurrences("race-calendar", None, 10)
            .unwrap();
        assert_eq!(
            rows.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![first + 1_800_000, first + 5_400_000]
        );
    }

    #[test]
    fn vm_target_survives_publication_and_refuses_host_only_controls() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut s = schedule();
        s.vm = Some(VmScheduleTarget {
            sandbox_id: "sandbox_1".into(),
            connection_profile: "production".into(),
            timeout_secs: 30,
        });
        s.job.workdir = Some("/workspace".into());
        store.create_interval_schedule("guest", &s).unwrap();
        store.materialize_interval("guest", 100, 1).unwrap();
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(reopened.interval_schedule("guest").unwrap(), s);
        let records = reopened
            .committed_interval_occurrences("guest", None, 1)
            .unwrap();
        assert_eq!(records[0].vm, s.vm);
        assert_eq!(records[0].job.workdir, s.job.workdir);
        s.job.sandbox.memory = Some("1G".into());
        assert!(s.validate().is_err());
        s.job.sandbox = crate::SandboxSettings::default();
        s.job.workdir = Some("relative".into());
        assert!(s.validate().is_err());
        s.job.workdir = None;
        s.vm.as_mut().unwrap().connection_profile = "https://endpoint/?secret=key".into();
        assert!(s.validate().is_err());
        s.vm.as_mut().unwrap().connection_profile = "production".into();
        s.vm.as_mut().unwrap().timeout_secs = 0;
        assert!(s.validate().is_err());
    }

    #[test]
    fn schedule_discovery_is_paged_and_excludes_interrupted_temporaries() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(store.interval_schedule_ids(None, 2).unwrap().is_empty());
        for id in ["z", "a", "middle"] {
            store.create_interval_schedule(id, &schedule()).unwrap();
        }
        let temp = dir
            .path()
            .join("schedules")
            .join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(temp, b"unfinished").unwrap();
        assert_eq!(
            store.interval_schedule_ids(None, 2).unwrap(),
            vec!["a", "middle"]
        );
        assert_eq!(
            store.interval_schedule_ids(Some("middle"), 2).unwrap(),
            vec!["z"]
        );
        store.cancel_interval_schedule("a").unwrap();
        assert_eq!(store.interval_schedule_ids(None, 1).unwrap(), vec!["a"]);
        assert!(store.interval_schedule_ids(None, 0).is_err());
        assert!(store.interval_schedule_ids(Some("../outside"), 1).is_err());
    }

    #[test]
    fn cancellation_preserves_history_and_blocks_future_publication() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("cancel", &schedule())
            .unwrap();
        store.materialize_interval("cancel", 120, 3).unwrap();
        store.cancel_interval_schedule("cancel").unwrap();
        let reopened = Store::open(dir.path()).unwrap();
        assert!(reopened.interval_schedule_cancelled("cancel").unwrap());
        reopened.cancel_interval_schedule("cancel").unwrap();
        assert_eq!(reopened.interval_progress("cancel").unwrap(), Some(120));
        assert!(reopened.materialize_interval("cancel", 150, 3).is_err());
        assert!(reopened
            .advance_interval_progress("cancel", Some(120), 130)
            .is_err());
        assert_eq!(
            reopened
                .committed_interval_occurrences("cancel", None, 10)
                .unwrap()
                .len(),
            3
        );
        reopened
            .create_interval_schedule("empty", &schedule())
            .unwrap();
        reopened.cancel_interval_schedule("empty").unwrap();
        assert_eq!(reopened.interval_progress("empty").unwrap(), None);
        assert!(reopened
            .committed_interval_occurrences("empty", None, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn cancellation_racing_publication_always_closes_the_chain() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("racecancel", &schedule())
            .unwrap();
        store.record_interval_occurrence("racecancel", 100).unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let publish = scope.spawn(|| {
                barrier.wait();
                store.advance_interval_progress("racecancel", None, 100)
            });
            let cancel = scope.spawn(|| {
                barrier.wait();
                store.cancel_interval_schedule("racecancel")
            });
            cancel.join().unwrap().unwrap();
            let result = publish.join().unwrap();
            assert!(result.is_ok() || matches!(result, Err(JobError::Conflict(_))));
        });
        assert!(store.interval_schedule_cancelled("racecancel").unwrap());
        let count = store
            .committed_interval_occurrences("racecancel", None, 10)
            .unwrap()
            .len();
        assert!(count <= 1);
        assert!(store.materialize_interval("racecancel", 110, 2).is_err());
    }

    #[test]
    fn committed_pages_recover_after_restart_and_exclude_orphan_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.create_interval_schedule("page", &schedule()).unwrap();
        store.materialize_interval("page", 155, 3).unwrap();
        store.materialize_interval("page", 155, 3).unwrap();
        store.record_interval_occurrence("page", 160).unwrap();
        let reopened = Store::open(dir.path()).unwrap();
        let first = reopened
            .committed_interval_occurrences("page", None, 2)
            .unwrap();
        assert_eq!(
            first.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![100, 110]
        );
        let second = reopened
            .committed_interval_occurrences("page", Some(110), 3)
            .unwrap();
        assert_eq!(
            second.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![120, 130, 140]
        );
        let third = reopened
            .committed_interval_occurrences("page", Some(140), 3)
            .unwrap();
        assert_eq!(
            third.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![150]
        );
        assert!(reopened
            .committed_interval_occurrences("page", Some(150), 3)
            .unwrap()
            .is_empty());
        assert!(reopened
            .committed_interval_occurrences("page", None, 0)
            .is_err());
        let mut s = schedule();
        s.missed_policy = MissedOccurrencePolicy::Coalesce;
        reopened.create_interval_schedule("coal", &s).unwrap();
        reopened.record_interval_occurrence("coal", 110).unwrap();
        reopened.materialize_interval("coal", 155, 3).unwrap();
        let committed = reopened
            .committed_interval_occurrences("coal", None, 3)
            .unwrap();
        assert_eq!(
            committed.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![150]
        );
    }

    #[test]
    fn batch_refuses_disagreeing_orphan_without_committing_progress() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("corrupt-batch", &schedule())
            .unwrap();
        let mut record = store
            .record_interval_occurrence("corrupt-batch", 100)
            .unwrap();
        record.job.command = vec!["different".into()];
        let path = dir.path().join("occurrences/corrupt-batch--100");
        let bytes = serde_json::to_vec(&record).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            store.materialize_interval("corrupt-batch", 155, 3),
            Err(JobError::Corrupt(_))
        ));
        assert_eq!(store.interval_progress("corrupt-batch").unwrap(), None);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(store
            .record_interval_occurrence("corrupt-batch", 101)
            .is_err());
    }

    #[test]
    fn materialization_reconciles_interrupted_publication_and_continues_batches() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.create_interval_schedule("tick", &schedule()).unwrap();
        // Simulate a process that published part of a batch and exited before
        // the progress commit. Recovery must reuse, rather than replace it.
        let original = store.record_interval_occurrence("tick", 100).unwrap();
        let path = dir.path().join("occurrences/tick--100");
        let bytes = std::fs::read(&path).unwrap();
        let reopened = Store::open(dir.path()).unwrap();
        let batch = reopened.materialize_interval("tick", 155, 3).unwrap();
        assert_eq!(batch[0], original);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(
            batch.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![100, 110, 120]
        );
        assert_eq!(reopened.interval_progress("tick").unwrap(), Some(120));
        let next = reopened.materialize_interval("tick", 155, 3).unwrap();
        assert_eq!(
            next.iter().map(|r| r.scheduled_ms).collect::<Vec<_>>(),
            vec![130, 140, 150]
        );
        assert!(reopened
            .materialize_interval("tick", 155, 3)
            .unwrap()
            .is_empty());
        assert_eq!(reopened.interval_progress("tick").unwrap(), Some(150));
        assert_eq!(
            std::fs::read_dir(dir.path().join("occurrences"))
                .unwrap()
                .count(),
            6
        );
        assert!(reopened.list().unwrap().is_empty());
    }

    #[test]
    fn progress_requires_complete_records_and_recovers_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("recover", &schedule())
            .unwrap();
        assert_eq!(store.interval_progress("recover").unwrap(), None);
        store.record_interval_occurrence("recover", 110).unwrap();
        assert!(store
            .advance_interval_progress("recover", None, 110)
            .is_err());
        store.record_interval_occurrence("recover", 100).unwrap();
        store
            .advance_interval_progress("recover", None, 110)
            .unwrap();
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(reopened.interval_progress("recover").unwrap(), Some(110));
        assert!(matches!(
            reopened.advance_interval_progress("recover", None, 110),
            Err(JobError::Conflict(_))
        ));
        assert!(reopened
            .advance_interval_progress("recover", Some(110), 100)
            .is_err());
        reopened.record_interval_occurrence("recover", 120).unwrap();
        reopened
            .advance_interval_progress("recover", Some(110), 120)
            .unwrap();
        assert_eq!(reopened.interval_progress("recover").unwrap(), Some(120));
    }

    #[test]
    fn racing_progress_commits_have_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut s = schedule();
        s.missed_policy = MissedOccurrencePolicy::Coalesce;
        store.create_interval_schedule("race", &s).unwrap();
        store.record_interval_occurrence("race", 150).unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        store.advance_interval_progress("race", None, 150)
                    })
                })
                .collect();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            assert!(results
                .iter()
                .all(|r| r.is_ok() || matches!(r, Err(JobError::Conflict(_)))));
        });
        assert_eq!(store.interval_progress("race").unwrap(), Some(150));
        assert_eq!(
            std::fs::read_dir(dir.path().join("schedule-progress"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn catch_up_is_bounded_and_resumes_after_recorded_occurrences() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .create_interval_schedule("batch", &schedule())
            .unwrap();
        let first = schedule().due_occurrences(None, 155, 3).unwrap();
        assert_eq!(first, vec![100, 110, 120]);
        for time in &first {
            store.record_interval_occurrence("batch", *time).unwrap();
        }
        let reopened = Store::open(dir.path()).unwrap();
        let s = reopened.interval_schedule("batch").unwrap();
        assert_eq!(
            s.due_occurrences(first.last().copied(), 155, 3).unwrap(),
            vec![130, 140, 150]
        );
        assert_eq!(
            s.due_occurrences(Some(150), 149, 3).unwrap(),
            Vec::<u64>::new()
        );
        assert_eq!(s.due_occurrences(Some(150), 160, 3).unwrap(), vec![160]);
        assert!(s.due_occurrences(None, 155, 0).is_err());
        assert!(s.due_occurrences(None, 155, 1025).is_err());
    }

    #[test]
    fn coalesce_selects_latest_due_and_handles_timestamp_end() {
        let mut s = schedule();
        s.missed_policy = MissedOccurrencePolicy::Coalesce;
        assert_eq!(s.due_occurrences(None, 99, 1).unwrap(), Vec::<u64>::new());
        assert_eq!(s.due_occurrences(None, 155, 1).unwrap(), vec![150]);
        assert_eq!(
            s.due_occurrences(Some(150), 155, 1).unwrap(),
            Vec::<u64>::new()
        );
        s.first_ms = u64::MAX - 5;
        s.every_ms = 5;
        assert_eq!(
            s.due_occurrences(None, u64::MAX, 1).unwrap(),
            vec![u64::MAX]
        );
        assert_eq!(
            s.due_occurrences(Some(u64::MAX), u64::MAX, 1).unwrap(),
            Vec::<u64>::new()
        );
        s.missed_policy = MissedOccurrencePolicy::CatchUp;
        assert_eq!(
            s.due_occurrences(None, u64::MAX, 5).unwrap(),
            vec![u64::MAX - 5, u64::MAX]
        );
    }

    #[test]
    fn boundaries_and_overflow_do_not_shift_anchor() {
        let mut s = schedule();
        assert_eq!(s.at_or_after(0).unwrap(), Some(100));
        assert_eq!(s.at_or_after(100).unwrap(), Some(100));
        assert_eq!(s.at_or_after(101).unwrap(), Some(110));
        s.first_ms = u64::MAX - 5;
        assert_eq!(s.at_or_after(u64::MAX).unwrap(), None);
        s.every_ms = 0;
        assert!(s.at_or_after(0).is_err());
    }

    #[test]
    fn racing_publishers_and_restart_preserve_one_occurrence() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| store.create_interval_schedule("daily", &schedule())))
                .collect();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            assert!(results
                .iter()
                .all(|r| r.is_ok() || matches!(r, Err(JobError::Conflict(_)))));
        });
        assert_eq!(store.interval_schedule("daily").unwrap(), schedule());
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| store.record_interval_occurrence("daily", 110)))
                .collect();
            for handle in handles {
                assert_eq!(handle.join().unwrap().unwrap().scheduled_ms, 110);
            }
        });
        let reopened = Store::open(dir.path()).unwrap();
        let occurrence = reopened.record_interval_occurrence("daily", 110).unwrap();
        assert_eq!(occurrence.job.not_before_ms, Some(110));
        assert_eq!(
            std::fs::read_dir(dir.path().join("occurrences"))
                .unwrap()
                .count(),
            1
        );
        assert!(reopened.record_interval_occurrence("daily", 111).is_err());
        assert!(reopened
            .create_interval_schedule("../escape", &schedule())
            .is_err());
        assert!(reopened.list().unwrap().is_empty());
    }
}
