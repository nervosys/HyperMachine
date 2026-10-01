//! Immutable schedule and occurrence records. These do not dispatch jobs yet.
//! Publication uses an exclusive hard link to a completely written file, so
//! racing publishers cannot expose partial JSON or replace the winning record.

use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::{JobError, JobSpec, Result, Store};

/// An interval anchored to an absolute Unix timestamp, without clock drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalSchedule {
    pub first_ms: u64,
    pub every_ms: u64,
    pub job: JobSpec,
}

impl IntervalSchedule {
    pub fn validate(&self) -> Result<()> {
        if self.every_ms == 0 {
            return Err(JobError::InvalidSpec(
                "schedule interval must be positive".into(),
            ));
        }
        self.job.validate()
    }

    /// The first occurrence at or after `time_ms`; None means overflow.
    pub fn at_or_after(&self, time_ms: u64) -> Result<Option<u64>> {
        self.validate()?;
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
    pub job: JobSpec,
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
    /// Create an immutable interval schedule. Existing names are conflicts.
    pub fn create_interval_schedule(&self, id: &str, schedule: &IntervalSchedule) -> Result<()> {
        check_schedule_id(id)?;
        schedule.validate()?;
        self.publish_schedule_record("schedules", id, schedule)
    }

    pub fn interval_schedule(&self, id: &str) -> Result<IntervalSchedule> {
        check_schedule_id(id)?;
        let schedule: IntervalSchedule =
            crate::read_json(&self.root().join("schedules").join(id), id)?;
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
        let mut job = schedule.job;
        job.not_before_ms = Some(scheduled_ms);
        let occurrence = Occurrence {
            schedule_id: id.into(),
            scheduled_ms,
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

    fn publish_schedule_record(
        &self,
        directory: &str,
        key: &str,
        value: &impl Serialize,
    ) -> Result<()> {
        let dir = self.root().join(directory);
        std::fs::create_dir_all(&dir)?;
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
            match std::fs::hard_link(&temp, dir.join(key)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule() -> IntervalSchedule {
        IntervalSchedule {
            first_ms: 100,
            every_ms: 10,
            job: serde_json::from_value(serde_json::json!({"command":["echo","ok"]})).unwrap(),
        }
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
