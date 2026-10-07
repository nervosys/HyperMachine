//! Reboot in place: the same sandbox, a fresh guest.
//!
//! A guest that reboots itself (`reboot`, a kernel panic, a triple fault)
//! makes KVM stop the VM. Before this, that left a sandbox that listed as
//! running while every request to it timed out until its lifetime ran out.
//! Now the expiry loop notices a stopped VM within a second, and the sandbox
//! is brought back up with the same ID, access token, URL, ports and network
//! policy. `POST /sandboxes/{id}/reboot` does the same on request.
//!
//! What comes back is what the sandbox was *created* with: its template's
//! guest, booted fresh, with its volumes and disk mounted again and its
//! `envVars` put back. What the guest wrote to its own root filesystem, which
//! is memory, does not survive, as it does not on any machine whose root is a
//! RAM disk; what it wrote to a volume or a disk does.
//!
//! The old VM is stopped *before* the new one starts. A checkpoint restore
//! does the reverse, so that a failure leaves the sandbox as it was, but here
//! the old guest is usually dead already, and a disk must never be attached
//! to two running guests: that is how a filesystem is corrupted.
//!
//! A guest that keeps dying is not rebooted forever: after
//! [`MAX_REBOOTS`] within [`REBOOT_WINDOW`] the sandbox is ended as lost.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use super::{
    api_error, bring_up, disks, end_sandbox, env_vars, forwards, register, telemetry,
    transition_lock, AppState, NetworkSpec, RegistrationContext,
};

/// The most reboots a sandbox may take within [`REBOOT_WINDOW`].
pub(crate) const MAX_REBOOTS: usize = 5;
/// The window [`MAX_REBOOTS`] is counted over.
pub(crate) const REBOOT_WINDOW: Duration = Duration::from_secs(60);

/// When each sandbox last rebooted, newest last.
static RECENT: Mutex<Option<HashMap<String, VecDeque<Instant>>>> = Mutex::new(None);

/// Record a reboot of `sandbox_id`; whether it is within the limit.
fn admit(sandbox_id: &str, now: Instant) -> bool {
    let mut guard = RECENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let recent = guard
        .get_or_insert_with(HashMap::new)
        .entry(sandbox_id.to_string())
        .or_default();
    while recent
        .front()
        .is_some_and(|t| now.duration_since(*t) > REBOOT_WINDOW)
    {
        recent.pop_front();
    }
    if recent.len() >= MAX_REBOOTS {
        return false;
    }
    recent.push_back(now);
    true
}

/// The sandbox ended; its count goes with it.
pub(crate) fn forget(sandbox_id: &str) {
    if let Some(map) = RECENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        map.remove(sandbox_id);
    }
}

/// Why a reboot happens, which decides when it is still wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// Asked for: reboot whatever state the guest is in.
    Request,
    /// The VM was found stopped: reboot only if it still is, since another
    /// caller may have rebooted it while this one waited for the lock.
    Stopped,
}

/// `POST /sandboxes/{id}/reboot`.
pub(crate) async fn route(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
) -> Response {
    match reboot(&state, &sandbox_id, Cause::Request).await {
        Ok(true) => Json(json!({ "sandboxID": sandbox_id })).into_response(),
        Ok(false) => api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
        Err((status, message)) => api_error(status, message),
    }
}

/// Reboot every sandbox whose VM has stopped by itself. Called by the expiry
/// loop once a second.
pub(crate) async fn reboot_stopped(state: &Arc<AppState>) {
    let stopped: Vec<String> = state
        .sandboxes
        .lock()
        .iter()
        .filter(|(_, live)| live.vm.state() == hv2_core::VMState::Stopped)
        .map(|(id, _)| id.clone())
        .collect();
    for id in stopped {
        let state = Arc::clone(state);
        tokio::spawn(async move {
            tracing::info!("sandbox {id}: its guest stopped by itself; rebooting it");
            if let Err((_, e)) = reboot(&state, &id, Cause::Stopped).await {
                tracing::warn!("sandbox {id}: reboot failed: {e}");
            }
        });
    }
}

/// Reboot `sandbox_id` in place. `Ok(false)` when there is no such running
/// sandbox, or when `cause` is [`Cause::Stopped`] and its guest is running.
///
/// # Errors
///
/// 409 while its registration is unsettled; the bring-up's own error when
/// the new guest does not come up, which leaves the sandbox with a stopped VM
/// for the expiry loop to try again, within the limit.
pub(crate) async fn reboot(
    state: &Arc<AppState>,
    sandbox_id: &str,
    cause: Cause,
) -> Result<bool, (StatusCode, String)> {
    let lock = transition_lock(state, sandbox_id);
    let held = lock.lock().await;
    let source = state.sandboxes.lock().get(sandbox_id).map(|live| {
        (
            Arc::clone(&live.vm),
            live.pending_registration.is_some(),
            live.record.clone(),
            live.descriptor.envd_access_token.clone(),
            live.network.as_ref().map(|n| NetworkSpec {
                policy: n.gateway.policy(),
                proxy: n.gateway.egress_proxy(),
                tokens: live
                    .network_request
                    .as_ref()
                    .map(|r| r.iam.clone())
                    .unwrap_or_default(),
            }),
        )
    });
    let Some((old_vm, pending, record, token, network)) = source else {
        return Ok(false);
    };
    if cause == Cause::Stopped && old_vm.state() != hv2_core::VMState::Stopped {
        return Ok(false);
    }
    if pending {
        return Err((
            StatusCode::CONFLICT,
            "reconcile registration before rebooting".into(),
        ));
    }
    if !admit(sandbox_id, Instant::now()) {
        drop(held);
        tracing::warn!(
            "sandbox {sandbox_id}: {MAX_REBOOTS} reboots within {REBOOT_WINDOW:?}; ending it"
        );
        end_sandbox(state, sandbox_id, "sandbox-lost").await;
        return Err((
            StatusCode::CONFLICT,
            format!(
                "sandbox {sandbox_id} rebooted {MAX_REBOOTS} times within {REBOOT_WINDOW:?} \
                 and was ended"
            ),
        ));
    }

    let started = Instant::now();
    // Unroutable while it is down, and its guest gone before another starts.
    forwards::stop(state, sandbox_id);
    state.routes.remove_sandbox(sandbox_id);
    if let Err(e) = old_vm.stop().await {
        tracing::debug!("stopping {sandbox_id}'s old guest: {e}");
    }

    let disk = disks::reattach(state, sandbox_id);
    let env = env_vars::kept(sandbox_id);
    let running = bring_up(
        state,
        sandbox_id,
        &record.template_id,
        None,
        network,
        &record.volume_mounts,
        record.team_id.as_ref(),
        disk.as_ref(),
        &env,
        &token,
    )
    .await
    .map_err(|(status, e)| (status, format!("rebooting {sandbox_id}: {e}")))?;

    // The swap, still under the transition lock.
    let Some(old) = state.sandboxes.lock().remove(sandbox_id) else {
        let _ = running.vm.stop().await;
        return Ok(false);
    };
    let _ = old.process_shutdown.send(());
    if let Some(network) = old.network {
        network.bridge.abort();
    }
    let mut descriptor = old.descriptor;
    descriptor.process_port = running.process_addr.port();
    let mut record = old.record;
    record.descriptor = serde_json::to_value(&descriptor).unwrap_or_default();
    register(
        state,
        old._slot,
        running,
        descriptor,
        record,
        old.lifecycle,
        old.network_request,
        RegistrationContext {
            event: Some("sandbox-updated"),
            ..RegistrationContext::default()
        },
    )
    .await?;
    state.metrics.reboots.inc();
    telemetry::log(
        state,
        sandbox_id,
        "info",
        format!(
            "rebooted ({}) in {:?}",
            match cause {
                Cause::Request => "requested",
                Cause::Stopped => "its guest stopped",
            },
            started.elapsed()
        ),
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guest_that_keeps_dying_is_not_rebooted_forever() {
        let id = "sbx-crash-loop";
        let t0 = Instant::now();
        for i in 0..MAX_REBOOTS {
            assert!(admit(id, t0 + Duration::from_secs(i as u64)), "reboot {i}");
        }
        assert!(!admit(id, t0 + Duration::from_secs(10)), "one too many");
        // Outside the window, the oldest no longer count.
        assert!(admit(id, t0 + REBOOT_WINDOW + Duration::from_secs(2)));
        forget(id);
        assert!(admit(id, t0));
    }
}
