//! Volumes: E2B's persistent storage. A volume is a directory on the node
//! -- in the snapshot store when there is one, so every node has the same
//! volumes -- that sandboxes mount (`volumeMounts` on create), live and
//! shared, and that outlives them.
//!
//! - `POST /volumes`, `GET /volumes`, `GET` and `DELETE /volumes/{id}`:
//!   E2B's volume API, behind the API key.
//! - `/volumecontent/{id}/{file,dir,path}`: E2B's volume content API, which
//!   the SDK's `Volume` calls with the volume's own bearer token.
//! - A mount is a 9P2000.L server ([`crate::ninep`]) on a vsock connection
//!   the guest's kernel mounts; see there for how a guest is kept inside it.
//!
//! The content API resolves paths the way the 9P server does -- beneath
//! the volume, no symlinks followed -- so it cannot be steered out either.

use std::os::fd::AsRawFd;
use std::time::Duration;

use axum::body::Body;
use axum::http::HeaderMap;
use serde_json::{Value, json};

use hv2_agent::AgentVM;
use hv2_cluster::model::VolumeMount;

use super::{
    AppState, Arc, Deserialize, IntoResponse, Json, Path, Query, Response, Serialize, State,
    StatusCode, api_error, new_access_token,
};
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
    token: String,
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

fn all(state: &AppState) -> Vec<Meta> {
    let Ok(listing) = std::fs::read_dir(root(state)) else {
        return Vec::new();
    };
    let mut volumes: Vec<Meta> = listing
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join("meta.json")).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    volumes.sort_by(|a, b| a.name.cmp(&b.name));
    volumes
}

fn by_id(state: &AppState, id: &str) -> Option<Meta> {
    if !valid_name(id) {
        return None;
    }
    let bytes = std::fs::read(root(state).join(id).join("meta.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn by_name(state: &AppState, name: &str) -> Option<Meta> {
    all(state).into_iter().find(|v| v.name == name)
}

/// E2B's pattern for a volume name, which an ID also meets.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn with_token(meta: &Meta) -> Value {
    json!({ "volumeID": meta.id, "name": meta.name, "token": meta.token })
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
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
            .mode(0o600).open(&temporary)?;
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

#[derive(Debug, Deserialize)]
pub(crate) struct NewVolume {
    name: String,
}

/// `POST /volumes`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NewVolume>,
) -> Response {
    if !valid_name(&req.name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("volume name {:?}: letters, digits, _ and - only", req.name),
        );
    }
    if by_name(&state, &req.name).is_some() {
        return api_error(StatusCode::CONFLICT, format!("volume {} exists", req.name));
    }
    let meta = Meta {
        id: hv2_cluster::model::volume_id(&req.name),
        name: req.name,
        token: new_access_token(),
    };
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
    (StatusCode::CREATED, Json(with_token(&meta))).into_response()
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[test]
    fn sync_failures_preserve_published_metadata_and_data() {
        for failed_stage in 1..=4 {
            let root = std::env::temp_dir().join(format!("hm-volume-sync-{}", uuid::Uuid::new_v4()));
            let meta = Meta { id: "vol-owned".into(), name: "owned".into(), token: "owned-token".into() };
            let mut stage = 0;
            let result = create_metadata_with_sync(&root, &meta, |_| {
                stage += 1;
                if stage == failed_stage { Err(std::io::Error::other("injected sync failure")) } else { Ok(()) }
            });
            assert!(result.is_err());
            let directory = root.join(&meta.id);
            if failed_stage < 3 {
                assert!(!directory.exists());
            } else {
                assert_eq!(std::fs::read(directory.join("meta.json")).unwrap(), serde_json::to_vec(&meta).unwrap());
                assert!(directory.join("data").is_dir());
                assert_eq!(create_metadata(&root, &meta).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
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
                        token: index.to_string(),
                    };
                    barrier.wait();
                    (meta.token.clone(), create_metadata(&root, &meta))
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
        assert_eq!(std::fs::metadata(&metadata).unwrap().permissions().mode() & 0o777, 0o600);
        let before = std::fs::read(&metadata).unwrap();
        let meta: Meta = serde_json::from_slice(&before).unwrap();
        assert_eq!(Some(meta.token), winner);
        let marker = root.join("vol-owned/data/marker");
        std::fs::write(&marker, b"owned bytes").unwrap();
        let duplicate = Meta {
            id: "vol-owned".into(),
            name: "owned".into(),
            token: "replacement".into(),
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
            token: "replacement".into(),
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

/// `GET /volumes`.
pub(crate) async fn list(State(state): State<Arc<AppState>>) -> Response {
    let listed: Vec<Value> = all(&state)
        .iter()
        .map(|m| json!({ "volumeID": m.id, "name": m.name }))
        .collect();
    Json(listed).into_response()
}

/// `GET /volumes/{id}`.
pub(crate) async fn get(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match by_id(&state, &id) {
        Some(meta) => Json(with_token(&meta)).into_response(),
        None => api_error(StatusCode::NOT_FOUND, format!("no volume {id}")),
    }
}

/// `DELETE /volumes/{id}`: the volume and its files. Sandboxes still
/// mounting it see an empty directory from then on.
pub(crate) async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    if by_id(&state, &id).is_none() {
        return api_error(StatusCode::NOT_FOUND, format!("no volume {id}"));
    }
    match std::fs::remove_dir_all(root(&state).join(&id)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// The mounts a create asks for, checked before anything boots: each names
/// a volume that exists, at an absolute path, no path twice.
pub(crate) fn check(state: &AppState, mounts: &[VolumeMount]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for m in mounts {
        if by_name(state, &m.name).is_none() {
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
    vm: &Arc<AgentVM>,
    mounts: &[VolumeMount],
) -> Result<(), String> {
    for m in mounts {
        let meta =
            by_name(state, &m.name).ok_or_else(|| format!("no volume named {:?}", m.name))?;
        let fs = Beneath::open(&data_dir(state, &meta.id))
            .map_err(|e| format!("volume {}: {e}", m.name))?;
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

/// The volume `id`, if the request carries its token.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn authorized(
    state: &AppState,
    id: &str,
    headers: &HeaderMap,
) -> Result<(Meta, Beneath), Response> {
    use subtle::ConstantTimeEq;
    let meta = by_id(state, id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("no volume {id}")))?;
    let sent = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if !bool::from(sent.as_bytes().ct_eq(meta.token.as_bytes())) {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "not this volume's token",
        ));
    }
    let fs = Beneath::open(&data_dir(state, id))
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok((meta, fs))
}

fn io_error(e: &std::io::Error) -> Response {
    let status = match e.raw_os_error() {
        Some(libc::ENOENT) => StatusCode::NOT_FOUND,
        Some(libc::EEXIST | libc::ENOTEMPTY) => StatusCode::CONFLICT,
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
    depth: Option<u32>,
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
fn make_parents(fs: &Beneath, parts: &[String]) -> std::io::Result<()> {
    for depth in 1..parts.len() {
        match fs.entry(&parts[..depth]) {
            Ok(e) if ninep::is_dir(&e.stat) => {}
            Ok(_) => return Err(std::io::Error::from_raw_os_error(libc::ENOTDIR)),
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => mkdir(fs, &parts[..depth], 0o755)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn mkdir(fs: &Beneath, parts: &[String], mode: u32) -> std::io::Result<()> {
    let (name, dir) = parts
        .split_last()
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EEXIST))?;
    let d = fs.dir(dir)?;
    let n = std::ffi::CString::new(name.as_str())
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    if unsafe { libc::mkdirat(d.as_raw_fd(), n.as_ptr(), mode) } < 0 {
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
fn remove(fs: &Beneath, parts: &[String]) -> std::io::Result<()> {
    let (name, dir) = parts
        .split_last()
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBUSY))?;
    let entry = fs.entry(parts)?;
    if ninep::is_dir(&entry.stat) {
        for child in list_names(fs, parts)? {
            let mut p = parts.to_vec();
            p.push(child);
            remove(fs, &p)?;
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
    let stream = tokio_util_stream(file);
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// A file as a stream of chunks, so a large one is not held in memory.
fn tokio_util_stream(
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
    if at.force {
        if let Err(e) = make_parents(&fs, &parts) {
            return io_error(&e);
        }
    }
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
    let fd = match fs.open_at(&parts, flags, at.mode.unwrap_or(0o644)) {
        Ok(fd) => fd,
        Err(e) => return io_error(&e),
    };
    let mut file = tokio::fs::File::from_std(std::fs::File::from(fd));
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
        if let Err(e) = file.write_all(&chunk).await {
            return io_error(&e);
        }
    }
    if let Err(e) = file.flush().await {
        return io_error(&e);
    }
    if let Err(e) = apply(&fs, &parts, at.uid, at.gid, at.mode) {
        return io_error(&e);
    }
    if let Err(e) = file.sync_all().await {
        return io_error(&e);
    }
    // Sync each containing directory, including parents made by force=true.
    // O_PATH descriptors cannot be fsynced; open real directory descriptors
    // through the same beneath-root, no-symlink resolver.
    for depth in (0..parts.len()).rev() {
        let fd = match fs.open_at(&parts[..depth], libc::O_RDONLY | libc::O_DIRECTORY, 0) {
            Ok(fd) => fd,
            Err(e) => return io_error(&e),
        };
        if let Err(e) = tokio::fs::File::from_std(std::fs::File::from(fd)).sync_all().await {
            return io_error(&e);
        }
    }
    match stat_json(&fs, &parts) {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => io_error(&e),
    }
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
    if at.force {
        if let Err(e) = make_parents(&fs, &parts) {
            return io_error(&e);
        }
    }
    match mkdir(&fs, &parts, at.mode.unwrap_or(0o755)) {
        Ok(()) => {}
        Err(e) if at.force && e.raw_os_error() == Some(libc::EEXIST) => {}
        Err(e) => return io_error(&e),
    }
    if let Err(e) = apply(&fs, &parts, at.uid, at.gid, at.mode) {
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
    match remove(&fs, &parts) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => io_error(&e),
    }
}
