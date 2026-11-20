//! Volumes: E2B's persistent storage. A volume is a directory on the node
//! -- in the snapshot store when there is one, so every node has the same
//! volumes -- that sandboxes mount (`volumeMounts` on create), live and
//! shared, and that outlives them.
//!
//! - `POST /volumes`, `GET /volumes`, `GET` and `DELETE /volumes/{id}`, and
//!   `POST /volumes/{id}/token` (rotate): E2B's volume API, behind the API
//!   key. The content token is returned by create and rotate only; the node
//!   keeps just its SHA-256.
//! - `/volumecontent/{id}/{file,dir,path}`: E2B's volume content API, which
//!   the SDK's `Volume` calls with the volume's own bearer token.
//! - A mount is a 9P2000.L server ([`crate::ninep`]) on a vsock connection
//!   the guest's kernel mounts; see there for how a guest is kept inside it.
//!
//! The content API resolves paths the way the 9P server does -- beneath
//! the volume, no symlinks followed -- so it cannot be steered out either.
//!
//! A volume made for a team ([`hv2_cluster::ownership::TEAM_HEADER`], from
//! an authenticated control plane) belongs to it: its ID is derived from the
//! team and the name, the volume API shows it to that team only, and a
//! sandbox mounts by name from its own team's volumes. Without the header --
//! an administrator, or a deployment without teams -- every volume is in
//! reach, as before.

use std::os::fd::AsRawFd;
use std::time::Duration;

use axum::body::Body;
use axum::http::HeaderMap;
use serde_json::{json, Value};

use hv2_agent::AgentVM;
use hv2_cluster::model::VolumeMount;
use hv2_cluster::ownership::TeamId;

use super::{
    api_error, new_access_token, AppState, Arc, Deserialize, IntoResponse, Json, Path, Query,
    Response, Serialize, State, StatusCode,
};
use crate::ninep::quota::{self, Budget, Cost};
use crate::ninep::{self, Beneath};

/// How long a guest has to mount a volume.
const MOUNT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most one file written through the content API may hold.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// What is kept about a volume beside its files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Meta {
    #[serde(rename = "volumeID")]
    id: String,
    name: String,
    /// SHA-256 of the content token, lowercase hex. The token itself is never
    /// written: it is returned once, by create and by rotate.
    #[serde(rename = "tokenSHA256", default)]
    token_sha256: String,
    /// The token of a volume made before tokens were hashed. Read only to
    /// migrate it (see `read_meta`); never written back.
    #[serde(rename = "token", default, skip_serializing)]
    legacy_token: Option<String>,
    /// The team it belongs to; `None` outside a multi-tenant deployment.
    #[serde(rename = "teamID", default, skip_serializing_if = "Option::is_none")]
    team: Option<TeamId>,
}

impl Meta {
    /// Whether a caller acting for `team` may reach it: anyone without a
    /// team reaches every volume, a team only its own.
    fn reachable_by(&self, team: Option<&TeamId>) -> bool {
        team.is_none_or(|team| self.team.as_ref() == Some(team))
    }
}

/// The team the control plane says the caller acts for, if any.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn caller_team(state: &AppState, headers: &HeaderMap) -> Result<Option<TeamId>, Response> {
    super::creator_team(
        headers,
        state.node.is_some(),
        state
            .opts
            .cluster_token
            .as_ref()
            .is_some_and(|token| !token.is_empty()),
    )
    .map_err(|(status, error)| api_error(status, error))
}

fn root(state: &AppState) -> std::path::PathBuf {
    match (&state.opts.volume_dir, &state.store) {
        (Some(dir), _) => std::path::PathBuf::from(dir),
        (None, Some(store)) => store.dir.join("volumes"),
        (None, None) => state.suspend_dir.join("volumes"),
    }
}

fn data_dir(state: &AppState, id: &str) -> std::path::PathBuf {
    root(state).join(id).join("data")
}

/// `--volume-reserve-percent`: keep this much of a volume's filesystem free
/// (0 turns the guard off).
pub(crate) fn set_reserve_percent(percent: u64) {
    quota::configure(|policy| policy.reserve_percent = percent);
}

/// `--volume-max-bytes`: the most contents one volume may hold (0: no limit).
pub(crate) fn set_max_bytes(bytes: u64) {
    quota::configure(|policy| policy.max_bytes = bytes);
}

/// `--volume-max-inodes`: the most entries one volume may hold (0: no limit).
pub(crate) fn set_max_inodes(inodes: u64) {
    quota::configure(|policy| policy.max_inodes = inodes);
}

/// What the volume at `dir` may still grow by, shared with its mounts. The
/// first call per volume walks it when a quota is set, so it runs off the
/// async runtime.
async fn budget(dir: &std::path::Path) -> Arc<Budget> {
    let owned = dir.to_path_buf();
    match tokio::task::spawn_blocking(move || quota::budget_for(&owned)).await {
        Ok(budget) => budget,
        Err(_) => quota::budget_for(dir),
    }
}

fn all(state: &AppState) -> Vec<Meta> {
    all_in(&root(state))
}

/// Every volume under `root`, by name. A directory whose name starts with a
/// dot is not one: a deleted volume is renamed to such a name before its
/// files go.
fn all_in(root: &std::path::Path) -> Vec<Meta> {
    let Ok(listing) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut volumes: Vec<Meta> = listing
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| read_meta(&e.path()))
        .collect();
    volumes.sort_by(|a, b| a.name.cmp(&b.name));
    volumes
}

fn by_id(state: &AppState, id: &str) -> Option<Meta> {
    by_id_in(&root(state), id)
}

fn by_id_in(root: &std::path::Path, id: &str) -> Option<Meta> {
    if !valid_name(id) {
        return None;
    }
    read_meta(&root.join(id))
}

/// `team`'s volume named `name`. Names are unique within a team, and the
/// same name in two teams is two volumes.
fn by_name(state: &AppState, team: Option<&TeamId>, name: &str) -> Option<Meta> {
    all(state)
        .into_iter()
        .find(|v| v.name == name && v.team.as_ref() == team)
}

/// E2B's pattern for a volume name, which an ID also meets.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What create and rotate answer: the volume and its token, once.
fn with_token(meta: &Meta, token: &str) -> Value {
    json!({ "volumeID": meta.id, "name": meta.name, "token": token })
}

/// What get answers: the volume with no token. The key stays, because E2B's
/// `VolumeAndToken` schema requires it; it is null.
fn without_token(meta: &Meta) -> Value {
    json!({ "volumeID": meta.id, "name": meta.name, "token": Value::Null })
}

/// Reserve the deterministic ID before publishing metadata. An existing
/// directory is never reused, including incomplete or concurrently-created
/// volumes, so a duplicate cannot replace an owner's access token or data.
fn create_metadata(root: &std::path::Path, meta: &Meta) -> std::io::Result<()> {
    create_metadata_with_sync(root, meta, |path| std::fs::File::open(path)?.sync_all())
}

fn create_metadata_with_sync(
    root: &std::path::Path,
    meta: &Meta,
    mut sync: impl FnMut(&std::path::Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::create_dir_all(root)?;
    let directory = root.join(&meta.id);
    std::fs::create_dir(&directory)?;
    let temporary = directory.join(".meta.json");
    let data = directory.join("data");
    let mut published = false;
    let result = std::fs::create_dir(&data).and_then(|()| {
        let bytes = serde_json::to_vec(meta).map_err(std::io::Error::other)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        sync(&temporary)?;
        sync(&data)?;
        std::fs::rename(&temporary, directory.join("meta.json"))?;
        published = true;
        sync(&directory)?;
        sync(root)
    });
    if result.is_err() && !published {
        // Only our unpublished reservation; no recursive cleanup and no
        // existing metadata/data is overwritten or removed on conflict.
        let _ = std::fs::remove_file(&temporary);
        let _ = std::fs::remove_dir(&data);
        let _ = std::fs::remove_dir(&directory);
    }
    result
}

/// SHA-256 of a content token, as lowercase hex: the only form of a token
/// `meta.json` keeps. Tokens are random and long, so an unsalted digest is
/// enough to make a leaked metadata file useless as a credential.
fn token_digest(token: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A new volume's metadata, and its token, which only the caller keeps.
fn new_meta(name: String, team: Option<TeamId>) -> (Meta, String) {
    let token = new_access_token();
    let meta = Meta {
        id: hv2_cluster::model::team_volume_id(team.as_ref(), &name),
        name,
        token_sha256: token_digest(&token),
        legacy_token: None,
        team,
    };
    (meta, token)
}

/// The metadata of the volume in `directory`. A file written before tokens
/// were hashed carries its token in the clear: the token is hashed here and
/// the file rewritten, so the plaintext is gone after the first read. If the
/// rewrite fails the volume is still served, from memory, and the next read
/// tries again.
///
/// The rewrite happens under [`METADATA_WRITES`] and only if the file still
/// holds a plaintext token when re-read there, so a read that began before a
/// rotation cannot write the old token's digest back over the new one.
fn read_meta(directory: &std::path::Path) -> Option<Meta> {
    let parse = || -> Option<Meta> {
        let bytes = std::fs::read(directory.join("meta.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    };
    let meta = parse()?;
    if meta.legacy_token.is_none() {
        return Some(meta);
    }
    let _writing = METADATA_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut meta = parse()?;
    if let Some(token) = meta.legacy_token.take() {
        meta.token_sha256 = token_digest(&token);
        if let Err(e) = publish_metadata(directory, &meta) {
            tracing::warn!(
                "volume {}: its token is not yet hashed on disk: {e}",
                meta.id
            );
        }
    }
    Some(meta)
}

/// Held while a volume's `meta.json` is rewritten after creation (token
/// migration and rotation), so the two cannot interleave.
static METADATA_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Replace `directory`'s metadata atomically, as creation publishes it: a
/// private temporary file is written and synced, renamed over `meta.json`,
/// and the directory synced.
fn publish_metadata(directory: &std::path::Path, meta: &Meta) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let bytes = serde_json::to_vec(meta).map_err(std::io::Error::other)?;
    let temporary = directory.join(format!(".meta.json.{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, directory.join("meta.json"))?;
        std::fs::File::open(directory)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Give `meta` a new token and publish it. The old token stops working when
/// the new metadata is in place.
fn rotate_in(root: &std::path::Path, meta: Meta) -> std::io::Result<(Meta, String)> {
    let token = new_access_token();
    let updated = Meta {
        token_sha256: token_digest(&token),
        legacy_token: None,
        ..meta
    };
    let _writing = METADATA_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    publish_metadata(&root.join(&updated.id), &updated)?;
    Ok((updated, token))
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewVolume {
    name: String,
}

/// `POST /volumes`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewVolume>,
) -> Response {
    let team = match caller_team(&state, &headers) {
        Ok(team) => team,
        Err(response) => return response,
    };
    if !valid_name(&req.name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("volume name {:?}: letters, digits, _ and - only", req.name),
        );
    }
    if by_name(&state, team.as_ref(), &req.name).is_some() {
        return api_error(StatusCode::CONFLICT, format!("volume {} exists", req.name));
    }
    let (meta, token) = new_meta(req.name, team);
    let dir = root(&state).join(&meta.id);
    let made = create_metadata(&root(&state), &meta);
    if let Err(e) = made {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            return api_error(StatusCode::CONFLICT, format!("volume {} exists", meta.name));
        }
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{}: {e}", dir.display()),
        );
    }
    (StatusCode::CREATED, Json(with_token(&meta, &token))).into_response()
}

#[cfg(test)]
mod team_tests {
    use super::*;

    /// A volume written before teams reads back as no team's, which only a
    /// caller without a team reaches; a team reaches its own and no other.
    #[test]
    fn a_volume_is_reachable_by_its_team_and_by_callers_without_one() {
        let old: Meta =
            serde_json::from_str(r#"{"volumeID":"vol-1","name":"data","token":"t"}"#).unwrap();
        assert!(old.team.is_none());
        let red = TeamId::parse("red").unwrap();
        let blue = TeamId::parse("blue").unwrap();
        assert!(old.reachable_by(None));
        assert!(!old.reachable_by(Some(&red)));
        let reds = Meta {
            team: Some(red.clone()),
            ..old
        };
        assert!(reds.reachable_by(None));
        assert!(reds.reachable_by(Some(&red)));
        assert!(!reds.reachable_by(Some(&blue)));
        let written = serde_json::to_value(&reds).unwrap();
        assert_eq!(written["teamID"], "red");
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;

    /// The volume API's create, chmod and PUT keep no setuid or setgid bit,
    /// whatever mode a request asks for; permissions and sticky are kept.
    #[test]
    fn the_volume_api_stores_no_setuid_or_setgid_bit() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("hm-volume-modes-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let fs = Beneath::open(&root).unwrap();
        let mode = |name: &str| {
            std::fs::symlink_metadata(root.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        // PUT without an atomic stage: the file is created with the mode.
        let fd = fs.open_at(&["put".into()], libc::O_WRONLY | libc::O_CREAT, 0o6755);
        drop(fd.unwrap());
        assert_eq!(mode("put"), 0o755);
        // mkdir, as the directory endpoint does it.
        mkdir(&fs, &quota::budget_for(&root), &["dir".into()], 0o3777).unwrap();
        // Sticky is kept (the umask may take permission bits); setgid is not.
        assert_eq!(mode("dir") & 0o7000, 0o1000);
        // chmod of an existing file or directory, and the atomic PUT's own.
        apply(&fs, &["put".into()], None, None, Some(0o4777)).unwrap();
        assert_eq!(mode("put"), 0o777);
        apply(&fs, &["dir".into()], None, None, Some(0o2755)).unwrap();
        assert_eq!(mode("dir"), 0o755);
        let file = std::fs::File::open(root.join("put")).unwrap();
        ninep::chmod(std::os::fd::AsRawFd::as_raw_fd(&file), 0o6644).unwrap();
        assert_eq!(mode("put"), 0o644);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_upload_preserves_previous_file_until_publication() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("hm-atomic-upload-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("target"), b"previous").unwrap();
        let fs = Beneath::open(&root).unwrap();
        let parts = vec!["target".to_string()];
        {
            let (_stage, mut file) = AtomicUpload::new(&fs, &parts).unwrap();
            file.write_all(b"interrupted").unwrap();
            assert_eq!(std::fs::read(root.join("target")).unwrap(), b"previous");
        }
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        {
            let (mut stage, mut file) = AtomicUpload::new(&fs, &parts).unwrap();
            file.write_all(b"replacement").unwrap();
            file.sync_all().unwrap();
            stage.publish(true).unwrap();
        }
        assert_eq!(std::fs::read(root.join("target")).unwrap(), b"replacement");
        {
            let (mut stage, mut file) = AtomicUpload::new(&fs, &parts).unwrap();
            file.write_all(b"must not replace").unwrap();
            assert_eq!(
                stage.publish(false).unwrap_err().kind(),
                std::io::ErrorKind::AlreadyExists
            );
        }
        assert_eq!(std::fs::read(root.join("target")).unwrap(), b"replacement");
        {
            let absent = vec!["new-file".into()];
            let (mut stage, mut file) = AtomicUpload::new(&fs, &absent).unwrap();
            file.write_all(b"new bytes").unwrap();
            stage.publish(false).unwrap();
        }
        assert_eq!(std::fs::read(root.join("new-file")).unwrap(), b"new bytes");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::os::unix::fs::symlink("target", root.join("link")).unwrap();
        assert!(AtomicUpload::new(&fs, &["link".into()]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sync_failures_preserve_published_metadata_and_data() {
        for failed_stage in 1..=4 {
            let root =
                std::env::temp_dir().join(format!("hm-volume-sync-{}", uuid::Uuid::new_v4()));
            let meta = Meta {
                id: "vol-owned".into(),
                name: "owned".into(),
                token_sha256: token_digest("owned-token"),
                legacy_token: None,
                team: None,
            };
            let mut stage = 0;
            let result = create_metadata_with_sync(&root, &meta, |_| {
                stage += 1;
                if stage == failed_stage {
                    Err(std::io::Error::other("injected sync failure"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            let directory = root.join(&meta.id);
            if failed_stage < 3 {
                assert!(!directory.exists());
            } else {
                assert_eq!(
                    std::fs::read(directory.join("meta.json")).unwrap(),
                    serde_json::to_vec(&meta).unwrap()
                );
                assert!(directory.join("data").is_dir());
                assert_eq!(
                    create_metadata(&root, &meta).unwrap_err().kind(),
                    std::io::ErrorKind::AlreadyExists
                );
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn concurrent_creators_preserve_the_winning_token_and_data() {
        let root = std::env::temp_dir().join(format!("hm-volume-create-{}", uuid::Uuid::new_v4()));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let workers: Vec<_> = (0..16)
            .map(|index| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let meta = Meta {
                        id: "vol-owned".into(),
                        name: "owned".into(),
                        token_sha256: token_digest(&index.to_string()),
                        legacy_token: None,
                        team: None,
                    };
                    barrier.wait();
                    (meta.token_sha256.clone(), create_metadata(&root, &meta))
                })
            })
            .collect();
        let mut winner = None;
        for worker in workers {
            let (token, result) = worker.join().unwrap();
            match result {
                Ok(()) => {
                    assert!(winner.is_none());
                    winner = Some(token);
                }
                Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists),
            }
        }
        let metadata = root.join("vol-owned/meta.json");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&metadata).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let before = std::fs::read(&metadata).unwrap();
        let meta: Meta = serde_json::from_slice(&before).unwrap();
        assert_eq!(Some(meta.token_sha256), winner);
        let marker = root.join("vol-owned/data/marker");
        std::fs::write(&marker, b"owned bytes").unwrap();
        let duplicate = Meta {
            id: "vol-owned".into(),
            name: "owned".into(),
            token_sha256: token_digest("replacement"),
            legacy_token: None,
            team: None,
        };
        assert_eq!(
            create_metadata(&root, &duplicate).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(metadata).unwrap(), before);
        assert_eq!(std::fs::read(marker).unwrap(), b"owned bytes");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn incomplete_reservations_are_never_overwritten() {
        let root =
            std::env::temp_dir().join(format!("hm-volume-incomplete-{}", uuid::Uuid::new_v4()));
        let reserved = root.join("vol-owned");
        std::fs::create_dir_all(&reserved).unwrap();
        std::fs::write(reserved.join("marker"), b"existing reservation").unwrap();
        let meta = Meta {
            id: "vol-owned".into(),
            name: "owned".into(),
            token_sha256: token_digest("replacement"),
            legacy_token: None,
            team: None,
        };
        assert_eq!(
            create_metadata(&root, &meta).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert!(!reserved.join("meta.json").exists());
        assert_eq!(
            std::fs::read(reserved.join("marker")).unwrap(),
            b"existing reservation"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("hm-volume-token-{name}-{}", uuid::Uuid::new_v4()))
    }

    /// Creation writes only the token's digest: the file holds no plaintext.
    #[test]
    fn meta_json_holds_no_plaintext_token_after_create() {
        let root = scratch("create");
        let (meta, token) = new_meta("workspace".into(), None);
        create_metadata(&root, &meta).unwrap();
        let on_disk = std::fs::read_to_string(root.join(&meta.id).join("meta.json")).unwrap();
        assert!(!on_disk.contains(&token), "plaintext token on disk");
        assert!(!on_disk.contains("\"token\""));
        assert!(on_disk.contains(&token_digest(&token)));
        assert!(authorize(&root, &meta.id, &token).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A volume written with a plaintext token keeps working, and its file is
    /// rewritten without the token on first read.
    #[test]
    fn a_legacy_plaintext_token_is_migrated_and_still_works() {
        let root = scratch("legacy");
        let directory = root.join("vol-legacy");
        std::fs::create_dir_all(directory.join("data")).unwrap();
        let legacy = "t".repeat(32);
        let old = json!({ "volumeID": "vol-legacy", "name": "legacy", "token": legacy });
        std::fs::write(
            directory.join("meta.json"),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        assert!(authorize(&root, "vol-legacy", &legacy).is_ok());
        let on_disk = std::fs::read_to_string(directory.join("meta.json")).unwrap();
        assert!(!on_disk.contains(&legacy), "plaintext survived migration");
        assert!(on_disk.contains(&token_digest(&legacy)));
        assert!(authorize(&root, "vol-legacy", &"u".repeat(32)).is_err());
        assert!(directory.join("data").is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Get's answer carries no token; the key stays, as null.
    #[test]
    fn get_never_shows_the_token() {
        let (meta, token) = new_meta("shown".into(), None);
        let answer = serde_json::to_string(&without_token(&meta)).unwrap();
        assert!(!answer.contains(&token));
        assert!(without_token(&meta)["token"].is_null());
    }

    /// Rotation: the new token works, the old one stops, the volume and its
    /// team stay.
    #[test]
    fn rotation_replaces_the_token_and_keeps_the_volume() {
        let root = scratch("rotate");
        let (meta, old) = new_meta("rotated".into(), None);
        create_metadata(&root, &meta).unwrap();
        assert!(authorize(&root, &meta.id, &old).is_ok());
        let (updated, new) = rotate_in(&root, meta.clone()).unwrap();
        assert_ne!(new, old);
        assert_eq!(updated.team, meta.team);
        assert_eq!(updated.id, meta.id);
        assert!(authorize(&root, &meta.id, &old).is_err());
        assert!(authorize(&root, &meta.id, &new).is_ok());
        let on_disk = std::fs::read_to_string(root.join(&meta.id).join("meta.json")).unwrap();
        assert!(!on_disk.contains(&new), "plaintext token on disk");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A wrong token and a volume that does not exist are one refusal.
    #[test]
    fn a_wrong_token_and_a_missing_volume_are_the_same_refusal() {
        let root = scratch("refusal");
        let (meta, _) = new_meta("guarded".into(), None);
        create_metadata(&root, &meta).unwrap();
        let wrong = authorize(&root, &meta.id, &"w".repeat(64)).err().unwrap();
        let empty = authorize(&root, &meta.id, "").err().unwrap();
        let missing = authorize(&root, "vol-absent", &"w".repeat(64))
            .err()
            .unwrap();
        assert_eq!(wrong.0, StatusCode::NOT_FOUND);
        assert_eq!(empty.0, StatusCode::NOT_FOUND);
        assert_eq!(missing.0, StatusCode::NOT_FOUND);
        // The body names the ID asked for, so compare it with the ID removed.
        assert_eq!(wrong.1.replace(&meta.id, "ID"), "no volume ID");
        assert_eq!(empty.1.replace(&meta.id, "ID"), "no volume ID");
        assert_eq!(missing.1.replace("vol-absent", "ID"), "no volume ID");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A deleted volume waiting for its files to go is not listed.
    #[test]
    fn a_volume_being_deleted_is_not_listed() {
        let root = scratch("tombstone");
        let (meta, _) = new_meta("gone".into(), None);
        create_metadata(&root, &meta).unwrap();
        std::fs::rename(root.join(&meta.id), root.join(".deleted-x")).unwrap();
        assert!(all_in(&root).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// `GET /volumes`.
pub(crate) async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let team = match caller_team(&state, &headers) {
        Ok(team) => team,
        Err(response) => return response,
    };
    let listed: Vec<Value> = all(&state)
        .iter()
        .filter(|m| m.reachable_by(team.as_ref()))
        .map(|m| json!({ "volumeID": m.id, "name": m.name, "teamID": m.team }))
        .collect();
    Json(listed).into_response()
}

/// The volume `id`, if the caller's team may reach it. Another team's is
/// not there, rather than forbidden.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn reachable(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Meta, Response> {
    let team = caller_team(state, headers)?;
    by_id(state, id)
        .filter(|meta| meta.reachable_by(team.as_ref()))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("no volume {id}")))
}

/// `GET /volumes/{id}`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    match reachable(&state, &headers, &id) {
        Ok(meta) => Json(without_token(&meta)).into_response(),
        Err(response) => response,
    }
}

/// `DELETE /volumes/{id}`: the volume and its files. Sandboxes still
/// mounting it see an empty directory from then on.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = reachable(&state, &headers, &id) {
        return response;
    }
    // Renamed out of the volume namespace first. Once that is done the volume
    // is gone for every caller, and a failure part way through removing its
    // files leaves only a hidden directory: never a name that is taken but
    // not listed, which a new volume could not reuse.
    let tombstone = root(&state).join(format!(".deleted-{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::rename(root(&state).join(&id), &tombstone) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    quota::forget(&data_dir(&state, &id));
    match std::fs::remove_dir_all(&tombstone) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("volume {id} is deleted, but not all its files were removed: {e}"),
        ),
    }
}

/// `POST /volumes/{id}/token`: a new content token, returned once. The old
/// one stops working when the new metadata is published. Authorised exactly
/// as `GET` and `DELETE` are, through `reachable`: the content token is not
/// accepted here.
pub(crate) async fn rotate_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let meta = match reachable(&state, &headers, &id) {
        Ok(meta) => meta,
        Err(response) => return response,
    };
    match rotate_in(&root(&state), meta) {
        Ok((updated, token)) => Json(with_token(&updated, &token)).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// The mounts a create asks for, checked before anything boots: each names
/// a volume that exists, at an absolute path, no path twice.
pub(crate) fn check(
    state: &AppState,
    team: Option<&TeamId>,
    mounts: &[VolumeMount],
) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for m in mounts {
        if by_name(state, team, &m.name).is_none() {
            return Err(format!("no volume named {:?}", m.name));
        }
        let parts = ninep::components(&m.path).map_err(|_| format!("mount path {:?}", m.path))?;
        if !m.path.starts_with('/') || parts.is_empty() {
            return Err(format!("mount path {:?}: an absolute path, not /", m.path));
        }
        if !seen.insert(parts) {
            return Err(format!("{} is mounted twice", m.path));
        }
    }
    Ok(())
}

/// Mount `mounts` in the guest of `vm`: one 9P server each, on a thread of
/// its own for as long as the guest keeps the connection. Returns once
/// every mount is in the guest's mount table.
pub(crate) async fn mount(
    state: &AppState,
    team: Option<&TeamId>,
    vm: &Arc<AgentVM>,
    mounts: &[VolumeMount],
) -> Result<(), String> {
    for m in mounts {
        let meta =
            by_name(state, team, &m.name).ok_or_else(|| format!("no volume named {:?}", m.name))?;
        let dir = data_dir(state, &meta.id);
        let fs = Beneath::open(&dir).map_err(|e| format!("volume {}: {e}", m.name))?;
        // Counting what a volume already holds can take a while: do it before
        // the guest starts waiting for its mount, and off the runtime.
        // (`Server::new` finds it, already made, in the registry.)
        budget(&dir).await;
        let path = format!(
            "/{}",
            ninep::components(&m.path)
                .map_err(|e| e.to_string())?
                .join("/")
        );
        let (channel, pending) = vm
            .mount_volume_in_guest(&path, MOUNT_TIMEOUT)
            .await
            .map_err(|e| format!("mounting {} at {path}: {e}", m.name))?;
        let label = format!("hv2-9p-{}", meta.name);
        std::thread::Builder::new()
            .name(label)
            .spawn(move || ninep::serve(channel, pending, ninep::Server::new(fs)))
            .map_err(|e| e.to_string())?;

        let deadline = std::time::Instant::now() + MOUNT_TIMEOUT;
        let wanted = format!(" {path} 9p ");
        loop {
            let table = vm
                .read_file_in_guest("/proc/mounts", 1 << 20, MOUNT_TIMEOUT)
                .await
                .map_err(|e| e.to_string())?;
            if String::from_utf8_lossy(&table).contains(&wanted) {
                break;
            }
            if std::time::Instant::now() > deadline {
                return Err(format!("volume {} never appeared at {path}", m.name));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    Ok(())
}

// ── The content API ─────────────────────────────────────────────────────────

/// The bearer token a request carries, or "" when it carries none.
fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
}

/// Whether `presented` is the token of `meta`, compared as digests in
/// constant time. A missing volume is compared all the same, so the answer
/// takes the same work whether or not the volume exists.
fn token_matches(meta: Option<&Meta>, presented: &str) -> bool {
    use subtle::ConstantTimeEq;
    let digest = token_digest(presented);
    let stored = meta.map_or("", |m| m.token_sha256.as_str());
    bool::from(digest.as_bytes().ct_eq(stored.as_bytes())) && meta.is_some()
}

/// The volume `id` under `root`, if `token` is its token. A volume that does
/// not exist and a wrong token are one refusal: 404 with the same body, so a
/// caller cannot tell which volume IDs exist.
fn authorize(root: &std::path::Path, id: &str, token: &str) -> Result<Meta, (StatusCode, String)> {
    let meta = by_id_in(root, id);
    let ok = token_matches(meta.as_ref(), token);
    match meta {
        Some(meta) if ok => Ok(meta),
        _ => Err((StatusCode::NOT_FOUND, format!("no volume {id}"))),
    }
}

/// The volume `id`, if the request carries its token.
#[allow(clippy::result_large_err)]
fn authorized(
    state: &AppState,
    id: &str,
    headers: &HeaderMap,
) -> Result<(Meta, Beneath), Response> {
    let meta = authorize(&root(state), id, bearer(headers))
        .map_err(|(status, message)| api_error(status, message))?;
    let fs = Beneath::open(&data_dir(state, id))
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok((meta, fs))
}

fn io_error(e: &std::io::Error) -> Response {
    let status = match e.raw_os_error() {
        Some(libc::ENOENT) => StatusCode::NOT_FOUND,
        Some(libc::EEXIST | libc::ENOTEMPTY) => StatusCode::CONFLICT,
        Some(libc::ENOSPC | libc::EDQUOT) => StatusCode::INSUFFICIENT_STORAGE,
        Some(
            libc::ELOOP | libc::EXDEV | libc::EACCES | libc::EINVAL | libc::ENOTDIR | libc::EISDIR,
        ) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    api_error(status, e)
}

#[derive(Debug, Deserialize)]
pub(crate) struct At {
    path: String,
    uid: Option<u32>,
    gid: Option<u32>,
    mode: Option<u32>,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    atomic: bool,
    #[serde(default = "overwrite_default")]
    overwrite: bool,
    depth: Option<u32>,
}

fn overwrite_default() -> bool {
    true
}

/// What an upload has charged a volume for a file that is not yet in place:
/// given back when the upload is dropped before it gets there.
struct Held<'a> {
    budget: &'a Budget,
    cost: Cost,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.budget.release(self.cost);
    }
}

/// A replacement staged beside its destination; dropping before publication
/// removes only the exclusively-created temporary name through a pinned parent.
struct AtomicUpload {
    parent: std::fs::File,
    temporary: std::ffi::CString,
    destination: std::ffi::CString,
    published: bool,
}

impl AtomicUpload {
    fn new(fs: &Beneath, parts: &[String]) -> std::io::Result<(Self, std::fs::File)> {
        use std::os::fd::FromRawFd;
        match fs.entry(parts) {
            Ok(entry) if entry.stat.st_mode & libc::S_IFMT != libc::S_IFREG => {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            Err(error) if error.raw_os_error() != Some(libc::ENOENT) => return Err(error),
            _ => {}
        }
        let (name, parents) = parts
            .split_last()
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?;
        let parent =
            std::fs::File::from(fs.open_at(parents, libc::O_RDONLY | libc::O_DIRECTORY, 0)?);
        let temporary =
            std::ffi::CString::new(format!(".hm-upload-{}", uuid::Uuid::new_v4())).unwrap();
        let destination = std::ffi::CString::new(name.as_str())
            .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok((
            Self {
                parent,
                temporary,
                destination,
                published: false,
            },
            unsafe { std::fs::File::from_raw_fd(fd) },
        ))
    }

    fn publish(&mut self, overwrite: bool) -> std::io::Result<()> {
        if unsafe {
            libc::renameat2(
                self.parent.as_raw_fd(),
                self.temporary.as_ptr(),
                self.parent.as_raw_fd(),
                self.destination.as_ptr(),
                if overwrite { 0 } else { libc::RENAME_NOREPLACE },
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        self.published = true;
        Ok(())
    }
}

impl Drop for AtomicUpload {
    fn drop(&mut self) {
        if !self.published {
            unsafe {
                libc::unlinkat(self.parent.as_raw_fd(), self.temporary.as_ptr(), 0);
            }
        }
    }
}

/// E2B's `VolumeEntryStat` for what `parts` names.
fn stat_json(fs: &Beneath, parts: &[String]) -> std::io::Result<Value> {
    let entry = fs.entry(parts)?;
    let st = entry.stat;
    let kind = if ninep::is_dir(&st) {
        "directory"
    } else if ninep::is_symlink(&st) {
        "symlink"
    } else if st.st_mode & libc::S_IFMT == libc::S_IFREG {
        "file"
    } else {
        "unknown"
    };
    let (uid, gid) = ninep::owner(entry.fd.as_raw_fd(), &st, (0, 0));
    let time = |sec: i64| hv2_cluster::model::rfc3339(u64::try_from(sec).unwrap_or(0) * 1000);
    let mut v = json!({
        "name": parts.last().cloned().unwrap_or_default(),
        "type": kind,
        "path": format!("/{}", parts.join("/")),
        "size": st.st_size,
        "mode": st.st_mode & 0o7777,
        "uid": uid,
        "gid": gid,
        "atime": time(st.st_atime),
        "mtime": time(st.st_mtime),
        "ctime": time(st.st_ctime),
    });
    if kind == "symlink" {
        if let Some((name, dir)) = parts.split_last() {
            let d = fs.dir(dir)?;
            let n = std::ffi::CString::new(name.as_str())
                .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
            let mut buf = vec![0u8; 4096];
            let len = unsafe {
                libc::readlinkat(
                    d.as_raw_fd(),
                    n.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if len >= 0 {
                buf.truncate(len as usize);
                v["target"] = json!(String::from_utf8_lossy(&buf));
            }
        }
    }
    Ok(v)
}

/// Each missing directory on the way to `parts` (excluding the last).
fn make_parents(fs: &Beneath, budget: &Budget, parts: &[String]) -> std::io::Result<()> {
    for depth in 1..parts.len() {
        match fs.entry(&parts[..depth]) {
            Ok(e) if ninep::is_dir(&e.stat) => {}
            Ok(_) => return Err(std::io::Error::from_raw_os_error(libc::ENOTDIR)),
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {
                mkdir(fs, budget, &parts[..depth], 0o755)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn mkdir(fs: &Beneath, budget: &Budget, parts: &[String], mode: u32) -> std::io::Result<()> {
    let (name, dir) = parts
        .split_last()
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EEXIST))?;
    let d = fs.dir(dir)?;
    let n = std::ffi::CString::new(name.as_str())
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let charge = Cost {
        inodes: 1,
        ..Cost::default()
    };
    budget.admit(charge)?;
    if unsafe { libc::mkdirat(d.as_raw_fd(), n.as_ptr(), ninep::safe_mode(mode)) } < 0 {
        budget.release(charge);
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Owner and mode, as a request sets them.
fn apply(
    fs: &Beneath,
    parts: &[String],
    uid: Option<u32>,
    gid: Option<u32>,
    mode: Option<u32>,
) -> std::io::Result<()> {
    let entry = fs.entry(parts)?;
    if ninep::is_symlink(&entry.stat) {
        return Ok(());
    }
    let fd = entry.fd.as_raw_fd();
    if let Some(mode) = mode {
        ninep::chmod(fd, mode)?;
    }
    if uid.is_some() || gid.is_some() {
        let (u, g) = ninep::owner(fd, &entry.stat, (0, 0));
        ninep::set_owner(fd, uid.unwrap_or(u), gid.unwrap_or(g))?;
    }
    Ok(())
}

/// Everything at `parts`, a directory's contents first.
fn remove(fs: &Beneath, budget: &Budget, parts: &[String]) -> std::io::Result<()> {
    let (name, dir) = parts
        .split_last()
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBUSY))?;
    let entry = fs.entry(parts)?;
    if ninep::is_dir(&entry.stat) {
        for child in list_names(fs, parts)? {
            let mut p = parts.to_vec();
            p.push(child);
            remove(fs, budget, &p)?;
        }
    }
    let d = fs.dir(dir)?;
    let n = std::ffi::CString::new(name.as_str())
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let flags = if ninep::is_dir(&entry.stat) {
        libc::AT_REMOVEDIR
    } else {
        0
    };
    if unsafe { libc::unlinkat(d.as_raw_fd(), n.as_ptr(), flags) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    budget.removed(&entry.stat);
    Ok(())
}

fn list_names(fs: &Beneath, parts: &[String]) -> std::io::Result<Vec<String>> {
    let d = fs.dir(parts)?;
    let path = ninep::proc_path(d.as_raw_fd());
    let mut names: Vec<String> = std::fs::read_dir(path.to_str().unwrap_or_default())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

#[allow(clippy::result_large_err)]
fn parts_of(path: &str) -> Result<Vec<String>, Response> {
    ninep::components(path).map_err(|e| io_error(&e))
}

/// `GET /volumecontent/{id}/file`.
pub(crate) async fn read_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
) -> Response {
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let fd = match fs.open_at(&parts, libc::O_RDONLY, 0) {
        Ok(fd) => fd,
        Err(e) => return io_error(&e),
    };
    let file = tokio::fs::File::from_std(std::fs::File::from(fd));
    let stream = file_stream(file);
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// A file as a stream of chunks, so a large one is not held in memory.
pub(crate) fn file_stream(
    mut file: tokio::fs::File,
) -> impl tokio_stream::Stream<Item = std::io::Result<Vec<u8>>> {
    async_stream(move |tx| async move {
        use tokio::io::AsyncReadExt;
        loop {
            let mut buf = vec![0u8; 256 * 1024];
            match file.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    buf.truncate(n);
                    if tx.send(Ok(buf)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
    })
}

fn async_stream<F, Fut>(f: F) -> tokio_stream::wrappers::ReceiverStream<std::io::Result<Vec<u8>>>
where
    F: FnOnce(tokio::sync::mpsc::Sender<std::io::Result<Vec<u8>>>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::spawn(f(tx));
    tokio_stream::wrappers::ReceiverStream::new(rx)
}

/// `PUT /volumecontent/{id}/file`: create or replace a file, streamed.
pub(crate) async fn write_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    use tokio::io::AsyncWriteExt;
    use tokio_stream::StreamExt;
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => return api_error(StatusCode::BAD_REQUEST, "a file path"),
        Err(r) => return r,
    };
    let budget = budget(&data_dir(&state, &id)).await;
    if at.force {
        if let Err(e) = make_parents(&fs, &budget, &parts) {
            return io_error(&e);
        }
    }
    // What a regular file already at the path holds: given back when it is
    // emptied or replaced; none is a new entry, charged before it is made.
    let existing = fs
        .entry(&parts)
        .ok()
        .map(|e| e.stat)
        .filter(|st| st.st_mode & libc::S_IFMT == libc::S_IFREG);
    let new_entry = Cost {
        inodes: 1,
        ..Cost::default()
    };
    // What this upload has charged for a file that is not yet in place; given
    // back if it never gets there.
    let mut staged = Held {
        budget: &budget,
        cost: Cost::default(),
    };
    let (mut atomic, file) = if at.atomic {
        if let Err(e) = budget.admit(new_entry) {
            return io_error(&e);
        }
        staged.cost.inodes = 1;
        match AtomicUpload::new(&fs, &parts) {
            Ok((stage, file)) => (Some(stage), file),
            Err(e) => return io_error(&e),
        }
    } else {
        if existing.is_none() {
            if let Err(e) = budget.admit(new_entry) {
                return io_error(&e);
            }
        }
        let flags = libc::O_WRONLY
            | libc::O_CREAT
            | if at.overwrite {
                libc::O_TRUNC
            } else {
                libc::O_EXCL
            };
        match fs.open_at(&parts, flags, at.mode.unwrap_or(0o644)) {
            Ok(fd) => {
                if let (Some(old), true) = (&existing, at.overwrite) {
                    // Opened with O_TRUNC: its bytes are free again.
                    if let Ok(now) = quota::size_of(fd.as_raw_fd()) {
                        budget.release(Cost {
                            size: u64::try_from(old.st_size).unwrap_or(0).saturating_sub(now),
                            ..Cost::default()
                        });
                    }
                }
                (None, std::fs::File::from(fd))
            }
            Err(e) => {
                if existing.is_none() {
                    budget.release(new_entry);
                }
                return io_error(&e);
            }
        }
    };
    let mut file = tokio::fs::File::from_std(file);
    let mut stream = body.into_data_stream();
    let mut total = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        };
        total += chunk.len() as u64;
        if total > MAX_FILE_BYTES {
            return api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "larger than a volume file may be",
            );
        }
        // Charged before it is written, so a volume at its limit stops the
        // upload at the chunk that would pass it.
        let growth = Cost {
            size: chunk.len() as u64,
            disk: chunk.len() as u64,
            inodes: 0,
        };
        if let Err(e) = budget.admit(growth) {
            return io_error(&e);
        }
        if atomic.is_some() {
            staged.cost.size += growth.size;
        }
        if let Err(e) = file.write_all(&chunk).await {
            if atomic.is_none() {
                budget.release(growth);
            }
            return io_error(&e);
        }
    }
    if let Err(e) = file.flush().await {
        return io_error(&e);
    }
    if atomic.is_some() {
        let metadata = (|| {
            let fd = file.as_raw_fd();
            ninep::chmod(fd, at.mode.unwrap_or(0o644))?;
            if at.uid.is_some() || at.gid.is_some() {
                let stat = ninep::fstat(fd)?;
                let (uid, gid) = ninep::owner(fd, &stat, (0, 0));
                ninep::set_owner(fd, at.uid.unwrap_or(uid), at.gid.unwrap_or(gid))?;
            }
            Ok::<_, std::io::Error>(())
        })();
        if let Err(e) = metadata {
            return io_error(&e);
        }
    } else if let Err(e) = apply(&fs, &parts, at.uid, at.gid, at.mode) {
        return io_error(&e);
    }
    if let Err(e) = file.sync_all().await {
        return io_error(&e);
    }
    if let Some(stage) = atomic.as_mut() {
        if let Err(e) = stage.publish(at.overwrite) {
            return io_error(&e);
        }
        // In place: what it holds is the volume's now, and what it replaced
        // is gone.
        staged.cost = Cost::default();
        if let Some(old) = &existing {
            budget.removed(old);
        }
        let parent = match stage.parent.try_clone() {
            Ok(file) => file,
            Err(e) => return io_error(&e),
        };
        if let Err(e) = tokio::fs::File::from_std(parent).sync_all().await {
            return io_error(&e);
        }
    }
    // Sync each containing directory, including parents made by force=true.
    // O_PATH descriptors cannot be fsynced; open real directory descriptors
    // through the same beneath-root, no-symlink resolver.
    let unsynced_directories = if atomic.is_some() {
        // Publication already flushed the pinned immediate parent. Continue
        // with its ancestors without reopening and flushing it a second time.
        parts.len() - 1
    } else {
        parts.len()
    };
    if let Err(e) = sync_directories(&fs, &parts, unsynced_directories).await {
        return io_error(&e);
    }
    match stat_json(&fs, &parts) {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => io_error(&e),
    }
}

/// Flush selected containing directories deepest-first, ending at the data root.
async fn sync_directories(fs: &Beneath, parts: &[String], count: usize) -> std::io::Result<()> {
    for depth in (0..count).rev() {
        let fd = fs.open_at(&parts[..depth], libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        tokio::fs::File::from_std(std::fs::File::from(fd))
            .sync_all()
            .await?;
    }
    Ok(())
}

/// `GET /volumecontent/{id}/dir`: a directory's entries, to `depth`.
pub(crate) async fn list_dir(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
) -> Response {
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let depth = at.depth.unwrap_or(1).clamp(1, 32);
    let mut out = Vec::new();
    let mut frontier = vec![(parts, 1u32)];
    while let Some((dir, level)) = frontier.pop() {
        let names = match list_names(&fs, &dir) {
            Ok(n) => n,
            Err(e) => return io_error(&e),
        };
        for name in names {
            let mut p = dir.clone();
            p.push(name);
            let Ok(v) = stat_json(&fs, &p) else { continue };
            if v["type"] == "directory" && level < depth {
                frontier.push((p, level + 1));
            }
            out.push(v);
        }
    }
    Json(out).into_response()
}

/// `POST /volumecontent/{id}/dir`: make a directory; with `force`, its
/// parents too, and one already there is fine.
pub(crate) async fn make_dir(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
) -> Response {
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => return api_error(StatusCode::BAD_REQUEST, "a directory path"),
        Err(r) => return r,
    };
    let budget = budget(&data_dir(&state, &id)).await;
    if at.force {
        if let Err(e) = make_parents(&fs, &budget, &parts) {
            return io_error(&e);
        }
    }
    match mkdir(&fs, &budget, &parts, at.mode.unwrap_or(0o755)) {
        Ok(()) => {}
        Err(e) if at.force && e.raw_os_error() == Some(libc::EEXIST) => {
            // Force accepts an existing directory, never a regular file or link.
            if let Err(e) = fs.dir(&parts) {
                return io_error(&e);
            }
        }
        Err(e) => return io_error(&e),
    }
    if let Err(e) = apply(&fs, &parts, at.uid, at.gid, at.mode) {
        return io_error(&e);
    }
    // Flush the created/updated directory and its containing directories.
    // Include parents created by force=true, all the way to the data root.
    if let Err(e) = sync_directories(&fs, &parts, parts.len() + 1).await {
        return io_error(&e);
    }
    match stat_json(&fs, &parts) {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => io_error(&e),
    }
}

/// `GET /volumecontent/{id}/path`: what is there.
pub(crate) async fn stat(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
) -> Response {
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match stat_json(&fs, &parts) {
        Ok(v) => Json(v).into_response(),
        Err(e) => io_error(&e),
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Metadata {
    uid: Option<u32>,
    gid: Option<u32>,
    mode: Option<u32>,
}

/// `PATCH /volumecontent/{id}/path`: owner and mode.
pub(crate) async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
    body: Option<Json<Metadata>>,
) -> Response {
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let m = body.map(|Json(m)| m).unwrap_or_default();
    if let Err(e) = apply(&fs, &parts, m.uid, m.gid, m.mode) {
        return io_error(&e);
    }
    match stat_json(&fs, &parts) {
        Ok(v) => Json(v).into_response(),
        Err(e) => io_error(&e),
    }
}

/// `DELETE /volumecontent/{id}/path`: a file, or a directory and all in it.
pub(crate) async fn remove_path(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(at): Query<At>,
    headers: HeaderMap,
) -> Response {
    let (_, fs) = match authorized(&state, &id, &headers) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let parts = match parts_of(&at.path) {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => return api_error(StatusCode::BAD_REQUEST, "not the volume's root"),
        Err(r) => return r,
    };
    let budget = budget(&data_dir(&state, &id)).await;
    match remove(&fs, &budget, &parts) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => io_error(&e),
    }
}
