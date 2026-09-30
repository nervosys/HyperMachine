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
//! At most [`MAX_PER_SANDBOX`] per sandbox; they end with it. They live on
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
    valid_template_name, AppState, NetworkSpec,
};

/// The most checkpoints one sandbox keeps.
pub(crate) const MAX_PER_SANDBOX: usize = 10;

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
        Some(name) => name,
        None => format!("cp-{}", now_ms()),
    };
    if state.templates.read().is_empty() {
        return api_error(
            StatusCode::CONFLICT,
            "checkpoints need sandboxes restored from a template, and this node boots them",
        );
    }

    let lock = transition_lock(&state, &sandbox_id);
    let _held = lock.lock().await;
    let Some(vm) = state
        .sandboxes
        .lock()
        .get(&sandbox_id)
        .map(|live| Arc::clone(&live.vm))
    else {
        return not_running(&state, &sandbox_id);
    };
    {
        let index = state.checkpoints.lock();
        let held = index.get(&sandbox_id);
        if held.is_some_and(|h| h.contains_key(&name)) {
            return api_error(
                StatusCode::CONFLICT,
                format!("{sandbox_id} already has a checkpoint named {name:?}; delete it first"),
            );
        }
        if held.map_or(0, BTreeMap::len) >= MAX_PER_SANDBOX {
            return api_error(
                StatusCode::CONFLICT,
                format!("{sandbox_id} has {MAX_PER_SANDBOX} checkpoints, the most it keeps"),
            );
        }
    }

    let dir = dir(&state, &sandbox_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    let path = dir.join(format!("{name}.snap"));
    let started = std::time::Instant::now();
    if let Err(e) = vm.checkpoint_to(&path).await {
        let _ = std::fs::remove_file(&path);
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("checkpointing {sandbox_id}: {e}"),
        );
    }
    state.metrics.checkpoint_latency.observe(started.elapsed());
    let created_ms = now_ms();
    state
        .checkpoints
        .lock()
        .entry(sandbox_id.clone())
        .or_default()
        .insert(name.clone(), Checkpoint { path, created_ms });
    telemetry::log(
        &state,
        &sandbox_id,
        "info",
        format!("checkpoint {name} saved"),
    );
    (
        StatusCode::CREATED,
        Json(json!({ "name": name, "createdAt": created_ms })),
    )
        .into_response()
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

    // The replacement first: until it is up, the sandbox is untouched.
    let started = std::time::Instant::now();
    let running = match bring_up(
        &state,
        &sandbox_id,
        &record.template_id,
        Some(&checkpoint.path),
        network,
        &record.volume_mounts,
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
    register(
        &state,
        old._slot,
        running,
        descriptor,
        record.clone(),
        old.lifecycle,
        old.network_request,
        Some("sandbox-updated"),
    )
    .await;
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
