//! Snapshots of sandboxes, kept as templates: E2B's
//! `POST /sandboxes/{id}/snapshots`, `GET /snapshots` and
//! `DELETE /templates/{id}`.
//!
//! A snapshot is a layered checkpoint -- only the pages a sandbox changed
//! since its template -- plus the template it is layered over. A sandbox
//! created from one is restored as a fork is: the template mapped, the
//! snapshot's pages copied in. So a snapshot costs what the sandbox
//! changed, not its RAM, and creating from one costs a restore.
//!
//! With a snapshot store, snapshots live in it and every node sharing it
//! offers them: the template under them is content-addressed there, so a
//! snapshot taken on one node restores on any other.

use super::{
    api_error, now_ms, transition_lock, valid_template_name, AppState, Arc, BTreeMap, Deserialize,
    Duration, IntoResponse, Json, Path, Query, Response, Serialize, State, StatusCode,
};
use hv2_cluster::ownership::TeamId;
use serde_json::json;

/// A snapshot offered as a template.
pub(crate) struct Snapshot {
    /// The template its pages are layered over.
    pub base: String,
    pub file: std::path::PathBuf,
    pub sandbox_id: String,
    pub created_ms: u64,
    /// The team of the sandbox it was taken from: the only team that may
    /// use it. `None` for an operator's, which every team may use.
    pub team: Option<TeamId>,
    /// Deleted with the last reference: one taken on a node with no store.
    /// A store's are deleted by `DELETE /templates/{id}` only.
    owned: bool,
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

/// What a snapshot store keeps about a snapshot, beside its pages.
#[derive(Serialize, Deserialize)]
struct Record {
    base: String,
    /// The pages' file, in the store's `snapshots/` directory.
    file: String,
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    #[serde(rename = "createdAt")]
    created_ms: u64,
    #[serde(rename = "teamID", default, skip_serializing_if = "Option::is_none")]
    team: Option<TeamId>,
}

pub(crate) use hv2_cluster::model::untagged;

fn snapshot_id(name: &str) -> String {
    format!("{name}:default")
}

fn info(name: &str) -> serde_json::Value {
    json!({ "snapshotID": snapshot_id(name), "names": [snapshot_id(name)] })
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct SnapshotRequest {
    name: Option<String>,
}

/// `POST /sandboxes/{id}/snapshots`: checkpoint a running sandbox, in
/// place, into a template new sandboxes are created from. The sandbox runs
/// on; it is paused only for the checkpoint. A name already in use as a
/// snapshot is given the new one, as E2B does.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<SnapshotRequest>>,
) -> Response {
    let requested = body.and_then(|Json(b)| b.name);
    let name = match requested.as_deref().map(untagged) {
        Some(name) if !valid_template_name(name) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                format!("snapshot name {name:?}: letters, digits, - _ . only"),
            );
        }
        Some(name) => name.to_string(),
        None => format!("snap-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
    };
    if state.initrds.read().contains_key(&name) {
        return api_error(
            StatusCode::CONFLICT,
            format!("{name} is a template built from an image, not a snapshot"),
        );
    }
    let file = match new_file(&state, &name) {
        Ok(file) => file,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };

    let (base, team) = {
        let lock = transition_lock(&state, &sandbox_id);
        let _held = lock.lock().await;
        let source = state.sandboxes.lock().get(&sandbox_id).map(|live| {
            (
                Arc::clone(&live.vm),
                live.record.template_id.clone(),
                live.record.team_id.clone(),
            )
        });
        let Some((vm, base, team)) = source else {
            return if state.paused.lock().contains_key(&sandbox_id) {
                api_error(
                    StatusCode::CONFLICT,
                    format!("sandbox {sandbox_id} is paused; resume it to snapshot it"),
                )
            } else {
                api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))
            };
        };
        if !state.templates.read().contains_key(&base) {
            return api_error(
                StatusCode::CONFLICT,
                "snapshots need sandboxes restored from a template, and this one was booted",
            );
        }
        // Names are one namespace, so a team cannot take over another's:
        // replacing a snapshot is for the team that owns it.
        if lookup(&state, &name).is_some_and(|existing| existing.team != team) {
            let _ = std::fs::remove_file(&file);
            return api_error(
                StatusCode::CONFLICT,
                format!("the snapshot name {name} is taken"),
            );
        }
        let started = std::time::Instant::now();
        if let Err(e) = vm.checkpoint_to(&file).await {
            let _ = std::fs::remove_file(&file);
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("checkpointing {sandbox_id}: {e}"),
            );
        }
        state.metrics.checkpoint_latency.observe(started.elapsed());
        (base, team)
    };
    if let Err(e) = keep(&state, &name, base, file, sandbox_id, team).await {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    tracing::info!("snapshot {name} taken");
    (StatusCode::CREATED, Json(info(&name))).into_response()
}

/// Where a new snapshot named `name` is written: in the store, when there
/// is one, for every node to restore from.
pub(crate) fn new_file(state: &AppState, name: &str) -> Result<std::path::PathBuf, String> {
    let dir = match &state.store {
        Some(store) => store.dir.join("snapshots"),
        None => state.suspend_dir.join("snapshots"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir.join(format!("{name}-{}.snap", uuid::Uuid::new_v4().simple())))
}

/// Offer the snapshot in `file`, layered over the template `base`, as the
/// template `name` -- in place of any snapshot of that name before it -- and
/// tell the cluster.
pub(crate) async fn keep(
    state: &AppState,
    name: &str,
    base: String,
    file: std::path::PathBuf,
    sandbox_id: String,
    team: Option<TeamId>,
) -> Result<(), String> {
    let snapshot = Snapshot {
        base,
        file,
        sandbox_id,
        created_ms: now_ms(),
        team,
        owned: state.store.is_none(),
    };
    if let (Some(_), Some(dir)) = (&state.store, snapshot.file.parent()) {
        let record = Record {
            base: snapshot.base.clone(),
            file: snapshot
                .file
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default(),
            sandbox_id: snapshot.sandbox_id.clone(),
            created_ms: snapshot.created_ms,
            team: snapshot.team.clone(),
        };
        if let Err(e) = publish(dir, name, &record) {
            let _ = std::fs::remove_file(&snapshot.file);
            return Err(e);
        }
    }
    let replaced = state
        .snapshots
        .write()
        .insert(name.to_string(), Arc::new(snapshot));
    if let Some(old) = replaced {
        forget_file(state, &old);
    }
    announce(state).await;
    Ok(())
}

/// Written beside, then renamed over: another node never reads half of one.
fn publish(dir: &std::path::Path, name: &str, record: &Record) -> Result<(), String> {
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{name}.{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dir.join(format!("{name}.json"))).map_err(|e| e.to_string())
}

/// A store's snapshot file, no longer named by any record: removed. One not
/// in a store goes with its last reference.
fn forget_file(state: &AppState, snapshot: &Snapshot) {
    if state.store.is_some() {
        let _ = std::fs::remove_file(&snapshot.file);
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    #[serde(rename = "sandboxID")]
    sandbox_id: Option<String>,
    name: Option<String>,
}

/// `GET /snapshots`: the snapshots this node offers, E2B's `SnapshotInfo`
/// each, filtered by source sandbox or name as asked.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Response {
    let wanted = query.name.as_deref().map(untagged);
    let listed: Vec<serde_json::Value> = state
        .snapshots
        .read()
        .iter()
        .filter(|(name, s)| {
            wanted.is_none_or(|w| w == name.as_str())
                && query
                    .sandbox_id
                    .as_deref()
                    .is_none_or(|id| id == s.sandbox_id)
        })
        .map(|(name, s)| {
            let mut entry = info(name);
            entry["sandboxID"] = json!(s.sandbox_id);
            entry["templateID"] = json!(s.base);
            entry["createdAt"] = json!(s.created_ms);
            entry["teamID"] = json!(s.team);
            entry
        })
        .collect();
    Json(listed).into_response()
}

/// `DELETE /templates/{id}`: delete a snapshot. Sandboxes already created
/// from it run on; they copied what they needed out of it.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    Path(template): Path<String>,
) -> Response {
    let name = untagged(&template).to_string();
    let Some(removed) = state.snapshots.write().remove(&name) else {
        return if state.initrds.read().contains_key(&name) {
            api_error(
                StatusCode::CONFLICT,
                format!("{name} is a template built from an image; only snapshots are deleted"),
            )
        } else {
            api_error(StatusCode::NOT_FOUND, format!("no snapshot {name}"))
        };
    };
    if let Some(store) = &state.store {
        let _ = std::fs::remove_file(store.dir.join("snapshots").join(format!("{name}.json")));
    }
    forget_file(&state, &removed);
    announce(&state).await;
    StatusCode::NO_CONTENT.into_response()
}

/// The templates this node offers, told to its cluster now rather than at
/// the next heartbeat: a create through a control plane right after a
/// snapshot is routed to a node that has it.
async fn announce(state: &AppState) {
    super::advertise_templates(state);
    if let Some(node) = &state.node {
        let running = u32::try_from(state.sandboxes.lock().len()).unwrap_or(u32::MAX);
        if let Err(e) = node.announce(running).await {
            tracing::warn!("announcing this node's templates: {e}");
        }
    }
}

/// The snapshot `name`: this node's, or one another node took into the
/// store since this one last looked -- a create right after a snapshot
/// finds it wherever it lands.
pub(crate) fn lookup(state: &AppState, name: &str) -> Option<Arc<Snapshot>> {
    if let Some(found) = state.snapshots.read().get(name) {
        return Some(Arc::clone(found));
    }
    let dir = state.store.as_ref()?.dir.join("snapshots");
    let record: Record = std::fs::read(dir.join(format!("{name}.json")))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())?;
    let snapshot = Arc::new(Snapshot {
        base: record.base,
        file: dir.join(&record.file),
        sandbox_id: record.sandbox_id,
        created_ms: record.created_ms,
        team: record.team,
        owned: false,
    });
    state
        .snapshots
        .write()
        .insert(name.to_string(), Arc::clone(&snapshot));
    super::advertise_templates(state);
    Some(snapshot)
}

/// Keep this node's snapshots the store's: those another node took are
/// offered, those another node deleted are not -- at start, then every
/// few seconds.
pub(crate) async fn follow_store(state: Arc<AppState>) {
    let Some(store) = &state.store else {
        return;
    };
    let dir = store.dir.join("snapshots");
    loop {
        // One taken here since the scan began is not in it, and not deleted.
        let scanned_at = now_ms();
        let mut found: BTreeMap<String, Record> = BTreeMap::new();
        if let Ok(listing) = std::fs::read_dir(&dir) {
            for entry in listing.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Some(name) = file.strip_suffix(".json") else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                if let Some(record) = std::fs::read(entry.path())
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Record>(&b).ok())
                {
                    found.insert(name.to_string(), record);
                }
            }
        }
        let changed = {
            let mut snapshots = state.snapshots.write();
            let before = snapshots.len();
            let mut changed = false;
            snapshots.retain(|name, s| found.contains_key(name) || s.created_ms >= scanned_at);
            changed |= snapshots.len() != before;
            for (name, record) in found {
                let file = dir.join(&record.file);
                if snapshots.get(&name).is_some_and(|s| s.file == file) {
                    continue;
                }
                snapshots.insert(
                    name,
                    Arc::new(Snapshot {
                        base: record.base,
                        file,
                        sandbox_id: record.sandbox_id,
                        created_ms: record.created_ms,
                        team: record.team,
                        owned: false,
                    }),
                );
                changed = true;
            }
            changed
        };
        if changed {
            super::advertise_templates(&state);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record written before teams is an operator's snapshot; a team's
    /// round-trips with its team.
    #[test]
    fn a_snapshot_record_carries_its_team_and_older_ones_have_none() {
        let old: Record = serde_json::from_str(
            r#"{"base":"base","file":"a.snap","sandboxID":"sbx","createdAt":1}"#,
        )
        .unwrap();
        assert!(old.team.is_none());
        let red = Record {
            team: Some(TeamId::parse("red").unwrap()),
            ..old
        };
        let written = serde_json::to_string(&red).unwrap();
        assert!(written.contains(r#""teamID":"red""#));
        let back: Record = serde_json::from_str(&written).unwrap();
        assert_eq!(back.team, red.team);
    }
}
