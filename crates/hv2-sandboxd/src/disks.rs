//! Disks: persistent block devices, attached to one sandbox at a time.
//!
//! A volume ([`crate::volumes`]) is a directory several sandboxes share live.
//! A disk is the other kind of storage: an ext4 image the guest sees as
//! `/dev/vda` and mounts itself, at block-device speed, which one sandbox
//! holds at a time and which outlives it. End the sandbox and the disk is
//! free; create another with the same `diskMount` and it has the same files.
//!
//! - `POST /disks` (`{"name", "sizeMiB"}`), `GET /disks`, `GET` and
//!   `DELETE /disks/{diskID}`, behind the API key.
//! - `"diskMount": {"name", "path"}` on `POST /sandboxes` attaches one.
//!
//! # What holds a disk
//!
//! Its `meta.json` names the sandbox it is attached to, so `GET /disks` can
//! say. That alone would make a crash leave a disk claimed forever, so the
//! claim counts only while this process holds it too: a disk whose
//! `attachedTo` is not in [`HELD`] was left by a daemon that is gone, and
//! with it every VM it ran, so it is free. Two daemons sharing one disk
//! directory would each think the other's claims stale; a disk directory is
//! one node's, which is why it is not the shared snapshot store.
//!
//! # What a sandbox with a disk cannot do yet
//!
//! - Restore from a template. virtio-mmio has no hot-plug, and a template's
//!   guest booted without a disk has no driver bound to one, so a sandbox
//!   with a disk cold-boots.
//! - Pause, fork or checkpoint. Its memory would be restored against a disk
//!   that may have moved on since, which is how a filesystem is corrupted.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use hv2_agent::AgentVM;

use super::{
    api_error, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, Serialize, State,
    StatusCode,
};

/// Smallest disk: ext4 needs room for its journal.
const MIN_MIB: u64 = 16;
/// Largest disk, unless an operator raises it. The image is sparse, so this
/// bounds what a guest can fill, not what creation costs.
const MAX_MIB: u64 = 1024 * 1024;
/// How long a guest has to mount its disk.
const MOUNT_TIMEOUT: Duration = Duration::from_secs(30);

/// Serialises every read-modify-write of a disk's metadata, so two sandboxes
/// asking for one disk at once cannot both be told yes.
static LOCK: Mutex<()> = Mutex::new(());

/// Sandboxes that hold a disk claim made by this process.
static HELD: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn held(sandbox_id: &str) -> bool {
    HELD.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|h| h.contains(sandbox_id))
}

fn set_held(sandbox_id: &str, on: bool) {
    let mut guard = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let set = guard.get_or_insert_with(HashSet::new);
    if on {
        set.insert(sandbox_id.to_string());
    } else {
        set.remove(sandbox_id);
    }
}

/// What is kept about a disk beside its image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Meta {
    #[serde(rename = "diskID")]
    id: String,
    name: String,
    #[serde(rename = "sizeMiB")]
    size_mib: u64,
    /// The sandbox holding it, as last recorded. See the module docs for
    /// when that is believed.
    #[serde(rename = "attachedTo", default)]
    attached_to: Option<String>,
    /// Where that sandbox mounts it, so a reboot can mount it there again.
    #[serde(rename = "mountPath", default, skip_serializing_if = "Option::is_none")]
    mount_path: Option<String>,
}

impl Meta {
    /// The sandbox that really holds it, if any.
    fn holder(&self) -> Option<&str> {
        self.attached_to.as_deref().filter(|id| held(id))
    }

    fn describe(&self) -> Value {
        json!({
            "diskID": self.id,
            "name": self.name,
            "sizeMiB": self.size_mib,
            "attachedTo": self.holder(),
        })
    }
}

fn root(state: &AppState) -> PathBuf {
    match &state.opts.disk_dir {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir().join("hv2-sandboxd-disks"),
    }
}

fn image(state: &AppState, id: &str) -> PathBuf {
    root(state).join(id).join("disk.img")
}

fn read_meta(dir: &std::path::Path) -> Option<Meta> {
    serde_json::from_slice(&std::fs::read(dir.join("meta.json")).ok()?).ok()
}

/// Publish `meta` by rename, so a reader sees the old record or the new one
/// and never half of either.
fn write_meta(state: &AppState, meta: &Meta) -> std::io::Result<()> {
    let dir = root(state).join(&meta.id);
    let temporary = dir.join(".meta.json");
    let bytes = serde_json::to_vec(meta).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, bytes)?;
    std::fs::File::open(&temporary)?.sync_all()?;
    std::fs::rename(&temporary, dir.join("meta.json"))
}

fn all(state: &AppState) -> Vec<Meta> {
    let Ok(listing) = std::fs::read_dir(root(state)) else {
        return Vec::new();
    };
    let mut disks: Vec<Meta> = listing
        .flatten()
        .filter_map(|e| read_meta(&e.path()))
        .collect();
    disks.sort_by(|a, b| a.name.cmp(&b.name));
    disks
}

fn by_name(state: &AppState, name: &str) -> Option<Meta> {
    all(state).into_iter().find(|d| d.name == name)
}

fn by_id(state: &AppState, id: &str) -> Option<Meta> {
    if !valid_name(id) {
        return None;
    }
    read_meta(&root(state).join(id))
}

/// The same rule as a volume's name, which an ID also meets.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A disk's ID: derived from its name, as a volume's is.
fn disk_id(name: &str) -> String {
    let volume = hv2_cluster::model::volume_id(name);
    format!("disk-{}", volume.trim_start_matches("vol-"))
}

/// A path to mount at: absolute, and nothing a shell would read as syntax.
fn valid_mount_path(path: &str) -> bool {
    path.starts_with('/')
        && path != "/"
        && path.len() <= 255
        && !path.split('/').any(|part| part == "..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewDisk {
    name: String,
    #[serde(rename = "sizeMiB")]
    size_mib: u64,
}

/// `POST /disks`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NewDisk>,
) -> Response {
    if !valid_name(&req.name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("disk name {:?}: letters, digits, _ and - only", req.name),
        );
    }
    if !(MIN_MIB..=MAX_MIB).contains(&req.size_mib) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("sizeMiB must be between {MIN_MIB} and {MAX_MIB}"),
        );
    }
    let meta = Meta {
        id: disk_id(&req.name),
        name: req.name,
        size_mib: req.size_mib,
        attached_to: None,
        mount_path: None,
    };
    let state2 = Arc::clone(&state);
    let made = tokio::task::spawn_blocking(move || make(&state2, &meta).map(|()| meta))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        .and_then(|r| r);
    match made {
        Ok(meta) => (StatusCode::CREATED, Json(meta.describe())).into_response(),
        Err((status, message)) => api_error(status, message),
    }
}

/// Reserve the directory, make and format the image, then publish its
/// metadata. Until the metadata exists the disk does not, and a failure
/// removes only what this call made.
fn make(state: &AppState, meta: &Meta) -> Result<(), (StatusCode, String)> {
    let internal = |e: String| (StatusCode::INTERNAL_SERVER_ERROR, e);
    let _lock = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if by_name(state, &meta.name).is_some() {
        return Err((StatusCode::CONFLICT, format!("disk {} exists", meta.name)));
    }
    let root = root(state);
    std::fs::create_dir_all(&root).map_err(|e| internal(format!("{}: {e}", root.display())))?;
    let dir = root.join(&meta.id);
    if let Err(e) = std::fs::create_dir(&dir) {
        return Err(if e.kind() == std::io::ErrorKind::AlreadyExists {
            (StatusCode::CONFLICT, format!("disk {} exists", meta.name))
        } else {
            internal(format!("{}: {e}", dir.display()))
        });
    }
    let result = (|| {
        let path = dir.join("disk.img");
        std::fs::File::create_new(&path)
            .and_then(|f| f.set_len(meta.size_mib * 1024 * 1024))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        format(&path, &meta.name)?;
        write_meta(state, meta).map_err(|e| format!("recording disk {}: {e}", meta.name))
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(internal(e));
    }
    Ok(())
}

/// Put an empty ext4 filesystem on the image.
///
/// On the host, so a guest needs nothing but a kernel that reads ext4 to use
/// one: not every template's image carries a `mkfs`.
fn format(path: &std::path::Path, label: &str) -> Result<(), String> {
    let label: String = label.chars().take(16).collect();
    let out = std::process::Command::new("mkfs.ext4")
        .args(["-F", "-q", "-L", &label])
        .arg(path)
        .output()
        .map_err(|e| format!("running mkfs.ext4 (is e2fsprogs installed?): {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "mkfs.ext4 failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// `GET /disks`.
pub(crate) async fn list(State(state): State<Arc<AppState>>) -> Response {
    let disks: Vec<Value> = all(&state).iter().map(Meta::describe).collect();
    Json(disks).into_response()
}

/// `GET /disks/{diskID}`.
pub(crate) async fn get(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match by_id(&state, &id) {
        Some(meta) => Json(meta.describe()).into_response(),
        None => api_error(StatusCode::NOT_FOUND, format!("no disk {id}")),
    }
}

/// `DELETE /disks/{diskID}`: refused while a sandbox holds it. Unlike a
/// volume's, whose sandboxes are left looking at an empty directory, a
/// disk's sandbox would be left writing to a file that no longer exists.
pub(crate) async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let _lock = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(meta) = by_id(&state, &id) else {
        return api_error(StatusCode::NOT_FOUND, format!("no disk {id}"));
    };
    if let Some(holder) = meta.holder() {
        return api_error(
            StatusCode::CONFLICT,
            format!("disk {} is attached to sandbox {holder}", meta.name),
        );
    }
    match std::fs::remove_dir_all(root(&state).join(&meta.id)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// A disk a sandbox asked for, at a path in its guest. The request's
/// `diskMount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskMount {
    pub name: String,
    pub path: String,
}

/// A disk claimed for one sandbox. Dropped without [`Claim::keep`], it is
/// given back: a create that fails after claiming must not leave the disk
/// held by a sandbox that never existed.
pub(crate) struct Claim {
    state: Arc<AppState>,
    sandbox_id: String,
    pub image: PathBuf,
    pub serial: String,
    pub path: String,
    kept: bool,
}

impl Claim {
    /// The sandbox exists now; its end gives the disk back, through
    /// [`release`].
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if !self.kept {
            release(&self.state, &self.sandbox_id);
        }
    }
}

/// Claim `mount`'s disk for `sandbox_id`.
///
/// # Errors
///
/// 400 for a bad path, 404 for no such disk, 409 if another sandbox holds it.
pub(crate) fn claim(
    state: &Arc<AppState>,
    sandbox_id: &str,
    mount: &DiskMount,
) -> Result<Claim, (StatusCode, String)> {
    if !valid_mount_path(&mount.path) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "diskMount path {:?}: absolute, of letters, digits and / _ - . only",
                mount.path
            ),
        ));
    }
    let _lock = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(mut meta) = by_name(state, &mount.name) else {
        return Err((StatusCode::NOT_FOUND, format!("no disk {}", mount.name)));
    };
    if let Some(holder) = meta.holder() {
        return Err((
            StatusCode::CONFLICT,
            format!("disk {} is attached to sandbox {holder}", meta.name),
        ));
    }
    meta.attached_to = Some(sandbox_id.to_string());
    meta.mount_path = Some(mount.path.clone());
    write_meta(state, &meta).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("claiming disk {}: {e}", meta.name),
        )
    })?;
    set_held(sandbox_id, true);
    Ok(Claim {
        state: Arc::clone(state),
        sandbox_id: sandbox_id.to_string(),
        image: image(state, &meta.id),
        serial: meta.id,
        path: mount.path.clone(),
        kept: false,
    })
}

/// Give back every disk `sandbox_id` holds. Called when a sandbox ends; a
/// sandbox that held none costs a directory listing.
pub(crate) fn release(state: &AppState, sandbox_id: &str) {
    if !held(sandbox_id) {
        return;
    }
    let _lock = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for mut meta in all(state) {
        if meta.attached_to.as_deref() == Some(sandbox_id) {
            meta.attached_to = None;
            meta.mount_path = None;
            if let Err(e) = write_meta(state, &meta) {
                // Still free: the claim is believed only while held.
                tracing::warn!("releasing disk {}: {e}", meta.name);
            }
        }
    }
    set_held(sandbox_id, false);
}

/// The disk `sandbox_id` already holds, to attach to its next guest: what a
/// reboot boots with. Dropping it gives nothing back; the sandbox's end does.
pub(crate) fn reattach(state: &Arc<AppState>, sandbox_id: &str) -> Option<Claim> {
    if !held(sandbox_id) {
        return None;
    }
    let _lock = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let meta = all(state)
        .into_iter()
        .find(|m| m.attached_to.as_deref() == Some(sandbox_id))?;
    Some(Claim {
        state: Arc::clone(state),
        sandbox_id: sandbox_id.to_string(),
        image: image(state, &meta.id),
        path: meta.mount_path.clone()?,
        serial: meta.id,
        kept: true,
    })
}

/// Whether `sandbox_id` holds a disk, for the operations it rules out.
pub(crate) fn holds_one(sandbox_id: &str) -> bool {
    held(sandbox_id)
}

/// Mount the attached disk in the guest at `path`.
pub(crate) async fn mount(vm: &Arc<AgentVM>, path: &str) -> Result<(), String> {
    let script =
        format!("/bin/busybox mkdir -p {path} && /bin/busybox mount -t ext4 /dev/vda {path}");
    let out = vm
        .exec_in_guest(
            "/bin/busybox",
            &["sh".to_string(), "-c".to_string(), script],
            MOUNT_TIMEOUT,
        )
        .await
        .map_err(|e| format!("mounting the disk: {e}"))?;
    if out.exit_code == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "mounting the disk at {path} failed ({:?}): {}",
            out.exit_code,
            out.stderr.trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_id_is_derived_from_its_name() {
        assert_eq!(disk_id("data"), disk_id("data"));
        assert_ne!(disk_id("data"), disk_id("other"));
        assert!(disk_id("data").starts_with("disk-"));
        assert!(valid_name(&disk_id("data")));
    }

    #[test]
    fn mount_paths_cannot_smuggle_shell_or_escape() {
        assert!(valid_mount_path("/data"));
        assert!(valid_mount_path("/mnt/disk-1"));
        assert!(!valid_mount_path("data"));
        assert!(!valid_mount_path("/"));
        assert!(!valid_mount_path("/data; rm -rf /"));
        assert!(!valid_mount_path("/a/../etc"));
        assert!(!valid_mount_path("/$(id)"));
    }

    #[test]
    fn a_claim_counts_only_while_this_process_holds_it() {
        let meta = Meta {
            id: disk_id("x"),
            name: "x".into(),
            size_mib: 16,
            attached_to: Some("sbx-from-a-dead-daemon".into()),
            mount_path: Some("/data".into()),
        };
        assert_eq!(meta.holder(), None, "a claim nobody holds is stale");
        set_held("sbx-from-a-dead-daemon", true);
        assert_eq!(meta.holder(), Some("sbx-from-a-dead-daemon"));
        set_held("sbx-from-a-dead-daemon", false);
        assert_eq!(meta.holder(), None);
    }
}
