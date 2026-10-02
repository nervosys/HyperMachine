//! Durable ownership for VM occurrences. No lease expiry automatically starts
//! another command: a claim without completion requires guest reconciliation.

use crate::{schedule::Occurrence, JobError, Result, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchClaim {
    pub occurrence: Occurrence,
    pub worker: String,
    pub token: String,
}

/// How the result was obtained. Unknown preserves receipts written before
/// provenance was recorded; no origin proves exactly-once side effects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionOrigin {
    #[default]
    Unknown,
    ApiResponse,
    OperatorRecorded,
}

/// A guest result, not proof of exactly-once external side effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchCompletion {
    #[serde(default)]
    pub origin: CompletionOrigin,
    pub claim_token: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
    #[serde(default)]
    pub stdout_truncated: bool,
    #[serde(default)]
    pub stderr_truncated: bool,
}

pub const MAX_RECEIPT_OUTPUT_BYTES: usize = 65_536;

/// Bound a UTF-8 stream without splitting a character.
pub fn bounded_output(text: &str) -> (String, bool) {
    let mut end = text.len().min(MAX_RECEIPT_OUTPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), end < text.len())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchState {
    pub claim: DispatchClaim,
    /// None means unresolved: the guest might be running, finished, or never
    /// started. Losing a worker is not evidence that re-execution is safe.
    pub completion: Option<DispatchCompletion>,
}

impl Store {
    /// Select the oldest unclaimed VM occurrence. An unresolved predecessor
    /// blocks later work; completed receipts allow progress even after restart.
    /// Ordering is per schedule, not across schedules targeting the same VM.
    pub fn next_vm_occurrence(&self, id: &str) -> Result<Option<Occurrence>> {
        if self.interval_schedule(id)?.vm.is_none() {
            return Err(JobError::InvalidSpec("schedule has no VM target".into()));
        }
        let mut cursor = None;
        loop {
            // Most active schedules can decide from the first record.
            // Use full pages only after finding completed history.
            let limit = if cursor.is_none() { 1 } else { 1024 };
            let records = self.committed_interval_occurrences(id, cursor, limit)?;
            if records.is_empty() {
                return Ok(None);
            }
            for record in records {
                match self.read_vm_dispatch_state(id, record.scheduled_ms) {
                    Ok(state) if state.completion.is_some() => cursor = Some(record.scheduled_ms),
                    Ok(_) => {
                        return Err(JobError::Conflict(
                            "earlier occurrence is unresolved; guest reconciliation is required"
                                .into(),
                        ))
                    }
                    Err(JobError::NotFound(_)) => return Ok(Some(record)),
                    Err(error) => return Err(error),
                }
            }
        }
    }

    /// Exclusively claim a committed VM occurrence. Claims are never replaced
    /// or automatically retried. A competing worker receives Conflict.
    pub fn claim_vm_occurrence(
        &self,
        id: &str,
        scheduled_ms: u64,
        worker: &str,
    ) -> Result<DispatchClaim> {
        if worker.is_empty()
            || worker.len() > 64
            || !worker
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return Err(JobError::InvalidSpec(
                "dispatch worker requires 1-64 ASCII letters, digits, hyphens or underscores"
                    .into(),
            ));
        }
        let occurrence = self
            .next_vm_occurrence(id)?
            .filter(|record| record.scheduled_ms == scheduled_ms)
            .ok_or_else(|| {
                JobError::NotFound(format!("committed occurrence {id}/{scheduled_ms}"))
            })?;
        if occurrence.vm.is_none() {
            return Err(JobError::InvalidSpec("occurrence has no VM target".into()));
        }
        let claim = DispatchClaim {
            occurrence,
            worker: worker.into(),
            token: uuid::Uuid::new_v4().to_string(),
        };
        self.publish_schedule_record("dispatch-claims", &format!("{id}--{scheduled_ms}"), &claim)?;
        Ok(claim)
    }

    /// Read unresolved or completed ownership after a dispatcher restart.
    pub fn vm_dispatch_state(&self, id: &str, scheduled_ms: u64) -> Result<DispatchState> {
        self.interval_schedule(id)?;
        self.read_vm_dispatch_state(id, scheduled_ms)
    }

    // Call only after validating the immutable schedule in this operation.
    // Claim identity and result token checks still apply to every record.
    fn read_vm_dispatch_state(&self, id: &str, scheduled_ms: u64) -> Result<DispatchState> {
        let key = format!("{id}--{scheduled_ms}");
        let claim: DispatchClaim =
            crate::read_json(&self.root().join("dispatch-claims").join(&key), &key)?;
        if claim.occurrence.schedule_id != id || claim.occurrence.scheduled_ms != scheduled_ms {
            return Err(JobError::Corrupt(format!(
                "dispatch claim {key} has the wrong occurrence"
            )));
        }
        let completion: Option<DispatchCompletion> =
            match crate::read_json(&self.root().join("dispatch-results").join(&key), &key) {
                Ok(completion) => Some(completion),
                Err(JobError::NotFound(_)) => None,
                Err(error) => return Err(error),
            };
        if completion
            .as_ref()
            .is_some_and(|result| result.claim_token != claim.token)
        {
            return Err(JobError::Corrupt(format!(
                "dispatch result {key} has the wrong claim"
            )));
        }
        Ok(DispatchState { claim, completion })
    }

    /// Commit one immutable result for the claim. Repeating the same result
    /// succeeds; changing a previously recorded result is a conflict.
    pub fn complete_vm_occurrence(
        &self,
        id: &str,
        scheduled_ms: u64,
        result: &DispatchCompletion,
    ) -> Result<()> {
        if [&result.stdout, &result.stderr]
            .into_iter()
            .flatten()
            .any(|text| text.len() > MAX_RECEIPT_OUTPUT_BYTES)
        {
            return Err(JobError::InvalidSpec(
                "dispatch output exceeds the per-stream receipt limit".into(),
            ));
        }
        if result
            .exit_code
            .is_some_and(|code| !(0..=255).contains(&code))
        {
            return Err(JobError::InvalidSpec(
                "guest exit code must be 0-255 or null".into(),
            ));
        }
        let state = self.vm_dispatch_state(id, scheduled_ms)?;
        if state.claim.token != result.claim_token {
            return Err(JobError::Conflict(
                "dispatch claim token does not match".into(),
            ));
        }
        let key = format!("{id}--{scheduled_ms}");
        match self.publish_schedule_record("dispatch-results", &key, result) {
            Ok(()) => Ok(()),
            Err(JobError::Conflict(_))
                if self
                    .vm_dispatch_state(id, scheduled_ms)?
                    .completion
                    .as_ref()
                    == Some(result) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn racing_claims_and_restart_leave_unresolved_work_owned_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let schedule = serde_json::from_value(serde_json::json!({
            "first_ms":100,"every_ms":10,"vm":{"sandbox_id":"guest","connection_profile":"local","timeout_secs":30},
            "job":{"command":["echo","ok"]}
        })).unwrap();
        store
            .create_interval_schedule("dispatch", &schedule)
            .unwrap();
        store.record_interval_occurrence("dispatch", 100).unwrap();
        assert!(store
            .claim_vm_occurrence("dispatch", 100, "worker")
            .is_err());
        store.materialize_interval("dispatch", 100, 1).unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        store.claim_vm_occurrence("dispatch", 100, "worker")
                    })
                })
                .collect();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            assert!(results
                .iter()
                .all(|r| r.is_ok() || matches!(r, Err(JobError::Conflict(_)))));
        });
        let reopened = Store::open(dir.path()).unwrap();
        let state = reopened.vm_dispatch_state("dispatch", 100).unwrap();
        assert!(state.completion.is_none());
        reopened.materialize_interval("dispatch", 110, 1).unwrap();
        assert!(reopened
            .claim_vm_occurrence("dispatch", 110, "later")
            .is_err());
        assert!(reopened
            .claim_vm_occurrence("dispatch", 100, "replacement")
            .is_err());
        let mut result = DispatchCompletion {
            origin: CompletionOrigin::Unknown,
            claim_token: "wrong".into(),
            exit_code: Some(0),
            timed_out: false,
            stdout: Some("output".into()),
            stderr: Some(String::new()),
            stdout_truncated: false,
            stderr_truncated: false,
        };
        assert!(reopened
            .complete_vm_occurrence("dispatch", 100, &result)
            .is_err());
        result.claim_token = state.claim.token;
        reopened
            .complete_vm_occurrence("dispatch", 100, &result)
            .unwrap();
        assert_eq!(
            reopened
                .next_vm_occurrence("dispatch")
                .unwrap()
                .unwrap()
                .scheduled_ms,
            110
        );
        reopened
            .complete_vm_occurrence("dispatch", 100, &result)
            .unwrap();
        result.exit_code = Some(7);
        assert!(reopened
            .complete_vm_occurrence("dispatch", 100, &result)
            .is_err());
        assert_eq!(
            reopened
                .vm_dispatch_state("dispatch", 100)
                .unwrap()
                .completion
                .unwrap()
                .exit_code,
            Some(0)
        );
    }

    #[test]
    fn receipt_output_is_bounded_at_utf8_boundaries_and_old_receipts_are_readable() {
        let text = format!("{}é", "x".repeat(MAX_RECEIPT_OUTPUT_BYTES - 1));
        let (bounded, truncated) = bounded_output(&text);
        assert!(truncated);
        assert_eq!(bounded.len(), MAX_RECEIPT_OUTPUT_BYTES - 1);
        assert_eq!(bounded_output("small"), ("small".into(), false));
        let legacy: DispatchCompletion = serde_json::from_value(
            serde_json::json!({"claim_token":"old","exit_code":0,"timed_out":false}),
        )
        .unwrap();
        assert_eq!(legacy.origin, CompletionOrigin::Unknown);
        assert!(legacy.stdout.is_none());
        assert!(legacy.stderr.is_none());
    }
}
