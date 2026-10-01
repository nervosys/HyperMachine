//! One explicit VM dispatch. Unknown transport outcomes remain durably claimed.
use anyhow::{bail, Context, Result};
use hv2_jobs::{dispatch::DispatchCompletion, schedule::Occurrence, Store};
use reqwest::Method;
use serde_json::{json, Value};
use std::path::Path;

fn guest_command(record: &Occurrence) -> Result<String> {
    let mut args = vec!["env".to_string(), "--".to_string()];
    args.extend(
        record
            .job
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    args.push(format!(
        "HM_JOB_ID={}--{}",
        record.schedule_id, record.scheduled_ms
    ));
    args.extend(record.job.command.clone());
    let command = crate::sandbox_vm::shell_exec(&args)?;
    Ok(match &record.job.workdir {
        Some(path) => format!(
            "cd '{}' && {command}",
            path.to_str()
                .context("invalid guest working directory")?
                .replace('\'', "'\\''")
        ),
        None => command,
    })
}

/// Dispatch a committed occurrence once. Failures after claim creation require
/// reconciliation; calling this again cannot execute the command a second time.
pub async fn dispatch_once(
    store: &Store,
    id: &str,
    scheduled_ms: u64,
    worker: &str,
    profiles: &Path,
) -> Result<Value> {
    let records = store.committed_interval_occurrences(id, scheduled_ms.checked_sub(1), 1)?;
    let record = records
        .first()
        .filter(|r| r.scheduled_ms == scheduled_ms)
        .context("committed occurrence not found")?;
    let target = record.vm.as_ref().context("occurrence has no VM target")?;
    let (api, request_timeout) =
        crate::jobs_profile::resolve_connection_profile(profiles, &target.connection_profile)?;
    if request_timeout <= target.timeout_secs {
        bail!("profile request timeout must exceed the guest command timeout");
    }
    let command = guest_command(record)?;
    let claim = store.claim_vm_occurrence(id, scheduled_ms, worker)?;
    // Connect resumes paused sandboxes; extend the lifetime for the bounded
    // command. Do not return descriptor tokens or server error bodies to logs.
    api.request(
        Method::POST,
        &["sandboxes", &target.sandbox_id, "connect"],
        Some(json!({"timeout": target.timeout_secs + 60})),
    )
    .await
    .map_err(|_| anyhow::anyhow!("VM connection failed; dispatch remains unresolved"))?;
    let response = api
        .request(
            Method::POST,
            &["sandboxes", &target.sandbox_id, "exec"],
            Some(json!({"cmd": command, "timeout_secs": target.timeout_secs})),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("VM execution response unavailable; dispatch remains unresolved")
        })?;
    let timed_out = response["timed_out"]
        .as_bool()
        .context("invalid guest result; dispatch remains unresolved")?;
    let exit_code = match response.get("exit_code") {
        Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_i64()
                .filter(|code| (0..=255).contains(code))
                .context("invalid guest exit code; dispatch remains unresolved")?
                as i32,
        ),
        None => bail!("missing guest exit code; dispatch remains unresolved"),
    };
    let stdout = response["stdout"]
        .as_str()
        .context("invalid guest stdout; dispatch remains unresolved")?;
    let stderr = response["stderr"]
        .as_str()
        .context("invalid guest stderr; dispatch remains unresolved")?;
    store.complete_vm_occurrence(
        id,
        scheduled_ms,
        &DispatchCompletion {
            claim_token: claim.token,
            exit_code,
            timed_out,
        },
    )?;
    // Output is not durable yet. Completion is recoverable even if this stdout
    // is lost; persistent bounded guest logs require the process-based executor.
    Ok(
        json!({"schedule_id": id, "scheduled_ms": scheduled_ms, "exit_code": exit_code, "timed_out": timed_out, "stdout": stdout, "stderr": stderr}),
    )
}
