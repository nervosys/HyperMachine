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
    // `env` treats any leading NAME=VALUE argument as an assignment, including
    // a literal executable containing '='. An explicit shell executable ends
    // assignment parsing, then preserves the user's argv through shell_exec.
    args.extend([
        "/bin/sh".into(),
        "-c".into(),
        crate::sandbox_vm::shell_exec(&record.job.command)?,
    ]);
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn real_shell_preserves_environment_arguments_and_equals_in_executable() {
        let dir = tempfile::Builder::new()
            .prefix("guest work ")
            .tempdir()
            .unwrap();
        let executable = dir.path().join("tool=literal");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$TEST\" \"$1\" \"$HM_JOB_ID\"\nexit 7\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let record: Occurrence = serde_json::from_value(json!({
            "schedule_id":"literal", "scheduled_ms":100,
            "job":{"command":["./tool=literal", "a'$(printf expanded)"],
                "env":{"TEST":"v'$(printf changed)"}, "workdir":dir.path()}
        }))
        .unwrap();
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &guest_command(&record).unwrap()])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(7),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            b"v'$(printf changed)\na'$(printf expanded)\nliteral--100\n"
        );
    }
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
    let (stdout, stdout_truncated) = hv2_jobs::dispatch::bounded_output(stdout);
    let (stderr, stderr_truncated) = hv2_jobs::dispatch::bounded_output(stderr);
    store.complete_vm_occurrence(
        id,
        scheduled_ms,
        &DispatchCompletion {
            claim_token: claim.token,
            exit_code,
            timed_out,
            stdout: Some(stdout.clone()),
            stderr: Some(stderr.clone()),
            stdout_truncated,
            stderr_truncated,
        },
    )?;
    Ok(
        json!({"schedule_id": id, "scheduled_ms": scheduled_ms, "exit_code": exit_code, "timed_out": timed_out, "stdout": stdout, "stderr": stderr, "stdout_truncated": stdout_truncated, "stderr_truncated": stderr_truncated}),
    )
}
