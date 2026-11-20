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
use hv2_cluster::ownership::{RequestTeam, TeamId};
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
    /// Other teams its owner let start sandboxes from it. Changed in place:
    /// a share does not make a new snapshot.
    pub shared_with: parking_lot::RwLock<Vec<TeamId>>,
    /// The size of the guest it was taken from, when that is not its base
    /// template's: a sandbox started from it is that size.
    pub sizes: Option<crate::Sizes>,
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
    #[serde(rename = "sharedWith", default, skip_serializing_if = "Vec::is_empty")]
    shared_with: Vec<TeamId>,
    /// Both or neither: the size, where it is not the base template's.
    #[serde(rename = "cpuCount", default, skip_serializing_if = "Option::is_none")]
    cpu_count: Option<u32>,
    #[serde(rename = "memoryMB", default, skip_serializing_if = "Option::is_none")]
    memory_mb: Option<u64>,
}

impl Record {
    fn sizes(&self) -> Option<crate::Sizes> {
        Some(crate::Sizes {
            cpus: self.cpu_count?,
            memory_mb: self.memory_mb?,
        })
    }
}

/// The size of a guest started from `snapshot`: its own where it has one,
/// and otherwise its base template's.
pub(crate) fn size_of(state: &AppState, snapshot: &Snapshot) -> crate::Sizes {
    snapshot
        .sizes
        .unwrap_or_else(|| crate::sizes_of(state, &snapshot.base))
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
    if crate::disks::holds_one(&sandbox_id) {
        return api_error(
            StatusCode::CONFLICT,
            format!("sandbox {sandbox_id} holds a disk, and a sandbox with a disk cannot be snapshotted"),
        );
    }
    let file = match new_file(&state, &name) {
        Ok(file) => file,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };

    let (base, team, sizes) = {
        let lock = transition_lock(&state, &sandbox_id);
        let _held = lock.lock().await;
        let source = state.sandboxes.lock().get(&sandbox_id).map(|live| {
            (
                Arc::clone(&live.vm),
                live.record.template_id.clone(),
                live.record.team_id.clone(),
                // A size of its own is kept with the snapshot, and its
                // memory is written whole: what it changed is not against
                // its template's image.
                super::resized(&state, &live.record).then_some(crate::Sizes {
                    cpus: live.record.cpu_count,
                    memory_mb: live.record.memory_mb,
                }),
            )
        });
        let Some((vm, base, team, sizes)) = source else {
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
        let written = if sizes.is_some() {
            vm.checkpoint_with(&file, true, || Ok::<(), String>(()))
                .await
        } else {
            vm.checkpoint_to(&file).await
        };
        if let Err(e) = written {
            let _ = std::fs::remove_file(&file);
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("checkpointing {sandbox_id}: {e}"),
            );
        }
        state.metrics.checkpoint_latency.observe(started.elapsed());
        (base, team, sizes)
    };
    if let Err(e) = keep(&state, &name, base, file, sandbox_id, team, sizes).await {
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
    sizes: Option<crate::Sizes>,
) -> Result<(), String> {
    let snapshot = Snapshot {
        base,
        file,
        sandbox_id,
        created_ms: now_ms(),
        team,
        shared_with: parking_lot::RwLock::new(Vec::new()),
        sizes,
        owned: state.store.is_none(),
    };
    if let (Some(_), Some(dir)) = (&state.store, snapshot.file.parent()) {
        let record = record_of(&snapshot);
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

/// What a store keeps about `snapshot`.
fn record_of(snapshot: &Snapshot) -> Record {
    Record {
        base: snapshot.base.clone(),
        file: snapshot
            .file
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sandbox_id: snapshot.sandbox_id.clone(),
        created_ms: snapshot.created_ms,
        team: snapshot.team.clone(),
        shared_with: snapshot.shared_with.read().clone(),
        cpu_count: snapshot.sizes.map(|s| s.cpus),
        memory_mb: snapshot.sizes.map(|s| s.memory_mb),
    }
}

/// Whether a sandbox of `team` may be started from `snapshot`: an operator's
/// is every team's, a team's is that team's and those it was shared with, and
/// a caller with no team gets none of a team's.
pub(crate) fn usable_by(snapshot: &Snapshot, team: Option<&TeamId>) -> bool {
    match &snapshot.team {
        None => true,
        Some(owner) => {
            Some(owner) == team || team.is_some_and(|t| snapshot.shared_with.read().contains(t))
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ShareRequest {
    #[serde(rename = "acknowledgeContents", default)]
    acknowledge_contents: bool,
}

/// The team's snapshot `name` and the team to share it with, or why not.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn shareable(
    state: &AppState,
    name: &str,
    with: &str,
) -> Result<(Arc<Snapshot>, TeamId), Response> {
    let name = untagged(name);
    let Some(snapshot) = lookup(state, name) else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("no snapshot {name}"),
        ));
    };
    let Some(owner) = &snapshot.team else {
        return Err(api_error(
            StatusCode::CONFLICT,
            format!("{name} is an operator's template, which every team already uses"),
        ));
    };
    let with = TeamId::parse(with).map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    if &with == owner {
        return Err(api_error(
            StatusCode::CONFLICT,
            format!("{name} is that team's own snapshot"),
        ));
    }
    Ok((snapshot, with))
}

/// Write a changed share list to the store, where another node reads it, and
/// tell the cluster.
async fn shared_changed(state: &AppState, name: &str, snapshot: &Snapshot) -> Result<(), String> {
    if let Some(store) = &state.store {
        publish(&store.dir.join("snapshots"), name, &record_of(snapshot))?;
    }
    announce(state).await;
    Ok(())
}

/// `PUT /templates/{id}/shares/{team}`: let `team` start sandboxes from this
/// team's snapshot. What it starts from is the source sandbox as it was --
/// its memory, its files and its environment variables -- so the request
/// must say it knows.
pub(crate) async fn share(
    State(state): State<Arc<AppState>>,
    Path((name, with)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    let (snapshot, with) = match shareable(&state, &name, &with) {
        Ok(found) => found,
        Err(response) => return response,
    };
    // A missing or malformed body has not said it.
    if !serde_json::from_slice::<ShareRequest>(&body).is_ok_and(|b| b.acknowledge_contents) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "a shared snapshot gives the other team the sandbox's memory, files and environment \
             variables as they were: send {\"acknowledgeContents\": true} to share it",
        );
    }
    {
        let mut shared = snapshot.shared_with.write();
        if !shared.contains(&with) {
            shared.push(with);
        }
    }
    if let Err(e) = shared_changed(&state, untagged(&name), &snapshot).await {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    let shared = snapshot.shared_with.read().clone();
    Json(json!({
        "snapshotID": snapshot_id(untagged(&name)),
        "teamID": snapshot.team,
        "sharedWith": shared,
    }))
    .into_response()
}

/// `DELETE /templates/{id}/shares/{team}`: stop. Sandboxes the team already
/// started from it run on.
pub(crate) async fn unshare(
    State(state): State<Arc<AppState>>,
    Path((name, with)): Path<(String, String)>,
) -> Response {
    let (snapshot, with) = match shareable(&state, &name, &with) {
        Ok(found) => found,
        Err(response) => return response,
    };
    snapshot.shared_with.write().retain(|t| *t != with);
    if let Err(e) = shared_changed(&state, untagged(&name), &snapshot).await {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    StatusCode::NO_CONTENT.into_response()
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
            entry["sharedWith"] = json!(s.shared_with.read().clone());
            entry
        })
        .collect();
    Json(listed).into_response()
}

/// Whether a caller acting for `caller` may delete the snapshot `name`, which
/// belongs to `owner`: a team deletes its own, and a teamless principal its
/// own scope's templates by their scoped names. No caller (an administrator)
/// deletes any. Mirrors the control plane's rule, which names the same
/// scopes, so that a node refuses what the control plane would.
pub(crate) fn deletable_by(name: &str, owner: Option<&TeamId>, caller: Option<&TeamId>) -> bool {
    let Some(caller) = caller else {
        return true;
    };
    if !RequestTeam(Some(caller.clone())).may_name_template(name) {
        return false;
    }
    match owner {
        Some(owner) => owner == caller,
        // An operator's template, or a build's: no owner on the node, so
        // only a principal's own scope's, by the name it was built under.
        None => caller
            .template_prefix()
            .is_some_and(|prefix| name.starts_with(&prefix)),
    }
}

/// `DELETE /templates/{id}`: delete a snapshot. Sandboxes already created
/// from it run on; they copied what they needed out of it.
///
/// The control plane sends the caller's team with the delete, as it does
/// with every team call, and a call with a team may delete only what
/// [`deletable_by`] allows. Another scope's snapshot is answered as a missing
/// one is, and so is an image-built template, so a scoped caller learns no
/// names. With no team (an administrator) the delete is as it always was.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(template): Path<String>,
) -> Response {
    let name = untagged(&template).to_string();
    let caller = match super::caller_team(&state, &headers) {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    // Checked and removed under one lock, so nothing changes between.
    let removed = {
        let mut snapshots = state.snapshots.write();
        if caller.is_some()
            && !snapshots
                .get(&name)
                .is_some_and(|s| deletable_by(&name, s.team.as_ref(), caller.as_ref()))
        {
            return api_error(StatusCode::NOT_FOUND, format!("no snapshot {name}"));
        }
        snapshots.remove(&name)
    };
    let Some(removed) = removed else {
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
    let sizes = record.sizes();
    let snapshot = Arc::new(Snapshot {
        base: record.base,
        file: dir.join(&record.file),
        sandbox_id: record.sandbox_id,
        created_ms: record.created_ms,
        team: record.team,
        shared_with: parking_lot::RwLock::new(record.shared_with),
        sizes,
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
                if let Some(existing) = snapshots.get(&name).filter(|s| s.file == file) {
                    // Shared or unshared by another node since.
                    if *existing.shared_with.read() != record.shared_with {
                        *existing.shared_with.write() = record.shared_with;
                        changed = true;
                    }
                    continue;
                }
                let sizes = record.sizes();
                snapshots.insert(
                    name,
                    Arc::new(Snapshot {
                        base: record.base,
                        file,
                        sandbox_id: record.sandbox_id,
                        created_ms: record.created_ms,
                        team: record.team,
                        shared_with: parking_lot::RwLock::new(record.shared_with),
                        sizes,
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

    /// A share list is kept with the record, and an older record has none.
    #[test]
    fn a_snapshot_record_carries_who_it_is_shared_with() {
        let old: Record = serde_json::from_str(
            r#"{"base":"base","file":"a.snap","sandboxID":"sbx","createdAt":1,"teamID":"red"}"#,
        )
        .unwrap();
        assert!(old.shared_with.is_empty());
        assert!(!serde_json::to_string(&old).unwrap().contains("sharedWith"));
        let shared = Record {
            shared_with: vec![TeamId::parse("blue").unwrap()],
            ..old
        };
        let written = serde_json::to_string(&shared).unwrap();
        assert!(written.contains(r#""sharedWith":["blue"]"#), "{written}");
        let back: Record = serde_json::from_str(&written).unwrap();
        assert_eq!(back.shared_with, shared.shared_with);
    }

    /// An operator's snapshot is every team's; a team's is its own and those
    /// it was shared with, and never a caller with no team's.
    #[test]
    fn a_shared_snapshot_is_usable_by_its_owner_and_the_teams_shared_with() {
        let team = |t: &str| TeamId::parse(t).unwrap();
        let snapshot = |owner: Option<&str>, shared: &[&str]| Snapshot {
            base: "base".into(),
            file: std::path::PathBuf::from("a.snap"),
            sandbox_id: "sbx".into(),
            created_ms: 1,
            team: owner.map(team),
            shared_with: parking_lot::RwLock::new(shared.iter().map(|t| team(t)).collect()),
            sizes: None,
            owned: false,
        };
        let operators = snapshot(None, &[]);
        assert!(usable_by(&operators, None) && usable_by(&operators, Some(&team("blue"))));
        let reds = snapshot(Some("red"), &["blue"]);
        assert!(usable_by(&reds, Some(&team("red"))));
        assert!(usable_by(&reds, Some(&team("blue"))));
        assert!(!usable_by(&reds, Some(&team("green"))));
        assert!(!usable_by(&reds, None));
        reds.shared_with.write().clear();
        assert!(!usable_by(&reds, Some(&team("blue"))));
    }

    /// A team deletes its own snapshots and no other team's; a principal its
    /// own scope's templates, by their scoped names, and never another's; an
    /// administrator any.
    #[test]
    fn a_team_deletes_its_own_snapshots_and_a_principal_its_own_templates() {
        let team = |t: &str| TeamId::parse(t).unwrap();
        let scope_a = TeamId::personal(&hv2_cluster::ownership::OwnerId::parse("alice").unwrap());
        let scope_b = TeamId::personal(&hv2_cluster::ownership::OwnerId::parse("bob").unwrap());
        let (a, b) = (
            scope_a.template_prefix().unwrap(),
            scope_b.template_prefix().unwrap(),
        );
        let (red, blue) = (team("red"), team("blue"));

        // A team's snapshot: its own team's, and nobody else's.
        assert!(deletable_by("snap", Some(&red), Some(&red)));
        assert!(!deletable_by("snap", Some(&red), Some(&blue)));
        assert!(!deletable_by("snap", Some(&red), Some(&scope_a)));
        assert!(deletable_by("snap", Some(&red), None));

        // A principal's build under its scope: its own, by the scoped name.
        let mine = format!("{a}app");
        assert!(deletable_by(&mine, None, Some(&scope_a)));
        assert!(!deletable_by(&mine, None, Some(&scope_b)));
        assert!(!deletable_by(&mine, None, Some(&red)));
        assert!(deletable_by(&mine, None, None));

        // Another scope's name, raw, is refused to every caller but an administrator,
        // even when the owner recorded is the caller's own.
        let theirs = format!("{b}app");
        assert!(!deletable_by(&theirs, None, Some(&scope_a)));
        assert!(!deletable_by(&theirs, None, Some(&red)));
        assert!(deletable_by(&theirs, None, Some(&scope_b)));
        assert!(deletable_by(&theirs, None, None));
        assert!(!deletable_by(
            &format!("{a}app"),
            Some(&scope_b),
            Some(&scope_a)
        ));

        // A team may not reach a scope's name: a team has no scope to name it by.
        assert!(!deletable_by(&theirs, Some(&scope_b), Some(&red)));
        assert!(!deletable_by(&mine, Some(&scope_a), Some(&red)));

        // An operator's template, or a build made before scopes: no owner, so
        // only an administrator deletes it; a scoped caller may not.
        assert!(!deletable_by("base", None, Some(&red)));
        assert!(!deletable_by("app", None, Some(&scope_a)));
        assert!(deletable_by("base", None, None));
        assert!(deletable_by("app", None, None));
    }

    /// A size is kept with a snapshot of a sandbox of its own, and a record
    /// without one is the base template's.
    #[test]
    fn a_snapshot_record_carries_a_size_of_its_own_and_older_ones_have_none() {
        let old: Record = serde_json::from_str(
            r#"{"base":"base","file":"a.snap","sandboxID":"sbx","createdAt":1}"#,
        )
        .unwrap();
        assert!(old.sizes().is_none());
        let sized = Record {
            cpu_count: Some(2),
            memory_mb: Some(1024),
            ..old
        };
        let written = serde_json::to_string(&sized).unwrap();
        assert!(written.contains(r#""cpuCount":2"#) && written.contains(r#""memoryMB":1024"#));
        let back: Record = serde_json::from_str(&written).unwrap();
        let size = back.sizes().unwrap();
        assert_eq!((size.cpus, size.memory_mb), (2, 1024));
        // Half a size is none: it cannot say what the guest was.
        let half = Record {
            memory_mb: None,
            ..back
        };
        assert!(half.sizes().is_none());
    }
}
