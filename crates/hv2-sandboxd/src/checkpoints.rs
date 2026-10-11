//! Checkpoints: save a running sandbox, and later roll it back to exactly
//! that state, in place -- the same ID, access token, URL and ports, with its
//! memory and filesystem as they were. boxd's "undo button for a machine".
//!
//! Not in E2B's API. A snapshot (`POST /sandboxes/{id}/snapshots`) makes a
//! template new sandboxes start from; a checkpoint belongs to one sandbox and
//! rewinds that sandbox. Both are the same layered snapshot underneath, taken
//! with the guest running: only the pages it changed since its template.
//!
//! - `POST   /sandboxes/{id}/checkpoints`                 `{"name": ...}` saves one
//! - `GET    /sandboxes/{id}/checkpoints`                 lists them
//! - `POST   /sandboxes/{id}/checkpoints/{name}/restore`  rolls back to one
//! - `DELETE /sandboxes/{id}/checkpoints/{name}`          removes one
//!
//! A restore brings the checkpoint up *before* stopping the sandbox it
//! replaces, so a restore that fails leaves the sandbox exactly as it was.
//! The swap then happens under the sandbox's transition lock, and a client
//! connected to the old guest is disconnected, as from a reboot.
//!
//! # The one the node takes by itself
//!
//! A sandbox created with `autoCheckpoint` (seconds), or on a node run with
//! `--auto-checkpoint-every`, is checkpointed on that interval without being
//! asked, under the name [`AUTO`]. There is one, replaced each time, and it
//! is rolled back to like any other. It is not taken when nothing has
//! happened in the sandbox since the last one: no request, and a quiet CPU.
//! boat's "automatic snapshots", with the memory that boat's leave out.
//!
//! At most [`MAX_PER_SANDBOX`] per sandbox, [`AUTO`] apart; they end with it. They live on
//! the node that took them: a sandbox paused and resumed on another node
//! leaves them behind, and the listing says so by being empty there.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{
    api_error, bring_up, forwards, now_ms, register, telemetry, transition_lock,
    valid_template_name, AppState, NetworkSpec, RegistrationContext,
};

/// The most checkpoints one sandbox keeps, [`AUTO`] apart.
pub(crate) const MAX_PER_SANDBOX: usize = 10;

/// The name of the checkpoint the node takes by itself. A caller cannot save
/// under it; restoring and deleting it are the caller's as with any other.
pub(crate) const AUTO: &str = "auto";

/// The shortest interval between automatic checkpoints. Each one stops the
/// guest for as long as its changed memory takes to write.
pub(crate) const MIN_AUTO_SECS: u64 = 60;

/// An automatic-checkpoint interval is 0 (off) or at least [`MIN_AUTO_SECS`].
pub(crate) fn check_interval(secs: u64) -> Result<(), String> {
    if secs != 0 && secs < MIN_AUTO_SECS {
        return Err(format!(
            "{secs}s is too often to stop a guest for a checkpoint; 0 or at least {MIN_AUTO_SECS}"
        ));
    }
    Ok(())
}

/// Whether a sandbox whose last automatic checkpoint was at `last` is due
/// another at `now`, every `every_secs`, given whether anything has happened
/// in it since.
///
/// `last` is the time of that checkpoint, or of the sandbox's start when it
/// has none: the first is taken an interval after it starts, not at once.
/// With one already saved and nothing since, there is nothing new to save.
pub(crate) fn due(now: u64, last: u64, every_secs: u64, has_one: bool, used_since: bool) -> bool {
    every_secs > 0
        && now.saturating_sub(last) >= every_secs.saturating_mul(1000)
        && (used_since || !has_one)
}

/// One saved state of one sandbox.
#[derive(Debug, Clone)]
pub(crate) struct Checkpoint {
    path: std::path::PathBuf,
    created_ms: u64,
}

/// Every sandbox's checkpoints on this node, by sandbox then name.
pub(crate) type Index = parking_lot::Mutex<HashMap<String, BTreeMap<String, Checkpoint>>>;

fn dir(state: &AppState, sandbox_id: &str) -> std::path::PathBuf {
    state.suspend_dir.join("checkpoints").join(sandbox_id)
}

/// Remove a sandbox's checkpoints, when it ends.
pub(crate) fn forget(state: &AppState, sandbox_id: &str) {
    if state.checkpoints.lock().remove(sandbox_id).is_some() {
        let _ = std::fs::remove_dir_all(dir(state, sandbox_id));
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct NewCheckpoint {
    name: Option<String>,
}

/// `POST /sandboxes/{id}/checkpoints`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<NewCheckpoint>>,
) -> Response {
    let name = match body.and_then(|Json(b)| b.name) {
        Some(name) if !valid_template_name(&name) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                format!("checkpoint name {name:?}: letters, digits, - _ . only"),
            )
        }
        Some(name) if name == AUTO => {
            return api_error(
                StatusCode::BAD_REQUEST,
                format!("{AUTO:?} is the name of the checkpoint the node takes by itself"),
            )
        }
        Some(name) => name,
        None => format!("cp-{}", now_ms()),
    };
    match save(&state, &sandbox_id, &name).await {
        Ok(created_ms) => (
            StatusCode::CREATED,
            Json(json!({ "name": name, "createdAt": created_ms })),
        )
            .into_response(),
        Err(Refused::NotRunning) => not_running(&state, &sandbox_id),
        Err(Refused::Because(status, why)) => api_error(status, why),
    }
}

/// Why a checkpoint was not saved.
enum Refused {
    /// The sandbox is paused, or is not there.
    NotRunning,
    Because(StatusCode, String),
}

/// Save `sandbox_id` as it is now under `name`, and return when.
///
/// [`AUTO`] replaces the one before it, and only once the new one is whole:
/// it is written beside the old and renamed over it, so a checkpoint that
/// fails leaves the last good one. Any other name must be free, and counts
/// toward [`MAX_PER_SANDBOX`].
async fn save(state: &Arc<AppState>, sandbox_id: &str, name: &str) -> Result<u64, Refused> {
    let automatic = name == AUTO;
    if state.templates.read().is_empty() {
        return Err(Refused::Because(
            StatusCode::CONFLICT,
            "checkpoints need sandboxes restored from a template, and this node boots them".into(),
        ));
    }
    if crate::disks::holds_one(sandbox_id) {
        return Err(Refused::Because(
            StatusCode::CONFLICT,
            format!(
                "sandbox {sandbox_id} holds a disk, and a sandbox with a disk cannot be checkpointed"
            ),
        ));
    }

    let lock = transition_lock(state, sandbox_id);
    let _held = lock.lock().await;
    let Some(vm) = state
        .sandboxes
        .lock()
        .get(sandbox_id)
        .map(|live| Arc::clone(&live.vm))
    else {
        return Err(Refused::NotRunning);
    };
    if !automatic {
        let index = state.checkpoints.lock();
        let held = index.get(sandbox_id);
        if held.is_some_and(|h| h.contains_key(name)) {
            return Err(Refused::Because(
                StatusCode::CONFLICT,
                format!("{sandbox_id} already has a checkpoint named {name:?}; delete it first"),
            ));
        }
        let named = held.map_or(0, |h| h.keys().filter(|k| k.as_str() != AUTO).count());
        if named >= MAX_PER_SANDBOX {
            return Err(Refused::Because(
                StatusCode::CONFLICT,
                format!("{sandbox_id} has {MAX_PER_SANDBOX} checkpoints, the most it keeps"),
            ));
        }
    }

    let failed = |e: String| Refused::Because(StatusCode::INTERNAL_SERVER_ERROR, e);
    let dir = dir(state, sandbox_id);
    std::fs::create_dir_all(&dir).map_err(|e| failed(e.to_string()))?;
    let path = dir.join(format!("{name}.snap"));
    let writing = if automatic {
        dir.join(format!("{name}.snap.new"))
    } else {
        path.clone()
    };
    let started = std::time::Instant::now();
    if let Err(e) = vm.checkpoint_to(&writing).await {
        let _ = std::fs::remove_file(&writing);
        return Err(failed(format!("checkpointing {sandbox_id}: {e}")));
    }
    if automatic {
        if let Err(e) = std::fs::rename(&writing, &path) {
            let _ = std::fs::remove_file(&writing);
            return Err(failed(format!(
                "putting {sandbox_id}'s new checkpoint in place: {e}"
            )));
        }
    }
    state.metrics.checkpoint_latency.observe(started.elapsed());
    let created_ms = now_ms();
    state
        .checkpoints
        .lock()
        .entry(sandbox_id.to_string())
        .or_default()
        .insert(name.to_string(), Checkpoint { path, created_ms });
    telemetry::log(
        state,
        sandbox_id,
        "info",
        format!("checkpoint {name} saved"),
    );
    Ok(created_ms)
}

/// Checkpoint every running sandbox whose interval has come round, as of
/// `now`. See the module's header for which, and when not.
pub(crate) async fn take_due(state: &Arc<AppState>, now: u64) {
    let candidates: Vec<(String, u64, u64, u64)> = state
        .sandboxes
        .lock()
        .iter()
        .filter(|(_, live)| live.lifecycle.auto_checkpoint_secs > 0 && !live.vm.in_standby())
        .map(|(id, live)| {
            (
                id.clone(),
                live.lifecycle.auto_checkpoint_secs,
                live.record.started_at_ms,
                live.activity.last_active_ms(),
            )
        })
        .collect();
    for (id, every_secs, started_ms, last_active_ms) in candidates {
        let saved = state
            .checkpoints
            .lock()
            .get(&id)
            .and_then(|held| held.get(AUTO).map(|c| c.created_ms));
        let last = saved.unwrap_or(started_ms);
        // Cheap first: most sandboxes on most ticks are not due.
        if now.saturating_sub(last) < every_secs.saturating_mul(1000) {
            continue;
        }
        let used_since = last_active_ms > last
            || !crate::idle::quiet(telemetry::busiest_since(state, &id, last));
        if !due(now, last, every_secs, saved.is_some(), used_since) {
            continue;
        }
        match save(state, &id, AUTO).await {
            Ok(_) => tracing::info!("sandbox {id} was checkpointed on its interval"),
            // It was paused or ended since it was chosen.
            Err(Refused::NotRunning) => {}
            Err(Refused::Because(_, why)) => {
                tracing::warn!("checkpointing sandbox {id} on its interval: {why}");
            }
        }
    }
}

/// `GET /sandboxes/{id}/checkpoints`, oldest first.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
) -> Response {
    let known = state.sandboxes.lock().contains_key(&sandbox_id)
        || state.paused.lock().contains_key(&sandbox_id);
    if !known {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"));
    }
    let mut out: Vec<(u64, String)> = state
        .checkpoints
        .lock()
        .get(&sandbox_id)
        .map(|held| {
            held.iter()
                .map(|(name, c)| (c.created_ms, name.clone()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    Json(
        out.into_iter()
            .map(|(created, name)| json!({ "name": name, "createdAt": created }))
            .collect::<Vec<_>>(),
    )
    .into_response()
}

/// `DELETE /sandboxes/{id}/checkpoints/{name}`.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    Path((sandbox_id, name)): Path<(String, String)>,
) -> Response {
    let removed = state
        .checkpoints
        .lock()
        .get_mut(&sandbox_id)
        .and_then(|held| held.remove(&name));
    match removed {
        Some(checkpoint) => {
            let _ = std::fs::remove_file(&checkpoint.path);
            StatusCode::NO_CONTENT.into_response()
        }
        None => api_error(
            StatusCode::NOT_FOUND,
            format!("{sandbox_id} has no checkpoint {name:?}"),
        ),
    }
}

/// `POST /sandboxes/{id}/checkpoints/{name}/restore`.
pub(crate) async fn restore(
    State(state): State<Arc<AppState>>,
    Path((sandbox_id, name)): Path<(String, String)>,
) -> Response {
    let Some(checkpoint) = state
        .checkpoints
        .lock()
        .get(&sandbox_id)
        .and_then(|held| held.get(&name).cloned())
    else {
        return api_error(
            StatusCode::NOT_FOUND,
            format!("{sandbox_id} has no checkpoint {name:?}"),
        );
    };

    let lock = transition_lock(&state, &sandbox_id);
    let _held = lock.lock().await;
    let source = state.sandboxes.lock().get(&sandbox_id).map(|live| {
        (
            live.record.clone(),
            live.descriptor.clone(),
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
    let Some((record, descriptor, network)) = source else {
        return not_running(&state, &sandbox_id);
    };
    if state
        .sandboxes
        .lock()
        .get(&sandbox_id)
        .is_some_and(|live| live.pending_registration.is_some())
    {
        return api_error(
            StatusCode::CONFLICT,
            "reconcile registration before restoring a checkpoint",
        );
    }

    // The replacement first: until it is up, the sandbox is untouched.
    let started = std::time::Instant::now();
    let running = match bring_up(
        &state,
        &sandbox_id,
        &record.template_id,
        Some(&checkpoint.path),
        network,
        &record.volume_mounts,
        record.team_id.as_ref(),
        None,
        &BTreeMap::new(),
        &descriptor.envd_access_token,
    )
    .await
    {
        Ok(running) => running,
        Err((status, e)) => {
            return api_error(status, format!("restoring {sandbox_id} to {name}: {e}"))
        }
    };

    // Then the swap, still under the transition lock.
    let Some(old) = state.sandboxes.lock().remove(&sandbox_id) else {
        let _ = running.vm.stop().await;
        return not_running(&state, &sandbox_id);
    };
    forwards::stop(&state, &sandbox_id);
    state.routes.remove_sandbox(&sandbox_id);
    let _ = old.process_shutdown.send(());
    if let Some(network) = old.network {
        network.bridge.abort();
    }
    if let Err(e) = old.vm.stop().await {
        tracing::warn!("stopping {sandbox_id}'s replaced guest: {e}");
    }

    let mut descriptor = old.descriptor;
    descriptor.process_port = running.process_addr.port();
    let mut record = old.record;
    record.descriptor = serde_json::to_value(&descriptor).unwrap_or_default();
    if let Err((status, error)) = register(
        &state,
        old._slot,
        running,
        descriptor,
        record.clone(),
        old.lifecycle,
        old.network_request,
        RegistrationContext {
            event: Some("sandbox-updated"),
            ..RegistrationContext::default()
        },
    )
    .await
    {
        return api_error(status, error);
    }
    telemetry::log(
        &state,
        &sandbox_id,
        "info",
        format!("restored to checkpoint {name} in {:?}", started.elapsed()),
    );
    Json(json!({ "sandboxID": sandbox_id, "checkpoint": name })).into_response()
}

fn not_running(state: &AppState, sandbox_id: &str) -> Response {
    if state.paused.lock().contains_key(sandbox_id) {
        api_error(
            StatusCode::CONFLICT,
            format!("sandbox {sandbox_id} is paused; resume it first"),
        )
    } else {
        api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interval_is_off_or_long_enough_to_be_worth_stopping_a_guest_for() {
        assert!(check_interval(0).is_ok());
        assert!(check_interval(MIN_AUTO_SECS).is_ok());
        assert!(check_interval(3600).is_ok());
        assert!(check_interval(1).is_err());
        assert!(check_interval(MIN_AUTO_SECS - 1).is_err());
    }

    /// The first comes an interval after the sandbox starts; each later one
    /// an interval after the last, and only if something happened since.
    #[test]
    fn a_checkpoint_is_due_on_its_interval_and_only_when_there_is_something_new() {
        let (start, every) = (1_000_000, 60);
        // Not yet an interval old: not due, used or not.
        assert!(!due(start + 59_999, start, every, false, true));
        // An interval old with none saved: due even if untouched, so there
        // is always one to go back to.
        assert!(due(start + 60_000, start, every, false, false));
        assert!(due(start + 60_000, start, every, false, true));
        // One saved, an interval ago, and nothing since: nothing new to save.
        assert!(!due(start + 120_000, start + 60_000, every, true, false));
        // One saved, and the sandbox was used since: due.
        assert!(due(start + 120_000, start + 60_000, every, true, true));
        // Off.
        assert!(!due(start + 10_000_000, start, 0, false, true));
        // A clock that went backwards is not a reason.
        assert!(!due(start - 1, start, every, false, true));
    }
}
